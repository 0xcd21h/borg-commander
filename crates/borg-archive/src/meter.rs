//! Progress of a job that writes an archive: bytes go through readers that count them, report
//! how far the job got, and stop it when it is cancelled or wait while it is paused.

use std::{
    cell::{Cell, RefCell},
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
    time::{Duration, Instant},
};

use borg_jobs::{Control, Event, Progress, Totals};

use crate::error::halted;

const REPORT_EVERY: Duration = Duration::from_millis(100);

/// How far a job got, out of what it has to get through.
pub(crate) struct Meter<'a, 'e> {
    control: &'a Control,
    emit: RefCell<&'e mut dyn FnMut(Event)>,
    done: Cell<Totals>,
    total: Totals,
    current: PathBuf,
    reported: Cell<Option<Instant>>,
}

impl<'a, 'e> Meter<'a, 'e> {
    pub fn new(control: &'a Control, emit: &'e mut dyn FnMut(Event), total: Totals, current: PathBuf) -> Self {
        Self {
            control,
            emit: RefCell::new(emit),
            done: Cell::new(Totals::default()),
            total,
            current,
            reported: Cell::new(None),
        }
    }

    pub fn emit(&self, event: Event) {
        (self.emit.borrow_mut())(event);
    }

    /// Counts `bytes` more, failing once the job is cancelled.
    pub fn advance(&self, bytes: u64) -> io::Result<()> {
        let done = self.done.get();
        self.done.set(Totals { bytes: done.bytes + bytes, ..done });
        self.report(false);
        if self.control.proceed() { Ok(()) } else { Err(halted()) }
    }

    /// Counts one entry done.
    pub fn entry(&self) {
        let done = self.done.get();
        self.done.set(Totals { files: done.files + 1, ..done });
    }

    pub fn report(&self, force: bool) {
        let due = self.reported.get().is_none_or(|at| at.elapsed() >= REPORT_EVERY);
        if !force && !due {
            return;
        }
        self.reported.set(Some(Instant::now()));
        let progress = Progress { done: self.done.get(), total: self.total, current: self.current.clone() };
        self.emit(Event::Progress(progress));
    }
}

/// A reader whose bytes count toward a job's progress.
pub(crate) struct Counted<'m, 'a, 'e, R> {
    pub inner: R,
    pub meter: &'m Meter<'a, 'e>,
}

impl<R: Read> Read for Counted<'_, '_, '_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.meter.advance(read as u64)?;
        Ok(read)
    }
}

impl<R: Seek> Seek for Counted<'_, '_, '_, R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}
