//! Archives shown in a panel as folders: entering and leaving one, its password, copying files
//! out of it, which unpacks them as a job, and changing it: files copied or moved in, entries
//! moved, renamed and deleted, new folders, and files edited and saved back. Borg changes zip, 7z
//! and tar archives; RAR and single compressed files stay read only.

use std::{
    fmt,
    path::{Path, PathBuf},
    time::Instant,
};

use borg_i18n::{Count, count, tr};
use borg_vfs::Entry;

use borg_archive::Change;
use borg_jobs::Spec;

use crate::{AppState, EditorSlot, Effect, JobKind, LineInput, Overlay, Prompt, PromptPurpose, Side};

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
    /// Borg's own temporary folder, to edit the one file in the editor in `slot`; each save
    /// writes it back into the archive.
    Editor {
        shown: PathBuf,
        slot: EditorSlot,
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
    /// Whether the entries leave the archive once unpacked, as a move does: each entry that
    /// unpacked whole.
    pub remove: bool,
}

/// A change to an archive, run as a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteRequest {
    pub archive: PathBuf,
    pub password: Option<Password>,
    pub change: Change,
    /// Files of this machine moved into the archive, each deleted once it is in it whole.
    pub originals: Vec<PathBuf>,
}

/// A file of an archive open in the editor, which each save writes back into the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedFile {
    pub archive: PathBuf,
    pub password: Option<Password>,
    /// The entry's path inside the archive.
    pub entry: String,
    /// The path the panel shows for the entry.
    pub shown: PathBuf,
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
        typed: None,
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
            remove: false,
        },
    }]
}

/// F4 and Alt-E on a file in an archive: unpacks it into Borg's temporary folder for the editor
/// in `slot`, which writes each save back into the archive.
pub(crate) fn edit(state: &mut AppState, slot: EditorSlot, now: Instant) -> Vec<Effect> {
    let side = state.active;
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
            target: UnpackTarget::Editor { shown, slot },
            remove: false,
        },
    }]
}

/// The editor in `slot` opened a file unpacked from an archive.
pub(crate) fn editor_opened(
    state: &mut AppState,
    slot: EditorSlot,
    packed: PackedFile,
    result: Result<crate::EditorFile, crate::OpenError>,
) -> Vec<Effect> {
    if state.opening.as_ref() == Some(&packed.shown) {
        state.opening = None;
    }
    let effects = crate::editor::opened(state, slot, result);
    crate::editor::pack(state, slot, packed);
    effects
}

/// Writes the file the editor saved at `source` back into the archive it came from.
pub(crate) fn save(state: &mut AppState, packed: &PackedFile, source: PathBuf, now: Instant) -> Vec<Effect> {
    let change = Change::Replace { path: packed.entry.clone(), source };
    let request = RewriteRequest {
        archive: packed.archive.clone(),
        password: packed.password.clone(),
        change,
        originals: Vec::new(),
    };
    start_rewrite(state, JobKind::Save, state.active, request, now)
}

fn start_rewrite(
    state: &mut AppState,
    kind: JobKind,
    shown_on: Side,
    request: RewriteRequest,
    now: Instant,
) -> Vec<Effect> {
    let (id, mut effects) = crate::jobs::begin_on(state, kind, shown_on, now);
    effects.push(Effect::StartRewrite { id, request });
    effects
}

fn rewrite(mount: &ArchiveMount, change: Change, originals: Vec<PathBuf>) -> RewriteRequest {
    RewriteRequest { archive: mount.archive.clone(), password: mount.password.clone(), change, originals }
}

/// F5 and F6 into the archive the other panel shows, once the prompt names `text`: copies
/// `sources` into the archive folder it names, and a move deletes each once it is in whole. A
/// folder outside the archive takes them as any copy or move does.
pub(crate) fn pack(state: &mut AppState, sources: Vec<PathBuf>, text: &str, moving: bool, now: Instant) -> Vec<Effect> {
    let other = state.active.other();
    let target = state.panel(other).dir().join(text);
    let Some(mount) = state.panel(other).archive.clone().filter(|mount| mount.holds(&target)) else {
        let (kind, spec) = if moving {
            (JobKind::Move, Spec::Move { sources, target })
        } else {
            (JobKind::Copy, Spec::Copy { sources, target, optimize_hdd: false })
        };
        return crate::jobs::start(state, kind, spec, now);
    };
    let Some(into) = mount.inside(&target) else { return Vec::new() };
    let (kind, originals) = if moving { (JobKind::Move, sources.clone()) } else { (JobKind::Pack, Vec::new()) };
    start_rewrite(state, kind, other, rewrite(&mount, Change::Add { sources, into }, originals), now)
}

/// F6 in an archive, once the prompt names `text`: a place in the same archive moves the
/// entries there, and a folder of this machine takes them out of the archive.
pub(crate) fn move_out(state: &mut AppState, sources: &[PathBuf], text: &str, now: Instant) -> Vec<Effect> {
    let side = state.active;
    let Some((mount, base)) = shown_folder(state, side) else { return Vec::new() };
    let target = state.panel(side).dir().join(text);
    let paths: Vec<String> = sources.iter().filter_map(|source| mount.inside(source)).collect();
    if let Some(to) = mount.inside(&target) {
        return start_rewrite(state, JobKind::Move, side, rewrite(&mount, Change::Move { paths, to }, Vec::new()), now);
    }
    if !takes_copies(state, side.other()) {
        state.notice = Some(tr!(archive_copy_destination).into());
        return Vec::new();
    }
    let (id, mut effects) = crate::jobs::begin(state, JobKind::Move, now);
    effects.push(Effect::StartUnpack {
        id,
        request: UnpackRequest {
            archive: mount.archive,
            password: mount.password,
            paths,
            base,
            target: UnpackTarget::Folder(target),
            remove: true,
        },
    });
    effects
}

/// F8 in an archive, once confirmed: takes `sources` out of it.
pub(crate) fn remove(state: &mut AppState, sources: &[PathBuf], now: Instant) -> Vec<Effect> {
    let side = state.active;
    let Some(mount) = state.panel(side).archive.clone() else { return Vec::new() };
    let paths = sources.iter().filter_map(|source| mount.inside(source)).collect();
    start_rewrite(state, JobKind::Delete, side, rewrite(&mount, Change::Remove { paths }, Vec::new()), now)
}

/// F7 in an archive: makes the folder `text` names in the folder the panel shows.
pub(crate) fn mkdir(state: &mut AppState, text: &str, now: Instant) -> Vec<Effect> {
    let side = state.active;
    let Some(mount) = state.panel(side).archive.clone() else { return Vec::new() };
    let Some(path) = mount.inside(&state.panel(side).dir().join(text)) else { return Vec::new() };
    start_rewrite(state, JobKind::ArchiveFolder, side, rewrite(&mount, Change::Folder { path }, Vec::new()), now)
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
            remove: false,
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
            remove: false,
        },
    });
    effects
}

#[cfg(test)]
mod tests;
