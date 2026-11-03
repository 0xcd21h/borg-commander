//! Archives shown in a panel as folders: entering and leaving one, its password, and copying
//! files out of it, which unpacks them as a job. Archives open read only.

use std::{
    fmt,
    path::{Path, PathBuf},
    time::Instant,
};

use borg_i18n::{Count, count, tr};
use borg_vfs::Entry;

use crate::{AppState, Effect, JobKind, LineInput, Overlay, Prompt, PromptPurpose, Side};

const SEPARATOR: &str = "/";
const HIDDEN: &str = "<hidden>";
const SUFFIX_DOT: char = '.';

/// A password given for an archive, kept in memory while a panel shows the archive and never
/// printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Password(String);

impl Password {
    pub fn new(text: &str) -> Self {
        Self(text.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(HIDDEN)
    }
}

/// An archive a panel shows as a folder. The panel's folder is the archive's path followed by
/// the folder inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveMount {
    pub archive: PathBuf,
    /// The folder to return to when leaving the archive's top.
    pub host: PathBuf,
    pub password: Option<Password>,
    /// Whether the password opened the archive's encrypted entries.
    pub verified: bool,
    /// How much of the archive is read so far while it is listed for the first time, and its size.
    pub reading: Option<(u64, u64)>,
    /// What the archive turned out to be, once it opened.
    pub format: Option<borg_archive::Format>,
}

impl ArchiveMount {
    fn new(archive: PathBuf, host: PathBuf) -> Self {
        Self { archive, host, password: None, verified: false, reading: None, format: None }
    }

    /// The path inside the archive that `path` names, `""` for the archive's top.
    pub fn inside(&self, path: &Path) -> Option<String> {
        let rest = path.strip_prefix(&self.archive).ok()?;
        let parts: Vec<String> = rest.iter().map(|part| part.to_string_lossy().into_owned()).collect();
        Some(parts.join(SEPARATOR))
    }

    /// Whether `dir` lies in the archive.
    pub fn holds(&self, dir: &Path) -> bool {
        dir.starts_with(&self.archive)
    }

    /// What the archive is, as the panel's badge names it: the format it opened as, or the
    /// ending of its name until it opens.
    pub fn label(&self) -> &'static str {
        let ending = || self.archive.file_name()?.to_str().and_then(borg_archive::suffix);
        let label = self.format.map(borg_archive::Format::label).or_else(ending).unwrap_or_default();
        label.trim_start_matches(SUFFIX_DOT)
    }

    fn name(&self) -> String {
        self.archive.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

/// Where an unpack job writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnpackTarget {
    Folder(PathBuf),
    /// Borg's own temporary folder, to show the one file in the viewer as `shown`.
    Viewer {
        shown: PathBuf,
    },
}

/// Entries to unpack from an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpackRequest {
    pub archive: PathBuf,
    pub password: Option<Password>,
    /// Paths inside the archive; `""` stands for all of it.
    pub paths: Vec<String>,
    /// The folder inside the archive the paths were chosen in, which they land without.
    pub base: String,
    pub target: UnpackTarget,
}

/// Whether Enter shows what `entry` holds, by its name.
pub(crate) fn opens(entry: &Entry) -> bool {
    !entry.is_dir() && borg_archive::named(&entry.name)
}

/// Shows the archive at `archive` in the panel on `side`, from a folder of this machine.
pub(crate) fn enter(state: &mut AppState, side: Side, archive: PathBuf) -> Vec<Effect> {
    let panel = state.panel(side);
    if panel.archive.is_some() {
        state.notice = Some(tr!(archive_nested).into());
        return Vec::new();
    }
    let host = archive.parent().map_or_else(|| panel.dir().to_path_buf(), Path::to_path_buf);
    state.panel_mut(side).archive = Some(ArchiveMount::new(archive.clone(), host));
    vec![state.navigate(side, archive, None)]
}

/// Leaves the archive for the folder that holds it, on the archive.
pub(crate) fn unmount(state: &mut AppState, side: Side) -> Option<Effect> {
    let mount = state.panel_mut(side).archive.take()?;
    let focus = Some(mount.name());
    Some(state.navigate(side, mount.host, focus))
}

/// Whether the panel on `side` shows the archive's top, which `..` leaves.
pub(crate) fn at_top(state: &AppState, side: Side) -> bool {
    let panel = state.panel(side);
    panel.archive.as_ref().is_some_and(|mount| panel.dir() == mount.archive)
}

/// Forgets the archive once the panel's folder is no longer in it, as after a listing of the
/// archive failed and the panel kept the folder it showed.
pub(crate) fn settle(state: &mut AppState, side: Side) {
    let panel = state.panel_mut(side);
    if panel.archive.as_ref().is_some_and(|mount| !mount.holds(panel.destination())) {
        panel.archive = None;
    }
}

fn listing_mount(state: &mut AppState, side: Side, generation: u64) -> Option<&mut ArchiveMount> {
    let panel = state.panel_mut(side);
    if panel.generation() != generation {
        return None;
    }
    panel.archive.as_mut()
}

/// How far the first read of the archive got.
pub(crate) fn reading(state: &mut AppState, side: Side, generation: u64, read: u64, total: u64) {
    if let Some(mount) = listing_mount(state, side, generation) {
        mount.reading = Some((read, total));
    }
}

/// The archive opened as `format`; `left_out` entries had names or kinds Borg does not unpack.
pub(crate) fn opened(state: &mut AppState, side: Side, generation: u64, format: borg_archive::Format, left_out: usize) {
    let Some(mount) = listing_mount(state, side, generation) else { return };
    mount.reading = None;
    mount.format = Some(format);
    mount.verified = mount.password.is_some();
    if left_out > 0 {
        state.notice = Some(tr!(archive_left_out, entries = count(left_out, Count::Item)));
    }
}

/// The archive needs a password, or a better one than it was given.
pub(crate) fn locked(state: &mut AppState, side: Side, generation: u64, wrong: bool) {
    let Some(mount) = listing_mount(state, side, generation) else { return };
    mount.reading = None;
    mount.password = None;
    mount.verified = false;
    let name = mount.name();
    let label = if wrong { tr!(archive_password_wrong, name = name) } else { tr!(archive_password_for, name = name) };
    state.overlay = Some(Overlay::Prompt(Prompt {
        title: tr!(archive_password_title),
        label,
        input: LineInput::default(),
        purpose: PromptPurpose::ArchivePassword { side },
        option_focused: false,
    }));
}

/// Opens the archive again with `password`.
pub(crate) fn unlock(state: &mut AppState, side: Side, password: &str) -> Vec<Effect> {
    let Some(mount) = state.panel_mut(side).archive.as_mut() else { return Vec::new() };
    mount.password = Some(Password::new(password));
    let dir = state.panel(side).destination().to_path_buf();
    vec![state.navigate(side, dir, None)]
}

/// Esc while the archive is read for the first time: leaves it.
pub(crate) fn stop_reading(state: &mut AppState) -> Option<Effect> {
    let side = state.active;
    let reading = state.panel(side).archive.as_ref().is_some_and(|mount| mount.reading.is_some());
    if reading { unmount(state, side) } else { None }
}

/// The path inside the archive of the folder the panel on `side` shows.
fn shown_folder(state: &AppState, side: Side) -> Option<(ArchiveMount, String)> {
    let panel = state.panel(side);
    let mount = panel.archive.clone()?;
    let base = mount.inside(panel.dir())?;
    Some((mount, base))
}

/// F3 and Enter on a file in an archive: unpacks it into Borg's temporary folder and shows it.
pub(crate) fn view(state: &mut AppState, side: Side, now: Instant) -> Vec<Effect> {
    let Some((mount, base)) = shown_folder(state, side) else { return Vec::new() };
    let panel = state.panel(side);
    let Some(shown) = panel.hovered_entry().filter(|entry| !entry.is_dir()).map(|entry| entry.path(panel.dir())) else {
        return Vec::new();
    };
    let Some(path) = mount.inside(&shown) else { return Vec::new() };
    state.opening = Some(shown.clone());
    let id = crate::jobs::begin_view(state, shown.clone(), now);
    vec![Effect::StartUnpack {
        id,
        request: UnpackRequest {
            archive: mount.archive,
            password: mount.password,
            paths: vec![path],
            base,
            target: UnpackTarget::Viewer { shown },
        },
    }]
}

/// Whether the panel on `side` can take files copied out of an archive.
pub(crate) fn takes_copies(state: &AppState, side: Side) -> bool {
    let panel = state.panel(side);
    !panel.foreign() && !panel.is_results()
}

/// F5 from an archive, once the prompt names the folder `target`: unpacks `sources`, paths the
/// panel shows, there.
pub(crate) fn unpack(state: &mut AppState, sources: &[PathBuf], target: PathBuf, now: Instant) -> Vec<Effect> {
    let side = state.active;
    let Some((mount, base)) = shown_folder(state, side) else { return Vec::new() };
    let paths = sources.iter().filter_map(|source| mount.inside(source)).collect();
    let (id, mut effects) = crate::jobs::begin(state, JobKind::Unpack, now);
    effects.push(Effect::StartUnpack {
        id,
        request: UnpackRequest {
            archive: mount.archive,
            password: mount.password,
            paths,
            base,
            target: UnpackTarget::Folder(target),
        },
    });
    effects
}

/// Extract here and Extract to other panel: unpacks all of the hovered archive into the folder
/// of the panel on `to`.
pub(crate) fn extract(state: &mut AppState, to: Side, now: Instant) -> Vec<Effect> {
    let panel = state.active_panel();
    let Some(archive) = panel.hovered_entry().filter(|entry| opens(entry)).map(|entry| entry.path(panel.dir())) else {
        return Vec::new();
    };
    if !takes_copies(state, to) {
        state.notice = Some(tr!(archive_copy_destination).into());
        return Vec::new();
    }
    let target = state.panel(to).dir().to_path_buf();
    let (id, mut effects) = crate::jobs::begin_on(state, JobKind::Unpack, to, now);
    effects.push(Effect::StartUnpack {
        id,
        request: UnpackRequest {
            archive,
            password: None,
            paths: vec![String::new()],
            base: String::new(),
            target: UnpackTarget::Folder(target),
        },
    });
    effects
}

#[cfg(test)]
mod tests;
