//! This session's asks, for the borgs of paired computers whose panels show it: what waits here,
//! their answers to it, and their word to turn off Approve all. Only a viewer connection reaches
//! these calls, and an answer counts only while its ask still waits and no other computer
//! controls the session.

use super::{App, Flow};
use crate::agent::{Call, Reply};
use crate::link::{ASKS_ANSWER, ASKS_WAIT, ASKS_WAIT_TIME};
use anyhow::Result;
use borg_core::{AgentEvent, HostAnswer, Msg, check_answer};
use tokio::sync::oneshot::Sender;

/// The paired computer a call came from, when it came on that computer's viewer connection.
fn viewer(call: &Call) -> Option<String> {
    call.origin.clone().filter(|_| call.viewer)
}

impl App {
    /// Tells the viewers what waits here now, when that changed.
    pub(super) fn publish_asks(&mut self) {
        let controller = self.native_mirror.controller();
        self.ask_board.send_if_modified(|board| board.refresh(&self.state, controller.as_deref()));
    }

    /// A viewer's call: refused unless it came on a viewer connection.
    pub(super) fn on_viewer(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        let Some(viewer) = viewer(&call) else {
            drop(reply.send(Reply::error(format!("unknown tool {}", call.tool))));
            return Ok(Flow::Continue);
        };
        match call.tool.as_str() {
            ASKS_WAIT => {
                self.wait_for_asks(&call, viewer, reply);
                Ok(Flow::Continue)
            }
            ASKS_ANSWER => self.take_answer(&call, viewer, reply),
            _ => {
                self.approval_mode_off(Some(&viewer));
                drop(reply.send(Reply::ok("Approve all is off")));
                Ok(Flow::Continue)
            }
        }
    }

    /// Answers with what waits here once it differs from the version the viewer has, or after
    /// a while with what it is.
    fn wait_for_asks(&self, call: &Call, viewer: String, mut reply: Sender<Reply>) {
        let after = call.args["after"].as_u64().unwrap_or_default();
        let mut board = self.ask_board.subscribe();
        let run = self.run.clone();
        tokio::spawn(async move {
            if board.borrow_and_update().version() == after {
                tokio::select! {
                    _ = board.changed() => {}
                    () = reply.closed() => return,
                    () = tokio::time::sleep(ASKS_WAIT_TIME) => {}
                }
            }
            let feed = board.borrow().feed(&run, &viewer);
            drop(reply.send(match serde_json::to_string(&feed) {
                Ok(text) => Reply::ok(text),
                Err(error) => Reply::error(error.to_string()),
            }));
        });
    }

    /// A viewer's answer: carried out as one given here when it may count, else refused with
    /// the reason, which the reply names.
    fn take_answer(&mut self, call: &Call, viewer: String, reply: Sender<Reply>) -> Result<Flow> {
        let Ok(answer) = serde_json::from_value::<HostAnswer>(call.args["answer"].clone()) else {
            drop(reply.send(Reply::error("asks_answer needs an answer")));
            return Ok(Flow::Continue);
        };
        let controller = self.native_mirror.controller();
        let refused = check_answer(&self.state, &answer, &self.run, &viewer, controller.as_deref()).err();
        drop(reply.send(Reply::ok(serde_json::to_string(&refused)?)));
        if refused.is_some() {
            return Ok(Flow::Continue);
        }
        self.dispatch(Msg::Agent(AgentEvent::AnsweredFrom { id: answer.id, approved: answer.approved, by: viewer }))
    }
}
