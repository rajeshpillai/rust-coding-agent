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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_endpoints() {
        let e = Endpoint::parse("https://api.openai.com").unwrap();
        assert!(e.tls && e.host == "api.openai.com" && e.port == 443);
        let e = Endpoint::parse("http://localhost:11434/").unwrap();
        assert!(!e.tls && e.host == "localhost" && e.port == 11434);
    }
}
