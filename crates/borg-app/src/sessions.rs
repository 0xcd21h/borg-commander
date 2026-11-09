//! Named workspaces on this machine. Pairing keys stay machine-wide; panel and terminal state
//! belongs to a stable session ID so renames and restarts do not move its files.

use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail, ensure};
use borg_core::{DEFAULT_SESSION, SESSION_NAME_LIMIT};
use serde::{Deserialize, Serialize};

use crate::{session::StateDir, store::write_atomically};

const REGISTRY: &str = "sessions.json";
/// The file in a session's folder that a running Borg holds locked.
const LOCK_FILE: &str = ".lock";
const STOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const STOP_POLL: std::time::Duration = std::time::Duration::from_millis(50);

pub async fn delete_remote(state: &StateDir, runtime: &std::path::Path, id: &str, serving: Option<&str>) -> Result<()> {
    let selected = list(state)?.into_iter().find(|session| session.id == id).context("The session no longer exists")?;
    ensure!(id != DEFAULT_SESSION, "The Default session is protected and cannot be removed");
    ensure!(
        serving != Some(id),
        "This session is hosting the remote connection. Close it on the host before removing it"
    );
    archive(state, id, true)?;
    let result = async {
        if delete(state, id).is_ok() {
            return Ok(());
        }
        let _ = tokio::time::timeout(STOP_TIMEOUT, async {
            if let Some(mut session) = crate::agent::Session::open(runtime, Some(id)).await {
                let _ = session.call(&crate::agent::Call::new("session_stop", serde_json::json!({"id":id}))).await;
            }
        })
        .await;
        let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
        loop {
            match delete(state, id) {
                Ok(()) => return Ok(()),
                Err(error) if tokio::time::Instant::now() >= deadline => {
                    return Err(error.context("Could not stop the session; close it on the host and retry"));
                }
                Err(_) => tokio::time::sleep(STOP_POLL).await,
            }
        }
    }
    .await;
    if result.is_err() {
        let _ = archive(state, id, selected.archived);
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedSession {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub last_opened: u64,
    /// The background service keeps this session running.
    #[serde(default)]
    pub background: bool,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |age| age.as_secs())
}

fn path(state: &StateDir) -> std::path::PathBuf {
    state.path().join(REGISTRY)
}

pub fn list(state: &StateDir) -> Result<Vec<NamedSession>> {
    let sessions: Vec<NamedSession> = match fs::read(path(state)) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("reading named Borg sessions"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error).context("reading named Borg sessions"),
    }?;
    let mut ids = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    for session in &sessions {
        ensure!(
            session.id == DEFAULT_SESSION
                || session.id.len() == 32 && session.id.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid Borg session ID in registry"
        );
        ensure!(ids.insert(&session.id) && names.insert(&session.name), "duplicate Borg session in registry");
    }
    Ok(sessions)
}

pub fn ensure_default(state: &StateDir) -> Result<Vec<NamedSession>> {
    let sessions = list(state)?;
    if !sessions.is_empty() {
        return Ok(sessions);
    }
    let default = NamedSession {
        id: DEFAULT_SESSION.into(),
        name: "Default".into(),
        archived: false,
        last_opened: now(),
        background: false,
    };
    save(state, std::slice::from_ref(&default))?;
    Ok(vec![default])
}

fn save(state: &StateDir, sessions: &[NamedSession]) -> Result<()> {
    fs::create_dir_all(state.path())?;
    write_atomically(&path(state), &serde_json::to_vec_pretty(sessions)?)
}

fn validate_name(name: &str, sessions: &[NamedSession], except: Option<&str>) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > SESSION_NAME_LIMIT || name.chars().any(char::is_control) {
        bail!("session name must contain 1 to {SESSION_NAME_LIMIT} printable characters");
    }
    if sessions.iter().any(|session| session.id != except.unwrap_or_default() && session.name == name) {
        bail!("a session named {name} already exists");
    }
    Ok(name.to_owned())
}

fn resolve<'a>(sessions: &'a [NamedSession], name: &str) -> Result<&'a NamedSession> {
    sessions
        .iter()
        .find(|session| session.id == name || session.name == name)
        .with_context(|| format!("no session named {name}"))
}

pub fn create(state: &StateDir, name: &str) -> Result<NamedSession> {
    let mut sessions = ensure_default(state)?;
    let name = validate_name(name, &sessions, None)?;
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random)?;
    let id = borg_link::hex(&random);
    let session = NamedSession { id, name, archived: false, last_opened: now(), background: false };
    sessions.push(session.clone());
    save(state, &sessions)?;
    Ok(session)
}

/// The session with this name or ID, as the registry has it.
pub fn find(state: &StateDir, name: &str) -> Result<NamedSession> {
    resolve(&list(state)?, name).cloned()
}

pub fn open(state: &StateDir, name: &str) -> Result<NamedSession> {
    let mut sessions = list(state)?;
    let entry = resolve(&sessions, name)?.id.clone();
    let session = sessions.iter_mut().find(|session| session.id == entry).expect("resolved session");
    session.last_opened = now();
    session.archived = false;
    let selected = session.clone();
    save(state, &sessions)?;
    Ok(selected)
}

pub fn rename(state: &StateDir, target: &str, name: &str) -> Result<NamedSession> {
    let mut sessions = list(state)?;
    let id = resolve(&sessions, target)?.id.clone();
    let name = validate_name(name, &sessions, Some(&id))?;
    let session = sessions.iter_mut().find(|session| session.id == id).expect("resolved session");
    session.name = name;
    let renamed = session.clone();
    save(state, &sessions)?;
    Ok(renamed)
}

/// Has the background service keep the session running, or lets it end with its Borg.
pub fn keep(state: &StateDir, target: &str, background: bool) -> Result<NamedSession> {
    let mut sessions = list(state)?;
    let id = resolve(&sessions, target)?.id.clone();
    let session = sessions.iter_mut().find(|session| session.id == id).expect("resolved session");
    session.background = background;
    let kept = session.clone();
    save(state, &sessions)?;
    Ok(kept)
}

pub fn archive(state: &StateDir, target: &str, archived: bool) -> Result<()> {
    let mut sessions = list(state)?;
    let id = resolve(&sessions, target)?.id.clone();
    sessions.iter_mut().find(|session| session.id == id).expect("resolved session").archived = archived;
    save(state, &sessions)
}

pub fn delete(state: &StateDir, target: &str) -> Result<()> {
    let mut sessions = list(state)?;
    let id = resolve(&sessions, target)?.id.clone();
    if id == DEFAULT_SESSION {
        bail!("the Default session holds Borg's existing state and cannot be deleted");
    }
    let _guard = lock(state, &id)?;
    let workspace = state.workspace(&id);
    // A session name is never used as a path. The ID is generated from random bytes only.
    if workspace.exists() {
        fs::remove_dir_all(&workspace).with_context(|| format!("removing {}", workspace.display()))?;
    }
    sessions.retain(|session| session.id != id);
    save(state, &sessions)
}

/// Held by a running terminal Borg. A second standalone process or deletion of its saved
/// workspace fails rather than racing with that process's session writes.
pub fn lock(state: &StateDir, id: &str) -> Result<fs::File> {
    let workspace = state.workspace(id);
    fs::create_dir_all(&workspace)?;
    let file =
        fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(workspace.join(LOCK_FILE))?;
    file.try_lock().with_context(|| format!("session {id} is already running"))?;
    Ok(file)
}

/// Whether a running Borg holds the session's lock now, in a terminal or in the background
/// service. Asking makes no folder for a session that never ran.
pub fn held(state: &StateDir, id: &str) -> bool {
    fs::File::open(state.workspace(id).join(LOCK_FILE))
        .is_ok_and(|file| matches!(file.try_lock(), Err(fs::TryLockError::WouldBlock)))
}

/// The session to open: the one asked for, the only one there is, or the one chosen in the
/// start window. `None` when the user quits the window.
pub async fn choose(state: &StateDir, requested: Option<&str>) -> Result<Option<NamedSession>> {
    use std::io::IsTerminal;
    let sessions = ensure_default(state)?;
    if let Some(requested) = requested {
        return open(state, requested).map(Some);
    }
    let visible: Vec<_> = sessions.iter().filter(|session| !session.archived).collect();
    if visible.len() == 1 {
        return open(state, &visible[0].id).map(Some);
    }
    if !std::io::stdin().is_terminal() {
        bail!("multiple Borg sessions exist; start with `borg --session NAME`");
    }
    crate::start_window::run(state).await
}

pub fn command(state: &StateDir, args: &[String]) -> Result<()> {
    let _ = ensure_default(state)?;
    match args {
        list_args if list_args.is_empty() || list_args == ["list"] => {
            for session in ensure_default(state)? {
                println!("{}  {}{}", session.id, session.name, if session.archived { "  (archived)" } else { "" });
            }
        }
        [action, name] if action == "create" => {
            let session = create(state, name)?;
            println!("Created {} ({}).", session.name, session.id);
        }
        [action, target, name] if action == "rename" => {
            let session = rename(state, target, name)?;
            println!("Renamed session to {} ({}).", session.name, session.id);
        }
        [action, target] if action == "archive" || action == "unarchive" => {
            archive(state, target, action == "archive")?;
            println!("{} {target}.", if action == "archive" { "Archived" } else { "Unarchived" });
        }
        [action, target] if action == "delete" => {
            use std::io::{IsTerminal, Write};
            if !std::io::stdin().is_terminal() {
                bail!("delete needs an interactive terminal for confirmation");
            }
            print!("Delete the saved Borg workspace for {target}? Files on the machine are not deleted. Type DELETE: ");
            std::io::stdout().flush()?;
            let mut answer = String::new();
            std::io::stdin().read_line(&mut answer)?;
            if answer.trim() != "DELETE" {
                bail!("session deletion cancelled");
            }
            delete(state, target)?;
            println!("Deleted {target}.");
        }
        _ => bail!(
            "usage: borg sessions [list | create NAME | rename OLD NEW | archive NAME | unarchive NAME | delete NAME]"
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn remote_removal_keeps_machine_files_and_protects_default_and_the_serving_session() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let state = StateDir::at(temp.path().join("state"));
        let first = create(&state, "junk")?;
        let serving = create(&state, "host")?;
        state.for_session(&first.id).save_terminal(b"saved history")?;
        let machine_file = temp.path().join("keep.txt");
        fs::write(&machine_file, b"keep")?;
        assert!(delete_remote(&state, temp.path(), "default", None).await.is_err());
        assert!(delete_remote(&state, temp.path(), &serving.id, Some(&serving.id)).await.is_err());
        assert!(!list(&state)?.iter().find(|entry| entry.id == serving.id).unwrap().archived);
        delete_remote(&state, temp.path(), &first.id, Some(&serving.id)).await?;
        assert!(!state.workspace(&first.id).exists());
        assert!(!list(&state)?.iter().any(|entry| entry.id == first.id));
        assert_eq!(fs::read(machine_file)?, b"keep");
        Ok(())
    }

    #[tokio::test]
    async fn failed_remote_removal_keeps_the_session_visible_and_its_saved_state() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let state = StateDir::at(temp.path().join("state"));
        let session = create(&state, "busy")?;
        state.for_session(&session.id).save_terminal(b"keep history")?;
        let _running = lock(&state, &session.id)?;
        assert!(delete_remote(&state, temp.path(), &session.id, None).await.is_err());
        assert!(!list(&state)?.iter().find(|entry| entry.id == session.id).unwrap().archived);
        assert_eq!(state.for_session(&session.id).load_terminal().as_deref(), Some(b"keep history".as_slice()));
        Ok(())
    }

    #[test]
    fn names_are_stable_across_rename_archive_and_reopen() {
        let temp = tempfile::tempdir().unwrap();
        let state = StateDir::at(temp.path().join("state"));
        let created = create(&state, "prod").unwrap();
        assert!(create(&state, "prod").is_err());
        assert_eq!(rename(&state, "prod", "web").unwrap().id, created.id);
        archive(&state, "web", true).unwrap();
        assert!(list(&state).unwrap().into_iter().find(|session| session.id == created.id).unwrap().archived);
        assert_eq!(open(&state, "web").unwrap().id, created.id);
        assert!(!list(&state).unwrap().into_iter().find(|session| session.id == created.id).unwrap().archived);
        delete(&state, "web").unwrap();
        assert_eq!(list(&state).unwrap().iter().map(|session| session.id.as_str()).collect::<Vec<_>>(), ["default"]);
    }

    #[test]
    fn workspaces_are_separate_and_running_one_cannot_be_deleted() {
        let temp = tempfile::tempdir().unwrap();
        let state = StateDir::at(temp.path().join("state"));
        let first = create(&state, "first").unwrap();
        let second = create(&state, "second").unwrap();
        let first_dir = state.for_session(&first.id);
        let second_dir = state.for_session(&second.id);
        first_dir.save_terminal(b"first shell").unwrap();
        assert!(second_dir.load_terminal().is_none());
        let guard = lock(&state, &first.id).unwrap();
        assert!(delete(&state, &first.id).is_err());
        drop(guard);
        delete(&state, &first.id).unwrap();
        assert!(list(&state).unwrap().iter().any(|session| session.id == second.id));
    }
}
