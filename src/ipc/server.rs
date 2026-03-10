use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};

use super::protocol::{Request, Response, SOCKET_PATH};
use crate::daemon::manager::DaemonManager;

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

        let response = {
            let mut mgr = manager.lock().unwrap();
            mgr.handle_request(request)
        };

        send_response(&mut stream, &response)?;
    }

    Ok(())
}

fn send_response(stream: &mut UnixStream, response: &Response) -> std::io::Result<()> {
    let mut json = serde_json::to_string(response)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    json.push('\n');
    stream.write_all(json.as_bytes())?;
    stream.flush()
}
