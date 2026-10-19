use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use borg_core::{Aside, Msg, Peek, Peeked};
use tokio::{
    sync::{mpsc::UnboundedSender, watch},
    task::JoinHandle,
};

#[derive(Clone)]
struct Request {
    aside: Aside,
    path: PathBuf,
    size: (usize, usize),
    cell: (u16, u16),
    cancelled: Arc<AtomicBool>,
}

pub struct Loader {
    requests: watch::Sender<Option<Request>>,
    cancelled: Option<Arc<AtomicBool>>,
    worker: JoinHandle<()>,
}

impl Loader {
    pub fn new(messages: UnboundedSender<Msg>) -> Self {
        Self::with_load(messages, crate::peek::peek)
    }

    fn with_load<F>(messages: UnboundedSender<Msg>, load: F) -> Self
    where
        F: Fn(Aside, &Path, (usize, usize), (u16, u16), Arc<AtomicBool>) -> Peek + Send + Sync + 'static,
    {
        let (requests, mut incoming) = watch::channel(None::<Request>);
        let load = Arc::new(load);
        let worker = tokio::spawn(async move {
            while incoming.changed().await.is_ok() {
                let Some(request) = incoming.borrow_and_update().clone() else { continue };
                let cancelled = request.cancelled.clone();
                let load = load.clone();
                let loaded = tokio::task::spawn_blocking(move || {
                    let peek = load(request.aside, &request.path, request.size, request.cell, request.cancelled);
                    Peeked { aside: request.aside, path: request.path, peek }
                })
                .await;
                if !cancelled.load(Ordering::Relaxed)
                    && let Ok(peeked) = loaded
                {
                    let _ = messages.send(Msg::Peeked(peeked));
                }
            }
        });
        Self { requests, cancelled: None, worker }
    }

    pub fn request(&mut self, aside: Aside, path: PathBuf, size: (usize, usize), cell: (u16, u16)) {
        self.cancel();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.cancelled = Some(cancelled.clone());
        self.requests.send_replace(Some(Request { aside, path, size, cell, cancelled }));
    }

    pub fn cancel(&mut self) {
        if let Some(cancelled) = self.cancelled.take() {
            cancelled.store(true, Ordering::Relaxed);
            self.requests.send_replace(None);
        }
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.cancel();
        self.worker.abort();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, mpsc};

    use super::*;

    #[tokio::test]
    async fn new_selection_cancels_the_active_load_and_skips_obsolete_requests() {
        let (messages, mut received) = tokio::sync::mpsc::unbounded_channel();
        let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
        let (release, gate) = mpsc::channel();
        let gate = Mutex::new(gate);
        let mut loader = Loader::with_load(messages, move |_, path, _, _, cancelled| {
            started.send(path.to_path_buf()).unwrap();
            if path == Path::new("first") {
                gate.lock().unwrap().recv().unwrap();
                assert!(cancelled.load(Ordering::Relaxed));
            }
            Peek::Unavailable(path.display().to_string())
        });
        loader.request(Aside::QuickView, "first".into(), (40, 20), (8, 16));
        assert_eq!(starts.recv().await.unwrap(), Path::new("first"));
        loader.request(Aside::QuickView, "skipped".into(), (40, 20), (8, 16));
        loader.request(Aside::QuickView, "latest".into(), (40, 20), (8, 16));
        release.send(()).unwrap();
        assert_eq!(starts.recv().await.unwrap(), Path::new("latest"));
        let Msg::Peeked(peeked) = received.recv().await.unwrap() else { panic!("expected a preview") };
        assert_eq!(peeked.path, Path::new("latest"));
        assert!(received.try_recv().is_err());
        assert!(starts.try_recv().is_err());
    }
}
