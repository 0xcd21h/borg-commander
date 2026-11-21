//! Reads the places programs start from, for the startup items screen and the agent: each
//! file, who owns it and when it changed, whether the program it runs is there, and the rules
//! the file and the program match.

use anyhow::{Context, Result};
use borg_core::{
    Msg, ProgramPresence, RuleHit, StartupEntry, StartupFormat, StartupItem, StartupReport, StartupSpot, StartupSystem,
    UNIT_LINK_FOLDERS, newest_first, parse_startup, startup_spots,
};
use borg_rules::{RuleMatch, Rules, Subject};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, Read},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};
use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle};

/// Bytes of a startup file read; the rules see these too.
const FILE_LIMIT: u64 = 1024 * 1024;
/// Programs larger than this are not scanned with the rules.
const PROGRAM_LIMIT: u64 = 64 * 1024 * 1024;
/// Where a program named without a folder is looked for: the folders cron and launchd search.
const SEARCH_PATH: &[&str] = &["/usr/local/sbin", "/usr/local/bin", "/usr/sbin", "/usr/bin", "/sbin", "/bin"];
const HOME_PREFIX: &str = "~/";
const CRONTAB: &str = "crontab";
const LIST_CRONTAB: &str = "-l";

#[cfg(target_os = "macos")]
const SYSTEM: StartupSystem = StartupSystem::MacOs;
#[cfg(not(target_os = "macos"))]
const SYSTEM: StartupSystem = StartupSystem::Linux;

pub struct StartupItems {
    messages: UnboundedSender<Msg>,
    task: Option<JoinHandle<()>>,
}

impl StartupItems {
    pub fn new(messages: UnboundedSender<Msg>) -> Self {
        Self { messages, task: None }
    }

    /// Reads every place in the background, in place of a read still running.
    pub fn request(&mut self, id: u64) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        let messages = self.messages.clone();
        self.task = Some(tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(read)
                .await
                .map_err(anyhow::Error::from)
                .and_then(|result| result)
                .map_err(|error| format!("{error:#}"));
            let _ = messages.send(Msg::StartupItems { id, result });
        }));
    }
}

/// Every startup item of the user Borg runs as, with Borg's rules and the user's.
pub fn read() -> Result<StartupReport> {
    let home = std::env::var_os("HOME").map(PathBuf::from).context("HOME is not set")?;
    let rules = crate::rule_scan::compile(crate::rule_scan::folder().as_deref());
    Ok(read_places(&startup_spots(SYSTEM, &home), &home, &rules))
}

/// The items `spots` hold, newest change first, with the places that are there but could not
/// be read.
pub fn read_places(spots: &[StartupSpot], home: &Path, rules: &Rules) -> StartupReport {
    let mut reader =
        Reader { home, rules, seen: HashSet::new(), programs: HashMap::new(), report: StartupReport::default() };
    for spot in spots {
        reader.spot(spot);
    }
    let mut report = reader.report;
    newest_first(&mut report.items);
    report.problems = rules.problems().to_vec();
    report
}

/// What a file says about all its items: who owns it, when it changed, and the rules it matched.
struct Facts {
    owner: Option<String>,
    modified: Option<SystemTime>,
    hits: Vec<RuleMatch>,
}

struct Reader<'a> {
    home: &'a Path,
    rules: &'a Rules,
    /// Files already read, by their real path, so a folder linked into another is read once.
    seen: HashSet<PathBuf>,
    /// What each program matched, so a program many items run is scanned once.
    programs: HashMap<PathBuf, Vec<RuleMatch>>,
    report: StartupReport,
}

impl Reader<'_> {
    fn spot(&mut self, spot: &StartupSpot) {
        if !spot.folder {
            self.file(spot, &spot.path);
            return;
        }
        let names = match folder_names(&spot.path) {
            Ok(names) => names,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return,
            Err(error) if spot.format == StartupFormat::Crontab => {
                if !self.own_crontab(spot) {
                    self.unread(&spot.path, &error);
                }
                return;
            }
            Err(error) => return self.unread(&spot.path, &error),
        };
        let mut linked = Vec::new();
        for name in names {
            let path = spot.path.join(&name);
            if spot.format == StartupFormat::Unit && UNIT_LINK_FOLDERS.iter().any(|ending| name.ends_with(ending)) {
                linked.push(path);
            } else if spot.takes(&name) {
                self.file(spot, &path);
            }
        }
        for folder in linked {
            for name in folder_names(&folder).unwrap_or_default().into_iter().filter(|name| spot.takes(name)) {
                self.file(spot, &folder.join(name));
            }
        }
    }

    fn unread(&mut self, path: &Path, error: &io::Error) {
        self.report.unread.push((path.to_path_buf(), error.to_string()));
    }

    fn file(&mut self, spot: &StartupSpot, path: &Path) {
        let Ok(metadata) = fs::metadata(path) else { return };
        if !metadata.is_file() || !self.seen.insert(fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())) {
            return;
        }
        let bytes = match read_start(path) {
            Ok(bytes) => bytes,
            Err(error) => return self.unread(path, &error),
        };
        let name = path.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let mut entries = parse_startup(spot.format, &name, &bytes);
        if entries.is_empty() && spot.format == StartupFormat::Launchd {
            entries.push(StartupEntry { name: name.clone(), ..StartupEntry::default() });
        }
        let facts = Facts {
            owner: Some(owner(metadata.uid())),
            modified: metadata.modified().ok(),
            hits: self.scan(&bytes, path),
        };
        for entry in entries {
            self.add(spot, path, entry, &facts);
        }
    }

    /// The user's own crontab through `crontab -l`, when the folder of crontabs is closed to
    /// them, as it is on macOS. Whether `crontab` ran; a user without a crontab has no items.
    fn own_crontab(&mut self, spot: &StartupSpot) -> bool {
        let Some(user) = uzers::get_current_username().map(|name| name.to_string_lossy().into_owned()) else {
            return false;
        };
        let Ok(output) = Command::new(CRONTAB).arg(LIST_CRONTAB).output() else { return false };
        if output.status.success() {
            let path = spot.path.join(&user);
            let facts = Facts { owner: Some(user.clone()), modified: None, hits: self.scan(&output.stdout, &path) };
            for entry in parse_startup(StartupFormat::Crontab, &user, &output.stdout) {
                self.add(spot, &path, entry, &facts);
            }
        }
        true
    }

    fn add(&mut self, spot: &StartupSpot, path: &Path, entry: StartupEntry, facts: &Facts) {
        let mut item = StartupItem::new(spot, path.to_path_buf(), entry);
        item.owner = facts.owner.clone();
        item.modified = facts.modified;
        item.hits = facts.hits.iter().map(|rule| RuleHit { rule: rule.clone(), in_program: false }).collect();
        if let Some(program) = item.found.program.clone() {
            let (presence, resolved) = locate(&program, self.home);
            item.presence = presence;
            if let Some(found) = resolved.as_ref().filter(|found| found.as_path() != Path::new(&program)) {
                item.resolved = Some(found.clone());
            }
            if let Some(found) = resolved.filter(|found| found.as_path() != path && !spot.read_only) {
                let matched = self.program_hits(&found);
                item.hits.extend(matched.into_iter().map(|rule| RuleHit { rule, in_program: true }));
            }
        }
        self.report.items.push(item);
    }

    fn scan(&self, bytes: &[u8], path: &Path) -> Vec<RuleMatch> {
        self.rules.scan(bytes, Subject { path: &path.to_string_lossy(), memory: false }).matches
    }

    fn program_hits(&mut self, program: &Path) -> Vec<RuleMatch> {
        if let Some(matched) = self.programs.get(program) {
            return matched.clone();
        }
        let mut buffer = Vec::new();
        let matched = match fs::metadata(program) {
            Ok(metadata) if metadata.is_file() && metadata.len() <= PROGRAM_LIMIT => {
                crate::rule_scan::scan_file(self.rules, program, &mut buffer).ok().flatten().unwrap_or_default()
            }
            _ => Vec::new(),
        };
        self.programs.insert(program.to_path_buf(), matched.clone());
        matched
    }
}

/// A folder's entries by name, in order.
fn folder_names(folder: &Path) -> io::Result<Vec<String>> {
    let mut names: Vec<String> = fs::read_dir(folder)?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    Ok(names)
}

fn read_start(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(FILE_LIMIT).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn owner(uid: u32) -> String {
    uzers::get_user_by_uid(uid).map_or_else(|| uid.to_string(), |user| user.name().to_string_lossy().into_owned())
}

/// Whether a program is on disk, and where: an absolute path or one in the home folder as
/// given, a bare name in the folders cron and launchd search. A relative path, or a bare name
/// found nowhere, may be found through a search path the file sets, so neither counts as
/// missing.
fn locate(program: &str, home: &Path) -> (ProgramPresence, Option<PathBuf>) {
    let path = match program.strip_prefix(HOME_PREFIX) {
        Some(rest) => home.join(rest),
        None => PathBuf::from(program),
    };
    if path.is_absolute() {
        return if fs::metadata(&path).is_ok() {
            (ProgramPresence::Present, Some(path))
        } else {
            (ProgramPresence::Missing, None)
        };
    }
    if program.contains('/') {
        return (ProgramPresence::Unknown, None);
    }
    SEARCH_PATH
        .iter()
        .map(|folder| Path::new(folder).join(program))
        .find(|candidate| candidate.is_file())
        .map_or((ProgramPresence::Unknown, None), |found| (ProgramPresence::Present, Some(found)))
}

#[cfg(test)]
mod tests;
