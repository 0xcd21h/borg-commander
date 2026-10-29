//! The relay's routing: registered servers by key, sessions waiting to be joined, and the
//! forwarding between joined sockets, with the limits that keep the relay for borg's use.

use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use borg_link::{
    KEY_LEN, PublicKey, hex,
    relay::{ACCEPT, CONNECT, LIMITED, LISTEN, OFFLINE, READY, SESSION, challenge, verify},
    unhex,
};
use futures::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        Message,
        handshake::server::{Request, Response},
        protocol::{CloseFrame, WebSocketConfig, frame::coding::CloseCode},
    },
};

use crate::Io;

type Socket = WebSocketStream<Box<dyn Io>>;

/// Connections one address may hold at once.
const PER_ADDRESS: usize = 32;
/// Servers registered at once, and sessions joined at once.
const MAX_LISTENERS: usize = 5_000;
const MAX_SESSIONS: usize = 5_000;
/// Clients one server may have waiting to be joined.
const MAX_WAITING: usize = 8;
/// How long a server has to answer the proof, and to take a session a client opened.
const PROOF_TIMEOUT: Duration = Duration::from_secs(10);
const JOIN_TIMEOUT: Duration = Duration::from_secs(10);
const REGISTRATION_WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// A registered server is pinged this often, and dropped when it stops answering. Both ends of
/// a session are pinged as often, since proxies in front close sockets that stay quiet.
const PING_EVERY: Duration = Duration::from_secs(30);
const SILENCE_LIMIT: Duration = Duration::from_secs(90);
/// The largest frame forwarded: one Noise message.
const FRAME_LIMIT: usize = 65_535;
/// A session's bandwidth, in bytes per second, with a burst on top, and the most it may
/// carry in all.
const RATE: f64 = 2.0 * 1024.0 * 1024.0;
const BURST: f64 = 8.0 * 1024.0 * 1024.0;
const SESSION_BYTES: u64 = 1024 * 1024 * 1024;
/// A session ends after this long without traffic, and after this long in any case.
const IDLE_LIMIT: Duration = Duration::from_secs(15 * 60);
const LIFETIME: Duration = Duration::from_secs(12 * 60 * 60);
const SESSION_ID_LEN: usize = 16;

struct Registered {
    generation: u64,
    notify: mpsc::Sender<String>,
    waiting: usize,
}

struct Waiting {
    server: PublicKey,
    joined: oneshot::Sender<Socket>,
}

#[derive(Default)]
pub struct Hub {
    servers: HashMap<PublicKey, Registered>,
    waiting: HashMap<String, Waiting>,
    addresses: HashMap<IpAddr, usize>,
    sessions: usize,
    generation: u64,
    /// Since the relay started: sessions joined, bytes they carried, and connections or
    /// sessions refused or ended by a limit.
    joined: u64,
    carried: u64,
    limited: u64,
}

/// One connection counted against its address while it lasts.
pub struct Admitted {
    hub: Arc<Mutex<Hub>>,
    address: IpAddr,
}

impl Drop for Admitted {
    fn drop(&mut self) {
        let mut hub = self.hub.lock().expect("hub lock");
        if let Some(count) = hub.addresses.get_mut(&self.address) {
            *count -= 1;
            if *count == 0 {
                hub.addresses.remove(&self.address);
            }
        }
    }
}

impl Hub {
    /// Counts a new connection from `address`, unless it already holds its share.
    pub fn admit(hub: &Arc<Mutex<Self>>, address: IpAddr) -> Option<Admitted> {
        let mut locked = hub.lock().expect("hub lock");
        let count = locked.addresses.entry(address).or_default();
        if *count >= PER_ADDRESS {
            locked.limited += 1;
            drop(locked);
            event!("limit: too many connections from {address}");
            return None;
        }
        *count += 1;
        Some(Admitted { hub: hub.clone(), address })
    }

    /// How busy the relay is, in counts only, given the `connections` it holds.
    pub fn stats(&self, connections: usize) -> String {
        format!(
            "stats: connections={connections} servers={} sessions={} joined={} carried={:.1}MiB limited={}",
            self.servers.len(),
            self.sessions,
            self.joined,
            self.carried as f64 / (1024.0 * 1024.0),
            self.limited
        )
    }
}

fn short(key: &PublicKey) -> String {
    hex(&key[..4])
}

async fn close(mut socket: Socket, code: u16, reason: &'static str) {
    let frame = CloseFrame { code: CloseCode::from(code), reason: reason.into() };
    let _ = socket.close(Some(frame)).await;
}

/// Serves one connection: the WebSocket upgrade, then what its path asks for.
#[expect(clippy::result_large_err, reason = "the upgrade callback's error type is tungstenite's")]
pub async fn handle(hub: Arc<Mutex<Hub>>, stream: Box<dyn Io>, address: IpAddr) {
    let mut path = String::new();
    let config = WebSocketConfig::default().max_message_size(Some(FRAME_LIMIT)).max_frame_size(Some(FRAME_LIMIT));
    let upgrade = tokio_tungstenite::accept_hdr_async_with_config(
        stream,
        |request: &Request, response: Response| {
            path = request.uri().path().to_owned();
            Ok(response)
        },
        Some(config),
    );
    let Ok(Ok(socket)) = tokio::time::timeout(crate::HANDSHAKE_TIMEOUT, upgrade).await else { return };
    if let Some(key) = path.strip_prefix(LISTEN).and_then(unhex::<KEY_LEN>) {
        listen(hub, socket, key).await;
    } else if let Some(key) = path.strip_prefix(CONNECT).and_then(unhex::<KEY_LEN>) {
        connect(hub, socket, key, address).await;
    } else if let Some(id) = path.strip_prefix(ACCEPT) {
        accept(hub, socket, id);
    } else {
        close(socket, u16::from(CloseCode::Policy), "unknown path").await;
    }
}

/// A server registers: it proves it holds `key`, then hears of each client that asks for it
/// while it stays connected. A new registration for the same key replaces the old one.
async fn listen(hub: Arc<Mutex<Hub>>, mut socket: Socket, key: PublicKey) {
    let full = {
        let mut locked = hub.lock().expect("hub lock");
        let full = locked.servers.len() >= MAX_LISTENERS && !locked.servers.contains_key(&key);
        locked.limited += u64::from(full);
        full
    };
    if full {
        log!("limit: too many servers registered");
        return close(socket, LIMITED, "too many servers").await;
    }
    let Ok((handshake, message)) = challenge(&key) else { return };
    if let Err(error) = write_registration(&mut socket, Message::Binary(message.into())).await {
        event!("server {}: challenge failed: {error}", short(&key));
        return;
    }
    let answer = tokio::time::timeout(PROOF_TIMEOUT, socket.next()).await;
    let answer = match answer {
        Ok(Some(Ok(Message::Binary(answer)))) => answer,
        Err(_) => {
            event!("server {}: registration proof timed out", short(&key));
            return;
        }
        _ => {
            event!("server {}: registration closed before proof", short(&key));
            return;
        }
    };
    if !verify(handshake, &answer) {
        event!("server {}: proof refused", short(&key));
        return close(socket, u16::from(CloseCode::Policy), "proof refused").await;
    }
    let (notify, mut announced) = mpsc::channel(MAX_WAITING);
    let generation = {
        let mut locked = hub.lock().expect("hub lock");
        locked.generation += 1;
        let generation = locked.generation;
        locked.servers.insert(key, Registered { generation, notify, waiting: 0 });
        generation
    };
    event!("server {}: registered", short(&key));
    let started = tokio::time::Instant::now();
    let ended = registration(&mut socket, &mut announced).await;
    let mut locked = hub.lock().expect("hub lock");
    if locked.servers.get(&key).is_some_and(|server| server.generation == generation) {
        locked.servers.remove(&key);
    }
    drop(locked);
    event!(
        "server {}: gone after {}s: {} (last received {}s ago; pongs={})",
        short(&key),
        started.elapsed().as_secs(),
        ended.reason,
        ended.last_received.as_secs(),
        ended.pongs
    );
}

struct RegistrationEnd {
    reason: String,
    last_received: Duration,
    pongs: u64,
}

async fn write_registration(socket: &mut Socket, message: Message) -> Result<(), String> {
    tokio::time::timeout(REGISTRATION_WRITE_TIMEOUT, socket.send(message))
        .await
        .map_err(|_| "write timed out".to_owned())?
        .map_err(|error| format!("write failed: {error}"))
}

async fn registration(socket: &mut Socket, announced: &mut mpsc::Receiver<String>) -> RegistrationEnd {
    let mut heard = tokio::time::Instant::now();
    let mut pongs = 0;
    let reason = match write_registration(socket, Message::Text(READY.into())).await {
        Err(error) => format!("ready {error}"),
        Ok(()) => {
            let mut ping = tokio::time::interval(PING_EVERY);
            loop {
                tokio::select! {
                    id = announced.recv() => match id {
                        Some(id) => if let Err(error) = write_registration(socket, Message::Text(format!("{SESSION}{id}").into())).await {
                            break format!("session notification {error}");
                        },
                        None => break "replaced by another registration".into(),
                    },
                    message = socket.next() => match message {
                        Some(Ok(Message::Close(frame))) => break match frame {
                            Some(frame) => format!("host closed with code {}", u16::from(frame.code)),
                            None => "host closed".into(),
                        },
                        None => break "connection ended".into(),
                        Some(Err(error)) => break format!("read failed: {error}"),
                        Some(Ok(message)) => {
                            heard = tokio::time::Instant::now();
                            if matches!(message, Message::Pong(_)) { pongs += 1; }
                        },
                    },
                    _ = ping.tick() => {
                        if heard.elapsed() > SILENCE_LIMIT {
                            break "heartbeat timeout".into();
                        }
                        if let Err(error) = write_registration(socket, Message::Ping(Vec::new().into())).await {
                            break format!("heartbeat {error}");
                        }
                    }
                }
            }
        }
    };
    RegistrationEnd { reason, last_received: heard.elapsed(), pongs }
}

/// A client asks for the server with `key`: the relay announces it, waits for the server to
/// take it, then forwards between them.
async fn connect(hub: Arc<Mutex<Hub>>, mut socket: Socket, key: PublicKey, address: IpAddr) {
    let mut id = [0; SESSION_ID_LEN];
    if getrandom::fill(&mut id).is_err() {
        return;
    }
    let id = hex(&id);
    let (joined, server) = oneshot::channel();
    let refused = {
        let mut locked = hub.lock().expect("hub lock");
        let sessions = locked.sessions;
        match locked.servers.get_mut(&key) {
            None => Some((OFFLINE, "offline")),
            Some(_) if sessions >= MAX_SESSIONS => Some((LIMITED, "too many sessions")),
            Some(server) if server.waiting >= MAX_WAITING => Some((LIMITED, "too many waiting")),
            Some(server) => {
                if server.notify.try_send(id.clone()).is_err() {
                    Some((LIMITED, "too many waiting"))
                } else {
                    server.waiting += 1;
                    locked.waiting.insert(id.clone(), Waiting { server: key, joined });
                    None
                }
            }
        }
    };
    if let Some((code, reason)) = refused {
        if code == LIMITED {
            hub.lock().expect("hub lock").limited += 1;
            event!("limit: {reason} for server {} from {address}", short(&key));
        }
        return close(socket, code, reason).await;
    }
    let taken = tokio::time::timeout(JOIN_TIMEOUT, server).await;
    let server_socket = {
        let mut locked = hub.lock().expect("hub lock");
        if locked.waiting.remove(&id).is_some()
            && let Some(server) = locked.servers.get_mut(&key)
        {
            server.waiting = server.waiting.saturating_sub(1);
        }
        match taken {
            Ok(Ok(server_socket)) => {
                locked.sessions += 1;
                locked.joined += 1;
                Some(server_socket)
            }
            _ => None,
        }
    };
    let Some(server_socket) = server_socket else { return close(socket, OFFLINE, "the server did not answer").await };
    let ended = if socket.send(Message::Text(READY.into())).await.is_ok() {
        let (carried, (code, reason)) = forward(socket, server_socket).await;
        event!("session for server {}: {reason} after {carried} bytes", short(&key));
        Some((carried, code == LIMITED))
    } else {
        None
    };
    let mut locked = hub.lock().expect("hub lock");
    locked.sessions -= 1;
    if let Some((carried, limited)) = ended {
        locked.carried += carried;
        locked.limited += u64::from(limited);
    }
}

/// A server takes the session `id` it was told of.
fn accept(hub: Arc<Mutex<Hub>>, socket: Socket, id: &str) {
    let mut locked = hub.lock().expect("hub lock");
    let Some(waiting) = locked.waiting.remove(id) else { return };
    if let Some(server) = locked.servers.get_mut(&waiting.server) {
        server.waiting = server.waiting.saturating_sub(1);
    }
    let _ = waiting.joined.send(socket);
}

/// Rate limiting by token bucket: bytes may go as fast as `RATE` allows, with `BURST` on top.
struct Bucket {
    tokens: f64,
    refilled: Instant,
}

impl Bucket {
    async fn take(&mut self, bytes: usize) {
        let now = Instant::now();
        self.tokens = (self.tokens + now.duration_since(self.refilled).as_secs_f64() * RATE).min(BURST);
        self.refilled = now;
        self.tokens -= bytes as f64;
        if self.tokens < 0.0 {
            tokio::time::sleep(Duration::from_secs_f64(-self.tokens / RATE)).await;
        }
    }
}

/// Forwards binary frames both ways until either side closes or a limit ends the session;
/// returns how many bytes it carried, and the close code and reason it ended with.
async fn forward(client: Socket, server: Socket) -> (u64, (u16, &'static str)) {
    let (mut client_tx, mut client_rx) = client.split();
    let (mut server_tx, mut server_rx) = server.split();
    let mut bucket = Bucket { tokens: BURST, refilled: Instant::now() };
    let (started, mut active) = (Instant::now(), Instant::now());
    let mut carried: u64 = 0;
    let mut ping = tokio::time::interval_at(tokio::time::Instant::now() + PING_EVERY, PING_EVERY);
    let reason = loop {
        let idle = tokio::time::sleep_until((active + IDLE_LIMIT).into());
        let over = tokio::time::sleep_until((started + LIFETIME).into());
        let (message, to_server) = tokio::select! {
            message = client_rx.next() => (message, true),
            message = server_rx.next() => (message, false),
            () = idle => break (u16::from(CloseCode::Normal), "idle"),
            () = over => break (LIMITED, "session too long"),
            // A ping is not activity: a session with nothing to carry still ends when idle.
            _ = ping.tick() => {
                let ping = || Message::Ping(Vec::new().into());
                if client_tx.send(ping()).await.is_err() || server_tx.send(ping()).await.is_err() {
                    break (u16::from(CloseCode::Normal), "closed");
                }
                continue;
            }
        };
        let bytes = match message {
            Some(Ok(Message::Binary(bytes))) => bytes,
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            _ => break (u16::from(CloseCode::Normal), "closed"),
        };
        if bytes.len() > FRAME_LIMIT {
            break (LIMITED, "frame too large");
        }
        carried += bytes.len() as u64;
        if carried > SESSION_BYTES {
            break (LIMITED, "session carried too much");
        }
        bucket.take(bytes.len()).await;
        active = Instant::now();
        let sent = if to_server {
            server_tx.send(Message::Binary(bytes)).await
        } else {
            client_tx.send(Message::Binary(bytes)).await
        };
        if sent.is_err() {
            break (u16::from(CloseCode::Normal), "closed");
        }
    };
    let (code, text) = reason;
    for tx in [&mut client_tx, &mut server_tx] {
        let frame = CloseFrame { code: CloseCode::from(code), reason: text.into() };
        let _ = tx.send(Message::Close(Some(frame))).await;
    }
    (carried, reason)
}

#[cfg(test)]
mod tests {
    use tokio_tungstenite::tungstenite::protocol::Role;

    use super::*;

    /// A socket as the relay holds it, and the borg at its other end.
    async fn joined() -> (Socket, WebSocketStream<tokio::io::DuplexStream>) {
        joined_with_capacity(64 * 1024).await
    }

    async fn joined_with_capacity(capacity: usize) -> (Socket, WebSocketStream<tokio::io::DuplexStream>) {
        let (near, far) = tokio::io::duplex(capacity);
        let near: Box<dyn Io> = Box::new(near);
        (
            WebSocketStream::from_raw_socket(near, Role::Server, None).await,
            WebSocketStream::from_raw_socket(far, Role::Client, None).await,
        )
    }

    #[tokio::test(start_paused = true)]
    async fn registration_reports_missing_heartbeats_instead_of_a_generic_disconnect() {
        let (mut socket, _host) = joined().await;
        let (_sender, mut announced) = mpsc::channel(1);
        let ended = registration(&mut socket, &mut announced).await;
        assert_eq!(ended.reason, "heartbeat timeout");
        assert!(ended.last_received > SILENCE_LIMIT);
        assert!(ended.last_received <= SILENCE_LIMIT + PING_EVERY);
        assert_eq!(ended.pongs, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn registration_reports_a_stalled_ready_write_with_a_deadline() {
        let (mut socket, _host) = joined_with_capacity(1).await;
        let (_sender, mut announced) = mpsc::channel(1);
        let start = tokio::time::Instant::now();
        let ended = registration(&mut socket, &mut announced).await;
        assert_eq!(ended.reason, "ready write timed out");
        assert_eq!(start.elapsed(), REGISTRATION_WRITE_TIMEOUT);
    }

    #[tokio::test(start_paused = true)]
    async fn registration_reports_host_close_and_replacement_separately() {
        let (mut socket, mut host) = joined().await;
        let (_sender, mut announced) = mpsc::channel(1);
        host.close(Some(CloseFrame { code: CloseCode::Normal, reason: "test".into() })).await.unwrap();
        let ended = registration(&mut socket, &mut announced).await;
        assert_eq!(ended.reason, "host closed with code 1000");

        let (mut socket, _host) = joined().await;
        let (sender, mut announced) = mpsc::channel(1);
        drop(sender);
        assert_eq!(registration(&mut socket, &mut announced).await.reason, "replaced by another registration");
    }

    #[tokio::test(start_paused = true)]
    async fn registration_keeps_answering_a_healthy_host_and_reports_its_pongs() {
        let (mut socket, mut host) = joined().await;
        let (sender, mut announced) = mpsc::channel(1);
        let task = tokio::spawn(async move { registration(&mut socket, &mut announced).await });
        assert!(matches!(host.next().await, Some(Ok(Message::Text(text))) if text == READY));
        for _ in 0..6 {
            assert!(matches!(host.next().await, Some(Ok(Message::Ping(_)))));
            host.flush().await.unwrap();
        }
        host.send(Message::Ping(b"sync".to_vec().into())).await.unwrap();
        assert!(matches!(host.next().await, Some(Ok(Message::Pong(bytes))) if bytes.as_ref() == b"sync"));
        sender.send("next-client".into()).await.unwrap();
        assert!(matches!(host.next().await, Some(Ok(Message::Text(text))) if text == format!("{SESSION}next-client")));
        assert!(!task.is_finished());
        drop(sender);
        let ended = task.await.unwrap();
        assert_eq!(ended.reason, "replaced by another registration");
        assert_eq!(ended.pongs, 6);
        assert!(ended.last_received < PING_EVERY);
    }

    #[tokio::test(start_paused = true)]
    async fn a_quiet_session_is_pinged_at_both_ends_and_goes_on() {
        let ((client, mut client_borg), (server, mut server_borg)) = (joined().await, joined().await);
        let forwarding = tokio::spawn(forward(client, server));
        tokio::time::sleep(PING_EVERY + Duration::from_secs(1)).await;
        for borg in [&mut client_borg, &mut server_borg] {
            assert!(matches!(borg.next().await, Some(Ok(Message::Ping(_)))));
        }
        client_borg.send(Message::Binary(b"after".to_vec().into())).await.unwrap();
        let forwarded = server_borg.next().await;
        assert!(matches!(&forwarded, Some(Ok(Message::Binary(bytes))) if &bytes[..] == b"after"), "{forwarded:?}");
        assert!(!forwarding.is_finished());
    }
}
