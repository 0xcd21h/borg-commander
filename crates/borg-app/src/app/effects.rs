//! Carrying out the effects the core asks for, each in the part of the app that owns it; the
//! listings, saves and editor files are here.

use super::{App, Flow, Plain, TERMINAL_CLIPBOARD_LIMIT};
use anyhow::Result;
use borg_core::{Aside, EditorSlot, Effect, FileStamp, FirstMatch, Msg, Side, viewer_size};
use borg_i18n::tr;
use borg_term::Scroll;
use futures::StreamExt;
use std::path::PathBuf;

impl App {
    pub(super) fn apply(&mut self, effects: Vec<Effect>) -> Result<Flow> {
        for effect in effects {
            if let Flow::Quit = self.carry_out(effect)? {
                return Ok(Flow::Quit);
            }
        }
        Ok(Flow::Continue)
    }

    fn carry_out(&mut self, effect: Effect) -> Result<Flow> {
        match effect {
            Effect::CancelShellExit { token } => self.pty.write(format!("{token}:no\n").as_bytes())?,
            Effect::MeasureSizes { id, root } => self.search.measure(id, root),
            Effect::CancelMeasure => self.search.cancel_measure(),
            Effect::CancelMemorySearch => self.monitor.cancel_search(),
            Effect::SizeChildren { side, generation, root, hidden, parent } => {
                self.size_children(side, generation, root, hidden, parent)
            }
            Effect::Activity { id, request } => self.monitor.request(id, request, self.state.activity.selected),
            Effect::Backup(request) => self.backup.request(request),
            Effect::Docker { id, request } => self.docker.request(id, request),
            Effect::DockerWatch(on) => self.docker.watch(on),
            Effect::DockerShell(request) => self.container_shell(request),
            Effect::ExportMemory { path, bytes } => self.export_memory(path, bytes),
            Effect::SaveGame2048 => self.save_game2048(),
            Effect::Search { side, generation, request } => self.search.search(side, generation, request),
            Effect::RefreshCollection { side, generation, entries } => self.search.refresh(side, generation, entries),
            Effect::CancelSearch(side) => self.search.cancel(side),
            Effect::SetIndexRoots(roots) => self.set_index_roots(roots),
            Effect::SaveSearchHistory => self.save_search_history(),
            Effect::SaveCollections => self.save_collections(),
            Effect::List { side, generation, dir } => self.list(side, generation, dir),
            Effect::GitScan { side, generation, dir } => self.plugins.scan(side, generation, dir),
            Effect::GitTool { id, tool, dir, paths } => self.run_git_tool(id, tool, dir, paths),
            Effect::Write(bytes) => self.write_console(bytes)?,
            Effect::OpenEditor { path, slot } => self.read_for_editor(path, slot),
            Effect::SaveFile { path, bytes, expected, force, slot } => {
                self.save_file(path, bytes, expected, force, slot)
            }
            Effect::WriteConcealed(bytes) => {
                self.console_emulator().conceal();
                self.write_console(bytes)?;
            }
            Effect::ChangeShellDir { dir, typed } => self.change_shell_dir(&dir, &typed)?,
            Effect::ScrollConsole(scroll) => self.console_emulator().scroll(scroll),
            Effect::UseConsole(console) => self.use_console(console),
            Effect::MirrorRemote { machine, session } => self.mirror_remote(machine, session),
            Effect::CancelRemoteConnection => self.mirror_connect = None,
            Effect::ForgetRemoteHost { machine } => self.forget_remote_host(machine),
            Effect::DeleteRemoteSession { machine, session } => self.delete_remote_session(machine, session),
            Effect::CloseRemoteConsole { machine, session } => self.remote_consoles.close(&(machine, session)),
            Effect::SaveRemoteName { identity, session, name } => self.save_remote_name(identity, session, name),
            Effect::OpenRemotes => self.open_remotes(),
            Effect::MountRemote { side, machine, session, name } => {
                return self.mount_remote(side, machine, session, name);
            }
            Effect::ApprovalModeOff => self.approval_mode_off(),
            Effect::Copy(text) => self.copy(text),
            Effect::ShowPlain(text) => self.plain = Some(Plain::Waiting(text)),
            Effect::StartPairing => self.start_pairing(),
            Effect::SetRelay(text) => self.set_relay(&text),
            Effect::ForgetComputers => self.forget_computers(),
            Effect::AgentAnswer { id, approved } => return self.agent_answer(id, approved),
            Effect::View(path) => self.open_viewer(path),
            Effect::FindInViewer { id, query, from, direction, count } => {
                self.find_in_viewer(id, query, from, direction, count)
            }
            Effect::StopFindInViewer => self.stop_viewer_search(),
            Effect::RemoteView { machine, session, path } => self.open_remote_viewer(machine, session, path),
            Effect::SetLanguage(locale) => borg_i18n::set_locale(locale),
            Effect::SaveConfig => self.save_config(),
            Effect::StartJob { id, spec } => self.start_job(id, spec),
            Effect::StartRemoteCopy { id, from, to, sources, target } => {
                self.start_remote_copy(id, crate::remote_copy::CopyJob { from, to, sources, target })
            }
            Effect::StartDockerCopy { id, container, inbound, sources, target } => {
                self.start_docker_copy(id, crate::docker::CopyJob { container, inbound, sources, target })
            }
            Effect::JobControl { id, command } => self.control_job(id, command),
            Effect::Peek { aside, path, at, size } => self.peek(aside, path, at, size),
            Effect::Quit => return Ok(Flow::Quit),
        }
        Ok(Flow::Continue)
    }

    /// Reads the measured sizes of `parent`'s children for the size view, off the event loop.
    fn size_children(&self, side: Side, generation: u64, root: PathBuf, hidden: bool, parent: PathBuf) {
        let cache = self.search.cache.clone();
        let messages = self.messages.clone();
        tokio::task::spawn_blocking(move || {
            let result = cache.children(&root, hidden, &parent).ok_or_else(|| tr!(size_branch_unavailable).to_owned());
            let _ = messages.send(Msg::SizeChildren { side, generation, parent, result });
        });
    }

    fn export_memory(&self, path: PathBuf, bytes: Vec<u8>) {
        let messages = self.messages.clone();
        tokio::task::spawn_blocking(move || {
            let notice = match crate::activity::export(&path, &bytes) {
                Ok(()) => tr!(activity_exported, path = path.display()),
                Err(error) => tr!(activity_export_error, error = error),
            };
            let _ = messages.send(Msg::Notice(notice));
        });
    }

    fn save_game2048(&mut self) {
        if let (Some(dir), Some(game)) = (&self.state_dir, &self.state.game2048)
            && let Err(error) = dir.save_game2048(game)
        {
            self.state.notice = Some(tr!(game2048_save_error, error = error));
        }
    }

    /// Indexes the folders that are new among `roots`, and remembers them all in the config.
    fn set_index_roots(&mut self, roots: Vec<PathBuf>) {
        let added: Vec<_> = roots.iter().filter(|root| !self.search.indexes.roots().contains(root)).cloned().collect();
        self.search.indexes.set_roots(roots.clone());
        for root in added {
            self.search.indexes.refresh(root, self.state.settings.search.clone());
        }
        self.config.set_index_roots(&roots);
        self.save_config();
    }

    fn save_search_history(&mut self) {
        if let Some(dir) = &self.state_dir
            && let Err(error) = dir.save_search_history(&self.state.search_history)
        {
            self.state.notice = Some(tr!(search_history_save_error, error = error));
        }
    }

    /// Saves the collections, unless the saved ones could not be read, which stay as they are.
    fn save_collections(&mut self) {
        if !self.collections_writable {
            self.state.notice = Some(tr!(collections_preserved).into());
            return;
        }
        if let Some(dir) = &self.state_dir
            && let Err(error) = dir.save_collections(&self.state.collections)
        {
            self.state.notice = Some(tr!(collections_save_error, error = error));
        }
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

    fn run_git_tool(&self, id: u64, tool: borg_git::Tool, dir: PathBuf, paths: Vec<PathBuf>) {
        let messages = self.messages.clone();
        let size = viewer_size(self.state.cols, self.state.rows);
        tokio::spawn(async move {
            let result = crate::plugins::run(dir, tool, paths, size).await.map_err(|error| format!("{error:#}"));
            let _ = messages.send(Msg::GitFinished { id, result });
        });
    }

    fn read_for_editor(&self, path: PathBuf, slot: EditorSlot) {
        let messages = self.messages.clone();
        tokio::task::spawn_blocking(move || {
            let _ = messages.send(Msg::EditorOpened(slot, crate::edit_file::read(&path)));
        });
    }

    fn save_file(&self, path: PathBuf, bytes: Vec<u8>, expected: FileStamp, force: bool, slot: EditorSlot) {
        let messages = self.messages.clone();
        tokio::task::spawn_blocking(move || {
            let saved = crate::edit_file::write(&path, &bytes, expected, force);
            let _ = messages.send(Msg::EditorSaved(slot, saved));
        });
    }

    /// Takes the shell to `dir` without a command line of its own, or types `typed` when the
    /// shell has no quiet way in.
    fn change_shell_dir(&mut self, dir: &std::path::Path, typed: &[u8]) -> std::io::Result<()> {
        self.emulator.scroll(Scroll::Bottom);
        match borg_term::request_cd(&self.cd_request, dir) {
            Ok(()) => self.pty.write(borg_term::SILENT_CD_KEY),
            Err(_) => self.pty.write(typed),
        }
    }

    /// Puts `text` on the computer's clipboard through the terminal, unless it is too long for
    /// terminals to take; the core has kept it in Borg's either way.
    fn copy(&mut self, text: String) {
        if text.len() > TERMINAL_CLIPBOARD_LIMIT {
            self.state.notice = Some(tr!(clipboard_borg_only).into());
            return;
        }
        self.native_mirror.copy(&text);
        self.clipboard = Some(text);
    }

    pub(super) fn peek(&mut self, aside: Aside, path: PathBuf, at: Option<FirstMatch>, size: (usize, usize)) {
        let cell = self.graphics.capabilities().cell;
        self.preview.request(aside, path, at, size, (cell.width, cell.height));
    }

    /// Lists `dir` into the panel on `side`, from this machine, a container or a remote Borg,
    /// in place of any listing or search that panel had going.
    pub(super) fn list(&mut self, side: Side, generation: u64, dir: PathBuf) {
        self.search.cancel(side);
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
