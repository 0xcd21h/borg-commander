//! Native panel access to a named session on a paired machine.

use std::{io, path::PathBuf};

use anyhow::{Context, Result, bail};
use borg_vfs::{Batches, Entry};
use futures::{StreamExt, stream};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{agent::Call, link, session::StateDir};

const DELETE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

pub async fn delete_session(machine: &str, session: &str) -> Result<()> {
    tokio::time::timeout(DELETE_TIMEOUT, async {
        let state = StateDir::locate().context("Borg has no state folder")?;
        let server = link::server(state.path(), machine)?;
        let mut remote = link::Remote::open(state.path(), &server).await?;
        let reply = remote.call(&Call::new("session_delete", json!({"id":session}))).await?;
        if !reply.ok {
            if reply.text.to_ascii_lowercase().contains("unknown tool") {
                bail!(borg_i18n::tr!(remote_session_delete_upgrade));
            }
            bail!("{}", reply.text);
        }
        Ok(())
    })
    .await
    .context(borg_i18n::tr!(remote_session_delete_timeout))?
}

#[derive(Deserialize)]
struct HostInfo {
    version: String,
    os: String,
    arch: String,
    service: bool,
    sessions: Vec<HostSession>,
}

#[derive(Deserialize)]
struct HostSession {
    id: String,
    name: String,
}

async fn host_catalog(state: &std::path::Path, server: link::Server) -> Vec<borg_core::RemoteChoice> {
    let mut info = borg_core::RemoteHostInfo { relay: server.relay.clone(), ..Default::default() };
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut remote = link::Remote::open(state, &server).await?;
        let start = std::time::Instant::now();
        let reply = remote.call(&Call::new("host_info", json!({}))).await?;
        info.latency_ms = Some(start.elapsed().as_millis().min(u64::MAX as u128) as u64);
        if reply.ok {
            let host: HostInfo = serde_json::from_str(&reply.text)?;
            info.version = Some(host.version);
            info.platform = Some(format!("{} / {}", host.os, host.arch));
            info.service = Some(host.service);
            Ok::<_, anyhow::Error>(host.sessions)
        } else {
            // Older hosts remain usable, but do not infer their version from the relay.
            let reply = remote.call(&Call::new("list_sessions", json!({}))).await?;
            if !reply.ok {
                bail!("{}", reply.text);
            }
            Ok(reply
                .text
                .lines()
                .filter_map(|line| {
                    let (id, name) = line.split_once(char::is_whitespace)?;
                    Some(HostSession { id: id.into(), name: name.trim().into() })
                })
                .collect())
        }
    })
    .await;
    match result {
        Ok(Ok(sessions)) if !sessions.is_empty() => sessions
            .into_iter()
            .map(|session| borg_core::RemoteChoice {
                identity: server.key.clone(),
                machine: server.name.clone(),
                session: Some(session.id),
                name: session.name,
                info: info.clone(),
                ..Default::default()
            })
            .collect(),
        other => {
            let name = match other {
                Ok(Ok(_)) => "No sessions".into(),
                Ok(Err(error)) => format!("Unavailable: {error:#}"),
                Err(_) => "Unavailable: connection timed out".into(),
            };
            vec![borg_core::RemoteChoice {
                identity: server.key,
                machine: server.name,
                session: None,
                name,
                info,
                ..Default::default()
            }]
        }
    }
}

/// Probe hosts concurrently and display each result as soon as it arrives.
pub async fn catalog(mut report: impl FnMut(Vec<borg_core::RemoteChoice>, bool)) {
    let Some(state) = StateDir::locate() else {
        report(Vec::new(), true);
        return;
    };
    let servers = link::servers(state.path());
    let count = servers.len();
    if count == 0 {
        report(Vec::new(), true);
        return;
    }
    let mut pending = stream::iter(servers.into_iter().enumerate().map(|(index, server)| {
        let state = state.clone();
        async move { (index, host_catalog(state.path(), server).await) }
    }))
    .buffer_unordered(8);
    let mut hosts = std::collections::BTreeMap::new();
    while let Some((index, choices)) = pending.next().await {
        hosts.insert(index, choices);
        report(hosts.values().flatten().cloned().collect(), hosts.len() == count);
    }
}

#[derive(Debug, Clone)]
pub struct Target {
    pub machine: String,
    pub session: String,
    pub name: String,
}

/// A private local workspace for a viewer. The service only starts registered sessions.
pub fn viewer_id(target: &Target) -> String {
    let digest = Sha256::digest(format!("{}\0{}", target.machine, target.session).as_bytes());
    format!("viewer-{}", borg_link::hex(&digest[..12]))
}

pub async fn resolve(text: &str) -> Result<Target> {
    let (machine, name) = text
        .split_once(':')
        .filter(|(machine, name)| !machine.is_empty() && !name.is_empty())
        .context("use MACHINE:SESSION to open a paired remote panel")?;
    let state = StateDir::locate().context("set HOME or BORG_STATE_DIR")?;
    let server = link::server(state.path(), machine)?;
    let mut remote = link::Remote::open(state.path(), &server).await?;
    let reply = remote.call(&Call::new("list_sessions", json!({}))).await?;
    if !reply.ok {
        bail!("{}", reply.text);
    }
    for line in reply.text.lines() {
        let Some((id, label)) = line.split_once(char::is_whitespace) else { continue };
        if id == name || label.trim() == name {
            return Ok(Target { machine: machine.into(), session: id.into(), name: label.trim().into() });
        }
    }
    bail!("no session named {name} on {machine}")
}

#[derive(Deserialize)]
struct Page {
    entries: Vec<Entry>,
    next: Option<usize>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    streaming: bool,
    #[serde(default)]
    pushed: bool,
}

fn io_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

pub fn listing(machine: String, session: String, dir: PathBuf) -> Batches {
    let initial = (None::<link::Remote>, Some(0_usize), None::<String>, false);
    stream::try_unfold(initial, move |(open, offset, cursor, pushed)| {
        let (machine, session, dir) = (machine.clone(), session.clone(), dir.clone());
        async move {
            let Some(offset) = offset else { return Ok(None) };
            let state = StateDir::locate().ok_or_else(|| io_error("Borg has no state folder"))?;
            let server = link::server(state.path(), &machine).map_err(io_error)?;
            let mut remote = match open {
                Some(remote) => remote,
                None => link::Remote::open(state.path(), &server).await.map_err(io_error)?,
            };
            let reply = if pushed {
                remote.receive().await
            } else {
                remote
                    .call(&Call::new(
                        "fs_list",
                        json!({
                            "session": session, "path": dir, "offset": offset,
                            "stream": true, "push": true, "cursor": cursor, "limit": if offset == 0 { 128 } else { 512 }
                        }),
                    ))
                    .await
            }
            .map_err(io_error)?;
            if !reply.ok {
                return Err(io_error(reply.text));
            }
            let page: Page = serde_json::from_str(&reply.text).map_err(io_error)?;
            let next =
                if page.streaming { page.cursor.as_ref().map(|_| offset + page.entries.len()) } else { page.next };
            Ok(Some((page.entries, (Some(remote), next, page.cursor, page.pushed))))
        }
    })
    .boxed()
}
