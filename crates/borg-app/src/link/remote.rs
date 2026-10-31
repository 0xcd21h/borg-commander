//! The computer's side of borg cloud: pairing with a server, calling it, and copying files to and from it.

use super::{
    APPROVAL_WAIT, Answer, Hello, LINK_DIR, SERVERS_FILE, Server, clean_name, identity, load, save, this_computer,
};
use crate::agent::{Call, Reply};
use anyhow::{Context, Result, bail};
use borg_link::{Channel, PairingCode, PairingLink, PublicKey, Relay, hex, initiate, relay, unhex};
use serde::Deserialize;
use serde_json::json;
use std::{path::Path, time::Duration};

/// The computer's side: pairs with the server a link names. The user approves it in borg on
/// the server, comparing the code `shown` here; then the server is saved by name.
pub async fn pair(state: &Path, link: &str, shown: impl FnOnce(&PairingCode)) -> Result<Server> {
    let link = PairingLink::parse(link).context("that is not a borg pairing link")?;
    let identity = identity(state)?;
    let socket = relay::dial(&link.relay, &link.server).await?;
    let hello = serde_json::to_vec(&Hello::Pair { secret: hex(&link.secret), name: this_computer() })?;
    let mut channel = initiate(socket, &identity, &link.server, &hello).await?;
    shown(&channel.pairing_code());
    let reply = tokio::time::timeout(APPROVAL_WAIT + Duration::from_secs(30), channel.recv())
        .await
        .context("no answer from borg on the server")??
        .context("the server closed the connection")?;
    let answer: Answer = serde_json::from_slice(&reply)?;
    if !answer.ok {
        bail!("{}", answer.error);
    }
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
    let server = Server { name, relay: link.relay.to_string(), key: hex(&link.server) };
    servers.push(server.clone());
    save(&path, &servers)?;
    Ok(server)
}

/// A session with a paired server, for calls.
pub struct Remote {
    pub(super) channel: Channel,
}

impl Remote {
    pub async fn open(state: &Path, server: &Server) -> Result<Self> {
        let relay = Relay::parse(&server.relay).context("the saved relay is not valid")?;
        let key: PublicKey = unhex(&server.key).context("the saved key is not valid")?;
        let identity = identity(state)?;
        let socket =
            relay::dial(&relay, &key).await.with_context(|| format!("borg on {} is not reachable", server.name))?;
        let mut channel = initiate(socket, &identity, &key, &serde_json::to_vec(&Hello::Session)?).await?;
        let reply = channel.recv().await?.context("the server closed the connection")?;
        let answer: Answer = serde_json::from_slice(&reply)?;
        if !answer.ok {
            bail!("{}", answer.error);
        }
        Ok(Self { channel })
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
