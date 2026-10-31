//! A container's files in a panel: browsing them, and copying, deleting, making folders and viewing there.

use super::{DockerRequest, FileOp, FileWork, Mount, close};
use crate::{AppState, Effect, Side};
use borg_docker::State;
use borg_i18n::tr;
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

/// The container a panel shows, if it shows one's filesystem.
pub(crate) fn mounted(state: &AppState, side: Side) -> Option<&Mount> {
    state.panel(side).mount.as_ref()
}

pub(super) fn file_work(state: &mut AppState, side: Side, container: String, op: FileOp) -> Vec<Effect> {
    let docker = &mut state.docker;
    docker.next_id += 1;
    let id = docker.next_id;
    docker.files.push(FileWork { id, side, op: op.clone() });
    vec![Effect::Docker { id, request: DockerRequest::Files { container, op } }]
}

/// Shows the selected container's filesystem in the Docker list's panel, starting in its
/// working directory, and closes the Docker view. The other panel keeps its host folder,
/// so files copy between the two.
pub(crate) fn browse(state: &mut AppState) -> Vec<Effect> {
    let Some(container) = state.docker.container().cloned() else {
        state.notice = Some(tr!(docker_choose_container).into());
        return Vec::new();
    };
    if container.state != State::Running {
        state.notice = Some(tr!(docker_start_to_browse).into());
        return Vec::new();
    }
    let workdir = state.docker.selected_details().map(|d| d.workdir.clone()).filter(|dir| dir.starts_with('/'));
    let side = state.docker.side.unwrap_or(state.active);
    close(state);
    state.aside = None;
    state.active = side;
    let panel = state.panel(side);
    let host = panel.mount.as_ref().map_or_else(|| panel.dir().to_path_buf(), |mount| mount.host.clone());
    state.panel_mut(side).mount = Some(Mount { id: container.id, name: container.name, host });
    vec![state.navigate(side, PathBuf::from(workdir.unwrap_or_else(|| "/".into())), None)]
}

/// Leaves a container's filesystem for the host folder the panel showed before.
pub(crate) fn unmount(state: &mut AppState, side: Side) -> Option<Effect> {
    let mount = state.panel_mut(side).mount.take()?;
    Some(state.navigate(side, mount.host, None))
}

/// Copies the selection between a container panel and a host panel as a copy job, which asks
/// before replacing what exists; `target` is a folder in the other panel, absolute or
/// relative to its folder.
pub(crate) fn copy(state: &mut AppState, sources: Vec<PathBuf>, target: &str, now: Instant) -> Vec<Effect> {
    let (from, to) = (state.active, state.active.other());
    let target = state.panel(to).dir().join(target);
    let (container, inbound) = match (mounted(state, from), mounted(state, to)) {
        (Some(mount), None) => (mount.id.clone(), false),
        (None, Some(mount)) => (mount.id.clone(), true),
        _ => {
            state.notice = Some(tr!(docker_copy_between_containers).into());
            return Vec::new();
        }
    };
    let (id, mut effects) = crate::jobs::begin(state, crate::JobKind::Copy, now);
    effects.push(Effect::StartDockerCopy { id, container, inbound, sources, target });
    effects
}

pub(crate) fn delete(state: &mut AppState, paths: Vec<PathBuf>) -> Vec<Effect> {
    let side = state.active;
    let Some(mount) = mounted(state, side).cloned() else { return Vec::new() };
    file_work(state, side, mount.id, FileOp::Delete { paths })
}

pub(crate) fn mkdir(state: &mut AppState, name: &str) -> Vec<Effect> {
    let side = state.active;
    let Some(mount) = mounted(state, side).cloned() else { return Vec::new() };
    let path = state.panel(side).dir().join(name);
    file_work(state, side, mount.id, FileOp::Mkdir { path })
}

/// Opens a container file in the viewer, once copied out.
pub(crate) fn view(state: &mut AppState, path: &Path) -> Vec<Effect> {
    let side = state.active;
    let Some(mount) = mounted(state, side).cloned() else { return Vec::new() };
    file_work(state, side, mount.id, FileOp::Fetch { path: path.to_path_buf() })
}
