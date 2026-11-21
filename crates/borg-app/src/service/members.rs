//! The service's side of the Borgs registered with it: one for each running session, in a
//! terminal or in the background. Calls for a session go over its Borg's registration.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::{
        Notify,
        mpsc::{UnboundedSender, unbounded_channel},
        oneshot,
    },
};

use super::wire::{self, FromMember, Place, Registration, ToMember};
use crate::agent::{Call, Reply};

/// What a registered Borg asks of the service itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Request {
    Pair,
    Forget,
}

/// The registered Borgs, shared by the service and its borg cloud connections.
#[derive(Clone, Default)]
pub struct Members {
    roll: Arc<Mutex<Roll>>,
    joined: Arc<Notify>,
}

#[derive(Default)]
struct Roll {
    members: HashMap<String, Member>,
    /// Calls sent and not answered yet, by their number, with the registration each went to.
    pending: HashMap<u64, (u64, oneshot::Sender<Reply>)>,
    /// How many paired computers' connections reach each session.
    agents: HashMap<String, usize>,
    next_call: u64,
    next_member: u64,
    /// Counts up each time a session is used, so the highest is the one used last.
    clock: u64,
}

struct Member {
    token: u64,
    registration: Registration,
    sender: UnboundedSender<ToMember>,
    used: u64,
}

impl Roll {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn send(&self, session: &str, message: ToMember) {
        if let Some(member) = self.members.get(session) {
            let _ = member.sender.send(message);
        }
    }
}

/// A paired computer's connection reaching a session, counted while it lasts.
pub struct Lease {
    members: Members,
    session: String,
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.members.count_agents(&self.session, |count| count.saturating_sub(1));
    }
}

impl Members {
    fn roll(&self) -> std::sync::MutexGuard<'_, Roll> {
        self.roll.lock().expect("members lock")
    }

    /// Takes a Borg's registration, in place of an earlier one for its session.
    fn join(&self, registration: Registration, sender: UnboundedSender<ToMember>) -> u64 {
        let mut roll = self.roll();
        roll.next_member += 1;
        let token = roll.next_member;
        let used = roll.tick();
        let session = registration.session.clone();
        let agents = roll.agents.get(&session).copied().unwrap_or_default();
        let _ = sender.send(ToMember::Agents { count: agents });
        roll.members.insert(session, Member { token, registration, sender, used });
        drop(roll);
        self.joined.notify_waiters();
        token
    }

    /// Forgets a registration that ended, and fails the calls it left unanswered.
    fn leave(&self, session: &str, token: u64) {
        let mut roll = self.roll();
        if roll.members.get(session).is_some_and(|member| member.token == token) {
            roll.members.remove(session);
        }
        roll.pending.retain(|_, (member, _)| *member != token);
    }

    fn replied(&self, id: u64, reply: Reply) {
        if let Some((_, sender)) = self.roll().pending.remove(&id) {
            let _ = sender.send(reply);
        }
    }

    fn used(&self, session: &str) {
        let mut roll = self.roll();
        let now = roll.tick();
        if let Some(member) = roll.members.get_mut(session) {
            member.used = now;
        }
    }

    /// Where the session's Borg runs, and its mirror's socket, while it is registered.
    pub fn place(&self, session: &str) -> Option<(Place, PathBuf)> {
        self.roll().members.get(session).map(|member| (member.registration.place, member.registration.mirror.clone()))
    }

    /// The registered sessions and where each runs.
    pub fn sessions(&self) -> Vec<(String, Place)> {
        self.roll().members.iter().map(|(id, member)| (id.clone(), member.registration.place)).collect()
    }

    /// The session the user worked in last, of those registered.
    pub fn last_used(&self) -> Option<String> {
        self.roll().members.iter().max_by_key(|(_, member)| member.used).map(|(id, _)| id.clone())
    }

    pub(super) fn send(&self, session: &str, message: ToMember) {
        self.roll().send(session, message);
    }

    pub(super) fn broadcast(&self, message: impl Fn() -> ToMember) {
        for member in self.roll().members.values() {
            let _ = member.sender.send(message());
        }
    }

    /// Hands `call` to the session's Borg and waits for its reply.
    pub async fn ask(&self, session: &str, call: Call) -> Reply {
        let receiver = {
            let mut roll = self.roll();
            let Some((token, sender)) = roll.members.get(session).map(|member| (member.token, member.sender.clone()))
            else {
                return Reply::error(format!("session {session} is not running"));
            };
            roll.next_call += 1;
            let id = roll.next_call;
            let (reply, receiver) = oneshot::channel();
            roll.pending.insert(id, (token, reply));
            if sender.send(ToMember::Call { id, call }).is_err() {
                roll.pending.remove(&id);
                return Reply::error(format!("session {session} is not running"));
            }
            receiver
        };
        receiver.await.unwrap_or_else(|_| Reply::error("Borg closed before answering."))
    }

    /// Counts a paired computer's connection as reaching the session until the lease drops.
    pub fn lease(&self, session: &str) -> Lease {
        self.count_agents(session, |count| count + 1);
        Lease { members: self.clone(), session: session.to_owned() }
    }

    fn count_agents(&self, session: &str, change: impl Fn(usize) -> usize) {
        let mut roll = self.roll();
        let count = change(roll.agents.get(session).copied().unwrap_or_default());
        if count == 0 {
            roll.agents.remove(session);
        } else {
            roll.agents.insert(session.to_owned(), count);
        }
        roll.send(session, ToMember::Agents { count });
    }

    /// Whether the session's Borg registers within `wait`.
    pub async fn joined(&self, session: &str, wait: Duration) -> bool {
        tokio::time::timeout(wait, async {
            loop {
                let joined = self.joined.notified();
                if self.place(session).is_some() {
                    return;
                }
                joined.await;
            }
        })
        .await
        .is_ok()
    }
}

/// Serves a registered Borg until it goes: calls go out to it, and its replies and requests
/// come back. The Borg says nothing more until the service's first message, so nothing it
/// sent is left behind in the reader of its registration.
pub(super) async fn serve(
    stream: UnixStream,
    registration: Registration,
    members: Members,
    computers: Vec<String>,
    requests: UnboundedSender<(String, Request)>,
) {
    let session = registration.session.clone();
    let (sender, mut outgoing) = unbounded_channel();
    let _ = sender.send(ToMember::Computers { names: computers });
    if let Some(version) = newer_for(&registration, crate::update::VERSION) {
        let _ = sender.send(ToMember::Relaunch { version });
    }
    let token = members.join(registration, sender);
    let (read, mut write) = stream.into_split();
    let writing = tokio::spawn(async move {
        while let Some(message) = outgoing.recv().await {
            let Ok(line) = wire::line(&message) else { continue };
            if write.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    });
    let mut lines = BufReader::new(read).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        match serde_json::from_str::<FromMember>(&line) {
            Ok(FromMember::Reply { id, reply }) => members.replied(id, reply),
            Ok(FromMember::Active) => members.used(&session),
            Ok(FromMember::Pair) => {
                let _ = requests.send((session.clone(), Request::Pair));
            }
            Ok(FromMember::Forget) => {
                let _ = requests.send((session.clone(), Request::Forget));
            }
            Err(_) => break,
        }
    }
    writing.abort();
    members.leave(&session, token);
}

/// The version a registered Borg moves to: the service's, when the Borg runs an older one
/// and says so.
fn newer_for(registration: &Registration, current: &str) -> Option<String> {
    let theirs: borg_update::Version = registration.version.as_deref()?.parse().ok()?;
    let ours: borg_update::Version = current.parse().ok()?;
    (theirs < ours).then(|| current.to_owned())
}

#[cfg(test)]
mod tests;
