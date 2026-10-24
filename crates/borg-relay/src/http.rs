//! The relay's HTTP front: it reads a request's head to tell a WebSocket upgrade from a plain
//! request, answers health checks, and learns the client's address from a proxy in front.

use std::{
    io,
    net::IpAddr,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

/// The most a request's head may take.
const HEAD_LIMIT: usize = 8 * 1024;
pub const HEALTH: &str = "/health";

/// What the relay needs from a request's head.
#[derive(Debug, PartialEq, Eq)]
pub struct Head {
    pub path: String,
    pub upgrade: bool,
    /// The client's address as a proxy in front reports it.
    pub forwarded: Option<IpAddr>,
}

/// Reads a request's head, up to the blank line that ends it: the head, and every byte read,
/// which the WebSocket handshake reads again.
pub async fn read_head<S: AsyncRead + Unpin>(stream: &mut S) -> io::Result<(Head, Vec<u8>)> {
    let mut bytes = Vec::with_capacity(1024);
    let mut chunk = [0; 1024];
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            return Ok((parse(&bytes[..end]), bytes));
        }
        if bytes.len() > HEAD_LIMIT {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "request head too large"));
        }
    }
}

fn parse(head: &[u8]) -> Head {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n");
    let path = lines.next().and_then(|line| line.split(' ').nth(1)).unwrap_or("/").to_owned();
    let mut upgrade = false;
    let (mut forwarded_for, mut real_ip) = (None, None);
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "upgrade" => upgrade |= value.eq_ignore_ascii_case("websocket"),
            // The first address is the client's; the proxies it passed add theirs after it.
            "x-forwarded-for" => forwarded_for = value.split(',').next().and_then(|first| first.trim().parse().ok()),
            "x-real-ip" => real_ip = value.parse().ok(),
            _ => {}
        }
    }
    Head { path, upgrade, forwarded: forwarded_for.or(real_ip) }
}

/// Answers a plain request: health checks and a line saying what this is.
pub async fn answer<S: AsyncWrite + Unpin>(stream: &mut S, head: &Head) -> io::Result<()> {
    let (status, body) = match head.path.as_str() {
        HEALTH => ("200 OK", "ok\n".to_owned()),
        "/" => ("200 OK", format!("borg-relay {}\n", env!("CARGO_PKG_VERSION"))),
        _ => ("404 Not Found", "not found\n".to_owned()),
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

/// A stream that gives back bytes already read from it before reading on.
pub struct Rewind<S> {
    prefix: Vec<u8>,
    taken: usize,
    inner: S,
}

impl<S> Rewind<S> {
    pub fn new(prefix: Vec<u8>, inner: S) -> Self {
        Self { prefix, taken: 0, inner }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Rewind<S> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.taken < this.prefix.len() {
            let rest = &this.prefix[this.taken..];
            let count = rest.len().min(buf.remaining());
            buf.put_slice(&rest[..count]);
            this.taken += count;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Rewind<S> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, data)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

/// `borg-relay --health`: whether the relay at `address` answers its health check.
pub fn check(address: &str) -> bool {
    use std::io::{Read, Write};
    let Ok(mut stream) = std::net::TcpStream::connect(address) else { return false };
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let request = format!("GET {HEALTH} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let mut response = String::new();
    stream.write_all(request.as_bytes()).is_ok()
        && stream.read_to_string(&mut response).is_ok()
        && response.starts_with("HTTP/1.1 200")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_head_tells_upgrades_paths_and_the_forwarded_client() {
        let head = parse(b"GET /v1/connect/ab HTTP/1.1\r\nHost: relay\r\nUpgrade: websocket\r\nX-Forwarded-For: 203.0.113.7, 10.0.0.2");
        assert_eq!(
            head,
            Head { path: "/v1/connect/ab".into(), upgrade: true, forwarded: Some("203.0.113.7".parse().unwrap()) }
        );
        let plain = parse(b"GET /health HTTP/1.1\r\nX-Real-IP: 2001:db8::1");
        assert_eq!(
            plain,
            Head { path: "/health".into(), upgrade: false, forwarded: Some("2001:db8::1".parse().unwrap()) }
        );
    }
}
