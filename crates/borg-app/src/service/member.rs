//! A Borg's side of its registration with the service: it keeps registering while the service
//! runs, answers the calls the service brings, and passes on the user's requests for borg cloud.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use borg_core::{AgentEvent, Msg};
use serde::Serialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::{
        mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
        oneshot, watch,
    },
    task::JoinHandle,
};

use super::{
    service_socket,
    wire::{self, FromMember, Place, REGISTER, Registration, ToMember},
};
use crate::agent::{Call, Reply};

/// How often a Borg tries to register while no service answers.
const RETRY: Duration = Duration::from_secs(2);
/// How often at most the service hears that the user works in this session.
const ACTIVE_EVERY: Duration = Duration::from_secs(2);

#[derive(Serialize)]
struct Request<'a> {
    mode: &'a str,
    #[serde(flatten)]
    registration: &'a Registration,
}

/// Where a registered Borg's calls and news go in the Borg.
#[derive(Clone)]
pub struct Inbox {
    pub calls: UnboundedSender<(Call, oneshot::Sender<Reply>)>,
    pub messages: UnboundedSender<Msg>,
    /// The agents connected to this Borg, which the service's paired computers add to.
    pub clients: Arc<AtomicUsize>,
}

/// This Borg's registration with the service, kept for as long as the Borg runs.
pub struct Membership {
    registered: watch::Receiver<bool>,
    requests: UnboundedSender<FromMember>,
    last_active: Option<Instant>,
    task: JoinHandle<()>,
}

impl Membership {
    pub fn start(session: &str, mirror: PathBuf, place: Place, inbox: Inbox) -> Self {
        let registration = Registration { session: session.to_owned(), pid: std::process::id(), mirror, place };
        Self::start_at(service_socket(), registration, inbox)
    }

    fn start_at(socket: PathBuf, registration: Registration, inbox: Inbox) -> Self {
        let (registered, watching) = watch::channel(false);
        let (requests, pending) = unbounded_channel();
        let task = tokio::spawn(keep_registering(socket, registration, inbox, registered, pending));
        Self { registered: watching, requests, last_active: None, task }
    }

    /// Whether the service has this Borg's registration now, and with it borg cloud.
    pub fn registered(&self) -> bool {
        *self.registered.borrow()
    }

    /// Asks the service for a pairing link, which comes back as the Connect dialog's.
    pub fn pair(&self) {
        let _ = self.requests.send(FromMember::Pair);
    }

    pub fn forget(&self) {
        let _ = self.requests.send(FromMember::Forget);
    }

    /// Tells the service the user works here, now and then.
    pub fn active(&mut self, now: Instant) {
        if self.last_active.is_some_and(|last| now.duration_since(last) < ACTIVE_EVERY) {
            return;
        }
        self.last_active = Some(now);
        let _ = self.requests.send(FromMember::Active);
    }
}

impl Drop for Membership {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn keep_registering(
    socket: PathBuf,
    registration: Registration,
    inbox: Inbox,
    registered: watch::Sender<bool>,
    mut requests: UnboundedReceiver<FromMember>,
) {
    loop {
        if let Ok(stream) = UnixStream::connect(&socket).await {
            while requests.try_recv().is_ok() {}
            let _ = serve(stream, &registration, &inbox, &registered, &mut requests).await;
            registered.send_replace(false);
        }
        tokio::time::sleep(RETRY).await;
    }
}

/// Registers on `stream` and serves the service until it goes.
async fn serve(
    stream: UnixStream,
    registration: &Registration,
    inbox: &Inbox,
    registered: &watch::Sender<bool>,
    requests: &mut UnboundedReceiver<FromMember>,
) -> anyhow::Result<()> {
    let (read, mut write) = stream.into_split();
    write.write_all(wire::line(&Request { mode: REGISTER, registration })?.as_bytes()).await?;
    let mut lines = BufReader::new(read).lines();
    let (replies, mut answered) = unbounded_channel();
    let mut agents = Agents { inbox: inbox.clone(), count: 0 };
    let mut first = true;
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else { break };
                let message: ToMember = serde_json::from_str(&line)?;
                if std::mem::take(&mut first) {
                    registered.send_replace(true);
                }
                take(message, inbox, &replies, &mut agents);
            }
            Some(reply) = answered.recv() => write.write_all(wire::line(&reply)?.as_bytes()).await?,
            Some(request) = requests.recv(), if !first => write.write_all(wire::line(&request)?.as_bytes()).await?,
        }
    }
    Ok(())
}

/// What the service said: a call goes to the Borg and its reply back, news goes to the screen.
fn take(message: ToMember, inbox: &Inbox, replies: &UnboundedSender<FromMember>, agents: &mut Agents) {
    match message {
        ToMember::Call { id, call } => {
            let (reply, answer) = oneshot::channel();
            if inbox.calls.send((call, reply)).is_err() {
                return;
            }
            let replies = replies.clone();
            tokio::spawn(async move {
                let reply = answer.await.unwrap_or_else(|_| Reply::error("Borg closed before answering."));
                let _ = replies.send(FromMember::Reply { id, reply });
            });
        }
        ToMember::Link { link } => {
            let _ = inbox.messages.send(Msg::Agent(AgentEvent::Pairing(link)));
        }
        ToMember::Computers { names } => {
            let _ = inbox.messages.send(Msg::Agent(AgentEvent::Paired { computers: names }));
        }
        ToMember::Agents { count } => agents.set(count),
    }
}

/// The paired computers' connections the service counts for this session, added to the
/// Borg's agents while registered.
struct Agents {
    inbox: Inbox,
    count: usize,
}

impl Agents {
    fn set(&mut self, count: usize) {
        let before = std::mem::replace(&mut self.count, count);
        let total = if count >= before {
            self.inbox.clients.fetch_add(count - before, Ordering::SeqCst) + (count - before)
        } else {
            self.inbox.clients.fetch_sub(before - count, Ordering::SeqCst) - (before - count)
        };
        if count != before {
            let _ = self.inbox.messages.send(Msg::Agent(AgentEvent::Connected { clients: total }));
        }
    }
}

impl Drop for Agents {
    fn drop(&mut self) {
        self.set(0);
    }
}

#[cfg(test)]
mod tests;
