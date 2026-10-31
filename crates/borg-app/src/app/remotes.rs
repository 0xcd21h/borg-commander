//! Remote Borgs: their catalog and names, their consoles and mirrors, and their folders in a panel.

use super::{App, Flow, REMOTE_CLIENT_REFRESH};
use crate::session::StateDir;
use anyhow::{Context, Result};
use borg_core::{Console, Msg, Side, update};
use borg_i18n::tr;
use borg_term::GridSize;
use std::time::Instant;

impl App {
    /// Shows `console` below the panels, connecting a remote one's terminal first.
    pub(super) fn use_console(&mut self, console: Console) {
        if let Console::Remote { machine, session } = &console {
            self.remote_consoles.open(
                (machine.clone(), session.clone()),
                self.state.active_panel().destination().into(),
                GridSize::console(self.state.cols, self.state.rows),
                self.state.settings.scrollback,
            );
        }
        self.console = console;
    }

    pub(super) fn mirror_remote(&mut self, machine: String, session: String) {
        self.mirror_connect =
            Some(Box::pin(async move { crate::mirror::connect(&format!("{machine}:{session}"), false).await }));
    }

    /// Lists the remote Borgs and their names, and keeps the computers paired here current
    /// while the list is open.
    pub(super) fn open_remotes(&mut self) {
        let messages = self.messages.clone();
        if let Some(previous) = self.remote_catalog.take() {
            previous.abort();
        }
        self.remote_catalog = Some(tokio::spawn(async move {
            if let Some(dir) = StateDir::locate() {
                let names =
                    crate::remote_names::load(dir.path()).map_err(|error| tr!(remote_names_error, error = error));
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

    pub(super) fn forget_remote_host(&mut self, machine: String) {
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

    pub(super) fn delete_remote_session(&self, machine: String, session: String) {
        let messages = self.messages.clone();
        tokio::spawn(async move {
            let result = crate::remote::delete_session(&machine, &session).await.map_err(|error| format!("{error:#}"));
            let _ = messages.send(Msg::RemoteSessionDeleted { machine, session, result });
        });
    }

    pub(super) fn save_remote_name(&self, identity: String, session: Option<String>, name: String) {
        let messages = self.messages.clone();
        tokio::task::spawn_blocking(move || {
            let result = StateDir::locate()
                .context("Borg has no state folder")
                .and_then(|state| crate::remote_names::rename(state.path(), &identity, session.as_deref(), &name))
                .map_err(|error| tr!(remote_names_error, error = error));
            let _ = messages.send(Msg::RemoteNames(result));
        });
    }

    /// Shows a remote session's folder in the panel on `side`, which the core does once it is asked.
    pub(super) fn mount_remote(&mut self, side: Side, machine: String, session: String, name: String) -> Result<Flow> {
        let effects = update(&mut self.state, Msg::MountRemote { side, machine, session, name }, Instant::now());
        self.apply(effects)
    }
}
