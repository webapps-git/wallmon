//! Minimal HTTP/1.0 GET over TCP or a unix socket, enough for local status endpoints
//! (Apache `server-status`, the Docker API). No TLS: these never leave the machine.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// GET `http://host[:port]/path`. Returns the status code and body.
pub fn get(url: &str, timeout: Duration) -> Result<(u16, String), String> {
    let rest = url.strip_prefix("http://").ok_or(format!("{url}: only http:// addresses are supported"))?;
    let (authority, path) = rest.split_once('/').map_or((rest, "/".to_string()), |(a, p)| (a, format!("/{p}")));
    let target = if authority.contains(':') { authority.to_string() } else { format!("{authority}:80") };
    let addr = target
        .to_socket_addrs()
        .map_err(|e| format!("{authority}: {e}"))?
        .next()
        .ok_or(format!("{authority}: no address"))?;
    let stream = TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("{authority}: {e}"))?;
    stream.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    request(stream, authority, &path)
}

/// GET `path` from an HTTP server listening on a unix socket.
pub fn get_unix(socket: &str, path: &str, timeout: Duration) -> Result<(u16, String), String> {
    let stream = UnixStream::connect(socket).map_err(|e| format!("{socket}: {e}"))?;
    stream.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    request(stream, "localhost", path)
}

fn request<S: Read + Write>(mut s: S, host: &str, path: &str) -> Result<(u16, String), String> {
    write!(s, "GET {path} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: wallmon\r\nConnection: close\r\n\r\n")
        .map_err(|e| e.to_string())?;
    let mut buf = vec![];
    s.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    parse_response(&buf)
}

pub fn parse_response(raw: &[u8]) -> Result<(u16, String), String> {
    let end = find(raw, b"\r\n\r\n").ok_or("incomplete HTTP response")?;
    let head = String::from_utf8_lossy(&raw[..end]);
    let body = &raw[end + 4..];
    let code = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or("unrecognised HTTP response")?;
    let chunked = head.lines().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    let body = if chunked { dechunk(body) } else { body.to_vec() };
    Ok((code, String::from_utf8_lossy(&body).into_owned()))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn dechunk(mut s: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    while let Some(eol) = find(s, b"\r\n") {
        let size = String::from_utf8_lossy(&s[..eol]);
        let Ok(n) = usize::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16) else { break };
        let rest = &s[eol + 2..];
        if n == 0 || rest.len() < n {
            break;
        }
        out.extend_from_slice(&rest[..n]);
        s = rest[n..].strip_prefix(b"\r\n").unwrap_or(&rest[n..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_chunked_responses() {
        let (code, body) = parse_response(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nBusyWorkers: 4\n").unwrap();
        assert_eq!((code, body.as_str()), (200, "BusyWorkers: 4\n"));
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n[{}]\r\n3\r\n,{}\r\n0\r\n\r\n";
        assert_eq!(parse_response(raw).unwrap().1, "[{}],{}");
    }
}
