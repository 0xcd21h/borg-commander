//! A volume's files in a panel, read only: from where the engine keeps or shares them on this
//! machine, or through a helper container that mounts the volume read only. Copying out and
//! viewing work as in any folder; nothing changes them.

use super::{DockerData, DockerRequest, Mount, close};
use crate::{Action, AppState, Confirm, ConfirmPurpose, Effect, Overlay, Side};
use borg_docker::helper;
use borg_i18n::tr;
use std::path::{Path, PathBuf};

/// A Docker volume whose files a panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeMount {
    pub name: String,
    /// Where its files are, on this machine or in the helper; `..` here leaves the volume.
    pub root: PathBuf,
    /// The folder the panel showed before.
    pub host: PathBuf,
    /// The helper container that shows it, which the panel's container mount names; `None`
    /// when the panel reads the volume's folder on this machine.
    pub helper: Option<String>,
}

impl VolumeMount {
    /// Whether `dir` is inside the volume.
    pub fn holds(&self, dir: &Path) -> bool {
        dir.starts_with(&self.root)
    }

    /// Whether the panel's container mount is the one that shows the volume: its helper, or
    /// none for a folder on this machine.
    pub fn shown_by(&self, mount: Option<&Mount>) -> bool {
        self.helper.as_deref() == mount.map(|mount| mount.id.as_str())
    }

    /// `dir` as the volume sees it, from its top.
    pub fn inside(&self, dir: &Path) -> PathBuf {
        Path::new("/").join(dir.strip_prefix(&self.root).unwrap_or(dir))
    }
}

/// A helper container on its way to showing a volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opening {
    pub id: u64,
    pub name: String,
    /// Whether the engine downloads the helper's image first.
    pub pull: bool,
}

/// Shows the selected volume's files in the disk use view's panel and closes the Docker view:
/// from `root` on this machine, or from `root` in the `helper` container.
fn show(state: &mut AppState, name: String, root: PathBuf, helper: Option<String>) -> Vec<Effect> {
    let side = state.docker.side.unwrap_or(state.active);
    close(state);
    state.aside = None;
    state.active = side;
    let panel = state.panel(side);
    let host = panel
        .mount
        .as_ref()
        .map(|mount| mount.host.clone())
        .or_else(|| panel.volume.as_ref().map(|volume| volume.host.clone()))
        .unwrap_or_else(|| panel.dir().to_path_buf());
    let mount = helper.clone().map(|id| Mount { id, name: name.clone(), host: host.clone() });
    let panel = state.panel_mut(side);
    panel.mount = mount;
    panel.volume = Some(VolumeMount { name, root: root.clone(), host, helper });
    vec![state.navigate(side, root, None)]
}

/// Opens the selected volume: its folder here when Borg can read it, or else a helper
/// container, after asking to download the helper's image when the engine lacks it.
pub(super) fn browse(state: &mut AppState) -> Vec<Effect> {
    let disk = &state.docker.disk;
    let Some(volume) = disk.volume() else { return Vec::new() };
    let name = volume.name.clone();
    if let Some(folder) = disk.folders.get(&name).cloned() {
        return show(state, name, folder, None);
    }
    if disk.opening.is_some() {
        return Vec::new();
    }
    if disk.usage.as_ref().is_some_and(|usage| usage.engine.helper) {
        return open(state, name, false);
    }
    state.overlay = Some(Overlay::Confirm(Confirm {
        title: tr!(docker_helper_title),
        message: tr!(docker_helper_confirm, image = helper::IMAGE_NAME),
        purpose: ConfirmPurpose::VolumeHelper { volume: name },
        accept: true,
    }));
    Vec::new()
}

/// Starts a helper container for `name`, downloading its image first when `pull`.
pub(crate) fn open(state: &mut AppState, name: String, pull: bool) -> Vec<Effect> {
    if !state.docker.disk.open {
        return Vec::new();
    }
    state.docker.request(DockerRequest::OpenVolume { name, pull })
}

/// Shows the volume the helper container `result` names, while disk use still waits for it.
pub(super) fn opened(state: &mut AppState, opening: Opening, result: Result<DockerData, String>) -> Vec<Effect> {
    match result {
        Ok(DockerData::Helper(container)) if state.docker.disk.open => {
            if let Some(usage) = state.docker.disk.usage.as_mut() {
                usage.engine.helper = true;
            }
            show(state, opening.name, PathBuf::from(helper::ROOT), Some(container))
        }
        Err(error) => {
            state.notice = Some(tr!(docker_action_failed, error = error));
            Vec::new()
        }
        Ok(_) => Vec::new(),
    }
}

/// Removes the helper containers no panel shows any more.
pub(crate) fn release(state: &mut AppState) -> Vec<Effect> {
    let shown: Vec<String> =
        state.panels.iter().filter_map(|panel| panel.mount.as_ref()).map(|mount| mount.id.clone()).collect();
    let (kept, unused): (Vec<String>, Vec<String>) =
        std::mem::take(&mut state.docker.helpers).into_iter().partition(|id| shown.contains(id));
    state.docker.helpers = kept;
    unused.into_iter().flat_map(|id| state.docker.request(DockerRequest::CloseVolume(id))).collect()
}

/// Leaves the volume for the folder the panel showed before.
pub(crate) fn unmount(state: &mut AppState, side: Side) -> Option<Effect> {
    let panel = state.panel_mut(side);
    let volume = panel.volume.take()?;
    if volume.helper.is_some() {
        panel.mount = None;
    }
    Some(state.navigate(side, volume.host, None))
}

/// Whether the panel on `side` shows the volume's top, which `..` leaves.
pub(crate) fn at_root(state: &AppState, side: Side) -> bool {
    let panel = state.panel(side);
    panel.volume.as_ref().is_some_and(|volume| panel.dir() == volume.root)
}

/// Actions that change the files of the active panel's folder.
fn changes_here(action: Action) -> bool {
    matches!(
        action,
        Action::Move
            | Action::Delete
            | Action::DeleteOriginals
            | Action::Edit
            | Action::EditInPanel
            | Action::EditExternal
            | Action::Mkdir
            | Action::Compress
            | Action::ExtractHere
    )
}

/// Actions that write into the other panel's folder.
fn changes_other(action: Action) -> bool {
    matches!(action, Action::Copy | Action::Move | Action::ExtractToOther | Action::Collect)
}

/// What Borg says instead of `action` when it would change a volume's files.
pub(crate) fn refusal(state: &AppState, action: Action) -> Option<&'static str> {
    let here = state.panel(state.active).volume.is_some() && changes_here(action);
    let there = state.panel(state.active.other()).volume.is_some() && changes_other(action);
    (here || there).then(|| tr!(docker_volume_read_only))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_volume_shows_its_folders_from_its_own_top() {
        let volume = VolumeMount {
            name: "shop_data".into(),
            root: "/home/me/OrbStack/docker/volumes/shop_data".into(),
            host: "/home/me".into(),
            helper: None,
        };
        assert!(volume.holds(Path::new("/home/me/OrbStack/docker/volumes/shop_data/base")));
        assert!(!volume.holds(Path::new("/home/me/OrbStack/docker/volumes/shop_static")));
        assert_eq!(volume.inside(Path::new("/home/me/OrbStack/docker/volumes/shop_data/base/1")), Path::new("/base/1"));
        assert_eq!(volume.inside(&volume.root), Path::new("/"));
    }
}
