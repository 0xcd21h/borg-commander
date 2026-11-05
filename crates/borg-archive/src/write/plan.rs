//! What a change makes of an archive: the entries it keeps, under which paths, and the entries it
//! adds, settled with the questions a copy asks about names already there.

use std::{
    collections::{HashMap, HashSet},
    fs::{self, Metadata},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use borg_i18n::tr;
use borg_jobs::{Ask, Control, Decision, Event, Totals};

use crate::{Index, Item, name};

use super::{Change, Halt};

const PERMISSIONS: u32 = 0o777;
const FOLDER_MODE: u32 = 0o755;
const SEPARATOR: char = '/';
const REPORT_EVERY: Duration = Duration::from_millis(100);

/// An entry of the archive the change keeps, with the path it gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Kept {
    pub item: Item,
    pub path: String,
}

impl Kept {
    /// The name to store: the archive's own when the path stays, which keeps it byte for byte.
    pub fn stored(&self) -> String {
        if self.path == self.item.path {
            self.item.raw.clone()
        } else if self.item.is_folder() {
            format!("{}{SEPARATOR}", self.path)
        } else {
            self.path.clone()
        }
    }
}

/// What an added entry holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Adding {
    File { source: PathBuf, size: u64 },
    Folder,
    Symlink(String),
}

/// An entry the change adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Added {
    pub path: String,
    pub adding: Adding,
    pub mode: u32,
    pub modified: Option<SystemTime>,
}

impl Added {
    pub fn size(&self) -> u64 {
        match self.adding {
            Adding::File { size, .. } => size,
            Adding::Folder | Adding::Symlink(_) => 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Plan {
    /// The entries kept, by their place in the archive.
    pub kept: HashMap<usize, Kept>,
    /// Places of every entry the listing knows, kept or not. An entry Borg left out of the
    /// listing, such as a device file, stays in the archive as it is.
    pub known: HashSet<usize>,
    pub added: Vec<Added>,
    /// Folders moved, each with its new path, for links that name what they hold.
    pub moves: Vec<(String, String)>,
    /// Whether every entry of the archive stays as it was, so new entries may follow them.
    pub appends: bool,
    /// What the change touched, for the job's summary.
    pub affected: Totals,
    pub skipped: u64,
    /// Files and folders of this machine the change left out, wholly or in part.
    pub left: Vec<PathBuf>,
}

impl Plan {
    pub fn added_bytes(&self) -> u64 {
        self.added.iter().map(Added::size).sum()
    }

    /// Where an entry that was at `path` is now.
    pub fn moved(&self, path: &str) -> String {
        self.moves
            .iter()
            .find(|(from, _)| name::within(path, from))
            .map_or_else(|| path.to_owned(), |(from, to)| join(to, name::relative(path, from)))
    }

    /// The first source of the entries added, which says the drive they are read from.
    pub fn source(&self) -> Option<&Path> {
        self.added.iter().find_map(|added| match &added.adding {
            Adding::File { source, .. } => Some(source.as_path()),
            Adding::Folder | Adding::Symlink(_) => None,
        })
    }
}

fn join(folder: &str, rest: &str) -> String {
    match (folder.is_empty(), rest.is_empty()) {
        (true, _) => rest.to_owned(),
        (_, true) => folder.to_owned(),
        _ => format!("{folder}{SEPARATOR}{rest}"),
    }
}

fn base(index: &Index, keep: impl Fn(&Item) -> Option<String>) -> Plan {
    let mut plan = Plan { appends: true, ..Plan::default() };
    plan.known.extend(index.shadowed());
    for item in index.entries() {
        let Some(slot) = item.slot else { continue };
        plan.known.insert(slot);
        match keep(item) {
            Some(path) => {
                plan.appends &= path == item.path;
                plan.kept.insert(slot, Kept { item: item.clone(), path });
            }
            None => plan.appends = false,
        }
    }
    plan
}

/// Asks the questions a change raises: about names already there, through `control`.
pub(crate) struct Asker<'a, 'e> {
    pub control: &'a Control,
    pub emit: &'e mut dyn FnMut(Event),
    /// Shows a path inside the archive as the panel does.
    pub shown: &'a Path,
    overwrite: Option<bool>,
    skip_errors: bool,
    reported: Option<Instant>,
}

impl<'a, 'e> Asker<'a, 'e> {
    pub fn new(control: &'a Control, emit: &'e mut dyn FnMut(Event), shown: &'a Path) -> Self {
        Self { control, emit, shown, overwrite: None, skip_errors: false, reported: None }
    }

    fn conflict(&mut self, source: &Path, local: &Metadata, path: &str, there: &There) -> Result<Decision, Halt> {
        match self.overwrite {
            Some(true) => return Ok(Decision::Overwrite),
            Some(false) => return Ok(Decision::Skip),
            None => {}
        }
        let modified = local.modified().ok();
        (self.emit)(Event::Asking(Ask::Conflict {
            source: source.to_path_buf(),
            target: self.shown.join(path),
            sizes: (local.len(), there.size),
            newer: modified.zip(there.modified).is_some_and(|(local, there)| local > there),
        }));
        let decision = self.control.decide();
        match decision {
            Decision::OverwriteAll => self.overwrite = Some(true),
            Decision::SkipAll => self.overwrite = Some(false),
            Decision::Cancel => return Err(Halt::Stopped),
            _ => {}
        }
        Ok(decision)
    }

    /// Asks what to do about `path`, which the change cannot take, and leaves it out.
    fn failure(&mut self, path: &Path, message: String) -> Result<(), Halt> {
        if self.skip_errors {
            return Ok(());
        }
        (self.emit)(Event::Asking(Ask::Failure { path: path.to_path_buf(), message }));
        match self.control.decide() {
            Decision::Cancel => Err(Halt::Stopped),
            Decision::SkipAll => {
                self.skip_errors = true;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn scanning(&mut self, found: Totals) -> Result<(), Halt> {
        if self.reported.is_none_or(|at| at.elapsed() >= REPORT_EVERY) {
            self.reported = Some(Instant::now());
            (self.emit)(Event::Scanning(found));
        }
        if self.control.proceed() { Ok(()) } else { Err(Halt::Stopped) }
    }
}

pub(crate) fn plan(change: &Change, index: &Index, asker: &mut Asker<'_, '_>) -> Result<Plan, Halt> {
    match change {
        Change::Remove { paths } => Ok(remove(index, paths)),
        Change::Move { paths, to } => rename(index, &moves(index, paths, to)),
        Change::Folder { path } => folder(index, path),
        Change::Replace { path, source } => replace(index, path, source),
        Change::Add { into, .. } if !into.is_empty() && name::clean(into, false).as_deref() != Some(into) => {
            Err(Halt::Failed(tr!(archive_bad_name, name = into)))
        }
        Change::Add { sources, into } => Adder::new(index, asker).all(sources, into),
    }
}

/// Where each of `paths` goes when moved to `to`.
fn moves(index: &Index, paths: &[String], to: &str) -> Vec<(String, String)> {
    let into = paths.len() > 1 || to.is_empty() || index.item(to).is_some_and(Item::is_folder);
    let target = |from: &String| if into { join(to, name::base(from)) } else { to.to_owned() };
    paths.iter().map(|from| (from.clone(), target(from))).collect()
}

fn remove(index: &Index, paths: &[String]) -> Plan {
    let selection = index.selection(paths);
    let dropped: HashSet<&str> = selection.iter().map(|item| item.path.as_str()).collect();
    let mut plan = base(index, |item| (!dropped.contains(item.path.as_str())).then(|| item.path.clone()));
    plan.affected = Totals { files: Index::selected_totals(&selection).files, bytes: 0 };
    plan
}

fn rename(index: &Index, moves: &[(String, String)]) -> Result<Plan, Halt> {
    let moves: Vec<(String, String)> = moves.iter().filter(|(from, to)| from != to).cloned().collect();
    for (from, to) in &moves {
        if name::within(to, from) {
            return Err(Halt::Failed(tr!(archive_into_itself, path = from)));
        }
        if name::clean(to, false).as_deref() != Some(to.as_str()) {
            return Err(Halt::Failed(tr!(archive_bad_name, name = to)));
        }
        if index.item(to).is_some() {
            return Err(Halt::Failed(tr!(archive_name_taken, path = to)));
        }
    }
    let probe = Plan { moves: moves.clone(), ..Plan::default() };
    let mut plan = base(index, |item| Some(probe.moved(&item.path)));
    let froms: Vec<String> = moves.iter().map(|(from, _)| from.clone()).collect();
    plan.affected = Totals { files: Index::selected_totals(&index.selection(&froms)).files, bytes: 0 };
    plan.moves = moves;
    Ok(plan)
}

fn folder(index: &Index, path: &str) -> Result<Plan, Halt> {
    if name::clean(path, false).as_deref() != Some(path) {
        return Err(Halt::Failed(tr!(archive_bad_name, name = path)));
    }
    if index.item(path).is_some() {
        return Err(Halt::Failed(tr!(archive_name_taken, path = path)));
    }
    let mut plan = base(index, |item| Some(item.path.clone()));
    plan.added.push(Added {
        path: path.to_owned(),
        adding: Adding::Folder,
        mode: FOLDER_MODE,
        modified: Some(SystemTime::now()),
    });
    plan.affected = Totals { files: 1, bytes: 0 };
    Ok(plan)
}

fn replace(index: &Index, path: &str, source: &Path) -> Result<Plan, Halt> {
    let Some(item) = index.item(path).filter(|item| item.kind == crate::ItemKind::File) else {
        return Err(Halt::Failed(tr!(archive_missing_entry, path = path)));
    };
    let local = fs::metadata(source).map_err(|error| Halt::Failed(format!("{}: {error}", source.display())))?;
    let mut plan = base(index, |kept| (kept.path != path).then(|| kept.path.clone()));
    plan.added.push(Added {
        path: path.to_owned(),
        adding: Adding::File { source: source.to_path_buf(), size: local.len() },
        mode: item.mode,
        modified: local.modified().ok(),
    });
    plan.affected = Totals { files: 1, bytes: local.len() };
    Ok(plan)
}

/// What already takes a path the change adds an entry under: an entry of the archive, or one
/// the same change added before.
struct There {
    folder: bool,
    size: u64,
    modified: Option<SystemTime>,
    /// The place in `added` of an entry the change added.
    added: Option<usize>,
}

/// Adds files and folders of this machine, walking each folder.
struct Adder<'i, 'q, 'a, 'e> {
    index: &'i Index,
    asker: &'q mut Asker<'a, 'e>,
    /// Paths already taken in the new archive.
    taken: HashSet<String>,
    dropped: HashSet<String>,
    added: Vec<Added>,
    /// The file or folder of this machine each added path comes from.
    sources: HashMap<String, PathBuf>,
    skipped: u64,
    left: Vec<PathBuf>,
    found: Totals,
}

impl<'i, 'q, 'a, 'e> Adder<'i, 'q, 'a, 'e> {
    fn new(index: &'i Index, asker: &'q mut Asker<'a, 'e>) -> Self {
        let taken = index.entries().map(|item| item.path.clone()).collect();
        Self {
            index,
            asker,
            taken,
            dropped: HashSet::new(),
            added: Vec::new(),
            sources: HashMap::new(),
            skipped: 0,
            left: Vec::new(),
            found: Totals::default(),
        }
    }

    fn all(mut self, sources: &[PathBuf], into: &str) -> Result<Plan, Halt> {
        for source in sources {
            let Some(name) = source.file_name().map(|name| name.to_string_lossy().into_owned()) else { continue };
            self.walk(source, join(into, &name))?;
        }
        let dropped = self.dropped;
        let mut plan = base(self.index, |item| (!dropped.contains(&item.path)).then(|| item.path.clone()));
        let added_files = self.added.iter().filter(|added| !matches!(added.adding, Adding::Folder)).count() as u64;
        plan.affected = Totals { files: added_files, bytes: self.added.iter().map(Added::size).sum() };
        plan.added = self.added;
        plan.skipped = self.skipped;
        plan.left = self.left;
        Ok(plan)
    }

    fn walk(&mut self, source: &Path, path: String) -> Result<(), Halt> {
        let local = match fs::symlink_metadata(source) {
            Ok(local) => local,
            Err(error) => {
                self.leave(source);
                return self.asker.failure(source, error.to_string());
            }
        };
        let kind = local.file_type();
        if !(kind.is_file() || kind.is_dir() || kind.is_symlink()) {
            self.leave(source);
            return Ok(());
        }
        let Some(path) = self.settle(source, &local, path)? else { return Ok(()) };
        if kind.is_dir() {
            if self.there(&path).is_none() {
                self.push(source, path.clone(), Adding::Folder, &local);
            }
            let mut children: Vec<PathBuf> = match fs::read_dir(source) {
                Ok(children) => children.filter_map(|child| child.ok().map(|child| child.path())).collect(),
                Err(error) => {
                    self.leave(source);
                    return self.asker.failure(source, error.to_string());
                }
            };
            children.sort();
            for child in children {
                let Some(name) = child.file_name().map(|name| name.to_string_lossy().into_owned()) else { continue };
                self.walk(&child, join(&path, &name))?;
            }
        } else if kind.is_symlink() {
            match fs::read_link(source) {
                Ok(target) => self.push(source, path, Adding::Symlink(target.to_string_lossy().into_owned()), &local),
                Err(error) => {
                    self.leave(source);
                    return self.asker.failure(source, error.to_string());
                }
            }
        } else {
            let adding = Adding::File { source: source.to_path_buf(), size: local.len() };
            self.found = Totals { files: self.found.files + 1, bytes: self.found.bytes + local.len() };
            self.push(source, path, adding, &local);
        }
        self.asker.scanning(self.found)
    }

    /// What takes `path` already: an entry this change added, or else one of the archive it
    /// keeps.
    fn there(&self, path: &str) -> Option<There> {
        if self.sources.contains_key(path) {
            let place = self.added.iter().position(|added| added.path == path)?;
            let added = &self.added[place];
            let folder = matches!(added.adding, Adding::Folder);
            return Some(There { folder, size: added.size(), modified: added.modified, added: Some(place) });
        }
        let item = self.index.item(path).filter(|item| !self.dropped.contains(&item.path))?;
        Some(There { folder: item.is_folder(), size: item.size, modified: item.modified, added: None })
    }

    /// The path an entry is added under, once a name already there is settled; `None` leaves it
    /// out.
    fn settle(&mut self, source: &Path, local: &Metadata, path: String) -> Result<Option<String>, Halt> {
        let Some(there) = self.there(&path) else {
            return Ok(Some(path));
        };
        let folder = local.file_type().is_dir();
        if folder && there.folder {
            return Ok(Some(path));
        }
        if folder != there.folder {
            self.leave(source);
            self.asker.failure(source, tr!(archive_kind_taken, path = path))?;
            return Ok(None);
        }
        Ok(match self.asker.conflict(source, local, &path, &there)? {
            Decision::Overwrite | Decision::OverwriteAll => {
                match there.added {
                    Some(place) => self.replaced(place),
                    None => {
                        self.dropped.insert(path.clone());
                    }
                }
                Some(path)
            }
            Decision::KeepBoth => {
                let unique = borg_jobs::unique_by(Path::new(&path), |candidate| {
                    self.taken.contains(candidate.to_string_lossy().as_ref())
                });
                Some(unique.to_string_lossy().into_owned())
            }
            _ => {
                self.leave(source);
                None
            }
        })
    }

    /// Takes back the entry at `place` in `added`, which a later source of the same name
    /// replaces, and keeps its source out of what the change took whole.
    fn replaced(&mut self, place: usize) {
        let earlier = self.added.remove(place);
        if let Some(source) = self.sources.remove(&earlier.path) {
            self.left.push(source);
        }
    }

    /// Leaves `source` out of the archive.
    fn leave(&mut self, source: &Path) {
        self.skipped += 1;
        self.left.push(source.to_path_buf());
    }

    fn push(&mut self, source: &Path, path: String, adding: Adding, local: &Metadata) {
        self.taken.insert(path.clone());
        self.sources.insert(path.clone(), source.to_path_buf());
        let mode = local.permissions().mode() & PERMISSIONS;
        self.added.push(Added { path, adding, mode, modified: local.modified().ok() });
    }
}
