//! The HTTPS proxy a capture runs for programs that ignore `SSLKEYLOGFILE`. A program that goes
//! through it asks for a tunnel with `CONNECT`. When TLS starts in the tunnel, the proxy shows a
//! certificate its authority signs for the host, opens its own TLS connection to the host,
//! checks that one as the system does, and writes the keys of both to the capture's key log, so
//! the capture's copy of the connection to the host can be read. Anything else in a tunnel
//! passes through as it is. It listens on the loopback address only, while the capture runs.

mod authority;

use authority::CertificateAuthority;
use base64::{Engine, engine::general_purpose::STANDARD};
use borg_core::{Authority, ProxyEndpoint};
use rustls_platform_verifier::Verifier;
use std::{
    fmt,
    fs::{File, OpenOptions},
    io::{self, Write},
    net::{Ipv4Addr, SocketAddr},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional},
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
    time::timeout,
};
use tokio_rustls::{
    LazyConfigAcceptor, TlsConnector,
    rustls::{
        ClientConfig, KeyLog,
        crypto::ring,
        pki_types::{CertificateDer, ServerName},
        server::Acceptor,
    },
};

/// The longest request for a tunnel the proxy reads, and what ends one.
const HEAD_LIMIT: usize = 8 * 1024;
const HEAD_END: &[u8] = b"\r\n\r\n";
const HEADERS: usize = 64;
const CONNECT: &str = "CONNECT";
const ESTABLISHED: &[u8] = b"HTTP/1.1 200 Connection Established\r\n\r\n";
const BAD_REQUEST: &[u8] = b"HTTP/1.1 400 Bad Request\r\n\r\n";
const NOT_IMPLEMENTED: &[u8] = b"HTTP/1.1 501 Not Implemented\r\n\r\n";
const BAD_GATEWAY: &[u8] = b"HTTP/1.1 502 Bad Gateway\r\n\r\n";
/// The first byte of a TLS handshake record.
const HANDSHAKE: u8 = 0x16;
/// How long a program has to ask for its tunnel, and a host to answer.
const WAIT: Duration = Duration::from_secs(10);
const FILE_MODE: u32 = 0o600;
const PEM_BEGIN: &str = "-----BEGIN CERTIFICATE-----\n";
const PEM_END: &str = "-----END CERTIFICATE-----\n";
const PEM_LINE: usize = 64;

/// What the proxy tells the capture as it works.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    /// It opened `local` to `remote` for the program on the other end of `client`.
    Opened { client: SocketAddr, local: SocketAddr, remote: SocketAddr },
    /// A program did not take the certificate the proxy showed it for `host`.
    Refused { host: String },
    /// `host` did not prove who it is, so the proxy closed the connection.
    Unverified { host: String, error: String },
}

pub(crate) type Report = Box<dyn Fn(Event) + Send + Sync>;

/// The proxy while it runs; it stops and removes its trust bundle when dropped.
pub(crate) struct Proxy {
    endpoint: ProxyEndpoint,
    task: JoinHandle<()>,
}

impl Proxy {
    /// Starts the proxy with the certificates `authority` signs, writing TLS keys to `key_log`
    /// and the certificates programs should trust to `trust`.
    pub(crate) async fn start(
        authority: &Authority,
        key_log: &Path,
        trust: PathBuf,
        report: Report,
    ) -> Result<Self, String> {
        let chosen = authority.clone();
        let written = trust.clone();
        let authority = tokio::task::spawn_blocking(move || {
            let authority = match &chosen {
                Authority::Temporary => CertificateAuthority::temporary().map_err(|error| error.to_string())?,
                Authority::Folder(folder) => CertificateAuthority::from_folder(folder)?,
            };
            write_private(&written, trust_bundle(authority.pem()).as_bytes()).map_err(|error| error.to_string())?;
            Ok::<_, String>(authority)
        })
        .await
        .map_err(|error| error.to_string())??;
        let started = Self::listen(authority, key_log, report).await;
        if started.is_err() {
            let _ = std::fs::remove_file(&trust);
        }
        let (address, task) = started.map_err(|error| error.to_string())?;
        Ok(Self { endpoint: ProxyEndpoint { address, trust }, task })
    }

    async fn listen(
        authority: CertificateAuthority,
        key_log: &Path,
        report: Report,
    ) -> io::Result<(SocketAddr, JoinHandle<()>)> {
        let keys: Arc<dyn KeyLog> = Arc::new(KeyLogFile(Mutex::new(OpenOptions::new().append(true).open(key_log)?)));
        let upstream = upstream_config(authority.own_roots(), keys.clone()).map_err(io::Error::other)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let shared = Arc::new(Shared { authority, upstream: Arc::new(upstream), keys, report });
        Ok((address, tokio::spawn(serve(listener, shared))))
    }

    pub(crate) fn endpoint(&self) -> ProxyEndpoint {
        self.endpoint.clone()
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_file(&self.endpoint.trust);
    }
}

/// The certificates a program that goes through the proxy trusts: the proxy's authority, then
/// the system's, for the hosts it reaches some other way.
fn trust_bundle(authority: &str) -> String {
    let system = rustls_native_certs::load_native_certs().certs;
    std::iter::once(authority.to_owned()).chain(system.iter().map(|certificate| pem(certificate))).collect()
}

fn pem(der: &[u8]) -> String {
    let encoded = STANDARD.encode(der);
    let lines: Vec<&str> =
        encoded.as_bytes().chunks(PEM_LINE).filter_map(|line| std::str::from_utf8(line).ok()).collect();
    format!("{PEM_BEGIN}{}\n{PEM_END}", lines.join("\n"))
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    OpenOptions::new().write(true).create(true).truncate(true).mode(FILE_MODE).open(path)?.write_all(bytes)
}

/// How the proxy reaches the hosts: checked as the system checks them, trusting your own CA
/// too when it signs, and writing the keys to the capture's key log.
fn upstream_config(
    own: Vec<CertificateDer<'static>>,
    keys: Arc<dyn KeyLog>,
) -> Result<ClientConfig, tokio_rustls::rustls::Error> {
    let provider = Arc::new(ring::default_provider());
    let verifier = if own.is_empty() {
        Verifier::new(provider.clone())?
    } else {
        Verifier::new_with_extra_roots(own, provider.clone())?
    };
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    config.key_log = keys;
    Ok(config)
}

/// Writes TLS keys as browsers do, one line for each secret.
struct KeyLogFile(Mutex<File>);

impl fmt::Debug for KeyLogFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("KeyLogFile")
    }
}

impl KeyLog for KeyLogFile {
    fn log(&self, label: &str, client_random: &[u8], secret: &[u8]) {
        let line = format!("{label} {} {}\n", hex(client_random), hex(secret));
        if let Ok(mut file) = self.0.lock() {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct Shared {
    authority: CertificateAuthority,
    upstream: Arc<ClientConfig>,
    keys: Arc<dyn KeyLog>,
    report: Report,
}

async fn serve(listener: TcpListener, shared: Arc<Shared>) {
    let mut tunnels = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                if let Ok((client, peer)) = accepted {
                    tunnels.spawn(tunnel(client, peer, shared.clone()));
                }
            }
            Some(_) = tunnels.join_next(), if !tunnels.is_empty() => {}
        }
    }
}

/// Where a program wants its tunnel to go.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    host: String,
    port: u16,
}

/// The tunnel a request asks for, or what the proxy answers when it asks for something else.
fn requested(head: &[u8]) -> Result<Target, &'static [u8]> {
    let mut headers = [httparse::EMPTY_HEADER; HEADERS];
    let mut request = httparse::Request::new(&mut headers);
    if !matches!(request.parse(head), Ok(httparse::Status::Complete(_))) {
        return Err(BAD_REQUEST);
    }
    if request.method != Some(CONNECT) {
        return Err(NOT_IMPLEMENTED);
    }
    request.path.and_then(target).ok_or(BAD_REQUEST)
}

/// `host:port`, with an IPv6 address in brackets.
fn target(authority: &str) -> Option<Target> {
    let (host, port) = authority.rsplit_once(':')?;
    let host = host.strip_prefix('[').and_then(|host| host.strip_suffix(']')).unwrap_or(host);
    let port = port.parse().ok()?;
    (!host.is_empty()).then(|| Target { host: host.to_owned(), port })
}

/// Reads the request for a tunnel up to its end, and nothing the program sends after it.
async fn read_head(client: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut head = Vec::new();
    while !head.ends_with(HEAD_END) {
        if head.len() == HEAD_LIMIT {
            return Err(io::ErrorKind::InvalidData.into());
        }
        head.push(client.read_u8().await?);
    }
    Ok(head)
}

async fn tunnel(mut client: TcpStream, peer: SocketAddr, shared: Arc<Shared>) -> io::Result<()> {
    let head = timeout(WAIT, read_head(&mut client)).await??;
    let target = match requested(&head) {
        Ok(target) => target,
        Err(answer) => return client.write_all(answer).await,
    };
    let Ok(Ok(upstream)) = timeout(WAIT, TcpStream::connect((target.host.as_str(), target.port))).await else {
        return client.write_all(BAD_GATEWAY).await;
    };
    client.write_all(ESTABLISHED).await?;
    if let (Ok(local), Ok(remote)) = (upstream.local_addr(), upstream.peer_addr()) {
        (shared.report)(Event::Opened { client: peer, local, remote });
    }
    let mut first = [0; 1];
    if client.peek(&mut first).await? == 0 {
        return Ok(());
    }
    if first[0] != HANDSHAKE {
        let (mut client, mut upstream) = (client, upstream);
        return copy_bidirectional(&mut client, &mut upstream).await.map(drop);
    }
    intercept(client, upstream, &target.host, &shared).await
}

/// Answers the program's TLS with the authority's certificate for the host it names, after
/// the proxy's own connection to that host is up and has chosen the protocol both speak.
async fn intercept(client: TcpStream, upstream: TcpStream, host: &str, shared: &Shared) -> io::Result<()> {
    let start = LazyConfigAcceptor::new(Acceptor::default(), client).await?;
    let hello = start.client_hello();
    let name = hello.server_name().unwrap_or(host).to_owned();
    let offered: Vec<Vec<u8>> =
        hello.alpn().map(|protocols| protocols.map(<[u8]>::to_vec).collect()).unwrap_or_default();
    let server = ServerName::try_from(name.clone()).map_err(io::Error::other)?;
    let mut config = (*shared.upstream).clone();
    config.alpn_protocols = offered;
    let mut upstream = match TlsConnector::from(Arc::new(config)).connect(server, upstream).await {
        Ok(upstream) => upstream,
        Err(error) => {
            (shared.report)(Event::Unverified { host: name, error: error.to_string() });
            return Ok(());
        }
    };
    let protocol = upstream.get_ref().1.alpn_protocol().map(<[u8]>::to_vec);
    let answer = shared.authority.server_config(&name, protocol, shared.keys.clone())?;
    let mut client = match start.into_stream(answer).await {
        Ok(client) => client,
        Err(error) => {
            if refused(&error) {
                (shared.report)(Event::Refused { host: name });
            }
            return Ok(());
        }
    };
    copy_bidirectional(&mut client, &mut upstream).await.map(drop)
}

/// Whether a program ended the handshake with an alert, as one does that does not trust the
/// certificate it got.
fn refused(error: &io::Error) -> bool {
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<tokio_rustls::rustls::Error>())
        .is_some_and(|error| matches!(error, tokio_rustls::rustls::Error::AlertReceived(_)))
}

#[cfg(test)]
mod tests;
