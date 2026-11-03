//! Unpacking chosen entries of an archive into a folder as a job, with progress, questions about
//! names already there, pause and cancel. A file stops at the size the archive gives it, so an
//! entry that holds more than its header says cannot fill the disk.

use std::{
    collections::HashMap,
    io::{self, Read, Write},
    os::fd::OwnedFd,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use borg_i18n::tr;
use borg_jobs::{Ask, Control, Decision, Event, Outcome, Progress, Totals};
use rustix::fs::{AtFlags, renameat, symlinkat, unlinkat};

use crate::{Archive, Error, Flow, Index, Item, ItemKind, name};

pub(crate) mod guard;
mod place;

use place::{Destination, Existing};

const CHUNK: usize = 1 << 20;
/// A symlink's target is short; more data than this is not a target.
const LINK_LIMIT: u64 = 4096;
const FOLDER_MODE: u32 = 0o755;
const PARTIAL_PREFIX: &str = ".";
const PARTIAL_SUFFIX: &str = ".borg-partial";

/// Entries to unpack: their paths inside the archive, chosen in the archive folder `base`,
/// whose own path is left out of where they land below `target`.
#[derive(Debug, Clone, Copy)]
pub struct Unpack<'a> {
    pub archive: Archive<'a>,
    pub paths: &'a [String],
    pub base: &'a str,
    pub target: &'a Path,
}

impl Unpack<'_> {
    /// What the chosen entries hold altogether.
    pub fn totals(&self, index: &Index) -> crate::Totals {
        Index::selected_totals(&index.selection(self.paths))
    }

    /// Runs the job, reporting to `emit` and asking through `control`.
    pub fn run(&self, index: &Index, control: &Control, emit: &mut dyn FnMut(Event)) -> Outcome {
        let selection = index.selection(self.paths);
        let totals = Index::selected_totals(&selection);
        let total = Totals { files: totals.files, bytes: totals.bytes };
        emit(Event::Scanning(total));
        let free = guard::free_space(self.target);
        for ask in guard::questions(self.archive.path, self.target, index.bytes, totals, free) {
            emit(Event::Asking(ask));
            if control.decide() != Decision::Proceed {
                return Outcome::Cancelled { done: Totals::default() };
            }
        }
        let destination = match Destination::open(self.target) {
            Ok(destination) => destination,
            Err(error) => return Outcome::Failed(format!("{}: {error}", self.target.display())),
        };
        let mut run = Run {
            job: self,
            index,
            destination,
            control,
            emit,
            done: Totals::default(),
            total,
            skipped: 0,
            overwrite: None,
            skip_errors: false,
            placed: HashMap::new(),
            skipped_folders: Vec::new(),
        };
        match run.all(&selection) {
            Ok(()) => Outcome::Done { done: run.done, skipped: run.skipped },
            Err(_) if control.cancelled() => Outcome::Cancelled { done: run.done },
            Err(Error::Stopped) => Outcome::Cancelled { done: run.done },
            Err(error) => Outcome::Failed(error.to_string()),
        }
    }
}

struct Run<'a, 'e> {
    job: &'a Unpack<'a>,
    index: &'a Index,
    destination: Destination,
    control: &'a Control,
    emit: &'e mut dyn FnMut(Event),
    done: Totals,
    total: Totals,
    skipped: u64,
    /// The answer to apply to every later name that is already there.
    overwrite: Option<bool>,
    skip_errors: bool,
    /// Where each unpacked entry landed below the target, for hard links to it.
    placed: HashMap<String, String>,
    skipped_folders: Vec<String>,
}

/// Why writing one entry did not finish.
enum Fault {
    Stopped,
    Failed(String),
}

impl From<io::Error> for Fault {
    fn from(error: io::Error) -> Self {
        Self::Failed(error.to_string())
    }
}

/// How a name that is already there is settled.
enum Settled {
    Write { name: String, replace: bool },
    Skip,
}

impl Run<'_, '_> {
    fn all(&mut self, selection: &[&Item]) -> Result<(), Error> {
        for item in selection.iter().filter(|item| item.is_folder()) {
            self.attempt(item, |run| run.folder(item))?;
        }
        let skipped = |item: &&&Item, folders: &[String]| folders.iter().any(|folder| name::within(&item.path, folder));
        let data: Vec<&Item> = selection
            .iter()
            .filter(|item| item.carries_data() && !skipped(item, &self.skipped_folders))
            .copied()
            .collect();
        let (archive, index) = (self.job.archive, self.index);
        archive.each(index, &data, &mut |item, reader| self.visit(item, reader))?;
        let links: Vec<&Item> = selection
            .iter()
            .filter(|item| matches!(item.kind, ItemKind::Symlink | ItemKind::HardLink) && !item.carries_data())
            .filter(|item| !skipped(item, &self.skipped_folders))
            .copied()
            .collect();
        for item in links {
            self.attempt(item, |run| run.link(item))?;
        }
        for item in selection.iter().rev().filter(|item| item.is_folder() && item.modified.is_some()) {
            let _ = self.folder_times(item);
        }
        Ok(())
    }

    fn check(&self) -> Result<(), Fault> {
        if self.control.proceed() { Ok(()) } else { Err(Fault::Stopped) }
    }

    /// Does `step` for `item`, asking what to do when it fails.
    fn attempt(&mut self, item: &Item, mut step: impl FnMut(&mut Self) -> Result<(), Fault>) -> Result<(), Error> {
        loop {
            match step(self) {
                Ok(()) => return Ok(()),
                Err(Fault::Stopped) => return Err(Error::Stopped),
                Err(Fault::Failed(message)) => match self.failed(item, message) {
                    Decision::Retry => continue,
                    Decision::Cancel => return Err(Error::Stopped),
                    _ => {
                        self.skip(item);
                        return Ok(());
                    }
                },
            }
        }
    }

    fn failed(&mut self, item: &Item, message: String) -> Decision {
        if self.skip_errors {
            return Decision::Skip;
        }
        let path = self.shown(item);
        (self.emit)(Event::Asking(Ask::Failure { path, message }));
        let decision = self.control.decide();
        if decision == Decision::SkipAll {
            self.skip_errors = true;
        }
        decision
    }

    fn skip(&mut self, item: &Item) {
        self.skipped += 1;
        if item.is_folder() {
            self.skipped_folders.push(item.path.clone());
        }
    }

    /// Writes one entry the archive hands over with its data, asking again on failure; a retry
    /// reads the entry from the archive once more.
    fn visit(&mut self, item: &Item, reader: &mut dyn Read) -> Result<Flow, Error> {
        match self.write(item, reader) {
            Ok(()) => Ok(Flow::Next),
            Err(Fault::Stopped) => Err(Error::Stopped),
            Err(Fault::Failed(message)) => match self.failed(item, message) {
                Decision::Retry => {
                    let (archive, index) = (self.job.archive, self.index);
                    archive.each(index, &[item], &mut |item, fresh| self.visit(item, fresh))?;
                    Ok(Flow::Next)
                }
                Decision::Cancel => Err(Error::Stopped),
                _ => {
                    self.skip(item);
                    Ok(Flow::Next)
                }
            },
        }
    }

    /// The entry's path below the target, split into its folders and its name.
    fn parts<'i>(&self, item: &'i Item) -> (Vec<&'i str>, &'i str) {
        let mut parts: Vec<&str> = name::relative(&item.path, self.job.base).split('/').collect();
        let last = parts.pop().unwrap_or_default();
        (parts, last)
    }

    fn shown(&self, item: &Item) -> PathBuf {
        self.job.archive.path.join(&item.path)
    }

    fn folder(&mut self, item: &Item) -> Result<(), Fault> {
        self.check()?;
        let (mut parts, last) = self.parts(item);
        parts.push(last);
        self.destination.folder(&parts, item.mode)?;
        self.placed.insert(item.path.clone(), parts.join("/"));
        Ok(())
    }

    fn write(&mut self, item: &Item, reader: &mut dyn Read) -> Result<(), Fault> {
        self.check()?;
        let (parts, last) = self.parts(item);
        let folder = self.destination.folder(&parts, FOLDER_MODE)?;
        let Settled::Write { name, replace } = self.settle(&folder, &parts, last, item)? else { return Ok(()) };
        let written = if item.kind == ItemKind::Symlink {
            let target = link_target(item, reader)?;
            place_symlink(&folder, &name, &target, replace)?;
            0
        } else {
            self.place_file(&folder, &name, replace, item, reader)?
        };
        self.placed.insert(item.path.clone(), joined(&parts, &name));
        self.done.files += 1;
        self.done.bytes += written;
        self.progress(item, 0);
        Ok(())
    }

    fn place_file(
        &mut self,
        folder: &OwnedFd,
        name: &str,
        replace: bool,
        item: &Item,
        reader: &mut dyn Read,
    ) -> Result<u64, Fault> {
        let partial = format!("{PARTIAL_PREFIX}{name}{PARTIAL_SUFFIX}");
        let writing = if replace { partial.as_str() } else { name };
        if replace {
            let _ = unlinkat(folder, writing, AtFlags::empty());
        }
        let mut file = Destination::create(folder, writing, item.mode)?;
        let copied = self.copy(item, reader, &mut file);
        let finished = copied.and_then(|written| {
            if let Some(modified) = item.modified {
                file.set_times(std::fs::FileTimes::new().set_modified(modified))?;
            }
            if replace {
                renameat(folder, writing, folder, name).map_err(io::Error::from)?;
            }
            Ok(written)
        });
        if finished.is_err() {
            let _ = unlinkat(folder, writing, AtFlags::empty());
        }
        finished
    }

    /// Copies the entry's data, refusing more than the archive says it holds and less than that.
    fn copy(&mut self, item: &Item, reader: &mut dyn Read, file: &mut std::fs::File) -> Result<u64, Fault> {
        let mut buffer = vec![0; CHUNK];
        let mut written: u64 = 0;
        loop {
            self.check()?;
            let read = match reader.read(&mut buffer) {
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted && !is_halt(&error) => continue,
                Err(error) => return Err(Fault::Failed(crate::read::read_failure(item, error).to_string())),
            };
            if read == 0 {
                break;
            }
            written += read as u64;
            if written > item.size {
                return Err(Fault::Failed(tr!(archive_entry_too_long, path = item.path)));
            }
            file.write_all(&buffer[..read])?;
            self.progress(item, written);
        }
        if written < item.size {
            return Err(Fault::Failed(tr!(archive_entry_short, path = item.path)));
        }
        Ok(written)
    }

    fn progress(&mut self, item: &Item, writing: u64) {
        let done = Totals { bytes: self.done.bytes + writing, ..self.done };
        let current = self.shown(item);
        (self.emit)(Event::Progress(Progress { done, total: self.total, current }));
    }

    /// Decides the name to write, asking when one is already there.
    fn settle(&mut self, folder: &OwnedFd, parts: &[&str], last: &str, item: &Item) -> Result<Settled, Fault> {
        let Some(existing) = Destination::existing(folder, last)? else {
            return Ok(Settled::Write { name: last.to_owned(), replace: false });
        };
        let target = self.job.target.join(joined(parts, last));
        let decision = match self.overwrite {
            Some(true) => Decision::Overwrite,
            Some(false) => Decision::Skip,
            None => {
                let source = self.shown(item);
                (self.emit)(Event::Asking(Ask::Conflict {
                    source,
                    target: target.clone(),
                    sizes: (item.size, existing.size),
                    newer: newer(item, existing),
                }));
                self.control.decide()
            }
        };
        Ok(match decision {
            Decision::Cancel => return Err(Fault::Stopped),
            Decision::Skip | Decision::SkipAll => {
                if decision == Decision::SkipAll {
                    self.overwrite = Some(false);
                }
                self.skipped += 1;
                Settled::Skip
            }
            Decision::KeepBoth => {
                let mut failure = None;
                let unique = borg_jobs::unique_by(&target, |candidate| {
                    let name =
                        candidate.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                    Destination::existing(folder, &name)
                        .unwrap_or_else(|error| {
                            failure = Some(error);
                            None
                        })
                        .is_some()
                });
                if let Some(error) = failure {
                    return Err(error.into());
                }
                let name = unique.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                Settled::Write { name, replace: false }
            }
            Decision::OverwriteAll => {
                self.overwrite = Some(true);
                Settled::Write { name: last.to_owned(), replace: true }
            }
            _ => Settled::Write { name: last.to_owned(), replace: true },
        })
    }

    /// A symlink whose target the archive stores in its header, or a hard link to an entry
    /// unpacked before.
    fn link(&mut self, item: &Item) -> Result<(), Fault> {
        self.check()?;
        let (parts, last) = self.parts(item);
        let folder = self.destination.folder(&parts, FOLDER_MODE)?;
        let target = item.link.clone().unwrap_or_default();
        let source = match item.kind {
            ItemKind::HardLink => match self.placed.get(&target) {
                Some(placed) => Some(placed.clone()),
                None => return Err(Fault::Failed(tr!(archive_link_left_out, path = item.path, target = target))),
            },
            _ => None,
        };
        let Settled::Write { name, replace } = self.settle(&folder, &parts, last, item)? else { return Ok(()) };
        match source {
            Some(placed) => {
                let (from_parts, from_name) = split(&placed);
                let from = self.destination.folder(&from_parts, FOLDER_MODE)?;
                if replace {
                    let _ = unlinkat(&folder, name.as_str(), AtFlags::empty());
                }
                rustix::fs::linkat(&from, from_name, &folder, name.as_str(), AtFlags::empty())
                    .map_err(io::Error::from)?;
            }
            None => place_symlink(&folder, &name, &target, replace)?,
        }
        self.placed.insert(item.path.clone(), joined(&parts, &name));
        self.done.files += 1;
        self.progress(item, 0);
        Ok(())
    }

    fn folder_times(&self, item: &Item) -> io::Result<()> {
        let Some(placed) = self.placed.get(&item.path) else { return Ok(()) };
        let parts: Vec<&str> = placed.split('/').collect();
        let folder = std::fs::File::from(self.destination.folder(&parts, FOLDER_MODE)?);
        let modified = item.modified.unwrap_or(UNIX_EPOCH);
        folder.set_times(std::fs::FileTimes::new().set_modified(modified))
    }
}

fn newer(item: &Item, existing: Existing) -> bool {
    let seconds = |time: SystemTime| time.duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64);
    item.modified.is_some_and(|modified| seconds(modified) > existing.modified)
}

fn joined(parts: &[&str], last: &str) -> String {
    parts.iter().copied().chain([last]).collect::<Vec<_>>().join("/")
}

fn split(path: &str) -> (Vec<&str>, &str) {
    let mut parts: Vec<&str> = path.split('/').collect();
    let last = parts.pop().unwrap_or_default();
    (parts, last)
}

fn link_target(item: &Item, reader: &mut dyn Read) -> Result<String, Fault> {
    let mut target = Vec::new();
    reader
        .take(LINK_LIMIT + 1)
        .read_to_end(&mut target)
        .map_err(|error| Fault::Failed(crate::read::read_failure(item, error).to_string()))?;
    if target.len() as u64 > LINK_LIMIT || target.is_empty() {
        return Err(Fault::Failed(tr!(archive_entry_too_long, path = item.path)));
    }
    Ok(String::from_utf8_lossy(&target).into_owned())
}

fn place_symlink(folder: &OwnedFd, name: &str, target: &str, replace: bool) -> io::Result<()> {
    if !replace {
        return symlinkat(target, folder, name).map_err(io::Error::from);
    }
    let partial = format!("{PARTIAL_PREFIX}{name}{PARTIAL_SUFFIX}");
    let _ = unlinkat(folder, partial.as_str(), AtFlags::empty());
    symlinkat(target, folder, partial.as_str())?;
    renameat(folder, partial.as_str(), folder, name).map_err(io::Error::from)
}

fn is_halt(error: &io::Error) -> bool {
    error.get_ref().is_some_and(|inner| inner.is::<crate::error::Halt>())
}
