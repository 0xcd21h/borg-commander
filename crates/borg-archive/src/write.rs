//! Changing an archive as a job: adding files and folders, removing, renaming and replacing
//! entries. The changed archive is written beside the original and takes its place once it is
//! complete and reads back, so a job that fails or is cancelled leaves the original as it was.
//! Entries the change keeps are copied as they are stored wherever the format allows it, and
//! Borg asks before a change that would take long, by what it knows of the drive.

mod plan;
mod seven;
mod swap;
mod tar;
mod zip;

use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use borg_drives::{Drive, Drives, Work};
use borg_jobs::{Ask, Control, Decision, Event, Outcome, Pace, Totals};

use crate::{
    Archive, Error, Format, Index, Report, Watch,
    cost::{codec, packing, unpacking},
    meter::Meter,
    unpack::guard::free_space,
};

use plan::{Asker, Plan};
use swap::{Partial, lock};

/// How long a change to an archive may take before Borg asks.
pub const REWRITE_PATIENCE: Duration = Duration::from_secs(20);

/// What a job does to an archive. Paths inside the archive are relative to its top, such as
/// `docs/guide.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Files and folders of this machine, copied into the archive folder `into`.
    Add {
        sources: Vec<PathBuf>,
        into: String,
    },
    /// Entries removed, each with everything beneath it.
    Remove {
        paths: Vec<String>,
    },
    /// Entries moved to `to`, each with everything beneath it: into the folder `to` names when
    /// the archive holds one, and otherwise the one entry takes the path `to`.
    Move {
        paths: Vec<String>,
        to: String,
    },
    Folder {
        path: String,
    },
    /// The file at `path` takes the contents of `source`, as when an edited file is saved.
    Replace {
        path: String,
        source: PathBuf,
    },
}

/// Why a change stopped before it wrote anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Halt {
    Stopped,
    Failed(String),
}

/// Whether Borg changes archives of this format. RAR is read through another program, and a
/// single compressed file holds no folder to change.
pub fn writable(format: Format) -> bool {
    matches!(format, Format::Zip | Format::SevenZip | Format::Tar(_))
}

/// The listing of an archive, kept from an earlier read while the file stays the same.
pub type List<'a> = &'a dyn Fn(Archive<'_>, Watch) -> Result<Arc<Index>, Error>;

/// A change to the archive `archive` names.
#[derive(Debug, Clone, Copy)]
pub struct Rewrite<'a> {
    pub archive: Archive<'a>,
    pub change: &'a Change,
}

/// How the changed archive comes about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strategy {
    /// A copy of the archive, which a filesystem that clones files makes at once, with the new
    /// entries after the last one.
    Append,
    /// A new archive with the entries kept and the entries added.
    Rewrite,
}

impl Rewrite<'_> {
    /// Runs the job, reporting to `emit` and asking through `control`. The report keeps the
    /// sources an addition did not take whole.
    pub fn run(
        &self,
        list: List<'_>,
        drives: &Drives,
        control: &Control,
        emit: &mut dyn FnMut(Event),
    ) -> Report<PathBuf> {
        let mut left = Vec::new();
        let outcome = match self.change_archive(list, drives, control, emit, &mut left) {
            Ok(outcome) => outcome,
            Err(_) if control.cancelled() => Outcome::Cancelled { done: Totals::default() },
            Err(Error::Stopped) => Outcome::Cancelled { done: Totals::default() },
            Err(error) => Outcome::Failed(error.to_string()),
        };
        let kept = match self.change {
            Change::Add { sources, .. } => {
                sources.iter().filter(|source| left.iter().any(|left| left.starts_with(source))).cloned().collect()
            }
            _ => Vec::new(),
        };
        Report { outcome, kept }
    }

    fn change_archive(
        &self,
        list: List<'_>,
        drives: &Drives,
        control: &Control,
        emit: &mut dyn FnMut(Event),
        left: &mut Vec<PathBuf>,
    ) -> Result<Outcome, Error> {
        let real = self.archive.path.canonicalize()?;
        let _locked = lock(&real, control)?;
        let index = list(self.archive, &mut |_, _| control.proceed())?;
        if !writable(index.format) {
            return Ok(Outcome::Failed(borg_i18n::tr!(archive_read_only, format = index.format.label())));
        }
        let password = self.password(&index)?;
        let plan = {
            let mut asker = Asker::new(control, emit, self.archive.path);
            match plan::plan(self.change, &index, &mut asker) {
                Ok(plan) => plan,
                Err(Halt::Stopped) => return Ok(Outcome::Cancelled { done: Totals::default() }),
                Err(Halt::Failed(message)) => return Ok(Outcome::Failed(message)),
            }
        };
        left.clone_from(&plan.left);
        if plan.added.is_empty() && plan.appends {
            return Ok(Outcome::Done { done: Totals::default(), skipped: plan.skipped });
        }
        let drive = drives.drive(&real);
        let strategy = strategy(index.format, &plan);
        let touched = match index.format {
            Format::SevenZip => seven::touched(&real, &plan, password)?,
            _ => 0,
        };
        let work = work(drives, &drive, &index, &plan, strategy, touched);
        let pace = Pace { drives, work, path: self.archive.path, file: true, patience: REWRITE_PATIENCE };
        if !pace.ask(control, emit) || !room(&real, &pace.work, &drive, control, emit) {
            return Ok(Outcome::Cancelled { done: Totals::default() });
        }
        let started = Instant::now();
        let copied = if strategy == Strategy::Append && drive.clones { 0 } else { index.bytes };
        let total = Totals { files: (plan.kept.len() + plan.added.len()) as u64, bytes: copied + plan.added_bytes() };
        let meter = Meter::new(control, emit, total, self.archive.path.to_path_buf());
        meter.report(true);
        let (partial, mut file) = write(&real, index.format, strategy, &plan, password, drive.clones, &meter)?;
        verify(&mut file, index.format, password)?;
        partial.place(file, &real)?;
        meter.report(true);
        pace.learn(started);
        Ok(Outcome::Done { done: plan.affected, skipped: plan.skipped })
    }

    /// The password new entries are encrypted with, once it is known to open the archive.
    fn password(&self, index: &Index) -> Result<Option<&str>, Error> {
        match self.archive.password {
            Some(password) => {
                self.archive.check_password(index)?;
                Ok(Some(password))
            }
            None if index.encrypted() => Err(Error::NeedsPassword),
            None => Ok(None),
        }
    }
}

fn strategy(format: Format, plan: &Plan) -> Strategy {
    match format {
        Format::Zip | Format::Tar(None) if plan.appends => Strategy::Append,
        _ => Strategy::Rewrite,
    }
}

/// What the change reads and writes, and the time it spends compressing: `touched` bytes of a
/// 7z archive are unpacked and packed again.
fn work(drives: &Drives, drive: &Drive, index: &Index, plan: &Plan, strategy: Strategy, touched: u64) -> Work {
    let added = plan.added_bytes();
    let copied = if strategy == Strategy::Append && drive.clones { 0 } else { index.bytes };
    let mut work = Work::default().read(drive, copied).write(drive, copied + added);
    if let Some(source) = plan.source() {
        work = work.read(&drives.drive(source), added);
    }
    let codec = codec(index.format);
    let compute = match index.format {
        Format::Tar(Some(_)) => {
            let unpacked = index.totals().bytes;
            unpacking(codec, unpacked) + packing(codec, unpacked + added)
        }
        Format::SevenZip => unpacking(codec, touched) + packing(codec, touched + added),
        _ => packing(codec, added),
    };
    work.computing(compute)
}

/// Asks first when the drive that holds the archive lacks the room for the new one beside it.
fn room(real: &Path, work: &Work, drive: &Drive, control: &Control, emit: &mut dyn FnMut(Event)) -> bool {
    let folder = real.parent().unwrap_or(real);
    let needed = work.writes.iter().filter(|(on, _)| on.id == drive.id).map(|(_, bytes)| *bytes).sum();
    match free_space(folder).filter(|free| *free < needed) {
        Some(free) => {
            emit(Event::Asking(Ask::Room { target: folder.to_path_buf(), needed, free }));
            control.decide() == Decision::Proceed
        }
        None => true,
    }
}

fn write(
    real: &Path,
    format: Format,
    strategy: Strategy,
    plan: &Plan,
    password: Option<&str>,
    clones: bool,
    meter: &Meter<'_, '_>,
) -> Result<(Partial, File), Error> {
    if strategy == Strategy::Append {
        let (partial, file) = Partial::copy(real, clones, meter)?;
        let file = match format {
            Format::Zip => zip::append(file, plan, password, meter)?,
            _ => tar::append(file, plan, meter)?,
        };
        return Ok((partial, file));
    }
    let (partial, file) = Partial::fresh(real)?;
    let file = match format {
        Format::Zip => zip::rewrite(real, plan, password, file, meter)?,
        Format::SevenZip => seven::rewrite(real, plan, password, file, meter)?,
        Format::Tar(compression) => tar::rewrite(real, compression, plan, file, meter)?,
        Format::Rar | Format::Compressed(_) => return Err(Error::Unknown),
    };
    Ok((partial, file))
}

/// Reads back the list of entries of formats that keep one, before the new archive takes the
/// original's place.
fn verify(file: &mut File, format: Format, password: Option<&str>) -> Result<(), Error> {
    match format {
        Format::Zip => zip::verify(file),
        Format::SevenZip => seven::verify(file, password),
        _ => Ok(()),
    }
}
