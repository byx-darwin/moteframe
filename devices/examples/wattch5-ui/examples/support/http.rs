//! Bounded loopback preview requests; no filesystem or hardware API.
use std::io::{self, Read};
pub struct Request {
    pub line: String,
    pub body: String,
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid preview request")
}
pub fn read(reader: &mut impl Read, expected_origin: &str) -> io::Result<Request> {
    let mut data = Vec::new();
    let header_end = loop {
        if let Some(index) = data.windows(4).position(|s| s == b"\r\n\r\n") {
            break index + 4;
        }
        if data.len() >= 4096 {
            return Err(invalid());
        }
        let mut chunk = [0; 256];
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            return Err(invalid());
        }
        data.extend_from_slice(&chunk[..n]);
    };
    let headers = std::str::from_utf8(&data[..header_end]).map_err(|_| invalid())?;
    let line = headers.lines().next().ok_or_else(invalid)?.to_owned();
    let mut length = None;
    for header in headers.lines().skip(1) {
        if let Some((key, value)) = header.split_once(':') {
            if key.eq_ignore_ascii_case("Content-Length") {
                if length.is_some() {
                    return Err(invalid());
                }
                length = Some(value.trim().parse::<usize>().map_err(|_| invalid())?);
            }
            if key.eq_ignore_ascii_case("Transfer-Encoding") {
                return Err(invalid());
            }
            if key.eq_ignore_ascii_case("Origin") && value.trim() != expected_origin {
                return Err(invalid());
            }
        }
    }
    let length = length.unwrap_or(0);
    if length > 128 {
        return Err(invalid());
    }
    while data.len() < header_end + length {
        let mut chunk = [0; 128];
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            return Err(invalid());
        }
        data.extend_from_slice(&chunk[..n]);
    }
    let body = std::str::from_utf8(&data[header_end..header_end + length])
        .map_err(|_| invalid())?
        .to_owned();
    Ok(Request { line, body })
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Segments<'a>(&'a [u8]);
    impl Read for Segments<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let n = out.len().min(3).min(self.0.len());
            out[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        }
    }
    #[test]
    fn test_should_accept_only_the_callers_configured_loopback_origin() {
        let bytes = b"POST /event HTTP/1.1\r\nOrigin: http://127.0.0.1:25339\r\n\r\n";
        assert!(read(&mut Segments(bytes), "http://127.0.0.1:25339").is_ok());
        assert!(read(&mut Segments(bytes), "http://127.0.0.1:25331").is_err());
    }
    #[test]
    fn test_should_read_event_body_split_across_socket_reads() {
        let mut input = Segments(b"POST /event HTTP/1.1\r\nContent-Length: 9\r\n\r\ntap 10 20");
        let req = read(&mut input, "http://127.0.0.1:25331").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(req.body, "tap 10 20");
    }
    #[test]
    fn test_should_reject_truncated_or_excessive_or_foreign_requests() {
        for bytes in [
            b"POST /event HTTP/1.1\r\nContent-Length: 3\r\n\r\nu".as_slice(),
            b"POST /event HTTP/1.1\r\nContent-Length: 129\r\n\r\n",
            b"POST /event HTTP/1.1\r\nOrigin: https://example.com\r\n\r\n",
        ] {
            assert!(read(&mut Segments(bytes), "http://127.0.0.1:25331").is_err());
        }
    }
}
