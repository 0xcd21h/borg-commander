//! Named workspaces on this machine. Pairing keys stay machine-wide; panel and terminal state
//! belongs to a stable session ID so renames and restarts do not move its files.

use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::{session::StateDir, store::write_atomically};

const REGISTRY: &str = "sessions.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedSession {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub last_opened: u64,
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
            session.id == "default"
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
    let default = NamedSession { id: "default".into(), name: "Default".into(), archived: false, last_opened: now() };
    save(state, std::slice::from_ref(&default))?;
    Ok(vec![default])
}

fn save(state: &StateDir, sessions: &[NamedSession]) -> Result<()> {
    fs::create_dir_all(state.path())?;
    write_atomically(&path(state), &serde_json::to_vec_pretty(sessions)?)
}

fn validate_name(name: &str, sessions: &[NamedSession], except: Option<&str>) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 || name.chars().any(char::is_control) {
        bail!("session name must contain 1–40 printable characters");
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
    let session = NamedSession { id, name, archived: false, last_opened: now() };
    sessions.push(session.clone());
    save(state, &sessions)?;
    Ok(session)
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

pub fn archive(state: &StateDir, target: &str, archived: bool) -> Result<()> {
    let mut sessions = list(state)?;
    let id = resolve(&sessions, target)?.id.clone();
    sessions.iter_mut().find(|session| session.id == id).expect("resolved session").archived = archived;
    save(state, &sessions)
}

pub fn delete(state: &StateDir, target: &str) -> Result<()> {
    let mut sessions = list(state)?;
    let id = resolve(&sessions, target)?.id.clone();
    if id == "default" {
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
        fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(workspace.join(".lock"))?;
    file.try_lock().with_context(|| format!("session {id} is already running"))?;
    Ok(file)
}

pub fn choose(state: &StateDir, requested: Option<&str>) -> Result<NamedSession> {
    use std::io::{IsTerminal, Write};
    let sessions = ensure_default(state)?;
    if let Some(requested) = requested {
        return open(state, requested);
    }
    let visible: Vec<_> = sessions.iter().filter(|session| !session.archived).collect();
    if visible.len() == 1 {
        return open(state, &visible[0].id);
    }
    if !std::io::stdin().is_terminal() {
        bail!("multiple Borg sessions exist; start with `borg --session NAME`");
    }
    println!("Borg sessions:");
    for (index, session) in visible.iter().enumerate() {
        println!("  {}. {}", index + 1, session.name);
    }
    println!("  n. Create a new session");
    print!("Choose a session: ");
    std::io::stdout().flush()?;
    let mut choice = String::new();
    std::io::stdin().read_line(&mut choice)?;
    if choice.trim().eq_ignore_ascii_case("n") {
        print!("New session name: ");
        std::io::stdout().flush()?;
        let mut name = String::new();
        std::io::stdin().read_line(&mut name)?;
        return create(state, &name);
    }
    let index: usize = choice.trim().parse().context("enter a session number or n")?;
    let session = visible.get(index.saturating_sub(1)).filter(|_| index != 0).context("no such session number")?;
    open(state, &session.id)
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
