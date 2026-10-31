//! What an agent asks to change: files to write or upload, commands to run, text to type, and the user's answers.

use super::{App, Flow, fail, text};
use crate::agent::{self, Call, Location, Reply, Spot, Target, Work};
use crate::app::{DOCKER_SCRATCH, render::from_origin};
use anyhow::Result;
use borg_core::{AgentEvent, Ask, AskKind, DiffLine, Msg};
use borg_i18n::tr;
use tokio::sync::oneshot::Sender;

impl App {
    /// Works out a file's new text for a write or an edit, off the event loop.
    pub(super) fn prepare_change(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        let edit = call.tool == "edit";
        let Some(path) = text(&call, "path") else { return fail(reply, format!("{} needs a path", call.tool)) };
        let (content, old, new) = (call.args["content"].as_str(), call.args["old"].as_str(), call.args["new"].as_str());
        let change = match (edit, content, old, new) {
            (false, Some(content), ..) => Err(content.to_owned()),
            (true, _, Some(old), Some(new)) if !old.is_empty() => Ok((old.to_owned(), new.to_owned())),
            (false, ..) => return fail(reply, "write needs content".into()),
            (true, ..) => return fail(reply, "edit needs old (not empty) and new".into()),
        };
        let all = call.args["all"] == true;
        let location = agent::locate(&self.state, &path);
        let Some((spot, _, reply)) = self.spot(&location, &call, reply) else { return Ok(Flow::Continue) };
        let requeue = self.agent.requeue_sender();
        tokio::spawn(async move {
            let uri = spot.uri();
            let before = match agent::load(&spot).await {
                Ok(before) => before,
                Err(error) => return drop(reply.send(Reply::error(format!("{uri}: {error:#}")))),
            };
            let after = match (change, &before) {
                (Err(content), _) => content,
                (Ok(_), None) => {
                    return drop(reply.send(Reply::error(format!("{uri} does not exist; write creates files"))));
                }
                (Ok((old, new)), Some(text)) => match text.matches(old.as_str()).count() {
                    0 => {
                        return drop(reply.send(Reply::error(format!(
                            "old was not found in {uri}: it must match exactly, spaces and line breaks included. Read the lines again."
                        ))));
                    }
                    count if count > 1 && !all => {
                        return drop(reply.send(Reply::error(format!(
                            "old matches {count} places in {uri}: include more of the lines around it, or set all"
                        ))));
                    }
                    _ if all => text.replace(old.as_str(), &new),
                    _ => text.replacen(old.as_str(), &new, 1),
                },
            };
            let args = serde_json::json!({ "spot": spot, "before": before, "after": after });
            drop(requeue.send((call.follow("_propose", args), reply)));
        });
        Ok(Flow::Continue)
    }

    /// Asks the user to approve a change, showing what it changes.
    pub(super) fn propose_change(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        let (Ok(spot), Some(after)) =
            (serde_json::from_value::<Spot>(call.args["spot"].clone()), call.args["after"].as_str())
        else {
            return fail(reply, "unreadable change".into());
        };
        let (before, after) = (call.args["before"].as_str().map(str::to_owned), after.to_owned());
        let body = match &before {
            Some(old) => agent::diff(old, &after),
            None => agent::whole(&after),
        };
        if before.is_some() && body.is_empty() {
            drop(reply.send(Reply::ok(format!("{} already holds that; nothing changed", spot.uri()))));
            return Ok(Flow::Continue);
        }
        let (summary, place) = match &spot {
            Spot::Host(path) => (agent::home_relative(path), String::new()),
            Spot::Container { name, path, .. } => (path.display().to_string(), tr!(agent_container_place, name = name)),
        };
        let kind = if before.is_some() { AskKind::Edit } else { AskKind::Create };
        let place = from_origin(place, call.origin.as_deref());
        let ask = Ask { kind, summary, place, body };
        let id = self.agent.ask(reply, Work::Write { spot, before, after });
        if call.origin.as_ref().is_some_and(|origin| self.approve_all.contains(origin)) {
            return self.agent_answer(id, true);
        }
        self.dispatch(Msg::Agent(AgentEvent::Ask { id, ask }))
    }

    /// Asks the user to approve running a command on the host or in a container.
    pub(super) fn ask_to_run(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        let Some(command) = text(&call, "command") else { return fail(reply, "run needs a command".into()) };
        let at = text(&call, "at");
        let mut reply = reply;
        let target = match at.as_deref().map(|at| agent::locate(&self.state, at)) {
            None => Target::Host(agent::host_dir(&self.state)),
            Some(Location::Host(dir)) => Target::Host(dir),
            Some(location) => {
                let Some((spot, running, back)) = self.spot(&location, &call, reply) else {
                    return Ok(Flow::Continue);
                };
                reply = back;
                let Spot::Container { id, name, path } = spot else {
                    return fail(reply, "unreadable location".into());
                };
                if !running {
                    return fail(reply, format!("{name} is not running."));
                }
                if borg_core::container_shell_busy(&self.state, &id) {
                    return fail(
                        reply,
                        format!("The shell in {name} is running something; try again when it is back at its prompt."),
                    );
                }
                // A folder runs the command there; the container alone keeps the shell's.
                let named = at
                    .as_deref()
                    .and_then(|at| at.trim().strip_prefix(agent::DOCKER_SCHEME))
                    .is_some_and(|rest| rest.contains('/'));
                Target::Container { id, name, dir: named.then_some(path) }
            }
        };
        if matches!(target, Target::Host(_))
            && self.console == borg_core::Console::Host
            && self.state.shell.integrated
            && !borg_core::host_console_ready(&self.state)
        {
            return fail(
                reply,
                "Borg's command line is busy: the user is typing there, or a program runs. Try again when it is back at its prompt.".into(),
            );
        }
        let ask = Ask {
            kind: AskKind::Run,
            summary: command.clone(),
            place: from_origin(agent::place(&target), call.origin.as_deref()),
            body: command.lines().map(DiffLine::plain).collect(),
        };
        let id = self.agent.ask(reply, Work::Run { command, target });
        if call.origin.as_ref().is_some_and(|origin| self.approve_all.contains(origin)) {
            return self.agent_answer(id, true);
        }
        self.dispatch(Msg::Agent(AgentEvent::Ask { id, ask }))
    }

    /// Types text at the console's cursor, for the user to review and run.
    pub(super) fn type_text(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        let text = agent::typable(call.args["text"].as_str().unwrap_or_default());
        if text.trim().is_empty() {
            return fail(reply, "type needs text".into());
        }
        // At a prompt without paste support, a tab would complete; it types as a space.
        let pastes = self.state.shell.observed.mode.contains(borg_term::TermMode::BRACKETED_PASTE);
        let text = if pastes { text } else { text.replace('\t', " ") };
        if let Some(reason) = borg_core::typing_refusal(&self.state, &text) {
            return fail(reply, reason.into());
        }
        self.agent.note(&format!("typed into the console\n{text}"));
        let flow = self.dispatch(Msg::Agent(AgentEvent::Paste { text }))?;
        drop(reply.send(Reply::ok(
            "Typed at the cursor in Borg's console; nothing ran. The user reviews it and runs or saves it.",
        )));
        Ok(flow)
    }

    /// An upload from another computer: the first call announces the file and waits for the
    /// user's approval; the calls after it bring its pieces, the last one marked done.
    pub(super) fn put(
        &mut self,
        call: crate::agent::Call,
        reply: tokio::sync::oneshot::Sender<crate::agent::Reply>,
    ) -> Result<Flow> {
        use crate::agent::{self, Reply, Work};
        use base64::Engine as _;
        use borg_core::{AgentEvent, Ask, AskKind, DiffLine};
        let Some(token) = call.args["token"].as_str().map(str::to_owned) else {
            let (Some(path), Some(size)) = (call.args["path"].as_str(), call.args["size"].as_u64()) else {
                drop(reply.send(Reply::error("put needs a path and a size")));
                return Ok(Flow::Continue);
            };
            let location = agent::locate(&self.state, path);
            let Some((spot, _, reply)) = self.spot(&location, &call, reply) else { return Ok(Flow::Continue) };
            // Small sizes show as bytes; larger ones as the panels show them, with the bytes.
            let shown = if size < 10_000 {
                format!("{size} bytes")
            } else {
                format!("{} ({size} bytes)", borg_ui::size_text(size))
            };
            let place = from_origin(shown.clone(), call.origin.as_deref());
            let body = vec![DiffLine::plain(spot.uri()), DiffLine::plain(shown)];
            let ask = Ask { kind: AskKind::Upload, summary: spot.uri(), place, body };
            let id = self.agent.ask(reply, Work::Upload { spot, size });
            if call.origin.as_ref().is_some_and(|origin| self.approve_all.contains(origin)) {
                return self.agent_answer(id, true);
            }
            return self.dispatch(Msg::Agent(AgentEvent::Ask { id, ask }));
        };
        let Some(upload) = self.uploads.get_mut(&token) else {
            drop(reply.send(Reply::error("no such upload; start it again")));
            return Ok(Flow::Continue);
        };
        let data = base64::engine::general_purpose::STANDARD.decode(call.args["data"].as_str().unwrap_or_default());
        let written = data.map_err(anyhow::Error::from).and_then(|data| upload.write(&data));
        if let Err(error) = written {
            self.uploads.remove(&token);
            drop(reply.send(Reply::error(format!("{error:#}"))));
            return Ok(Flow::Continue);
        }
        if call.args["done"] != true {
            drop(reply.send(Reply::ok("")));
            return Ok(Flow::Continue);
        }
        let Some(upload) = self.uploads.remove(&token) else { return Ok(Flow::Continue) };
        let uri = upload.spot.uri();
        self.agent.note(&format!("uploaded {uri}"));
        let messages = self.messages.clone();
        tokio::spawn(async move {
            let finished = upload.finish().await;
            drop(reply.send(match finished {
                Ok(size) => Reply::ok(format!("Uploaded {uri} ({size} bytes)")),
                Err(error) => Reply::error(format!("{uri}: {error:#}")),
            }));
            drop(messages.send(Msg::Agent(AgentEvent::Finished { id: 0, notice: tr!(agent_wrote, path = uri) })));
        });
        Ok(Flow::Continue)
    }

    /// Carries out an approved ask: a command typed into the host console (or run in the
    /// background where that console cannot take it) or into a container's shell, which the
    /// Docker view then shows, or a change written to a file.
    pub(in crate::app) fn agent_answer(&mut self, id: u64, approved: bool) -> Result<Flow> {
        use crate::agent::{Approved, Target, Typed, Work};
        let Some(Approved { id, reply, work }) = self.agent.answer(id, approved) else { return Ok(Flow::Continue) };
        let (command, target) = match work {
            Work::Write { spot, before, after } => {
                self.agent.write(id, reply, spot, before, after);
                return Ok(Flow::Continue);
            }
            Work::Pair { .. } => {
                drop(reply.send(crate::agent::Reply::ok("approved")));
                return Ok(Flow::Continue);
            }
            Work::ApprovalMode { origin } => {
                self.approve_all.insert(origin.clone());
                self.state.agent.auto_approve.push(origin.clone());
                self.agent.note(&format!("Approve all on for {origin}"));
                drop(reply.send(crate::agent::Reply::ok(format!("Approve all on for {origin}"))));
                return Ok(Flow::Continue);
            }
            Work::Forward(mut call) => {
                if call.tool == "get"
                    && let (Some(origin), Some(path)) = (call.origin.clone(), call.args["path"].as_str())
                {
                    self.downloads.insert((origin, path.to_owned()));
                }
                call.approved = true;
                drop(self.agent.requeue_sender().send((call, reply)));
                return Ok(Flow::Continue);
            }
            Work::Upload { spot, size } => {
                let token = borg_link::hex(&borg_link::secret());
                let scratch = self.runtime_dir.join(DOCKER_SCRATCH);
                drop(reply.send(match crate::agent::Upload::begin(spot, size, &scratch, &token) {
                    Ok(upload) => {
                        self.uploads.insert(token.clone(), upload);
                        crate::agent::Reply::ok(token)
                    }
                    Err(error) => crate::agent::Reply::error(format!("{error:#}")),
                }));
                return Ok(Flow::Continue);
            }
            Work::Run { command, target } => (command, target),
        };
        let place = crate::agent::place(&target);
        match target {
            Target::Host(dir) if self.console == borg_core::Console::Host && self.state.shell.integrated => {
                let line = if dir == self.state.shell.cwd {
                    command.clone()
                } else {
                    format!("cd -- {} && {command}", borg_core::shell_quote(&dir.to_string_lossy()))
                };
                let typed = Typed { id, reply, line: line.clone(), command, place };
                if !borg_core::host_console_ready(&self.state) {
                    typed.fail(
                        "Borg's command line became busy before the command could run; try again.",
                        &self.messages,
                    );
                    return Ok(Flow::Continue);
                }
                self.host_capture = Some((typed, Vec::new()));
                self.dispatch(Msg::Agent(borg_core::AgentEvent::Type { line }))
            }
            Target::Host(dir) => {
                self.agent.run_in_background(id, reply, command, dir);
                Ok(Flow::Continue)
            }
            Target::Container { id: container, name, dir } => {
                let line = match dir {
                    Some(dir) => format!("cd -- {} && {command}", borg_core::shell_quote(&dir.to_string_lossy())),
                    None => command.clone(),
                };
                let typed = Typed { id, reply, line, command, place };
                let flow =
                    self.dispatch(Msg::Agent(borg_core::AgentEvent::ShowShell { container: container.clone(), name }))?;
                if let Err(typed) = self.shells.run(&container, typed) {
                    typed.fail("Borg could not open a shell in that container.", &self.messages);
                }
                Ok(flow)
            }
        }
    }
}
