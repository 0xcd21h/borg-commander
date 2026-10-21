use std::{
    ffi::OsString,
    fs::{self, File, FileTimes},
    io::{self, Read, Write},
    ops::Add,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    Ask, Control, Decision, Event, Outcome, Progress, Spec, Totals,
    control::Stopped,
    plan::{Item, Kind, Root, scan},
};

const CHUNK: u64 = 4 * 1024 * 1024;
const REPORT_EVERY: Duration = Duration::from_millis(50);
const PARTIAL_PREFIX: &str = ".";
const PARTIAL_SUFFIX: &str = ".borg-partial";
const FIRST_COPY: u32 = 2;
const INTO_ITSELF: &str = "cannot copy a folder into itself";
const BUFFERED_FILE_BYTES: usize = 1024 * 1024;
const BATCH_BYTES: usize = 64 * 1024 * 1024;
const BATCH_FILES: usize = 4096;

struct BufferedFile<'a> {
    item: &'a Item,
    dest: PathBuf,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct Batch<'a> {
    files: Vec<BufferedFile<'a>>,
    bytes: usize,
}

impl Batch<'_> {
    fn full(&self) -> bool {
        self.files.len() >= BATCH_FILES || self.bytes + BUFFERED_FILE_BYTES > BATCH_BYTES
    }
}

#[derive(Debug)]
enum Halt {
    Stopped,
    Failed(String),
}

impl From<Stopped> for Halt {
    fn from(_: Stopped) -> Self {
        Self::Stopped
    }
}

#[derive(Debug)]
enum Fault {
    Io(io::Error),
    Halt(Halt),
}

impl From<io::Error> for Fault {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<Stopped> for Fault {
    fn from(stopped: Stopped) -> Self {
        Self::Halt(stopped.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sticky {
    Ask,
    Always,
    Never,
}

struct Job<'a> {
    control: &'a Control,
    emit: &'a mut dyn FnMut(Event),
    done: Totals,
    total: Totals,
    skipped: u64,
    overwrite: Sticky,
    errors: Sticky,
    current: PathBuf,
    reported: Option<Instant>,
}

pub fn run(spec: &Spec, control: &Control, emit: &mut dyn FnMut(Event)) -> Outcome {
    let mut job = Job {
        control,
        emit,
        done: Totals::default(),
        total: Totals::default(),
        skipped: 0,
        overwrite: Sticky::Ask,
        errors: Sticky::Ask,
        current: PathBuf::new(),
        reported: None,
    };
    let outcome = match job.execute(spec) {
        Ok(()) => Outcome::Done { done: job.done, skipped: job.skipped },
        Err(Halt::Stopped) => Outcome::Cancelled { done: job.done },
        Err(Halt::Failed(message)) => Outcome::Failed(message),
    };
    (job.emit)(Event::Finished(outcome.clone()));
    outcome
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn numbered(path: &Path, n: u32) -> PathBuf {
    let stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    let name = match path.extension() {
        Some(ext) if !stem.is_empty() => format!("{stem} ({n}).{}", ext.to_string_lossy()),
        _ => format!("{} ({n})", path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default()),
    };
    path.with_file_name(name)
}

fn unique(path: &Path) -> PathBuf {
    (FIRST_COPY..).map(|n| numbered(path, n)).find(|candidate| !exists(candidate)).unwrap_or_else(|| path.to_path_buf())
}

fn partial(dest: &Path) -> PathBuf {
    let name = dest.file_name().map(OsString::from).unwrap_or_default();
    let mut hidden = OsString::from(PARTIAL_PREFIX);
    hidden.push(name);
    hidden.push(PARTIAL_SUFFIX);
    dest.with_file_name(hidden)
}

fn finish_metadata(item: &Item, dest: &Path) {
    let _ = fs::set_permissions(dest, fs::Permissions::from_mode(item.mode));
    if let (Some(modified), Ok(file)) = (item.modified, File::open(dest)) {
        let _ = file.set_times(FileTimes::new().set_modified(modified));
    }
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    fs::symlink_metadata(path).and_then(|meta| meta.modified()).ok()
}

impl Job<'_> {
    fn execute(&mut self, spec: &Spec) -> Result<(), Halt> {
        let roots = self.scan(spec.sources())?;
        self.total = roots.iter().map(Root::totals).fold(Totals::default(), Totals::add);
        let single = roots.len() == 1;
        let mut batch = Batch::default();
        for root in &roots {
            match spec {
                Spec::Copy { target, optimize_hdd, .. } => {
                    self.copy_root(root, &destination(root, target, single), *optimize_hdd, &mut batch)?;
                }
                Spec::Move { target, .. } => self.move_root(root, &destination(root, target, single))?,
                Spec::Delete { .. } => self.delete_root(root, true)?,
            }
        }
        self.flush_batch(&mut batch)?;
        self.report(true);
        Ok(())
    }

    fn report(&mut self, force: bool) {
        let due = self.reported.is_none_or(|at| at.elapsed() >= REPORT_EVERY);
        if force || due {
            self.reported = Some(Instant::now());
            (self.emit)(Event::Progress(Progress {
                done: self.done,
                total: self.total,
                current: self.current.clone(),
            }));
        }
    }

    fn scan(&mut self, sources: &[PathBuf]) -> Result<Vec<Root>, Halt> {
        let mut roots = Vec::new();
        let mut before = Totals::default();
        for source in sources {
            let emit = &mut *self.emit;
            let mut last: Option<Instant> = None;
            let mut seen = |found: Totals| {
                if last.is_none_or(|at| at.elapsed() >= REPORT_EVERY) {
                    last = Some(Instant::now());
                    emit(Event::Scanning(before.add(found)));
                }
            };
            match scan(source, &mut seen) {
                Ok(root) => {
                    before = before.add(root.totals());
                    roots.push(root);
                }
                Err(error) => {
                    if !self.forgive(source, &error)? {
                        return Err(Halt::Failed(format!("{}: {error}", source.display())));
                    }
                }
            }
        }
        Ok(roots)
    }

    fn forgive(&mut self, path: &Path, error: &io::Error) -> Result<bool, Halt> {
        if self.errors == Sticky::Always {
            return Ok(true);
        }
        (self.emit)(Event::Asking(Ask::Failure { path: path.to_path_buf(), message: error.to_string() }));
        match self.control.ask() {
            Decision::Skip | Decision::KeepBoth | Decision::Overwrite => Ok(true),
            Decision::SkipAll | Decision::OverwriteAll => {
                self.errors = Sticky::Always;
                Ok(true)
            }
            Decision::Retry => Ok(false),
            Decision::Cancel => Err(Halt::Stopped),
        }
    }

    fn retrying(
        &mut self,
        item: &Item,
        mut operation: impl FnMut(&mut Self) -> Result<(), Fault>,
    ) -> Result<bool, Halt> {
        loop {
            let before = self.done;
            match operation(self) {
                Ok(()) => return Ok(true),
                Err(Fault::Halt(halt)) => return Err(halt),
                Err(Fault::Io(error)) => {
                    self.done = before;
                    let retry = !self.forgive(&item.source, &error)?;
                    if !retry {
                        self.skip(item);
                        return Ok(false);
                    }
                }
            }
        }
    }

    fn skip(&mut self, item: &Item) {
        self.skipped += 1;
        self.done = self.done.add(item.totals());
        self.report(false);
    }

    fn resolve(&mut self, item: &Item, dest: PathBuf) -> Result<Option<PathBuf>, Halt> {
        loop {
            if !exists(&dest) {
                return Ok(Some(dest));
            }
            match self.overwrite {
                Sticky::Always => return Ok(Some(dest)),
                Sticky::Never => return Ok(None),
                Sticky::Ask => {}
            }
            let sizes = (item.totals().bytes, fs::symlink_metadata(&dest).map(|meta| meta.len()).unwrap_or_default());
            let newer = item.modified.zip(modified(&dest)).is_some_and(|(source, target)| source > target);
            (self.emit)(Event::Asking(Ask::Conflict {
                source: item.source.clone(),
                target: dest.clone(),
                sizes,
                newer,
            }));
            match self.control.ask() {
                Decision::Overwrite => return Ok(Some(dest)),
                Decision::OverwriteAll => {
                    self.overwrite = Sticky::Always;
                    return Ok(Some(dest));
                }
                Decision::Skip => return Ok(None),
                Decision::SkipAll => {
                    self.overwrite = Sticky::Never;
                    return Ok(None);
                }
                Decision::KeepBoth => return Ok(Some(unique(&dest))),
                Decision::Retry => {}
                Decision::Cancel => return Err(Halt::Stopped),
            }
        }
    }

    fn copy_root<'a>(
        &mut self,
        root: &'a Root,
        dest_root: &Path,
        optimize_hdd: bool,
        batch: &mut Batch<'a>,
    ) -> Result<bool, Halt> {
        let dest_root = if dest_root == root.source { unique(dest_root) } else { dest_root.to_path_buf() };
        let is_dir = root.items.first().is_some_and(|item| item.kind == Kind::Dir);
        if is_dir && dest_root.starts_with(&root.source) {
            return Err(Halt::Failed(format!("{}: {INTO_ITSELF}", root.source.display())));
        }

        let mut complete = true;
        let mut made = Vec::new();
        let mut renamed: Option<PathBuf> = None;
        for item in &root.items {
            self.control.checkpoint()?;
            let base = renamed.as_deref().unwrap_or(&dest_root);
            let dest =
                if item.relative.as_os_str().is_empty() { base.to_path_buf() } else { base.join(&item.relative) };
            self.current = item.source.clone();
            if optimize_hdd && matches!(item.kind, Kind::File(size) if size <= BUFFERED_FILE_BYTES as u64) {
                if batch.full() {
                    complete &= self.flush_batch(batch)?;
                }
                let mut bytes = None;
                let loaded = self.retrying(item, |job| {
                    bytes = job.read_buffered(item)?;
                    Ok(())
                })?;
                if !loaded {
                    complete = false;
                    continue;
                }
                if let Some(bytes) = bytes {
                    batch.bytes += bytes.len();
                    batch.files.push(BufferedFile { item, dest, bytes });
                    self.current = item.source.clone();
                    self.report(false);
                    continue;
                }
            }
            complete &= self.flush_batch(batch)?;
            self.current = item.source.clone();
            let copied = match &item.kind {
                Kind::Dir => self.make_dir(item, &dest, &mut made, &mut renamed)?,
                Kind::File(_) => match self.resolve(item, dest)? {
                    Some(dest) => self.retrying(item, |job| job.copy_file(item, &dest))?,
                    None => {
                        self.skip(item);
                        false
                    }
                },
                Kind::Link(link) => match self.resolve(item, dest)? {
                    Some(dest) => self.retrying(item, |job| job.copy_link(item, link, &dest))?,
                    None => {
                        self.skip(item);
                        false
                    }
                },
            };
            complete &= copied;
        }
        if is_dir {
            complete &= self.flush_batch(batch)?;
        }
        made.iter().rev().for_each(|(item, dest): &(Item, PathBuf)| finish_metadata(item, dest));
        Ok(complete)
    }

    fn read_buffered(&self, item: &Item) -> Result<Option<Vec<u8>>, Fault> {
        self.control.checkpoint()?;
        let mut source = File::open(&item.source)?.take((BUFFERED_FILE_BYTES + 1) as u64);
        let mut bytes = Vec::new();
        source.read_to_end(&mut bytes)?;
        self.control.checkpoint()?;
        Ok((bytes.len() <= BUFFERED_FILE_BYTES).then_some(bytes))
    }

    fn flush_batch(&mut self, batch: &mut Batch<'_>) -> Result<bool, Halt> {
        let mut complete = true;
        for file in batch.files.drain(..) {
            self.control.checkpoint()?;
            self.current = file.item.source.clone();
            match self.resolve(file.item, file.dest)? {
                Some(dest) => {
                    complete &= self.retrying(file.item, |job| job.copy_data(file.item, &dest, Some(&file.bytes)))?
                }
                None => {
                    self.skip(file.item);
                    complete = false;
                }
            }
        }
        batch.bytes = 0;
        Ok(complete)
    }

    fn make_dir(
        &mut self,
        item: &Item,
        dest: &Path,
        made: &mut Vec<(Item, PathBuf)>,
        renamed: &mut Option<PathBuf>,
    ) -> Result<bool, Halt> {
        let dest = match fs::metadata(dest) {
            Ok(meta) if meta.is_dir() => dest.to_path_buf(),
            Ok(_) => match self.resolve(item, dest.to_path_buf())? {
                Some(target) if target != dest => {
                    if item.relative.as_os_str().is_empty() {
                        *renamed = Some(target.clone());
                    }
                    target
                }
                Some(target) => {
                    let removed = self.retrying(item, |_| fs::remove_file(&target).map_err(Fault::Io))?;
                    if !removed {
                        return Ok(false);
                    }
                    target
                }
                None => {
                    self.skip(item);
                    return Ok(false);
                }
            },
            Err(_) => dest.to_path_buf(),
        };
        let created = self.retrying(item, |_| match fs::create_dir(&dest) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            other => other.map_err(Fault::Io),
        })?;
        if created {
            made.push((item.clone(), dest));
        }
        Ok(created)
    }

    fn copy_file(&mut self, item: &Item, dest: &Path) -> Result<(), Fault> {
        self.copy_data(item, dest, None)
    }

    fn copy_data(&mut self, item: &Item, dest: &Path, bytes: Option<&[u8]>) -> Result<(), Fault> {
        let temporary = partial(dest);
        let copied = match bytes {
            Some(bytes) => self.write_buffered(bytes, &temporary),
            None => self.stream(item, &temporary),
        };
        let result = copied.and_then(|()| {
            finish_metadata(item, &temporary);
            fs::rename(&temporary, dest).map_err(Fault::Io)
        });
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
        self.done.files += 1;
        self.report(false);
        Ok(())
    }

    fn write_buffered(&mut self, bytes: &[u8], temporary: &Path) -> Result<(), Fault> {
        let mut out = File::create(temporary)?;
        for chunk in bytes.chunks(CHUNK as usize) {
            self.control.checkpoint()?;
            out.write_all(chunk)?;
            self.done.bytes += chunk.len() as u64;
            self.report(false);
        }
        Ok(())
    }

    fn stream(&mut self, item: &Item, temporary: &Path) -> Result<(), Fault> {
        let mut source = File::open(&item.source)?;
        let mut out = File::create(temporary)?;
        loop {
            self.control.checkpoint()?;
            let copied = io::copy(&mut (&mut source).take(CHUNK), &mut out)?;
            if copied == 0 {
                return Ok(());
            }
            self.done.bytes += copied;
            self.report(false);
        }
    }

    fn copy_link(&mut self, item: &Item, link: &Path, dest: &Path) -> Result<(), Fault> {
        if fs::symlink_metadata(dest).is_ok_and(|meta| !meta.is_dir()) {
            fs::remove_file(dest)?;
        }
        symlink(link, dest)?;
        self.done = self.done.add(item.totals());
        self.report(false);
        Ok(())
    }

    fn move_root(&mut self, root: &Root, dest: &Path) -> Result<(), Halt> {
        if dest == root.source {
            self.done = self.done.add(root.totals());
            return Ok(());
        }
        let Some(first) = root.items.first() else { return Ok(()) };
        let same_device = dest
            .parent()
            .and_then(|parent| fs::metadata(parent).ok())
            .is_some_and(|meta| std::os::unix::fs::MetadataExt::dev(&meta) == first.device);
        if same_device && !exists(dest) {
            self.current = root.source.clone();
            let moved = self.retrying(first, |_| fs::rename(&root.source, dest).map_err(Fault::Io))?;
            if moved {
                self.done = self.done.add(root.totals());
                self.report(false);
            }
            return Ok(());
        }
        if self.copy_root(root, dest, false, &mut Batch::default())? {
            self.delete_root(root, false)?;
        }
        Ok(())
    }

    fn delete_root(&mut self, root: &Root, count: bool) -> Result<(), Halt> {
        for item in root.items.iter().rev() {
            self.control.checkpoint()?;
            self.current = item.source.clone();
            let removed = self.retrying(item, |_| {
                match item.kind {
                    Kind::Dir => fs::remove_dir(&item.source),
                    Kind::File(_) | Kind::Link(_) => fs::remove_file(&item.source),
                }
                .map_err(Fault::Io)
            })?;
            if removed && count {
                self.done = self.done.add(item.totals());
                self.report(false);
            }
        }
        Ok(())
    }
}

fn destination(root: &Root, target: &Path, single: bool) -> PathBuf {
    let name = root.source.file_name().map(PathBuf::from).unwrap_or_default();
    match fs::metadata(target) {
        Ok(meta) if meta.is_dir() => target.join(name),
        _ if single => target.to_path_buf(),
        _ => target.join(name),
    }
}
