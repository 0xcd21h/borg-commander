//! Starting file jobs and the copies to remote Borgs and containers, and steering them as they run.

use super::App;
use borg_core::{JobCommand, Msg};
use borg_jobs::{Control, Spec, run};
use std::sync::Arc;

impl App {
    pub(super) fn start_job(&mut self, id: u64, spec: Spec) {
        let cache = self.search.cache.clone();
        let mut changed = match &spec {
            Spec::Copy { .. } => Vec::new(),
            _ => spec.sources().to_vec(),
        };
        if let Spec::Copy { target, .. } | Spec::Move { target, .. } = &spec {
            changed.push(target.clone());
        }
        cache.begin_changes();
        let control = self.control_for(id);
        let messages = self.messages.clone();
        tokio::task::spawn_blocking(move || {
            run(&spec, &control, &mut |event| {
                if matches!(event, borg_jobs::Event::Finished(_)) {
                    cache.invalidate(&changed);
                }
                drop(messages.send(Msg::Job { id, event }));
            });
        });
    }

    pub(super) fn start_docker_copy(&mut self, id: u64, job: crate::docker::CopyJob) {
        let control = self.control_for(id);
        let messages = self.messages.clone();
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let outcome = job.run(&runtime, &control, &mut |event| drop(messages.send(Msg::Job { id, event })));
            drop(messages.send(Msg::Job { id, event: borg_jobs::Event::Finished(outcome) }));
        });
    }

    pub(super) fn start_remote_copy(&mut self, id: u64, job: crate::remote_copy::CopyJob) {
        let control = self.control_for(id);
        let messages = self.messages.clone();
        let runtime = tokio::runtime::Handle::current();
        let cache = self.search.cache.clone();
        let changed = if job.to.is_none() { vec![job.target.clone()] } else { Vec::new() };
        cache.begin_changes();
        tokio::task::spawn_blocking(move || {
            let outcome = job.run(&runtime, &control, &mut |event| drop(messages.send(Msg::Job { id, event })));
            cache.invalidate(&changed);
            drop(messages.send(Msg::Job { id, event: borg_jobs::Event::Finished(outcome) }));
        });
    }

    pub(super) fn control_job(&self, id: u64, command: JobCommand) {
        let Some(control) = self.jobs.get(&id) else { return };
        match command {
            JobCommand::Pause(paused) => control.set_paused(paused),
            JobCommand::Cancel => control.cancel(),
            JobCommand::Answer(decision) => control.answer(decision),
        }
    }

    /// A new control for job `id`, kept with those of the jobs still running.
    fn control_for(&mut self, id: u64) -> Arc<Control> {
        let running: Vec<u64> = self.state.jobs.iter().map(|job| job.id).collect();
        self.jobs.retain(|id, _| running.contains(id));
        let control = Arc::new(Control::default());
        self.jobs.insert(id, Arc::clone(&control));
        control
    }
}
