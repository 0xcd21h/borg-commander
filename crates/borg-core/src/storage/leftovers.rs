//! Which of Borg's files no running Borg uses any more. The app lists Borg's folders and says
//! which processes run and which sockets answer; these rules decide what is left over. A name
//! Borg does not recognize is never left over, and nothing younger than [`SETTLE`] is, so a
//! Borg that is starting keeps what it just made.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// How old a file must be before it can be left over.
pub const SETTLE: Duration = Duration::from_secs(10 * 60);
/// Folders of other Borg versions, and folders without a process in their name, are left over
/// once nothing in them changed for this long. macOS clears its temporary folder after as long.
pub const STALE: Duration = Duration::from_secs(3 * 24 * 60 * 60);
/// Copies out of containers, remote previews and files unpacked for the viewer are left over
/// after a day.
pub const SCRATCH_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// The file in the runtime folder that says until when a running Borg uses it.
pub const TTL_FILE: &str = "TTL";
/// Where Borg copies files out of containers, in the runtime folder.
pub const DOCKER_SCRATCH: &str = "docker";
/// Where Borg keeps the files it shows from remote machines, in the runtime folder.
pub const REMOTE_PREVIEWS: &str = "remote-preview";
/// Where Borg unpacks the files it shows from archives, in the runtime folder.
pub const ARCHIVE_VIEWS: &str = "archive-view";
/// The start of a remote console's folder in the runtime folder, followed by its process.
pub const CONSOLE_PREFIX: &str = "console-";
/// The start of the certificates a capture's proxy has programs trust, in the runtime folder,
/// followed by its process and the capture.
pub const TRUST_PREFIX: &str = "trust-";
pub const TRUST_EXTENSION: &str = "pem";
/// The start of a size scan's file in the cache folder.
pub const SIZES_PREFIX: &str = "sizes-";
/// The list of size scans the cache keeps.
pub const SIZES_INDEX: &str = "sizes-index.json";
/// A size scan being written, followed by its process.
pub const SCAN_PREFIX: &str = ".scan-";
pub const SQLITE_SUFFIX: &str = ".sqlite";
/// Size scans of Borg 0.4 to 0.5, which later versions no longer read.
const OLD_SIZES_SUFFIX: &str = ".json";
/// SQLite's own files beside a database.
const SQLITE_SIDE_FILES: [&str; 3] = ["-journal", "-wal", "-shm"];
/// The folder of name indexes in the cache folder.
pub const INDEX_DIR: &str = "index";
pub const INDEX_SUFFIX: &str = ".idx";
/// The process watcher's alerts, in the state folder. The watcher writes it and every Borg on
/// the machine reads it; nothing cleans it up, since it is the user's record of what happened.
pub const ALERT_LOG: &str = "alerts.json";
/// An alert log Borg could not read, renamed so the watcher can start a new one, and kept for
/// the same reason.
pub const ALERT_LOG_ASIDE: &str = "alerts-unreadable.json";
/// What the process watcher has learned about this machine, in the state folder, and kept for
/// the same reason.
pub const WATCH_BASELINE: &str = "watch-baseline.json";
/// The starts the user has allowed on the Alerts page, in the state folder. The page writes it
/// and the watcher reads it, so neither can lose what the other wrote.
pub const WATCH_ALLOWED: &str = "watch-allowed.json";
/// Messages the process watcher has yet to send off the machine, in the state folder. Nothing
/// cleans it up, so a message waits however long the place it goes is away.
pub const WATCH_OUTBOX: &str = "watch-outbox.json";
/// An outbox Borg could not read, renamed so the sender can start a new one, and kept so the
/// messages still waiting in it are there to read.
pub const WATCH_OUTBOX_ASIDE: &str = "watch-outbox-unreadable.json";
/// A file Borg is replacing, written next to it first.
pub const STAGING_SUFFIX: &str = "part";
pub const SOCKET_SUFFIX: &str = ".sock";
const PROCESS_SEPARATOR: char = '-';
const TTL_STAGING: &str = "TTL.";
const VERSION_SEPARATOR: char = '.';

/// What the app found in one of Borg's folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub kind: FoundKind,
    /// The newest change to it or to anything inside it.
    pub modified: SystemTime,
    /// Its bytes, with everything inside it.
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoundKind {
    File,
    /// A folder, with what the app listed inside it.
    Folder(Vec<Found>),
    /// A Unix socket, and whether a process answers on it.
    Socket {
        answers: bool,
    },
}

/// A file or folder no running Borg uses, and its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leftover {
    pub path: PathBuf,
    pub bytes: u64,
}

/// What the rules know besides the files: the time, this Borg's version, whether any Borg used
/// the runtime folder lately, and which processes run.
pub struct Judge<'a> {
    pub now: SystemTime,
    pub version: &'a str,
    /// The runtime folder's time to live had passed when this Borg started.
    pub idle: bool,
    pub runs: &'a dyn Fn(u32) -> bool,
}

impl Judge<'_> {
    fn age(&self, found: &Found) -> Duration {
        self.now.duration_since(found.modified).unwrap_or_default()
    }

    fn settled(&self, found: &Found) -> bool {
        self.age(found) >= SETTLE
    }

    fn stale(&self, found: &Found) -> bool {
        self.age(found) >= STALE
    }

    /// The process `found` names after `prefix` has ended.
    fn ended(&self, found: &Found, prefix: &str, rest: fn(&str) -> bool) -> bool {
        process(&found.name, prefix, rest).is_some_and(|pid| !(self.runs)(pid))
    }
}

/// The process id after `prefix` in `name`, when what follows it is what `rest` expects.
fn process(name: &str, prefix: &str, rest: fn(&str) -> bool) -> Option<u32> {
    let tail = name.strip_prefix(prefix)?;
    let digits = tail.find(|c: char| !c.is_ascii_digit()).unwrap_or(tail.len());
    let pid = tail[..digits].parse().ok()?;
    rest(&tail[digits..]).then_some(pid)
}

fn nothing(rest: &str) -> bool {
    rest.is_empty()
}

fn separated(rest: &str) -> bool {
    rest.starts_with(PROCESS_SEPARATOR)
}

fn cd_request(rest: &str) -> bool {
    rest.is_empty() || rest.strip_prefix('.') == Some(borg_term::CD_STAGING)
}

fn is_version(name: &str) -> bool {
    let parts: Vec<&str> = name.split(VERSION_SEPARATOR).collect();
    parts.len() > 1 && parts.iter().all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn whole(path: PathBuf, found: &Found) -> Leftover {
    Leftover { path, bytes: found.bytes }
}

fn children(found: &Found) -> &[Found] {
    match &found.kind {
        FoundKind::Folder(children) => children,
        FoundKind::File | FoundKind::Socket { .. } => &[],
    }
}

/// What is left over in a runtime folder: the folders of other Borg versions nobody uses,
/// folder requests, staged files and capture proxy certificates of ended processes, remote
/// consoles that ended, and old copies out of containers, remote previews and files unpacked
/// for the viewer.
pub fn runtime_leftovers(root: &Path, entries: &[Found], judge: &Judge) -> Vec<Leftover> {
    entries.iter().flat_map(|found| runtime_entry(root, found, judge)).collect()
}

fn runtime_entry(root: &Path, found: &Found, judge: &Judge) -> Vec<Leftover> {
    let path = root.join(&found.name);
    match &found.kind {
        FoundKind::Folder(inside) if is_version(&found.name) => version_folder(path, found, inside, judge),
        FoundKind::Folder(inside)
            if [DOCKER_SCRATCH, REMOTE_PREVIEWS, ARCHIVE_VIEWS].contains(&found.name.as_str()) =>
        {
            inside
                .iter()
                .filter(|child| judge.age(child) >= SCRATCH_AGE)
                .map(|child| whole(path.join(&child.name), child))
                .collect()
        }
        FoundKind::Folder(_) if found.name.starts_with(CONSOLE_PREFIX) && judge.settled(found) => {
            let ended = match process(&found.name, CONSOLE_PREFIX, separated) {
                Some(pid) => !(judge.runs)(pid),
                None => judge.stale(found),
            };
            if ended { vec![whole(path, found)] } else { Vec::new() }
        }
        FoundKind::File if judge.settled(found) && runtime_file_ended(found, judge) => vec![whole(path, found)],
        _ => Vec::new(),
    }
}

fn runtime_file_ended(found: &Found, judge: &Judge) -> bool {
    judge.ended(found, TTL_STAGING, nothing) || judge.ended(found, TRUST_PREFIX, trust_bundle)
}

fn trust_bundle(rest: &str) -> bool {
    rest.strip_prefix(PROCESS_SEPARATOR)
        .and_then(|rest| Path::new(rest).extension())
        .is_some_and(|extension| extension == TRUST_EXTENSION)
}

fn version_folder(path: PathBuf, found: &Found, inside: &[Found], judge: &Judge) -> Vec<Leftover> {
    let used = inside
        .iter()
        .any(|child| process(&child.name, borg_term::CD_PREFIX, cd_request).is_some_and(|pid| (judge.runs)(pid)));
    let other = found.name != judge.version;
    if other && !used && judge.settled(found) && (judge.idle || judge.stale(found)) {
        return vec![whole(path, found)];
    }
    inside
        .iter()
        .filter(|child| judge.settled(child) && judge.ended(child, borg_term::CD_PREFIX, cd_request))
        .map(|child| whole(path.join(&child.name), child))
        .collect()
}

/// A runtime folder Borg no longer uses, as one made by a Borg older than 0.6 that others can
/// enter: left over once nothing in it changed for [`STALE`].
pub fn abandoned_leftover(path: &Path, found: &Found, judge: &Judge) -> Option<Leftover> {
    judge.stale(found).then(|| whole(path.to_path_buf(), found))
}

/// What is left over in a folder of sockets: sockets no process answers on, or the whole
/// folder once nothing in it answers, unless it is the folder this Borg uses.
pub fn socket_leftovers(path: &Path, found: &Found, current: bool, judge: &Judge) -> Vec<Leftover> {
    let inside = children(found);
    let silent: Vec<&Found> = inside
        .iter()
        .filter(|child| {
            matches!(child.kind, FoundKind::Socket { answers: false })
                && child.name.ends_with(SOCKET_SUFFIX)
                && judge.settled(child)
        })
        .collect();
    if !current && silent.len() == inside.len() && judge.settled(found) {
        return vec![whole(path.to_path_buf(), found)];
    }
    silent.into_iter().map(|child| whole(path.join(&child.name), child)).collect()
}

/// The cache's own lists: which size scans its index keeps, and which name indexes the
/// settings ask for, by file name.
pub struct CacheLists<'a> {
    /// `None` when the index of size scans cannot be read, so no size scan is left over.
    pub sizes: Option<&'a HashSet<String>>,
    pub indexes: &'a HashSet<String>,
}

/// What is left over in the cache folder: size scans of an older format or that the index no
/// longer lists, scans of processes that ended, name indexes of folders no longer indexed,
/// and files that were never finished.
pub fn cache_leftovers(root: &Path, entries: &[Found], lists: &CacheLists, judge: &Judge) -> Vec<Leftover> {
    entries
        .iter()
        .filter(|found| judge.settled(found))
        .flat_map(|found| {
            let path = root.join(&found.name);
            match &found.kind {
                FoundKind::Folder(inside) if found.name == INDEX_DIR => inside
                    .iter()
                    .filter(|child| judge.settled(child) && !lists.indexes.contains(&child.name))
                    .filter(|child| child.name.ends_with(INDEX_SUFFIX) || staged(&child.name))
                    .map(|child| whole(path.join(&child.name), child))
                    .collect(),
                FoundKind::File if cache_file_left(&found.name, found, lists, judge) => vec![whole(path, found)],
                _ => Vec::new(),
            }
        })
        .collect()
}

fn cache_file_left(name: &str, found: &Found, lists: &CacheLists, judge: &Judge) -> bool {
    if name == SIZES_INDEX {
        return false;
    }
    if judge.ended(found, SCAN_PREFIX, separated) || staged(name) {
        return true;
    }
    if !name.starts_with(SIZES_PREFIX) {
        return false;
    }
    if name.ends_with(OLD_SIZES_SUFFIX) {
        return true;
    }
    let database = SQLITE_SIDE_FILES.iter().find_map(|side| name.strip_suffix(side)).unwrap_or(name);
    database.ends_with(SQLITE_SUFFIX) && lists.sizes.is_some_and(|listed| !listed.contains(database))
}

fn staged(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|extension| extension == STAGING_SUFFIX)
}
