//! The computer's side of borg cloud: pairing with a server, calling it, and copying files to and from it.

use super::{
    APPROVAL_WAIT, Answer, Hello, LINK_DIR, ROTATE_CALL, SERVERS_FILE, Server, clean_name,
    keys::{ComputerKeys, computer_keys, legacy_key, lock, retire_unused, saved_server, update_server},
    load, save, this_computer,
};
use crate::agent::{Call, Reply};
use anyhow::{Context, Result, bail};
use borg_link::{Channel, Identity, PairingCode, PairingLink, PublicKey, Relay, hex, initiate, relay, unhex};
use serde::Deserialize;
use serde_json::json;
use std::{path::Path, time::Duration};

const ROUNDS: usize = 3;

/// The computer's side: pairs with the server a link names. The user approves it in borg on
/// the server, comparing the code `shown` here; then the server is saved by name.
pub async fn pair(state: &Path, link: &str, shown: impl FnOnce(&PairingCode)) -> Result<Server> {
    let link = PairingLink::parse(link).context("that is not a borg pairing link")?;
    let keys = computer_keys(state)?;
    let identity = &keys.current;
    let socket = relay::dial(&link.relay, &link.server).await?;
    let hello = serde_json::to_vec(&Hello::Pair { secret: hex(&link.secret), name: this_computer() })?;
    let mut channel = initiate(socket, identity, &link.server, &hello).await?;
    shown(&channel.pairing_code());
    let reply = tokio::time::timeout(APPROVAL_WAIT + Duration::from_secs(30), channel.recv())
        .await
        .context("no answer from borg on the server")??
        .context("the server closed the connection")?;
    let answer: Answer = serde_json::from_slice(&reply)?;
    if !answer.ok {
        bail!("{}", answer.error);
    }
    let lock = lock(state)?;
    let path = state.join(LINK_DIR).join(SERVERS_FILE);
    let mut servers: Vec<Server> = load(&path);
    servers.retain(|server| server.key != hex(&link.server));
    let base = clean_name(&answer.name).replace(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_', "-");
    let mut name = base.clone();
    let mut n = 2;
    while servers.iter().any(|server| server.name == name) {
        name = format!("{base}-{n}");
        n += 1;
    }
    let server = Server {
        name,
        relay: link.relay.to_string(),
        key: hex(&link.server),
        computer: Some(hex(identity.public())),
        offered: None,
    };
    servers.push(server.clone());
    save(&path, &servers)?;
    drop(lock);
    retire_unused(state)?;
    Ok(server)
}

/// A session with a paired server, for calls.
pub struct Remote {
    pub(super) channel: Channel,
}

impl Remote {
    pub async fn open(state: &Path, server: &Server) -> Result<Self> {
        Self::open_as(state, server, &Hello::Session).await
    }

    /// A viewer connection, for the asks of a session the panels here show.
    pub async fn open_viewer(state: &Path, server: &Server) -> Result<Self> {
        Self::open_as(state, server, &Hello::Viewer).await
    }

    async fn open_as(state: &Path, server: &Server, hello: &Hello) -> Result<Self> {
        let relay = Relay::parse(&server.relay).context("the saved relay is not valid")?;
        let key: PublicKey = unhex(&server.key).context("the saved key is not valid")?;
        let hello_bytes = serde_json::to_vec(hello)?;
        for round in 1..=ROUNDS {
            let keys = computer_keys(state)?;
            let entry = saved_server(state, &server.key).unwrap_or_else(|| server.clone());
            let mut opened = None;
            let unheld = |public: &Option<String>| public.as_deref().is_none_or(|public| keys.held(public).is_none());
            let legacy = (unheld(&entry.computer) || unheld(&entry.offered)).then(|| legacy_key(state)).flatten();
            for candidate in candidates(&entry, &keys, legacy) {
                if let Some((channel, answer)) = connect(&relay, &key, &candidate, &hello_bytes, &server.name).await? {
                    opened = Some((candidate, channel, answer));
                    break;
                }
            }
            let Some((used, mut channel, answer)) = opened else {
                bail!("{}", borg_i18n::tr!(cloud_key_refused, server = server.name));
            };
            let used_key = hex(used.public());
            if entry.computer.as_deref() != Some(used_key.as_str()) || entry.offered.is_some() {
                update_server(state, &server.key, |saved| {
                    saved.computer = Some(used_key.clone());
                    saved.offered = None;
                })?;
            }
            let rotates = matches!(hello, Hello::Session) && answer.rotation && round < ROUNDS;
            if used == keys.current || !rotates {
                return Ok(Self { channel });
            }
            let new_key = hex(keys.current.public());
            update_server(state, &server.key, |saved| saved.offered = Some(new_key.clone()))?;
            match rotate(&mut channel, &keys.current).await {
                Ok(true) => {
                    update_server(state, &server.key, |saved| {
                        saved.computer = Some(new_key.clone());
                        saved.offered = None;
                    })?;
                    retire_unused(state)?;
                    channel.close().await;
                }
                Ok(false) => {
                    update_server(state, &server.key, |saved| saved.offered = None)?;
                    return Ok(Self { channel });
                }
                Err(_) => {}
            }
        }
        bail!("borg on {} did not settle which key this computer uses", server.name)
    }

    pub async fn call(&mut self, call: &Call) -> Result<Reply> {
        self.channel.send(&serde_json::to_vec(call)?).await?;
        self.receive().await
    }

    /// Next reply from a directory stream already requested on this connection.
    pub async fn receive(&mut self) -> Result<Reply> {
        let reply = self.channel.recv().await?.context("the server closed the connection")?;
        Ok(serde_json::from_slice(&reply)?)
    }
}

fn candidates(entry: &Server, keys: &ComputerKeys, legacy: Option<Identity>) -> Vec<Identity> {
    let held = |public: &str| {
        keys.held(public).cloned().or_else(|| legacy.clone().filter(|legacy| hex(legacy.public()) == public))
    };
    let accepted = match entry.computer.as_deref() {
        Some(public) => held(public),
        None => legacy.clone(),
    };
    let mut list: Vec<Identity> = Vec::new();
    for candidate in
        [entry.offered.as_deref().and_then(held), accepted, Some(keys.current.clone())].into_iter().flatten()
    {
        if !list.contains(&candidate) {
            list.push(candidate);
        }
    }
    list
}

pub(super) async fn connect(
    relay: &Relay,
    server: &PublicKey,
    identity: &Identity,
    hello: &[u8],
    name: &str,
) -> Result<Option<(Channel, Answer)>> {
    let socket = relay::dial(relay, server).await.with_context(|| format!("borg on {name} is not reachable"))?;
    let mut channel = initiate(socket, identity, server, hello).await?;
    let reply = channel.recv().await?.context("the server closed the connection")?;
    let answer: Answer = serde_json::from_slice(&reply)?;
    if answer.ok {
        Ok(Some((channel, answer)))
    } else if answer.refused() {
        channel.close().await;
        Ok(None)
    } else {
        bail!("{}", answer.error)
    }
}

pub(super) async fn rotate(channel: &mut Channel, new: &Identity) -> Result<bool> {
    let proof = channel.rotation_proof(new).context("the new key shares no secret with the server")?;
    let call = Call::new(ROTATE_CALL, json!({ "key": hex(new.public()), "proof": hex(&proof) }));
    channel.send(&serde_json::to_vec(&call)?).await?;
    let reply = channel.recv().await?.context("the server closed the connection")?;
    Ok(serde_json::from_slice::<Reply>(&reply)?.ok)
}

/// Bytes a single call to `get` or `put` carries.
pub const CHUNK: u64 = 1024 * 1024;

#[derive(Deserialize)]
pub(super) struct Piece {
    pub(super) size: u64,
    pub(super) data: String,
}

/// Copies a file from the server to `to` on this computer, piece by piece; its size.
pub async fn download_in_session(
    remote: &mut Remote,
    session: Option<&str>,
    path: &str,
    to: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<u64> {
    use base64::Engine as _;
    use std::io::Write as _;
    let staged = to.with_extension(format!("borg-download-{}", std::process::id()));
    let mut file = std::fs::File::create(&staged).with_context(|| format!("could not write {}", staged.display()))?;
    let mut offset = 0;
    let result = async {
        loop {
            let reply =
                remote.call(&Call::new("get", json!({ "session": session, "path": path, "offset": offset }))).await?;
            if !reply.ok {
                bail!("{}", reply.text);
            }
            let piece: Piece = serde_json::from_str(&reply.text)?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(piece.data)?;
            file.write_all(&bytes)?;
            offset += bytes.len() as u64;
            progress(offset, piece.size);
            if offset >= piece.size || bytes.is_empty() {
                return Ok(offset);
            }
        }
    }
    .await;
    drop(file);
    match result {
        Ok(size) => {
            std::fs::rename(&staged, to)?;
            Ok(size)
        }
        Err(error) => {
            let _ = std::fs::remove_file(&staged);
            Err(error)
        }
    }
}

/// Copies a file from this computer to `path` on the server, after the user approves it
/// there; its size.
pub async fn upload_in_session(
    remote: &mut Remote,
    session: Option<&str>,
    from: &Path,
    path: &str,
    mut progress: impl FnMut(u64, u64),
) -> Result<u64> {
    use base64::Engine as _;
    use std::io::Read as _;
    let mut file = std::fs::File::open(from).with_context(|| format!("could not read {}", from.display()))?;
    let size = file.metadata()?.len();
    let reply = remote.call(&Call::new("put", json!({ "session": session, "path": path, "size": size }))).await?;
    if !reply.ok {
        bail!("{}", reply.text);
    }
    let token = reply.text;
    let mut sent = 0;
    let mut buffer = vec![0; usize::try_from(CHUNK).unwrap_or(1 << 20)];
    loop {
        let read = file.read(&mut buffer)?;
        let done = read == 0;
        let data = base64::engine::general_purpose::STANDARD.encode(&buffer[..read]);
        let args = json!({ "session": session, "token": token, "data": data, "done": done });
        let reply = remote.call(&Call::new("put", args)).await?;
        if !reply.ok {
            bail!("{}", reply.text);
        }
        sent += read as u64;
        progress(sent, size);
        if done {
            return Ok(sent);
        }
    }
}
