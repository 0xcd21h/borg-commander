//! Archives in panels: their listings, kept while the archive file stays the same, and the jobs
//! that unpack their entries into a folder or for the viewer.

use std::{
    collections::{HashMap, VecDeque},
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use borg_archive::{Archive, Error, Index, Item, ItemKind, RAR_PROGRAMS, Tool, Unpack};
use borg_core::{ArchiveMount, Msg, Password, Side, UnpackRequest};
use borg_i18n::tr;
use borg_jobs::{Control, Event, Outcome};
use borg_vfs::{Entry, EntryKind, FolderUsage};
use tokio::sync::mpsc::UnboundedSender;

/// How many archives' listings Borg keeps.
const KEPT: usize = 4;
/// How often the first read of an archive reports how far it got.
const REPORT_EVERY: Duration = Duration::from_millis(100);
const EXECUTABLE: u32 = 0o111;
/// Only the user reads what Borg unpacks for the viewer.
const PRIVATE_FOLDER: u32 = 0o700;
const VIEW_PREFIX: &str = "view-";

/// The size and time that tell whether an archive file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl Stamp {
    fn of(path: &Path) -> std::io::Result<Self> {
        let meta = fs::metadata(path)?;
        Ok(Self { len: meta.len(), modified: meta.modified().ok() })
    }
}

#[derive(Default)]
struct Kept {
    order: VecDeque<PathBuf>,
    indexes: HashMap<PathBuf, (Stamp, Arc<Index>)>,
}

/// The listings of the archives the panels showed lately, by the archive's path, while its file
/// keeps its size and time.
#[derive(Clone, Default)]
pub(crate) struct Indexes(Arc<Mutex<Kept>>);

impl Indexes {
    /// The archive's listing, read again once the file changed.
    fn index(&self, archive: Archive<'_>, watch: borg_archive::Watch) -> Result<Arc<Index>, Error> {
        let stamp = Stamp::of(archive.path)?;
        if let Some(index) = self.kept(archive.path, stamp) {
            return Ok(index);
        }
        let index = Arc::new(archive.index(watch)?);
        self.keep(archive.path.to_path_buf(), stamp, Arc::clone(&index));
        Ok(index)
    }

    fn kept(&self, path: &Path, stamp: Stamp) -> Option<Arc<Index>> {
        let kept = self.0.lock().ok()?;
        kept.indexes.get(path).filter(|(kept, _)| *kept == stamp).map(|(_, index)| Arc::clone(index))
    }

    fn keep(&self, path: PathBuf, stamp: Stamp, index: Arc<Index>) {
        let Ok(mut kept) = self.0.lock() else { return };
        kept.order.retain(|known| *known != path);
        kept.order.push_back(path.clone());
        kept.indexes.insert(path, (stamp, index));
        while kept.order.len() > KEPT {
            if let Some(oldest) = kept.order.pop_front() {
                kept.indexes.remove(&oldest);
            }
        }
    }
}

/// The installed program that reads RAR archives, the first Borg prefers.
fn rar_tool() -> Option<Tool> {
    RAR_PROGRAMS
        .into_iter()
        .find_map(|(program, kind)| crate::tools::find(program).map(|program| Tool { kind, program }))
}

fn password(given: Option<&Password>) -> Option<&str> {
    given.map(Password::as_str)
}

/// Where a listing reports to: the panel it fills, which stops it once it shows something else.
pub(crate) struct Listing {
    pub side: Side,
    pub generation: u64,
    pub dir: PathBuf,
    pub mount: ArchiveMount,
    pub messages: UnboundedSender<Msg>,
    pub stopped: Arc<AtomicBool>,
}

impl Listing {
    fn send(&self, msg: Msg) {
        let _ = self.messages.send(msg);
    }

    fn finish(&self, error: Option<String>) {
        self.send(Msg::Listed { side: self.side, generation: self.generation, error });
    }

    fn locked(&self, wrong: bool) {
        self.send(Msg::ArchiveLocked { side: self.side, generation: self.generation, wrong });
    }

    /// Lists the folder of the archive the panel opens, reading the archive the first time.
    pub(crate) fn run(self, indexes: &Indexes) {
        let tool = rar_tool();
        let archive = Archive {
            path: &self.mount.archive,
            password: password(self.mount.password.as_ref()),
            tool: tool.as_ref(),
        };
        let mut reported = Instant::now();
        let mut watch = |read: u64, total: u64| {
            if self.stopped.load(Ordering::Relaxed) {
                return false;
            }
            if reported.elapsed() >= REPORT_EVERY {
                reported = Instant::now();
                self.send(Msg::ArchiveReading { side: self.side, generation: self.generation, read, total });
            }
            true
        };
        let opened = indexes.index(archive, &mut watch).and_then(|index| self.unlock(archive, index));
        match opened {
            Ok(index) => self.fill(&index),
            Err(Error::NeedsPassword) => self.locked(false),
            Err(Error::WrongPassword) => self.locked(true),
            Err(Error::NoTool) => self.send(Msg::ArchiveNeedsTool {
                side: self.side,
                generation: self.generation,
                installer: crate::tools::installer(),
            }),
            Err(Error::Stopped) => {}
            Err(error) => self.finish(Some(error.to_string())),
        }
    }

    /// The listing, once the password opens the encrypted entries.
    fn unlock(&self, archive: Archive<'_>, index: Arc<Index>) -> Result<Arc<Index>, Error> {
        if !index.encrypted() || self.mount.verified {
            return Ok(index);
        }
        if archive.password.is_none() {
            return Err(Error::NeedsPassword);
        }
        archive.check_password(&index)?;
        Ok(index)
    }

    fn fill(&self, index: &Index) {
        let inside = self.mount.inside(&self.dir).unwrap_or_default();
        let Some(items) = index.list(&inside) else {
            self.finish(Some(tr!(archive_missing_folder, path = inside)));
            return;
        };
        let entries = items.map(entry).collect();
        let (side, generation) = (self.side, self.generation);
        self.send(Msg::ArchiveOpened { side, generation, format: index.format, left_out: index.left_out });
        self.send(Msg::Batch { side, generation, entries });
        self.finish(None);
    }
}

/// An archive entry as a panel lists it: a folder with what it holds, a file with its size.
fn entry(item: &Item) -> Entry {
    let folder = item.is_folder();
    let kind = match item.kind {
        ItemKind::Folder => EntryKind::Dir,
        ItemKind::File | ItemKind::HardLink => EntryKind::File,
        ItemKind::Symlink => EntryKind::Symlink { to_dir: false },
    };
    let usage =
        folder.then(|| FolderUsage { files: item.inside.files, folders: item.inside.folders, ..Default::default() });
    Entry {
        usage,
        source: None,
        missing: false,
        name: item.name().to_owned(),
        kind,
        size: if folder { item.inside.bytes } else { item.size },
        modified: item.modified,
        executable: !folder && item.mode & EXECUTABLE != 0,
        permissions: item.mode,
    }
}

/// Unpacks what `request` names into `target`, reporting to `emit` and asking through `control`.
pub(crate) fn unpack(
    indexes: &Indexes,
    request: &UnpackRequest,
    target: &Path,
    control: &Control,
    emit: &mut dyn FnMut(Event),
) -> Outcome {
    let tool = rar_tool();
    let archive =
        Archive { path: &request.archive, password: password(request.password.as_ref()), tool: tool.as_ref() };
    let index = match indexes.index(archive, &mut |_, _| !control.cancelled()) {
        Ok(index) => index,
        Err(Error::NeedsPassword) => return Outcome::Failed(tr!(archive_extract_password).into()),
        Err(_) if control.cancelled() => return Outcome::Cancelled { done: Default::default() },
        Err(error) => return Outcome::Failed(error.to_string()),
    };
    if index.encrypted() && archive.password.is_none() {
        return Outcome::Failed(tr!(archive_extract_password).into());
    }
    Unpack { archive, paths: &request.paths, base: &request.base, target }.run(&index, control, emit)
}

/// A file unpacked for the viewer, in a folder of its own that goes away with it.
pub(crate) struct Unpacked {
    _folder: tempfile::TempDir,
    pub path: PathBuf,
}

/// Unpacks the one file `request` names into a new folder in `scratch`, Borg's folder for
/// files unpacked for the viewer.
pub(crate) fn unpack_for_viewer(
    indexes: &Indexes,
    request: &UnpackRequest,
    scratch: &Path,
    control: &Control,
    emit: &mut dyn FnMut(Event),
) -> (Outcome, Option<Unpacked>) {
    let folder = DirBuilder::new()
        .recursive(true)
        .mode(PRIVATE_FOLDER)
        .create(scratch)
        .and_then(|()| tempfile::Builder::new().prefix(VIEW_PREFIX).tempdir_in(scratch));
    let folder = match folder {
        Ok(folder) => folder,
        Err(error) => return (Outcome::Failed(format!("{}: {error}", scratch.display())), None),
    };
    let outcome = unpack(indexes, request, folder.path(), control, emit);
    let name = request.paths.first().map(|path| borg_archive::base(path)).unwrap_or_default();
    let path = folder.path().join(name);
    let unpacked = matches!(outcome, Outcome::Done { skipped: 0, .. }).then_some(Unpacked { _folder: folder, path });
    (outcome, unpacked)
}

/// Sets `stopped` when dropped, as when the task that waits for a listing is aborted.
pub(crate) struct StopOnDrop(pub Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests;
