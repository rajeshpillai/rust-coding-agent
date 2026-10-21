//! HTTP/1.1 by hand: one request per connection, over plain TCP or TLS.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

/// A byte stream that is either plain TCP or TCP wrapped in TLS.
pub enum Conn {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Conn::Plain(s) => s.read(buf),
            Conn::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Conn::Plain(s) => s.write(buf),
            Conn::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Conn::Plain(s) => s.flush(),
            Conn::Tls(s) => s.flush(),
        }
    }
}

/// Where a request goes: `https://api.openai.com` or `http://localhost:11434`.
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub tls: bool,
    pub host: String,
    pub port: u16,
}

impl Endpoint {
    pub fn parse(base: &str) -> io::Result<Endpoint> {
        let (tls, rest) = if let Some(r) = base.strip_prefix("https://") {
            (true, r)
        } else if let Some(r) = base.strip_prefix("http://") {
            (false, r)
        } else {
            return Err(io::Error::other(format!("not an http(s) URL: {base}")));
        };
        let authority = rest.trim_end_matches('/');
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => (h, p.parse().map_err(io::Error::other)?),
            None => (authority, if tls { 443 } else { 80 }),
        };
        Ok(Endpoint {
            tls,
            host: host.to_string(),
            port,
        })
    }

    fn connect(&self) -> io::Result<Conn> {
        let sock = TcpStream::connect((self.host.as_str(), self.port))?;
        if !self.tls {
            return Ok(Conn::Plain(sock));
        }
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let config =
            rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(io::Error::other)?
                .with_root_certificates(roots)
                .with_no_client_auth();
        let name = self.host.clone().try_into().map_err(io::Error::other)?;
        let conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(io::Error::other)?;
        Ok(Conn::Tls(Box::new(rustls::StreamOwned::new(conn, sock))))
    }
}

/// The status line and the body of a reply. The body is read as it arrives.
pub struct Response {
    pub status: u16,
    pub body: Box<dyn BufRead>,
}

/// POST a JSON body. `headers` are extra lines such as `("Authorization", "Bearer ...")`.
pub fn post(ep: &Endpoint, path: &str, headers: &[(String, String)], body: &[u8]) -> io::Result<Response> {
    let mut conn = ep.connect()?;
    let mut head = format!(
        "POST {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        ep.host,
        body.len()
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    conn.write_all(head.as_bytes())?;
    conn.write_all(body)?;
    conn.flush()?;

    let mut r = BufReader::new(conn);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let status = line
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| io::Error::other(format!("bad status line: {line:?}")))?;

    let mut length = None;
    let mut chunked = false;
    loop {
        line.clear();
        r.read_line(&mut line)?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            length = Some(v.trim().parse::<u64>().map_err(io::Error::other)?);
        }
        if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
            chunked = true;
        }
    }
    let body: Box<dyn BufRead> = if chunked {
        Box::new(BufReader::new(Chunked {
            inner: r,
            left: 0,
            done: false,
        }))
    } else if let Some(n) = length {
        Box::new(r.take(n))
    } else {
        // No length and not chunked: the body ends when the server closes the connection.
        Box::new(r)
    };
    Ok(Response { status, body })
}

/// Decodes `Transfer-Encoding: chunked`: a hex size line, that many bytes, CRLF, repeated; size 0 ends.
struct Chunked<R> {
    inner: R,
    left: usize,
    done: bool,
}

impl<R: BufRead> Read for Chunked<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done {
            return Ok(0);
        }
        if self.left == 0 {
            let mut size = String::new();
            self.inner.read_line(&mut size)?;
            let hex = size.trim().split(';').next().unwrap_or("");
            self.left = usize::from_str_radix(hex, 16)
                .map_err(|_| io::Error::other(format!("bad chunk size: {size:?}")))?;
            if self.left == 0 {
                self.done = true;
                return Ok(0);
            }
        }
        let n = self.inner.by_ref().take(self.left as u64).read(buf)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.left -= n;
        if self.left == 0 {
            let mut crlf = [0; 2];
            self.inner.read_exact(&mut crlf)?;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_chunks() {
        let raw = b"5\r\nhello\r\n7\r\n, world\r\n0\r\n\r\n";
        let mut out = String::new();
        Chunked {
            inner: &raw[..],
            left: 0,
            done: false,
        }
        .read_to_string(&mut out)
        .unwrap();
        assert_eq!(out, "hello, world");
    }

    #[test]
    fn parses_endpoints() {
        let e = Endpoint::parse("https://api.openai.com").unwrap();
        assert!(e.tls && e.host == "api.openai.com" && e.port == 443);
        let e = Endpoint::parse("http://localhost:11434/").unwrap();
        assert!(!e.tls && e.host == "localhost" && e.port == 11434);
    }
}
