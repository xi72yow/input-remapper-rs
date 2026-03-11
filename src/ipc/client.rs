use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use super::protocol::{Request, Response, SOCKET_PATH};

const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);

pub fn send_request(request: &Request) -> std::io::Result<Response> {
    let mut stream = UnixStream::connect(SOCKET_PATH).map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!(
                "Could not connect to daemon at {}: {}. Is the daemon running?",
                SOCKET_PATH, e
            ),
        )
    })?;

    stream.set_read_timeout(Some(SOCKET_TIMEOUT))?;
    stream.set_write_timeout(Some(SOCKET_TIMEOUT))?;

    let mut json = serde_json::to_string(request)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    json.push('\n');
    stream.write_all(json.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;

    let response: Response = serde_json::from_str(&line)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(response)
}

/// Send a record request and stream events via callback.
/// Calls `on_event` for each event until the callback returns false or the connection drops.
pub fn record_events<F>(device: &str, mut on_event: F) -> std::io::Result<()>
where
    F: FnMut(&Response) -> bool,
{
    let mut stream = UnixStream::connect(SOCKET_PATH).map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!(
                "Could not connect to daemon at {}: {}. Is the daemon running?",
                SOCKET_PATH, e
            ),
        )
    })?;

    let request = Request::Record {
        device: device.to_string(),
    };
    let mut json = serde_json::to_string(&request)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    json.push('\n');
    stream.write_all(json.as_bytes())?;
    stream.flush()?;

    let reader = BufReader::new(&stream);
    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }

        let response: Response = serde_json::from_str(&line)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // If it's an error, pass it through and stop
        if matches!(response, Response::Error { .. }) {
            on_event(&response);
            break;
        }

        if !on_event(&response) {
            break;
        }
    }

    Ok(())
}
