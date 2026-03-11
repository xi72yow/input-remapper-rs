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
                        if let Err(e) = handle_connection(stream, &manager) {
                            eprintln!("Connection error: {}", e);
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

        let response = {
            let mut mgr = manager.lock().unwrap();
            mgr.handle_request(request)
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

    // Open the first device path for recording (no grab — don't steal events)
    let mut reader = DeviceReader::open(&dev_info.paths[0])?;

    eprintln!("Recording events from '{}'", dev_info.name);

    loop {
        match reader.read_events(1000) {
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
            Ok(None) => {} // timeout, no events
            Err(e) => {
                eprintln!("Record read error: {}", e);
                return Err(e);
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
