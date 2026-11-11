//! A volume's files in a panel, read only, from where the engine keeps or shares them on this
//! machine. Copying out, viewing and searching work as in any folder; nothing changes them.

use super::close;
use crate::{Action, AppState, Effect, Side};
use borg_i18n::tr;
use std::path::{Path, PathBuf};

/// A Docker volume whose files a panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeMount {
    pub name: String,
    /// Where its files are on this machine; `..` here leaves the volume.
    pub root: PathBuf,
    /// The folder the panel showed before.
    pub host: PathBuf,
}

impl VolumeMount {
    /// Whether `dir` is inside the volume.
    pub fn holds(&self, dir: &Path) -> bool {
        dir.starts_with(&self.root)
    }

    /// `dir` as the volume sees it, from its top.
    pub fn inside(&self, dir: &Path) -> PathBuf {
        Path::new("/").join(dir.strip_prefix(&self.root).unwrap_or(dir))
    }
}

/// Shows the selected volume's files in the disk use view's panel and closes the Docker view.
pub(super) fn browse(state: &mut AppState, name: String, root: PathBuf) -> Vec<Effect> {
    let side = state.docker.side.unwrap_or(state.active);
    close(state);
    state.aside = None;
    state.active = side;
    let panel = state.panel(side);
    let host = panel.volume.as_ref().map_or_else(|| panel.dir().to_path_buf(), |volume| volume.host.clone());
    state.panel_mut(side).volume = Some(VolumeMount { name, root: root.clone(), host });
    vec![state.navigate(side, root, None)]
}

/// Leaves the volume for the folder the panel showed before.
pub(crate) fn unmount(state: &mut AppState, side: Side) -> Option<Effect> {
    let volume = state.panel_mut(side).volume.take()?;
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
        };
        assert!(volume.holds(Path::new("/home/me/OrbStack/docker/volumes/shop_data/base")));
        assert!(!volume.holds(Path::new("/home/me/OrbStack/docker/volumes/shop_static")));
        assert_eq!(volume.inside(Path::new("/home/me/OrbStack/docker/volumes/shop_data/base/1")), Path::new("/base/1"));
        assert_eq!(volume.inside(&volume.root), Path::new("/"));
    }
}
