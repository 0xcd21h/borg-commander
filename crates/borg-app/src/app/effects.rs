//! Carrying out the effects the core asks for: files, jobs, viewers, listings and settings.

use super::{App, Flow, Plain, REMOTE_CLIENT_REFRESH, TERMINAL_CLIPBOARD_LIMIT};
use crate::session::StateDir;
use anyhow::{Context, Result};
use borg_core::{Aside, Effect, JobCommand, Msg, Side, Viewer, update, viewer_size};
use borg_i18n::tr;
use borg_jobs::{Control, Spec, run};
use borg_term::{GridSize, Scroll};
use futures::StreamExt;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

impl App {
    pub(super) fn apply(&mut self, effects: Vec<Effect>) -> Result<Flow> {
        for effect in effects {
            match effect {
                Effect::CancelShellExit { token } => self.pty.write(format!("{token}:no\n").as_bytes())?,
                Effect::MeasureSizes { id, root } => self.search.measure(id, root),
                Effect::CancelMeasure => self.search.cancel_measure(),
                Effect::CancelMemorySearch => self.monitor.cancel_search(),
                Effect::SizeChildren { side, generation, root, hidden, parent } => {
                    let cache = self.search.cache.clone();
                    let messages = self.messages.clone();
                    tokio::task::spawn_blocking(move || {
                        let result = cache
                            .children(&root, hidden, &parent)
                            .ok_or_else(|| tr!(size_branch_unavailable).to_owned());
                        let _ = messages.send(Msg::SizeChildren { side, generation, parent, result });
                    });
                }
                Effect::Activity { id, request } => self.monitor.request(id, request, self.state.activity.selected),
                Effect::Backup(request) => self.backup.request(request),
                Effect::Docker { id, request } => self.docker.request(id, request),
                Effect::DockerWatch(on) => self.docker.watch(on),
                Effect::DockerShell(request) => self.container_shell(request),
                Effect::ExportMemory { path, bytes } => {
                    let messages = self.messages.clone();
                    tokio::task::spawn_blocking(move || {
                        let notice = match crate::activity::export(&path, &bytes) {
                            Ok(()) => tr!(activity_exported, path = path.display()),
                            Err(error) => tr!(activity_export_error, error = error),
                        };
                        let _ = messages.send(Msg::Notice(notice));
                    });
                }
                Effect::SaveGame2048 => {
                    if let (Some(dir), Some(game)) = (&self.state_dir, &self.state.game2048)
                        && let Err(error) = dir.save_game2048(game)
                    {
                        self.state.notice = Some(tr!(game2048_save_error, error = error));
                    }
                }
                Effect::Search { side, generation, request } => self.search.search(side, generation, request),
                Effect::RefreshCollection { side, generation, entries } => {
                    self.search.refresh(side, generation, entries)
                }
                Effect::CancelSearch(side) => self.search.cancel(side),
                Effect::SetIndexRoots(roots) => {
                    let added: Vec<_> =
                        roots.iter().filter(|root| !self.search.indexes.roots().contains(root)).cloned().collect();
                    self.search.indexes.set_roots(roots.clone());
                    for root in added {
                        self.search.indexes.refresh(root, self.state.settings.search.clone());
                    }
                    self.config.set_index_roots(&roots);
                    self.save_config();
                }
                Effect::SaveSearchHistory => {
                    if let Some(dir) = &self.state_dir
                        && let Err(error) = dir.save_search_history(&self.state.search_history)
                    {
                        self.state.notice = Some(tr!(search_history_save_error, error = error));
                    }
                }
                Effect::SaveCollections => {
                    if !self.collections_writable {
                        self.state.notice = Some(tr!(collections_preserved).into());
                        continue;
                    }
                    if let Some(dir) = &self.state_dir
                        && let Err(error) = dir.save_collections(&self.state.collections)
                    {
                        self.state.notice = Some(tr!(collections_save_error, error = error));
                    }
                }
                Effect::List { side, generation, dir } => {
                    self.search.cancel(side);
                    self.list(side, generation, dir);
                }
                Effect::GitScan { side, generation, dir } => self.plugins.scan(side, generation, dir),
                Effect::GitTool { id, tool, dir, paths } => {
                    let messages = self.messages.clone();
                    let size = viewer_size(self.state.cols, self.state.rows);
                    tokio::spawn(async move {
                        let result =
                            crate::plugins::run(dir, tool, paths, size).await.map_err(|error| format!("{error:#}"));
                        let _ = messages.send(Msg::GitFinished { id, result });
                    });
                }
                Effect::Write(bytes) => self.write_console(bytes)?,
                Effect::OpenEditor { path, slot } => {
                    let messages = self.messages.clone();
                    tokio::task::spawn_blocking(move || {
                        let _ = messages.send(Msg::EditorOpened(slot, crate::edit_file::read(&path)));
                    });
                }
                Effect::SaveFile { path, bytes, expected, force, slot } => {
                    let messages = self.messages.clone();
                    tokio::task::spawn_blocking(move || {
                        let saved = crate::edit_file::write(&path, &bytes, expected, force);
                        let _ = messages.send(Msg::EditorSaved(slot, saved));
                    });
                }
                Effect::WriteConcealed(bytes) => {
                    self.console_emulator().conceal();
                    self.write_console(bytes)?;
                }
                Effect::ChangeShellDir { dir, typed } => {
                    self.emulator.scroll(Scroll::Bottom);
                    match borg_term::request_cd(&self.cd_request, &dir) {
                        Ok(()) => self.pty.write(borg_term::SILENT_CD_KEY)?,
                        Err(_) => self.pty.write(&typed)?,
                    }
                }
                Effect::ScrollConsole(scroll) => self.console_emulator().scroll(scroll),
                Effect::UseConsole(console) => {
                    if let borg_core::Console::Remote { machine, session } = &console {
                        self.remote_consoles.open(
                            (machine.clone(), session.clone()),
                            self.state.active_panel().destination().into(),
                            GridSize::console(self.state.cols, self.state.rows),
                            self.state.settings.scrollback,
                        );
                    }
                    self.console = console;
                }
                Effect::MirrorRemote { machine, session } => {
                    self.mirror_connect =
                        Some(Box::pin(
                            async move { crate::mirror::connect(&format!("{machine}:{session}"), false).await },
                        ));
                }
                Effect::CancelRemoteConnection => self.mirror_connect = None,
                Effect::ForgetRemoteHost { machine } => {
                    if let Some(previous) = self.remote_catalog.take() {
                        previous.abort();
                    }
                    let messages = self.messages.clone();
                    tokio::task::spawn_blocking(move || {
                        let result = StateDir::locate()
                            .context("Borg has no state folder")
                            .and_then(|state| crate::link::forget_server(state.path(), &machine).map(|_| ()))
                            .map_err(|error| format!("{error:#}"));
                        let _ = messages.send(Msg::RemoteHostForgotten { machine, result });
                    });
                }
                Effect::DeleteRemoteSession { machine, session } => {
                    let messages = self.messages.clone();
                    tokio::spawn(async move {
                        let result = crate::remote::delete_session(&machine, &session)
                            .await
                            .map_err(|error| format!("{error:#}"));
                        let _ = messages.send(Msg::RemoteSessionDeleted { machine, session, result });
                    });
                }
                Effect::CloseRemoteConsole { machine, session } => self.remote_consoles.close(&(machine, session)),
                Effect::SaveRemoteName { identity, session, name } => {
                    let messages = self.messages.clone();
                    tokio::task::spawn_blocking(move || {
                        let result = StateDir::locate()
                            .context("Borg has no state folder")
                            .and_then(|state| {
                                crate::remote_names::rename(state.path(), &identity, session.as_deref(), &name)
                            })
                            .map_err(|error| tr!(remote_names_error, error = error));
                        let _ = messages.send(Msg::RemoteNames(result));
                    });
                }
                Effect::OpenRemotes => {
                    let messages = self.messages.clone();
                    if let Some(previous) = self.remote_catalog.take() {
                        previous.abort();
                    }
                    self.remote_catalog = Some(tokio::spawn(async move {
                        if let Some(dir) = StateDir::locate() {
                            let names = crate::remote_names::load(dir.path())
                                .map_err(|error| tr!(remote_names_error, error = error));
                            let _ = messages.send(Msg::RemoteNames(names));
                        }
                        let hosts = crate::remote::catalog(|choices, complete| {
                            let _ = messages.send(Msg::RemoteCatalog { choices, complete });
                        });
                        let clients = async {
                            loop {
                                let clients = match StateDir::locate() {
                                    Some(dir) => crate::link::paired_clients(dir.path()).await,
                                    None => Vec::new(),
                                };
                                if messages.send(Msg::RemoteClients(clients)).is_err() {
                                    break;
                                }
                                tokio::time::sleep(REMOTE_CLIENT_REFRESH).await;
                            }
                        };
                        tokio::join!(hosts, clients);
                    }));
                }
                Effect::MountRemote { side, machine, session, name } => {
                    let effects =
                        update(&mut self.state, Msg::MountRemote { side, machine, session, name }, Instant::now());
                    if let Flow::Quit = self.apply(effects)? {
                        return Ok(Flow::Quit);
                    }
                }
                Effect::ApprovalModeOff => {
                    self.approve_all.clear();
                    self.state.agent.auto_approve.clear();
                    self.state.notice = Some("Approve all is off for this session.".into());
                    self.agent.note("Approve all turned off locally");
                }
                Effect::Copy(text) if text.len() > TERMINAL_CLIPBOARD_LIMIT => {
                    self.state.notice = Some(tr!(clipboard_borg_only).into());
                }
                Effect::Copy(text) => {
                    self.native_mirror.copy(&text);
                    self.clipboard = Some(text);
                }
                Effect::ShowPlain(text) => self.plain = Some(Plain::Waiting(text)),
                Effect::StartPairing => self.start_pairing(),
                Effect::SetRelay(text) => self.set_relay(&text),
                Effect::ForgetComputers => {
                    if let Err(error) = self.link.forget() {
                        self.state.notice = Some(format!("{error:#}"));
                    }
                }
                Effect::AgentAnswer { id, approved } => {
                    if let Flow::Quit = self.agent_answer(id, approved)? {
                        return Ok(Flow::Quit);
                    }
                }
                Effect::View(path) => self.open_viewer(path),
                Effect::FindInViewer { id, query, from, direction, count } => {
                    self.find_in_viewer(id, query, from, direction, count);
                }
                Effect::StopFindInViewer => self.stop_viewer_search(),
                Effect::RemoteView { machine, session, path } => self.open_remote_viewer(machine, session, path),
                Effect::SetLanguage(locale) => borg_i18n::set_locale(locale),
                Effect::SaveConfig => self.save_config(),
                Effect::StartJob { id, spec } => self.start_job(id, spec),
                Effect::StartRemoteCopy { id, from, to, sources, target } => {
                    self.start_remote_copy(id, crate::remote_copy::CopyJob { from, to, sources, target });
                }
                Effect::StartDockerCopy { id, container, inbound, sources, target } => {
                    self.start_docker_copy(id, crate::docker::CopyJob { container, inbound, sources, target });
                }
                Effect::JobControl { id, command } => self.control_job(id, command),
                Effect::Peek { aside, path, size } => self.peek(aside, path, size),
                Effect::Quit => return Ok(Flow::Quit),
            }
        }
        Ok(Flow::Continue)
    }

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
        let running: Vec<u64> = self.state.jobs.iter().map(|job| job.id).collect();
        self.jobs.retain(|id, _| running.contains(id));
        let control = Arc::new(Control::default());
        self.jobs.insert(id, Arc::clone(&control));
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
        let running: Vec<u64> = self.state.jobs.iter().map(|job| job.id).collect();
        self.jobs.retain(|id, _| running.contains(id));
        let control = Arc::new(Control::default());
        self.jobs.insert(id, Arc::clone(&control));
        let messages = self.messages.clone();
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let outcome = job.run(&runtime, &control, &mut |event| drop(messages.send(Msg::Job { id, event })));
            drop(messages.send(Msg::Job { id, event: borg_jobs::Event::Finished(outcome) }));
        });
    }

    pub(super) fn start_remote_copy(&mut self, id: u64, job: crate::remote_copy::CopyJob) {
        let running: Vec<u64> = self.state.jobs.iter().map(|job| job.id).collect();
        self.jobs.retain(|id, _| running.contains(id));
        let control = Arc::new(Control::default());
        self.jobs.insert(id, Arc::clone(&control));
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

    /// Makes a pairing link for the Connect dialog, or says why there is none.
    pub(super) fn start_pairing(&mut self) {
        let pairing = if self.cloud_suppressed {
            Err("The background service or viewer owns no pairing connection; use borg service pair.".into())
        } else if !self.state.settings.preferences.agent_cloud {
            Err(tr!(agent_cloud_off).to_owned())
        } else if self.link.relay().is_none() {
            Err(tr!(agent_cloud_no_relay).to_owned())
        } else {
            self.link.pair().map_err(|error| format!("{error:#}"))
        };
        self.state.agent.pairing = Some(pairing);
        self.link.sync(self.state.settings.preferences.agent_cloud && !self.cloud_suppressed);
    }

    /// Saves the relay typed into the Connect dialog, empty for none, in its full form, and
    /// asks it for a pairing link.
    pub(super) fn set_relay(&mut self, text: &str) {
        let relay = borg_link::Relay::parse(text);
        if relay.is_none() && !text.is_empty() {
            self.state.agent.connect.relay = self.link.relay().map(ToString::to_string).unwrap_or_default();
            self.state.agent.pairing = Some(Err(tr!(agent_cloud_bad_relay).to_owned()));
            return;
        }
        let saved = relay.as_ref().map(ToString::to_string);
        self.state.agent.connect.relay = saved.clone().unwrap_or_default();
        if let Some(borg_core::Overlay::Connect(connect)) = self.state.overlay.as_mut() {
            connect.relay = borg_core::LineInput::new(self.state.agent.connect.relay.clone());
        }
        self.config.relay = saved;
        self.save_config();
        self.link.set_relay(relay);
        self.start_pairing();
    }

    pub(super) fn save_config(&self) {
        let appearance = &self.state.appearance;
        let config = self
            .config
            .capture(appearance.current_id(), &self.state.settings.preferences, appearance.unlocked())
            .with_search(&self.state.settings.search);
        let saved = self.config_dir.as_ref().map(|dir| dir.save(&config));
        if let Some(Err(e)) = saved {
            let _ = self.messages.send(Msg::Notice(format!("{e:#}")));
        }
    }

    /// Looks for `query` in the viewer's file on a thread of its own, saying how far it got
    /// when it takes a while.
    pub(super) fn find_in_viewer(
        &mut self,
        id: u64,
        query: borg_view::Query,
        from: u64,
        direction: borg_core::Direction,
        count: bool,
    ) {
        const QUIET: Duration = Duration::from_millis(300);
        const PROGRESS_EVERY: Duration = Duration::from_millis(150);
        self.stop_viewer_search();
        let Some(borg_core::Content::Text(pager)) = self.state.viewer.as_ref().map(|viewer| &viewer.content) else {
            return;
        };
        let source = pager.source();
        let stop = Arc::new(AtomicBool::new(false));
        self.viewer_search = Some(stop.clone());
        let messages = self.messages.clone();
        tokio::task::spawn_blocking(move || {
            let started = Instant::now();
            let said = std::cell::Cell::new(started);
            let len = source.len().max(1);
            let go_on = |at: u64| {
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    return false;
                }
                if started.elapsed() > QUIET && said.get().elapsed() > PROGRESS_EVERY {
                    said.set(Instant::now());
                    // How much of the file is behind the search, in the direction it reads.
                    let done = match direction {
                        borg_core::Direction::Forward => at.wrapping_sub(from).wrapping_add(len) % len,
                        borg_core::Direction::Backward => from.wrapping_sub(at).wrapping_add(len) % len,
                    };
                    let percent = (done.saturating_mul(100) / len).min(99) as u8;
                    let _ = messages.send(Msg::SearchingViewer { id, percent });
                }
                true
            };
            let found = borg_core::find_in(&*source, &query, from, direction, count, &go_on);
            if !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = messages.send(Msg::FoundInViewer { id, found: found.map_err(|error| error.to_string()) });
            }
        });
    }

    pub(super) fn stop_viewer_search(&mut self) {
        if let Some(stop) = self.viewer_search.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub(super) fn open_viewer(&self, path: PathBuf) {
        let (width, height) = viewer_size(self.state.cols, self.state.rows);
        let cell = self.graphics.capabilities().cell;
        let messages = self.messages.clone();
        tokio::task::spawn_blocking(move || {
            let content = crate::document::open(
                &path,
                (width, height),
                (cell.width, cell.height),
                Arc::new(AtomicBool::new(false)),
            );
            let opened = content.map(|content| Msg::Opened(Viewer::new(path.clone(), content))).unwrap_or_else(|e| {
                Msg::OpenFailed { notice: format!("{}: {e:#}", path.display()), path: path.clone() }
            });
            let _ = messages.send(opened);
        });
    }

    pub(super) fn open_remote_viewer(&self, machine: String, session: String, path: PathBuf) {
        let (width, height) = viewer_size(self.state.cols, self.state.rows);
        let cell = self.graphics.capabilities().cell;
        let state = self.state_dir.clone();
        let scratch = self.runtime_dir.join("remote-preview");
        let messages = self.messages.clone();
        tokio::spawn(async move {
            let opened: Result<borg_core::Content> = async {
                let state = state.context("Borg has no state folder for paired servers")?;
                let server = crate::link::server(state.path(), &machine)?;
                let mut remote = crate::link::Remote::open(state.path(), &server).await?;
                std::fs::create_dir_all(&scratch)?;
                let suffix = path
                    .extension()
                    .and_then(|part| part.to_str())
                    .filter(|part| part.len() <= 16 && part.bytes().all(|byte| byte.is_ascii_alphanumeric()))
                    .map_or(String::new(), |part| format!(".{part}"));
                let temporary =
                    tempfile::Builder::new().prefix("view-").suffix(&suffix).tempfile_in(&scratch)?.into_temp_path();
                crate::link::download_in_session(
                    &mut remote,
                    Some(&session),
                    &path.to_string_lossy(),
                    &temporary,
                    |_, _| {},
                )
                .await?;
                let local = temporary.to_path_buf();
                let content = tokio::task::spawn_blocking(move || {
                    crate::document::open(
                        &local,
                        (width, height),
                        (cell.width, cell.height),
                        Arc::new(AtomicBool::new(false)),
                    )
                })
                .await??;
                Ok(content)
            }
            .await;
            let msg = opened.map(|content| Msg::Opened(Viewer::new(path.clone(), content))).unwrap_or_else(|error| {
                Msg::OpenFailed { notice: format!("{} on {}: {error:#}", path.display(), machine), path: path.clone() }
            });
            let _ = messages.send(msg);
        });
    }

    pub(super) fn peek(&mut self, aside: Aside, path: PathBuf, size: (usize, usize)) {
        let cell = self.graphics.capabilities().cell;
        self.preview.request(aside, path, size, (cell.width, cell.height));
    }

    pub(super) fn list(&mut self, side: Side, generation: u64, dir: PathBuf) {
        let mount = self.state.panel(side).mount.clone();
        let remote = self.state.panel(side).remote_mount.clone();
        let mut batches = match (&mount, &remote) {
            (_, Some(remote)) => crate::remote::listing(remote.machine.clone(), remote.session.clone(), dir.clone()),
            (Some(mount), None) => crate::docker::listing(mount.id.clone(), dir.clone()),
            (None, None) => self.backend.list(&dir),
        };
        let messages = self.messages.clone();
        // Measured folder sizes belong to this machine, not to a container.
        let cache = (mount.is_none() && remote.is_none()).then(|| self.search.cache.clone());
        let task = tokio::spawn(async move {
            let observed_dir = dir.clone();
            let totals = match cache {
                Some(cache) => {
                    tokio::task::spawn_blocking(move || cache.totals(&observed_dir)).await.unwrap_or_default()
                }
                None => Default::default(),
            };
            while let Some(batch) = batches.next().await {
                let msg = match batch {
                    Ok(mut entries) => {
                        for entry in entries.iter_mut().filter(|e| e.kind == borg_vfs::EntryKind::Dir) {
                            if let Some((bytes, usage)) = totals.get(&entry.path(&dir)) {
                                entry.size = *bytes;
                                entry.usage = Some(usage.clone());
                            }
                        }
                        Msg::Batch { side, generation, entries }
                    }
                    Err(e) => {
                        let _ = messages.send(Msg::Listed { side, generation, error: Some(e.to_string()) });
                        return;
                    }
                };
                if messages.send(msg).is_err() {
                    return;
                }
            }
            let _ = messages.send(Msg::Listed { side, generation, error: None });
        });

        if let Some(previous) = self.listings[side.index()].replace(task) {
            previous.abort();
        }
    }
}
