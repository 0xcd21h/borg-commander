//! What a Borg carries into the newer Borg that an update starts in its place: the session's
//! lock, and the shell with its screen and what Borg knew of it.

use std::{fs, os::fd::OwnedFd, path::PathBuf};

use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use borg_term::Detached;
use serde::{Deserialize, Serialize};

use crate::relaunch::Carried;

/// The layout of the record, which changes whenever what it holds does.
const FORMAT: u32 = 1;

/// The shell under the panels, handed to the newer Borg.
#[derive(Debug)]
pub struct Shell {
    pub detached: Detached,
    /// The bytes that draw its screen and history.
    pub screen: Vec<u8>,
    /// Where the shell reads the folder to change into, which it was told when it started.
    pub cd_request: PathBuf,
    /// The shell reports its prompts and commands.
    pub integrated: bool,
    /// The shell changes folder without a typed command line.
    pub silent_cd: bool,
}

/// What the newer Borg starts from, besides the session's lock.
#[derive(Debug)]
pub struct Resumed {
    pub shell: Shell,
    /// What the user reads once the session runs again.
    pub notice: String,
}

#[derive(Serialize, Deserialize)]
struct Record {
    format: u32,
    lock: Option<usize>,
    master: usize,
    process: u32,
    screen: String,
    cd_request: PathBuf,
    integrated: bool,
    silent_cd: bool,
}

pub fn pack(lock: Option<OwnedFd>, shell: Shell) -> Result<Carried> {
    let mut carried = Carried::default();
    let lock = lock.map(|lock| carried.carry(lock));
    let record = Record {
        format: FORMAT,
        lock,
        master: carried.carry(shell.detached.master),
        process: shell.detached.process,
        screen: STANDARD.encode(shell.screen),
        cd_request: shell.cd_request,
        integrated: shell.integrated,
        silent_cd: shell.silent_cd,
    };
    carried.record = serde_json::to_vec(&record)?;
    Ok(carried)
}

/// The session's lock and the shell from what the previous Borg carried, with `notice` to show.
pub fn unpack(carried: Carried, notice: String) -> Result<(Option<fs::File>, Resumed)> {
    let (record, mut files) = carried.into_parts();
    let record: Record = serde_json::from_slice(&record).context("reading what the previous Borg carried")?;
    ensure!(record.format == FORMAT, "the previous Borg carried its session in another layout");
    let lock = record.lock.map(|lock| files.take(lock).map(fs::File::from)).transpose()?;
    let shell = Shell {
        detached: Detached { master: files.take(record.master)?, process: record.process },
        screen: STANDARD.decode(&record.screen)?,
        cd_request: record.cd_request,
        integrated: record.integrated,
        silent_cd: record.silent_cd,
    };
    Ok((lock, Resumed { shell, notice }))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, Write};

    use super::*;

    fn file_with(text: &str) -> OwnedFd {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(text.as_bytes()).unwrap();
        OwnedFd::from(file)
    }

    fn text_of(file: impl Into<OwnedFd>) -> String {
        let mut file = fs::File::from(file.into());
        file.rewind().unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn the_session_arrives_with_its_lock_and_its_shell_as_borg_knew_it() {
        let shell = Shell {
            detached: Detached { master: file_with("terminal"), process: 42 },
            screen: b"\x1b[1;1H$ make".to_vec(),
            cd_request: PathBuf::from("/run/borg/0.11.0/cd-7"),
            integrated: true,
            silent_cd: true,
        };
        let carried = pack(Some(file_with("lock")), shell).unwrap();
        let (lock, resumed) = unpack(carried, "moved".into()).unwrap();
        assert_eq!(text_of(lock.expect("the lock")), "lock");
        let shell = resumed.shell;
        assert_eq!((text_of(shell.detached.master), shell.detached.process), ("terminal".into(), 42));
        assert_eq!(shell.screen, b"\x1b[1;1H$ make");
        assert_eq!(shell.cd_request, PathBuf::from("/run/borg/0.11.0/cd-7"));
        assert!(shell.integrated && shell.silent_cd);
        assert_eq!(resumed.notice, "moved");
    }
}
