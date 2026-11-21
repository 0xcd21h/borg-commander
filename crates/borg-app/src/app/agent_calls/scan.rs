//! Scanning files and processes with rules, and checking a process's integrity, for an agent.

use super::{App, Flow, fail};
use crate::activity::ScanRequest;
use crate::agent::{self, Call, Location, Reply, ScanCall, ScanTarget, Work};
use crate::rule_scan::RuleSet;
use anyhow::Result;
use borg_core::{AgentEvent, Msg, ScanSubject};
use serde_json::json;
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};
use tokio::sync::oneshot::Sender;

/// Set once the agent's own rule text compiled, off the event loop, so the user is never asked
/// to approve rules that cannot run.
const COMPILED: &str = "_compiled";
/// The start of the process the user approved, so a new one that takes its PID is not read.
const START: &str = "_start";

impl App {
    /// Scans a file or folder with rules at once, or a process's memory once the user lets the
    /// agent read that process.
    pub(super) fn scan(&mut self, call: Call, reply: Sender<Reply>) -> Result<Flow> {
        let asked = match agent::scan_call(&call.args) {
            Ok(asked) => asked,
            Err(error) => return fail(reply, error),
        };
        let rules = match &asked.rules {
            Some(text) => RuleSet::Inline(text.clone()),
            None => RuleSet::Installed { folder: crate::rule_scan::folder() },
        };
        match &asked.target {
            ScanTarget::Location(path) => self.scan_files(path, rules, asked.limit, reply),
            ScanTarget::Process(pid) => {
                let pid = *pid;
                self.scan_process(call, asked, pid, rules, reply)
            }
        }
    }

    fn scan_files(&mut self, path: &str, rules: RuleSet, limit: usize, reply: Sender<Reply>) -> Result<Flow> {
        let root = match agent::locate(&self.state, path) {
            Location::Host(root) => root,
            Location::Container { .. } => {
                return fail(reply, "scan reads files on the machine Borg runs on: name a path there".into());
            }
        };
        let whose = if matches!(rules, RuleSet::Inline(_)) { "its own rules" } else { "rules" };
        self.agent.note(&format!("scanned {} with {whose}", root.display()));
        tokio::task::spawn_blocking(move || drop(reply.send(scan_files(&root, &rules, limit))));
        Ok(Flow::Continue)
    }

    fn scan_process(
        &mut self,
        mut call: Call,
        asked: ScanCall,
        pid: u32,
        rules: RuleSet,
        reply: Sender<Reply>,
    ) -> Result<Flow> {
        if let RuleSet::Inline(text) = &rules
            && call.args[COMPILED] != true
        {
            let (requeue, text) = (self.agent.requeue_sender(), text.clone());
            tokio::task::spawn_blocking(move || match borg_rules::Rules::inline(&text) {
                Ok(_) => {
                    call.args[COMPILED] = json!(true);
                    drop(requeue.send((call, reply)));
                }
                Err(error) => drop(reply.send(Reply::error(agent::rule_text_problem(&error)))),
            });
            return Ok(Flow::Continue);
        }
        let Some((process, name, program)) = crate::activity::identify(pid) else {
            return fail(reply, format!("No process with PID {pid} is running"));
        };
        if call.approved && call.args[START].as_u64() != Some(process.start) {
            return fail(reply, format!("The process with PID {pid} ended before the scan could start"));
        }
        if !self.may_read_memory(&call, process) {
            let subject = ScanSubject { name: &name, pid, program: program.as_deref() };
            let ask = borg_core::scan_ask(subject, asked.checks, asked.rules.as_deref(), call.origin.clone());
            call.args[START] = json!(process.start);
            let id = self.agent.ask(reply, Work::Forward(call));
            return self.dispatch(Msg::Agent(AgentEvent::Ask { id, ask }));
        }
        self.memory_readers.insert((call.origin.clone(), process));
        let checks = match (asked.checks.integrity(), asked.checks.rules()) {
            (true, true) => "integrity and rules",
            (true, false) => "integrity",
            _ => "rules",
        };
        self.agent.note(&format!("scanned the memory of {name} (PID {pid}) for {checks}"));
        let request =
            ScanRequest { process, integrity: asked.checks.integrity(), rules: asked.checks.rules().then_some(rules) };
        let (limit, admin) = (asked.limit, asked.admin);
        tokio::spawn(async move {
            let cancelled = Arc::new(AtomicBool::new(false));
            let answer = match crate::activity::scan_for_agent(request, admin, cancelled).await {
                Ok(scan) => agent::process_answer(&scan, &name, pid, limit, admin),
                Err(error) => Reply::error(format!("{error:#}")),
            };
            drop(reply.send(answer));
        });
        Ok(Flow::Continue)
    }
}

/// Compiles the rules and scans the file or folder at `root` with them.
fn scan_files(root: &Path, rules: &RuleSet, limit: usize) -> Reply {
    let rules = match rules.compile() {
        Ok(rules) => rules,
        Err(error) => return Reply::error(agent::rule_text_problem(&error)),
    };
    match crate::rule_scan::scan_files(&rules, root, &AtomicBool::new(false)) {
        Ok(scan) => agent::files_answer(&scan, root, limit),
        Err(error) => Reply::error(format!("{}: {error}", root.display())),
    }
}
