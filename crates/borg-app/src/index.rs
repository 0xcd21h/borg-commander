//! The name index: every name below a folder chosen for instant search, saved in the cache.
//! A build walks the folder with the same exclusions as a live search and replaces the file
//! atomically, so the previous index answers until the next one is complete.

use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    ffi::OsStr,
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail, ensure};
use borg_core::{Candidate, Msg, SearchKind, SearchQuery, SearchSettings};
use borg_vfs::EntryKind;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::UnboundedSender;

const MAGIC: &[u8; 8] = b"BORGNDX\x01";
/// Rows an index holds; a larger folder is indexed this far and marked incomplete.
pub const ROW_LIMIT: usize = 8_000_000;
const FILE_LIMIT: u64 = 1024 * 1024 * 1024;
/// Matches ranked before the best are kept.
const RANKED: usize = 200_000;
const NONE: u32 = u32::MAX;
const HIDDEN: u8 = 1;
const LEFT_OUT: u8 = 2;

#[derive(Debug, Clone, Copy)]
struct Row {
    dir: u32,
    name: u32,
    len: u16,
    kind: u8,
    flags: u8,
    size: u64,
    modified: i64,
}

#[derive(Debug, Clone, Copy)]
struct Dir {
    parent: u32,
    /// This folder's own row, `NONE` for the root.
    row: u32,
    /// One past the last folder below it: folders are numbered in walking order.
    end: u32,
}

#[derive(Debug)]
pub struct Index {
    pub root: PathBuf,
    pub built: SystemTime,
    /// The exclusions it was built with, as JSON: only searches with the same ones use it.
    settings: String,
    pub complete: bool,
    dirs: Vec<Dir>,
    rows: Vec<Row>,
    names: Vec<u8>,
    children: OnceLock<HashMap<(u32, Vec<u8>), u32>>,
}

/// The exclusions an index applies, comparable between a build and a search.
fn settings_key(settings: &SearchSettings) -> String {
    serde_json::json!({"exclude": settings.exclude, "artifacts": settings.artifacts, "ignore": settings.ignore_files})
        .to_string()
}

fn kind_code(kind: EntryKind) -> u8 {
    match kind {
        EntryKind::File => 0,
        EntryKind::Dir => 1,
        EntryKind::Symlink { to_dir: false } => 2,
        EntryKind::Symlink { to_dir: true } => 3,
        EntryKind::Other => 4,
    }
}

fn kind_of(code: u8) -> EntryKind {
    match code {
        0 => EntryKind::File,
        1 => EntryKind::Dir,
        2 => EntryKind::Symlink { to_dir: false },
        3 => EntryKind::Symlink { to_dir: true },
        _ => EntryKind::Other,
    }
}

struct Builder {
    dirs: Vec<Dir>,
    rows: Vec<Row>,
    names: Vec<u8>,
    folders: HashMap<PathBuf, u32>,
}

impl Builder {
    fn add(&mut self, path: &Path, kind: EntryKind, meta: Option<fs::Metadata>, flags: u8) -> Result<()> {
        let parent = path.parent().and_then(|parent| self.folders.get(parent)).copied().unwrap_or(0);
        let name = path.file_name().unwrap_or_default().as_bytes();
        let row = u32::try_from(self.rows.len())?;
        let hidden = if name.starts_with(b".") { HIDDEN } else { 0 };
        self.rows.push(Row {
            dir: parent,
            name: u32::try_from(self.names.len())?,
            len: u16::try_from(name.len())?,
            kind: kind_code(kind),
            flags: flags | hidden,
            size: meta.as_ref().map_or(0, fs::Metadata::len),
            modified: meta
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |since| since.as_secs() as i64),
        });
        self.names.extend_from_slice(name);
        if kind == EntryKind::Dir && flags & LEFT_OUT == 0 {
            self.folders.insert(path.to_path_buf(), u32::try_from(self.dirs.len())?);
            self.dirs.push(Dir { parent, row, end: 0 });
        }
        Ok(())
    }
}

impl Index {
    /// Walks `root`, leaving out what `settings` skip, until done, cancelled or `ROW_LIMIT`.
    pub fn build(root: &Path, settings: &SearchSettings, cancelled: &AtomicBool) -> Result<Self> {
        let walker = crate::search::Walker::new(root, true, Some(settings)).map_err(anyhow::Error::msg)?;
        let mut builder = Builder {
            dirs: vec![Dir { parent: NONE, row: NONE, end: 0 }],
            rows: Vec::new(),
            names: Vec::new(),
            folders: HashMap::from([(root.to_path_buf(), 0)]),
        };
        let mut complete = true;
        for item in walker.walk.build() {
            if cancelled.load(Ordering::Relaxed) {
                bail!("cancelled");
            }
            let Ok(item) = item else { continue };
            if item.depth() == 0 {
                continue;
            }
            for path in crate::search::Walker::take_left_out(&walker.left_out) {
                builder.add(&path, EntryKind::Dir, fs::symlink_metadata(&path).ok(), LEFT_OUT)?;
            }
            let meta = item.metadata().ok();
            let kind = match item.file_type() {
                Some(kind) if kind.is_symlink() => {
                    EntryKind::Symlink { to_dir: fs::metadata(item.path()).is_ok_and(|m| m.is_dir()) }
                }
                Some(kind) if kind.is_dir() => EntryKind::Dir,
                Some(kind) if kind.is_file() => EntryKind::File,
                _ => EntryKind::Other,
            };
            builder.add(item.path(), kind, meta, 0)?;
            if builder.rows.len() >= ROW_LIMIT {
                complete = false;
                break;
            }
        }
        for path in crate::search::Walker::take_left_out(&walker.left_out) {
            builder.add(&path, EntryKind::Dir, fs::symlink_metadata(&path).ok(), LEFT_OUT)?;
        }
        let mut dirs = builder.dirs;
        let count = dirs.len() as u32;
        for at in (0..dirs.len()).rev() {
            dirs[at].end = dirs[at].end.max(at as u32 + 1).min(count);
            if let Some(parent) = dirs.get(at).map(|dir| dir.parent).filter(|parent| *parent != NONE) {
                dirs[parent as usize].end = dirs[parent as usize].end.max(dirs[at].end);
            }
        }
        Ok(Self {
            root: root.to_path_buf(),
            built: SystemTime::now(),
            settings: settings_key(settings),
            complete,
            dirs,
            rows: builder.rows,
            names: builder.names,
            children: OnceLock::new(),
        })
    }

    pub fn entries(&self) -> usize {
        self.rows.len()
    }

    fn name(&self, row: &Row) -> &[u8] {
        &self.names[row.name as usize..row.name as usize + usize::from(row.len)]
    }

    fn path(&self, row: &Row) -> PathBuf {
        let mut parts = vec![self.name(row)];
        let mut dir = row.dir;
        while let Some(found) = self.dirs.get(dir as usize).filter(|found| found.row != NONE) {
            parts.push(self.name(&self.rows[found.row as usize]));
            dir = found.parent;
        }
        parts.iter().rev().fold(self.root.clone(), |path, part| path.join(OsStr::from_bytes(part)))
    }

    /// The folder `path` names in the index, if it is the root or a folder below it.
    fn folder(&self, path: &Path) -> Option<u32> {
        let children = self.children.get_or_init(|| {
            self.dirs
                .iter()
                .enumerate()
                .filter(|(_, dir)| dir.row != NONE)
                .map(|(at, dir)| ((dir.parent, self.name(&self.rows[dir.row as usize]).to_vec()), at as u32))
                .collect()
        });
        path.strip_prefix(&self.root)
            .ok()?
            .components()
            .try_fold(0, |dir, part| children.get(&(dir, part.as_os_str().as_bytes().to_vec())).copied())
    }

    /// Whether a name between the row and the folder searched starts with a dot.
    fn hidden_below(&self, row: &Row, within: u32) -> bool {
        if row.flags & HIDDEN != 0 {
            return true;
        }
        let mut dir = row.dir;
        while dir != within && dir != NONE {
            let Some(found) = self.dirs.get(dir as usize) else { return false };
            if found.row != NONE && self.rows[found.row as usize].flags & HIDDEN != 0 {
                return true;
            }
            dir = found.parent;
        }
        false
    }

    /// The paths below `request.root` that match, best first: names that are the first word,
    /// then names starting with it, then shallower and shorter paths. `None` when the folder
    /// is not in the index, so a live search has to answer.
    pub fn search(&self, request: &borg_core::SearchRequest, query: &SearchQuery, limit: usize) -> Option<Search> {
        let within = self.folder(&request.root)?;
        let range = within..self.dirs[within as usize].end;
        let now = SystemTime::now();
        let primary = query.primary().map(str::to_lowercase);
        let mut ranked = Vec::new();
        let mut limited = false;
        for (at, row) in self.rows.iter().enumerate() {
            if !range.contains(&row.dir) {
                continue;
            }
            let kind = kind_of(row.kind);
            let wanted = match request.options.kind {
                SearchKind::Files => kind == EntryKind::File,
                SearchKind::Folders => kind == EntryKind::Dir,
                _ => true,
            };
            let candidate = IndexRow { index: self, row };
            if !wanted
                || (request.options.minimum > 0 && (kind.is_dir() || row.size < request.options.minimum))
                || !query.accepts(&candidate, now)
                || (!request.hidden && self.hidden_below(row, within))
            {
                continue;
            }
            let name = borg_core::composed(&candidate.name()).to_lowercase();
            let rank = match &primary {
                Some(word) if name == *word => 0u8,
                Some(word) if name.starts_with(word.as_str()) => 1,
                _ => 2,
            };
            let mut depth = 0u16;
            let mut dir = row.dir;
            while dir != within && dir != NONE {
                depth = depth.saturating_add(1);
                dir = self.dirs[dir as usize].parent;
            }
            ranked.push((rank, depth, row.len, at as u32));
            if ranked.len() >= RANKED {
                limited = true;
                break;
            }
        }
        ranked.sort_unstable();
        limited |= ranked.len() > limit;
        ranked.truncate(limit);
        let paths = ranked.into_iter().map(|(_, _, _, at)| self.path(&self.rows[at as usize])).collect();
        Some(Search { paths, limited })
    }
}

pub struct Search {
    pub paths: Vec<PathBuf>,
    pub limited: bool,
}

struct IndexRow<'a> {
    index: &'a Index,
    row: &'a Row,
}

impl Candidate for IndexRow<'_> {
    fn name(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(self.index.name(self.row))
    }
    fn kind(&self) -> EntryKind {
        kind_of(self.row.kind)
    }
    fn size(&self) -> u64 {
        self.row.size
    }
    fn modified(&self) -> Option<SystemTime> {
        (self.row.modified > 0).then(|| UNIX_EPOCH + Duration::from_secs(self.row.modified as u64))
    }
    fn path(&self) -> Cow<'_, Path> {
        Cow::Owned(self.index.path(self.row))
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8]> {
        let end =
            self.at.checked_add(count).filter(|end| *end <= self.bytes.len()).context("index file is cut short")?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
    }
    fn bytes(&mut self) -> Result<Vec<u8>> {
        let len = self.u32()? as usize;
        Ok(self.take(len)?.to_vec())
    }
}

impl Index {
    fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        let built = self.built.duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
        out.extend_from_slice(&built.to_le_bytes());
        out.push(u8::from(self.complete));
        for part in [self.root.as_os_str().as_bytes(), self.settings.as_bytes()] {
            out.extend_from_slice(&(part.len() as u32).to_le_bytes());
            out.extend_from_slice(part);
        }
        out.extend_from_slice(&(self.dirs.len() as u32).to_le_bytes());
        for dir in &self.dirs {
            for value in [dir.parent, dir.row, dir.end] {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        out.extend_from_slice(&(self.rows.len() as u32).to_le_bytes());
        for row in &self.rows {
            out.extend_from_slice(&row.dir.to_le_bytes());
            out.extend_from_slice(&row.name.to_le_bytes());
            out.extend_from_slice(&row.len.to_le_bytes());
            out.extend_from_slice(&[row.kind, row.flags]);
            out.extend_from_slice(&row.size.to_le_bytes());
            out.extend_from_slice(&row.modified.to_le_bytes());
        }
        out.extend_from_slice(&(self.names.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.names);
        out
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let mut read = Reader { bytes, at: 0 };
        ensure!(read.take(MAGIC.len())? == MAGIC, "not a Borg index");
        let built = UNIX_EPOCH + Duration::from_secs(read.u64()?);
        let complete = read.u8()? != 0;
        let root = PathBuf::from(OsStr::from_bytes(&read.bytes()?));
        let settings = String::from_utf8(read.bytes()?)?;
        let count = read.u32()? as usize;
        ensure!(count <= ROW_LIMIT + 1, "index has too many folders");
        let mut dirs = Vec::with_capacity(count);
        for _ in 0..count {
            dirs.push(Dir { parent: read.u32()?, row: read.u32()?, end: read.u32()? });
        }
        let count = read.u32()? as usize;
        ensure!(count <= ROW_LIMIT, "index has too many rows");
        let mut rows = Vec::with_capacity(count);
        for _ in 0..count {
            let (dir, name, len) = (read.u32()?, read.u32()?, read.u16()?);
            let (kind, flags) = (read.u8()?, read.u8()?);
            rows.push(Row { dir, name, len, kind, flags, size: read.u64()?, modified: read.u64()? as i64 });
        }
        let names = read.bytes()?;
        let valid = |dir: u32| dir == NONE || (dir as usize) < dirs.len();
        ensure!(
            dirs.iter().all(|dir| valid(dir.parent) && (dir.row == NONE || (dir.row as usize) < rows.len()))
                && rows.iter().all(|row| valid(row.dir) && row.name as usize + usize::from(row.len) <= names.len()),
            "index refers outside itself"
        );
        Ok(Self { root, built, settings, complete, dirs, rows, names, children: OnceLock::new() })
    }
}

/// The indexed folders, their saved indexes and the one build running at a time.
#[derive(Clone)]
pub struct Indexes(Arc<Shared>);

struct Shared {
    dir: Option<PathBuf>,
    roots: Mutex<Vec<PathBuf>>,
    refresh: Duration,
    loaded: Mutex<HashMap<PathBuf, Arc<Index>>>,
    queue: Mutex<VecDeque<(PathBuf, SearchSettings)>>,
    building: AtomicBool,
    messages: Mutex<Option<UnboundedSender<Msg>>>,
}

impl Indexes {
    pub fn new(cache: Option<PathBuf>, refresh: Duration) -> Self {
        Self(Arc::new(Shared {
            dir: cache.map(|cache| cache.join("index")),
            roots: Mutex::new(Vec::new()),
            refresh,
            loaded: Mutex::new(HashMap::new()),
            queue: Mutex::new(VecDeque::new()),
            building: AtomicBool::new(false),
            messages: Mutex::new(None),
        }))
    }

    pub fn connect(&self, messages: UnboundedSender<Msg>) {
        *self.0.messages.lock().expect("index messages") = Some(messages);
    }

    fn file(&self, root: &Path) -> Option<PathBuf> {
        let digest = Sha256::digest(root.as_os_str().as_bytes());
        let name: String = digest[..16].iter().map(|byte| format!("{byte:02x}")).collect();
        self.0.dir.as_ref().map(|dir| dir.join(format!("{name}.idx")))
    }

    /// Replaces the indexed folders: indexes of folders no longer listed are deleted.
    pub fn set_roots(&self, roots: Vec<PathBuf>) {
        let old = std::mem::replace(&mut *self.0.roots.lock().expect("index roots"), roots.clone());
        for root in old.iter().filter(|root| !roots.contains(root)) {
            self.0.loaded.lock().expect("loaded indexes").remove(root);
            if let Some(file) = self.file(root) {
                let _ = fs::remove_file(file);
            }
        }
    }

    pub fn roots(&self) -> Vec<PathBuf> {
        self.0.roots.lock().expect("index roots").clone()
    }

    fn load(&self, root: &Path) -> Option<Arc<Index>> {
        if let Some(index) = self.0.loaded.lock().expect("loaded indexes").get(root) {
            return Some(index.clone());
        }
        let file = self.file(root)?;
        let size = fs::metadata(&file).ok()?.len();
        let index = (size <= FILE_LIMIT)
            .then(|| fs::read(&file).ok())
            .flatten()
            .and_then(|bytes| Index::decode(&bytes).ok())?;
        let index = Arc::new(index);
        (index.root == root).then(|| {
            self.0.loaded.lock().expect("loaded indexes").insert(root.to_path_buf(), index.clone());
            index
        })
    }

    /// The index that can answer `request`: of the deepest indexed folder holding its folder,
    /// built with the same exclusions. A stale or missing index is rebuilt in the background.
    pub fn covering(&self, request: &borg_core::SearchRequest) -> Option<Arc<Index>> {
        let settings = request.skip.as_ref()?;
        if matches!(request.options.kind, SearchKind::FolderTotals | SearchKind::FolderTree) {
            return None;
        }
        let root = self
            .roots()
            .into_iter()
            .filter(|root| request.root.starts_with(root))
            .max_by_key(|root| root.components().count())?;
        let index = self.load(&root);
        let fresh = index.as_ref().is_some_and(|index| {
            index.settings == settings_key(settings) && index.built.elapsed().is_ok_and(|age| age < self.0.refresh)
        });
        if !fresh {
            self.refresh(root, settings.clone());
        }
        index.filter(|index| index.settings == settings_key(settings))
    }

    /// Rebuilds the indexes that are missing, stale, or built with other exclusions.
    pub fn refresh_stale(&self, settings: &SearchSettings) {
        for root in self.roots() {
            let current = self.load(&root).is_some_and(|index| {
                index.settings == settings_key(settings) && index.built.elapsed().is_ok_and(|age| age < self.0.refresh)
            });
            if !current {
                self.refresh(root, settings.clone());
            }
        }
    }

    /// Builds the index of `root` after any already waiting.
    pub fn refresh(&self, root: PathBuf, settings: SearchSettings) {
        {
            let mut queue = self.0.queue.lock().expect("index queue");
            if !queue.iter().any(|(queued, _)| *queued == root) {
                queue.push_back((root, settings));
            }
        }
        self.next();
    }

    fn next(&self) {
        if self.0.building.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some((root, settings)) = self.0.queue.lock().expect("index queue").pop_front() else {
            self.0.building.store(false, Ordering::SeqCst);
            return;
        };
        let indexes = self.clone();
        tokio::task::spawn_blocking(move || {
            let result = indexes.build(&root, &settings);
            let message = match result {
                Ok(entries) => Msg::Indexed { root, entries, error: None },
                Err(error) => Msg::Indexed { root, entries: 0, error: Some(format!("{error:#}")) },
            };
            if let Some(messages) = indexes.0.messages.lock().expect("index messages").as_ref() {
                let _ = messages.send(message);
            }
            indexes.0.building.store(false, Ordering::SeqCst);
            indexes.next();
        });
    }

    fn build(&self, root: &Path, settings: &SearchSettings) -> Result<usize> {
        if !self.roots().iter().any(|indexed| indexed == root) {
            bail!("{} is no longer indexed", root.display());
        }
        let index = Index::build(root, settings, &AtomicBool::new(false))?;
        let entries = index.entries();
        if let Some(file) = self.file(root) {
            fs::create_dir_all(file.parent().context("index folder")?)?;
            crate::store::write_privately(&file, &index.encode())?;
        }
        if self.roots().iter().any(|indexed| indexed == root) {
            self.0.loaded.lock().expect("loaded indexes").insert(root.to_path_buf(), Arc::new(index));
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for file in [
            "notes.md",
            "src/main.rs",
            "src/lib/main.rs",
            "src/lib/mainframe.rs",
            "docs/main/readme.md",
            ".config/main.toml",
            "app/node_modules/dep/main.rs",
        ] {
            let path = root.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, file).unwrap();
        }
        root
    }

    fn request(root: &Path, query: &str, hidden: bool) -> borg_core::SearchRequest {
        borg_core::SearchRequest {
            root: root.into(),
            query: query.into(),
            hidden,
            options: Default::default(),
            skip: Some(SearchSettings::default()),
        }
    }

    fn found(index: &Index, root: &Path, query: &str, hidden: bool) -> Vec<String> {
        let search = index.search(&request(root, query, hidden), &SearchQuery::parse(query).unwrap(), 100).unwrap();
        search.paths.iter().map(|path| path.strip_prefix(&index.root).unwrap().display().to_string()).collect()
    }

    #[test]
    fn indexes_rank_exact_names_first_and_answer_for_any_folder_below_the_root() {
        let root = tree();
        let index = Index::build(root.path(), &SearchSettings::default(), &AtomicBool::new(false)).unwrap();
        let index = Index::decode(&index.encode()).unwrap();
        assert!(index.complete);
        assert_eq!(
            found(&index, root.path(), "main", false),
            ["docs/main", "src/main.rs", "src/lib/main.rs", "src/lib/mainframe.rs"]
        );
        assert_eq!(
            found(&index, &root.path().join("src/lib"), "main", false),
            ["main.rs", "mainframe.rs"].map(|f| format!("src/lib/{f}"))
        );
        assert_eq!(found(&index, root.path(), "main.toml", true), [".config/main.toml"]);
        assert!(found(&index, root.path(), "main.toml", false).is_empty(), "hidden names wait for hidden:yes");
        assert_eq!(found(&index, root.path(), "node_modules", false), ["app/node_modules"]);
        assert!(found(&index, root.path(), "dep", false).is_empty(), "left-out folders keep their contents out");
        assert!(
            index
                .search(&request(&root.path().join("missing"), "a", false), &SearchQuery::parse("a").unwrap(), 9)
                .is_none()
        );
    }

    #[test]
    fn damaged_files_are_refused_instead_of_read_past_their_end() {
        let root = tree();
        let bytes = Index::build(root.path(), &SearchSettings::default(), &AtomicBool::new(false)).unwrap().encode();
        assert!(Index::decode(&bytes[..bytes.len() - 3]).is_err());
        assert!(Index::decode(b"BORGNDX\x02").is_err());
    }
}
