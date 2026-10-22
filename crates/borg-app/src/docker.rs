//! Runs the Docker view's requests through the `docker` CLI: list and inspect queries, one
//! at a time, actions alongside them, and the stats stream while the view is open.

use std::time::Duration;

use borg_core::{DockerData, DockerRequest, Msg};
use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle};

/// How long to wait before restarting a stats stream that ended, such as when the engine
/// restarts.
const WATCH_RETRY: Duration = Duration::from_secs(5);

pub struct Docker {
    messages: UnboundedSender<Msg>,
    query: Option<JoinHandle<()>>,
    watch: Option<JoinHandle<()>>,
}

impl Docker {
    pub fn new(messages: UnboundedSender<Msg>) -> Self {
        Self { messages, query: None, watch: None }
    }

    pub fn request(&mut self, id: u64, request: DockerRequest) {
        let messages = self.messages.clone();
        let query = !matches!(request, DockerRequest::Act { .. });
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
    }
}
