//! The server's side of borg cloud: the relay connection, pairing, and the calls of each paired computer.

use super::caller::Caller;
use super::{
    APPROVAL_WAIT, Answer, CLIENTS_SOCKET, CLIENTS_TIMEOUT, COMPUTERS_FILE, ClientConnection, Computer, Hello,
    LINK_DIR, PAIR_CALL, PAIRING_TRIES, PRIVATE_FILE, RETRY_FIRST, RETRY_LAST, Shared, clean_name, client_status, same,
    save, this_computer,
};
use crate::{
    agent::{Call, Reply, note},
    session::StateDir,
    sessions::NamedSession,
};
use anyhow::{Context, Result};
use borg_core::{AgentEvent, Msg};
use borg_link::{Channel, Identity, Relay, SECRET_LEN, fingerprint, hex, relay, respond, unhex};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicUsize},
    time::Instant,
};
use tokio::{
    io::AsyncWriteExt,
    net::UnixListener,
    sync::{mpsc::UnboundedSender, oneshot},
    task::JoinSet,
};

/// What the relay connection and its sessions need.
#[derive(Clone)]
pub(super) struct Served {
    pub(super) background_service: bool,
    pub(super) relay: Relay,
    pub(super) identity: Identity,
    pub(super) shared: Arc<Mutex<Shared>>,
    pub(super) state: Option<PathBuf>,
    pub(super) runtime_dir: PathBuf,
    pub(super) session_id: Option<String>,
    pub(super) calls: UnboundedSender<(Call, oneshot::Sender<Reply>)>,
    pub(super) messages: UnboundedSender<Msg>,
    pub(super) clients: Arc<AtomicUsize>,
}

impl Served {
    pub(super) async fn run(self) {
        let status = async {
            let Some(state) = &self.state else { return std::future::pending::<()>().await };
            let path = state.join(LINK_DIR).join(CLIENTS_SOCKET);
            let _ = std::fs::remove_file(&path);
            let listener = match UnixListener::bind(&path) {
                Ok(listener) => listener,
                Err(_) => return std::future::pending::<()>().await,
            };
            use std::os::unix::fs::PermissionsExt;
            if std::fs::set_permissions(&path, std::fs::Permissions::from_mode(PRIVATE_FILE)).is_err() {
                return std::future::pending::<()>().await;
            }
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return std::future::pending::<()>().await;
                };
                let status = client_status(&self.shared.lock().expect("link lock"));
                if let Ok(mut bytes) = serde_json::to_vec(&status) {
                    bytes.push(b'\n');
                    let _ = tokio::time::timeout(CLIENTS_TIMEOUT, stream.write_all(&bytes)).await;
                }
            }
        };
        tokio::select! {
            _ = self.listen() => {},
            _ = status => {},
        }
    }

    /// Registers at the relay and serves the sessions it announces, registering again after
    /// the relay drops it. Ending this task ends every session.
    pub(super) async fn listen(&self) {
        let mut retry = RETRY_FIRST;
        let mut told = false;
        // Sessions outlast a registration the relay dropped: each has a socket of its own.
        let mut sessions = JoinSet::new();
        loop {
            match relay::Listener::register(&self.relay, &self.identity).await {
                Ok(mut listener) => {
                    retry = RETRY_FIRST;
                    told = false;
                    note(&self.messages, &borg_i18n::tr!(agent_cloud_registered, relay = self.relay.to_string()));
                    loop {
                        tokio::select! {
                            next = listener.next() => match next {
                                Ok(Some(id)) => drop(sessions.spawn(self.clone().session(id))),
                                ended => {
                                    let error = match ended {
                                        Err(error) => format!("{error:#}"),
                                        _ => borg_i18n::tr!(agent_cloud_connection_closed).into(),
                                    };
                                    self.reconnecting(&error);
                                    break;
                                },
                            },
                            Some(_) = sessions.join_next(), if !sessions.is_empty() => {}
                        }
                    }
                }
                Err(error) if !told => {
                    told = true;
                    self.reconnecting(&format!("{error:#}"));
                }
                Err(_) => {}
            }
            tokio::time::sleep(retry).await;
            retry = (retry * 2).min(RETRY_LAST);
        }
    }

    pub(super) fn reconnecting(&self, error: &str) {
        let message = borg_i18n::tr!(agent_cloud_reconnecting, error = error);
        note(&self.messages, &message);
        let _ = self.messages.send(Msg::Notice(message));
    }

    pub(super) async fn session(self, id: String) {
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
    pub(super) async fn pair(&self, mut channel: Channel, secret: &str, name: &str) -> Result<()> {
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
        let code = channel.pairing_code();
        let ask = Call::new(PAIR_CALL, json!({ "name": name, "code": code.rows(true), "names": code.rows(false) }));
        self.calls.send((ask, reply)).context("borg closed")?;
        let approved = matches!(tokio::time::timeout(APPROVAL_WAIT, answer).await, Ok(Ok(reply)) if reply.ok);
        // The pairing lasts until the computer has its answer: with it gone and no computer
        // paired, borg would leave the relay and end this session first.
        if !approved {
            let refused = refuse(&mut channel, "the pairing was declined in borg").await;
            self.shared.lock().expect("link lock").pairing = None;
            return refused;
        }
        let computers = {
            let mut shared = self.shared.lock().expect("link lock");
            shared.pairing = None;
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
    pub(super) async fn serve(&self, mut channel: Channel) -> Result<()> {
        let key = hex(channel.peer());
        let computer = self.shared.lock().expect("link lock").computers.iter().find(|c| c.key == key).cloned();
        let Some(computer) = computer else {
            return refuse(&mut channel, "this computer is not paired with this borg").await;
        };
        let answer = Answer { ok: true, name: crate::agent::hostname(), error: String::new() };
        channel.send(&serde_json::to_vec(&answer)?).await?;
        let _connection =
            ClientConnection::new(self.shared.clone(), self.clients.clone(), self.messages.clone(), computer.clone());
        self.calls_of(&mut channel, &computer).await
    }

    /// Answers a paired computer's calls until it goes, then gives back what it held.
    async fn calls_of(&self, channel: &mut Channel, computer: &Computer) -> Result<()> {
        let mut caller = Caller::new(self, computer);
        let result = caller.serve(channel).await;
        caller.release().await;
        result
    }

    /// Hands `call` to the app and waits for its reply.
    pub(super) async fn ask(&self, call: Call) -> Result<Reply> {
        let (sender, receiver) = oneshot::channel();
        self.calls.send((call, sender)).context("borg closed")?;
        Ok(receiver.await.unwrap_or_else(|_| Reply::error("Borg closed before answering.")))
    }

    fn sessions(&self) -> Option<Result<Vec<NamedSession>>> {
        self.state.as_ref().map(|state| crate::sessions::list(&StateDir::at(state.clone())))
    }

    /// The named sessions on this machine, when Borg has a state folder and can read it.
    pub(super) fn named_sessions(&self) -> Option<Vec<NamedSession>> {
        self.sessions().and_then(Result::ok)
    }

    /// Whether `target` is the ID of a named session that is not archived.
    pub(super) fn is_active(&self, target: Option<&str>) -> bool {
        self.named_sessions().is_some_and(|sessions| {
            sessions.iter().any(|session| !session.archived && Some(session.id.as_str()) == target)
        })
    }

    /// This borg's version, machine and named sessions, for the computer's list of hosts.
    pub(super) fn host_info(&self) -> Reply {
        match self.sessions() {
            Some(Ok(sessions)) => Reply::ok(
                json!({
                    "version": env!("CARGO_PKG_VERSION"),
                    "hostname": this_computer(),
                    "os": std::env::consts::OS,
                    "arch": std::env::consts::ARCH,
                    "service": self.background_service,
                    "sessions": sessions.into_iter().filter(|entry| !entry.archived).collect::<Vec<_>>(),
                })
                .to_string(),
            ),
            Some(Err(error)) => Reply::error(format!("{error:#}")),
            None => Reply::error("Borg has no state folder for sessions"),
        }
    }

    pub(super) fn list_sessions(&self) -> Reply {
        match self.sessions() {
            Some(Ok(found)) => Reply::ok(
                found
                    .into_iter()
                    .map(|entry| {
                        format!("{}  {}{}", entry.id, entry.name, if entry.archived { " (archived)" } else { "" })
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            Some(Err(error)) => Reply::error(format!("{error:#}")),
            None => Reply::error("Borg has no state folder for sessions"),
        }
    }
}

pub(super) async fn refuse(channel: &mut Channel, error: &str) -> Result<()> {
    let answer = Answer { ok: false, name: String::new(), error: error.to_owned() };
    channel.send(&serde_json::to_vec(&answer)?).await
}
