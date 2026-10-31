//! Measuring what a folder holds: its largest folders, or its tree of folders, read from the
//! size cache when it can and stored there as each folder is measured.

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};

use borg_core::{Query, RESULT_LIMIT, SearchKind, SearchProgress, SearchRequest};
use borg_i18n::tr;
use borg_vfs::{Entry, FolderUsage};

use super::{MAX_DEPTH, Output, PROGRESS_EVERY, guarded};
use crate::size_cache::{ScanStore, SizeCache};

pub(super) fn scan_sizes(search: &SearchRequest, stream: &mut Output<'_>, cache: &SizeCache) {
    let tree = search.options.kind == SearchKind::FolderTree;
    let epoch = cache.epoch();
    if !search.options.refresh
        && let Some((entries, progress)) =
            if tree { cache.tree(&search.root, search.hidden) } else { cache.flat(search, || stream.stopped()) }
    {
        replay(stream, entries, progress);
        return;
    }
    let (root, identity) = match open_root(&search.root) {
        Ok(opened) => opened,
        Err(error) => {
            stream.listed(Some(error));
            return;
        }
    };
    let mut scan = SizeScan {
        search,
        query: Query::parse(&search.query),
        tree,
        device: identity.0,
        store: cache.begin(&search.root, search.hidden, identity, epoch),
        progress: SearchProgress::default(),
        largest: BTreeMap::new(),
        shown: 1,
        frontier: HashSet::new(),
        stack: Vec::new(),
        stream,
    };
    if scan.measure(root).is_some() {
        scan.finish();
    }
}

/// Sends the sizes the cache kept from before.
fn replay(stream: &mut Output<'_>, entries: Vec<Entry>, progress: SearchProgress) {
    for entry in entries {
        if !stream.push(entry) {
            return;
        }
    }
    if stream.flush() && stream.progress(progress) {
        stream.listed(None);
    }
}

/// The folder to measure, with its device and inode. It must be a folder, not a link to one.
fn open_root(root: &Path) -> Result<(Directory, (u64, u64)), String> {
    let directory = Directory::open(root).map_err(|error| error.to_string())?;
    match fs::symlink_metadata(root) {
        Ok(meta) if meta.is_dir() => Ok((directory, (meta.dev(), meta.ino()))),
        _ => Err(tr!(search_directory_required).into()),
    }
}

/// A measurement under way: the folders open from the root down, and what it found so far.
/// Its steps answer `None` once the reader stops listening.
struct SizeScan<'a, 'b> {
    search: &'a SearchRequest,
    stream: &'a mut Output<'b>,
    query: Query,
    tree: bool,
    /// The root's device; folders on other file systems are left out.
    device: u64,
    /// Where the cache takes each folder measured, until it fails once.
    store: Option<ScanStore>,
    progress: SearchProgress,
    /// The largest folders so far, by size, the smallest first.
    largest: BTreeMap<(u64, PathBuf), Entry>,
    /// How many rows of a tree show, the root's among them.
    shown: usize,
    /// The root's folders that show in a tree, which show again once measured.
    frontier: HashSet<PathBuf>,
    stack: Vec<Directory>,
}

impl SizeScan<'_, '_> {
    /// Reads the tree depth first, saying how far it has got as it goes.
    fn measure(&mut self, root: Directory) -> Option<()> {
        if self.tree && !self.stream.background {
            going(self.stream.push(root.entry.clone()) && self.stream.flush())?;
        }
        self.stack.push(root);
        let mut last = Instant::now();
        while !self.stack.is_empty() {
            going(!self.stream.stopped())?;
            if last.elapsed() >= PROGRESS_EVERY {
                self.report()?;
                last = Instant::now();
            }
            match self.current().reader.next() {
                None => self.close()?,
                Some(Err(_)) => self.failed(),
                Some(Ok(item)) => self.visit(&item)?,
            }
        }
        Some(())
    }

    fn current(&mut self) -> &mut Directory {
        self.stack.last_mut().expect("a folder is open while the scan reads")
    }

    /// Says how far the measurement has got. A background scan also shows how far each open
    /// folder has got, so large folders grow on screen instead of staying blank until they finish.
    fn report(&mut self) -> Option<()> {
        if self.stream.background {
            for entry in provisional(&self.stack) {
                going(self.stream.push(entry))?;
            }
        }
        going(self.stream.flush() && self.stream.progress(self.progress.clone()))
    }

    /// The folder read to its end: it adds into the one around it and goes into the cache, and
    /// it shows in a tree or is kept among the largest.
    fn close(&mut self) -> Option<()> {
        let mut finished = self.stack.pop().expect("a folder is open while the scan reads");
        if let Some(parent) = self.stack.last_mut() {
            parent.add(&finished);
        }
        finished.entry.size = finished.bytes;
        finished.usage.measured = Some(SystemTime::now());
        finished.entry.usage = Some(finished.usage.clone());
        self.store(&finished.entry);
        let identity = finished.entry.identity();
        if self.tree && (self.stream.background || identity == self.search.root || self.frontier.contains(identity)) {
            going(self.stream.push(finished.entry.clone()))?;
        }
        if !self.tree
            && finished.entry.identity() != self.search.root
            && finished.bytes >= self.search.options.minimum
            && finished.usage.files >= self.search.options.minimum_files
            && self.query.accepts(&finished.entry)
        {
            self.keep(finished.bytes, finished.entry);
        }
        Some(())
    }

    /// An item of the folder being read: a file adds its size and a folder opens below it.
    /// Hidden items are left out unless the search shows them.
    fn visit(&mut self, item: &fs::DirEntry) -> Option<()> {
        if !self.search.hidden && item.file_name().as_encoded_bytes().starts_with(b".") {
            self.skipped();
            return Some(());
        }
        self.progress.visited += 1;
        let Ok(meta) = fs::symlink_metadata(item.path()) else {
            self.failed();
            return Some(());
        };
        if meta.is_dir() {
            return self.descend(&item.path(), &meta);
        }
        if meta.is_file() {
            self.current().add_file(meta.len());
        }
        Some(())
    }

    /// Opens a folder below the one being read, unless it is on another file system, too deep,
    /// a loop back to a folder above, or guarded.
    fn descend(&mut self, path: &Path, meta: &fs::Metadata) -> Option<()> {
        if meta.dev() != self.device
            || self.stack.len() >= MAX_DEPTH
            || revisits(self.stack.iter().map(|directory| directory.identity), meta)
            || guarded(path)
        {
            self.skipped();
            return Some(());
        }
        match Directory::open(path) {
            Ok(directory) => self.enter(directory),
            Err(_) => {
                self.failed();
                self.unopened(path)
            }
        }
    }

    /// Starts reading a folder. In a tree the root's folders show at once, up to the limit.
    fn enter(&mut self, directory: Directory) -> Option<()> {
        if self.tree
            && !self.stream.background
            && directory.entry.identity().parent() == Some(self.search.root.as_path())
        {
            if self.shown < RESULT_LIMIT {
                self.shown += 1;
                self.frontier.insert(directory.entry.identity().to_path_buf());
                going(self.stream.push(directory.entry.clone()))?;
            } else {
                self.progress.limited = true;
            }
        }
        self.stack.push(directory);
        Some(())
    }

    /// A folder that would not open counts as empty and partly measured: it goes into the cache,
    /// shows in a tree, and is among the largest when no minimum leaves it out.
    fn unopened(&mut self, path: &Path) -> Option<()> {
        let Ok(mut entry) = borg_vfs::entry(path) else { return Some(()) };
        entry.size = 0;
        entry.usage = Some(FolderUsage { partial: true, ..Default::default() });
        self.store(&entry);
        if self.tree
            && (self.stream.background || entry.identity().parent() == Some(self.search.root.as_path()))
            && self.shown < RESULT_LIMIT
        {
            self.shown += 1;
            going(self.stream.push(entry.clone()))?;
        }
        if self.search.options.minimum == 0 && self.search.options.minimum_files == 0 && self.query.accepts(&entry) {
            self.keep(0, entry);
        }
        Some(())
    }

    /// Counts what could not be read; the folder it is in is then only partly measured.
    fn failed(&mut self) {
        self.progress.errors += 1;
        self.current().usage.partial = true;
    }

    /// Counts what is left out; the folder it is in is then only partly measured.
    fn skipped(&mut self) {
        self.progress.skipped += 1;
        self.current().usage.partial = true;
    }

    /// Puts a measured folder into the cache, which takes no more once it fails.
    fn store(&mut self, entry: &Entry) {
        if self.store.as_ref().is_some_and(|store| store.put(entry).is_none()) {
            self.store = None;
        }
    }

    /// Keeps a folder among the largest, letting the smallest go past the limit.
    fn keep(&mut self, bytes: u64, entry: Entry) {
        self.largest.insert((bytes, entry.identity().to_path_buf()), entry);
        if self.largest.len() > RESULT_LIMIT {
            self.largest.pop_first();
            self.progress.limited = true;
        }
    }

    /// Saves the measurement in the cache and sends the largest folders, the largest first. A
    /// background scan only fills the cache, so it says so when the cache could not take it.
    fn finish(mut self) {
        self.progress.measured = Some(SystemTime::now());
        let saved =
            !self.stream.stopped() && self.store.take().is_some_and(|store| store.finish(&self.progress).is_some());
        if self.stream.background && !saved {
            self.stream.flush();
            self.stream.progress(self.progress);
            self.stream.listed(Some(tr!(background_sizes_cache_unavailable).into()));
            return;
        }
        if !self.tree {
            for (_, entry) in self.largest.into_iter().rev() {
                if !self.stream.push(entry) {
                    return;
                }
            }
        }
        if self.stream.flush() && self.stream.progress(self.progress) {
            self.stream.listed(None);
        }
    }
}

/// Goes on while the reader listens.
fn going(listening: bool) -> Option<()> {
    listening.then_some(())
}

/// A folder being read, with what it holds so far.
pub(super) struct Directory {
    pub(super) reader: fs::ReadDir,
    pub(super) entry: Entry,
    pub(super) usage: FolderUsage,
    pub(super) bytes: u64,
    /// Device and inode, to recognise a folder that contains itself.
    pub(super) identity: (u64, u64),
}

impl Directory {
    fn open(path: &Path) -> std::io::Result<Self> {
        let meta = fs::symlink_metadata(path)?;
        Ok(Self {
            reader: fs::read_dir(path)?,
            entry: borg_vfs::entry(path)?,
            usage: Default::default(),
            bytes: 0,
            identity: (meta.dev(), meta.ino()),
        })
    }

    fn add_file(&mut self, bytes: u64) {
        self.bytes = self.bytes.saturating_add(bytes);
        self.usage.direct_bytes = self.usage.direct_bytes.saturating_add(bytes);
        self.usage.direct_files = self.usage.direct_files.saturating_add(1);
        self.usage.files = self.usage.files.saturating_add(1);
    }

    /// Adds a folder below, measured to its end.
    fn add(&mut self, child: &Directory) {
        self.bytes = self.bytes.saturating_add(child.bytes);
        self.usage.files = self.usage.files.saturating_add(child.usage.files);
        self.usage.folders = self.usage.folders.saturating_add(child.usage.folders).saturating_add(1);
        self.usage.partial |= child.usage.partial;
    }
}

/// Whether entering `meta` would revisit a folder already being walked. Symlinks are never
/// followed, so this catches loops made by bind mounts or hard-linked folders on the same
/// filesystem, which would otherwise repeat until the depth limit.
pub(super) fn revisits(ancestors: impl IntoIterator<Item = (u64, u64)>, meta: &fs::Metadata) -> bool {
    ancestors.into_iter().any(|identity| identity == (meta.dev(), meta.ino()))
}

/// Running totals for the folders still being measured, innermost first: each includes the
/// folders below it that are still open, and is marked partial because it is still growing.
pub(super) fn provisional(stack: &[Directory]) -> Vec<Entry> {
    let mut below = (0u64, 0u64, 0u64);
    let mut entries = Vec::with_capacity(stack.len());
    for directory in stack.iter().rev() {
        let bytes = directory.bytes.saturating_add(below.0);
        let files = directory.usage.files.saturating_add(below.1);
        let folders = directory.usage.folders.saturating_add(below.2);
        let mut entry = directory.entry.clone();
        entry.size = bytes;
        entry.usage = Some(FolderUsage { files, folders, partial: true, ..directory.usage.clone() });
        entries.push(entry);
        below = (bytes, files, folders.saturating_add(1));
    }
    entries
}
