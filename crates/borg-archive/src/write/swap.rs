//! Writing the changed archive beside the original and putting it in place: a lock keeps two jobs
//! from changing one archive at once, and a partial file that never finishes is removed.

use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use borg_jobs::Control;

use crate::{Error, meter::Meter};

const PARTIAL_PREFIX: &str = ".";
const PARTIAL_SUFFIX: &str = ".borg-partial";
/// Only the owner reads the archive until it takes the original's place and permissions.
const PRIVATE: u32 = 0o600;
const LOCK_WAIT: Duration = Duration::from_millis(100);
const CHUNK: usize = 1 << 20;

/// Holds the archive for one job until dropped.
pub(crate) struct Locked {
    _file: File,
}

/// Waits until no other job changes the archive at `real`, a path without links.
pub(crate) fn lock(real: &Path, control: &Control) -> Result<Locked, Error> {
    loop {
        let file = File::open(real)?;
        match file.try_lock() {
            Ok(()) if same_file(&file, real)? => return Ok(Locked { _file: file }),
            Ok(()) => continue,
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
        if !control.proceed() {
            return Err(Error::Stopped);
        }
        thread::sleep(LOCK_WAIT);
    }
}

/// Whether `file` is still the one at `path`, which a job that just finished may have replaced.
fn same_file(file: &File, path: &Path) -> io::Result<bool> {
    let (open, named) = (file.metadata()?, fs::metadata(path)?);
    Ok(open.ino() == named.ino() && open.dev() == named.dev())
}

/// The new archive while it is written, removed unless it takes the original's place.
pub(crate) struct Partial {
    path: PathBuf,
    placed: bool,
}

impl Partial {
    fn beside(real: &Path) -> PathBuf {
        let name = real.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        real.with_file_name(format!("{PARTIAL_PREFIX}{name}{PARTIAL_SUFFIX}"))
    }

    fn take(real: &Path) -> Self {
        let path = Self::beside(real);
        let _ = fs::remove_file(&path);
        Self { path, placed: false }
    }

    /// An empty file to write the new archive into.
    pub fn fresh(real: &Path) -> io::Result<(Self, File)> {
        let partial = Self::take(real);
        let file = OpenOptions::new().read(true).write(true).create_new(true).mode(PRIVATE).open(&partial.path)?;
        Ok((partial, file))
    }

    /// A copy of the archive to add entries to. A filesystem that clones files makes it at once;
    /// elsewhere the bytes count toward the job's progress.
    pub fn copy(real: &Path, clones: bool, meter: &Meter<'_, '_>) -> io::Result<(Self, File)> {
        if clones {
            let partial = Self::take(real);
            fs::copy(real, &partial.path)?;
            let file = OpenOptions::new().read(true).write(true).open(&partial.path)?;
            return Ok((partial, file));
        }
        let (partial, mut file) = Self::fresh(real)?;
        let mut source = File::open(real)?;
        let mut buffer = vec![0; CHUNK];
        loop {
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])?;
            meter.advance(read as u64)?;
        }
        Ok((partial, file))
    }

    /// Puts the finished archive in place of the original, with its permissions.
    pub fn place(mut self, file: File, real: &Path) -> io::Result<()> {
        file.sync_all()?;
        drop(file);
        fs::set_permissions(&self.path, fs::metadata(real)?.permissions())?;
        fs::rename(&self.path, real)?;
        self.placed = true;
        Ok(())
    }
}

impl Drop for Partial {
    fn drop(&mut self) {
        if !self.placed {
            let _ = fs::remove_file(&self.path);
        }
    }
}
