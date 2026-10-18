use std::{
    env,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use borg_core::{Motion, Msg, Settings};
use borg_term::GridSize;
use borg_testkit::{Snapshot, Stage, describe};
use borg_vfs::{Entry, EntryKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;

const CLOCK: &str = "2026-01-15T12:00:00+00:00[UTC]";
const RECENT: u64 = 1_768_037_400;
const OLD: u64 = 1_717_200_000;
const SIZE: GridSize = GridSize { cols: 90, rows: 22 };
const NARROW: GridSize = GridSize { cols: 44, rows: 12 };
const ENV_SCREENSHOTS: &str = "BORG_SCREENSHOTS";
const SCREENSHOT_DIR: &str = "ui";

fn at(secs: u64) -> Option<SystemTime> {
    Some(UNIX_EPOCH + Duration::from_secs(secs))
}

fn entry(name: &str, kind: EntryKind, size: u64, modified: u64, executable: bool) -> Entry {
    Entry { name: name.into(), kind, size, modified: at(modified), executable, permissions: 0o644 }
}

fn project() -> Vec<Entry> {
    vec![
        entry("src", EntryKind::Dir, 4096, RECENT, false),
        entry("docs", EntryKind::Dir, 4096, OLD, false),
        entry("target", EntryKind::Symlink { to_dir: true }, 12, RECENT, false),
        entry("Cargo.toml", EntryKind::File, 1_234, RECENT, false),
        entry("README.md", EntryKind::File, 48_213, OLD, false),
        entry("build.sh", EntryKind::File, 310, RECENT, true),
        entry("latest.log", EntryKind::Symlink { to_dir: false }, 9, RECENT, false),
        entry("日本語メモ.txt", EntryKind::File, 2_048, RECENT, false),
        entry("socket", EntryKind::Other, 0, OLD, false),
        entry(".env", EntryKind::File, 64, RECENT, false),
    ]
}

fn logs() -> Vec<Entry> {
    vec![
        entry("journal", EntryKind::Dir, 4096, RECENT, false),
        entry("syslog", EntryKind::File, 2_411_724, RECENT, false),
        entry("kern.log", EntryKind::File, 734_003, RECENT, false),
        entry("auth.log", EntryKind::File, 88_100, OLD, false),
        entry("dmesg.0", EntryKind::File, 5_368_709_120, OLD, false),
    ]
}

fn stage(size: GridSize, motion: Motion) -> Result<Stage> {
    let settings = Settings { motion, ..Settings::default() };
    Stage::new(
        [PathBuf::from("/home/me/projects/borg"), PathBuf::from("/var/log")],
        [project(), logs()],
        size,
        settings,
        CLOCK.parse()?,
    )
}

fn key(code: KeyCode) -> Msg {
    Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ctrl(c: char) -> Msg {
    Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

fn check(name: &str, stage: &Stage, buffer: Buffer) -> Result<()> {
    if let Some(dir) = env::var_os(ENV_SCREENSHOTS) {
        let path = PathBuf::from(dir).join(SCREENSHOT_DIR).join(name).with_extension("png");
        Snapshot::from_buffer(buffer.clone(), &stage.palette, None).save_png(&path)?;
    }
    insta::assert_snapshot!(name, describe(&buffer));
    Ok(())
}

#[test]
fn every_builtin_theme() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down));
    let ids: Vec<String> = stage.themes.themes().iter().map(|t| t.id.clone()).collect();

    for (index, id) in ids.iter().enumerate() {
        stage.state.appearance.select(index, stage.started, Duration::ZERO);
        let buffer = stage.render(stage.started)?;
        check(&format!("theme_{id}"), &stage, buffer)?;
    }
    Ok(())
}

#[test]
fn theme_picker_overlay() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(key(KeyCode::F(9))).send(key(KeyCode::Down));
    let buffer = stage.render(stage.started)?;
    check("picker", &stage, buffer)
}

#[test]
fn shell_view_shows_terminal() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.shell(b"ls -1\r\n\x1b[1;34msrc\x1b[0m\r\nCargo.toml\r\n\x1b]133;A\x07$ \x1b]133;B\x07");
    stage.send(ctrl('o'));
    let buffer = stage.render(stage.started)?;
    check("shell_view", &stage, buffer)
}

#[test]
fn crossfade_midpoint() -> Result<()> {
    let mut stage = stage(SIZE, Motion::LOCAL)?;
    stage.shell(b"echo hi\r\nhi\r\n\x1b]133;A\x07$ \x1b]133;B\x07");
    stage.send(ctrl('o'));
    let midpoint = stage.started + Motion::LOCAL.fade / 2;
    let buffer = stage.render(midpoint)?;
    check("crossfade_midpoint", &stage, buffer)
}

#[test]
fn narrow_terminal_drops_columns() -> Result<()> {
    let mut stage = stage(NARROW, Motion::STILL)?;
    let buffer = stage.render(stage.started)?;
    check("narrow", &stage, buffer)
}

#[test]
fn brief_listing_with_marks() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
    stage.send(key(KeyCode::Insert)).send(key(KeyCode::Down)).send(key(KeyCode::Insert));
    stage.state.panels[0].set_mode(borg_core::ListingMode::Brief);
    let buffer = stage.render(stage.started)?;
    check("brief_marked", &stage, buffer)
}

#[test]
fn file_menu_open() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down)).send(key(KeyCode::Down)).send(key(KeyCode::Down));
    stage.send(key(KeyCode::F(2)));
    let buffer = stage.render(stage.started)?;
    check("file_menu", &stage, buffer)
}

#[test]
fn copy_and_delete_dialogs() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down)).send(key(KeyCode::Down)).send(key(KeyCode::Down));
    stage.send(key(KeyCode::F(5)));
    let copy = stage.render(stage.started)?;
    check("copy_dialog", &stage, copy)?;

    stage.send(key(KeyCode::Esc)).send(key(KeyCode::F(8)));
    let delete = stage.render(stage.started)?;
    check("delete_dialog", &stage, delete)
}

#[test]
fn viewer_wraps_and_unwraps() -> Result<()> {
    use std::sync::Arc;

    let mut stage = stage(SIZE, Motion::STILL)?;
    let text: String = (1..=40)
        .map(|i| if i == 3 { format!("{i:>3} {}\n", "long ".repeat(30)) } else { format!("{i:>3} log line\n") })
        .collect();
    let (width, height) = borg_core::viewer_size(stage.state.cols, stage.state.rows);
    let pager = borg_view::Pager::new(Arc::new(text.into_bytes()), width, height)?;
    stage.send(Msg::Opened(borg_core::Viewer { path: PathBuf::from("/var/log/app.log"), pager }));
    let wrapped = stage.render(stage.started)?;
    check("viewer_wrapped", &stage, wrapped)?;

    stage.send(key(KeyCode::Char('w'))).send(key(KeyCode::Right));
    let unwrapped = stage.render(stage.started)?;
    check("viewer_unwrapped", &stage, unwrapped)
}

#[test]
fn widened_left_panel() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    let widen = KeyEvent::new(KeyCode::Right, KeyModifiers::ALT | KeyModifiers::SHIFT);
    (0..6).for_each(|_| {
        stage.send(Msg::Key(widen));
    });
    let buffer = stage.render(stage.started)?;
    check("widened", &stage, buffer)
}
