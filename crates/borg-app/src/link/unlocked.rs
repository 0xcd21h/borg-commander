use super::{
    LINK_DIR, key_agent,
    sealed::{DataKey, KEY_LEN},
};
use anyhow::{Context, Result, bail};
use borg_link::hex;
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub(super) const IDLE: Duration = Duration::from_secs(12 * 60 * 60);
pub(super) const AGENT_FLAG: &str = "--key-agent";
const AGENT_START: Duration = Duration::from_secs(3);
const AGENT_POLL: Duration = Duration::from_millis(20);

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Cache {
    pub keyring: Option<String>,
    pub socket: PathBuf,
}

impl Cache {
    pub fn choose(state: &Path, keyring: bool) -> Self {
        let canonical = std::fs::canonicalize(state).unwrap_or_else(|_| state.to_path_buf());
        let digest = Sha256::digest(canonical.as_os_str().as_encoded_bytes());
        Self {
            keyring: keyring.then(|| format!("borg-commander:{}", hex(&digest[..16]))),
            socket: state.join(LINK_DIR).join(key_agent::SOCKET_FILE),
        }
    }

    pub fn of(state: &Path) -> Self {
        Self::choose(state, keyring::available())
    }

    pub fn remember(&self, key: &DataKey, idle: Duration) -> Result<()> {
        match &self.keyring {
            Some(description) => keyring::remember(description, key, idle),
            None if key_agent::put(&self.socket, key) => Ok(()),
            None => start_agent(&self.socket, key),
        }
    }

    pub fn recall(&self, idle: Duration) -> Option<DataKey> {
        let held = self.keyring.as_deref().and_then(|description| keyring::recall(description, idle));
        held.or_else(|| key_agent::recall(&self.socket))
    }

    pub fn forget(&self) -> bool {
        let in_keyring = self.keyring.as_deref().is_some_and(keyring::forget);
        key_agent::forget(&self.socket) || in_keyring
    }
}

fn start_agent(socket: &Path, key: &DataKey) -> Result<()> {
    let state = socket.parent().and_then(Path::parent).context("the unlock socket has no state folder")?;
    let mut child = Command::new(crate::program::path()?)
        .arg(AGENT_FLAG)
        .arg(state)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .context("starting borg's key agent")?;
    let line = Zeroizing::new(format!("{}\n", hex(key.as_ref())));
    child.stdin.take().context("the key agent has no input")?.write_all(line.as_bytes())?;
    let started = Instant::now();
    while started.elapsed() < AGENT_START {
        if key_agent::recall(socket).is_some_and(|held| *held == **key) {
            return Ok(());
        }
        std::thread::sleep(AGENT_POLL);
    }
    bail!("borg's key agent did not start")
}

pub(super) async fn agent_command(args: &[String]) -> Result<()> {
    let state = PathBuf::from(args.first().context("usage: borg --key-agent STATE")?);
    let mut line = Zeroizing::new(String::new());
    std::io::stdin().read_line(&mut line)?;
    let key = key_agent::read_key(&line).context("the key agent got no key")?;
    rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)?;
    key_agent::serve(&state.join(LINK_DIR).join(key_agent::SOCKET_FILE), key, IDLE).await
}

mod keyring {
    use super::{DataKey, Duration, KEY_LEN, Result};
    use anyhow::anyhow;
    use linux_keyutils::{KeyError, KeyPermissionsBuilder, KeyRing, KeyRingIdentifier, Permission};
    use zeroize::Zeroizing;

    const LINKS: usize = 512;

    fn user() -> Result<KeyRing, KeyError> {
        KeyRing::from_special_id(KeyRingIdentifier::User, true)
    }

    pub fn available() -> bool {
        let Ok(user) = user() else { return false };
        KeyRing::from_special_id(KeyRingIdentifier::Session, false)
            .and_then(|session| session.get_links(LINKS))
            .is_ok_and(|links| links.iter().any(|node| node.as_ring() == Some(user)))
    }

    fn seconds(idle: Duration) -> usize {
        usize::try_from(idle.as_secs()).unwrap_or(usize::MAX)
    }

    pub fn remember(description: &str, key: &DataKey, idle: Duration) -> Result<()> {
        let failed = |error: KeyError| anyhow!("the kernel keyring refused the key: {error:?}");
        let held = user().map_err(failed)?.add_key(description, &key[..]).map_err(failed)?;
        let owner = KeyPermissionsBuilder::builder().posessor(Permission::ALL).user(Permission::ALL).build();
        held.set_perms(owner).map_err(failed)?;
        held.set_timeout(seconds(idle)).map_err(failed)
    }

    pub fn recall(description: &str, idle: Duration) -> Option<DataKey> {
        let held = user().ok()?.search(description).ok()?;
        let bytes = Zeroizing::new(held.read_to_vec().ok()?);
        let key = <[u8; KEY_LEN]>::try_from(bytes.as_slice()).ok().map(Zeroizing::new)?;
        held.set_timeout(seconds(idle)).ok();
        Some(key)
    }

    pub fn forget(description: &str) -> bool {
        user().and_then(|ring| ring.search(description)).and_then(|held| held.invalidate()).is_ok()
    }
}
