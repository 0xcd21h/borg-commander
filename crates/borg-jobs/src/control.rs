use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::Decision;

#[derive(Debug, Default)]
pub struct Control {
    cancelled: AtomicBool,
    paused: AtomicBool,
    answer: Mutex<Option<Decision>>,
    signal: Condvar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stopped;

impl Control {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.wake();
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
        self.wake();
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn answer(&self, decision: Decision) {
        if let Ok(mut answer) = self.answer.lock() {
            *answer = Some(decision);
        }
        self.wake();
    }

    fn wake(&self) {
        let _guard = self.answer.lock();
        self.signal.notify_all();
    }

    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub(crate) fn checkpoint(&self) -> Result<(), Stopped> {
        let mut guard = self.answer.lock().map_err(|_| Stopped)?;
        while self.is_paused() && !self.cancelled() {
            guard = self.signal.wait(guard).map_err(|_| Stopped)?;
        }
        if self.cancelled() { Err(Stopped) } else { Ok(()) }
    }

    pub(crate) fn ask(&self) -> Decision {
        let Ok(mut guard) = self.answer.lock() else { return Decision::Cancel };
        loop {
            if self.cancelled() {
                return Decision::Cancel;
            }
            if let Some(decision) = guard.take() {
                return decision;
            }
            match self.signal.wait(guard) {
                Ok(next) => guard = next,
                Err(_) => return Decision::Cancel,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, thread, time::Duration};

    use super::*;

    const PATIENCE: Duration = Duration::from_millis(30);

    #[test]
    fn answers_and_cancellation_wake_waiting_jobs() {
        let control = Arc::new(Control::default());
        let waiting = Arc::clone(&control);
        let asked = thread::spawn(move || waiting.ask());
        thread::sleep(PATIENCE);
        control.answer(Decision::Skip);
        assert_eq!(asked.join().ok(), Some(Decision::Skip));

        let waiting = Arc::clone(&control);
        let asked = thread::spawn(move || waiting.ask());
        thread::sleep(PATIENCE);
        control.cancel();
        assert_eq!(asked.join().ok(), Some(Decision::Cancel));
        assert_eq!(control.checkpoint(), Err(Stopped));
    }

    #[test]
    fn paused_jobs_wait_until_resumed() {
        let control = Arc::new(Control::default());
        control.set_paused(true);
        let waiting = Arc::clone(&control);
        let resumed = thread::spawn(move || waiting.checkpoint());
        thread::sleep(PATIENCE);
        assert!(!resumed.is_finished());
        control.set_paused(false);
        assert_eq!(resumed.join().ok(), Some(Ok(())));
    }
}
