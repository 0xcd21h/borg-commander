//! How borgs talk to a relay. A server registers to be found by its key, proving it holds
//! that key; a client asks for a server by key; the relay tells the server, which opens a
//! socket for that client, and the relay joins the two. What the two then send is encrypted
//! end to end, so the relay only forwards it.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::{Message, protocol::frame::coding::CloseCode};

use crate::{
    channel::Socket,
    key::{Identity, PublicKey, hex},
    link::Relay,
};

pub const LISTEN: &str = "/v1/listen/";
pub const CONNECT: &str = "/v1/connect/";
pub const ACCEPT: &str = "/v1/accept/";
/// The relay announces a client to a server as this, followed by the session's id.
pub const SESSION: &str = "session ";
pub const READY: &str = "ready";
/// The server a client asks for is not registered.
pub const OFFLINE: u16 = 4404;
/// A limit the relay keeps was reached.
pub const LIMITED: u16 = 4429;
/// The relay's proof that a server holds its key: a Noise NK handshake in which the relay
/// knows the key, so only its holder can finish it.
const PROOF: &str = "Noise_NK_25519_ChaChaPoly_BLAKE2s";
const PROOF_PROLOGUE: &[u8] = b"borg-relay proof 1";
const PROOF_LIMIT: usize = 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const PING_EVERY: Duration = Duration::from_secs(30);
const REGISTRATION_SILENCE: Duration = Duration::from_secs(90);

/// The relay's half of the proof: a challenge only the holder of `key` can answer.
pub fn challenge(key: &PublicKey) -> Result<(snow::HandshakeState, Vec<u8>)> {
    let mut handshake =
        snow::Builder::new(PROOF.parse()?).remote_public_key(key)?.prologue(PROOF_PROLOGUE)?.build_initiator()?;
    let mut message = vec![0; PROOF_LIMIT];
    let size = handshake.write_message(&[], &mut message)?;
    message.truncate(size);
    Ok((handshake, message))
}

/// Whether `answer` finishes the handshake `challenge` began: the server holds its key.
pub fn verify(mut handshake: snow::HandshakeState, answer: &[u8]) -> bool {
    let mut payload = vec![0; PROOF_LIMIT];
    handshake.read_message(answer, &mut payload).is_ok() && handshake.is_handshake_finished()
}

/// The server's half: its answer to the relay's challenge.
fn answer(identity: &Identity, challenge: &[u8]) -> Result<Vec<u8>> {
    let mut handshake = snow::Builder::new(PROOF.parse()?)
        .local_private_key(identity.private())?
        .prologue(PROOF_PROLOGUE)?
        .build_responder()?;
    let mut buffer = vec![0; PROOF_LIMIT];
    handshake.read_message(challenge, &mut buffer)?;
    let size = handshake.write_message(&[], &mut buffer)?;
    buffer.truncate(size);
    Ok(buffer)
}

async fn open(url: &str) -> Result<Socket> {
    let (socket, _) =
        tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async_with_config(url, None, true))
            .await
            .context("the relay did not answer in time")?
            .with_context(|| format!("could not reach the relay at {url}"))?;
    Ok(socket)
}

/// Why the relay closed a socket, in words for the user.
fn closed(code: Option<CloseCode>) -> &'static str {
    match code.map(u16::from) {
        Some(OFFLINE) => "that borg is not open, or not connected to this relay",
        Some(LIMITED) => "the relay is busy or a limit was reached; try again later",
        _ => "the relay closed the connection",
    }
}

/// A server's registration at a relay: once the relay accepts its proof, it hears of every
/// client that asks for it.
pub struct Listener {
    socket: Socket,
    heard: tokio::time::Instant,
    ping_due: tokio::time::Instant,
}

impl Listener {
    pub async fn register(relay: &Relay, identity: &Identity) -> Result<Self> {
        let mut socket = open(&relay.url(&format!("{LISTEN}{}", hex(identity.public())))).await?;
        register_socket(&mut socket, identity).await?;
        let heard = tokio::time::Instant::now();
        Ok(Self { socket, heard, ping_due: heard + PING_EVERY })
    }

    /// The id of the next session a client opens; `None` once the relay closed.
    pub async fn next(&mut self) -> Result<Option<String>> {
        next_session(&mut self.socket, &mut self.heard, &mut self.ping_due).await
    }
}

async fn register_socket<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>, identity: &Identity) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    tokio::time::timeout(CONNECT_TIMEOUT, async {
        let challenge = loop {
            match socket.next().await {
                Some(Ok(Message::Binary(challenge))) => break challenge,
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(Message::Close(frame))) => bail!(closed(frame.map(|frame| frame.code))),
                Some(Err(error)) => return Err(error.into()),
                _ => bail!("the relay closed the registration before sending its challenge"),
            }
        };
        socket.send(Message::Binary(answer(identity, &challenge)?.into())).await?;
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) if text.as_str() == READY => return Ok(()),
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(Message::Close(frame))) => bail!(closed(frame.map(|frame| frame.code))),
                Some(Err(error)) => return Err(error.into()),
                _ => bail!("the relay did not accept the registration"),
            }
        }
    })
    .await
    .context("the relay did not complete registration in time")?
}

async fn next_session<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
    heard: &mut tokio::time::Instant,
    ping_due: &mut tokio::time::Instant,
) -> Result<Option<String>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        let deadline = *heard + REGISTRATION_SILENCE;
        let message = match tokio::time::timeout_at(deadline.min(*ping_due), socket.next()).await {
            Ok(message) => message,
            Err(_) if tokio::time::Instant::now() >= deadline => {
                bail!("the relay stopped sending registration heartbeats")
            }
            Err(_) => {
                *ping_due = tokio::time::Instant::now() + PING_EVERY;
                tokio::time::timeout_at(deadline, socket.send(Message::Ping(Vec::new().into())))
                    .await
                    .context("the relay stopped accepting registration heartbeats")??;
                continue;
            }
        };
        match message {
            Some(Ok(Message::Close(_))) | None => return Ok(None),
            Some(Err(error)) => return Err(error.into()),
            Some(Ok(message)) => {
                *heard = tokio::time::Instant::now();
                if let Message::Text(text) = message
                    && let Some(id) = text.as_str().strip_prefix(SESSION)
                {
                    return Ok(Some(id.to_owned()));
                }
            }
        }
    }
}

/// The socket for a session the relay announced, joined to its client.
pub async fn take(relay: &Relay, session: &str) -> Result<Socket> {
    open(&relay.url(&format!("{ACCEPT}{session}"))).await
}

/// A socket joined to the server whose key is `server`, through the relay.
pub async fn dial(relay: &Relay, server: &PublicKey) -> Result<Socket> {
    let mut socket = open(&relay.url(&format!("{CONNECT}{}", hex(server)))).await?;
    // The relay says the server is ready to answer, or closes with the reason it is not.
    match socket.next().await {
        Some(Ok(Message::Text(text))) if text.as_str() == READY => Ok(socket),
        Some(Ok(Message::Close(frame))) => bail!(closed(frame.map(|frame| frame.code))),
        _ => bail!("the relay closed the connection"),
    }
}

#[cfg(test)]
mod tests;
