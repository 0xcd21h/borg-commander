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
/// A registered server is pinged this often, and dropped when it stops answering.
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
    if socket.send(Message::Binary(message.into())).await.is_err() {
        return;
    }
    let answer = tokio::time::timeout(PROOF_TIMEOUT, socket.next()).await;
    let Ok(Some(Ok(Message::Binary(answer)))) = answer else { return };
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
    if socket.send(Message::Text(READY.into())).await.is_ok() {
        let mut ping = tokio::time::interval(PING_EVERY);
        let mut heard = Instant::now();
        loop {
            tokio::select! {
                id = announced.recv() => match id {
                    Some(id) => if socket.send(Message::Text(format!("{SESSION}{id}").into())).await.is_err() { break },
                    // A newer registration for the key took over.
                    None => break,
                },
                message = socket.next() => match message {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => heard = Instant::now(),
                },
                _ = ping.tick() => {
                    if heard.elapsed() > SILENCE_LIMIT || socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
    let mut locked = hub.lock().expect("hub lock");
    if locked.servers.get(&key).is_some_and(|server| server.generation == generation) {
        locked.servers.remove(&key);
    }
    drop(locked);
    event!("server {}: gone", short(&key));
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
    let reason = loop {
        let idle = tokio::time::sleep_until((active + IDLE_LIMIT).into());
        let over = tokio::time::sleep_until((started + LIFETIME).into());
        let (message, to_server) = tokio::select! {
            message = client_rx.next() => (message, true),
            message = server_rx.next() => (message, false),
            () = idle => break (u16::from(CloseCode::Normal), "idle"),
            () = over => break (LIMITED, "session too long"),
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
