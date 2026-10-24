//! The encrypted channel: a Noise IK handshake over a socket the relay joined, then messages
//! of any size up to a limit, split into Noise messages and joined again.

use anyhow::{Context, Result, bail, ensure};
use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

use crate::key::{Identity, KEY_LEN, PublicKey};

pub(crate) const NOISE: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";
/// Binds every handshake to this protocol, so a message from another one cannot pass for it.
const PROLOGUE: &[u8] = b"borg-link 1";
const NOISE_LIMIT: usize = 65_535;
const TAG: usize = 16;
/// Payload bytes per Noise message: what is left after the tag and the byte that says
/// whether more of the message follows.
const CHUNK: usize = NOISE_LIMIT - TAG - 1;
const MORE: u8 = 1;
const LAST: u8 = 0;
/// The largest message a channel carries.
pub const MESSAGE_LIMIT: usize = 16 * 1024 * 1024;

pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct Channel {
    socket: Socket,
    noise: snow::TransportState,
    peer: PublicKey,
}

impl Channel {
    /// The key of the borg at the other end, proven by the handshake.
    pub fn peer(&self) -> &PublicKey {
        &self.peer
    }

    pub async fn send(&mut self, message: &[u8]) -> Result<()> {
        ensure!(message.len() <= MESSAGE_LIMIT, "message too large");
        let chunks: Vec<&[u8]> = if message.is_empty() { vec![&[]] } else { message.chunks(CHUNK).collect() };
        let last = chunks.len() - 1;
        let mut sealed = vec![0; NOISE_LIMIT];
        for (index, chunk) in chunks.into_iter().enumerate() {
            let mut plain = Vec::with_capacity(chunk.len() + 1);
            plain.push(if index == last { LAST } else { MORE });
            plain.extend_from_slice(chunk);
            let size = self.noise.write_message(&plain, &mut sealed)?;
            self.socket.feed(Message::Binary(sealed[..size].to_vec().into())).await?;
        }
        self.socket.flush().await?;
        Ok(())
    }

    /// The next whole message, or `None` once the other end closed.
    pub async fn recv(&mut self) -> Result<Option<Vec<u8>>> {
        let mut message = Vec::new();
        let mut opened = vec![0; NOISE_LIMIT];
        loop {
            let Some(sealed) = next_binary(&mut self.socket).await? else { return Ok(None) };
            let size = self.noise.read_message(&sealed, &mut opened).context("a message failed to decrypt")?;
            let (&flag, chunk) = opened[..size].split_first().context("an empty message")?;
            message.extend_from_slice(chunk);
            ensure!(message.len() <= MESSAGE_LIMIT, "message too large");
            match flag {
                LAST => return Ok(Some(message)),
                MORE => {}
                _ => bail!("a malformed message"),
            }
        }
    }

    pub async fn close(mut self) {
        let _ = self.socket.close(None).await;
    }
}

/// The next binary frame, skipping pings and pongs; `None` once the socket closed.
pub(crate) async fn next_binary(socket: &mut Socket) -> Result<Option<Vec<u8>>> {
    loop {
        match socket.next().await {
            Some(Ok(Message::Binary(bytes))) => return Ok(Some(bytes.to_vec())),
            Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
            Some(Ok(Message::Text(_))) => bail!("unexpected text from the relay"),
            Some(Ok(Message::Close(_))) | None => return Ok(None),
            Some(Err(error)) => return Err(error.into()),
        }
    }
}

fn builder(identity: &Identity) -> Result<snow::Builder<'_>> {
    Ok(snow::Builder::new(NOISE.parse()?).local_private_key(identity.private())?.prologue(PROLOGUE)?)
}

/// Opens a channel to the server whose key is `server`, over a socket the relay joined to
/// it. `hello` travels encrypted in the first message. Only the holder of `server`'s private
/// key can complete the handshake, so a relay cannot answer in the server's name.
pub async fn initiate(mut socket: Socket, identity: &Identity, server: &PublicKey, hello: &[u8]) -> Result<Channel> {
    let mut handshake = builder(identity)?.remote_public_key(server)?.build_initiator()?;
    let mut buffer = vec![0; NOISE_LIMIT];
    let size = handshake.write_message(hello, &mut buffer)?;
    socket.send(Message::Binary(buffer[..size].to_vec().into())).await?;
    let answer = next_binary(&mut socket).await?.context("the server closed the connection")?;
    handshake.read_message(&answer, &mut buffer).context("the server's answer failed to decrypt")?;
    let noise = handshake.into_transport_mode()?;
    Ok(Channel { socket, noise, peer: *server })
}

/// Answers a channel a client opened over a socket the relay joined: the channel, with the
/// client's key proven, and the client's hello.
pub async fn respond(mut socket: Socket, identity: &Identity) -> Result<(Channel, Vec<u8>)> {
    let mut handshake = builder(identity)?.build_responder()?;
    let mut buffer = vec![0; NOISE_LIMIT];
    let first = next_binary(&mut socket).await?.context("the client closed the connection")?;
    let size = handshake.read_message(&first, &mut buffer).context("the client's hello failed to decrypt")?;
    let hello = buffer[..size].to_vec();
    let peer: PublicKey = handshake
        .get_remote_static()
        .and_then(|key| <[u8; KEY_LEN]>::try_from(key).ok())
        .context("the client sent no key")?;
    let size = handshake.write_message(&[], &mut buffer)?;
    socket.send(Message::Binary(buffer[..size].to_vec().into())).await?;
    let noise = handshake.into_transport_mode()?;
    Ok((Channel { socket, noise, peer }, hello))
}
