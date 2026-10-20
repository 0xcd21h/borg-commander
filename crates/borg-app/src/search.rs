use borg_i18n::tr;
use std::{
    fs,
    os::unix::fs::MetadataExt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use borg_core::{Msg, Query, RESULT_LIMIT, SearchKind, SearchProgress, SearchRequest, Side};
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
    fn new(side: Side, messages: mpsc::Sender<Msg>) -> Self {
        let (requests, mut incoming) = watch::channel(None::<Request>);
        let task = tokio::spawn(async move {
            while incoming.changed().await.is_ok() {
                let Some(request) = incoming.borrow_and_update().clone() else { continue };
                let messages = messages.clone();
                let _ = tokio::task::spawn_blocking(move || run(side, request, &messages)).await;
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

pub struct Search {
    workers: [Worker; 2],
    pub incoming: mpsc::Receiver<Msg>,
}

impl Search {
    pub fn new() -> Self {
        let (messages, incoming) = mpsc::channel(CHANNEL_BATCHES);
        Self { workers: Side::ALL.map(|side| Worker::new(side, messages.clone())), incoming }
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

fn run(side: Side, request: Request, messages: &mpsc::Sender<Msg>) {
    let mut stream = Output {
        side,
        generation: request.generation,
        messages,
        cancelled: &request.cancelled,
        batch: Vec::with_capacity(BATCH_SIZE),
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
        Work::Search(search) if search.options.kind == SearchKind::FolderTotals => {
            scan_sizes(&search, &mut stream);
            return;
        }
        Work::Search(search) => {
            let query = Query::parse(&search.query);
            let mut progress = SearchProgress::default();
            let root = match fs::metadata(&search.root) {
                Ok(root) => root,
                Err(error) => {
                    stream.send(Msg::Listed { side, generation: request.generation, error: Some(error.to_string()) });
                    return;
                }
            };
            let device = root.dev();
            let reader = match fs::read_dir(&search.root) {
                Ok(reader) => reader,
                Err(error) => {
                    stream.send(Msg::Listed { side, generation: request.generation, error: Some(error.to_string()) });
                    return;
                }
            };
            let mut stack = vec![reader];
            let mut found = 0;
            let mut last = Instant::now();
            while let Some(reader) = stack.last_mut() {
                if stream.stopped() {
                    return;
                }
                let Some(item) = reader.next() else {
                    stack.pop();
                    continue;
                };
                let item = match item {
                    Ok(item) => item,
                    Err(_) => {
                        progress.errors += 1;
                        continue;
                    }
                };
                if !search.hidden && item.file_name().as_encoded_bytes().starts_with(b".") {
                    progress.skipped += 1;
                    continue;
                }
                let entry = match borg_vfs::entry(&item.path()) {
                    Ok(entry) => entry,
                    Err(_) => {
                        progress.errors += 1;
                        continue;
                    }
                };
                progress.visited += 1;
                let kind = match search.options.kind {
                    SearchKind::Files => entry.kind == borg_vfs::EntryKind::File,
                    SearchKind::Folders => entry.kind == borg_vfs::EntryKind::Dir,
                    _ => true,
                };
                if kind
                    && (search.options.minimum == 0 || (!entry.is_dir() && entry.size >= search.options.minimum))
                    && query.accepts(&entry)
                {
                    if !stream.push(entry.clone()) {
                        return;
                    }
                    found += 1;
                    if found >= RESULT_LIMIT {
                        progress.limited = true;
                        break;
                    }
                }
                if entry.kind == borg_vfs::EntryKind::Dir {
                    match fs::symlink_metadata(item.path()) {
                        Ok(meta) if meta.dev() != device || stack.len() >= MAX_DEPTH => progress.skipped += 1,
                        Ok(_) => match fs::read_dir(item.path()) {
                            Ok(reader) => stack.push(reader),
                            Err(_) => progress.errors += 1,
                        },
                        Err(_) => progress.errors += 1,
                    }
                }
                if last.elapsed() >= PROGRESS_EVERY {
                    if !stream.flush()
                        || !stream.send(Msg::SearchProgress {
                            side,
                            generation: request.generation,
                            progress: progress.clone(),
                        })
                    {
                        return;
                    }
                    last = Instant::now();
                }
            }
            if !stream.flush() {
                return;
            }
            if !stream.send(Msg::SearchProgress { side, generation: request.generation, progress }) {
                return;
            }
        }
    }
    if stream.flush() {
        stream.send(Msg::Listed { side, generation: request.generation, error: None });
    }
}

struct Directory {
    reader: fs::ReadDir,
    entry: Entry,
    usage: borg_vfs::FolderUsage,
    bytes: u64,
}

fn scan_sizes(search: &SearchRequest, stream: &mut Output<'_>) {
    let mut progress = SearchProgress::default();
    let mut largest = std::collections::BTreeMap::new();
    let query = Query::parse(&search.query);
    let open = |path: &std::path::Path| -> std::io::Result<Directory> {
        Ok(Directory {
            reader: fs::read_dir(path)?,
            entry: borg_vfs::entry(path)?,
            usage: Default::default(),
            bytes: 0,
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
    let device = match fs::symlink_metadata(&search.root) {
        Ok(meta) if meta.is_dir() => meta.dev(),
        _ => {
            stream.send(Msg::Listed {
                side: stream.side,
                generation: stream.generation,
                error: Some(tr!(search_directory_required).into()),
            });
            return;
        }
    };
    let mut stack = vec![root];
    let mut last = Instant::now();
    while !stack.is_empty() {
        if stream.stopped() {
            return;
        }
        if last.elapsed() >= PROGRESS_EVERY {
            if !stream.send(Msg::SearchProgress {
                side: stream.side,
                generation: stream.generation,
                progress: progress.clone(),
            }) {
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
                finished.entry.size = finished.bytes;
                finished.entry.usage = Some(finished.usage.clone());
                if finished.bytes >= search.options.minimum
                    && finished.usage.files >= search.options.minimum_files
                    && query.accepts(&finished.entry)
                {
                    largest.insert((finished.bytes, finished.entry.identity().to_path_buf()), finished.entry);
                    if largest.len() > RESULT_LIMIT {
                        largest.pop_first();
                        progress.limited = true;
                    }
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
            if meta.dev() != device || stack.len() >= MAX_DEPTH {
                progress.skipped += 1;
                stack.last_mut().unwrap().usage.partial = true;
                continue;
            }
            match open(&item.path()) {
                Ok(directory) => stack.push(directory),
                Err(_) => {
                    progress.errors += 1;
                    stack.last_mut().unwrap().usage.partial = true;
                    if let Ok(mut entry) = borg_vfs::entry(&item.path()) {
                        entry.size = 0;
                        entry.usage = Some(borg_vfs::FolderUsage { partial: true, ..Default::default() });
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
            current.usage.files = current.usage.files.saturating_add(1);
        }
    }
    for (_, entry) in largest.into_iter().rev() {
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
}

impl Output<'_> {
    fn stopped(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed) || self.messages.is_closed()
    }
    fn send(&self, mut msg: Msg) -> bool {
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
        let mut worker = Search::new();
        worker.search(
            Side::Left,
            9,
            SearchRequest {
                root: root.path().into(),
                query: "node_modules".into(),
                hidden: true,
                options: borg_core::SearchOptions { kind: SearchKind::FolderTotals, minimum: 2000, minimum_files: 200 },
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
        let mut search = Search::new();
        search.search(
            Side::Left,
            7,
            SearchRequest {
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
        let mut search = Search::new();
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
        let mut search = Search::new();
        search.search(
            Side::Left,
            1,
            SearchRequest { root: root.path().into(), query: "*".into(), hidden: false, options: Default::default() },
        );
        let _ = tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap();
        search.search(
            Side::Left,
            2,
            SearchRequest {
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
}
