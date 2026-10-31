//! Borg cloud: Claude on the user's computer reaches this borg through a relay, with no open
//! port and no SSH, over a channel encrypted end to end between the two borgs (borg-link).
//!
//! On a server, borg keeps a connection to the relay while it is open and either paired or
//! pairing, and serves each paired computer's calls as it serves the local socket's. On the
//! user's computer, `borg pair` pairs with a server once, `borg mcp remote` is the MCP server
//! Claude starts, and `borg get` and `borg put` copy files.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicUsize},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use borg_core::Msg;
use borg_link::{Identity, PairingLink, Relay, SECRET_LEN};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc::UnboundedSender, oneshot},
    task::JoinHandle,
};

use crate::agent::{Call, Reply};

mod caller;
mod clients;
mod commands;
mod remote;
mod served;
mod store;

pub use clients::paired_clients;
use clients::{ClientConnection, client_status};
pub use commands::{get_command, pair_command, put_command};
pub use remote::{CHUNK, Remote, download_in_session, pair, upload_in_session};
use served::Served;
pub use store::{Computer, Server, forget_server, identity, server, servers};
use store::{claim, clean_name, load, same, save, state_dir, this_computer};

const LINK_DIR: &str = "link";
const KEY_FILE: &str = "key";
/// On a server: the computers paired with it. On a computer: the servers it paired with.
const COMPUTERS_FILE: &str = "computers.json";
const SERVERS_FILE: &str = "servers.json";
const CLIENTS_SOCKET: &str = "clients.sock";
const CLIENTS_TIMEOUT: Duration = Duration::from_secs(2);
const CLIENTS_LIMIT: u64 = 64 * 1024;
/// Held by the one borg on this machine that borg cloud reaches, with its process id inside.
const CLAIM_FILE: &str = "cloud.lock";
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
    connections: std::collections::HashMap<String, usize>,
}

/// The server's side: this borg reachable through the relay by the computers paired with it.
pub struct LinkServer {
    state: Option<PathBuf>,
    runtime_dir: PathBuf,
    session_id: Option<String>,
    relay: Option<Relay>,
    identity: Option<Identity>,
    shared: Arc<Mutex<Shared>>,
    task: Option<JoinHandle<()>>,
    /// This borg's claim on borg cloud, held while it keeps a connection to the relay.
    claimed: Option<std::fs::File>,
    /// Whether the user heard that another borg has borg cloud.
    told_taken: bool,
    background_service: bool,
    calls: UnboundedSender<(Call, oneshot::Sender<Reply>)>,
    messages: UnboundedSender<Msg>,
    clients: Arc<AtomicUsize>,
}

impl LinkServer {
    pub fn new(
        state: Option<PathBuf>,
        runtime_dir: PathBuf,
        session_id: Option<String>,
        relay: Option<Relay>,
        calls: UnboundedSender<(Call, oneshot::Sender<Reply>)>,
        messages: UnboundedSender<Msg>,
        clients: Arc<AtomicUsize>,
    ) -> Self {
        let computers =
            state.as_ref().map(|state| load(&state.join(LINK_DIR).join(COMPUTERS_FILE))).unwrap_or_default();
        let shared = Arc::new(Mutex::new(Shared { computers, pairing: None, connections: Default::default() }));
        Self {
            state,
            runtime_dir,
            session_id,
            relay,
            identity: None,
            shared,
            task: None,
            claimed: None,
            told_taken: false,
            background_service: false,
            calls,
            messages,
            clients,
        }
    }

    pub fn relay(&self) -> Option<&Relay> {
        self.relay.as_ref()
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn set_background_service(&mut self) {
        self.background_service = true;
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

    /// Holds borg cloud for this borg, unless another borg on this machine has it.
    fn claim(&mut self) -> Result<()> {
        if self.claimed.is_none() {
            self.identity()?;
            let state = self.state.as_deref().context("borg has no folder to keep its key in")?;
            self.claimed = Some(claim(state)?);
        }
        Ok(())
    }

    /// A pairing link that works once, for ten minutes, and replaces any earlier one.
    pub fn pair(&mut self) -> Result<String> {
        let relay = self.relay.clone().context("no relay is set")?;
        let identity = self.identity()?;
        self.claim()?;
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
            if let Err(error) = self.claim() {
                if !std::mem::replace(&mut self.told_taken, true) {
                    let _ = self.messages.send(Msg::Notice(format!("Borg cloud: {error:#}")));
                }
                return;
            }
            let (Some(relay), Ok(identity)) = (self.relay.clone(), self.identity()) else { return };
            let server = Served {
                background_service: self.background_service,
                relay,
                identity,
                shared: self.shared.clone(),
                state: self.state.clone(),
                runtime_dir: self.runtime_dir.clone(),
                session_id: self.session_id.clone(),
                calls: self.calls.clone(),
                messages: self.messages.clone(),
                clients: self.clients.clone(),
            };
            self.task = Some(tokio::spawn(server.run()));
        } else if !wanted {
            if let Some(task) = self.task.take() {
                task.abort();
            }
            self.claimed = None;
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

#[cfg(test)]
mod tests;
