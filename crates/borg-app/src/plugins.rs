use borg_i18n::tr;
use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use borg_core::{Content, Msg, Side, Viewer};
use borg_git::{Output, Tool};
use borg_view::Pager;
use tokio::{
    sync::{mpsc::UnboundedSender, watch},
    task::JoinHandle,
};

#[derive(Clone)]
struct Request {
    generation: u64,
    dir: PathBuf,
}

struct Scanner {
    requests: watch::Sender<Option<Request>>,
    worker: JoinHandle<()>,
}

impl Scanner {
    fn new(side: Side, messages: UnboundedSender<Msg>) -> Self {
        Self::with_inspect(side, messages, |dir| async move { borg_git::inspect(&dir).await })
    }

    fn with_inspect<F, Fut>(side: Side, messages: UnboundedSender<Msg>, inspect: F) -> Self
    where
        F: Fn(PathBuf) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<Option<borg_git::Repository>>> + Send,
    {
        let (requests, mut incoming) = watch::channel(None::<Request>);
        let worker = tokio::spawn(async move {
            while incoming.changed().await.is_ok() {
                loop {
                    let Some(request) = incoming.borrow_and_update().clone() else { break };
                    tokio::select! {
                        biased;
                        changed = incoming.changed() => if changed.is_err() { return; },
                        result = inspect(request.dir.clone()) => {
                            let _ = messages.send(Msg::GitStatus { side, generation: request.generation, dir: request.dir, result: result.map_err(|error| format!("{error:#}")) });
                            break;
                        },
                    }
                }
            }
        });
        Self { requests, worker }
    }
}

impl Drop for Scanner {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

pub struct Plugins {
    scanners: [Scanner; 2],
}

impl Plugins {
    pub fn new(messages: UnboundedSender<Msg>) -> Self {
        Self { scanners: Side::ALL.map(|side| Scanner::new(side, messages.clone())) }
    }

    pub fn scan(&mut self, side: Side, generation: u64, dir: PathBuf) {
        self.scanners[side.index()].requests.send_replace(Some(Request { generation, dir }));
    }

    pub fn cancel(&mut self) {
        for scanner in &self.scanners {
            if scanner.requests.borrow().is_some() {
                scanner.requests.send_replace(None);
            }
        }
    }
}

pub async fn run(dir: PathBuf, tool: Tool, paths: Vec<PathBuf>, size: (usize, usize)) -> Result<Option<Viewer>> {
    match borg_git::run(&dir, tool, &paths).await? {
        Output::IndexUpdated => Ok(None),
        Output::Document { name, text } => tokio::task::spawn_blocking(move || {
            let path = PathBuf::from(name);
            let pager = Pager::for_path(Arc::new(text), &path, size.0, size.1)?;
            Ok(Some(Viewer { path, content: Content::Text(pager) }))
        })
        .await
        .context(tr!(opening_git_output))?,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Cancelled {
        path: PathBuf,
        messages: UnboundedSender<PathBuf>,
    }
    impl Drop for Cancelled {
        fn drop(&mut self) {
            let _ = self.messages.send(self.path.clone());
        }
    }

    #[tokio::test]
    async fn new_requests_and_disabling_drop_active_scans() {
        let (messages, mut received) = tokio::sync::mpsc::unbounded_channel();
        let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
        let (dropped, mut drops) = tokio::sync::mpsc::unbounded_channel();
        let scanner = Scanner::with_inspect(Side::Left, messages, move |path| {
            let started = started.clone();
            let dropped = dropped.clone();
            async move {
                let _guard = Cancelled { path: path.clone(), messages: dropped };
                started.send(path.clone()).unwrap();
                if path != std::path::Path::new("latest") {
                    std::future::pending::<()>().await;
                }
                Ok(None)
            }
        });
        scanner.requests.send_replace(Some(Request { generation: 1, dir: "first".into() }));
        assert_eq!(starts.recv().await.unwrap(), PathBuf::from("first"));
        scanner.requests.send_replace(Some(Request { generation: 2, dir: "skipped".into() }));
        scanner.requests.send_replace(Some(Request { generation: 3, dir: "latest".into() }));
        assert_eq!(drops.recv().await.unwrap(), PathBuf::from("first"));
        assert_eq!(starts.recv().await.unwrap(), PathBuf::from("latest"));
        assert_eq!(drops.recv().await.unwrap(), PathBuf::from("latest"));
        let Msg::GitStatus { generation, dir, .. } = received.recv().await.unwrap() else { panic!("expected status") };
        assert_eq!(generation, 3);
        assert_eq!(dir, PathBuf::from("latest"));
        scanner.requests.send_replace(Some(Request { generation: 4, dir: "cancel".into() }));
        assert_eq!(starts.recv().await.unwrap(), PathBuf::from("cancel"));
        scanner.requests.send_replace(None);
        assert_eq!(drops.recv().await.unwrap(), PathBuf::from("cancel"));
        assert!(received.try_recv().is_err());
    }
}
