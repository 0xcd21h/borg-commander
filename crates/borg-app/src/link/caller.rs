//! One paired computer's calls while it is connected: each goes to the session it names, and the
//! mirrors, consoles and approval modes the computer takes are given back when it goes.

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Duration,
};

use anyhow::{Context, Result};
use borg_link::{Channel, KEY_LEN, PROOF_LEN, unhex};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::oneshot;

use super::{Computer, ROTATE_CALL, VIEWER_CALLS, served::Served};
use crate::{
    agent::{Call, Reply, Session},
    native_mirror, remote_copy,
    service::{Lease, Members, Place, START_CALL},
};

/// How long a session the service starts for a call has to register.
const START_WAIT: Duration = Duration::from_secs(15);

/// What a connection of a paired computer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    /// An agent's calls, a mirror's or a panel's.
    Session,
    /// The computer's borg, watching for the asks of a session its panels show. It makes only
    /// the viewer calls, and only it may make them.
    Viewer,
}

#[derive(Deserialize)]
struct Rotation {
    key: String,
    proof: String,
}

/// A paired computer while it is connected.
pub(super) struct Caller<'a> {
    served: &'a Served,
    role: Role,
    key: String,
    /// Who the calls come from. The name is only a label; approval and mirror leases bind to
    /// the paired key.
    origin: String,
    /// The other sessions on this machine that calls reached, by ID.
    sessions: HashMap<String, Session>,
    /// The sessions registered with the background service that calls reached, by ID.
    leases: HashMap<String, Lease>,
    transfer: remote_copy::Server,
    /// The session the file transfer belongs to, once one began.
    transfer_session: Option<String>,
    mirrors: HashMap<String, native_mirror::Peer>,
    mirror_leases: HashSet<String>,
    console_leases: HashSet<(String, String)>,
    approval_leases: HashSet<String>,
}

impl<'a> Caller<'a> {
    pub(super) fn new(served: &'a Served, computer: &Computer, role: Role) -> Self {
        Self {
            served,
            role,
            key: computer.key.clone(),
            origin: format!("{} [{}]", computer.name, computer.key.get(..32).unwrap_or(&computer.key)),
            sessions: HashMap::new(),
            leases: HashMap::new(),
            transfer: remote_copy::Server::default(),
            transfer_session: None,
            mirrors: HashMap::new(),
            mirror_leases: HashSet::new(),
            console_leases: HashSet::new(),
            approval_leases: HashSet::new(),
        }
    }

    /// Answers each call on `channel` in turn until the computer goes or is forgotten. A pushed
    /// listing goes on with its next page as a call of the server's own.
    pub(super) async fn serve(&mut self, channel: &mut Channel) -> Result<()> {
        let mut next_message = None;
        loop {
            let continuation = next_message.is_some();
            let message = match next_message.take() {
                Some(message) => message,
                None => match channel.recv().await? {
                    Some(message) => message,
                    None => break,
                },
            };
            // A computer forgotten while connected is cut off.
            if !self.served.shared.lock().expect("link lock").computers.iter().any(|c| c.key == self.key) {
                break;
            }
            let (reply, next) = match self.rotation(&message) {
                Some(rotation) => (self.rotate(channel, rotation), None),
                None => self.answer(&message, continuation).await?,
            };
            next_message = next;
            channel.send(&serde_json::to_vec(&reply)?).await?;
        }
        Ok(())
    }

    fn rotation(&self, message: &[u8]) -> Option<Rotation> {
        let call = serde_json::from_slice::<Call>(message).ok()?;
        (self.role == Role::Session && call.tool == ROTATE_CALL).then(|| serde_json::from_value(call.args).ok())?
    }

    fn rotate(&self, channel: &Channel, rotation: Rotation) -> Reply {
        let Some(new) = unhex::<KEY_LEN>(&rotation.key) else { return Reply::error("unreadable key") };
        let proven = unhex::<PROOF_LEN>(&rotation.proof)
            .is_some_and(|proof| channel.proves_rotation(&self.served.identity, &new, &proof));
        if !proven {
            return Reply::error("the proof for the new key does not hold");
        }
        match self.served.change_key(&self.key, &new) {
            Ok(_) => Reply::ok(""),
            Err(error) => Reply::error(format!("{error:#}")),
        }
    }

    /// The reply to one call, and the server's own call for the next page of a pushed listing.
    async fn answer(&mut self, message: &[u8], continuation: bool) -> Result<(Reply, Option<Vec<u8>>)> {
        let mut call = match serde_json::from_slice::<Call>(message).map(Call::external) {
            Ok(Ok(call)) => call,
            Ok(Err(refused)) => return Ok((refused, None)),
            Err(error) => return Ok((Reply::error(format!("unreadable call: {error}")), None)),
        };
        call.origin = Some(self.origin.clone());
        let viewing = VIEWER_CALLS.contains(&call.tool.as_str());
        if viewing != (self.role == Role::Viewer) {
            return Ok((Reply::error(format!("unknown tool {}", call.tool)), None));
        }
        call.viewer = viewing;
        // Only a server-created continuation inherits the first chunk's read approval.
        // External callers still cannot claim approval.
        if continuation {
            call.approved = true;
        }
        let push =
            (call.tool == "fs_list" && call.args["stream"] == true && call.args["push"] == true).then(|| call.clone());
        let approval = if call.tool == "approval_mode" { self.approval_target(&mut call) } else { None };
        let mut reply = self.dispatch(call).await?;
        if reply.ok
            && let Some((target, enabled)) = approval
        {
            if enabled {
                self.approval_leases.insert(target);
            } else {
                self.approval_leases.remove(&target);
            }
        }
        let next = match push {
            Some(request) if reply.ok => next_page(request, &mut reply)?,
            _ => None,
        };
        Ok((reply, next))
    }

    async fn dispatch(&mut self, call: Call) -> Result<Reply> {
        let tool = call.tool.clone();
        match tool.as_str() {
            "session_delete" | super::HOST_UPDATE_CALL => self.served.ask(call).await,
            "file_transfer" => Ok(self.file_transfer(call).await),
            "host_info" => Ok(self.served.host_info()),
            "list_sessions" => Ok(self.served.list_sessions()),
            tool if tool.starts_with("mirror_") => self.mirror(call).await,
            tool if tool.starts_with("console_") => self.console(call).await,
            _ => self.in_session(call).await,
        }
    }

    /// The session an `approval_mode` call turns on or off: the one it names, else the one
    /// the user worked in last, else this borg's own. The call is made to name it.
    fn approval_target(&self, call: &mut Call) -> Option<(String, bool)> {
        let requested = call.args["session"].as_str();
        let used = self.served.members.as_ref().and_then(Members::last_used);
        let target = self
            .served
            .named_sessions()
            .and_then(|entries| match requested {
                Some(name) => entries.into_iter().find(|entry| entry.id == name || entry.name == name),
                None => entries
                    .into_iter()
                    .filter(|entry| !entry.archived)
                    .max_by_key(|entry| (used.as_ref() == Some(&entry.id), entry.last_opened)),
            })
            .map(|entry| entry.id)
            .or_else(|| if requested.is_none() { self.served.session_id.clone() } else { None })?;
        call.args["session"] = json!(target);
        Some((target, call.args["enabled"] == true))
    }

    /// A piece of a file transfer, which stays with the active named session it began in.
    async fn file_transfer(&mut self, call: Call) -> Reply {
        let target = call.args["session"].as_str().map(str::to_owned);
        if !self.served.is_active(target.as_deref())
            || self.transfer_session.as_ref().is_some_and(|session| Some(session) != target.as_ref())
        {
            return Reply::error("No active named session for this transfer");
        }
        self.transfer_session = target;
        let (next, reply) = std::mem::take(&mut self.transfer).handle(call.args).await;
        self.transfer = next;
        reply
    }

    /// A mirror call. The background service answers it itself for the sessions it keeps
    /// running, starting one that runs nowhere; otherwise it goes to the named session's
    /// mirror, opened on first use. A mirror the computer takes is its lease.
    async fn mirror(&mut self, call: Call) -> Result<Reply> {
        let action = call.tool.clone();
        let target = call.args["session"].as_str().map(str::to_owned);
        let socket = match (&self.served.members, target.as_deref()) {
            (_, None) => return Ok(Reply::error("Mirror needs a session ID")),
            (Some(members), Some(target)) => match members.place(target) {
                Some((Place::Terminal, socket)) => Some(socket),
                Some((Place::Background, _)) => None,
                None => match self.start(members, target).await? {
                    Some(refused) => return Ok(refused),
                    None => None,
                },
            },
            (None, Some(target)) => Some(native_mirror::socket(&self.served.runtime_dir, target)?),
        };
        let reply = match (socket, target.as_deref()) {
            (Some(socket), Some(target)) => match self.mirror_call(target, &socket, &call).await {
                Ok(reply) => reply,
                Err(error) => {
                    self.mirrors.remove(target);
                    Reply::error(format!("{error:#}"))
                }
            },
            _ => self.served.ask(call).await?,
        };
        if reply.ok
            && let Some(target) = target
        {
            match action.as_str() {
                "mirror_take" => {
                    self.mirror_leases.insert(target);
                }
                "mirror_release" => {
                    self.mirror_leases.remove(&target);
                }
                _ => {}
            }
        }
        Ok(reply)
    }

    async fn mirror_call(&mut self, target: &str, socket: &Path, call: &Call) -> Result<Reply> {
        anyhow::ensure!(self.served.is_active(Some(target)), "No active named session for this mirror");
        if !self.mirrors.contains_key(target) {
            let peer = native_mirror::Peer::at(socket).await?;
            self.mirrors.insert(target.into(), peer);
        }
        self.mirrors.get_mut(target).context("Mirror session disconnected")?.call(call).await
    }

    /// A console call for an active named session. A console the computer opens is its lease.
    async fn console(&mut self, call: Call) -> Result<Reply> {
        if !self.served.is_active(call.args["session"].as_str()) {
            return Ok(Reply::error("No active named session for this console"));
        }
        let target = call.args["session"].as_str().unwrap_or_default().to_owned();
        let action = call.tool.clone();
        let id = call.args["id"].as_str().unwrap_or_default().to_owned();
        let reply = self.served.ask(call).await?;
        if reply.ok {
            match action.as_str() {
                "console_open" => {
                    if let Ok(value) = serde_json::from_str::<Value>(&reply.text)
                        && let Some(opened) = value["id"].as_str()
                    {
                        self.console_leases.insert((target, opened.to_owned()));
                    }
                }
                "console_close" => {
                    self.console_leases.remove(&(target, id));
                }
                _ => {}
            }
        }
        Ok(reply)
    }

    /// Any other call: to the named session it names on this machine, or to this borg. In the
    /// background service, a call that names no session goes to the one the user worked in last.
    async fn in_session(&mut self, mut call: Call) -> Result<Reply> {
        let target = call.args["session"].as_str().map(str::to_owned);
        if let Value::Object(args) = &mut call.args {
            args.remove("session");
        }
        let target = match (target, &self.served.members) {
            (Some(target), _) => target,
            (None, Some(members)) => members.last_used().unwrap_or_else(|| borg_core::DEFAULT_SESSION.to_owned()),
            (None, None) => return self.served.ask(call).await,
        };
        let found = self
            .served
            .named_sessions()
            .and_then(|entries| entries.into_iter().find(|entry| entry.id == target || entry.name == target));
        let Some(entry) = found else { return Ok(Reply::error(format!("no session named {target}"))) };
        if let Some(members) = self.served.members.clone() {
            return self.through_service(&members, &entry.id, call).await;
        }
        if self.served.session_id.as_deref() == Some(entry.id.as_str()) {
            return self.served.ask(call).await;
        }
        if !self.sessions.contains_key(&entry.id)
            && let Some(open) = Session::open(&self.served.runtime_dir, Some(&entry.id)).await
        {
            self.sessions.insert(entry.id.clone(), open);
        }
        call.forwarded_from = call.origin.clone();
        let reply = match self.sessions.get_mut(&entry.id) {
            Some(open) => open.call(&call).await,
            None => None,
        };
        if reply.is_none() {
            self.sessions.remove(&entry.id);
        }
        Ok(reply.unwrap_or_else(|| Reply::error(format!("session {} is offline or disconnected", entry.name))))
    }

    /// A call for a session registered with the background service, which the service starts
    /// in the background when it runs nowhere. The computer's connection counts as reaching
    /// the session from its first call there.
    async fn through_service(&mut self, members: &Members, id: &str, call: Call) -> Result<Reply> {
        if members.place(id).is_none()
            && let Some(refused) = self.start(members, id).await?
        {
            return Ok(refused);
        }
        if !self.leases.contains_key(id) {
            self.leases.insert(id.to_owned(), members.lease(id));
        }
        Ok(members.ask(id, call).await)
    }

    /// Has the service keep a session running in the background and waits for its Borg, or
    /// says why it cannot.
    async fn start(&self, members: &Members, id: &str) -> Result<Option<Reply>> {
        let reply = self.served.ask(Call::new(START_CALL, json!({ "session": id }))).await?;
        if !reply.ok {
            return Ok(Some(reply));
        }
        Ok((!members.joined(id, START_WAIT).await).then(|| Reply::error(format!("session {id} did not start in time"))))
    }

    /// Gives back what the computer held: closes its consoles, releases its mirrors and turns
    /// off the approval modes it turned on.
    pub(super) async fn release(mut self) {
        for (session, id) in std::mem::take(&mut self.console_leases) {
            let close = self.own_call("console_close", json!({ "session": session, "id": id }));
            let (sender, _) = oneshot::channel();
            let _ = self.served.calls.send((close, sender));
        }
        for id in std::mem::take(&mut self.mirror_leases) {
            let release = self.own_call("mirror_release", json!({ "session": id }));
            if let Some(peer) = self.mirrors.get_mut(&id) {
                let _ = tokio::time::timeout(Duration::from_secs(1), peer.call(&release)).await;
            } else if self.served.members.is_some() {
                let _ = self.served.ask(release).await;
            }
        }
        for id in std::mem::take(&mut self.approval_leases) {
            let mut args = json!({ "enabled": false });
            if !id.is_empty() {
                args["session"] = json!(id);
            }
            let release = self.own_call("approval_mode", args);
            if let Some(members) = &self.served.members {
                let _ = members.ask(&id, release).await;
            } else if id.is_empty() || self.served.session_id.as_deref() == Some(id.as_str()) {
                let _ = self.served.ask(release).await;
            } else if let Some(mut session) = Session::open(&self.served.runtime_dir, Some(&id)).await {
                let _ = session.call(&release).await;
            }
        }
    }

    fn own_call(&self, tool: &str, args: Value) -> Call {
        let mut call = Call::new(tool, args);
        call.origin = Some(self.origin.clone());
        call
    }
}

/// Marks a streamed listing's page as pushed, and makes the call for the page after it.
fn next_page(mut request: Call, reply: &mut Reply) -> Result<Option<Vec<u8>>> {
    let Ok(mut page) = serde_json::from_str::<Value>(&reply.text) else { return Ok(None) };
    if page["streaming"] != true {
        return Ok(None);
    }
    page["pushed"] = json!(true);
    let next = match page["cursor"].as_str() {
        Some(cursor) => {
            request.args["cursor"] = json!(cursor);
            request.args["limit"] = json!(512);
            Some(serde_json::to_vec(&request)?)
        }
        None => None,
    };
    reply.text = serde_json::to_string(&page)?;
    Ok(next)
}
