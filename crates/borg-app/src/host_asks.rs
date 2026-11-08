//! The asks of the host sessions the panels show. For each, borg keeps a viewer connection to the
//! host's borg that hears what waits there, and carries the user's answers back on a viewer
//! connection of their own, so an answer never waits behind the call that listens.

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use borg_core::{AskFeed, HostAnswer, HostEvent, Msg, Refusal};
use serde_json::json;
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    task::JoinHandle,
};

use crate::{
    agent::Call,
    link::{self, ASKS_ANSWER, ASKS_STOP, ASKS_WAIT, ASKS_WAIT_TIME, Remote},
    session::StateDir,
};

/// How long opening a viewer connection may take.
const OPEN_WAIT: Duration = Duration::from_secs(10);
/// How long past its own wait the host may take to answer an `asks_wait` call.
const REPLY_SLACK: Duration = Duration::from_secs(10);
/// How long the host may take to take an answer or turn off Approve all.
const ANSWER_WAIT: Duration = Duration::from_secs(10);
/// Waits between attempts to reach the host again.
const RETRY_FIRST: Duration = Duration::from_secs(2);
const RETRY_LAST: Duration = Duration::from_secs(60);

/// A host session by its machine and session ID, as a panel mounts it.
type Key = (String, String);

enum Command {
    Answer(HostAnswer),
    Stop,
}

struct Watcher {
    commands: UnboundedSender<Command>,
    task: JoinHandle<()>,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The host sessions watched for asks, one task each.
pub struct Watchers {
    messages: UnboundedSender<Msg>,
    watched: HashMap<Key, Watcher>,
}

impl Watchers {
    pub fn new(messages: UnboundedSender<Msg>) -> Self {
        Self { messages, watched: HashMap::new() }
    }

    /// Watches the host sessions `shown` names, and stops watching the others.
    pub fn sync(&mut self, shown: impl IntoIterator<Item = Key>) {
        let shown: HashSet<Key> = shown.into_iter().collect();
        let gone: Vec<Key> = self.watched.keys().filter(|key| !shown.contains(*key)).cloned().collect();
        for key in gone {
            self.watched.remove(&key);
            self.tell(&key, HostEvent::Unwatched);
        }
        for key in shown {
            if self.watched.contains_key(&key) {
                continue;
            }
            let (commands, received) = unbounded_channel();
            let task = tokio::spawn(watch(key.clone(), received, self.messages.clone()));
            self.watched.insert(key, Watcher { commands, task });
        }
    }

    /// Sends the user's answer to an ask of a watched host session.
    pub fn answer(&self, machine: String, session: String, answer: HostAnswer) {
        let id = answer.id;
        self.command((machine, session), Command::Answer(answer), HostEvent::Unreached(id));
    }

    /// Turns off Approve all in a watched host session.
    pub fn stop(&self, machine: String, session: String) {
        self.command((machine, session), Command::Stop, HostEvent::ApproveAllStillOn);
    }

    fn command(&self, key: Key, command: Command, failed: HostEvent) {
        let sent = self.watched.get(&key).is_some_and(|watcher| watcher.commands.send(command).is_ok());
        if !sent {
            self.tell(&key, failed);
        }
    }

    fn tell(&self, key: &Key, event: HostEvent) {
        tell(&self.messages, key, event);
    }
}

fn tell(messages: &UnboundedSender<Msg>, key: &Key, event: HostEvent) {
    let _ = messages.send(Msg::HostAsks { machine: key.0.clone(), session: key.1.clone(), event });
}

/// Listens to one host session until no panel shows it, opening the connection again whenever
/// it drops. What the user answers while it is down does not reach the host and says so.
async fn watch(key: Key, mut commands: UnboundedReceiver<Command>, messages: UnboundedSender<Msg>) {
    let mut retry = RETRY_FIRST;
    loop {
        let mut heard = false;
        let _ = listen(&key, &mut commands, &messages, &mut heard).await;
        tell(&messages, &key, HostEvent::Lost);
        if heard {
            retry = RETRY_FIRST;
        }
        let pause = tokio::time::sleep(retry);
        tokio::pin!(pause);
        loop {
            tokio::select! {
                () = &mut pause => break,
                command = commands.recv() => match command {
                    Some(command) => tell(&messages, &key, unsent(&command)),
                    None => return,
                },
            }
        }
        retry = (retry * 2).min(RETRY_LAST);
    }
}

/// What the user hears when `command` could not be sent.
fn unsent(command: &Command) -> HostEvent {
    match command {
        Command::Answer(answer) => HostEvent::Unreached(answer.id),
        Command::Stop => HostEvent::ApproveAllStillOn,
    }
}

/// One connection's life: what waits in the session as it changes, and the user's answers,
/// until either fails.
async fn listen(
    key: &Key,
    commands: &mut UnboundedReceiver<Command>,
    messages: &UnboundedSender<Msg>,
    heard: &mut bool,
) -> Result<()> {
    let state = StateDir::locate().context("Borg has no state folder")?;
    let server = link::server(state.path(), &key.0)?;
    let mut reader = tokio::time::timeout(OPEN_WAIT, Remote::open_viewer(state.path(), &server)).await??;
    let listening = async {
        let mut version = 0;
        loop {
            let call = Call::new(ASKS_WAIT, json!({ "session": key.1, "after": version }));
            let reply = tokio::time::timeout(ASKS_WAIT_TIME + REPLY_SLACK, reader.call(&call)).await??;
            if !reply.ok {
                bail!("{}", reply.text);
            }
            let feed: AskFeed = serde_json::from_str(&reply.text)?;
            *heard = true;
            if feed.version != version {
                version = feed.version;
                tell(messages, key, HostEvent::Feed(feed));
            }
        }
    };
    let answering = async {
        while let Some(command) = commands.recv().await {
            let event = match carry_out(state.path(), &server, &key.1, &command).await {
                Ok(event) => event,
                Err(_) => Some(unsent(&command)),
            };
            if let Some(event) = event {
                tell(messages, key, event);
            }
        }
        Ok(())
    };
    tokio::select! {
        result = listening => result,
        result = answering => result,
    }
}

/// Sends one of the user's commands to the host, and what the user hears of how it went.
async fn carry_out(state: &Path, server: &link::Server, session: &str, command: &Command) -> Result<Option<HostEvent>> {
    tokio::time::timeout(ANSWER_WAIT, async {
        let mut writer = Remote::open_viewer(state, server).await?;
        match command {
            Command::Answer(answer) => {
                let reply =
                    writer.call(&Call::new(ASKS_ANSWER, json!({ "session": session, "answer": answer }))).await?;
                if !reply.ok {
                    bail!("{}", reply.text);
                }
                let refused: Option<Refusal> = serde_json::from_str(&reply.text)?;
                Ok(refused.map(HostEvent::Refused))
            }
            Command::Stop => {
                let reply = writer.call(&Call::new(ASKS_STOP, json!({ "session": session }))).await?;
                if !reply.ok {
                    bail!("{}", reply.text);
                }
                Ok(Some(HostEvent::ApproveAllOff))
            }
        }
    })
    .await?
}
