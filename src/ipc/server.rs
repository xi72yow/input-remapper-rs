use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};

use super::protocol::{RecordEvent, Request, Response, SOCKET_PATH};
use crate::daemon::manager::DaemonManager;
use crate::device::{discover, reader::DeviceReader};

pub struct IpcServer {
    listener: UnixListener,
    manager: Arc<Mutex<DaemonManager>>,
}

impl IpcServer {
    pub fn new(manager: Arc<Mutex<DaemonManager>>) -> std::io::Result<Self> {
        // Remove stale socket file
        let _ = std::fs::remove_file(SOCKET_PATH);

        let listener = UnixListener::bind(SOCKET_PATH)?;

        // Make socket accessible
        std::fs::set_permissions(
            SOCKET_PATH,
            std::os::unix::fs::PermissionsExt::from_mode(0o660),
        )?;

        Ok(Self { listener, manager })
    }

    pub fn run(&self) -> std::io::Result<()> {
        eprintln!("Daemon listening on {}", SOCKET_PATH);

        for stream in self.listener.incoming() {
            match stream {
                Ok(stream) => {
                    let manager = Arc::clone(&self.manager);
                    std::thread::spawn(move || {
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            handle_connection(stream, &manager)
                        })) {
                            Ok(Err(e)) => eprintln!("Connection error: {}", e),
                            Err(panic) => {
                                let msg = if let Some(s) = panic.downcast_ref::<&str>() {
                                    s.to_string()
                                } else if let Some(s) = panic.downcast_ref::<String>() {
                                    s.clone()
                                } else {
                                    "unknown panic".into()
                                };
                                eprintln!("Connection PANIC: {}", msg);
                            }
                            Ok(Ok(())) => {}
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Accept error: {}", e);
                }
            }
        }

        Ok(())
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(SOCKET_PATH);
    }
}

fn handle_connection(
    mut stream: UnixStream,
    manager: &Arc<Mutex<DaemonManager>>,
) -> std::io::Result<()> {
    let reader = BufReader::new(stream.try_clone()?);

    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }

        let request: Request = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                let resp = Response::Error {
                    message: format!("Invalid request: {}", e),
                };
                send_response(&mut stream, &resp)?;
                continue;
            }
        };

        // Record is special: it streams events until the client disconnects
        if let Request::Record { ref device } = request {
            return handle_record(&mut stream, device);
        }

        // Device discovery doesn't need the manager lock — handle outside
        // to avoid blocking the daemon if evdev enumeration is slow
        let response = match request {
            Request::ListDevices => {
                let devices = discover::discover_devices()
                    .into_iter()
                    .map(|d| super::protocol::DeviceInfoResponse {
                        name: d.name,
                        key: d.key,
                        vendor: d.vendor,
                        product: d.product,
                    })
                    .collect();
                Response::Devices { devices }
            }
            Request::GetDeviceKeys { ref device } => {
                match discover::find_device_by_name(device) {
                    Some(dev_info) => {
                        let keys = dev_info
                            .supported_keys
                            .into_iter()
                            .map(|k| super::protocol::KeyInfoResponse {
                                code: k.code,
                                name: k.name,
                            })
                            .collect();
                        let is_pointing = dev_info.is_pointing;
                        Response::DeviceKeys { keys, is_pointing }
                    }
                    None => Response::Error {
                        message: format!("Device '{}' not found", device),
                    },
                }
            }
            other => {
                let mut mgr = manager.lock().unwrap();
                mgr.handle_request(other)
            }
        };

        send_response(&mut stream, &response)?;
    }

    Ok(())
}

fn handle_record(stream: &mut UnixStream, device_name: &str) -> std::io::Result<()> {
    let dev_info = match discover::find_device_by_name(device_name) {
        Some(info) => info,
        None => {
            return send_response(
                stream,
                &Response::Error {
                    message: format!("Device '{}' not found", device_name),
                },
            );
        }
    };

    // Open all sub-device paths for recording (no grab — don't steal events)
    let mut readers: Vec<DeviceReader> = Vec::new();
    for path in &dev_info.paths {
        match DeviceReader::open(path) {
            Ok(r) => readers.push(r),
            Err(e) => eprintln!("Warning: cannot open {:?}: {}", path, e),
        }
    }

    if readers.is_empty() {
        return send_response(
            stream,
            &Response::Error {
                message: format!("Cannot open any device paths for '{}'", dev_info.name),
            },
        );
    }

    eprintln!(
        "Recording events from '{}' ({} sub-devices)",
        dev_info.name,
        readers.len()
    );

    // Poll all sub-devices in a round-robin fashion
    loop {
        for reader in &mut readers {
            match reader.read_events(100) {
                Ok(Some(events)) => {
                    for event in &events {
                        let record = Response::RecordEvent(RecordEvent {
                            event_type: event.event_type().0,
                            code: event.code(),
                            code_name: keycode_name(event.event_type().0, event.code()),
                            value: event.value(),
                        });
                        // If write fails (client disconnected), stop recording
                        if send_response(stream, &record).is_err() {
                            eprintln!("Recording client disconnected");
                            return Ok(());
                        }
                    }
                }
                Ok(None) => {} // timeout, no events on this sub-device
                Err(e) => {
                    eprintln!("Record read error: {}", e);
                }
            }
        }
    }
}

/// Get a human-readable name for a keycode
fn keycode_name(event_type: u16, code: u16) -> String {
    use evdev::EventType;

    if event_type == EventType::KEY.0 {
        let key = evdev::KeyCode(code);
        return format!("{:?}", key);
    }

    if event_type == EventType::RELATIVE.0 {
        let axis = evdev::RelativeAxisCode(code);
        return format!("{:?}", axis);
    }

    if event_type == EventType::ABSOLUTE.0 {
        let axis = evdev::AbsoluteAxisCode(code);
        return format!("{:?}", axis);
    }

    format!("{}:{}", event_type, code)
}

fn send_response(stream: &mut UnixStream, response: &Response) -> std::io::Result<()> {
    let mut json = serde_json::to_string(response)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    json.push('\n');
    stream.write_all(json.as_bytes())?;
    stream.flush()
}
