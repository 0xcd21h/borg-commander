//! The computers connected through borg cloud right now, counted for every borg on this machine to show.

use super::{CLIENTS_LIMIT, CLIENTS_SOCKET, CLIENTS_TIMEOUT, COMPUTERS_FILE, Computer, LINK_DIR, Shared, load};
use crate::agent::note;
use borg_core::{AgentEvent, Msg, RemoteClient};
use borg_link::{KEY_LEN, fingerprint, unhex};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    net::UnixStream,
    sync::mpsc::UnboundedSender,
};

pub(super) fn client_status(shared: &Shared) -> Vec<RemoteClient> {
    shared
        .computers
        .iter()
        .map(|computer| RemoteClient {
            name: computer.name.clone(),
            fingerprint: unhex::<KEY_LEN>(&computer.key).map(|key| fingerprint(&key)).unwrap_or_default(),
            connections: Some(shared.connections.get(&computer.key).copied().unwrap_or_default()),
        })
        .collect()
}

pub(super) struct ClientConnection {
    pub(super) shared: Arc<Mutex<Shared>>,
    pub(super) clients: Arc<AtomicUsize>,
    pub(super) messages: UnboundedSender<Msg>,
    pub(super) computer: Computer,
}

impl ClientConnection {
    pub(super) fn new(
        shared: Arc<Mutex<Shared>>,
        clients: Arc<AtomicUsize>,
        messages: UnboundedSender<Msg>,
        computer: Computer,
    ) -> Self {
        *shared.lock().expect("link lock").connections.entry(computer.key.clone()).or_default() += 1;
        let count = clients.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = messages.send(Msg::Agent(AgentEvent::Connected { clients: count }));
        note(&messages, &format!("{} connected through borg cloud", computer.name));
        Self { shared, clients, messages, computer }
    }
}

impl Drop for ClientConnection {
    fn drop(&mut self) {
        let mut shared = self.shared.lock().expect("link lock");
        if let Some(count) = shared.connections.get_mut(&self.computer.key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                shared.connections.remove(&self.computer.key);
            }
        }
        drop(shared);
        let count = self.clients.fetch_sub(1, Ordering::SeqCst) - 1;
        let _ = self.messages.send(Msg::Agent(AgentEvent::Connected { clients: count }));
        note(&self.messages, &format!("{} disconnected", self.computer.name));
    }
}

pub async fn paired_clients(state: &Path) -> Vec<RemoteClient> {
    let status = async {
        let stream = UnixStream::connect(state.join(LINK_DIR).join(CLIENTS_SOCKET)).await?;
        let mut line = String::new();
        BufReader::new(stream.take(CLIENTS_LIMIT)).read_line(&mut line).await?;
        Ok::<Vec<RemoteClient>, anyhow::Error>(serde_json::from_str(&line)?)
    };
    if let Ok(Ok(clients)) = tokio::time::timeout(CLIENTS_TIMEOUT, status).await {
        return clients;
    }
    load::<Computer>(&state.join(LINK_DIR).join(COMPUTERS_FILE))
        .into_iter()
        .map(|computer| RemoteClient {
            name: computer.name,
            fingerprint: unhex::<KEY_LEN>(&computer.key).map(|key| fingerprint(&key)).unwrap_or_default(),
            connections: None,
        })
        .collect()
}
