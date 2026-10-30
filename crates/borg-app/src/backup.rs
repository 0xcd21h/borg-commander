//! Time Machine exclusions on macOS: the mark `tmutil addexclusion` leaves on an item, the
//! excluded paths in Time Machine's preferences, and `tmutil isexcluded` for the rest, such as
//! the places macOS leaves out by itself. Work runs on blocking workers; core asks for none of it
//! off macOS.

use borg_i18n::tr;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    ffi::OsStr,
    fs,
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
    process::{Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use borg_core::{BackupChange, BackupEvent, BackupItem, BackupListing, BackupRequest, Exclusion, Msg};
use tokio::sync::mpsc::UnboundedSender;

const TMUTIL: &str = "/usr/bin/tmutil";
const OSASCRIPT: &str = "/usr/bin/osascript";
const MDFIND: &str = "/usr/bin/mdfind";
const DEFAULTS: &str = "/usr/bin/defaults";
/// The extended attribute `tmutil addexclusion` sets: a sticky exclusion.
const MARK: &str = "com.apple.metadata:com_apple_backup_excludeItem";
const PREFERENCES: &str = "/Library/Preferences/com.apple.TimeMachine";
const SKIP_PATHS: &str = "SkipPaths";
const SPOTLIGHT_QUERY: &str = "com_apple_backup_excludeItem = 'com.apple.backupd'";
const VOLUMES: &str = "/Volumes";
const ROOT: &str = "/";
/// Places macOS is known to leave out by itself. Each is asked about, and only those Time
/// Machine confirms are listed, since the system's own list cannot be read.
const SYSTEM_PLACES: &[&str] = &[
    "/System",
    "/usr",
    "/bin",
    "/sbin",
    "/cores",
    "/private/var/vm",
    "/private/var/folders",
    "/private/var/log",
    "/Library/Caches",
    "/Library/Logs",
    "~/Library/Caches",
    "~/Library/Logs",
    "~/.Trash",
];
/// Version control data is the project's history, never a cache, and not worth walking.
const VERSION_CONTROL: &[&str] = &[".git", ".hg", ".svn"];
const ASK_TIMEOUT: Duration = Duration::from_secs(10);
const SPOTLIGHT_TIMEOUT: Duration = Duration::from_secs(30);
/// Long enough for the user to read the password dialog and answer it.
const ADMIN_TIMEOUT: Duration = Duration::from_secs(300);
const PREFERENCES_FRESH: Duration = Duration::from_secs(5);
const PROGRESS_EVERY: Duration = Duration::from_millis(100);
const FOUND_LIMIT: usize = 5_000;
const FOUND_BATCH: usize = 64;
const MARK_BUFFER: usize = 256;
const FULL_DISK_ACCESS: &str = "Full Disk Access";
/// What AppleScript reports when the password dialog is cancelled.
const USER_CANCELED: &str = "(-128)";

pub fn available() -> bool {
    cfg!(target_os = "macos") && Path::new(TMUTIL).exists()
}

/// How Time Machine treats `path`, for Info.
pub fn status(path: &Path) -> Exclusion {
    inspect(path, &skipped()).exclusion
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Search {
    Inspect,
    Caches,
    All,
}

pub struct Backup {
    messages: UnboundedSender<Msg>,
    listings: [Arc<AtomicBool>; 2],
    searches: HashMap<Search, Arc<AtomicBool>>,
}

impl Backup {
    pub fn new(messages: UnboundedSender<Msg>) -> Self {
        Self { messages, listings: Default::default(), searches: HashMap::new() }
    }

    pub fn request(&mut self, request: BackupRequest) {
        let messages = self.messages.clone();
        let send = move |event| {
            let _ = messages.send(Msg::Backup(event));
        };
        match request {
            BackupRequest::Listing { side, generation, dir } => {
                let cancelled = renew(&mut self.listings[side.index()]);
                tokio::task::spawn_blocking(move || {
                    let result = listing(&dir, &cancelled).map_err(|error| format!("{error:#}"));
                    if !cancelled.load(Ordering::Relaxed) {
                        send(BackupEvent::Listing { side, generation, dir, result });
                    }
                });
            }
            BackupRequest::Inspect { id, paths } => {
                let cancelled = self.search(Search::Inspect);
                tokio::task::spawn_blocking(move || {
                    let skipped = skipped();
                    let items = paths.iter().map(|path| inspect(path, &skipped)).collect();
                    if !cancelled.load(Ordering::Relaxed) {
                        send(BackupEvent::Found { id, items });
                        send(BackupEvent::Finished { id, error: None });
                    }
                });
            }
            BackupRequest::Caches { id, root } => {
                let cancelled = self.search(Search::Caches);
                tokio::task::spawn_blocking(move || {
                    let error = caches(id, &root, &cancelled, &send);
                    send(BackupEvent::Finished { id, error });
                });
            }
            BackupRequest::All { id, root } => {
                let cancelled = self.search(Search::All);
                tokio::task::spawn_blocking(move || {
                    let error = all(id, &root, &cancelled, &send);
                    send(BackupEvent::Finished { id, error });
                });
            }
            BackupRequest::Change { id, change, items } => {
                tokio::task::spawn_blocking(move || {
                    let (items, error) = apply(change, &items);
                    send(BackupEvent::Changed { id, items, error });
                });
            }
            BackupRequest::Cancel => self.searches.drain().for_each(|(_, flag)| flag.store(true, Ordering::Relaxed)),
        }
    }

    /// A flag for a new search, which stops the one it replaces.
    fn search(&mut self, kind: Search) -> Arc<AtomicBool> {
        renew(self.searches.entry(kind).or_default())
    }
}

fn renew(slot: &mut Arc<AtomicBool>) -> Arc<AtomicBool> {
    let fresh = Arc::new(AtomicBool::new(false));
    std::mem::replace(slot, fresh.clone()).store(true, Ordering::Relaxed);
    fresh
}

/// Runs a program on a blocking worker, with a deadline.
fn run<I>(program: &str, args: I, timeout: Duration) -> Result<Output>
where
    I: IntoIterator,
    I::Item: AsRef<OsStr>,
{
    let mut command = tokio::process::Command::new(program);
    command.args(args).env("LC_ALL", "C").stdin(Stdio::null()).kill_on_drop(true);
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return command.as_std_mut().output().map_err(Into::into);
    };
    runtime.block_on(async {
        tokio::time::timeout(timeout, command.output())
            .await
            .map_err(|_| anyhow!(tr!(backup_timed_out)))?
            .map_err(Into::into)
    })
}

/// Whether the item itself carries Time Machine's exclusion mark.
fn marked(path: &Path) -> bool {
    let mut value = [0; MARK_BUFFER];
    matches!(rustix::fs::lgetxattr(path, MARK, &mut value[..]), Ok(_) | Err(rustix::io::Errno::RANGE))
}

static SKIPPED: Mutex<Option<(Instant, Arc<Vec<PathBuf>>)>> = Mutex::new(None);

/// The fixed-path exclusions, read again once they are a few seconds old.
fn skipped() -> Arc<Vec<PathBuf>> {
    let mut cached = SKIPPED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((read, paths)) = cached.as_ref()
        && read.elapsed() < PREFERENCES_FRESH
    {
        return paths.clone();
    }
    let paths = Arc::new(read_skipped());
    *cached = Some((Instant::now(), paths.clone()));
    paths
}

fn forget_skipped() {
    *SKIPPED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// `defaults` reads the preferences through the system's preferences daemon, which needs no
/// Full Disk Access, unlike reading the file itself.
fn read_skipped() -> Vec<PathBuf> {
    run(DEFAULTS, ["export", PREFERENCES, "-"], ASK_TIMEOUT)
        .map(|output| plist_strings(&String::from_utf8_lossy(&output.stdout), SKIP_PATHS))
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

/// The strings of the array under `key` in an XML property list.
fn plist_strings(xml: &str, key: &str) -> Vec<String> {
    let Some((_, rest)) = xml.split_once(&format!("<key>{key}</key>")) else { return Vec::new() };
    let Some(body) = rest.trim_start().strip_prefix("<array>") else { return Vec::new() };
    let body = body.split("</array>").next().unwrap_or_default();
    body.split("<string>")
        .skip(1)
        .filter_map(|part| part.split_once("</string>"))
        .map(|(text, _)| unescape(text))
        .collect()
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Why backups leave `path` out, from what needs no other program: its mark, the excluded paths,
/// and the same for each folder above it.
fn own_or_inherited(path: &Path, skipped: &[PathBuf]) -> Option<Exclusion> {
    if marked(path) {
        return Some(Exclusion::Sticky);
    }
    if skipped.iter().any(|skip| skip == path) {
        return Some(Exclusion::Path);
    }
    path.ancestors()
        .skip(1)
        .find(|folder| marked(folder) || skipped.iter().any(|skip| skip == folder))
        .map(|folder| Exclusion::Inside(folder.to_path_buf()))
}

/// What `tmutil isexcluded` says, or nothing where it would not, as for folders that need Full
/// Disk Access.
fn asked(path: &Path) -> Option<bool> {
    let output = run(TMUTIL, [OsStr::new("isexcluded"), path.as_os_str()], ASK_TIMEOUT).ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    match text.split_whitespace().next()? {
        "[Excluded]" => Some(true),
        "[Included]" => Some(false),
        _ => None,
    }
}

/// A volume mounted under `/Volumes`, which only a volume exclusion covers.
fn is_volume(path: &Path) -> bool {
    path.parent() == Some(Path::new(VOLUMES))
        && match (fs::metadata(path), fs::metadata(VOLUMES)) {
            (Ok(volume), Ok(volumes)) => volume.dev() != volumes.dev(),
            _ => false,
        }
}

fn volume_of(path: &Path) -> Option<PathBuf> {
    let name = path.strip_prefix(VOLUMES).ok()?.components().next()?;
    Some(Path::new(VOLUMES).join(name)).filter(|root| is_volume(root))
}

fn classify(path: &Path, skipped: &[PathBuf]) -> Exclusion {
    if let Some(exclusion) = own_or_inherited(path, skipped) {
        return exclusion;
    }
    match asked(path) {
        None => Exclusion::Unknown,
        Some(false) => Exclusion::Included,
        // The sealed system volume; the folders people keep are on the data volume.
        Some(true) if path == Path::new(ROOT) => Exclusion::System,
        Some(true) => match volume_of(path) {
            Some(volume) if volume == path => Exclusion::Volume,
            Some(volume) if asked(&volume) == Some(true) => Exclusion::Inside(volume),
            _ => Exclusion::System,
        },
    }
}

fn inspect(path: &Path, skipped: &[PathBuf]) -> BackupItem {
    let metadata = fs::symlink_metadata(path).ok();
    let missing = metadata.is_none();
    let exclusion = match metadata {
        Some(_) => classify(path, skipped),
        None if skipped.iter().any(|skip| skip == path) => Exclusion::Path,
        None => Exclusion::Included,
    };
    BackupItem {
        path: path.to_path_buf(),
        dir: metadata.as_ref().is_some_and(fs::Metadata::is_dir),
        volume: is_volume(path),
        missing,
        size: metadata.as_ref().filter(|metadata| metadata.is_file()).map(fs::Metadata::len),
        exclusion,
        group: Exclusion::Included,
    }
}

/// The entries of a panel's folder excluded on their own, and why the folder is left out, if it is.
fn listing(dir: &Path, cancelled: &AtomicBool) -> Result<BackupListing> {
    let skipped = skipped();
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)?.flatten() {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(BackupListing::default());
        }
        let path = entry.path();
        let exclusion = if marked(&path) {
            Exclusion::Sticky
        } else if skipped.contains(&path) {
            Exclusion::Path
        } else {
            continue;
        };
        entries.push((entry.file_name().to_string_lossy().into_owned(), exclusion));
    }
    let folder = if dir == Path::new(ROOT) { Exclusion::Included } else { classify(dir, &skipped) };
    Ok(BackupListing { folder, entries })
}

/// Counts the folders a search looked through and reports them now and then.
struct Progress<'a> {
    id: u64,
    folders: u64,
    reported: Instant,
    send: &'a dyn Fn(BackupEvent),
}

impl<'a> Progress<'a> {
    fn new(id: u64, send: &'a dyn Fn(BackupEvent)) -> Self {
        Self { id, folders: 0, reported: Instant::now(), send }
    }

    fn folder(&mut self) {
        self.folders += 1;
        if self.reported.elapsed() >= PROGRESS_EVERY {
            self.report();
        }
    }

    fn report(&mut self) {
        self.reported = Instant::now();
        (self.send)(BackupEvent::Progress { id: self.id, folders: self.folders });
    }
}

/// The folders below `root` to walk on, in the order found: not other volumes, version control
/// data or links, and not folders already left out as a whole.
fn subfolders(dir: &Path, device: u64, skipped: &[PathBuf]) -> Vec<(PathBuf, bool)> {
    let Ok(reader) = fs::read_dir(dir) else { return Vec::new() };
    reader
        .flatten()
        .filter_map(|entry| {
            let kind = entry.file_type().ok()?;
            let path = entry.path();
            let folder = kind.is_dir()
                && !VERSION_CONTROL.iter().any(|name| entry.file_name() == *name)
                && entry.metadata().is_ok_and(|metadata| metadata.dev() == device);
            Some((path, folder))
        })
        .filter(|(path, folder)| *folder || marked(path) || skipped.iter().any(|skip| skip == path))
        .collect()
}

fn device(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| metadata.dev())
}

/// What a folder inside an excluded `root` inherits from it.
fn beneath(root: &Path, exclusion: &Exclusion) -> Exclusion {
    match exclusion {
        Exclusion::Inside(folder) => Exclusion::Inside(folder.clone()),
        Exclusion::System => Exclusion::System,
        _ => Exclusion::Inside(root.to_path_buf()),
    }
}

fn caches(id: u64, root: &Path, cancelled: &AtomicBool, send: &dyn Fn(BackupEvent)) -> Option<String> {
    let Some(device) = device(root).filter(|_| root.is_dir()) else {
        return Some(tr!(backup_not_a_folder, folder = root.display()));
    };
    let skipped = skipped();
    let around = classify(root, &skipped);
    let mut progress = Progress::new(id, send);
    let mut queue = VecDeque::from([root.to_path_buf()]);
    let mut found = 0;
    while let Some(dir) = queue.pop_front() {
        progress.folder();
        for (path, folder) in subfolders(&dir, device, &skipped) {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            if !folder {
                continue;
            }
            if crate::search::generated(&path) {
                let exclusion = if around.excluded() { beneath(root, &around) } else { classify(&path, &skipped) };
                let item = BackupItem { path: path.clone(), dir: true, exclusion, ..BackupItem::default() };
                send(BackupEvent::Found { id, items: vec![item] });
                if let Some(bytes) = measure(&path, cancelled, &mut progress) {
                    send(BackupEvent::Sized { id, path, bytes });
                }
                found += 1;
            } else if !marked(&path) && !skipped.contains(&path) {
                queue.push_back(path);
            }
        }
        if found >= FOUND_LIMIT {
            break;
        }
    }
    progress.report();
    None
}

/// The bytes of the files in a folder, without following links.
fn measure(root: &Path, cancelled: &AtomicBool, progress: &mut Progress<'_>) -> Option<u64> {
    let mut total = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        progress.folder();
        let Ok(reader) = fs::read_dir(&dir) else { continue };
        for entry in reader.flatten() {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => stack.push(entry.path()),
                Ok(kind) if kind.is_file() => total += entry.metadata().map_or(0, |metadata| metadata.len()),
                _ => {}
            }
        }
    }
    Some(total)
}

fn all(id: u64, root: &Path, cancelled: &AtomicBool, send: &dyn Fn(BackupEvent)) -> Option<String> {
    forget_skipped();
    let skipped = skipped();
    let fixed = skipped
        .iter()
        .map(|path| inspect(path, &skipped))
        .map(|item| BackupItem { exclusion: Exclusion::Path, ..item });
    send(BackupEvent::Found { id, items: fixed.collect() });
    send(BackupEvent::Found { id, items: volumes() });
    send(BackupEvent::Found { id, items: system_places() });

    let mut seen = HashSet::new();
    let mut batch = Vec::new();
    let flush = |batch: &mut Vec<BackupItem>| {
        if !batch.is_empty() {
            send(BackupEvent::Found { id, items: std::mem::take(batch) });
        }
    };
    for path in spotlight() {
        keep(&mut seen, &mut batch, path);
    }
    flush(&mut batch);

    let Some(device) = device(root).filter(|_| root.is_dir()) else {
        return Some(tr!(backup_not_a_folder, folder = root.display()));
    };
    if marked(root) {
        keep(&mut seen, &mut batch, root.to_path_buf());
    }
    let mut progress = Progress::new(id, send);
    let mut queue = VecDeque::from([root.to_path_buf()]);
    while let Some(dir) = queue.pop_front() {
        progress.folder();
        for (path, folder) in subfolders(&dir, device, &skipped) {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            if marked(&path) {
                keep(&mut seen, &mut batch, path);
            } else if folder && !skipped.contains(&path) {
                queue.push_back(path);
            }
            if batch.len() >= FOUND_BATCH {
                flush(&mut batch);
            }
        }
        if seen.len() >= FOUND_LIMIT {
            break;
        }
    }
    flush(&mut batch);
    progress.report();
    None
}

/// Adds a marked item to the next batch, once.
fn keep(seen: &mut HashSet<PathBuf>, batch: &mut Vec<BackupItem>, path: PathBuf) {
    if seen.insert(path.clone()) {
        let dir = fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_dir());
        batch.push(BackupItem { path, dir, exclusion: Exclusion::Sticky, ..BackupItem::default() });
    }
}

/// Items Spotlight knows carry the exclusion mark. It misses some, such as those in hidden
/// folders, so each one is checked and the root is walked as well.
fn spotlight() -> Vec<PathBuf> {
    let Ok(output) = run(MDFIND, [SPOTLIGHT_QUERY], SPOTLIGHT_TIMEOUT) else { return Vec::new() };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(PathBuf::from)
        .filter(|path| marked(path))
        .take(FOUND_LIMIT)
        .collect()
}

/// Mounted volumes Time Machine leaves out, apart from the startup disk.
fn volumes() -> Vec<BackupItem> {
    let Ok(reader) = fs::read_dir(VOLUMES) else { return Vec::new() };
    reader
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_volume(path) && fs::canonicalize(path).is_ok_and(|real| real != Path::new(ROOT)))
        .filter(|path| asked(path) == Some(true))
        .map(|path| BackupItem { path, dir: true, volume: true, exclusion: Exclusion::Volume, ..BackupItem::default() })
        .collect()
}

fn system_places() -> Vec<BackupItem> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let skipped = skipped();
    SYSTEM_PLACES
        .iter()
        .filter_map(|place| match place.strip_prefix("~/") {
            Some(rest) => home.as_ref().map(|home| home.join(rest)),
            None => Some(PathBuf::from(place)),
        })
        .filter(|path| path.exists())
        .map(|path| inspect(&path, &skipped))
        .filter(|item| item.exclusion == Exclusion::System)
        .collect()
}

/// A `tmutil` verb and its flag, if it has one.
type Verb = (&'static str, Option<&'static str>);

/// The `tmutil` command lines a change needs, the first without and the second with the
/// administrator's password.
fn commands(change: BackupChange, items: &[BackupItem]) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
    let mut groups: Vec<(Verb, Vec<String>)> = Vec::new();
    for item in items {
        let kind = match (change, &item.exclusion) {
            (BackupChange::Exclude | BackupChange::ExcludePath, _) if item.volume => ("addexclusion", Some("-v")),
            (BackupChange::Exclude, _) => ("addexclusion", None),
            (BackupChange::ExcludePath, _) => ("addexclusion", Some("-p")),
            (BackupChange::Include, Exclusion::Sticky) => ("removeexclusion", None),
            (BackupChange::Include, Exclusion::Path) => ("removeexclusion", Some("-p")),
            (BackupChange::Include, Exclusion::Volume) => ("removeexclusion", Some("-v")),
            (BackupChange::Include, _) => continue,
        };
        let path = item.path.to_string_lossy().into_owned();
        match groups.iter_mut().find(|(group, _)| *group == kind) {
            Some((_, paths)) => paths.push(path),
            None => groups.push((kind, vec![path])),
        }
    }
    let line = |(verb, flag): Verb, paths: Vec<String>| {
        [TMUTIL.to_owned(), verb.to_owned()].into_iter().chain(flag.map(str::to_owned)).chain(paths).collect::<Vec<_>>()
    };
    let (admin, plain): (Vec<_>, Vec<_>) = groups.into_iter().partition(|((_, flag), _)| flag.is_some());
    (
        plain.into_iter().map(|(kind, paths)| line(kind, paths)).collect(),
        admin.into_iter().map(|(kind, paths)| line(kind, paths)).collect(),
    )
}

fn apply(change: BackupChange, items: &[BackupItem]) -> (Vec<BackupItem>, Option<String>) {
    // A typed path may name a volume, which only a volume exclusion covers.
    let items: Vec<BackupItem> =
        items.iter().map(|item| BackupItem { volume: item.volume || is_volume(&item.path), ..item.clone() }).collect();
    let (plain, admin) = commands(change, &items);
    let mut errors = Vec::new();
    for words in plain {
        if let Err(error) = run(&words[0], &words[1..], ASK_TIMEOUT).and_then(succeeded) {
            errors.push(explain(&error.to_string()));
        }
    }
    if !admin.is_empty() {
        let outcome = elevated(&admin);
        if let Err(error) = outcome {
            errors.push(explain(&error.to_string()));
        }
    }
    forget_skipped();
    let skipped = skipped();
    let items = items.iter().map(|item| inspect(&item.path, &skipped)).collect();
    (items, (!errors.is_empty()).then(|| errors.join("; ")))
}

#[cfg(target_os = "macos")]
fn elevated(lines: &[Vec<String>]) -> Result<()> {
    let script = crate::admin::apple_script_all(lines)?;
    run(OSASCRIPT, ["-e", script.as_str()], ADMIN_TIMEOUT).and_then(succeeded)
}

#[cfg(not(target_os = "macos"))]
fn elevated(_lines: &[Vec<String>]) -> Result<()> {
    Err(anyhow!(tr!(backup_macos_only)))
}

fn succeeded(output: Output) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }
    let error = String::from_utf8_lossy(&output.stderr);
    let text = if error.trim().is_empty() { String::from_utf8_lossy(&output.stdout) } else { error };
    Err(anyhow!("{}", text.trim()))
}

fn explain(error: &str) -> String {
    if error.contains(USER_CANCELED) {
        tr!(backup_password_cancelled).into()
    } else if error.contains(FULL_DISK_ACCESS) {
        tr!(backup_needs_full_disk_access).into()
    } else {
        error.lines().next().unwrap_or(error).to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str, exclusion: Exclusion, volume: bool) -> BackupItem {
        BackupItem { path: path.into(), exclusion, volume, ..BackupItem::default() }
    }

    #[test]
    fn the_excluded_paths_come_from_the_preferences_xml() {
        let xml = "<plist><dict><key>AutoBackup</key><true/><key>SkipPaths</key>\n\t<array>\n\t\t<string>/home/me/.npm</string>\n\t\t<string>/home/me/R&amp;D &lt;old&gt;</string>\n\t</array>\n<key>Other</key><array><string>/no</string></array></dict></plist>";
        assert_eq!(plist_strings(xml, "SkipPaths"), ["/home/me/.npm", "/home/me/R&D <old>"]);
        assert!(plist_strings(xml, "Missing").is_empty());
        assert!(plist_strings("<key>SkipPaths</key><string>/x</string>", "SkipPaths").is_empty());
    }

    #[test]
    fn changes_run_without_the_password_unless_a_path_or_volume_is_involved() {
        let items = [
            item("/work/target", Exclusion::Included, false),
            item("/Volumes/Spare", Exclusion::Included, true),
            item("/work/.npm", Exclusion::Included, false),
        ];
        let (plain, admin) = commands(BackupChange::Exclude, &items);
        assert_eq!(plain, [vec![TMUTIL, "addexclusion", "/work/target", "/work/.npm"]]);
        assert_eq!(admin, [vec![TMUTIL, "addexclusion", "-v", "/Volumes/Spare"]]);

        let items = [
            item("/a", Exclusion::Sticky, false),
            item("/b", Exclusion::Path, false),
            item("/c", Exclusion::System, false),
            item("/d", Exclusion::Inside("/a".into()), false),
        ];
        let (plain, admin) = commands(BackupChange::Include, &items);
        assert_eq!(plain, [vec![TMUTIL, "removeexclusion", "/a"]]);
        assert_eq!(admin, [vec![TMUTIL, "removeexclusion", "-p", "/b"]]);
    }

    #[test]
    fn errors_say_what_to_do_about_them() {
        assert_eq!(explain("execution error: User canceled. (-128)"), tr!(backup_password_cancelled));
        assert_eq!(
            explain("tmutil: isexcluded requires Full Disk Access privileges.\nTo allow…"),
            tr!(backup_needs_full_disk_access)
        );
        assert_eq!(explain("tmutil: something else\nmore"), "tmutil: something else");
    }

    #[test]
    fn inherited_exclusions_name_the_folder_that_has_the_exclusion() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("project/node_modules");
        fs::create_dir_all(folder.join("pkg")).unwrap();
        let skipped = vec![root.path().join("project")];
        assert_eq!(own_or_inherited(&root.path().join("project"), &skipped), Some(Exclusion::Path));
        assert_eq!(
            own_or_inherited(&folder.join("pkg"), &skipped),
            Some(Exclusion::Inside(root.path().join("project")))
        );
        assert_eq!(own_or_inherited(root.path(), &skipped), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_mark_tmutil_leaves_is_what_counts_as_a_sticky_exclusion() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("cache");
        fs::create_dir(&folder).unwrap();
        assert!(!marked(&folder));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let output = runtime.block_on(async {
            tokio::process::Command::new(TMUTIL).arg("addexclusion").arg(&folder).output().await.unwrap()
        });
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(marked(&folder));
        assert_eq!(own_or_inherited(&folder.join("inner"), &[]), Some(Exclusion::Inside(folder.clone())));
    }
}
