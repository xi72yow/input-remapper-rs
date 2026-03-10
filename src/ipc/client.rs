use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use super::protocol::{Request, Response, SOCKET_PATH};

pub fn send_request(request: &Request) -> std::io::Result<Response> {
    let mut stream = UnixStream::connect(SOCKET_PATH).map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!("Could not connect to daemon at {}: {}. Is the daemon running?", SOCKET_PATH, e),
        )
    })?;

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
