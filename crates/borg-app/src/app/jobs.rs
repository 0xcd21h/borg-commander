//! Starting file jobs and the copies to remote Borgs and containers, and steering them as they run.

use super::App;
use borg_core::{
    JobCommand, Msg, PackedFile, ReplaceRequest, RewriteRequest, UnpackRequest, UnpackTarget, Viewer, viewer_size,
};
use borg_jobs::{Control, Spec, run};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

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
        let drives = self.drives.clone();
        tokio::task::spawn_blocking(move || {
            run(&spec, &drives, &control, &mut |event| {
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
        let drives = self.drives.clone();
        let changed = if job.to.is_none() { vec![job.target.clone()] } else { Vec::new() };
        cache.begin_changes();
        tokio::task::spawn_blocking(move || {
            let emit = &mut |event| drop(messages.send(Msg::Job { id, event }));
            let outcome = job.run(&runtime, &drives, &control, emit);
            cache.invalidate(&changed);
            drop(messages.send(Msg::Job { id, event: borg_jobs::Event::Finished(outcome) }));
        });
    }

    /// Unpacks entries of an archive into a folder, moving them when the request says so, or
    /// the one file the viewer or the editor waits for. The viewer shows its file from Borg's
    /// folder for such files, which then removes it; the editor keeps its file there.
    pub(super) fn start_unpack(&mut self, id: u64, request: UnpackRequest) {
        let control = self.control_for(id);
        let messages = self.messages.clone();
        let indexes = self.archives.clone();
        let drives = self.drives.clone();
        let cache = self.search.cache.clone();
        let scratch = self.runtime_dir.join(borg_core::ARCHIVE_VIEWS);
        let size = viewer_size(self.state.cols, self.state.rows);
        let cell = self.graphics.capabilities().cell;
        let changed: Vec<PathBuf> = match &request.target {
            UnpackTarget::Folder(target) if request.remove => vec![target.clone(), request.archive.clone()],
            UnpackTarget::Folder(target) => vec![target.clone()],
            UnpackTarget::Viewer { .. } | UnpackTarget::Editor { .. } => Vec::new(),
        };
        cache.begin_changes();
        tokio::task::spawn_blocking(move || {
            let emit = &mut |event| drop(messages.send(Msg::Job { id, event }));
            let outcome = match &request.target {
                UnpackTarget::Folder(target) if request.remove => {
                    crate::archive::move_out(&indexes, &request, target, &drives, &control, emit)
                }
                UnpackTarget::Folder(target) => {
                    crate::archive::unpack(&indexes, &request, target, &drives, &control, emit).outcome
                }
                UnpackTarget::Editor { shown, slot } => {
                    let (outcome, unpacked) =
                        crate::archive::unpack_for_editor(&indexes, &request, &scratch, &drives, &control, emit);
                    if let Some(unpacked) = unpacked {
                        let packed = PackedFile {
                            archive: request.archive.clone(),
                            password: request.password.clone(),
                            entry: request.paths.first().cloned().unwrap_or_default(),
                            shown: shown.clone(),
                        };
                        let result = crate::edit_file::read(&unpacked);
                        drop(messages.send(Msg::ArchiveEditorOpened { slot: *slot, packed, result }));
                    }
                    outcome
                }
                UnpackTarget::Viewer { shown } => {
                    let (outcome, unpacked) =
                        crate::archive::unpack_for_viewer(&indexes, &request, &scratch, &drives, &control, emit);
                    if let Some(unpacked) = unpacked {
                        let content = crate::document::open(
                            &unpacked.path,
                            size,
                            (cell.width, cell.height),
                            Arc::new(AtomicBool::new(false)),
                        );
                        drop(messages.send(match content {
                            Ok(content) => Msg::Opened(Viewer::new(shown.clone(), content)),
                            Err(error) => Msg::OpenFailed { path: shown.clone(), notice: format!("{error:#}") },
                        }));
                    }
                    outcome
                }
            };
            cache.invalidate(&changed);
            drop(messages.send(Msg::Job { id, event: borg_jobs::Event::Finished(outcome) }));
        });
    }

    /// Changes an archive: adds, removes, moves or replaces entries.
    pub(super) fn start_rewrite(&mut self, id: u64, request: RewriteRequest) {
        let control = self.control_for(id);
        let messages = self.messages.clone();
        let indexes = self.archives.clone();
        let drives = self.drives.clone();
        let cache = self.search.cache.clone();
        let mut changed = request.originals.clone();
        changed.push(request.archive.clone());
        cache.begin_changes();
        tokio::task::spawn_blocking(move || {
            let emit = &mut |event| drop(messages.send(Msg::Job { id, event }));
            let outcome = crate::archive::rewrite(&indexes, &request, &drives, &control, emit);
            cache.invalidate(&changed);
            drop(messages.send(Msg::Job { id, event: borg_jobs::Event::Finished(outcome) }));
        });
    }

    /// Replaces the text a contents search found in the files it found.
    pub(super) fn start_replace(&mut self, id: u64, request: ReplaceRequest) {
        let control = self.control_for(id);
        let messages = self.messages.clone();
        let cache = self.search.cache.clone();
        cache.begin_changes();
        tokio::task::spawn_blocking(move || {
            let emit = &mut |event| drop(messages.send(Msg::Job { id, event }));
            let outcome = crate::replace_files::run(&request, &control, emit);
            cache.invalidate(&request.files);
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
