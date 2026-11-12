//! Native panel access to a named session on a paired machine.

use std::{io, path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};
use borg_vfs::{Batches, Entry};
use futures::{StreamExt, stream};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use borg_core::HostUpdate;

use crate::{agent::Call, link, session::StateDir};

const DELETE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// How long a paired host has to download an update and say what it does with it.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(180);
/// How long a host has to come back after it moved. Its guard gives the newer Borg a minute
/// before it goes back to the previous one.
const RETURN_WAIT: Duration = Duration::from_secs(100);
const RETURN_POLL: Duration = Duration::from_secs(2);
const HOST_INFO_TIMEOUT: Duration = Duration::from_secs(5);
/// How a host names a call it does not know.
const UNKNOWN_TOOL: &str = "unknown tool";

pub async fn delete_session(machine: &str, session: &str) -> Result<()> {
    tokio::time::timeout(DELETE_TIMEOUT, async {
        let state = StateDir::locate().context("Borg has no state folder")?;
        let server = link::server(state.path(), machine)?;
        let mut remote = link::Remote::open(state.path(), &server).await?;
        let reply = remote.call(&Call::new("session_delete", json!({"id":session}))).await?;
        if !reply.ok {
            if reply.text.to_ascii_lowercase().contains(UNKNOWN_TOOL) {
                bail!(borg_i18n::tr!(remote_session_delete_upgrade));
            }
            bail!("{}", reply.text);
        }
        Ok(())
    })
    .await
    .context(borg_i18n::tr!(remote_session_delete_timeout))?
}

/// Updates a paired host to the latest Borg, and waits for it to come back on that version.
pub async fn update_host(machine: &str) -> Result<HostUpdate> {
    let state = StateDir::locate().context("Borg has no state folder")?;
    let server = link::server(state.path(), machine)?;
    let (before, updating) = tokio::time::timeout(UPDATE_TIMEOUT, async {
        let mut remote = link::Remote::open(state.path(), &server).await?;
        let before = host_version(&mut remote).await?;
        let reply = remote.call(&Call::new(link::HOST_UPDATE_CALL, json!({}))).await?;
        if !reply.ok {
            return Err(refused(&reply.text));
        }
        Ok((before, serde_json::from_str::<link::HostUpdating>(&reply.text)?))
    })
    .await
    .context(borg_i18n::tr!(remote_host_update_no_answer, machine = machine))??;
    if !updating.moving {
        return Ok(HostUpdate::Current(updating.version));
    }
    let deadline = tokio::time::Instant::now() + RETURN_WAIT;
    let mut away = false;
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(RETURN_POLL).await;
        let seen = tokio::time::timeout(HOST_INFO_TIMEOUT, async {
            let mut remote = link::Remote::open(state.path(), &server).await?;
            host_version(&mut remote).await
        })
        .await;
        let seen = seen.ok().and_then(Result::ok);
        away |= seen.is_none();
        match returned(seen.as_deref(), away, &before, &updating.version) {
            Some(Return::Moved) => return Ok(HostUpdate::Moved(updating.version)),
            Some(Return::Back) => {
                bail!(borg_i18n::tr!(remote_host_update_back, machine = machine, version = updating.version))
            }
            None => {}
        }
    }
    bail!(borg_i18n::tr!(remote_host_update_timeout, machine = machine, version = updating.version))
}

/// How a host that moved to a newer Borg came back.
#[derive(Debug, PartialEq, Eq)]
enum Return {
    Moved,
    /// It went away and came back on the version it ran before.
    Back,
}

fn returned(seen: Option<&str>, away: bool, before: &str, version: &str) -> Option<Return> {
    match seen? {
        seen if seen == version => Some(Return::Moved),
        seen if away && seen == before => Some(Return::Back),
        _ => None,
    }
}

async fn host_version(remote: &mut link::Remote) -> Result<String> {
    let reply = remote.call(&Call::new("host_info", json!({}))).await?;
    if !reply.ok {
        return Err(refused(&reply.text));
    }
    Ok(serde_json::from_str::<HostInfo>(&reply.text)?.version)
}

/// Why a host refused to update, which a host that does not know the calls says by naming
/// them unknown.
fn refused(text: &str) -> anyhow::Error {
    if text.to_ascii_lowercase().contains(UNKNOWN_TOOL) {
        anyhow::anyhow!(borg_i18n::tr!(remote_host_update_upgrade))
    } else {
        anyhow::anyhow!("{text}")
    }
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
    /// The host's background service coordinates it, and it runs in a terminal there.
    #[serde(default)]
    terminal: bool,
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
                    Some(HostSession { id: id.into(), name: name.trim().into(), terminal: false })
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
                info: borg_core::RemoteHostInfo {
                    service: info.service.map(|service| service && !session.terminal),
                    ..info.clone()
                },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_that_moved_is_done_once_it_answers_as_the_new_borg_or_went_back() {
        assert_eq!(returned(Some("0.12.0"), false, "0.11.0", "0.12.0"), Some(Return::Moved));
        assert_eq!(returned(Some("0.11.0"), false, "0.11.0", "0.12.0"), None, "it may not have moved yet");
        assert_eq!(returned(None, true, "0.11.0", "0.12.0"), None);
        assert_eq!(returned(Some("0.11.0"), true, "0.11.0", "0.12.0"), Some(Return::Back));
        assert_eq!(returned(Some("0.12.0"), true, "0.11.0", "0.12.0"), Some(Return::Moved));
    }

    #[test]
    fn a_host_that_knows_no_update_call_is_asked_to_update_itself() {
        let older = refused("unknown tool host_update").to_string();
        assert_eq!(older, borg_i18n::tr!(remote_host_update_upgrade));
        assert_eq!(refused("Borg came from Homebrew").to_string(), "Borg came from Homebrew");
    }
}
