use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use borg_core::{LineMatch, Msg, RESULT_LIMIT, Scan, SearchKind, SearchProgress, SearchQuery, SearchRequest, Side};
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

mod sizes;

use sizes::scan_sizes;

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
        found: Vec::new(),
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
            let index = search.options.contents.is_none().then(|| stores.indexes.covering(&search)).flatten();
            let answered = index.and_then(|index| search_index(&index, &search, &mut stream));
            if !answered.unwrap_or_else(|| search_names(&search, &mut stream)) {
                return;
            }
        }
    }
    if stream.flush() {
        stream.listed(None);
    }
}

/// Streams the entries below the search folder whose names match, leaving out what the request
/// skips. A left-out folder is still a result when it matches; only its contents are not read.
/// False when the search stopped before finishing.
fn search_names(search: &SearchRequest, stream: &mut Output<'_>) -> bool {
    let failed = |stream: &Output<'_>, error: String| {
        stream.listed(Some(error));
        false
    };
    let query = match SearchQuery::parse(&search.query) {
        Ok(query) => query,
        Err(error) => return failed(stream, error.message()),
    };
    let contents = match search.options.contents.as_deref().map(borg_view::Query::compile).transpose() {
        Ok(contents) => contents.flatten(),
        Err(reason) => return failed(stream, reason),
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
                if cheap.is_none() && !wanted(&entry, &path) {
                    continue;
                }
                let line = match &contents {
                    Some(contents) => match look_inside(&path, contents, stream, &mut progress) {
                        Inside::Found(line) => Some(line),
                        Inside::Passed => continue,
                        Inside::Stopped => return false,
                    },
                    None => None,
                };
                let pushed = match line {
                    Some(line) => stream.push_found(entry, line),
                    None => stream.push(entry),
                };
                if !pushed {
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
                if !stream.flush() || !stream.progress(progress.clone()) {
                    return false;
                }
                last = Instant::now();
            }
        }
        // The whole walk counts every hidden name again, the near ones included.
        progress.skipped = hidden_before.max(hidden.load(Ordering::Relaxed));
    }
    stream.flush() && stream.progress(progress)
}

/// What reading a file for a contents search came to.
enum Inside {
    Found(LineMatch),
    /// No match, or a file left unread or unreadable.
    Passed,
    Stopped,
}

/// Reads the file at `path` for `contents`, counting it as read, left unread or failed.
fn look_inside(
    path: &Path,
    contents: &borg_view::Search,
    stream: &Output<'_>,
    progress: &mut SearchProgress,
) -> Inside {
    let scanned = borg_view::FileSource::open(path)
        .and_then(|source| borg_core::scan_contents(&source, contents, &|_| !stream.stopped()));
    match scanned {
        Ok(Scan::Matched(line)) => {
            progress.read += 1;
            Inside::Found(line)
        }
        Ok(Scan::NoMatch) => {
            progress.read += 1;
            Inside::Passed
        }
        Ok(Scan::Binary | Scan::TooLarge) => {
            progress.unread += 1;
            Inside::Passed
        }
        Err(_) if stream.stopped() => Inside::Stopped,
        Err(_) => {
            progress.errors += 1;
            Inside::Passed
        }
    }
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
    Some(stream.flush() && stream.progress(progress))
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
pub(crate) fn generated(path: &Path) -> bool {
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

struct Output<'a> {
    side: Side,
    generation: u64,
    messages: &'a mpsc::Sender<Msg>,
    cancelled: &'a AtomicBool,
    batch: Vec<Entry>,
    /// Files a contents search found, sent apart from the batch so each keeps its match.
    found: Vec<(Entry, LineMatch)>,
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
    fn progress(&self, progress: SearchProgress) -> bool {
        self.send(Msg::SearchProgress { side: self.side, generation: self.generation, progress })
    }
    fn listed(&self, error: Option<String>) -> bool {
        self.send(Msg::Listed { side: self.side, generation: self.generation, error })
    }
    fn push(&mut self, entry: Entry) -> bool {
        self.batch.push(entry);
        self.batch.len() < BATCH_SIZE || self.flush()
    }
    fn push_found(&mut self, entry: Entry, line: LineMatch) -> bool {
        self.found.push((entry, line));
        self.found.len() < BATCH_SIZE || self.flush()
    }
    fn flush(&mut self) -> bool {
        if !self.found.is_empty() {
            let found = std::mem::take(&mut self.found);
            if !self.send(Msg::Found { side: self.side, generation: self.generation, found }) {
                return false;
            }
        }
        if self.batch.is_empty() {
            return !self.stopped();
        }
        let entries = std::mem::replace(&mut self.batch, Vec::with_capacity(BATCH_SIZE));
        self.send(Msg::Batch { side: self.side, generation: self.generation, entries })
    }
}

#[cfg(test)]
mod tests;
