//! The drives Borg met, shared by its jobs: what each is, how fast it turned out to be, and the
//! answers given about long work, kept in `drives.json` in Borg's state folder.

use std::{
    collections::HashMap,
    fs,
    io::{self, Read},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use crate::{
    Drive, DriveKind, Rates, Remember, Work,
    detect::{detect, existing},
    known::{FORMAT, Known},
};

const FILE: &str = "drives.json";
const STAGING_SUFFIX: &str = "staging";
const MAX_BYTES: u64 = 1024 * 1024;
const REMOTE_PREFIX: &str = "remote:";
/// Work shorter than this, or smaller, says little about a drive's speed, since caches hide it.
const LEARN_SHORTEST: Duration = Duration::from_secs(2);
const LEARN_SMALLEST: u64 = 64 * 1024 * 1024;

/// What Borg knows of the drive a path lies on, as the Info panel shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub drive: Drive,
    pub rates: Rates,
    /// Whether the rates were measured on this drive.
    pub measured: bool,
    /// Whether Borg asks before long work on the path, which an answer about it or about
    /// drives of its kind turns off.
    pub asks: bool,
}

#[derive(Default)]
struct Shared {
    file: Option<PathBuf>,
    known: Mutex<Known>,
    /// Drives already detected, by device number.
    seen: Mutex<HashMap<u64, Drive>>,
    /// The drive every path lies on and its speeds, for tests and previews.
    uniform: Option<(Drive, Rates)>,
}

/// Borg's drives, shared between threads.
#[derive(Clone, Default)]
pub struct Drives(Arc<Shared>);

impl std::fmt::Debug for Drives {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Drives").field("file", &self.0.file).finish_non_exhaustive()
    }
}

impl Drives {
    /// The drives Borg knows from earlier runs, kept in the state folder `state`.
    pub fn load(state: &Path) -> Self {
        let file = state.join(FILE);
        let known = read(&file).unwrap_or_default();
        Self(Arc::new(Shared { file: Some(file), known: Mutex::new(known), ..Shared::default() }))
    }

    /// Every path on `drive`, which moves data at `rates`, with nothing kept between runs, for
    /// tests and previews.
    pub fn uniform(drive: Drive, rates: Rates) -> Self {
        Self(Arc::new(Shared { uniform: Some((drive, rates)), ..Shared::default() }))
    }

    /// The drive of the machine a remote panel shows.
    pub fn remote(host: &str) -> Drive {
        Drive {
            id: format!("{REMOTE_PREFIX}{host}"),
            kind: DriveKind::Remote,
            filesystem: String::new(),
            clones: false,
        }
    }

    /// The drive `path` lies on, or would lie on once written.
    pub fn drive(&self, path: &Path) -> Drive {
        if let Some((drive, _)) = &self.0.uniform {
            return drive.clone();
        }
        let Some(device) = existing(path).and_then(|path| fs::metadata(path).ok()).map(|meta| meta.dev()) else {
            return detect(path);
        };
        if let Some(drive) = lock(&self.0.seen).get(&device) {
            return drive.clone();
        }
        let drive = detect(path);
        lock(&self.0.seen).insert(device, drive.clone());
        drive
    }

    pub fn rates(&self, drive: &Drive) -> Rates {
        self.measured(&self.known(), drive).0
    }

    /// The drive's speeds, and whether Borg measured them.
    fn measured(&self, known: &Known, drive: &Drive) -> (Rates, bool) {
        match &self.0.uniform {
            Some((_, rates)) => (*rates, true),
            None => known.rates(&drive.id, drive.kind),
        }
    }

    pub fn facts(&self, path: &Path) -> Facts {
        let drive = self.drive(path);
        let known = self.known();
        let (rates, measured) = self.measured(&known, &drive);
        let asks = known.asks(Some(path), drive.kind);
        Facts { drive, rates, measured, asks }
    }

    /// How long `work` takes: the time of the busiest drive, then the computing.
    pub fn estimate(&self, work: &Work) -> Duration {
        self.busiest(work).map_or(Duration::ZERO, |(_, _, _, busy)| busy) + work.compute
    }

    /// The drive that holds `work` up the most.
    pub fn slowest(&self, work: &Work) -> Option<Drive> {
        self.busiest(work).map(|(drive, _, _, _)| drive)
    }

    fn busiest(&self, work: &Work) -> Option<(Drive, u64, u64, Duration)> {
        let known = self.known();
        work.by_drive()
            .into_iter()
            .map(|(drive, read, write)| {
                let rates = self.measured(&known, drive).0;
                (drive.clone(), read, write, seconds(read, rates.read) + seconds(write, rates.write))
            })
            .max_by_key(|(_, _, _, busy)| *busy)
    }

    /// Learns from `work` that took `took`: the drive that held it up was as much faster or
    /// slower than expected as the work was. Work that spends more time computing than on its
    /// drives says little about them and teaches nothing.
    pub fn learn(&self, work: &Work, took: Duration) {
        let spent = took.saturating_sub(work.compute);
        let Some((drive, read, write, expected)) = self.busiest(work) else { return };
        if spent < LEARN_SHORTEST || read + write < LEARN_SMALLEST || expected.is_zero() || work.compute > expected {
            return;
        }
        let factor = expected.as_secs_f64() / spent.as_secs_f64();
        let mut known = self.known();
        let rates = self.measured(&known, &drive).0;
        let scaled = |rate: u64, bytes: u64| (bytes > 0).then_some((rate as f64 * factor) as u64);
        known.learn(&drive.id, drive.kind, scaled(rates.read, read), scaled(rates.write, write));
        self.save(&known);
    }

    /// Whether to ask before long work on `file`, on a drive of this kind.
    pub fn asks(&self, file: Option<&Path>, kind: DriveKind) -> bool {
        self.known().asks(file, kind)
    }

    pub fn remember(&self, remember: Remember, file: Option<&Path>, kind: DriveKind) {
        let mut known = self.known();
        known.remember(remember, file, kind);
        self.save(&known);
    }

    /// Forgets every answer, so Borg asks before long work again.
    pub fn ask_again(&self) {
        let mut known = self.known();
        known.quiet = Default::default();
        self.save(&known);
    }

    /// How many answers keep Borg from asking: files and kinds of drive.
    pub fn kept(&self) -> usize {
        let known = self.known();
        known.quiet.files.len() + known.quiet.kinds.len()
    }

    fn known(&self) -> MutexGuard<'_, Known> {
        lock(&self.0.known)
    }

    fn save(&self, known: &Known) {
        let Some(file) = &self.0.file else { return };
        let _ = write(file, known);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn seconds(bytes: u64, rate: u64) -> Duration {
    Duration::from_secs_f64(bytes as f64 / rate.max(1) as f64)
}

fn read(file: &Path) -> Option<Known> {
    let mut bytes = Vec::new();
    fs::File::open(file).ok()?.take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    serde_json::from_slice::<Known>(&bytes).ok().filter(|known| known.format == FORMAT)
}

fn write(file: &Path, known: &Known) -> io::Result<()> {
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder)?;
    }
    let staging = file.with_extension(STAGING_SUFFIX);
    fs::write(&staging, serde_json::to_vec_pretty(known)?)?;
    fs::rename(&staging, file)
}
