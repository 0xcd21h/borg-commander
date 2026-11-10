use super::sealed::{DataKey, KEY_LEN};
use anyhow::{Context, Result};
use borg_link::{hex, unhex};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::Path,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
    time::Instant,
};
use zeroize::Zeroizing;

pub(super) const SOCKET_FILE: &str = "unlocked.sock";
const GET: &str = "get";
const PUT: &str = "put";
const FORGET: &str = "forget";
const DONE: &str = "ok";
const LINE_LIMIT: u64 = 256;
const OWNER_ONLY: u32 = 0o600;
const CLIENT_WAIT: Duration = Duration::from_secs(5);
const REQUEST_WAIT: Duration = Duration::from_secs(1);

fn decode(line: &str) -> Option<DataKey> {
    unhex::<KEY_LEN>(line.trim()).map(Zeroizing::new)
}

pub(super) async fn serve(socket: &Path, key: DataKey, idle: Duration) -> Result<()> {
    std::fs::remove_file(socket).ok();
    let listener = UnixListener::bind(socket).with_context(|| format!("listening on {}", socket.display()))?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(OWNER_ONLY))?;
    let own = rustix::process::getuid().as_raw();
    let mut key = key;
    let mut deadline = Instant::now() + idle;
    loop {
        let Ok(accepted) = tokio::time::timeout_at(deadline, listener.accept()).await else { break };
        let Ok((stream, _)) = accepted else { continue };
        if stream.peer_cred().map(|peer| peer.uid()).ok() != Some(own) {
            continue;
        }
        let (reader, mut writer) = stream.into_split();
        let mut line = Zeroizing::new(String::new());
        let mut reader = tokio::io::BufReader::new(reader.take(LINE_LIMIT));
        let until = deadline.min(Instant::now() + REQUEST_WAIT);
        if !matches!(tokio::time::timeout_at(until, reader.read_line(&mut line)).await, Ok(Ok(_))) {
            continue;
        }
        let request = line.trim();
        let answer = match request.split_once(' ') {
            None if request == GET => Zeroizing::new(hex(key.as_ref())),
            Some((PUT, text)) => match decode(text) {
                Some(new) => {
                    key = new;
                    Zeroizing::new(DONE.to_owned())
                }
                None => continue,
            },
            None if request == FORGET => {
                writer.write_all(format!("{DONE}\n").as_bytes()).await.ok();
                break;
            }
            _ => continue,
        };
        deadline = Instant::now() + idle;
        writer.write_all(format!("{}\n", answer.as_str()).as_bytes()).await.ok();
    }
    std::fs::remove_file(socket).ok();
    Ok(())
}

fn ask(socket: &Path, request: &str) -> Option<Zeroizing<String>> {
    let mut stream = UnixStream::connect(socket).ok()?;
    stream.set_read_timeout(Some(CLIENT_WAIT)).ok()?;
    stream.set_write_timeout(Some(CLIENT_WAIT)).ok()?;
    stream.write_all(format!("{request}\n").as_bytes()).ok()?;
    let mut line = Zeroizing::new(String::new());
    BufReader::new(stream).read_line(&mut line).ok()?;
    (!line.is_empty()).then_some(line)
}

pub(super) fn recall(socket: &Path) -> Option<DataKey> {
    ask(socket, GET).and_then(|line| decode(&line))
}

pub(super) fn put(socket: &Path, key: &DataKey) -> bool {
    let request = Zeroizing::new(format!("{PUT} {}", hex(key.as_ref())));
    ask(socket, &request).is_some_and(|line| line.trim() == DONE)
}

pub(super) fn forget(socket: &Path) -> bool {
    ask(socket, FORGET).is_some_and(|line| line.trim() == DONE)
}

pub(super) fn read_key(line: &str) -> Option<DataKey> {
    decode(line)
}
