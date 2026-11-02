//! Carrying out what a connected agent asks: its tool calls, edits, searches and answers.

use super::{App, Flow};
use crate::agent::{self, Call, Reply, Spot, Work};
use crate::session::StateDir;
use anyhow::Result;
use borg_core::{AgentEvent, Ask, AskKind, DiffLine, Msg};
use borg_i18n::tr;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};
use tokio::sync::oneshot::Sender;
mod changes;

/// A tool call's text argument, unless it is missing or blank.
fn text(call: &Call, key: &str) -> Option<String> {
    call.args[key].as_str().map(str::to_owned).filter(|value| !value.trim().is_empty())
}

/// Answers a tool call with an error, and carries on.
fn fail(reply: Sender<Reply>, text: String) -> Result<Flow> {
    drop(reply.send(Reply::error(text)));
    Ok(Flow::Continue)
}

impl App {
    /// Answers an agent's tool call from what the session knows. Reading happens off the
    /// event loop, a reveal answers once the panel shows it, and commands and changes to
    /// files wait for the user's approval.
    pub(super) fn on_agent(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        // With the user's say-so, what another computer reads waits for approval too: once per
        // file for a download, which comes in pieces.
        if let Some(origin) = call.origin.clone()
            && !call.approved
            && self.state.settings.preferences.agent_ask_reads
            && matches!(call.tool.as_str(), "screen" | "read" | "get" | "fs_list")
            && !(call.tool == "get"
                && self.downloads.contains(&(origin.clone(), text(&call, "path").unwrap_or_default())))
        {
            let summary = match call.tool.as_str() {
                "screen" => "what borg shows".to_owned(),
                "get" => format!("download {}", text(&call, "path").unwrap_or_default()),
                _ => text(&call, "source").or_else(|| text(&call, "path")).unwrap_or_else(|| ".".into()),
            };
            let place = tr!(agent_asked_from).replace("{origin}", &origin);
            let ask =
                Ask { kind: AskKind::Read, summary: summary.clone(), place, body: vec![DiffLine::plain(summary)] };
            let id = self.agent.ask(reply, Work::Forward(call));
            return self.dispatch(Msg::Agent(AgentEvent::Ask { id, ask }));
        }
        match call.tool.as_str() {
            "session_stop" => self.stop_session(call, reply),
            "session_delete" => self.delete_session(call, reply),
            tool if tool.starts_with("console_") => {
                self.console_server.handle(call, reply);
                Ok(Flow::Continue)
            }
            "fs_list" => self.list_folder(call, reply),
            "approval_mode" => self.set_approval_mode(call, reply),
            crate::link::PAIR_CALL => self.ask_to_pair(call, reply),
            "get" => self.send_piece(call, reply),
            "put" => self.put(call, reply),
            "screen" => self.show_screen(reply),
            "search_memory" => self.search_memory(call, reply),
            "read" => self.read_for_agent(call, reply),
            "show" => self.show_spot(call, reply),
            "_show" => self.reveal_spot(call, reply),
            "write" | "edit" => self.prepare_change(call, reply),
            "_propose" => self.propose_change(call, reply),
            "run" => self.ask_to_run(call, reply),
            "type" => self.type_text(call, reply),
            other => fail(reply, format!("unknown tool {other}")),
        }
    }

    /// Stops this session when it is one pending removal, cancelling its jobs.
    fn stop_session(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        let Some(id) = text(&call, "id") else { return fail(reply, "session_stop needs an ID".into()) };
        let allowed = self.link.session_id() == Some(id.as_str())
            && id != "default"
            && StateDir::locate().is_some_and(|state| {
                crate::sessions::list(&state)
                    .is_ok_and(|sessions| sessions.iter().any(|session| session.id == id && session.archived))
            });
        if !allowed {
            return fail(reply, "Only a session pending removal can be stopped this way".into());
        }
        for control in self.jobs.values() {
            control.cancel();
        }
        let _ = reply.send(Reply::ok("Session stopping"));
        Ok(Flow::Quit)
    }

    /// Removes a session from this computer, off the event loop.
    fn delete_session(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        let Some(id) = text(&call, "id") else { return fail(reply, "session_delete needs an ID".into()) };
        let Some(state) = StateDir::locate() else { return fail(reply, "Borg has no state folder".into()) };
        let runtime = self.runtime_dir.clone();
        let serving = self.link.session_id().map(str::to_owned);
        let consoles = self.console_server.clone();
        tokio::spawn(async move {
            let result = crate::sessions::delete_remote(&state, &runtime, &id, serving.as_deref()).await;
            let answer = match result {
                Ok(()) => {
                    consoles.close_session(&id);
                    Reply::ok("Session removed")
                }
                Err(error) => Reply::error(format!("{error:#}")),
            };
            let _ = reply.send(answer);
        });
        Ok(Flow::Continue)
    }

    /// A page of a folder's listing, for another computer browsing this one.
    fn list_folder(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        let Some(path) = text(&call, "path").map(PathBuf::from).filter(|path| path.is_absolute()) else {
            return fail(reply, "fs_list needs an absolute path".into());
        };
        let offset = call.args["offset"].as_u64().unwrap_or(0).min(usize::MAX as u64) as usize;
        let streaming = call.args["stream"] == true;
        let cursor = text(&call, "cursor");
        let owner = call.origin.clone();
        let limit = call.args["limit"].as_u64().unwrap_or(128).min(1024) as usize;
        let listings = self.remote_listings.clone();
        tokio::task::spawn_blocking(move || {
            if streaming {
                let answer = match listings.page(path, owner, cursor, limit) {
                    Ok(page) => Reply::ok(serde_json::to_string(&page).expect("directory page")),
                    Err(error) => Reply::error(error.to_string()),
                };
                drop(reply.send(answer));
                return;
            }
            let answer = match crate::remote_fs::legacy_page(&path, offset) {
                Ok(page) => Reply::ok(serde_json::to_string(&page).expect("directory page")),
                Err(error) => Reply::error(format!("{}: {error}", path.display())),
            };
            drop(reply.send(answer));
        });
        Ok(Flow::Continue)
    }

    /// Turns off approving everything a paired computer asks, or asks the user to turn it on.
    fn set_approval_mode(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        let Some(origin) = call.origin.clone() else {
            return fail(reply, "Approve all applies only to a paired computer".into());
        };
        let Some(enabled) = call.args["enabled"].as_bool() else {
            return fail(reply, "approval_mode needs enabled: true or false".into());
        };
        if !enabled {
            self.approve_all.remove(&origin);
            self.state.agent.auto_approve.retain(|name| name != &origin);
            self.agent.note(&format!("Approve all off for {origin}"));
            drop(reply.send(Reply::ok(format!("Approve all off for {origin}"))));
            return Ok(Flow::Continue);
        }
        if self.approve_all.contains(&origin) {
            drop(reply.send(Reply::ok(format!("Approve all is already on for {origin}"))));
            return Ok(Flow::Continue);
        }
        let ask = Ask {
            kind: AskKind::ApprovalMode,
            summary: origin.clone(),
            place: format!("session: {}", self.state.agent.connect.name),
            body: vec![
                DiffLine::plain(format!("Allow {origin} to run commands and change files without individual prompts?")),
                DiffLine::plain("Pairing and trust changes still ask. This ends when its connection closes."),
            ],
        };
        let id = self.agent.ask(reply, Work::ApprovalMode { origin });
        self.dispatch(Msg::Agent(AgentEvent::Ask { id, ask }))
    }

    /// Asks the user whether to pair with a computer showing the same code.
    fn ask_to_pair(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        let name = text(&call, "name").unwrap_or_default();
        // Emoji only where the terminal draws them as wide as they are laid out.
        let code = call.args[if self.wide_emoji { "code" } else { "names" }].as_array().cloned().unwrap_or_default();
        let mut body = vec![DiffLine::plain(format!("Computer: {name}")), DiffLine::plain("")];
        body.extend(code.iter().filter_map(|row| row.as_str()).map(|row| DiffLine::plain(format!("  {row}"))));
        body.extend([
            DiffLine::plain(""),
            DiffLine::plain("Pair only if `borg pair` shows this code on that computer."),
        ]);
        let ask = Ask { kind: AskKind::Pair, summary: name.clone(), place: String::new(), body };
        let id = self.agent.ask(reply, Work::Pair { name });
        self.dispatch(Msg::Agent(AgentEvent::Ask { id, ask }))
    }

    /// A piece of a file to download.
    fn send_piece(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        let Some(path) = text(&call, "path") else { return fail(reply, "get needs a path".into()) };
        let offset = call.args["offset"].as_u64().unwrap_or(0);
        let location = agent::locate(&self.state, &path);
        let Some((spot, _, reply)) = self.spot(&location, &call, reply) else { return Ok(Flow::Continue) };
        if offset == 0 {
            self.agent.note(&format!("downloading {}", spot.uri()));
        }
        tokio::spawn(async move {
            let piece = agent::piece(&spot, offset).await;
            drop(reply.send(match piece {
                Ok(piece) => Reply::ok(piece),
                Err(error) => Reply::error(format!("{}: {error:#}", spot.uri())),
            }));
        });
        Ok(Flow::Continue)
    }

    /// What Borg shows: its panels and the console's text.
    fn show_screen(&mut self, reply: Sender<Reply>) -> Result<Flow> {
        self.agent.note("looked at the screen");
        let emulator = self.console_emulator();
        let console = agent::ConsoleText { lines: emulator.screen().live_text(), typed: emulator.command_line().text };
        drop(reply.send(Reply::ok(agent::screen(&self.state, &self.screen_text(), &console))));
        Ok(Flow::Continue)
    }

    /// The processes, the containers, the network, or a file's lines.
    fn read_for_agent(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        match text(&call, "source").as_deref() {
            Some("processes") => {
                const SHOWN: u64 = 40;
                let by_cpu = text(&call, "sort").as_deref() == Some("cpu");
                let shown = call.args["limit"].as_u64().unwrap_or(SHOWN).clamp(1, borg_core::PROCESS_LIMIT as u64);
                self.agent.note("read the processes");
                tokio::task::spawn_blocking(move || {
                    drop(reply.send(agent::processes(&crate::activity::processes(), by_cpu, shown as usize)));
                });
            }
            Some("containers") => {
                self.agent.note("read the containers");
                tokio::spawn(async move { drop(reply.send(agent::containers().await)) });
            }
            Some(source @ ("ports" | "connections" | "interfaces")) => {
                const SHOWN: u64 = 100;
                let source = source.to_owned();
                let shown = call.args["limit"].as_u64().unwrap_or(SHOWN).clamp(1, agent::LIST_LIMIT as u64) as usize;
                let filter = text(&call, "find").unwrap_or_default();
                let hosts = self.state.network.hosts.clone();
                self.agent.note(&format!("read the {source}"));
                tokio::task::spawn_blocking(move || {
                    let answer = match crate::network::measure() {
                        Ok((before, after, seconds)) => {
                            let network = borg_core::Network::measured(&before, after, seconds, hosts, &filter);
                            match source.as_str() {
                                "ports" => agent::ports(&network, shown),
                                "connections" => agent::connections(&network, shown),
                                _ => agent::interfaces(&network, shown),
                            }
                        }
                        Err(error) => Reply::error(format!("{error:#}")),
                    };
                    drop(reply.send(answer));
                });
            }
            Some(other) => {
                return fail(
                    reply,
                    format!(
                        "read has no source {other}: use processes, containers, ports, connections or interfaces, or a path"
                    ),
                );
            }
            None => {
                let lines = match text(&call, "lines").map(|lines| agent::parse_lines(&lines)).transpose() {
                    Ok(lines) => lines,
                    Err(error) => return fail(reply, error),
                };
                let find = call.args["find"].as_str().filter(|find| !find.is_empty()).map(str::to_owned);
                let location = agent::locate(&self.state, &text(&call, "path").unwrap_or_else(|| ".".into()));
                let Some((spot, _, reply)) = self.spot(&location, &call, reply) else { return Ok(Flow::Continue) };
                self.agent.note(&format!("read {}", spot.uri()));
                tokio::spawn(async move { drop(reply.send(agent::read(spot, lines, find).await)) });
            }
        }
        Ok(Flow::Continue)
    }

    /// Shows a place in the other panel, after learning whether it is a folder.
    fn show_spot(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        let Some(path) = text(&call, "path") else { return fail(reply, "show needs a path".into()) };
        let location = agent::locate(&self.state, &path);
        let Some((spot, _, reply)) = self.spot(&location, &call, reply) else { return Ok(Flow::Continue) };
        // A folder opens and a file is selected in its folder; a container is asked which.
        let requeue = self.agent.requeue_sender();
        tokio::spawn(async move {
            let folder = agent::is_folder(&spot).await;
            let args = serde_json::json!({ "spot": spot, "folder": folder });
            drop(requeue.send((call.follow("_show", args), reply)));
        });
        Ok(Flow::Continue)
    }

    /// Opens the folder, or selects the file in its folder, and answers once the panel shows it.
    fn reveal_spot(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        let Ok(spot) = serde_json::from_value::<Spot>(call.args["spot"].clone()) else {
            return fail(reply, "unreadable location".into());
        };
        let path = spot.path().to_path_buf();
        let (dir, focus) = if call.args["folder"] == true {
            (path, None)
        } else {
            let parent = path.parent().map_or_else(|| PathBuf::from("/"), Path::to_path_buf);
            (parent, path.file_name().map(|name| name.to_string_lossy().into_owned()))
        };
        let mount = match &spot {
            Spot::Container { id, name, .. } => Some((id.clone(), name.clone())),
            Spot::Host(_) => None,
        };
        self.agent.note(&format!("showed {}", spot.uri()));
        let flow = self.dispatch(Msg::Agent(AgentEvent::Reveal { dir, focus, mount }))?;
        let side = self.state.active.other();
        let shown = self.state.panel(side).destination().to_path_buf();
        self.reveals.push((side, shown, reply, spot.uri()));
        self.settle_reveals();
        Ok(flow)
    }

    /// The spot a location names, and whether its container runs, when the container is
    /// known now: found earlier for this call, or in the Docker view's list. Otherwise the
    /// call comes back once Docker has listed the containers, or fails if none matches.
    pub(super) fn spot(
        &self,
        location: &crate::agent::Location,
        call: &crate::agent::Call,
        reply: tokio::sync::oneshot::Sender<crate::agent::Reply>,
    ) -> Option<(crate::agent::Spot, bool, tokio::sync::oneshot::Sender<crate::agent::Reply>)> {
        use crate::agent::{self, Location, Reply, Spot};
        let (container, path) = match location {
            Location::Host(path) => return Some((Spot::Host(path.clone()), true, reply)),
            Location::Container { container, path } => (container.clone(), path.clone()),
        };
        let known = &call.args["_container"];
        if let (Some(id), Some(name)) = (known["id"].as_str(), known["name"].as_str()) {
            return Some((Spot::Container { id: id.into(), name: name.into(), path }, known["running"] == true, reply));
        }
        if let Some(found) = agent::find_container(&self.state.docker.containers, &container) {
            let running = found.state == borg_docker::State::Running;
            return Some((Spot::Container { id: found.id.clone(), name: found.name.clone(), path }, running, reply));
        }
        let requeue = self.agent.requeue_sender();
        let mut call = call.clone();
        tokio::spawn(async move {
            let containers = borg_docker::list().await.unwrap_or_default();
            match agent::find_container(&containers, &container) {
                Some(found) => {
                    let running = found.state == borg_docker::State::Running;
                    call.args["_container"] =
                        serde_json::json!({ "id": found.id, "name": found.name, "running": running });
                    drop(requeue.send((call, reply)));
                }
                None => drop(reply.send(Reply::error(format!(
                    "No container named {container}. read with source \"containers\" lists them."
                )))),
            }
        });
        None
    }

    /// Searches a process's memory for an agent, once the user approved searching that process.
    /// The approval names the process by its start too, so a new process that reuses the PID
    /// before the user answers is not searched.
    pub(super) fn search_memory(
        &mut self,
        mut call: crate::agent::Call,
        reply: tokio::sync::oneshot::Sender<crate::agent::Reply>,
    ) -> Result<Flow> {
        use crate::agent::{Reply, Work};
        use borg_core::{AgentEvent, Ask, AskKind, DiffLine, MemorySearchMode};
        let fail = |reply: tokio::sync::oneshot::Sender<Reply>, text: String| {
            drop(reply.send(Reply::error(text)));
            Ok(Flow::Continue)
        };
        let Some(pid) = call.args["pid"].as_u64().and_then(|pid| u32::try_from(pid).ok()).filter(|&pid| pid > 0) else {
            return fail(reply, "search_memory needs the PID of a running process".into());
        };
        let (mode, label) = match call.args["mode"].as_str().unwrap_or("indicators") {
            "indicators" => (MemorySearchMode::Indicators, tr!(memory_mode_indicators)),
            "strings" => (MemorySearchMode::Strings, tr!(memory_mode_strings)),
            "text" => (MemorySearchMode::Text, tr!(memory_mode_text)),
            "unicode" => (MemorySearchMode::Unicode, tr!(memory_mode_unicode)),
            "hex" => (MemorySearchMode::Hex, tr!(memory_mode_hex)),
            other => {
                return fail(
                    reply,
                    format!("search_memory has no mode {other}: use indicators, strings, text, unicode or hex"),
                );
            }
        };
        let query = crate::memory_search::Query {
            text: call.args["query"].as_str().unwrap_or_default().to_owned(),
            mode,
            match_case: call.args["match_case"].as_bool().unwrap_or(false),
        };
        if let Err(error) = crate::memory_search::validate(&query.text, mode) {
            return fail(reply, format!("{error:#}"));
        }
        let Some((process, name, program)) = crate::activity::identify(pid) else {
            return fail(reply, format!("No process with PID {pid} is running"));
        };
        if call.approved && call.args["_start"].as_u64() != Some(process.start) {
            return fail(reply, format!("The process with PID {pid} ended before the search could start"));
        }
        let allowed = call.approved
            || self.memory_readers.contains(&(call.origin.clone(), process))
            || call.origin.as_ref().is_some_and(|origin| self.approve_all.contains(origin));
        if !allowed {
            let mut body = vec![DiffLine::plain(tr!(agent_memory_process, name = name, pid = pid))];
            if let Some(program) = &program {
                body.push(DiffLine::plain(tr!(agent_memory_program, path = program.display())));
            }
            let searched = if query.text.is_empty() { label.to_owned() } else { format!("{label} · {}", query.text) };
            body.extend([
                DiffLine::plain(tr!(agent_memory_search, mode = searched)),
                DiffLine::plain(""),
                DiffLine::plain(tr!(agent_memory_warning)),
            ]);
            let place = call
                .origin
                .as_ref()
                .map_or_else(String::new, |origin| tr!(agent_asked_from).replace("{origin}", origin));
            let ask = Ask { kind: AskKind::Memory, summary: format!("{name} (PID {pid})"), place, body };
            call.args["_start"] = serde_json::json!(process.start);
            let id = self.agent.ask(reply, Work::Forward(call));
            return self.dispatch(Msg::Agent(AgentEvent::Ask { id, ask }));
        }
        self.memory_readers.insert((call.origin.clone(), process));
        let limit = call.args["limit"].as_u64().unwrap_or(100).clamp(1, 2000) as usize;
        let admin = call.args["admin"] == true;
        self.agent.note(&format!("searched the memory of {name} (PID {pid})"));
        tokio::spawn(async move {
            let cancelled = Arc::new(AtomicBool::new(false));
            let answer = match crate::activity::search_for_agent(process, query, admin, cancelled).await {
                Ok(report) => crate::agent::memory_report(&report, &name, limit),
                Err(error) => Reply::error(format!("{error:#}")),
            };
            drop(reply.send(answer));
        });
        Ok(Flow::Continue)
    }

    /// Answers reveals once their panel shows the folder, or is going somewhere else.
    pub(super) fn settle_reveals(&mut self) {
        let state = &self.state;
        let (done, waiting): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.reveals).into_iter().partition(|(side, dir, ..)| {
                let panel = state.panel(*side);
                panel.destination() != dir || (panel.dir() == dir && *panel.listing() != borg_core::Listing::Loading)
            });
        self.reveals = waiting;
        for (side, dir, reply, path) in done {
            let panel = self.state.panel(side);
            let answer = match panel.listing() {
                _ if panel.destination() != dir => {
                    crate::agent::Reply::ok(format!("Showed {path}; the panel has moved on since"))
                }
                borg_core::Listing::Failed(error) => crate::agent::Reply::error(format!("{path}: {error}")),
                _ => crate::agent::Reply::ok(format!("Showing {path} in Borg's other panel")),
            };
            drop(reply.send(answer));
        }
    }
}
