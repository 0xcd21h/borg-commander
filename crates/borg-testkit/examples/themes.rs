use std::{
    env, fs,
    path::PathBuf,
    time::{Duration, UNIX_EPOCH},
};

use anyhow::Result;
use borg_core::{Motion, Msg, Preferences, Settings};
use borg_term::GridSize;
use borg_testkit::{Snapshot, Stage};
use borg_vfs::{Entry, EntryKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const THEMES: [&str; 6] = ["midnight", "aurora", "neon", "classic", "paper", "ember"];
const SIZE: GridSize = GridSize { cols: 80, rows: 20 };
const CLOCK: &str = "2026-10-19T12:00:00+00:00[UTC]";
const MODIFIED: u64 = 1_792_368_000;

fn entry(name: &str, kind: EntryKind, size: u64) -> Entry {
    Entry {
        name: name.into(),
        kind,
        size,
        modified: Some(UNIX_EPOCH + Duration::from_secs(MODIFIED)),
        executable: false,
        permissions: 0o644,
    }
}

fn main() -> Result<()> {
    let output = PathBuf::from(env::args().nth(1).unwrap_or_else(|| "docs/themes".into()));
    fs::create_dir_all(&output)?;
    let project = vec![
        entry("src", EntryKind::Dir, 4096),
        entry("docs", EntryKind::Dir, 4096),
        entry("assets", EntryKind::Dir, 4096),
        entry("Cargo.toml", EntryKind::File, 1234),
        entry("README.md", EntryKind::File, 18432),
        entry("LICENSE", EntryKind::File, 1083),
        entry("release.sh", EntryKind::File, 512),
        entry("notes.txt", EntryKind::File, 2048),
    ];
    let releases = vec![
        entry("borg-macos.tar.gz", EntryKind::File, 4_194_304),
        entry("borg-linux.tar.gz", EntryKind::File, 5_242_880),
        entry("checksums.txt", EntryKind::File, 256),
        entry("release-notes.md", EntryKind::File, 2048),
    ];
    let preferences = Preferences { nerd_font: true, menu_bar: true, ..Preferences::DEFAULT };
    let settings = Settings { motion: Motion::STILL, preferences, ..Settings::default() };
    let mut stage = Stage::new(
        ["/home/demo/projects/borg".into(), "/home/demo/releases".into()],
        [project, releases],
        SIZE,
        settings,
        CLOCK.parse()?,
    )?;
    stage.home = Some("/home/demo".into());
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
    for id in THEMES {
        let index = stage.state.appearance.choices().iter().position(|choice| choice.id == id).unwrap();
        stage.state.appearance.select(index, stage.started, Duration::ZERO);
        let buffer = stage.render(stage.started)?;
        Snapshot::from_buffer(buffer, &stage.palette, None).save_png(&output.join(format!("{id}.png")))?;
    }
    Ok(())
}
