//! Borg cloud: Claude on the user's computer reaches this borg through a relay, with no open
//! port and no SSH, over a channel encrypted end to end between the two borgs (borg-link).
//!
//! On a server, borg keeps a connection to the relay while it is open and either paired or
//! pairing, and serves each paired computer's calls as it serves the local socket's. On the
//! user's computer, `borg pair` pairs with a server once, `borg mcp remote` is the MCP server
//! Claude starts, and `borg get` and `borg put` copy files.

use std::{
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use borg_core::{AgentEvent, Msg};
use borg_link::{
    Channel, Identity, PairingCode, PairingLink, PublicKey, Relay, SECRET_LEN, fingerprint, hex, initiate, relay,
    respond, unhex,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use tokio::{
    sync::{mpsc::UnboundedSender, oneshot},
    task::{JoinHandle, JoinSet},
};

use crate::agent::{Call, Reply, note};

const LINK_DIR: &str = "link";
const KEY_FILE: &str = "key";
/// On a server: the computers paired with it. On a computer: the servers it paired with.
const COMPUTERS_FILE: &str = "computers.json";
const SERVERS_FILE: &str = "servers.json";
const PRIVATE_FILE: u32 = 0o600;
/// How long a pairing link works, and how many wrong secrets end it sooner.
const PAIRING_TIME: Duration = Duration::from_secs(10 * 60);
const PAIRING_TRIES: u32 = 5;
/// How long a pairing waits for the user's answer in borg on the server.
const APPROVAL_WAIT: Duration = Duration::from_secs(5 * 60);
/// Waits between attempts to reach the relay.
const RETRY_FIRST: Duration = Duration::from_secs(2);
const RETRY_LAST: Duration = Duration::from_secs(60);
const NAME_LIMIT: usize = 40;
/// The relay borg uses unless the config names another, as `relay = "wss://host"`.
pub const DEFAULT_RELAY: Option<&str> = None;
/// The internal call that asks the user to approve a pairing.
pub const PAIR_CALL: &str = "_pair";

/// The first message of a channel, encrypted: a pairing, or a session of a paired computer.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Hello {
    Pair { secret: String, name: String },
    Session,
}

/// The server's answer to a hello.
#[derive(Serialize, Deserialize)]
struct Answer {
    ok: bool,
    #[serde(default)]
    name: String,
    #[serde(default)]
    error: String,
}

/// A computer paired with this server.
#[derive(Clone, Serialize, Deserialize)]
pub struct Computer {
    pub name: String,
    pub key: String,
}

/// A server this computer paired with.
#[derive(Clone, Serialize, Deserialize)]
pub struct Server {
    pub name: String,
    pub relay: String,
    pub key: String,
}

pub fn identity(state: &Path) -> Result<Identity> {
    let path = state.join(LINK_DIR).join(KEY_FILE);
    Identity::load_or_create(&path).with_context(|| format!("reading the key in {}", path.display()))
}

fn load<T: DeserializeOwned>(path: &Path) -> Vec<T> {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Writes a list owner-only, replacing the old one whole.
fn save<T: Serialize>(path: &Path, items: &[T]) -> Result<()> {
    use std::io::Write as _;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let staged = path.with_extension(format!("{}.new", std::process::id()));
    let mut file =
        std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(PRIVATE_FILE).open(&staged)?;
    file.write_all(&serde_json::to_vec_pretty(items)?)?;
    drop(file);
    std::fs::rename(&staged, path)?;
    Ok(())
}

/// A name from the other side, kept short and printable.
fn clean_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).take(NAME_LIMIT).collect();
    if name.trim().is_empty() { "computer".to_owned() } else { name.trim().to_owned() }
}

/// Compares secrets in time that does not depend on where they differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |differ, (x, y)| differ | (x ^ y)) == 0
}

/// Who this computer is to a server it pairs with: the user and the machine.
fn this_computer() -> String {
    let user = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_default();
    let host = crate::agent::hostname();
    if user.is_empty() { host } else { format!("{user}@{host}") }
}

struct Pairing {
    secret: [u8; SECRET_LEN],
    until: Instant,
    tries: u32,
    /// A computer used the secret, which works for no other, and waits for the user's answer.
    claimed: bool,
}

impl Pairing {
    /// Keeps the relay connection: open for a computer, or one waiting for approval.
    fn live(&self) -> bool {
        self.claimed || self.until > Instant::now()
    }
}

struct Shared {
    computers: Vec<Computer>,
    pairing: Option<Pairing>,
}

/// The server's side: this borg reachable through the relay by the computers paired with it.
pub struct LinkServer {
    state: Option<PathBuf>,
    relay: Option<Relay>,
    identity: Option<Identity>,
    shared: Arc<Mutex<Shared>>,
    task: Option<JoinHandle<()>>,
    calls: UnboundedSender<(Call, oneshot::Sender<Reply>)>,
    messages: UnboundedSender<Msg>,
    clients: Arc<AtomicUsize>,
}

impl LinkServer {
    pub fn new(
        state: Option<PathBuf>,
        relay: Option<Relay>,
        calls: UnboundedSender<(Call, oneshot::Sender<Reply>)>,
        messages: UnboundedSender<Msg>,
        clients: Arc<AtomicUsize>,
    ) -> Self {
        let computers =
            state.as_ref().map(|state| load(&state.join(LINK_DIR).join(COMPUTERS_FILE))).unwrap_or_default();
        let shared = Arc::new(Mutex::new(Shared { computers, pairing: None }));
        Self { state, relay, identity: None, shared, task: None, calls, messages, clients }
    }

    pub fn relay(&self) -> Option<&Relay> {
        self.relay.as_ref()
    }

    /// Goes through `relay` from now on; the connection to the one before ends.
    pub fn set_relay(&mut self, relay: Option<Relay>) {
        if relay != self.relay {
            self.relay = relay;
            if let Some(task) = self.task.take() {
                task.abort();
            }
        }
    }

    pub fn computers(&self) -> Vec<String> {
        self.shared.lock().expect("link lock").computers.iter().map(|computer| computer.name.clone()).collect()
    }

    fn identity(&mut self) -> Result<Identity> {
        if let Some(identity) = &self.identity {
            return Ok(identity.clone());
        }
        let state = self.state.as_deref().context("borg has no folder to keep its key in")?;
        let identity = identity(state)?;
        self.identity = Some(identity.clone());
        Ok(identity)
    }

    /// A pairing link that works once, for ten minutes, and replaces any earlier one.
    pub fn pair(&mut self) -> Result<String> {
        let relay = self.relay.clone().context("no relay is set")?;
        let identity = self.identity()?;
        let secret = borg_link::secret();
        self.shared.lock().expect("link lock").pairing =
            Some(Pairing { secret, until: Instant::now() + PAIRING_TIME, tries: 0, claimed: false });
        Ok(PairingLink { relay, server: *identity.public(), secret }.to_string())
    }

    /// Forgets every paired computer, and ends their sessions.
    pub fn forget(&mut self) -> Result<()> {
        self.shared.lock().expect("link lock").computers.clear();
        if let Some(state) = &self.state {
            save::<Computer>(&state.join(LINK_DIR).join(COMPUTERS_FILE), &[])?;
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
        Ok(())
    }

    /// Keeps the connection to the relay while it is wanted: enabled, and paired or pairing.
    pub fn sync(&mut self, enabled: bool) {
        let wanted = enabled && self.relay.is_some() && self.state.is_some() && {
            let shared = self.shared.lock().expect("link lock");
            !shared.computers.is_empty() || shared.pairing.as_ref().is_some_and(Pairing::live)
        };
        let running = self.task.as_ref().is_some_and(|task| !task.is_finished());
        if wanted && !running {
            let (Some(relay), Ok(identity)) = (self.relay.clone(), self.identity()) else { return };
            let server = Served {
                relay,
                identity,
                shared: self.shared.clone(),
                state: self.state.clone(),
                calls: self.calls.clone(),
                messages: self.messages.clone(),
                clients: self.clients.clone(),
            };
            self.task = Some(tokio::spawn(server.run()));
        } else if !wanted && let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl Drop for LinkServer {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// What the relay connection and its sessions need.
#[derive(Clone)]
struct Served {
    relay: Relay,
    identity: Identity,
    shared: Arc<Mutex<Shared>>,
    state: Option<PathBuf>,
    calls: UnboundedSender<(Call, oneshot::Sender<Reply>)>,
    messages: UnboundedSender<Msg>,
    clients: Arc<AtomicUsize>,
}

impl Served {
    /// Registers at the relay and serves the sessions it announces, registering again after
    /// the relay drops it. Ending this task ends every session.
    async fn run(self) {
        let mut retry = RETRY_FIRST;
        let mut told = false;
        loop {
            match relay::Listener::register(&self.relay, &self.identity).await {
                Ok(mut listener) => {
                    retry = RETRY_FIRST;
                    told = false;
                    let mut sessions = JoinSet::new();
                    loop {
                        tokio::select! {
                            next = listener.next() => match next {
                                Ok(Some(id)) => drop(sessions.spawn(self.clone().session(id))),
                                _ => break,
                            },
                            Some(_) = sessions.join_next(), if !sessions.is_empty() => {}
                        }
                    }
                }
                Err(error) if !told => {
                    told = true;
                    let _ = self.messages.send(Msg::Notice(format!("Borg cloud: {error:#}")));
                }
                Err(_) => {}
            }
            tokio::time::sleep(retry).await;
            retry = (retry * 2).min(RETRY_LAST);
        }
    }

    async fn session(self, id: String) {
        let Ok(socket) = relay::take(&self.relay, &id).await else { return };
        let Ok((mut channel, hello)) = respond(socket, &self.identity).await else { return };
        let _ = match serde_json::from_slice::<Hello>(&hello) {
            Ok(Hello::Pair { secret, name }) => self.pair(channel, &secret, &name).await,
            Ok(Hello::Session) => self.serve(channel).await,
            Err(_) => refuse(&mut channel, "unreadable hello").await,
        };
    }

    /// A computer pairs: its secret must match the link this borg shows, and the user must
    /// approve it here, by name and the code both ends show.
    async fn pair(&self, mut channel: Channel, secret: &str, name: &str) -> Result<()> {
        let valid = {
            let mut shared = self.shared.lock().expect("link lock");
            let offered = unhex::<SECRET_LEN>(secret);
            match shared.pairing.as_mut() {
                Some(pairing) if !pairing.claimed && pairing.until > Instant::now() => {
                    if offered.is_some_and(|offered| same(&offered, &pairing.secret)) {
                        // A link works once.
                        pairing.claimed = true;
                        true
                    } else {
                        pairing.tries += 1;
                        if pairing.tries >= PAIRING_TRIES {
                            shared.pairing = None;
                        }
                        false
                    }
                }
                _ => false,
            }
        };
        if !valid {
            return refuse(&mut channel, "this pairing link does not work any more; show a new one in borg").await;
        }
        let name = clean_name(name);
        let key = *channel.peer();
        let (reply, answer) = oneshot::channel();
        let ask = Call::new(PAIR_CALL, json!({ "name": name, "code": channel.pairing_code().rows() }));
        self.calls.send((ask, reply)).context("borg closed")?;
        let approved = matches!(tokio::time::timeout(APPROVAL_WAIT, answer).await, Ok(Ok(reply)) if reply.ok);
        self.shared.lock().expect("link lock").pairing = None;
        if !approved {
            return refuse(&mut channel, "the pairing was declined in borg").await;
        }
        let computers = {
            let mut shared = self.shared.lock().expect("link lock");
            shared.computers.retain(|computer| computer.key != hex(&key));
            shared.computers.push(Computer { name: name.clone(), key: hex(&key) });
            shared.computers.clone()
        };
        if let Some(state) = &self.state {
            save(&state.join(LINK_DIR).join(COMPUTERS_FILE), &computers)?;
        }
        note(&self.messages, &format!("paired with {name} ({})", fingerprint(&key)));
        let _ = self
            .messages
            .send(Msg::Agent(AgentEvent::Paired { computers: computers.into_iter().map(|c| c.name).collect() }));
        let answer = Answer { ok: true, name: crate::agent::hostname(), error: String::new() };
        channel.send(&serde_json::to_vec(&answer)?).await
    }

    /// A paired computer's session: its calls go to the session as the local socket's do,
    /// marked with the computer's name.
    async fn serve(&self, mut channel: Channel) -> Result<()> {
        let key = hex(channel.peer());
        let computer = self.shared.lock().expect("link lock").computers.iter().find(|c| c.key == key).cloned();
        let Some(computer) = computer else {
            return refuse(&mut channel, "this computer is not paired with this borg").await;
        };
        let answer = Answer { ok: true, name: crate::agent::hostname(), error: String::new() };
        channel.send(&serde_json::to_vec(&answer)?).await?;
        let count = self.clients.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.messages.send(Msg::Agent(AgentEvent::Connected { clients: count }));
        note(&self.messages, &format!("{} connected through borg cloud", computer.name));
        let result = self.calls_of(&mut channel, &computer).await;
        let count = self.clients.fetch_sub(1, Ordering::SeqCst) - 1;
        let _ = self.messages.send(Msg::Agent(AgentEvent::Connected { clients: count }));
        note(&self.messages, &format!("{} disconnected", computer.name));
        result
    }

    async fn calls_of(&self, channel: &mut Channel, computer: &Computer) -> Result<()> {
        while let Some(message) = channel.recv().await? {
            // A computer forgotten while connected is cut off.
            if !self.shared.lock().expect("link lock").computers.iter().any(|c| c.key == computer.key) {
                break;
            }
            let reply = match serde_json::from_slice::<Call>(&message).map(Call::external) {
                Ok(Ok(mut call)) => {
                    call.origin = Some(computer.name.clone());
                    let (sender, receiver) = oneshot::channel();
                    self.calls.send((call, sender)).context("borg closed")?;
                    receiver.await.unwrap_or_else(|_| Reply::error("Borg closed before answering."))
                }
                Ok(Err(refused)) => refused,
                Err(error) => Reply::error(format!("unreadable call: {error}")),
            };
            channel.send(&serde_json::to_vec(&reply)?).await?;
        }
        Ok(())
    }
}

async fn refuse(channel: &mut Channel, error: &str) -> Result<()> {
    let answer = Answer { ok: false, name: String::new(), error: error.to_owned() };
    channel.send(&serde_json::to_vec(&answer)?).await
}

/// The computer's side: pairs with the server a link names. The user approves it in borg on
/// the server, comparing the code `shown` here; then the server is saved by name.
pub async fn pair(state: &Path, link: &str, shown: impl FnOnce(&PairingCode)) -> Result<Server> {
    let link = PairingLink::parse(link).context("that is not a borg pairing link")?;
    let identity = identity(state)?;
    let socket = relay::dial(&link.relay, &link.server).await?;
    let hello = serde_json::to_vec(&Hello::Pair { secret: hex(&link.secret), name: this_computer() })?;
    let mut channel = initiate(socket, &identity, &link.server, &hello).await?;
    shown(&channel.pairing_code());
    let reply = tokio::time::timeout(APPROVAL_WAIT + Duration::from_secs(30), channel.recv())
        .await
        .context("no answer from borg on the server")??
        .context("the server closed the connection")?;
    let answer: Answer = serde_json::from_slice(&reply)?;
    if !answer.ok {
        bail!("{}", answer.error);
    }
    let path = state.join(LINK_DIR).join(SERVERS_FILE);
    let mut servers: Vec<Server> = load(&path);
    servers.retain(|server| server.key != hex(&link.server));
    let base = clean_name(&answer.name).replace(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_', "-");
    let mut name = base.clone();
    let mut n = 2;
    while servers.iter().any(|server| server.name == name) {
        name = format!("{base}-{n}");
        n += 1;
    }
    let server = Server { name, relay: link.relay.to_string(), key: hex(&link.server) };
    servers.push(server.clone());
    save(&path, &servers)?;
    Ok(server)
}

pub fn servers(state: &Path) -> Vec<Server> {
    load(&state.join(LINK_DIR).join(SERVERS_FILE))
}

/// Forgets a paired server by name; whether there was one.
pub fn forget_server(state: &Path, name: &str) -> Result<bool> {
    let path = state.join(LINK_DIR).join(SERVERS_FILE);
    let mut servers: Vec<Server> = load(&path);
    let before = servers.len();
    servers.retain(|server| server.name != name);
    save(&path, &servers)?;
    Ok(servers.len() < before)
}

pub fn server(state: &Path, name: &str) -> Result<Server> {
    servers(state).into_iter().find(|server| server.name == name).with_context(|| {
        format!("no server named {name} is paired; `borg pair` lists them, and pairs with a link borg shows")
    })
}

/// A session with a paired server, for calls.
pub struct Remote {
    channel: Channel,
}

impl Remote {
    pub async fn open(state: &Path, server: &Server) -> Result<Self> {
        let relay = Relay::parse(&server.relay).context("the saved relay is not valid")?;
        let key: PublicKey = unhex(&server.key).context("the saved key is not valid")?;
        let identity = identity(state)?;
        let socket =
            relay::dial(&relay, &key).await.with_context(|| format!("borg on {} is not reachable", server.name))?;
        let mut channel = initiate(socket, &identity, &key, &serde_json::to_vec(&Hello::Session)?).await?;
        let reply = channel.recv().await?.context("the server closed the connection")?;
        let answer: Answer = serde_json::from_slice(&reply)?;
        if !answer.ok {
            bail!("{}", answer.error);
        }
        Ok(Self { channel })
    }

    pub async fn call(&mut self, call: &Call) -> Result<Reply> {
        self.channel.send(&serde_json::to_vec(call)?).await?;
        let reply = self.channel.recv().await?.context("the server closed the connection")?;
        Ok(serde_json::from_slice(&reply)?)
    }
}

/// Bytes a single call to `get` or `put` carries.
pub const CHUNK: u64 = 1024 * 1024;

#[derive(Deserialize)]
struct Piece {
    size: u64,
    data: String,
}

/// Copies a file from the server to `to` on this computer, piece by piece; its size.
pub async fn download(remote: &mut Remote, path: &str, to: &Path, mut progress: impl FnMut(u64, u64)) -> Result<u64> {
    use base64::Engine as _;
    use std::io::Write as _;
    let staged = to.with_extension(format!("borg-download-{}", std::process::id()));
    let mut file = std::fs::File::create(&staged).with_context(|| format!("could not write {}", staged.display()))?;
    let mut offset = 0;
    let result = async {
        loop {
            let reply = remote.call(&Call::new("get", json!({ "path": path, "offset": offset }))).await?;
            if !reply.ok {
                bail!("{}", reply.text);
            }
            let piece: Piece = serde_json::from_str(&reply.text)?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(piece.data)?;
            file.write_all(&bytes)?;
            offset += bytes.len() as u64;
            progress(offset, piece.size);
            if offset >= piece.size || bytes.is_empty() {
                return Ok(offset);
            }
        }
    }
    .await;
    drop(file);
    match result {
        Ok(size) => {
            std::fs::rename(&staged, to)?;
            Ok(size)
        }
        Err(error) => {
            let _ = std::fs::remove_file(&staged);
            Err(error)
        }
    }
}

/// Copies a file from this computer to `path` on the server, after the user approves it
/// there; its size.
pub async fn upload(remote: &mut Remote, from: &Path, path: &str, mut progress: impl FnMut(u64, u64)) -> Result<u64> {
    use base64::Engine as _;
    use std::io::Read as _;
    let mut file = std::fs::File::open(from).with_context(|| format!("could not read {}", from.display()))?;
    let size = file.metadata()?.len();
    let reply = remote.call(&Call::new("put", json!({ "path": path, "size": size }))).await?;
    if !reply.ok {
        bail!("{}", reply.text);
    }
    let token = reply.text;
    let mut sent = 0;
    let mut buffer = vec![0; usize::try_from(CHUNK).unwrap_or(1 << 20)];
    loop {
        let read = file.read(&mut buffer)?;
        let done = read == 0;
        let data = base64::engine::general_purpose::STANDARD.encode(&buffer[..read]);
        let args = json!({ "token": token, "data": data, "done": done });
        let reply = remote.call(&Call::new("put", args)).await?;
        if !reply.ok {
            bail!("{}", reply.text);
        }
        sent += read as u64;
        progress(sent, size);
        if done {
            return Ok(sent);
        }
    }
}

/// The folder this computer keeps its key and paired servers in.
fn state_dir() -> Result<PathBuf> {
    crate::session::StateDir::locate()
        .map(|dir| dir.path().to_path_buf())
        .context("borg has no folder to keep its key in: set HOME or BORG_STATE_DIR")
}

/// `borg pair [LINK | --forget NAME]`: pairs this computer with the borg that shows LINK,
/// or lists the paired ones, or forgets one.
pub async fn pair_command(args: &[String]) -> Result<()> {
    let state = state_dir()?;
    match args {
        [] => {
            let servers = servers(&state);
            if servers.is_empty() {
                println!("No borg is paired with this computer. In borg on a server, press F1 then C for a link.");
            }
            for server in servers {
                println!("{}  {}  {}", server.name, server.relay, fingerprint(&unhex(&server.key).unwrap_or_default()));
            }
            Ok(())
        }
        [flag, name] if flag == "--forget" => {
            if forget_server(&state, name)? {
                println!("Forgot {name}.");
                Ok(())
            } else {
                bail!("no server named {name} is paired")
            }
        }
        [link] => {
            println!("Pairing through borg cloud…");
            let server = pair(&state, link, |code| {
                let [first, second] = code.rows();
                println!("The pairing code is\n\n  {first}\n  {second}\n");
                println!("Approve this computer in borg on the server when it shows the same code.");
            })
            .await?;
            let program = crate::agent::program_path();
            let args = ["mcp", "add", &format!("borg-{}", server.name), "--", &program, "mcp", "remote", &server.name];
            let shown: Vec<String> = args.iter().map(|arg| borg_core::shell_quote(arg)).collect();
            println!("\nPaired with {}.\n\nConnect Claude Code to it:\n\n  claude {}\n", server.name, shown.join(" "));
            offer_claude(&args);
            Ok(())
        }
        _ => bail!("usage: borg pair [LINK | --forget NAME]"),
    }
}

/// Offers to run `claude mcp add` for the user, when Claude Code is here and someone answers.
fn offer_claude(args: &[&str]) {
    use std::io::{BufRead, IsTerminal, Write};
    let found = std::process::Command::new("claude").arg("--version").output().is_ok_and(|out| out.status.success());
    if !found || !std::io::stdin().is_terminal() {
        return;
    }
    print!("Add it to Claude Code now? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err() || !answer.trim().eq_ignore_ascii_case("y") {
        return;
    }
    match std::process::Command::new("claude").args(args).status() {
        Ok(status) if status.success() => println!("Added. Claude Code reaches it in new sessions."),
        _ => println!("Claude Code did not take it; run the line above yourself."),
    }
}

/// Splits `NAME:PATH`, the form `borg get` and `borg put` name a file on a paired server by.
fn on_server(text: &str) -> Result<(&str, &str)> {
    text.split_once(':')
        .filter(|(name, path)| !name.is_empty() && !path.is_empty())
        .context("name the file as NAME:PATH")
}

fn progress(done: u64, total: u64) {
    use std::io::Write as _;
    let percent = (done * 100).checked_div(total).unwrap_or(100);
    eprint!("\r{percent:>3}%  {} of {}", borg_ui::size_text(done), borg_ui::size_text(total));
    let _ = std::io::stderr().flush();
}

/// `borg get NAME:PATH [TO]`: copies a file from a paired server to this computer.
pub async fn get_command(args: &[String]) -> Result<()> {
    let [from, rest @ ..] = args else { bail!("usage: borg get NAME:PATH [TO]") };
    let (name, path) = on_server(from)?;
    let state = state_dir()?;
    let server = server(&state, name)?;
    let mut remote = Remote::open(&state, &server).await?;
    let file_name = Path::new(path).file_name().context("the path names no file")?.to_os_string();
    let mut to = rest.first().map_or_else(|| PathBuf::from("."), PathBuf::from);
    if to.is_dir() {
        to = to.join(file_name);
    }
    let size = download(&mut remote, path, &to, progress).await?;
    eprintln!();
    println!("Downloaded {name}:{path} ({}) to {}", borg_ui::size_text(size), to.display());
    Ok(())
}

/// `borg put FROM NAME:PATH`: copies a file from this computer to a paired server, after the
/// user approves it in borg there.
pub async fn put_command(args: &[String]) -> Result<()> {
    let [from, to] = args else { bail!("usage: borg put FROM NAME:PATH") };
    let (name, path) = on_server(to)?;
    let state = state_dir()?;
    let server = server(&state, name)?;
    let mut remote = Remote::open(&state, &server).await?;
    println!("Approve the upload in borg on {name}…");
    let size = upload(&mut remote, Path::new(from), path, progress).await?;
    eprintln!();
    println!("Uploaded {from} ({}) to {name}:{path}", borg_ui::size_text(size));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_compare_whole_and_names_stay_short_and_printable() {
        assert!(same(b"abcd", b"abcd") && !same(b"abcd", b"abce") && !same(b"abc", b"abcd"));
        assert_eq!(clean_name("  dev\u{7}box  "), "devbox");
        assert_eq!(clean_name(""), "computer");
        assert_eq!(clean_name(&"x".repeat(100)).len(), NAME_LIMIT);
    }
}
