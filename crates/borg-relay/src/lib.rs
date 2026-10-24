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

mod hub;

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

/// Serves borgs on `listener` until the process ends: with TLS when `tls` is given.
pub async fn serve(listener: TcpListener, tls: Option<Tls>) {
    let hub = Arc::new(Mutex::new(hub::Hub::default()));
    loop {
        let (tcp, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                log!("accept failed: {error}");
                continue;
            }
        };
        let Some(guard) = hub::Hub::admit(&hub, peer.ip()) else {
            drop(tcp);
            continue;
        };
        let (hub, tls) = (hub.clone(), tls.clone());
        tokio::spawn(async move {
            let stream: Box<dyn Io> = match tls {
                None => Box::new(tcp),
                Some(tls) => match tokio::time::timeout(HANDSHAKE_TIMEOUT, tls.accept(tcp)).await {
                    Ok(Ok(Some(stream))) => Box::new(stream),
                    _ => return,
                },
            };
            hub::handle(hub, stream, peer.ip()).await;
            drop(guard);
        });
    }
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
