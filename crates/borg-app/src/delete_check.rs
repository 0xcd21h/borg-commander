//! Counting what a delete removes and finding the programs that use those files, while the
//! delete dialog asks. Both only read.

use anyhow::Result;
use borg_core::{DeleteFound, DeleteSize, FileHolder, Msg};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::sync::mpsc::UnboundedSender;

/// How long counting runs before the dialog shows what it found so far.
const COUNT_TIME: Duration = Duration::from_secs(3);
/// How long listing the open files of every process may take.
const HOLDERS_TIME: Duration = Duration::from_secs(10);

/// An open file: the process, its name, and the path it has open.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenFile {
    pid: u32,
    name: String,
    path: PathBuf,
}

pub(crate) fn check(id: u64, sources: Vec<PathBuf>, messages: UnboundedSender<Msg>) {
    let counting = sources.clone();
    let counted = messages.clone();
    tokio::task::spawn_blocking(move || {
        let size = tally(&counting, Instant::now() + COUNT_TIME);
        let _ = counted.send(Msg::DeleteChecked { id, found: DeleteFound::Size(size) });
    });
    tokio::spawn(async move {
        let holders = open_files().await.map(|open| holders(&sources, open)).map_err(|error| error.to_string());
        let _ = messages.send(Msg::DeleteChecked { id, found: DeleteFound::Holders(holders) });
    });
}

/// Adds up the files under `sources` the way a delete walks them, links counted as files of
/// their own and never followed, until `deadline`.
fn tally(sources: &[PathBuf], deadline: Instant) -> DeleteSize {
    let mut size = DeleteSize { complete: true, ..Default::default() };
    let mut pending = sources.to_vec();
    while let Some(path) = pending.pop() {
        if Instant::now() >= deadline {
            size.complete = false;
            break;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            size.complete = false;
            continue;
        };
        if !meta.is_dir() {
            size.files += 1;
            size.bytes += if meta.file_type().is_symlink() { 0 } else { meta.len() };
            continue;
        }
        let Ok(children) = fs::read_dir(&path) else {
            size.complete = false;
            continue;
        };
        for child in children {
            match child {
                Ok(child) => pending.push(child.path()),
                Err(_) => size.complete = false,
            }
        }
    }
    size
}

/// The programs other than Borg with a file open under `sources`, one row each, by name.
fn holders(sources: &[PathBuf], open: Vec<OpenFile>) -> Vec<FileHolder> {
    let targets: Vec<PathBuf> = sources.iter().filter_map(|source| resolved(source)).collect();
    let own = std::process::id();
    let holders: BTreeMap<(String, u32), ()> = open
        .into_iter()
        .filter(|file| file.pid != own && targets.iter().any(|target| file.path.starts_with(target)))
        .map(|file| ((file.name, file.pid), ()))
        .collect();
    holders.into_keys().map(|(name, pid)| FileHolder { pid, name }).collect()
}

/// `source` with the links in its folder resolved, as the system names open files. A link is
/// never open itself, so it has no holders.
fn resolved(source: &Path) -> Option<PathBuf> {
    if fs::symlink_metadata(source).ok()?.file_type().is_symlink() {
        return None;
    }
    Some(fs::canonicalize(source.parent()?).ok()?.join(source.file_name()?))
}

#[cfg(target_os = "macos")]
async fn open_files() -> Result<Vec<OpenFile>> {
    let mut command = tokio::process::Command::new(LSOF);
    command.args(["-n", "-P", "-w", "+c", "0", "-F0pcn"]).current_dir(ROOT);
    Ok(parse_lsof(&crate::activity::bounded_output(command, HOLDERS_TIME).await?))
}

#[cfg(target_os = "macos")]
const LSOF: &str = "/usr/sbin/lsof";
/// Where `lsof` runs, so its own working folder never shows among the files being deleted.
#[cfg(target_os = "macos")]
const ROOT: &str = "/";

/// Reads `lsof -F0pcn`: a process's fields, then one name field for each file it has open.
#[cfg(any(target_os = "macos", test))]
fn parse_lsof(bytes: &[u8]) -> Vec<OpenFile> {
    let mut files = Vec::new();
    let mut process: Option<(u32, String)> = None;
    for field in bytes.split(|&b| b == 0) {
        let field = field.strip_prefix(b"\n").unwrap_or(field);
        let Some((&key, value)) = field.split_first() else { continue };
        let value = String::from_utf8_lossy(value);
        match key {
            b'p' => process = value.parse().ok().map(|pid| (pid, String::new())),
            b'c' => {
                if let Some((_, name)) = process.as_mut() {
                    *name = value.into_owned();
                }
            }
            b'n' => {
                if let Some((pid, name)) = &process {
                    files.push(OpenFile { pid: *pid, name: name.clone(), path: PathBuf::from(value.as_ref()) });
                }
            }
            _ => {}
        }
    }
    files
}

#[cfg(target_os = "linux")]
async fn open_files() -> Result<Vec<OpenFile>> {
    let deadline = Instant::now() + HOLDERS_TIME;
    tokio::task::spawn_blocking(move || proc_open_files(Path::new(PROC), deadline)).await?
}

#[cfg(target_os = "linux")]
const PROC: &str = "/proc";

/// The working folder, the program and the open files of every process this user may look
/// at; the others are skipped.
#[cfg(target_os = "linux")]
fn proc_open_files(proc: &Path, deadline: Instant) -> Result<Vec<OpenFile>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(proc)?.flatten() {
        if Instant::now() >= deadline {
            anyhow::bail!(borg_i18n::tr!(activity_inspection_timeout));
        }
        let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()) else { continue };
        let dir = entry.path();
        let name = fs::read_to_string(dir.join("comm")).map(|name| name.trim_end().to_owned()).unwrap_or_default();
        let descriptors = fs::read_dir(dir.join("fd")).into_iter().flatten().flatten().map(|fd| fd.path());
        for link in [dir.join("cwd"), dir.join("exe")].into_iter().chain(descriptors) {
            if let Ok(path) = fs::read_link(link) {
                files.push(OpenFile { pid, name: name.clone(), path });
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests;
