//! Runs the Docker view's requests through the `docker` CLI: list and inspect queries, one
//! at a time, actions and file work alongside them, the stats stream while the view is
//! open, and folder listings for panels that show a container's filesystem.

use std::{
    io,
    path::PathBuf,
    time::{Duration, UNIX_EPOCH},
};

use borg_core::{DockerData, DockerRequest, FileOp, Msg};
use borg_vfs::{Batches, Entry, EntryKind};
use futures::StreamExt;
use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle};

/// How long to wait before restarting a stats stream that ended, such as when the engine
/// restarts.
const WATCH_RETRY: Duration = Duration::from_secs(5);

pub struct Docker {
    messages: UnboundedSender<Msg>,
    query: Option<JoinHandle<()>>,
    watch: Option<JoinHandle<()>>,
    /// Where container files are copied out for viewing; removed on exit.
    scratch: PathBuf,
}

impl Docker {
    pub fn new(messages: UnboundedSender<Msg>, scratch: PathBuf) -> Self {
        Self { messages, query: None, watch: None, scratch }
    }

    pub fn request(&mut self, id: u64, request: DockerRequest) {
        let messages = self.messages.clone();
        let query = request.is_query();
        let scratch = self.scratch.join(id.to_string());
        let task = tokio::spawn(async move {
            let result = match request {
                DockerRequest::List => {
                    let (context, containers) = tokio::join!(borg_docker::context(), borg_docker::list());
                    containers.map(|containers| DockerData::Listed { context: context.ok(), containers })
                }
                DockerRequest::Inspect(id) => borg_docker::inspect(&id)
                    .await
                    .map(|details| DockerData::Inspected { id, details: Box::new(details) }),
                DockerRequest::Act { action, ids } => borg_docker::act(action, &ids).await.map(|()| DockerData::Done),
                DockerRequest::Files { container, op } => files(&container, op, &scratch).await,
            };
            let _ = messages.send(Msg::Docker { id, result: result.map_err(|error| format!("{error:#}")) });
        });
        // Actions run to the end; a newer query replaces one still running, whose answer
        // the view would ignore anyway.
        if query && let Some(previous) = self.query.replace(task) {
            previous.abort();
        }
    }

    pub fn watch(&mut self, on: bool) {
        if let Some(task) = self.watch.take() {
            task.abort();
        }
        if !on {
            return;
        }
        let messages = self.messages.clone();
        self.watch = Some(tokio::spawn(async move {
            loop {
                let sender = messages.clone();
                let _ = borg_docker::watch(move |frame| {
                    let _ = sender.send(Msg::DockerStats(frame));
                })
                .await;
                tokio::time::sleep(WATCH_RETRY).await;
            }
        }));
    }

    /// Stops the running query once the view no longer waits for it.
    pub fn cancel(&mut self) {
        if let Some(task) = self.query.take() {
            task.abort();
        }
    }
}

impl Drop for Docker {
    fn drop(&mut self) {
        self.cancel();
        self.watch(false);
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

async fn files(container: &str, op: FileOp, scratch: &std::path::Path) -> anyhow::Result<DockerData> {
    match op {
        FileOp::CopyOut { sources, target } => borg_docker::copy_out(container, &sources, &target).await?,
        FileOp::CopyIn { sources, target } => borg_docker::copy_in(container, &sources, &target).await?,
        FileOp::Delete { paths } => borg_docker::remove(container, &paths).await?,
        FileOp::Mkdir { path } => borg_docker::mkdir(container, &path).await?,
        FileOp::Fetch { path } => return borg_docker::fetch(container, &path, scratch).await.map(DockerData::Fetched),
    }
    Ok(DockerData::Done)
}

/// Lists a folder inside a container for a panel, in one batch.
pub fn listing(container: String, dir: PathBuf) -> Batches {
    futures::stream::once(async move {
        let files =
            borg_docker::files(&container, &dir).await.map_err(|error| io::Error::other(format!("{error:#}")))?;
        Ok(files.into_iter().map(|file| entry(&dir, file)).collect())
    })
    .boxed()
}

fn entry(dir: &std::path::Path, file: borg_docker::File) -> Entry {
    const PERMISSION_BITS: u32 = 0o7777;
    let kind = if file.is_dir() {
        EntryKind::Dir
    } else if file.is_link() {
        EntryKind::Symlink { to_dir: file.to_dir }
    } else if file.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    Entry {
        source: Some(dir.join(&file.name)),
        missing: false,
        usage: None,
        kind,
        size: file.size,
        modified: u64::try_from(file.modified).ok().map(|secs| UNIX_EPOCH + Duration::from_secs(secs)),
        executable: file.executable(),
        permissions: file.mode & PERMISSION_BITS,
        name: file.name,
    }
}
