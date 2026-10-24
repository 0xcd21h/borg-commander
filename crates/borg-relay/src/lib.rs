//! borg-relay: where borgs on servers and borgs on users' computers meet. Both connect out to
//! it, so neither needs an open port. It joins a computer's socket to the server the computer
//! asks for and forwards what they send, which is encrypted end to end between them: the
//! relay can neither read it nor answer in either one's name. Limits keep it from serving as
//! a free tunnel.

/// Writes a line to the log, the standard error: a closed log must not stop the relay.
macro_rules! log {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

/// Writes a line to the event log, which is off unless the operator turns it on: it records
/// when servers and clients came, and from where.
macro_rules! event {
    ($($arg:tt)*) => {{
        if crate::EVENT_LOG.load(std::sync::atomic::Ordering::Relaxed) {
            log!($($arg)*);
        }
    }};
}

mod http;
mod hub;

pub use http::check as health;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::StreamExt;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
};

/// How long a connection has to finish TLS and the WebSocket upgrade.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// Connections the relay holds at once, from every address together, before it reads who
/// they are.
const MAX_CONNECTIONS: usize = 20_000;
/// How often the relay logs how busy it is, when that changed.
const STATS_EVERY: Duration = Duration::from_secs(60);

static EVENT_LOG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// How the relay meets the outside.
#[derive(Clone, Default)]
pub struct Options {
    /// Serve TLS itself, with a certificate from Let's Encrypt.
    pub tls: Option<Tls>,
    /// A proxy in front adds TLS and reports each client's address, which the relay then
    /// counts limits by. Only trusted when set, since a client could claim any address.
    pub behind_proxy: bool,
    /// Log each server's registration, each session and each limit reached, for debugging.
    pub log: bool,
}

/// Serves borgs on `listener` until the process ends.
pub async fn serve(listener: TcpListener, options: Options) {
    EVENT_LOG.store(options.log, std::sync::atomic::Ordering::Relaxed);
    let hub = Arc::new(Mutex::new(hub::Hub::default()));
    let open = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    tokio::spawn(report(hub.clone(), open.clone()));
    loop {
        let (tcp, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                log!("accept failed: {error}");
                continue;
            }
        };
        if open.load(std::sync::atomic::Ordering::SeqCst) >= MAX_CONNECTIONS {
            drop(tcp);
            continue;
        }
        let (hub, options, open) = (hub.clone(), options.clone(), open.clone());
        open.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        tokio::spawn(async move {
            connection(hub, options, tcp, peer.ip()).await;
            open.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        });
    }
}

/// Logs how busy the relay is, counts only, once a minute when they changed.
async fn report(hub: Arc<Mutex<hub::Hub>>, open: Arc<std::sync::atomic::AtomicUsize>) {
    let mut every = tokio::time::interval_at(tokio::time::Instant::now() + STATS_EVERY, STATS_EVERY);
    let mut last = hub.lock().expect("hub lock").stats(0);
    loop {
        every.tick().await;
        let line = hub.lock().expect("hub lock").stats(open.load(std::sync::atomic::Ordering::SeqCst));
        if line != last {
            log!("{line}");
            last = line;
        }
    }
}

/// One connection: TLS when the relay serves it, the request's head, then a health check's
/// answer or the WebSocket the head asks for, counted against the client's address.
async fn connection(hub: Arc<Mutex<hub::Hub>>, options: Options, tcp: tokio::net::TcpStream, peer: std::net::IpAddr) {
    let mut stream: Box<dyn Io> = match options.tls {
        None => Box::new(tcp),
        Some(tls) => match tokio::time::timeout(HANDSHAKE_TIMEOUT, tls.accept(tcp)).await {
            Ok(Ok(Some(stream))) => Box::new(stream),
            _ => return,
        },
    };
    let Ok(Ok((head, read))) = tokio::time::timeout(HANDSHAKE_TIMEOUT, http::read_head(&mut stream)).await else {
        return;
    };
    let address = if options.behind_proxy { head.forwarded.unwrap_or(peer) } else { peer };
    if !head.upgrade {
        let _ = http::answer(&mut stream, &head).await;
        return;
    }
    let Some(guard) = hub::Hub::admit(&hub, address) else { return };
    hub::handle(hub, Box::new(http::Rewind::new(read, stream)), address).await;
    drop(guard);
}

/// TLS with a certificate from Let's Encrypt, which the relay renews itself. The challenge is
/// answered on the same port (TLS-ALPN-01), so the relay needs no other.
#[derive(Clone)]
pub struct Tls {
    challenge: Arc<tokio_rustls::rustls::ServerConfig>,
    default: Arc<tokio_rustls::rustls::ServerConfig>,
}

impl Tls {
    /// The TLS stream, or `None` for a connection that only answered a certificate challenge.
    async fn accept(&self, tcp: tokio::net::TcpStream) -> std::io::Result<Option<impl Io + use<>>> {
        use tokio::io::AsyncWriteExt;
        let start = tokio_rustls::LazyConfigAcceptor::new(Default::default(), tcp).await?;
        if rustls_acme::is_tls_alpn_challenge(&start.client_hello()) {
            let mut stream = start.into_stream(self.challenge.clone()).await?;
            stream.shutdown().await?;
            return Ok(None);
        }
        Ok(Some(start.into_stream(self.default.clone()).await?))
    }
}

/// TLS for `domain`, with the certificate kept in `cache`; `staging` uses Let's Encrypt's
/// staging service, for trying the setup.
pub fn acme(domain: &str, email: Option<&str>, cache: PathBuf, staging: bool) -> Tls {
    let mut config = rustls_acme::AcmeConfig::new([domain])
        .cache(rustls_acme::caches::DirCache::new(cache))
        .directory_lets_encrypt(!staging);
    if let Some(email) = email {
        config = config.contact_push(format!("mailto:{email}"));
    }
    let mut state = config.state();
    let tls = Tls { challenge: state.challenge_rustls_config(), default: state.default_rustls_config() };
    tokio::spawn(async move {
        while let Some(event) = state.next().await {
            match event {
                Ok(event) => log!("certificate: {event:?}"),
                Err(error) => log!("certificate error: {error:?}"),
            }
        }
    });
    tls
}
