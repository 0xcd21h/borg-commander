//! Borg's own files on this computer: where its places are, what they hold, which files no
//! running Borg uses, removing those, and the list of captures Borg saved. Core's rules decide
//! what is left over; this module lists, measures and removes.

use std::{
    collections::HashSet,
    fs, io,
    os::unix::{fs::FileTypeExt, net::UnixStream},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, bail};
use borg_core::{
    CAPTURE_RECORDS, CacheLists, Cleanup, Found, FoundKind, Judge, Leftover, Msg, Place, PlaceUsage, SavedCapture,
    StorageReport, abandoned_leftover, cache_leftovers, leftover_lines, runtime_leftovers, socket_leftovers,
};
use borg_i18n::{Count, count, tr};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;
use unicode_width::UnicodeWidthStr;

/// How deep the rules look into a place; below that only bytes and times count.
const RULE_DEPTH: usize = 2;
/// How long after it starts Borg first cleans up, so the cleanup never slows the start.
const CLEANUP_DELAY: Duration = Duration::from_secs(30);
/// How often a running Borg cleans up after that.
const CLEANUP_EVERY: Duration = crate::launch::TTL_REFRESH;
/// The list of captures Borg saved, in the state folder.
const CAPTURES_FILE: &str = "captures.json";
const CAPTURES_BYTES: u64 = 1024 * 1024;
const CAPTURE_EXTENSIONS: [&str; 2] = ["pcap", "pcapng"];
const KEY_LOG_EXTENSION: &str = "keylog";
const CLEAN: &str = "clean";
const USAGE: &str = "usage: borg storage [clean]";
const SIZE_COLUMN: usize = 8;
const HOME_MARK: &str = "~";

/// The folders that list the index files the settings ask for, read when the rules run.
pub type Roots = Arc<dyn Fn() -> Vec<PathBuf> + Send + Sync>;

/// Where Borg keeps its files on this computer.
#[derive(Debug, Clone)]
pub struct Places {
    pub runtime: PathBuf,
    pub state: Option<PathBuf>,
    pub cache: Option<PathBuf>,
    pub config: Option<PathBuf>,
}

impl Places {
    fn service_runtime(&self) -> Option<PathBuf> {
        self.state.as_ref().map(|state| state.join(crate::service::SERVICE_RUNTIME)).filter(|dir| *dir != self.runtime)
    }

    fn sockets(&self) -> PathBuf {
        crate::agent::socket_path(&self.runtime)
    }

    fn temporary(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.runtime.clone()];
        dirs.extend(self.service_runtime());
        dirs.extend(crate::launch::abandoned_runtime_dirs(&self.runtime));
        dirs
    }
}

/// The Borg's files window's side in the app, and the cleanups that run while Borg does.
pub struct Storage {
    places: Places,
    roots: Roots,
    messages: UnboundedSender<Msg>,
    last: Arc<Mutex<Option<Cleanup>>>,
}

impl Storage {
    /// Cleans up a while after Borg starts and every hour after that. Only the first cleanup
    /// trusts `idle`: once this Borg runs, the runtime folder is in use.
    pub fn start(places: Places, idle: bool, roots: Roots, messages: UnboundedSender<Msg>) -> Self {
        let last = Arc::new(Mutex::new(None));
        let (task_places, task_roots, task_last) = (places.clone(), roots.clone(), last.clone());
        tokio::spawn(async move {
            tokio::time::sleep(CLEANUP_DELAY).await;
            let mut idle = idle;
            let mut every = tokio::time::interval(CLEANUP_EVERY);
            loop {
                every.tick().await;
                let (places, roots, last) = (task_places.clone(), task_roots.clone(), task_last.clone());
                let _ = tokio::task::spawn_blocking(move || {
                    let found = leftovers(&places, idle, &roots(), SystemTime::now());
                    record(&last, clean(&found, SystemTime::now()));
                })
                .await;
                idle = false;
            }
        });
        Self { places, roots, messages, last }
    }

    pub fn scan(&self) {
        self.after(|_, _| {});
    }

    /// Removes those of `paths` that are still left over, then measures again.
    pub fn clean(&self, paths: Vec<PathBuf>) {
        self.after(move |found, last| {
            let asked: HashSet<PathBuf> = paths.into_iter().collect();
            let chosen: Vec<Leftover> = found.into_iter().filter(|leftover| asked.contains(&leftover.path)).collect();
            record(last, clean(&chosen, SystemTime::now()));
        });
    }

    fn after(&self, work: impl FnOnce(Vec<Leftover>, &Mutex<Option<Cleanup>>) + Send + 'static) {
        let (places, roots, last, messages) =
            (self.places.clone(), self.roots.clone(), self.last.clone(), self.messages.clone());
        tokio::task::spawn_blocking(move || {
            work(leftovers(&places, false, &roots(), SystemTime::now()), &last);
            let cleaned = *last.lock().expect("last cleanup");
            let _ = messages.send(Msg::Storage(report(&places, &roots(), cleaned)));
        });
    }
}

fn record(last: &Mutex<Option<Cleanup>>, cleanup: Cleanup) {
    if cleanup.removed > 0 || cleanup.failed > 0 {
        *last.lock().expect("last cleanup") = Some(cleanup);
    }
}

/// What every place holds, the captures Borg saved and what is left over now.
pub fn report(places: &Places, roots: &[PathBuf], cleaned: Option<Cleanup>) -> StorageReport {
    let sockets = crate::agent::socket_dirs();
    let captures = places.state.as_deref().map(saved_captures).unwrap_or_default();
    let usage =
        |place, path: Option<PathBuf>, tally: Tally| PlaceUsage { place, path, bytes: tally.bytes, items: tally.items };
    let state = places.state.as_ref().map(|state| tally_except(state, places.service_runtime().as_deref()));
    let places_found = Place::ALL
        .into_iter()
        .map(|place| match place {
            Place::Temporary => usage(place, Some(places.runtime.clone()), sum(&places.temporary())),
            Place::Sockets => usage(place, Some(places.sockets()), sum(&sockets)),
            Place::Caches => usage(place, places.cache.clone(), sum(places.cache.as_slice())),
            Place::State => usage(place, places.state.clone(), state.unwrap_or_default()),
            Place::Settings => usage(place, places.config.clone(), sum(places.config.as_slice())),
            Place::Captures => PlaceUsage {
                place,
                path: None,
                bytes: captures.iter().map(|capture| capture.bytes).sum(),
                items: captures.len(),
            },
        })
        .collect();
    StorageReport {
        places: places_found,
        captures,
        leftovers: leftovers(places, false, roots, SystemTime::now()),
        cleaned,
    }
}

/// What no running Borg uses in any of Borg's places, by core's rules.
pub fn leftovers(places: &Places, idle: bool, roots: &[PathBuf], now: SystemTime) -> Vec<Leftover> {
    let judge = Judge { now, version: crate::update::VERSION, idle, runs: &runs };
    let busy = Judge { idle: false, ..judge };
    let mut found = runtime_leftovers(&places.runtime, &walk(&places.runtime, RULE_DEPTH, now), &judge);
    if let Some(service) = places.service_runtime() {
        found.extend(runtime_leftovers(&service, &walk(&service, RULE_DEPTH, now), &busy));
    }
    for dir in crate::launch::abandoned_runtime_dirs(&places.runtime) {
        found.extend(found_at(&dir, 0, now).and_then(|folder| abandoned_leftover(&dir, &folder, &judge)));
    }
    let current = places.sockets();
    for dir in crate::agent::socket_dirs() {
        if let Some(folder) = found_at(&dir, 1, now) {
            found.extend(socket_leftovers(&dir, &folder, dir == current, &judge));
        }
    }
    if let Some(cache) = &places.cache {
        let sizes = crate::size_cache::listed(cache);
        let indexes: HashSet<String> = roots.iter().map(|root| crate::index::file_name(root)).collect();
        let lists = CacheLists { sizes: sizes.as_ref(), indexes: &indexes };
        found.extend(cache_leftovers(cache, &walk(cache, RULE_DEPTH, now), &lists, &judge));
    }
    found
}

/// Removes the leftovers; one already gone counts for nothing.
pub fn clean(leftovers: &[Leftover], now: SystemTime) -> Cleanup {
    let mut cleanup = Cleanup { at: now, removed: 0, bytes: 0, failed: 0 };
    for leftover in leftovers {
        match remove(&leftover.path) {
            Ok(()) => {
                cleanup.removed += 1;
                cleanup.bytes += leftover.bytes;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => cleanup.failed += 1,
        }
    }
    cleanup
}

fn remove(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) }
}

fn runs(pid: u32) -> bool {
    let Some(pid) = i32::try_from(pid).ok().and_then(rustix::process::Pid::from_raw) else { return false };
    !matches!(rustix::process::test_kill_process(pid), Err(rustix::io::Errno::SRCH))
}

/// Whether a process answers on the socket: a session's socket names its process, and any
/// other socket is asked.
fn answers(path: &Path) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let process = name
        .strip_prefix(crate::agent::SOCKET_PREFIX)
        .and_then(|rest| rest.strip_suffix(borg_core::SOCKET_SUFFIX))
        .and_then(|rest| rest.rsplit('-').next())
        .and_then(|pid| pid.parse().ok());
    match process {
        Some(pid) => runs(pid),
        None => UnixStream::connect(path).is_ok(),
    }
}

/// Bytes, files and folders, and the newest change below a path.
#[derive(Debug, Clone, Copy, Default)]
struct Tally {
    bytes: u64,
    items: usize,
    newest: Option<SystemTime>,
}

impl Tally {
    fn add(self, other: Self) -> Self {
        Self { bytes: self.bytes + other.bytes, items: self.items + other.items, newest: self.newest.max(other.newest) }
    }
}

fn tally(path: &Path, meta: &fs::Metadata) -> Tally {
    let own = Tally { bytes: if meta.is_dir() { 0 } else { meta.len() }, items: 0, newest: meta.modified().ok() };
    if !meta.is_dir() {
        return own;
    }
    fs::read_dir(path).into_iter().flatten().flatten().fold(own, |total, entry| match entry.metadata() {
        Ok(meta) => total.add(Tally { items: 1, ..Tally::default() }).add(tally(&entry.path(), &meta)),
        Err(_) => total,
    })
}

fn tally_except(path: &Path, skipped: Option<&Path>) -> Tally {
    fs::read_dir(path).into_iter().flatten().flatten().filter(|entry| Some(entry.path().as_path()) != skipped).fold(
        Tally::default(),
        |total, entry| match entry.metadata() {
            Ok(meta) => total.add(Tally { items: 1, ..Tally::default() }).add(tally(&entry.path(), &meta)),
            Err(_) => total,
        },
    )
}

fn sum(paths: &[PathBuf]) -> Tally {
    paths
        .iter()
        .filter_map(|path| fs::symlink_metadata(path).ok().map(|meta| tally(path, &meta)))
        .fold(Tally::default(), Tally::add)
}

/// The entries of `dir` for the rules, `depth` levels down. A name that is not text is never
/// one of Borg's.
fn walk(dir: &Path, depth: usize, now: SystemTime) -> Vec<Found> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let meta = entry.metadata().ok()?;
            Some(found(&entry.path(), name, &meta, depth.saturating_sub(1), now))
        })
        .collect()
}

fn found_at(path: &Path, depth: usize, now: SystemTime) -> Option<Found> {
    let name = path.file_name()?.to_str()?.to_owned();
    let meta = fs::symlink_metadata(path).ok()?;
    Some(found(path, name, &meta, depth, now))
}

/// A file Borg cannot date counts as just changed, so it is never left over.
fn found(path: &Path, name: String, meta: &fs::Metadata, depth: usize, now: SystemTime) -> Found {
    let total = tally(path, meta);
    let kind = if meta.is_dir() {
        FoundKind::Folder(if depth > 0 { walk(path, depth, now) } else { Vec::new() })
    } else if meta.file_type().is_socket() {
        FoundKind::Socket { answers: answers(path) }
    } else {
        FoundKind::File
    };
    Found { name, kind, modified: total.newest.unwrap_or(now), bytes: total.bytes }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CaptureRecord {
    path: PathBuf,
    /// Seconds since 1970.
    saved: u64,
}

fn read_records(state: &Path) -> Vec<CaptureRecord> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    let read =
        fs::File::open(state.join(CAPTURES_FILE)).and_then(|file| file.take(CAPTURES_BYTES).read_to_end(&mut bytes));
    read.ok().and_then(|_| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// `path` first in the list of captures, once, with the list kept to [`CAPTURE_RECORDS`].
fn remembered(mut records: Vec<CaptureRecord>, path: &Path, saved: SystemTime) -> Vec<CaptureRecord> {
    records.retain(|record| record.path != path);
    let saved = saved.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    records.insert(0, CaptureRecord { path: path.to_path_buf(), saved });
    records.truncate(CAPTURE_RECORDS);
    records
}

/// Adds a capture Borg just saved to the list the Borg's files window shows.
pub fn remember_capture(path: &Path) {
    let Some(state) = crate::session::StateDir::locate().map(|dir| dir.path().to_path_buf()) else { return };
    let records = remembered(read_records(&state), path, SystemTime::now());
    if let Ok(bytes) = serde_json::to_vec(&records) {
        let _ = fs::create_dir_all(&state);
        let _ = crate::store::write_privately(&state.join(CAPTURES_FILE), &bytes);
    }
}

/// The captures on the list that are still there, under the name they have now: a capture
/// that took in its TLS keys became a pcapng file.
fn saved_captures(state: &Path) -> Vec<SavedCapture> {
    read_records(state).into_iter().filter_map(|record| resolve(&record)).collect()
}

fn resolve(record: &CaptureRecord) -> Option<SavedCapture> {
    let path = std::iter::once(record.path.clone())
        .chain(CAPTURE_EXTENSIONS.iter().map(|extension| record.path.with_extension(extension)))
        .find(|path| path.is_file())?;
    let keys = Some(path.with_extension(KEY_LOG_EXTENSION)).filter(|keys| keys.is_file());
    let bytes = [Some(&path), keys.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(|file| fs::metadata(file).ok())
        .map(|meta| meta.len())
        .sum();
    Some(SavedCapture { path, bytes, saved: UNIX_EPOCH + Duration::from_secs(record.saved), keys })
}

/// `borg storage`: prints where Borg keeps what; `borg storage clean` first removes what no
/// running Borg uses.
pub fn command(args: &[String]) -> Result<ExitCode> {
    let cleaning = match args.first().map(String::as_str) {
        None => false,
        Some(CLEAN) if args.len() == 1 => true,
        Some(_) => bail!("{USAGE}"),
    };
    let runtime = crate::launch::private_dir(&crate::launch::runtime_dir())?;
    let idle = crate::launch::idle(&runtime);
    let config = crate::config::ConfigDir::locate();
    let roots = config.as_ref().and_then(|dir| dir.load().ok()).map(|config| config.index_roots()).unwrap_or_default();
    let places = Places {
        runtime,
        state: crate::session::StateDir::locate().map(|dir| dir.path().to_path_buf()),
        cache: crate::size_cache::cache_dir(),
        config: config.map(|dir| dir.path().to_path_buf()),
    };
    let cleaned = cleaning.then(|| clean(&leftovers(&places, idle, &roots, SystemTime::now()), SystemTime::now()));
    let report = report(&places, &roots, None);
    print!("{}", table(&report));
    if let Some(cleanup) = cleaned {
        println!(
            "{}",
            tr!(
                cli_storage_cleaned,
                items = count(cleanup.removed, Count::Item),
                size = borg_ui::size_text(cleanup.bytes)
            )
        );
        if cleanup.failed > 0 {
            println!("{}", tr!(cli_storage_failed, items = count(cleanup.failed, Count::Item)));
        }
    } else if !report.leftovers.is_empty() {
        let size = borg_ui::size_text(report.leftover_bytes());
        println!("{}", tr!(cli_storage_leftovers, items = count(report.leftovers.len(), Count::Item), size = size));
        for line in leftover_lines(&report.leftovers, shown_path) {
            println!("  {line}");
        }
    } else {
        println!("{}", tr!(storage_no_leftovers));
    }
    Ok(if cleaned.is_some_and(|cleanup| cleanup.failed > 0) { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

/// `path` with the home folder as `~`.
fn shown_path(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.as_deref().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => Path::new(HOME_MARK).join(rest).display().to_string(),
        None => path.display().to_string(),
    }
}

/// Each place on its own lines: its name, size and folder, then what it holds.
fn table(report: &StorageReport) -> String {
    let shown = shown_path;
    let widest = report.places.iter().map(|usage| usage.place.label().width()).max().unwrap_or_default();
    let mut out = String::new();
    for usage in &report.places {
        let label = usage.place.label();
        let pad = " ".repeat(widest - label.width());
        let size = borg_ui::size_text(usage.bytes);
        let place = match &usage.path {
            Some(path) => shown(path),
            None => count(usage.items, Count::File),
        };
        out.push_str(&format!("{label}{pad}  {size:>SIZE_COLUMN$}  {place}\n  {}\n", usage.place.about()));
    }
    for capture in &report.captures {
        out.push_str(&format!("  {:>SIZE_COLUMN$}  {}\n", borg_ui::size_text(capture.bytes), shown(&capture.path)));
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests;
