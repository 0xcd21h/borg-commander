//! Opening files in the viewer, from this machine or a paired server, and searching them off
//! the event loop.

use super::App;
use anyhow::{Context, Result};
use borg_core::{Msg, Viewer, viewer_size};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

impl App {
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
                if stop.load(Ordering::Relaxed) {
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
            if !stop.load(Ordering::Relaxed) {
                let _ = messages.send(Msg::FoundInViewer { id, found: found.map_err(|error| error.to_string()) });
            }
        });
    }

    pub(super) fn stop_viewer_search(&mut self) {
        if let Some(stop) = self.viewer_search.take() {
            stop.store(true, Ordering::Relaxed);
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
}
