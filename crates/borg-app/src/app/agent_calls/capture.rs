//! Capturing traffic for an agent once the user approves it, and reading saved captures back.

use super::{App, Flow, fail, text};
use crate::agent::{self, Call, Location, Reply, Work};
use crate::app::render::from_origin;
use anyhow::Result;
use borg_core::{AgentEvent, CaptureRequest, CaptureScope, Keep, Msg, SeenSockets};
use borg_i18n::{Count, count, tr};
use serde_json::json;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot::Sender;

/// What a capture call comes back with once Borg has checked its folder and process and
/// picked its interface.
const FOLDER: &str = "_folder";
const INTERFACE: &str = "_interface";
const PROCESS: &str = "_process";
/// How many agent captures keep the sockets seen while they ran.
const CAPTURES_KEPT: usize = 16;

/// The sockets seen while recent agent captures ran, by the file each was saved to, so that a
/// later read of the file still tells each conversation's process.
#[derive(Clone, Default)]
pub(in crate::app) struct SavedCaptures(Arc<Mutex<Vec<(PathBuf, SeenSockets)>>>);

impl SavedCaptures {
    fn keep(&self, path: PathBuf, seen: SeenSockets) {
        if let Ok(mut saved) = self.0.lock() {
            saved.push((path, seen));
            let overflow = saved.len().saturating_sub(CAPTURES_KEPT);
            saved.drain(..overflow);
        }
    }

    fn seen(&self, path: &Path) -> SeenSockets {
        let saved = self.0.lock().ok();
        saved
            .and_then(|saved| saved.iter().rev().find(|(saved, _)| saved == path).map(|(_, seen)| seen.clone()))
            .unwrap_or_default()
    }
}

impl App {
    /// Asks the user to approve an agent's capture. First, off the event loop, Borg checks the
    /// folder and the process and picks the interface: the one the call names, or the one the
    /// network screen's dialog would, which takes a second of measuring. The call then comes
    /// back with what it found.
    pub(super) fn ask_to_capture(&mut self, mut call: Call, reply: Sender<Reply>) -> Result<Flow> {
        let asked = match agent::capture_call(&call.args) {
            Ok(asked) => asked,
            Err(error) => return fail(reply, error),
        };
        let (Some(folder), Some(interface)) = (text(&call, FOLDER), text(&call, INTERFACE)) else {
            let folder = match agent::locate(&self.state, &text(&call, "folder").unwrap_or_else(|| ".".into())) {
                Location::Host(folder) => folder,
                Location::Container { .. } => {
                    return fail(reply, "A capture is saved on the machine Borg runs on: name a folder there".into());
                }
            };
            let requeue = self.agent.requeue_sender();
            tokio::task::spawn_blocking(move || {
                let prepared = prepare(&folder, asked.scope, asked.interface);
                match prepared {
                    Ok((interface, process)) => {
                        call.args[FOLDER] = json!(folder.to_string_lossy());
                        call.args[INTERFACE] = json!(interface);
                        call.args[PROCESS] = json!(process);
                        drop(requeue.send((call, reply)));
                    }
                    Err(error) => drop(reply.send(Reply::error(error))),
                }
            });
            return Ok(Flow::Continue);
        };
        let request = CaptureRequest {
            interface,
            seconds: Some(asked.seconds),
            keep: if asked.headers_only { Keep::Headers } else { Keep::Whole },
            filter: asked.scope.filter(),
            folder: PathBuf::from(folder),
            administrator: false,
            processes: false,
            tls_keys: borg_core::TlsKeys::Off,
        };
        let place = from_origin(String::new(), call.origin.as_deref());
        let ask = borg_core::capture_ask(asked.scope, text(&call, PROCESS).as_deref(), &request, place);
        let id =
            self.agent.ask(reply, Work::Capture { request, filter: asked.scope.view_filter(), limit: asked.limit });
        if call.origin.as_ref().is_some_and(|origin| self.approve_all.contains(origin)) {
            return self.agent_answer(id, true);
        }
        self.dispatch(Msg::Agent(AgentEvent::Ask { id, ask }))
    }

    /// Runs an approved capture, answers with its summary, and tells the user where it went.
    pub(in crate::app) fn capture_for_agent(
        &mut self,
        id: u64,
        reply: Sender<Reply>,
        request: CaptureRequest,
        filter: String,
        limit: usize,
    ) {
        let seconds = request.seconds.unwrap_or_default();
        self.agent.note(&format!("capturing on {} for {seconds} s", request.interface));
        let (hosts, saved, messages) =
            (self.state.network.hosts.clone(), self.saved_captures.clone(), self.messages.clone());
        tokio::spawn(async move {
            let (answer, notice) = match crate::network::record_for_agent(&request).await {
                Ok((path, capture, seen)) => {
                    let packets = capture.packets.len();
                    agent::note(&messages, &format!("captured {packets} packets to {}", path.display()));
                    let notice = tr!(agent_captured, packets = count(packets, Count::Packet), path = path.display());
                    let interface = request.interface;
                    let summary = tokio::task::spawn_blocking(move || {
                        let mut view = seen.view(capture, &hosts, Some(path.clone()));
                        view.interface = Some(interface);
                        view.set_filter(&filter);
                        saved.keep(path, seen);
                        view.report(limit)
                    });
                    match summary.await {
                        Ok(summary) => (Reply::ok(summary), notice),
                        Err(error) => (Reply::error(error.to_string()), notice),
                    }
                }
                Err(error) => (Reply::error(error.clone()), error),
            };
            drop(reply.send(answer));
            drop(messages.send(Msg::Agent(AgentEvent::Finished { id, notice })));
        });
    }

    /// Reads a saved capture for an agent: its summary, kept by `find` as the viewer's filter
    /// keeps rows, or one packet by its number.
    pub(super) fn read_capture(
        &self,
        path: PathBuf,
        find: Option<String>,
        packet: Option<usize>,
        limit: usize,
        reply: Sender<Reply>,
    ) -> Result<Flow> {
        let (hosts, seen) = (self.state.network.hosts.clone(), self.saved_captures.seen(&path));
        tokio::task::spawn_blocking(move || {
            let answer = match crate::network::read_capture(&path) {
                Err(error) => Reply::error(error),
                Ok(capture) => {
                    let mut view = seen.view(capture, &hosts, Some(path));
                    match packet {
                        Some(number) => view.packet_report(number).map_or_else(
                            || {
                                let total = view.capture.packets.len();
                                Reply::error(format!("The capture has no packet {number}: it holds {total}"))
                            },
                            Reply::ok,
                        ),
                        None => {
                            view.set_filter(find.as_deref().unwrap_or_default());
                            Reply::ok(view.report(limit))
                        }
                    }
                }
            };
            drop(reply.send(answer));
        });
        Ok(Flow::Continue)
    }
}

/// Checks that captures can be saved in `folder` and that a process scope's process runs, and
/// picks the interface; answers with the interface and the process's name.
fn prepare(folder: &Path, scope: CaptureScope, interface: Option<String>) -> Result<(String, Option<String>), String> {
    if !folder.is_dir() {
        return Err(format!("{} is not a folder", folder.display()));
    }
    let process = match scope {
        CaptureScope::Process { pid } => match crate::activity::identify(pid) {
            Some((_, name, _)) => Some(name),
            None => return Err(format!("No process with PID {pid} is running")),
        },
        _ => None,
    };
    let interface = match interface {
        Some(interface) => interface,
        None => {
            let (before, after, seconds) = crate::network::measure().map_err(|error| format!("{error:#}"))?;
            let network = borg_core::Network::measured(&before, after, seconds, HashMap::new(), "");
            let picked = borg_core::capture_interfaces(&network, scope).into_iter().next();
            picked.ok_or_else(|| tr!(capture_no_interface).to_owned())?
        }
    };
    Ok((interface, process))
}
