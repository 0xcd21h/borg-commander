use super::{
    KeyLocked, LINK_DIR,
    keys::{COMPUTER_KEY_FILE, load_computer_keys, read_private, sealed_here, write_private},
    notify_locked,
    sealed::{Cost, DataKey, SEALED_FILE, Sealed, agent_challenge},
    ssh_agent::{Agent, AgentKey},
    unlocked::{Cache, IDLE},
    words::suggestion,
};
use anyhow::{Context, Result, anyhow, bail};
use borg_i18n::tr;
use std::{
    collections::HashMap,
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use zeroize::Zeroizing;

const TRIES: usize = 3;
const ED25519: &str = "ssh-ed25519";
const ALLOW_AGENT: &str = "--ssh-agent";
const FORBID_AGENT: &str = "--no-ssh-agent";

static TERMINAL: AtomicBool = AtomicBool::new(false);

fn held() -> &'static Mutex<HashMap<PathBuf, DataKey>> {
    static HELD: std::sync::OnceLock<Mutex<HashMap<PathBuf, DataKey>>> = std::sync::OnceLock::new();
    HELD.get_or_init(Default::default)
}

pub(super) fn allow_terminal() {
    TERMINAL.store(true, Ordering::Relaxed);
}

fn sealed_path(state: &Path) -> PathBuf {
    state.join(LINK_DIR).join(SEALED_FILE)
}

fn plain_path(state: &Path) -> PathBuf {
    state.join(LINK_DIR).join(COMPUTER_KEY_FILE)
}

fn load(state: &Path) -> Result<Option<Sealed>> {
    read_private(&sealed_path(state))?.map(|text| Sealed::from_text(&text)).transpose()
}

fn save(state: &Path, sealed: &Sealed) -> Result<()> {
    write_private(&sealed_path(state), &sealed.to_text()?)
}

fn held_key(state: &Path) -> Option<DataKey> {
    Cache::of(state).recall(IDLE).or_else(|| held().lock().expect("held keys").get(state).cloned())
}

fn keep(state: &Path, key: &DataKey) {
    if Cache::of(state).remember(key, IDLE).is_err() {
        held().lock().expect("held keys").insert(state.to_path_buf(), key.clone());
    }
}

pub(super) fn still_unlocked(state: &Path) -> bool {
    held_key(state).is_some()
}

const INTERRUPT: u8 = 0x03;
const END_OF_INPUT: u8 = 0x04;
const BACKSPACE: [u8; 2] = [0x08, 0x7f];
const ERASE_LINE: u8 = 0x15;
const LINE_ENDS: [u8; 2] = [b'\r', b'\n'];
const UTF8_CONTINUATION: std::ops::RangeInclusive<u8> = 0x80..=0xbf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Typed {
    More,
    Done,
    Stopped,
}

fn type_key(line: &mut Vec<u8>, key: u8) -> Typed {
    match key {
        INTERRUPT => return Typed::Stopped,
        END_OF_INPUT if line.is_empty() => return Typed::Stopped,
        END_OF_INPUT => return Typed::Done,
        key if LINE_ENDS.contains(&key) => return Typed::Done,
        key if BACKSPACE.contains(&key) => while line.pop().is_some_and(|byte| UTF8_CONTINUATION.contains(&byte)) {},
        ERASE_LINE => line.clear(),
        key => line.push(key),
    }
    Typed::More
}

fn typed_line(keys: impl Iterator<Item = std::io::Result<u8>>) -> Result<Zeroizing<String>> {
    let mut line = Zeroizing::new(Vec::new());
    for key in keys {
        match type_key(&mut line, key?) {
            Typed::More => {}
            Typed::Done => return Ok(Zeroizing::new(String::from_utf8_lossy(&line).into_owned())),
            Typed::Stopped => bail!(tr!(cli_unlock_no_input)),
        }
    }
    if line.is_empty() {
        bail!(tr!(cli_unlock_no_input));
    }
    Ok(Zeroizing::new(String::from_utf8_lossy(&line).into_owned()))
}

fn piped_line(stdin: &std::io::Stdin) -> Result<Zeroizing<String>> {
    let mut line = Zeroizing::new(String::new());
    if stdin.lock().read_line(&mut line)? == 0 {
        bail!(tr!(cli_unlock_no_input));
    }
    let len = line.trim_end_matches(['\r', '\n']).len();
    line.truncate(len);
    Ok(line)
}

fn ask(prompt: &str) -> Result<Zeroizing<String>> {
    use rustix::termios::{LocalModes, OptionalActions, tcgetattr, tcsetattr};
    let stdin = std::io::stdin();
    eprint!("{prompt} ");
    std::io::stderr().flush().ok();
    let saved = rustix::termios::isatty(&stdin).then(|| tcgetattr(&stdin).ok()).flatten();
    let Some(saved) = saved else { return piped_line(&stdin) };
    let mut quiet = saved.clone();
    quiet.local_modes.remove(LocalModes::ECHO | LocalModes::ICANON | LocalModes::ISIG);
    tcsetattr(&stdin, OptionalActions::Now, &quiet)?;
    let line = typed_line(stdin.lock().bytes());
    tcsetattr(&stdin, OptionalActions::Now, &saved)?;
    eprintln!();
    line
}

fn stretching() {
    eprintln!("{}", tr!(cli_unlock_working));
}

fn from_password(sealed: &Sealed) -> Result<DataKey> {
    for _ in 0..TRIES {
        let password = ask(tr!(cloud_unlock_label))?;
        stretching();
        if let Some(key) = sealed.open_with_password(&password)? {
            return Ok(key);
        }
        eprintln!("{}", tr!(cli_unlock_wrong));
    }
    bail!(tr!(cli_unlock_gave_up))
}

fn from_agent(sealed: &Sealed) -> Option<DataKey> {
    let slot = sealed.agent.as_ref()?;
    let mut agent = Agent::connect()?;
    let key = agent.keys().ok()?.into_iter().find(|key| key.fingerprint() == slot.fingerprint)?;
    let signature = Zeroizing::new(agent.sign(&key, &agent_challenge(&sealed.agent_salt()?)).ok()?);
    sealed.open_with_signature(&signature)
}

fn unlock(state: &Path, sealed: &Sealed) -> Result<DataKey> {
    if let Some(key) = held_key(state) {
        return Ok(key);
    }
    let key = match from_agent(sealed) {
        Some(key) => key,
        None if TERMINAL.load(Ordering::Relaxed) => from_password(sealed)?,
        None => {
            notify_locked();
            return Err(KeyLocked::Locked.into());
        }
    };
    keep(state, &key);
    Ok(key)
}

fn choose_password() -> Result<Zeroizing<String>> {
    eprintln!("{}", tr!(cli_unlock_choose, words = suggestion()?));
    for _ in 0..TRIES {
        let first = ask(tr!(cli_unlock_new))?;
        if first.trim().is_empty() {
            eprintln!("{}", tr!(cli_unlock_empty));
            continue;
        }
        if *ask(tr!(cli_unlock_repeat))? == *first {
            return Ok(first);
        }
        eprintln!("{}", tr!(cli_unlock_mismatch));
    }
    bail!(tr!(cli_unlock_none_chosen))
}

fn seal_new(state: &Path, text: &str) -> Result<()> {
    if !TERMINAL.load(Ordering::Relaxed) {
        return Err(KeyLocked::NoPassword.into());
    }
    let password = choose_password()?;
    stretching();
    let (sealed, key) = Sealed::seal(&password, Cost::for_this_machine(), text)?;
    save(state, &sealed)?;
    keep(state, &key);
    match std::fs::remove_file(plain_path(state)) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

pub(super) fn read(state: &Path) -> Result<Option<Zeroizing<String>>> {
    if let Some(sealed) = load(state)? {
        let key = unlock(state, &sealed)?;
        return sealed.open(&key).map(Some);
    }
    let Some(text) = read_private(&plain_path(state))? else { return Ok(None) };
    seal_new(state, &text)?;
    Ok(Some(text))
}

pub(super) fn write(state: &Path, text: &str) -> Result<()> {
    match load(state)? {
        Some(mut sealed) => {
            let key = unlock(state, &sealed)?;
            sealed.reseal(&key, text)?;
            save(state, &sealed)
        }
        None => seal_new(state, text),
    }
}

pub(super) fn unlock_with_password(state: &Path, password: &str) -> Result<bool> {
    let sealed = load(state)?.ok_or(KeyLocked::NoPassword)?;
    match sealed.open_with_password(password)? {
        Some(key) => {
            keep(state, &key);
            Ok(true)
        }
        None => Ok(false),
    }
}

fn keep_for_others(state: &Path) -> Result<()> {
    let cache = Cache::of(state);
    if cache.recall(IDLE).is_some() {
        return Ok(());
    }
    let key = held().lock().expect("held keys").get(state).cloned().context(tr!(cli_unlock_not_kept))?;
    cache.remember(&key, IDLE).context(tr!(cli_unlock_not_kept))
}

pub(super) fn lock(state: &Path) -> bool {
    let held = held().lock().expect("held keys").remove(state).is_some();
    Cache::of(state).forget() || held
}

fn agent_key(keys: Vec<AgentKey>) -> Option<AgentKey> {
    keys.into_iter().min_by_key(|key| key.kind != ED25519)
}

pub(super) fn allow_agent(state: &Path) -> Result<AgentKey> {
    let mut sealed = load(state)?.ok_or(KeyLocked::NoPassword)?;
    let key = unlock(state, &sealed)?;
    let mut agent = Agent::connect().ok_or_else(|| anyhow!(tr!(cli_unlock_no_agent)))?;
    let chosen = agent_key(agent.keys()?).context(tr!(cli_unlock_no_agent_key))?;
    let salt = Sealed::new_agent_salt()?;
    let challenge = agent_challenge(&salt);
    let first = Zeroizing::new(agent.sign(&chosen, &challenge)?);
    let second = Zeroizing::new(agent.sign(&chosen, &challenge)?);
    if *first != *second {
        bail!(tr!(cli_unlock_agent_random, fingerprint = chosen.fingerprint()));
    }
    sealed.allow_agent(&key, &chosen.fingerprint(), &salt, &first)?;
    save(state, &sealed)?;
    Ok(chosen)
}

pub(super) fn forbid_agent(state: &Path) -> Result<bool> {
    let Some(mut sealed) = load(state)? else { return Ok(false) };
    let had = sealed.agent.take().is_some();
    save(state, &sealed)?;
    Ok(had)
}

pub(super) async fn command(state: PathBuf, args: &[String]) -> Result<()> {
    if !sealed_here(&state) {
        println!("{}", tr!(cli_unlock_not_needed));
        return Ok(());
    }
    let flag = args.first().map(String::as_str);
    if args.len() > 1 || flag.is_some_and(|flag| flag != ALLOW_AGENT && flag != FORBID_AGENT) {
        bail!("usage: borg unlock [{ALLOW_AGENT} | {FORBID_AGENT}]");
    }
    allow_terminal();
    let loading = state.clone();
    tokio::task::spawn_blocking(move || load_computer_keys(&loading).and_then(|_| keep_for_others(&loading))).await??;
    match flag {
        Some(ALLOW_AGENT) => {
            let key = tokio::task::spawn_blocking(move || allow_agent(&state)).await??;
            println!("{}", tr!(cli_unlock_agent_allowed, key = key.comment, fingerprint = key.fingerprint()));
        }
        Some(_) => {
            tokio::task::spawn_blocking(move || forbid_agent(&state)).await??;
            println!("{}", tr!(cli_unlock_agent_forbidden));
        }
        None => {}
    }
    println!("{}", tr!(cli_unlocked));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(keys: &[u8]) -> Option<String> {
        typed_line(keys.iter().copied().map(Ok)).ok().map(|line| line.to_string())
    }

    #[test]
    fn a_password_is_what_was_typed_up_to_enter_with_corrections_applied() {
        assert_eq!(typed(b"secret\nmore"), Some("secret".into()));
        assert_eq!(typed(b"secrex\x7ft\r"), Some("secret".into()));
        assert_eq!(typed("pa\u{df}\u{4e16}\x08\x7fss\n".as_bytes()), Some("pass".into()));
        assert_eq!(typed(b"wrong\x15right\n"), Some("right".into()));
        assert_eq!(typed(b"\x7f\n"), Some(String::new()));
        assert_eq!(typed(b"piped"), Some("piped".into()));
    }

    #[test]
    fn interrupting_the_prompt_gives_no_password_instead_of_ending_borg_with_echo_off() {
        assert_eq!(typed(b"secr\x03"), None);
        assert_eq!(typed(b"\x04"), None);
        assert_eq!(typed(b""), None);
        assert_eq!(typed(b"secret\x04"), Some("secret".into()));
    }
}
