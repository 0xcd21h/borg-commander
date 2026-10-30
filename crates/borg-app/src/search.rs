use borg_i18n::tr;
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use borg_core::{Msg, Query, RESULT_LIMIT, SearchKind, SearchProgress, SearchQuery, SearchRequest, Side};
use borg_vfs::Entry;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

const CHANNEL_BATCHES: usize = 4;
const BATCH_SIZE: usize = 128;
const PROGRESS_EVERY: Duration = Duration::from_millis(150);
const BACKPRESSURE_WAIT: Duration = Duration::from_millis(5);
const MAX_DEPTH: usize = 256;
/// How deep a search looks first, so matches near its folder show before the whole tree is read.
const NEAR_DEPTH: usize = 3;
/// Borg's own ignore file, read like `.gitignore` where a search leaves ignored files out.
const BORG_IGNORE: &str = ".borgignore";

#[derive(Clone)]
enum Work {
    Search(SearchRequest),
    Refresh(Vec<Entry>),
}

#[derive(Clone)]
struct Request {
    generation: u64,
    work: Work,
    cancelled: Arc<AtomicBool>,
}

struct Worker {
    requests: watch::Sender<Option<Request>>,
    cancelled: Option<Arc<AtomicBool>>,
    task: JoinHandle<()>,
}

impl Worker {
    fn new(side: Side, messages: mpsc::Sender<Msg>, stores: Stores, background: bool) -> Self {
        let (requests, mut incoming) = watch::channel(None::<Request>);
        let task = tokio::spawn(async move {
            while incoming.changed().await.is_ok() {
                let Some(request) = incoming.borrow_and_update().clone() else { continue };
                let messages = messages.clone();
                let stores = stores.clone();
                let _ = tokio::task::spawn_blocking(move || run(side, request, &messages, &stores, background)).await;
            }
        });
        Self { requests, cancelled: None, task }
    }

    fn cancel(&mut self) {
        if let Some(cancelled) = self.cancelled.take() {
            cancelled.store(true, Ordering::Relaxed);
            self.requests.send_replace(None);
        }
    }

    fn request(&mut self, generation: u64, work: Work) {
        self.cancel();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.cancelled = Some(cancelled.clone());
        self.requests.send_replace(Some(Request { generation, work, cancelled }));
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
        self.task.abort();
    }
}

/// What searches read besides the filesystem: saved folder sizes and name indexes.
#[derive(Clone)]
struct Stores {
    cache: crate::size_cache::SizeCache,
    indexes: crate::index::Indexes,
}

pub struct Search {
    pub cache: crate::size_cache::SizeCache,
    pub indexes: crate::index::Indexes,
    workers: [Worker; 2],
    background: Worker,
    pub incoming: mpsc::Receiver<Msg>,
}

impl Search {
    pub fn new(refresh: Duration) -> Self {
        let indexes = crate::index::Indexes::new(crate::size_cache::cache_dir(), refresh);
        Self::with_stores(crate::size_cache::SizeCache::locate(), indexes)
    }
    #[cfg(test)]
    pub fn with_cache(cache: crate::size_cache::SizeCache) -> Self {
        Self::with_stores(cache, crate::index::Indexes::new(None, Duration::from_secs(60)))
    }
    fn with_stores(cache: crate::size_cache::SizeCache, indexes: crate::index::Indexes) -> Self {
        let (messages, incoming) = mpsc::channel(CHANNEL_BATCHES);
        let stores = Stores { cache: cache.clone(), indexes: indexes.clone() };
        Self {
            workers: Side::ALL.map(|side| Worker::new(side, messages.clone(), stores.clone(), false)),
            background: Worker::new(Side::Left, messages, stores, true),
            incoming,
            cache,
            indexes,
        }
    }
    pub fn measure(&mut self, id: u64, root: std::path::PathBuf) {
        self.background.request(
            id,
            Work::Search(SearchRequest {
                skip: None,
                root,
                query: "*".into(),
                hidden: true,
                options: borg_core::SearchOptions { kind: SearchKind::FolderTree, refresh: true, ..Default::default() },
            }),
        );
    }
    pub fn cancel_measure(&mut self) {
        self.background.cancel();
    }
    pub fn search(&mut self, side: Side, generation: u64, request: SearchRequest) {
        self.workers[side.index()].request(generation, Work::Search(request));
    }
    pub fn refresh(&mut self, side: Side, generation: u64, entries: Vec<Entry>) {
        self.workers[side.index()].request(generation, Work::Refresh(entries));
    }
    pub fn cancel(&mut self, side: Side) {
        self.workers[side.index()].cancel();
    }
}

fn run(side: Side, request: Request, messages: &mpsc::Sender<Msg>, stores: &Stores, background: bool) {
    let cache = &stores.cache;
    let mut stream = Output {
        side,
        generation: request.generation,
        messages,
        cancelled: &request.cancelled,
        batch: Vec::with_capacity(BATCH_SIZE),
        background,
    };
    match request.work {
        Work::Refresh(entries) => {
            for entry in entries {
                if stream.stopped() {
                    return;
                }
                let path = entry.identity();
                let fresh = borg_vfs::entry(path).unwrap_or(Entry { missing: true, ..entry });
                if !stream.push(fresh) {
                    return;
                }
            }
        }
        Work::Search(search) if matches!(search.options.kind, SearchKind::FolderTotals | SearchKind::FolderTree) => {
            scan_sizes(&search, &mut stream, cache);
            return;
        }
        Work::Search(search) => {
            let answered =
                stores.indexes.covering(&search).and_then(|index| search_index(&index, &search, &mut stream));
            if !answered.unwrap_or_else(|| search_names(&search, &mut stream)) {
                return;
            }
        }
    }
    if stream.flush() {
        stream.send(Msg::Listed { side, generation: request.generation, error: None });
    }
}

/// Streams the entries below the search folder whose names match, leaving out what the request
/// skips. A left-out folder is still a result when it matches; only its contents are not read.
/// False when the search stopped before finishing.
fn search_names(search: &SearchRequest, stream: &mut Output<'_>) -> bool {
    let (side, generation) = (stream.side, stream.generation);
    let failed = |stream: &Output<'_>, error: String| {
        stream.send(Msg::Listed { side, generation, error: Some(error) });
        false
    };
    let query = match SearchQuery::parse(&search.query) {
        Ok(query) => query,
        Err(error) => return failed(stream, error.message()),
    };
    if let Err(error) = fs::read_dir(&search.root) {
        return failed(stream, error.to_string());
    }
    let now = std::time::SystemTime::now();
    let metadata = query.needs_metadata() || search.options.minimum > 0;
    let wanted = |entry: &Entry, path: &Path| {
        let kind = match search.options.kind {
            SearchKind::Files => entry.kind == borg_vfs::EntryKind::File,
            SearchKind::Folders => entry.kind == borg_vfs::EntryKind::Dir,
            _ => true,
        };
        kind && (search.options.minimum == 0 || (!entry.is_dir() && entry.size >= search.options.minimum))
            && query.accepts(&(entry, path), now)
    };
    let depth = |path: &Path| path.strip_prefix(&search.root).map_or(0, |rest| rest.components().count());
    let mut progress = SearchProgress::default();
    let mut found = 0;
    let mut last = Instant::now();
    // A first pass reads only the folders near the search folder, so what lies close by shows at
    // once; the whole walk after it passes over what the first one looked at.
    'passes: for near in [true, false] {
        let Walker { mut walk, left_out, hidden } = match Walker::new(&search.root, search.hidden, search.skip.as_ref())
        {
            Ok(walker) => walker,
            Err(error) => return failed(stream, error),
        };
        if near {
            walk.max_depth(Some(NEAR_DEPTH));
        }
        let seen = |depth: usize| !near && depth <= NEAR_DEPTH;
        let hidden_before = progress.skipped;
        // Folders left out of the last folder read come after its last entry, so a last turn
        // takes them.
        for item in walk.build().map(Some).chain([None]) {
            if stream.stopped() {
                return false;
            }
            let mut candidates = Vec::new();
            match item {
                Some(Ok(item)) if item.depth() == 0 => continue,
                Some(Ok(item)) if !seen(item.depth()) => {
                    progress.visited += 1;
                    candidates.push((item.path().to_path_buf(), item.file_type()));
                }
                Some(Ok(_)) | None => {}
                Some(Err(error)) => {
                    progress.errors += usize::from(!error.depth().is_some_and(seen));
                    continue;
                }
            }
            let drained: Vec<_> =
                Walker::take_left_out(&left_out).into_iter().filter(|path| !seen(depth(path))).collect();
            progress.excluded += drained.len();
            candidates.extend(drained.into_iter().map(|path| (path, None)));
            for (path, kind) in candidates {
                // Name-only queries match on what the listing gives; only matches read metadata.
                let cheap = (!metadata).then_some(kind).flatten().filter(|kind| !kind.is_symlink()).map(|kind| Entry {
                    source: Some(path.clone()),
                    missing: false,
                    usage: None,
                    name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                    kind: if kind.is_dir() { borg_vfs::EntryKind::Dir } else { borg_vfs::EntryKind::File },
                    size: 0,
                    modified: None,
                    executable: false,
                    permissions: 0,
                });
                if cheap.as_ref().is_some_and(|entry| !wanted(entry, &path)) {
                    continue;
                }
                let entry = match borg_vfs::entry(&path) {
                    Ok(entry) => entry,
                    Err(_) => {
                        progress.errors += 1;
                        continue;
                    }
                };
                if (cheap.is_none() && !wanted(&entry, &path)) || !stream.push(entry) {
                    if stream.stopped() {
                        return false;
                    }
                    continue;
                }
                found += 1;
                if found >= RESULT_LIMIT {
                    progress.limited = true;
                    break 'passes;
                }
            }
            if last.elapsed() >= PROGRESS_EVERY {
                progress.skipped = hidden_before.max(hidden.load(Ordering::Relaxed));
                if !stream.flush() || !stream.send(Msg::SearchProgress { side, generation, progress: progress.clone() })
                {
                    return false;
                }
                last = Instant::now();
            }
        }
        // The whole walk counts every hidden name again, the near ones included.
        progress.skipped = hidden_before.max(hidden.load(Ordering::Relaxed));
    }
    stream.flush() && stream.send(Msg::SearchProgress { side, generation, progress })
}

/// Answers from an index: `None` when its folder is not in it, so a live search must answer;
/// otherwise whether the search finished. Matches are read again, so deleted files drop out.
fn search_index(index: &crate::index::Index, search: &SearchRequest, stream: &mut Output<'_>) -> Option<bool> {
    let query = SearchQuery::parse(&search.query).ok()?;
    let found = index.search(search, &query, RESULT_LIMIT)?;
    let mut progress = SearchProgress {
        visited: index.entries(),
        limited: found.limited || !index.complete,
        indexed: Some(index.built),
        ..Default::default()
    };
    for path in found.paths {
        if stream.stopped() {
            return Some(false);
        }
        match borg_vfs::entry(&path) {
            Ok(entry) => {
                if !stream.push(entry) {
                    return Some(false);
                }
            }
            Err(_) => progress.skipped += 1,
        }
    }
    let (side, generation) = (stream.side, stream.generation);
    Some(stream.flush() && stream.send(Msg::SearchProgress { side, generation, progress }))
}

/// A walk below a folder that leaves out what a search skips. Left-out folders are collected
/// rather than dropped, so a search can still match their names; hidden names are counted.
pub(crate) struct Walker {
    pub walk: ignore::WalkBuilder,
    pub left_out: Arc<std::sync::Mutex<Vec<PathBuf>>>,
    pub hidden: Arc<std::sync::atomic::AtomicUsize>,
}

impl Walker {
    pub fn new(root: &Path, show_hidden: bool, skip: Option<&borg_core::SearchSettings>) -> Result<Self, String> {
        let mut walk = ignore::WalkBuilder::new(root);
        walk.standard_filters(false).same_file_system(true).max_depth(Some(MAX_DEPTH));
        if skip.is_some_and(|skip| skip.ignore_files) {
            walk.git_ignore(true).git_global(true).git_exclude(true).ignore(true).parents(true).require_git(false);
            walk.add_custom_ignore_filename(".rgignore").add_custom_ignore_filename(BORG_IGNORE);
        }
        if let Some(globs) = skip.map(|skip| &skip.exclude).filter(|globs| !globs.is_empty()) {
            let mut overrides = ignore::overrides::OverrideBuilder::new(root);
            for glob in globs {
                overrides.add(&format!("!{glob}")).map_err(|error| error.to_string())?;
            }
            walk.overrides(overrides.build().map_err(|error| error.to_string())?);
        }
        let left_out = Arc::new(std::sync::Mutex::new(Vec::new()));
        let hidden = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (found, counted) = (left_out.clone(), hidden.clone());
        let artifacts = skip.is_some_and(|skip| skip.artifacts);
        walk.filter_entry(move |entry| {
            if entry.depth() == 0 {
                return true;
            }
            if !show_hidden && entry.file_name().as_encoded_bytes().starts_with(b".") {
                counted.fetch_add(1, Ordering::Relaxed);
                return false;
            }
            let left_out = entry.file_type().is_some_and(|kind| kind.is_dir())
                && (guarded(entry.path()) || (artifacts && generated(entry.path())));
            if left_out {
                found.lock().expect("left-out folders").push(entry.path().to_path_buf());
            }
            !left_out
        });
        Ok(Self { walk, left_out, hidden })
    }

    /// The folders left out since the last call.
    pub fn take_left_out(left_out: &std::sync::Mutex<Vec<PathBuf>>) -> Vec<PathBuf> {
        std::mem::take(&mut *left_out.lock().expect("left-out folders"))
    }
}

/// Whether `path` is where macOS keeps other apps' data, `~/Library/Containers` or `Group
/// Containers`. macOS asks whether to allow reading each app's folder there, and a read waits for
/// the answer, so walks leave the whole folder out, before anything in it is opened, whatever a
/// search skips. A search started there reads it.
fn guarded(path: &Path) -> bool {
    cfg!(target_os = "macos")
        && matches!(path.file_name().and_then(|name| name.to_str()), Some("Containers" | "Group Containers"))
        && path.parent().and_then(Path::file_name).is_some_and(|name| name == "Library")
}

/// Whether the folder at `path` is generated: by its name and marker files, or a cache tag.
fn generated(path: &Path) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    borg_core::is_artifact(&name, |file| path.with_file_name(file).exists(), |file| path.join(file).exists())
        || cache_tagged(path)
}

fn cache_tagged(path: &Path) -> bool {
    use std::io::Read;
    let mut start = [0; borg_core::CACHEDIR_SIGNATURE.len()];
    fs::File::open(path.join(borg_core::CACHEDIR_TAG))
        .and_then(|mut file| file.read_exact(&mut start))
        .is_ok_and(|()| start == borg_core::CACHEDIR_SIGNATURE)
}

struct Directory {
    reader: fs::ReadDir,
    entry: Entry,
    usage: borg_vfs::FolderUsage,
    bytes: u64,
    /// Device and inode, to recognise a folder that contains itself.
    identity: (u64, u64),
}

/// Whether entering `meta` would revisit a folder already being walked. Symlinks are never
/// followed, so this catches loops made by bind mounts or hard-linked folders on the same
/// filesystem, which would otherwise repeat until the depth limit.
fn revisits(ancestors: impl IntoIterator<Item = (u64, u64)>, meta: &fs::Metadata) -> bool {
    ancestors.into_iter().any(|identity| identity == (meta.dev(), meta.ino()))
}

/// Running totals for the folders still being measured, innermost first: each includes the
/// folders below it that are still open, and is marked partial because it is still growing.
fn provisional(stack: &[Directory]) -> Vec<Entry> {
    let mut below = (0u64, 0u64, 0u64);
    let mut entries = Vec::with_capacity(stack.len());
    for directory in stack.iter().rev() {
        let bytes = directory.bytes.saturating_add(below.0);
        let files = directory.usage.files.saturating_add(below.1);
        let folders = directory.usage.folders.saturating_add(below.2);
        let mut entry = directory.entry.clone();
        entry.size = bytes;
        entry.usage = Some(borg_vfs::FolderUsage { files, folders, partial: true, ..directory.usage.clone() });
        entries.push(entry);
        below = (bytes, files, folders.saturating_add(1));
    }
    entries
}

fn scan_sizes(search: &SearchRequest, stream: &mut Output<'_>, cache: &crate::size_cache::SizeCache) {
    let tree = search.options.kind == SearchKind::FolderTree;
    let epoch = cache.epoch();
    if !search.options.refresh
        && let Some((entries, progress)) =
            if tree { cache.tree(&search.root, search.hidden) } else { cache.flat(search, || stream.stopped()) }
    {
        for entry in entries {
            if !stream.push(entry) {
                return;
            }
        }
        if stream.flush()
            && stream.send(Msg::SearchProgress { side: stream.side, generation: stream.generation, progress })
        {
            stream.send(Msg::Listed { side: stream.side, generation: stream.generation, error: None });
        }
        return;
    }
    let mut progress = SearchProgress::default();
    let mut largest = std::collections::BTreeMap::new();
    let mut visible_count = 1usize;
    let mut frontier = std::collections::HashSet::new();
    let query = Query::parse(&search.query);
    let open = |path: &std::path::Path| -> std::io::Result<Directory> {
        let meta = fs::symlink_metadata(path)?;
        Ok(Directory {
            reader: fs::read_dir(path)?,
            entry: borg_vfs::entry(path)?,
            usage: Default::default(),
            bytes: 0,
            identity: (meta.dev(), meta.ino()),
        })
    };
    let root = match open(&search.root) {
        Ok(root) => root,
        Err(error) => {
            stream.send(Msg::Listed {
                side: stream.side,
                generation: stream.generation,
                error: Some(error.to_string()),
            });
            return;
        }
    };
    let identity = match fs::symlink_metadata(&search.root) {
        Ok(meta) if meta.is_dir() => (meta.dev(), meta.ino()),
        _ => {
            stream.send(Msg::Listed {
                side: stream.side,
                generation: stream.generation,
                error: Some(tr!(search_directory_required).into()),
            });
            return;
        }
    };
    let device = identity.0;
    let mut store = cache.begin(&search.root, search.hidden, identity, epoch);
    if tree && !stream.background && (!stream.push(root.entry.clone()) || !stream.flush()) {
        return;
    }
    let mut stack = vec![root];
    let mut last = Instant::now();
    while !stack.is_empty() {
        if stream.stopped() {
            return;
        }
        if last.elapsed() >= PROGRESS_EVERY {
            // Background scans also report how far each open folder has got, so large
            // folders grow on screen instead of staying blank until they finish.
            if stream.background && provisional(&stack).into_iter().any(|entry| !stream.push(entry)) {
                return;
            }
            if !stream.flush()
                || !stream.send(Msg::SearchProgress {
                    side: stream.side,
                    generation: stream.generation,
                    progress: progress.clone(),
                })
            {
                return;
            }
            last = Instant::now();
        }
        let item = stack.last_mut().unwrap().reader.next();
        let Some(item) = item else {
            let mut finished = stack.pop().unwrap();
            if let Some(parent) = stack.last_mut() {
                parent.bytes = parent.bytes.saturating_add(finished.bytes);
                parent.usage.files = parent.usage.files.saturating_add(finished.usage.files);
                parent.usage.folders = parent.usage.folders.saturating_add(finished.usage.folders).saturating_add(1);
                parent.usage.partial |= finished.usage.partial;
            }
            finished.entry.size = finished.bytes;
            finished.usage.measured = Some(std::time::SystemTime::now());
            finished.entry.usage = Some(finished.usage.clone());
            if store.as_ref().is_some_and(|store| store.put(&finished.entry).is_none()) {
                store = None;
            }
            if tree
                && (stream.background
                    || finished.entry.identity() == search.root
                    || frontier.contains(finished.entry.identity()))
                && !stream.push(finished.entry.clone())
            {
                return;
            }
            if !tree
                && finished.entry.identity() != search.root
                && finished.bytes >= search.options.minimum
                && finished.usage.files >= search.options.minimum_files
                && query.accepts(&finished.entry)
            {
                largest.insert((finished.bytes, finished.entry.identity().to_path_buf()), finished.entry);
                if largest.len() > RESULT_LIMIT {
                    largest.pop_first();
                    progress.limited = true;
                }
            }
            continue;
        };
        let item = match item {
            Ok(item) => item,
            Err(_) => {
                progress.errors += 1;
                stack.last_mut().unwrap().usage.partial = true;
                continue;
            }
        };
        if !search.hidden && item.file_name().as_encoded_bytes().starts_with(b".") {
            progress.skipped += 1;
            stack.last_mut().unwrap().usage.partial = true;
            continue;
        }
        progress.visited += 1;
        let meta = match fs::symlink_metadata(item.path()) {
            Ok(meta) => meta,
            Err(_) => {
                progress.errors += 1;
                stack.last_mut().unwrap().usage.partial = true;
                continue;
            }
        };
        if meta.is_dir() {
            if meta.dev() != device
                || stack.len() >= MAX_DEPTH
                || revisits(stack.iter().map(|directory| directory.identity), &meta)
                || guarded(&item.path())
            {
                progress.skipped += 1;
                stack.last_mut().unwrap().usage.partial = true;
                continue;
            }
            match open(&item.path()) {
                Ok(directory) => {
                    if tree && !stream.background && directory.entry.identity().parent() == Some(search.root.as_path())
                    {
                        if visible_count < RESULT_LIMIT {
                            visible_count += 1;
                            frontier.insert(directory.entry.identity().to_path_buf());
                            if !stream.push(directory.entry.clone()) {
                                return;
                            }
                        } else {
                            progress.limited = true;
                        }
                    }
                    stack.push(directory);
                }
                Err(_) => {
                    progress.errors += 1;
                    stack.last_mut().unwrap().usage.partial = true;
                    if let Ok(mut entry) = borg_vfs::entry(&item.path()) {
                        entry.size = 0;
                        entry.usage = Some(borg_vfs::FolderUsage { partial: true, ..Default::default() });
                        if store.as_ref().is_some_and(|store| store.put(&entry).is_none()) {
                            store = None;
                        }
                        if tree
                            && (stream.background || entry.identity().parent() == Some(search.root.as_path()))
                            && visible_count < RESULT_LIMIT
                        {
                            visible_count += 1;
                            if !stream.push(entry.clone()) {
                                return;
                            }
                        }
                        if search.options.minimum == 0 && search.options.minimum_files == 0 && query.accepts(&entry) {
                            largest.insert((0, entry.identity().to_path_buf()), entry);
                            if largest.len() > RESULT_LIMIT {
                                largest.pop_first();
                                progress.limited = true;
                            }
                        }
                    }
                }
            }
        } else if meta.is_file() {
            let current = stack.last_mut().unwrap();
            current.bytes = current.bytes.saturating_add(meta.len());
            current.usage.direct_bytes = current.usage.direct_bytes.saturating_add(meta.len());
            current.usage.direct_files = current.usage.direct_files.saturating_add(1);
            current.usage.files = current.usage.files.saturating_add(1);
        }
    }
    progress.measured = Some(std::time::SystemTime::now());
    let saved = !stream.stopped() && store.is_some_and(|store| store.finish(&progress).is_some());
    if stream.background && !saved {
        stream.flush();
        stream.send(Msg::SearchProgress { side: stream.side, generation: stream.generation, progress });
        stream.send(Msg::Listed {
            side: stream.side,
            generation: stream.generation,
            error: Some(tr!(background_sizes_cache_unavailable).into()),
        });
        return;
    }
    for (_, entry) in largest.into_iter().rev().filter(|_| !tree) {
        if !stream.push(entry) {
            return;
        }
    }
    if stream.flush() && stream.send(Msg::SearchProgress { side: stream.side, generation: stream.generation, progress })
    {
        stream.send(Msg::Listed { side: stream.side, generation: stream.generation, error: None });
    }
}

struct Output<'a> {
    side: Side,
    generation: u64,
    messages: &'a mpsc::Sender<Msg>,
    cancelled: &'a AtomicBool,
    batch: Vec<Entry>,
    background: bool,
}

impl Output<'_> {
    fn stopped(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed) || self.messages.is_closed()
    }
    fn send(&self, msg: Msg) -> bool {
        let mut msg = if self.background {
            match msg {
                Msg::Batch { generation, entries, .. } => Msg::Sized { id: generation, entries },
                Msg::SearchProgress { generation, progress, .. } => Msg::SizeProgress { id: generation, progress },
                Msg::Listed { generation, error, .. } => Msg::SizesFinished { id: generation, error },
                other => other,
            }
        } else {
            msg
        };
        loop {
            if self.stopped() {
                return false;
            }
            match self.messages.try_send(msg) {
                Ok(()) => return true,
                Err(mpsc::error::TrySendError::Closed(_)) => return false,
                Err(mpsc::error::TrySendError::Full(returned)) => {
                    msg = returned;
                    std::thread::park_timeout(BACKPRESSURE_WAIT);
                }
            }
        }
    }
    fn push(&mut self, entry: Entry) -> bool {
        self.batch.push(entry);
        self.batch.len() < BATCH_SIZE || self.flush()
    }
    fn flush(&mut self) -> bool {
        if self.batch.is_empty() {
            return !self.stopped();
        }
        let entries = std::mem::replace(&mut self.batch, Vec::with_capacity(BATCH_SIZE));
        self.send(Msg::Batch { side: self.side, generation: self.generation, entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[tokio::test]
    async fn folder_totals_find_many_small_files_and_filter_only_after_measuring() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("project/node_modules/.cache")).unwrap();
        fs::create_dir_all(root.path().join("small")).unwrap();
        for index in 0..300 {
            fs::write(root.path().join(format!("project/node_modules/.cache/{index}")), "1234567890").unwrap();
        }
        fs::write(root.path().join("small/file"), "x").unwrap();
        symlink(root.path(), root.path().join("project/node_modules/loop")).unwrap();
        let mut worker = Search::with_cache(crate::size_cache::SizeCache::at(None));
        worker.search(
            Side::Left,
            9,
            SearchRequest {
                skip: None,
                root: root.path().into(),
                query: "node_modules".into(),
                hidden: true,
                options: borg_core::SearchOptions {
                    kind: SearchKind::FolderTotals,
                    minimum: 2000,
                    minimum_files: 200,
                    ..Default::default()
                },
            },
        );
        let mut found = Vec::new();
        loop {
            match tokio::time::timeout(Duration::from_secs(5), worker.incoming.recv()).await.unwrap().unwrap() {
                Msg::Batch { entries, .. } => found.extend(entries),
                Msg::Listed { error, .. } => {
                    assert!(error.is_none());
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "node_modules");
        assert_eq!(found[0].size, 3000);
        assert_eq!(found[0].usage.as_ref().unwrap().files, 300);
        assert!(!found[0].usage.as_ref().unwrap().partial);
        worker.search(
            Side::Left,
            10,
            SearchRequest {
                skip: None,
                root: root.path().into(),
                query: "node_modules".into(),
                hidden: false,
                options: borg_core::SearchOptions { kind: SearchKind::FolderTotals, ..Default::default() },
            },
        );
        let mut partial = Vec::new();
        loop {
            match tokio::time::timeout(Duration::from_secs(5), worker.incoming.recv()).await.unwrap().unwrap() {
                Msg::Batch { entries, .. } => partial.extend(entries),
                Msg::Listed { .. } => break,
                _ => {}
            }
        }
        assert_eq!(partial[0].size, 0);
        assert!(partial[0].usage.as_ref().unwrap().partial);
    }

    #[tokio::test]
    async fn recursive_search_preserves_sources_and_does_not_follow_symlink_loops() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("one/nested")).unwrap();
        fs::create_dir(root.path().join("two")).unwrap();
        fs::create_dir(root.path().join(".hidden")).unwrap();
        for path in ["one/config.toml", "two/config.toml", ".hidden/config.toml", "one/nested/other.txt"] {
            fs::write(root.path().join(path), "hello").unwrap();
        }
        symlink(root.path(), root.path().join("one/loop")).unwrap();
        let mut search = Search::with_cache(crate::size_cache::SizeCache::at(None));
        search.search(
            Side::Left,
            7,
            SearchRequest {
                skip: None,
                root: root.path().into(),
                query: "*.toml".into(),
                hidden: false,
                options: Default::default(),
            },
        );
        let mut found = Vec::new();
        let progress = loop {
            match tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap() {
                Msg::Batch { entries, .. } => found.extend(entries),
                Msg::SearchProgress { progress, .. } => break progress,
                _ => {}
            }
        };
        assert_eq!(found.len(), 2);
        assert_ne!(found[0].source, found[1].source);
        assert_eq!(found[0].name, found[1].name);
        assert_eq!(progress.errors, 0);
        assert_eq!(progress.skipped, 1);
        assert!(!progress.limited);
    }

    #[tokio::test]
    async fn matches_near_the_search_folder_come_first_and_nothing_is_counted_twice() {
        let root = tempfile::tempdir().unwrap();
        let deep = root.path().join("a/b/c/d/e");
        fs::create_dir_all(&deep).unwrap();
        fs::create_dir(root.path().join("Downloads")).unwrap();
        fs::write(deep.join("регламент 2019.txt"), "old").unwrap();
        fs::write(root.path().join("Downloads/Регламент передачи ключей.pdf"), "new").unwrap();
        let mut search = Search::with_cache(crate::size_cache::SizeCache::at(None));
        search.search(
            Side::Left,
            7,
            SearchRequest {
                skip: None,
                root: root.path().into(),
                query: "*регламент*".into(),
                hidden: false,
                options: Default::default(),
            },
        );
        let mut found = Vec::new();
        let progress = loop {
            match tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap() {
                Msg::Batch { entries, .. } => found.extend(entries.into_iter().map(|entry| entry.name)),
                Msg::SearchProgress { progress, .. } => break progress,
                _ => {}
            }
        };
        assert_eq!(found, ["Регламент передачи ключей.pdf", "регламент 2019.txt"]);
        assert_eq!(progress.visited, 8, "five folders, Downloads and two files, each once");
        assert_eq!(progress.errors, 0);
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn other_apps_data_is_left_out_from_above_and_read_when_searched_in_its_own_folder() {
        let root = tempfile::tempdir().unwrap();
        let groups = root.path().join("Library/Group Containers");
        for folder in
            ["Library/Group Containers/TEAM.group.chat/Downloads", "Library/Containers/org.example.notes/Data"]
        {
            fs::create_dir_all(root.path().join(folder)).unwrap();
            fs::write(root.path().join(folder).join("report.txt"), "kept by an app").unwrap();
        }
        fs::write(root.path().join("Library/report.txt"), "mine").unwrap();
        async fn found(search: &mut Search, root: &Path, id: u64) -> (Vec<PathBuf>, usize) {
            let (skip, query, hidden, options) = (None, "report".into(), false, Default::default());
            search.search(Side::Left, id, SearchRequest { skip, root: root.into(), query, hidden, options });
            let mut found = Vec::new();
            loop {
                match tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap() {
                    Msg::Batch { entries, .. } => {
                        found.extend(entries.iter().map(|entry| entry.identity().to_path_buf()))
                    }
                    Msg::SearchProgress { progress, .. } => return (found, progress.excluded),
                    _ => {}
                }
            }
        }
        let mut search = Search::with_cache(crate::size_cache::SizeCache::at(None));
        let (found_above, excluded) = found(&mut search, root.path(), 1).await;
        assert_eq!(found_above, [root.path().join("Library/report.txt")]);
        assert_eq!(excluded, 2, "both folders of apps' data are left out, once each");
        let (found_inside, _) = found(&mut search, &groups, 2).await;
        assert_eq!(found_inside, [groups.join("TEAM.group.chat/Downloads/report.txt")]);
    }

    #[tokio::test]
    async fn name_searches_leave_out_generated_ignored_and_configured_paths_unless_asked() {
        let root = tempfile::tempdir().unwrap();
        let files = [
            "app/main.rs",
            "app/node_modules/dep/index.rs",
            "app/Cargo.toml",
            "app/target/debug/build.rs",
            "notes/target/plan.rs",
            "cache/CACHEDIR.TAG",
            "cache/blob.rs",
            "gen/out.rs",
            "app/ignored.rs",
            "big/huge.rs",
        ];
        for file in files {
            let path = root.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        fs::write(root.path().join("cache/CACHEDIR.TAG"), borg_core::CACHEDIR_SIGNATURE).unwrap();
        fs::write(root.path().join("app/.gitignore"), "ignored.rs\n").unwrap();
        fs::write(root.path().join(".borgignore"), "gen/\n").unwrap();
        let base = root.path().to_path_buf();
        let run = |query: &str, skip: Option<borg_core::SearchSettings>| {
            let base = base.clone();
            let mut search = Search::with_cache(crate::size_cache::SizeCache::at(None));
            let request = SearchRequest {
                root: base.clone(),
                query: query.into(),
                hidden: false,
                options: Default::default(),
                skip,
            };
            async move {
                search.search(Side::Left, 1, request);
                let mut found = Vec::new();
                loop {
                    match tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap() {
                        Msg::Batch { entries, .. } => found.extend(entries),
                        Msg::Listed { .. } => break,
                        _ => {}
                    }
                }
                let mut paths: Vec<String> = found
                    .iter()
                    .map(|entry| entry.identity().strip_prefix(&base).unwrap().display().to_string())
                    .collect();
                paths.sort();
                paths
            }
        };
        let settings = borg_core::SearchSettings { exclude: vec!["big".into()], ..Default::default() };
        assert_eq!(run("ext:rs", Some(settings.clone())).await, ["app/main.rs", "notes/target/plan.rs"]);
        assert_eq!(
            run("node_modules | target", Some(settings.clone())).await,
            ["app/node_modules", "app/target", "notes/target"]
        );
        assert_eq!(run("ext:rs", None).await.len(), 8, "nothing is left out without exclusions");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn indexed_folders_answer_from_their_index_and_searching_everything_walks() {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        for file in ["src/main.rs", "src/lib.rs", "docs/main.md"] {
            let path = root.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        let indexes = crate::index::Indexes::new(Some(cache.path().into()), Duration::from_secs(600));
        let (messages, mut built) = tokio::sync::mpsc::unbounded_channel();
        indexes.connect(messages);
        indexes.set_roots(vec![root.path().into()]);
        indexes.refresh(root.path().into(), borg_core::SearchSettings::default());
        let indexed = tokio::time::timeout(Duration::from_secs(5), built.recv()).await.unwrap();
        assert!(matches!(indexed, Some(Msg::Indexed { entries: 5, error: None, .. })), "{indexed:?}");
        let mut search = Search::with_stores(crate::size_cache::SizeCache::at(None), indexes);
        for (skip, from_index) in [(Some(borg_core::SearchSettings::default()), true), (None, false)] {
            let request = SearchRequest {
                root: root.path().join("src"),
                query: "main".into(),
                hidden: false,
                options: Default::default(),
                skip,
            };
            search.search(Side::Left, 1, request);
            let mut found = Vec::new();
            let progress = loop {
                match tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap() {
                    Msg::Batch { entries, .. } => found.extend(entries),
                    Msg::SearchProgress { progress, .. } => break progress,
                    _ => {}
                }
            };
            assert_eq!(found.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(), ["main.rs"]);
            assert_eq!(progress.indexed.is_some(), from_index);
        }
    }

    #[tokio::test]
    async fn refresh_keeps_missing_references_and_raw_filenames() {
        let root = tempfile::tempdir().unwrap();
        #[cfg(target_os = "linux")]
        let name = {
            use std::os::unix::ffi::OsStringExt;
            std::ffi::OsString::from_vec(b"raw-\xff.txt".to_vec())
        };
        #[cfg(not(target_os = "linux"))]
        let name = std::ffi::OsString::from("日本語.txt");
        let path = root.path().join(name);
        fs::write(&path, "hello").unwrap();
        let missing = root.path().join("missing");
        let mut search = Search::with_cache(crate::size_cache::SizeCache::at(None));
        search.refresh(Side::Right, 3, vec![Entry::unavailable(path.clone()), Entry::unavailable(missing.clone())]);
        let mut found = Vec::new();
        loop {
            match tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap() {
                Msg::Batch { entries, .. } => found.extend(entries),
                Msg::Listed { .. } => break,
                _ => {}
            }
        }
        assert_eq!(found[0].identity(), path);
        assert!(!found[0].missing);
        assert_eq!(found[1].identity(), missing);
        assert!(found[1].missing);
    }

    #[tokio::test]
    async fn replacing_a_search_cancels_backpressure_without_blocking_the_next_request() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..1500 {
            fs::write(root.path().join(format!("file-{index}")), "x").unwrap();
        }
        let mut search = Search::with_cache(crate::size_cache::SizeCache::at(None));
        search.search(
            Side::Left,
            1,
            SearchRequest {
                skip: None,
                root: root.path().into(),
                query: "*".into(),
                hidden: false,
                options: Default::default(),
            },
        );
        let _ = tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap();
        search.search(
            Side::Left,
            2,
            SearchRequest {
                skip: None,
                root: root.path().into(),
                query: "file-1499".into(),
                hidden: false,
                options: Default::default(),
            },
        );
        loop {
            if let Msg::Listed { generation: 2, .. } =
                tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap()
            {
                break;
            }
        }
    }
    async fn measured_tree(
        search: &mut Search,
        root: &std::path::Path,
        refresh: bool,
    ) -> (Vec<Entry>, borg_core::SearchProgress) {
        measured_request(
            search,
            SearchRequest {
                skip: None,
                root: root.into(),
                query: "*".into(),
                hidden: true,
                options: borg_core::SearchOptions { kind: SearchKind::FolderTree, refresh, ..Default::default() },
            },
        )
        .await
    }

    async fn measured_request(search: &mut Search, request: SearchRequest) -> (Vec<Entry>, borg_core::SearchProgress) {
        search.search(Side::Left, 1, request);
        let mut entries = std::collections::BTreeMap::new();
        let mut progress = Default::default();
        loop {
            match tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap() {
                Msg::Batch { entries: batch, .. } => {
                    for entry in batch {
                        entries.insert(entry.identity().to_path_buf(), entry);
                    }
                }
                Msg::SearchProgress { progress: update, .. } => progress = update,
                Msg::Listed { error, .. } => {
                    assert!(error.is_none());
                    break;
                }
                _ => {}
            }
        }
        (entries.into_values().collect(), progress)
    }

    #[tokio::test]
    async fn tree_measures_root_reuses_saved_scan_and_refreshes_nested_changes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(root.join("cache/nested")).unwrap();
        fs::write(root.join("cache/nested/data"), "12345").unwrap();
        fs::write(root.join("top"), "123").unwrap();
        symlink(&root, root.join("cache/loop")).unwrap();
        let cache = crate::size_cache::SizeCache::at(Some(temp.path().join("saved")));
        let mut search = Search::with_cache(cache.clone());
        let (entries, progress) = measured_tree(&mut search, &root, false).await;
        assert_eq!(entries.len(), 2);
        assert_eq!(cache.children(&root, true, &root.join("cache")).unwrap().0[0].name, "nested");
        assert_eq!(entries[0].identity(), root);
        assert_eq!(entries[0].size, 8);
        assert_eq!(entries[0].usage.as_ref().unwrap().files, 2);
        assert_eq!(entries[0].usage.as_ref().unwrap().folders, 2);
        assert!(!progress.cached);
        fs::write(root.join("cache/nested/data"), "1234567890").unwrap();
        let (saved, progress) = measured_tree(&mut search, &root, false).await;
        assert!(progress.cached);
        assert_eq!(saved[0].size, 8);
        assert_eq!(cache.totals(&root)[&root.join("cache")].0, 5);
        let (fresh, progress) = measured_tree(&mut search, &root, true).await;
        assert!(!progress.cached);
        assert_eq!(fresh[0].size, 13);
        assert_eq!(cache.totals(&root)[&root.join("cache")].0, 10);
    }
    #[tokio::test]
    async fn cancelled_tree_refresh_never_replaces_the_previous_saved_scan() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("data"), "old").unwrap();
        let cache = crate::size_cache::SizeCache::at(Some(temp.path().join("saved")));
        let mut search = Search::with_cache(cache.clone());
        measured_tree(&mut search, &root, false).await;
        for index in 0..500 {
            fs::create_dir(root.join(index.to_string())).unwrap();
        }
        fs::write(root.join("data"), "updated").unwrap();
        let request = SearchRequest {
            skip: None,
            root: root.clone(),
            query: "*".into(),
            hidden: true,
            options: borg_core::SearchOptions { kind: SearchKind::FolderTree, refresh: true, ..Default::default() },
        };
        let (messages, mut incoming) = mpsc::channel(1);
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let cancel_worker = cancelled.clone();
        let worker_cache = cache.clone();
        let job = tokio::task::spawn_blocking(move || {
            let mut stream = Output {
                side: Side::Left,
                generation: 2,
                messages: &messages,
                cancelled: &cancel_worker,
                batch: Vec::new(),
                background: false,
            };
            scan_sizes(&request, &mut stream, &worker_cache);
        });
        assert!(matches!(incoming.recv().await, Some(Msg::Batch { .. })));
        cancelled.store(true, Ordering::Relaxed);
        tokio::time::timeout(Duration::from_secs(5), job).await.unwrap().unwrap();
        assert_eq!(cache.load(&root, true).unwrap().0[0].size, 3);
    }
    #[tokio::test]
    async fn space_analysis_reuses_complete_snapshots_and_applies_filters_to_saved_totals() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(root.join("cache/nested")).unwrap();
        fs::write(root.join("cache/nested/data"), "12345").unwrap();
        let cache = crate::size_cache::SizeCache::at(Some(temp.path().join("saved")));
        let mut search = Search::with_cache(cache);
        measured_tree(&mut search, &root, false).await;
        fs::write(root.join("cache/nested/data"), "changed outside Borg").unwrap();
        let request = SearchRequest {
            skip: None,
            root: root.clone(),
            query: "cache".into(),
            hidden: true,
            options: borg_core::SearchOptions {
                kind: SearchKind::FolderTotals,
                minimum: 5,
                minimum_files: 1,
                ..Default::default()
            },
        };
        let (saved, progress) = measured_request(&mut search, request.clone()).await;
        assert!(progress.cached);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].identity(), root.join("cache"));
        assert_eq!(saved[0].size, 5);
        let mut filtered = request.clone();
        filtered.options.minimum = 6;
        assert!(measured_request(&mut search, filtered).await.0.is_empty());
        let mut refresh = request;
        refresh.options.refresh = true;
        let (fresh, progress) = measured_request(&mut search, refresh).await;
        assert!(!progress.cached);
        assert_eq!(fresh[0].size, 20);
    }

    #[tokio::test]
    async fn space_analysis_rescans_a_truncated_tree_snapshot_instead_of_hiding_large_folders() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(root.join("large")).unwrap();
        fs::write(root.join("large/data"), "1234567890").unwrap();
        let cache = crate::size_cache::SizeCache::at(Some(temp.path().join("saved")));
        let meta = fs::symlink_metadata(&root).unwrap();
        let mut entry = borg_vfs::entry(&root).unwrap();
        entry.usage = Some(Default::default());
        cache.save(
            &root,
            true,
            (meta.dev(), meta.ino()),
            &[entry],
            &borg_core::SearchProgress { limited: true, ..Default::default() },
            cache.epoch(),
        );
        let db = fs::read_dir(temp.path().join("saved"))
            .unwrap()
            .find_map(|e| {
                let p = e.unwrap().path();
                (p.extension().is_some_and(|x| x == "sqlite")).then_some(p)
            })
            .unwrap();
        let conn = rusqlite::Connection::open(db).unwrap();
        let bytes: Vec<u8> = conn.query_row("SELECT data FROM metadata", [], |r| r.get(0)).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["version"] = 1.into();
        conn.execute("UPDATE metadata SET data=?1", rusqlite::params![serde_json::to_vec(&value).unwrap()]).unwrap();
        drop(conn);
        let mut search = Search::with_cache(cache);
        let (fresh, progress) = measured_request(
            &mut search,
            SearchRequest {
                skip: None,
                root: root.clone(),
                query: "*".into(),
                hidden: true,
                options: borg_core::SearchOptions { kind: SearchKind::FolderTotals, ..Default::default() },
            },
        )
        .await;
        assert!(!progress.cached);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].identity(), root.join("large"));
        assert_eq!(fresh[0].size, 10);
    }
    #[tokio::test]
    async fn background_measurements_stream_all_folders_and_share_the_complete_cache() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir_all(root.join("alpha/deep")).unwrap();
        fs::create_dir_all(root.join("beta")).unwrap();
        fs::write(root.join("alpha/deep/data"), [0; 42]).unwrap();
        fs::write(root.join("beta/data"), [0; 17]).unwrap();
        let cache = crate::size_cache::SizeCache::at(Some(temp.path().join("cache")));
        let mut search = Search::with_cache(cache.clone());
        search.measure(21, root.clone());
        let mut entries = Vec::new();
        loop {
            match tokio::time::timeout(Duration::from_secs(10), search.incoming.recv()).await.unwrap().unwrap() {
                Msg::Sized { id: 21, entries: batch } => entries.extend(batch),
                Msg::SizeProgress { id: 21, progress } => assert!(progress.errors == 0 && progress.skipped == 0),
                Msg::SizesFinished { id: 21, error } => {
                    assert!(error.is_none(), "{error:?}");
                    break;
                }
                message => panic!("Background measurement leaked a panel search message: {message:?}"),
            }
        }
        assert_eq!(entries.len(), 4);
        assert_eq!(entries.iter().find(|e| e.identity() == root).unwrap().size, 59);
        assert_eq!(entries.iter().find(|e| e.identity() == root.join("alpha/deep")).unwrap().size, 42);
        let totals = cache.totals(&root);
        assert_eq!(totals[&root.join("alpha")].0, 42);
        assert_eq!(totals[&root.join("beta")].0, 17);
        let (tree, progress) = measured_tree(&mut search, &root, false).await;
        assert_eq!(tree.len(), 3);
        assert!(progress.cached);
    }

    #[test]
    fn folders_already_being_walked_are_not_entered_again() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("a/b")).unwrap();
        let id = |path: &std::path::Path| {
            let meta = fs::symlink_metadata(path).unwrap();
            (meta.dev(), meta.ino())
        };
        let walked = [id(temp.path()), id(&temp.path().join("a"))];
        assert!(revisits(walked, &fs::symlink_metadata(temp.path().join("a")).unwrap()));
        assert!(revisits(walked, &fs::symlink_metadata(temp.path()).unwrap()));
        assert!(!revisits(walked, &fs::symlink_metadata(temp.path().join("a/b")).unwrap()));
    }

    #[test]
    fn running_totals_include_open_subfolders_and_are_marked_partial() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("a/b")).unwrap();
        let frame = |path: std::path::PathBuf, bytes: u64, files: u64, folders: u64| Directory {
            reader: fs::read_dir(&path).unwrap(),
            entry: borg_vfs::entry(&path).unwrap(),
            usage: borg_vfs::FolderUsage { files, folders, ..Default::default() },
            bytes,
            identity: (0, 0),
        };
        let stack = [
            frame(temp.path().to_path_buf(), 10, 1, 0),
            frame(temp.path().join("a"), 5, 2, 0),
            frame(temp.path().join("a/b"), 3, 1, 0),
        ];
        let totals: Vec<_> = provisional(&stack)
            .into_iter()
            .map(|entry| {
                let usage = entry.usage.unwrap();
                (entry.name, entry.size, usage.files, usage.folders, usage.partial)
            })
            .collect();
        assert_eq!(totals[0], ("b".into(), 3, 1, 0, true));
        assert_eq!(totals[1], ("a".into(), 8, 3, 1, true));
        assert_eq!((totals[2].1, totals[2].2, totals[2].3, totals[2].4), (18, 4, 2, true));
    }
}
