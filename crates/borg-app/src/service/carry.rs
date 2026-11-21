//! What the service carries into the newer Borg that an update starts in its place: its lock,
//! the guard watching the move, and each background session's terminal with the screen it
//! shows. Its socket stays behind: macOS empties a listening socket that travels this way,
//! and every connection it takes after that reads as closed.

use std::os::fd::OwnedFd;

use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use borg_term::{Detached, GridSize};
use serde::{Deserialize, Serialize};

use crate::relaunch::Carried;

/// The layout of the record, which changes whenever what it holds does.
const FORMAT: u32 = 1;

/// The service taken apart for the move.
#[derive(Debug)]
pub(super) struct Handed {
    pub lock: OwnedFd,
    /// The process of the guard watching the move, which the new service collects.
    pub guard: Option<u32>,
    pub terminals: Vec<Terminal>,
}

/// A background session's terminal, and what its screen shows.
#[derive(Debug)]
pub(super) struct Terminal {
    pub session: String,
    pub detached: Detached,
    pub size: GridSize,
    /// How much output the session wrote, which mirrors count from.
    pub sequence: u64,
    pub screen: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct Record {
    format: u32,
    lock: usize,
    guard: Option<u32>,
    terminals: Vec<TerminalRecord>,
}

#[derive(Serialize, Deserialize)]
struct TerminalRecord {
    session: String,
    master: usize,
    process: u32,
    cols: u16,
    rows: u16,
    sequence: u64,
    /// The bytes that draw the screen and its history, in base64.
    screen: String,
}

pub(super) fn pack(handed: Handed) -> Result<Carried> {
    let mut carried = Carried::default();
    let lock = carried.carry(handed.lock);
    let terminals = handed
        .terminals
        .into_iter()
        .map(|terminal| TerminalRecord {
            session: terminal.session,
            master: carried.carry(terminal.detached.master),
            process: terminal.detached.process,
            cols: terminal.size.cols,
            rows: terminal.size.rows,
            sequence: terminal.sequence,
            screen: STANDARD.encode(terminal.screen),
        })
        .collect();
    let record = Record { format: FORMAT, lock, guard: handed.guard, terminals };
    carried.record = serde_json::to_vec(&record)?;
    Ok(carried)
}

pub(super) fn unpack(carried: Carried) -> Result<Handed> {
    let (record, mut files) = carried.into_parts();
    let record: Record = serde_json::from_slice(&record).context("reading what the previous service carried")?;
    ensure!(record.format == FORMAT, "the previous service carried its sessions in another layout");
    let terminals = record
        .terminals
        .into_iter()
        .map(|terminal| {
            Ok(Terminal {
                detached: Detached { master: files.take(terminal.master)?, process: terminal.process },
                size: GridSize { cols: terminal.cols, rows: terminal.rows },
                sequence: terminal.sequence,
                screen: STANDARD.decode(&terminal.screen)?,
                session: terminal.session,
            })
        })
        .collect::<Result<_>>()?;
    Ok(Handed { lock: files.take(record.lock)?, guard: record.guard, terminals })
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, Write};

    use super::*;

    fn file_with(dir: &tempfile::TempDir, name: &str) -> OwnedFd {
        let mut file = tempfile::tempfile_in(dir.path()).unwrap();
        file.write_all(name.as_bytes()).unwrap();
        OwnedFd::from(file)
    }

    fn text_of(file: OwnedFd) -> String {
        let mut file = std::fs::File::from(file);
        file.rewind().unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn the_service_arrives_with_its_lock_guard_and_terminals_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let terminal = |session: &str, process| Terminal {
            session: session.into(),
            detached: Detached { master: file_with(&dir, session), process },
            size: GridSize { cols: 120, rows: 40 },
            sequence: 7,
            screen: format!("screen of {session}").into_bytes(),
        };
        let handed = Handed {
            lock: file_with(&dir, "lock"),
            guard: Some(99),
            terminals: vec![terminal("work", 11), terminal("build", 12)],
        };
        let arrived = unpack(pack(handed).unwrap()).unwrap();
        assert_eq!(text_of(arrived.lock), "lock");
        assert_eq!(arrived.guard, Some(99));
        let terminals: Vec<_> = arrived
            .terminals
            .into_iter()
            .map(|terminal| {
                let screen = String::from_utf8(terminal.screen).unwrap();
                (terminal.session, text_of(terminal.detached.master), terminal.detached.process, terminal.size, screen)
            })
            .collect();
        let size = GridSize { cols: 120, rows: 40 };
        assert_eq!(
            terminals,
            [
                ("work".into(), "work".into(), 11, size, "screen of work".into()),
                ("build".into(), "build".into(), 12, size, "screen of build".into()),
            ]
        );
    }

    #[test]
    fn a_record_in_another_layout_is_refused() {
        let layout = serde_json::json!({
            "format": FORMAT + 1, "lock": 0, "guard": null, "terminals": []
        });
        let carried = Carried { record: serde_json::to_vec(&layout).unwrap(), files: Vec::new() };
        assert!(unpack(carried).is_err());
    }
}
