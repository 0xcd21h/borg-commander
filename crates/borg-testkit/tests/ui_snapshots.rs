use std::{
    env,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use borg_core::{Motion, Msg, Preferences, Settings};
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
const HOME: &str = "/home/me";
const QUIET: Preferences = Preferences::DEFAULT;

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

fn crew() -> Vec<Entry> {
    ["one", "two", "three", "four", "five", "six", "seven", "eight", "nine"]
        .iter()
        .map(|name| entry(&format!("{name}.drone"), EntryKind::File, 1_000, RECENT, false))
        .collect()
}

fn staged(motion: Motion, preferences: Preferences, left: Vec<Entry>, clock: &str) -> Result<Stage> {
    let settings = Settings { motion, preferences, ..Settings::default() };
    let mut stage = Stage::new(
        [PathBuf::from("/home/me/projects/borg"), PathBuf::from("/var/log")],
        [left, logs()],
        SIZE,
        settings,
        clock.parse()?,
    )?;
    stage.home = Some(PathBuf::from(HOME));
    Ok(stage)
}

fn stage(size: GridSize, motion: Motion) -> Result<Stage> {
    let settings = Settings { motion, preferences: QUIET, ..Settings::default() };
    let mut stage = Stage::new(
        [PathBuf::from("/home/me/projects/borg"), PathBuf::from("/var/log")],
        [project(), logs()],
        size,
        settings,
        CLOCK.parse()?,
    )?;
    stage.home = Some(PathBuf::from(HOME));
    Ok(stage)
}

fn chars(c: &str) -> impl Iterator<Item = Msg> + '_ {
    c.chars().map(|c| key(KeyCode::Char(c)))
}

fn key(code: KeyCode) -> Msg {
    Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ctrl(c: char) -> Msg {
    Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

fn check(name: &str, stage: &Stage, buffer: Buffer) -> Result<()> {
    check_with(name, stage, buffer, Vec::new())
}

fn check_with(name: &str, stage: &Stage, buffer: Buffer, placements: Vec<borg_gfx::Placement>) -> Result<()> {
    if let Some(dir) = env::var_os(ENV_SCREENSHOTS) {
        let path = PathBuf::from(dir).join(SCREENSHOT_DIR).join(name).with_extension("png");
        Snapshot::from_buffer(buffer.clone(), &stage.palette, None).with_placements(placements).save_png(&path)?;
    }
    insta::assert_snapshot!(name, describe(&buffer));
    Ok(())
}

#[test]
fn every_builtin_theme() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down));
    let ids: Vec<String> = stage.state.appearance.choices().iter().map(|t| t.id.clone()).collect();

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
fn shell_switch_is_immediate() -> Result<()> {
    let mut stage = stage(SIZE, Motion::LOCAL)?;
    stage.shell(b"echo hi\r\nhi\r\n\x1b]133;A\x07$ \x1b]133;B\x07");
    stage.send(ctrl('o'));
    let midpoint = stage.started + Motion::LOCAL.fade / 2;
    let buffer = stage.render(midpoint)?;
    check("shell_switch_immediate", &stage, buffer)
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
    stage.send(Msg::Opened(borg_core::Viewer {
        path: PathBuf::from("/var/log/app.log"),
        content: borg_core::Content::Text(pager),
    }));
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

#[test]
fn about_screen_and_cube_unlock() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(key(KeyCode::F(1)));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Resistance is futile.")?;
    check("about", &stage, buffer)?;

    chars("borg").for_each(|msg| {
        stage.send(msg);
    });
    let unlocked = stage.state.appearance.choices().iter().any(|choice| choice.id == "cube");
    anyhow::ensure!(unlocked, "typing borg in About must unlock the cube theme");
    let cube = stage.state.appearance.choices().len() - 1;
    stage.send(key(KeyCode::Esc));
    stage.state.appearance.select(cube, stage.started, Duration::ZERO);
    let buffer = stage.render(stage.started)?;
    check("theme_cube", &stage, buffer)
}

#[test]
fn konami_code_starts_glyph_rain() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(key(KeyCode::F(1)));
    [
        KeyCode::Up,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Left,
        KeyCode::Right,
    ]
    .into_iter()
    .map(key)
    .chain(chars("ba"))
    .for_each(|msg| {
        stage.send(msg);
    });
    anyhow::ensure!(stage.state.raining(stage.started), "konami must start the rain");
    let buffer = stage.render(stage.started + Duration::from_millis(1500))?;
    check("about_rain", &stage, buffer)
}

#[test]
fn screensaver_after_idle() -> Result<()> {
    let preferences = Preferences { screensaver: true, ..QUIET };
    let mut stage = staged(Motion::LOCAL, preferences, project(), CLOCK)?;
    stage.state.settings.screensaver_after = Duration::from_secs(5);
    let later = stage.started + Duration::from_secs(6);
    borg_core::update(&mut stage.state, Msg::Tick, later);
    anyhow::ensure!(stage.state.screensaver, "screensaver must start after the idle period");
    let buffer = stage.render(later)?;
    check("screensaver", &stage, buffer)?;

    borg_core::update(&mut stage.state, key(KeyCode::Char('x')), later);
    anyhow::ensure!(!stage.state.screensaver, "a key must wake the screensaver");
    Ok(())
}

#[test]
fn seven_of_nine_and_assimilation() -> Result<()> {
    let mut stage = staged(Motion::STILL, QUIET, crew(), CLOCK)?;
    stage.send(key(KeyCode::Down));
    (0..7).for_each(|_| {
        stage.send(key(KeyCode::Insert));
    });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Seven of Nine")?;
    check("seven_of_nine", &stage, buffer)?;

    stage.send(key(KeyCode::Insert)).send(key(KeyCode::Insert));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "all 9 assimilated")
}

#[test]
fn empty_folder_shows_stars() -> Result<()> {
    let mut stage = staged(Motion::STILL, QUIET, Vec::new(), CLOCK)?;
    let buffer = stage.render(stage.started + Duration::from_secs(2))?;
    ensure_text(&buffer, "empty")?;
    check("empty_folder", &stage, buffer)
}

#[test]
fn loading_shows_spinner_and_comet() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(key(KeyCode::Down));
    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), stage.started);
    let Some(borg_core::Effect::List { side, generation, .. }) = effects.into_iter().next() else {
        anyhow::bail!("entering a folder must list it");
    };
    stage.send(Msg::Batch { side, generation, entries: logs() });
    let buffer = stage.render(stage.started + Duration::from_millis(450))?;
    ensure_text(&buffer, "loading…")?;
    check("loading", &stage, buffer)
}

#[test]
fn entering_a_folder_shows_the_new_listing_at_once() -> Result<()> {
    let mut stage = stage(SIZE, Motion::LOCAL)?;
    stage.send(key(KeyCode::Down));
    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), stage.started);
    let Some(borg_core::Effect::List { side, generation, .. }) = effects.into_iter().next() else {
        anyhow::bail!("entering a folder must list it");
    };
    stage.send(Msg::Batch { side, generation, entries: logs() });
    stage.send(Msg::Listed { side, generation, error: None });
    let early = describe(&stage.render(stage.started + Motion::LOCAL.fade / 3)?);
    let late = describe(&stage.render(stage.started + Motion::LOCAL.fade * 2)?);
    anyhow::ensure!(early == late, "the listing must not animate in:\n{early}");
    Ok(())
}

#[test]
fn menu_opens_immediately() -> Result<()> {
    let mut stage = stage(SIZE, Motion::LOCAL)?;
    stage.send(key(KeyCode::F(2)));
    let buffer = stage.render(stage.started + Motion::LOCAL.fade / 3)?;
    check("menu_immediate", &stage, buffer)
}

#[test]
fn without_truecolor_every_colour_is_indexed() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.state.settings.truecolor = false;
    let neon = stage.state.appearance.choices().iter().position(|c| c.id == "neon").unwrap_or_default();
    stage.state.appearance.select(neon, stage.started, Duration::ZERO);
    stage.send(key(KeyCode::F(8)));
    let buffer = stage.render(stage.started)?;
    let truecolor = buffer.content.iter().any(|cell| {
        matches!(cell.fg, ratatui::style::Color::Rgb(..)) || matches!(cell.bg, ratatui::style::Color::Rgb(..))
    });
    anyhow::ensure!(!truecolor, "256-colour terminals must not receive 24-bit colours");
    Ok(())
}

fn ensure_text(buffer: &Buffer, needle: &str) -> Result<()> {
    let text = describe(buffer);
    anyhow::ensure!(text.contains(needle), "expected {needle:?} in\n{text}");
    Ok(())
}

#[test]
fn dialogs_close_immediately() -> Result<()> {
    let mut stage = stage(SIZE, Motion::LOCAL)?;
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down)).send(key(KeyCode::F(8)));
    let closing = stage.started + Motion::LOCAL.fade;
    borg_core::update(&mut stage.state, key(KeyCode::Esc), closing);
    let buffer = stage.render(closing + Motion::LOCAL.fade / 3)?;
    check("dialog_closed", &stage, buffer)
}

fn sunset(width: u32, height: u32) -> Result<borg_gfx::Picture> {
    let pixel = |x: u32, y: u32| {
        let t = y as f32 / height as f32;
        let (dx, dy) = (x as f32 - width as f32 * 0.5, y as f32 - height as f32 * 0.62);
        let sun = (dx * dx + dy * dy).sqrt() < height as f32 * 0.22;
        let sky = [255.0 * (0.25 + 0.75 * t), 80.0 + 60.0 * t, 160.0 * (1.0 - t) + 40.0];
        let [r, g, b] = if sun { [255.0, 210.0, 90.0] } else { sky };
        [r as u8, g as u8, b as u8, u8::MAX]
    };
    let rgba = (0..height).flat_map(|y| (0..width).flat_map(move |x| pixel(x, y))).collect();
    Ok(borg_gfx::Picture::from_rgba(width, height, rgba)?)
}

fn open_picture(stage: &mut Stage) -> Result<()> {
    let image =
        borg_core::Image { picture: sunset(320, 180)?, original: (3840, 2160), format: "jpg", bytes: 2_411_724 };
    let path = PathBuf::from("/home/me/projects/borg/sunset.jpg");
    stage.send(Msg::Opened(borg_core::Viewer { path, content: borg_core::Content::Image(image) }));
    Ok(())
}

#[test]
fn picture_viewer_draws_half_blocks_without_graphics() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    open_picture(&mut stage)?;
    let (buffer, placements) = stage.compose(stage.started)?;
    anyhow::ensure!(placements.is_empty(), "cells mode must not place images");
    ensure_text(&buffer, "3840×2160 · JPG · 2.3M")?;
    check("picture_cells", &stage, buffer)
}

#[test]
fn picture_viewer_places_pixels_with_graphics() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.pixels = borg_ui::Pixels::Pictures;
    open_picture(&mut stage)?;
    let (buffer, placements) = stage.compose(stage.started)?;
    anyhow::ensure!(placements.len() == 1, "expected one placement, got {placements:?}");
    anyhow::ensure!(!describe(&buffer).contains('▀'), "pixel mode must leave the picture area to the terminal");
    check_with("picture_pixels", &stage, buffer, placements)
}

#[test]
fn nerd_font_icons() -> Result<()> {
    let mut stage = staged(Motion::STILL, Preferences { nerd_font: true, ..QUIET }, project(), CLOCK)?;
    let neon = stage.state.appearance.choices().iter().position(|c| c.id == "neon").unwrap_or_default();
    stage.state.appearance.select(neon, stage.started, Duration::ZERO);
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down));
    let buffer = stage.render(stage.started)?;
    check("icons", &stage, buffer)
}

#[test]
fn quick_find_highlights_matches() -> Result<()> {
    let mut stage = staged(Motion::STILL, Preferences { nerd_font: true, ..QUIET }, project(), CLOCK)?;
    stage.send(ctrl('s'));
    chars("o").for_each(|msg| {
        stage.send(msg);
    });
    stage.send(key(KeyCode::Down));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "find: o  2/4")?;
    check("quick_find", &stage, buffer)
}

fn select_theme(stage: &mut Stage, id: &str) {
    let index = stage.state.appearance.choices().iter().position(|c| c.id == id).unwrap_or_default();
    stage.state.appearance.select(index, stage.started, Duration::ZERO);
}

#[test]
fn graphics_enabled_panels_and_dialogs_use_only_terminal_cells() -> Result<()> {
    let mut stage = staged(Motion::STILL, Preferences { nerd_font: true, ..QUIET }, project(), CLOCK)?;
    stage.pixels = borg_ui::Pixels::Pictures;
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down));
    for id in ["neon", "midnight"] {
        select_theme(&mut stage, id);
        let (buffer, placements) = stage.compose(stage.started)?;
        anyhow::ensure!(placements.is_empty(), "panels must not transmit images");
        anyhow::ensure!(describe(&buffer).lines().next().unwrap_or_default().contains('╭'));
    }
    stage.send(key(KeyCode::F(8)));
    let (buffer, placements) = stage.compose(stage.started)?;
    anyhow::ensure!(placements.is_empty(), "dialogs must not transmit images");
    ensure_text(&buffer, "Delete")?;
    Ok(())
}

fn running_copy(stage: &mut Stage) -> Result<u64> {
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down)).send(key(KeyCode::Down)).send(key(KeyCode::Down));
    stage.send(key(KeyCode::F(5)));
    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), stage.started);
    let Some(borg_core::Effect::StartJob { id, .. }) = effects.first() else { anyhow::bail!("copy must start a job") };
    let progress = borg_jobs::Progress {
        done: borg_jobs::Totals { files: 41, bytes: 1_610_612_736 },
        total: borg_jobs::Totals { files: 120, bytes: 4_294_967_296 },
        current: PathBuf::from("/home/me/projects/borg/build.sh"),
    };
    stage.send(Msg::Job { id: *id, event: borg_jobs::Event::Progress(progress) });
    Ok(*id)
}

#[test]
fn copy_job_progress_and_conflict() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    let id = running_copy(&mut stage)?;
    let later = stage.started + Duration::from_secs(20);
    let buffer = stage.render(later)?;
    ensure_text(&buffer, "Copying 37% · 77M/s · 0:34")?;
    check("job_progress", &stage, buffer)?;

    let ask = borg_jobs::Ask::Conflict {
        source: PathBuf::from("/home/me/projects/borg/build.sh"),
        target: PathBuf::from("/var/log/build.sh"),
        sizes: (310, 128),
        newer: true,
    };
    stage.send(Msg::Job { id, event: borg_jobs::Event::Asking(ask) });
    let buffer = stage.render(later)?;
    ensure_text(&buffer, "Overwrite all")?;
    check("job_conflict", &stage, buffer)?;

    stage.send(key(KeyCode::Char('s')));
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT)));
    let buffer = stage.render(later)?;
    ensure_text(&buffer, "p pause · c cancel")?;
    check("job_list", &stage, buffer)
}

fn peek(stage: &mut Stage, aside: borg_core::Aside, peek: borg_core::Peek) -> Result<()> {
    let panel = stage.state.active_panel();
    let Some(borg_core::Row::Entry(entry)) = panel.hovered() else { anyhow::bail!("the cursor must be on an entry") };
    let path = panel.dir().join(&entry.name);
    stage.send(Msg::Peeked(borg_core::Peeked { aside, path, peek }));
    Ok(())
}

fn hover(stage: &mut Stage, name: &str) {
    stage.send(key(KeyCode::Home));
    while stage.state.active_panel().hovered_entry().is_none_or(|entry| entry.name != name) {
        stage.send(key(KeyCode::Down));
    }
}

#[test]
fn quick_view_shows_text_folders_and_pictures() -> Result<()> {
    let mut stage = staged(Motion::STILL, Preferences { nerd_font: true, ..QUIET }, project(), CLOCK)?;
    hover(&mut stage, "Cargo.toml");
    stage.send(ctrl('q'));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Quick view")?;
    let manifest = ["[package]", "name = \"borg\"", "version = \"0.1.0\"", "", "[dependencies]", "ratatui = \"0.30\""];
    peek(
        &mut stage,
        borg_core::Aside::QuickView,
        borg_core::Peek::Text(borg_view::Pager::for_path(
            std::sync::Arc::new(manifest.join("\n").into_bytes()),
            std::path::Path::new("Cargo.toml"),
            43,
            15,
        )?),
    )?;
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Quick view · Cargo.toml")?;
    ensure_text(&buffer, "ratatui = \"0.30\"")?;
    check("quick_view_text", &stage, buffer)?;

    hover(&mut stage, "src");
    let entries = vec![
        entry("ui", EntryKind::Dir, 4096, RECENT, false),
        entry("main.rs", EntryKind::File, 2_048, RECENT, false),
        entry("lib.rs", EntryKind::File, 12_288, RECENT, false),
    ];
    let folder = borg_core::Peek::Folder { folders: 1, files: 2, bytes: 14_336, entries };
    peek(&mut stage, borg_core::Aside::QuickView, folder)?;
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "1 folder · 2 files · 14K")?;
    check("quick_view_folder", &stage, buffer)?;

    hover(&mut stage, "README.md");
    stage.pixels = borg_ui::Pixels::Pictures;
    let image =
        borg_core::Image { picture: sunset(320, 180)?, original: (3840, 2160), format: "jpg", bytes: 2_411_724 };
    peek(&mut stage, borg_core::Aside::QuickView, borg_core::Peek::Picture(image))?;
    let (buffer, placements) = stage.compose(stage.started)?;
    anyhow::ensure!(placements.len() == 1, "the picture is placed once, got {placements:?}");
    ensure_text(&buffer, "3840×2160 · JPG · 2.3M")?;
    stage.send(key(KeyCode::F(8)));
    let (_, placements) = stage.compose(stage.started)?;
    anyhow::ensure!(placements.is_empty(), "a dialog hides the picture when cells cannot cover it");
    Ok(())
}

#[test]
fn info_panel_lists_file_facts() -> Result<()> {
    let mut stage = staged(Motion::STILL, Preferences { nerd_font: true, ..QUIET }, project(), CLOCK)?;
    hover(&mut stage, "build.sh");
    stage.send(ctrl('l'));
    let facts = borg_core::Facts {
        nature: borg_core::Nature::File,
        size: 310,
        permissions: 0o755,
        owner: "me".into(),
        group: "staff".into(),
        modified: at(RECENT),
        accessed: at(RECENT),
        created: at(OLD),
        inode: 1_234_567,
        links: 1,
        items: None,
        disk: Some((128_849_018_880, 499_963_174_912)),
    };
    peek(&mut stage, borg_core::Aside::Info, borg_core::Peek::Info(facts))?;
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "rwxr-xr-x 0755")?;
    ensure_text(&buffer, "me:staff")?;
    ensure_text(&buffer, "120G of 466G")?;
    check("info_panel", &stage, buffer)
}

#[test]
fn composer_grows_from_the_prompt() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(Msg::CommandLine("for f in *.log".into()));
    stage.send(key(KeyCode::Enter));
    chars("do gzip \"$f\"").for_each(|msg| {
        stage.send(msg);
    });
    stage.send(key(KeyCode::Enter));
    chars("done").for_each(|msg| {
        stage.send(msg);
    });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "3 lines")?;
    ensure_text(&buffer, "Alt-Enter run")?;
    check("composer", &stage, buffer)
}

#[test]
#[ignore = "timing report, run with --ignored --nocapture"]
fn frame_cost() -> Result<()> {
    let big = GridSize { cols: 220, rows: 60 };
    for id in ["midnight", "neon", "paper"] {
        let mut stage = stage(big, Motion::LOCAL)?;
        select_theme(&mut stage, id);
        let rounds = 60u32;
        let start = std::time::Instant::now();
        for i in 0..rounds {
            stage.send(key(if i % 2 == 0 { KeyCode::Down } else { KeyCode::Up }));
            stage.render(stage.started + Duration::from_secs(5))?;
        }
        let plain = start.elapsed() / rounds;
        stage.send(key(KeyCode::F(8)));
        let start = std::time::Instant::now();
        for _ in 0..rounds {
            stage.render(stage.started + Motion::LOCAL.fade / 2)?;
        }
        println!("{id}: cursor move {plain:?} per frame, dialog fading {:?} per frame", start.elapsed() / rounds);
    }
    Ok(())
}

#[test]
fn console_history_and_function_alias_hints() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    for n in 1..=60 {
        stage.shell(format!("output line {n:02}\r\n").as_bytes());
    }
    stage.send(ctrl('o'));
    stage.emulator.scroll(borg_term::Scroll::PageUp);
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Borg console")?;
    ensure_text(&buffer, "Shift-End live")?;
    check("console_history", &stage, buffer)?;
    stage.send(ctrl('o')).send(key(KeyCode::Esc));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Esc 9 opens menus")?;
    check("function_alias_hint", &stage, buffer)
}

#[test]
fn all_overlays_survive_small_terminal_sizes() -> Result<()> {
    for (cols, rows) in [(193, 40), (64, 18), (20, 5), (2, 2), (1, 1), (0, 0)] {
        for code in [KeyCode::F(1), KeyCode::F(7), KeyCode::F(8), KeyCode::F(9)] {
            let mut stage = stage(SIZE, Motion::STILL)?;
            stage.send(key(KeyCode::Down)).send(key(code));
            stage.send(Msg::Resize { cols, rows });
            stage.emulator.resize(GridSize::console(cols, rows));
            stage.render(stage.started)?;
        }
    }
    Ok(())
}

#[test]
fn wide_keybar_advertises_function_key_alternatives() -> Result<()> {
    let mut stage = stage(GridSize { cols: 193, rows: 40 }, Motion::STILL)?;
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Esc then 1–9/0 = F1–F10")?;
    check("wide_keybar", &stage, buffer)
}

#[test]
fn mouse_targets_match_the_drawn_menu_and_brief_rows() -> Result<()> {
    use borg_core::{ListingMode, Pointer, Side};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut stage = stage(SIZE, Motion::STILL)?;
    let click = |x, y| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    let hit = |stage: &Stage, x, y| borg_ui::mouse_input(&stage.state, click(x, y), false, stage.started);
    let buffer = stage.render(stage.started)?;
    let screen = Snapshot::from_buffer(buffer, &stage.palette, None);
    let (x, y) = screen.find("9Menu").unwrap();
    assert!(matches!(hit(&stage, x, y), Some(Msg::Pointer(Pointer::Action(borg_core::Action::OpenMenuBar)))));
    stage.send(hit(&stage, x, y).unwrap());
    assert!(matches!(hit(&stage, 29, 0), Some(Msg::Pointer(Pointer::Menu(3)))));
    stage.send(hit(&stage, 29, 0).unwrap());
    let buffer = stage.render(stage.started)?;
    let screen = Snapshot::from_buffer(buffer, &stage.palette, None);
    let (x, y) = screen.find("Theme…").unwrap();
    assert!(matches!(hit(&stage, x, y), Some(Msg::Pointer(Pointer::MenuItem(0)))));
    stage.send(key(KeyCode::Esc));
    stage.send(Msg::Pointer(Pointer::Action(borg_core::Action::SetListing(Side::Left, ListingMode::Brief))));
    let rect = stage.state.layout(stage.started).panels[0];
    let row = rect.y + 2;
    assert!(matches!(
        hit(&stage, rect.x + 3, row),
        Some(Msg::Pointer(Pointer::Panel { side: Side::Left, index: 1, .. }))
    ));
    let second = hit(&stage, rect.x + rect.width - 3, row).unwrap();
    let expected = stage.state.active_panel().rows() + 1;
    assert!(matches!(second, Msg::Pointer(Pointer::Panel { index, .. }) if index == expected));
    Ok(())
}

#[test]
fn settings_window_has_checkboxes() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(borg_core::Msg::Pointer(borg_core::Pointer::Action(borg_core::Action::Settings)));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "[x] Screensaver when idle")?;
    ensure_text(&buffer, "[x] Return to panels after command")?;
    check("settings_window", &stage, buffer)
}

#[test]
fn markdown_renders_and_source_toggle_is_visible() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    let source = b"# Borg guide\n\nA **file manager** with `syntax highlighting`.\n\n- [x] Copy files\n- [ ] Read the guide\n\n```rust\nfn main() {\n    println!(\"Hello, Borg!\");\n}\n```\n\n[Project](https://example.com/borg)\n".to_vec();
    let path = PathBuf::from("/home/me/projects/borg/guide.md");
    let pager = borg_view::Pager::for_path(
        std::sync::Arc::new(source),
        &path,
        usize::from(SIZE.cols),
        usize::from(SIZE.rows - 1),
    )?;
    stage.send(Msg::Opened(borg_core::Viewer { path, content: borg_core::Content::Text(pager) }));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "s source/render")?;
    check("markdown_rendered", &stage, buffer)?;
    stage.send(key(KeyCode::Char('s')));
    let buffer = stage.render(stage.started)?;
    check("markdown_source", &stage, buffer)
}

#[test]
fn multiline_recalled_prompt_keeps_all_rows_visible() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage
        .shell(b"\x1b[20;1H\x1b]133;A\x07$ \x1b]133;B\x07for file in *.rs; do\r\n    printf '%s\\n' \"$file\"\r\ndone");
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "for file in *.rs; do")?;
    ensure_text(&buffer, "done")?;
    check("multiline_prompt", &stage, buffer)
}

#[test]
fn git_plugin_shows_status_and_can_be_disabled() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    let dir = stage.state.panels[0].dir().to_path_buf();
    let generation = stage.state.panels[0].generation();
    let repository = borg_git::Repository::parse(
        dir.clone(),
        b"## main\0 M src/main.rs\0A  Cargo.toml\0MM README.md\0?? build.sh\0UU socket\0",
    )?;
    stage.send(Msg::GitStatus { side: borg_core::Side::Left, generation, dir, result: Ok(Some(repository)) });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Git main")?;
    check("git_status", &stage, buffer)?;
    stage.state.panels[0].set_mode(borg_core::ListingMode::Brief);
    let buffer = stage.render(stage.started)?;
    check("git_status_brief", &stage, buffer)?;
    stage.state.panels[0].set_mode(borg_core::ListingMode::Full);
    let index = borg_core::menus(&stage.state).iter().position(|menu| menu.title == "Git").unwrap();
    stage.send(Msg::Pointer(borg_core::Pointer::Menu(index)));
    let buffer = stage.render(stage.started)?;
    check("git_menu", &stage, buffer)?;
    stage.state.overlay = Some(borg_core::Overlay::Plugins { selected: 0 });
    let buffer = stage.render(stage.started)?;
    check("plugins_enabled", &stage, buffer)?;
    stage.send(key(KeyCode::Char(' ')));
    let buffer = stage.render(stage.started)?;
    check("plugins_disabled", &stage, buffer)?;
    stage.send(key(KeyCode::Esc));
    let buffer = stage.render(stage.started)?;
    check("git_disabled_panels", &stage, buffer)
}

#[test]
fn git_and_plugin_windows_survive_tiny_terminals() -> Result<()> {
    for (cols, rows) in [(20, 8), (2, 2), (1, 1), (0, 0)] {
        let mut stage = stage(SIZE, Motion::STILL)?;
        let dir = stage.state.panels[0].dir().to_path_buf();
        let generation = stage.state.panels[0].generation();
        let repository = borg_git::Repository::parse(dir.clone(), b"## a-very-long-branch-name\0MM Cargo.toml\0")?;
        stage.send(Msg::GitStatus { side: borg_core::Side::Left, generation, dir, result: Ok(Some(repository)) });
        stage.send(Msg::Resize { cols, rows });
        stage.emulator.resize(GridSize::console(cols, rows));
        stage.render(stage.started)?;
        stage.state.overlay = Some(borg_core::Overlay::Plugins { selected: 0 });
        stage.render(stage.started)?;
    }
    Ok(())
}
