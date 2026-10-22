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
    Entry {
        source: None,
        missing: false,
        usage: None,
        name: name.into(),
        kind,
        size,
        modified: at(modified),
        executable,
        permissions: 0o644,
    }
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

    stage.send(key(KeyCode::Tab)).send(key(KeyCode::Char(' ')));
    let buffered = stage.render(stage.started)?;
    check("copy_dialog_hard_drive", &stage, buffered)?;

    stage.send(key(KeyCode::Esc)).send(key(KeyCode::F(8)));
    let delete = stage.render(stage.started)?;
    check("delete_dialog", &stage, delete)
}

#[test]
fn copy_dialog_keeps_shortcuts_visible_in_a_narrow_terminal() -> Result<()> {
    let mut stage = stage(NARROW, Motion::STILL)?;
    stage.send(key(KeyCode::Down)).send(key(KeyCode::F(5)));
    stage.send(key(KeyCode::Tab)).send(key(KeyCode::Char(' ')));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Tab · Space check · Enter · Esc")?;
    check("copy_dialog_hard_drive_narrow", &stage, buffer)
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
fn startup_intro_glitches_settles_and_gives_way_to_any_key() -> Result<()> {
    let mut stage = stage(SIZE, Motion::LOCAL)?;
    stage.state.intro = Some(stage.started);
    let buffer = stage.render(stage.started + Duration::from_millis(120))?;
    check("intro_glitch", &stage, buffer)?;
    let settled =
        Snapshot::from_buffer(stage.render(stage.started + Duration::from_millis(1000))?, &stage.palette, None);
    anyhow::ensure!(settled.contains("██████  ██    ██ ██████  ██   ███"), "the logo settles:\n{}", settled.text());
    stage.send(key(KeyCode::Down));
    anyhow::ensure!(stage.state.intro.is_none(), "a key dismisses the intro");
    anyhow::ensure!(stage.state.active_panel().cursor() == 1, "and still reaches the panels");
    stage.state.intro = Some(stage.started);
    stage.state.settings.preferences.screensaver = true;
    stage.state.settings.screensaver_after = Duration::from_secs(1);
    let ended = stage.started + borg_core::INTRO;
    borg_core::update(&mut stage.state, Msg::Tick, ended);
    anyhow::ensure!(stage.state.intro.is_none(), "the intro ends on its own");
    anyhow::ensure!(!stage.state.screensaver, "the screensaver does not replace the intro");
    borg_core::update(&mut stage.state, Msg::Tick, ended + Duration::from_millis(900));
    anyhow::ensure!(!stage.state.screensaver, "idle time counts from when the panels appear");
    borg_core::update(&mut stage.state, Msg::Tick, ended + Duration::from_secs(1));
    anyhow::ensure!(stage.state.screensaver, "and the screensaver follows after the idle period");
    Ok(())
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
    stage.state.saver = borg_core::Saver::Glitch;
    let calm = Snapshot::from_buffer(stage.render(later)?, &stage.palette, None);
    anyhow::ensure!(calm.contains("c o m m a n d e r"), "the glitch screensaver shows the logo");
    let burst = (0..120)
        .map(|tenth| later + Duration::from_millis(100 * tenth))
        .find(|&at| {
            let shot = Snapshot::from_buffer(stage.render(at).unwrap(), &stage.palette, None);
            shot.text().chars().filter(|c| matches!(c, '▓' | '▒' | '░')).count() >= 40
        })
        .ok_or_else(|| anyhow::anyhow!("the glitch screensaver must burst within 12 seconds"))?;
    let buffer = stage.render(burst)?;
    check("screensaver_glitch", &stage, buffer)?;

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
    let quick = Snapshot::from_buffer(stage.render(stage.started + Duration::from_millis(450))?, &stage.palette, None);
    anyhow::ensure!(!quick.contains("loading…"), "work shorter than the spinner delay shows no spinner");
    let buffer = stage.render(stage.started + Duration::from_millis(800))?;
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
    ensure_text(&buffer, "Find  o")?;
    ensure_text(&buffer, " 2/4 ")?;
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
        anyhow::ensure!(
            describe(&buffer)
                .lines()
                .nth(usize::from(stage.state.layout(stage.started).panels[0].y))
                .unwrap_or_default()
                .contains('╭')
        );
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
    ensure_text(&buffer, "Alt-J Copying 37%")?;
    let shot = Snapshot::from_buffer(buffer.clone(), &stage.palette, None);
    let (column, row) = shot.find("Alt-J Copying").unwrap();
    let click = crossterm::event::MouseEvent {
        column,
        row,
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        modifiers: KeyModifiers::NONE,
    };
    assert!(matches!(
        borg_ui::mouse_input(&stage.state, click, false, later),
        Some(Msg::Pointer(borg_core::Pointer::Action(borg_core::Action::Jobs)))
    ));
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
    ensure_text(&buffer, "Alt-J Waiting for you")?;
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
fn typed_multiline_commands_share_the_prompt_area_with_history() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    let before = stage.state.layout(stage.started).panels[0].height;
    stage.shell(b"\x1b[20;1H\x1b]133;A\x07$ \x1b]133;B\x07for file in *.log; do");
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)));
    stage.shell(b"\r\n    gzip \"$file\"\r\ndone");
    let buffer = stage.render(stage.started)?;
    assert!(stage.state.overlay.is_none());
    assert_eq!(stage.state.layout(stage.started).panels[0].height, before - 2);
    ensure_text(&buffer, "for file in *.log; do")?;
    ensure_text(&buffer, "done")?;
    check("inline_multiline_prompt", &stage, buffer)
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
    ensure_text(&buffer, "1Help")?;
    ensure_text(&buffer, "0Quit")?;
    check("function_alias_hint", &stage, buffer)?;
    let later = stage.started + std::time::Duration::from_secs(2);
    stage.send(Msg::Tick);
    let buffer = stage.render(later)?;
    ensure_text(&buffer, "F1Help")?;
    Ok(())
}

#[test]
fn hidden_2048_modal_themes_states_and_resize() -> Result<()> {
    use borg_core::{Game2048, Overlay, SavedGame2048};
    let saved = |cells| SavedGame2048 { version: 1, cells, score: 12340, best: 25600, random: 42, continued: false };
    let cells = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 0, 0, 0, 0, 0, 0];
    let mut stage = stage(GridSize { cols: 100, rows: 32 }, Motion::STILL)?;
    stage.send(key(KeyCode::F(1)));
    for digit in "2048".chars() {
        stage.send(key(KeyCode::Char(digit)));
    }
    assert!(matches!(stage.state.overlay, Some(Overlay::Game2048)));
    stage.state.game2048 = Game2048::restore(saved(cells));
    for theme in ["neon", "classic", "paper"] {
        select_theme(&mut stage, theme);
        let buffer = stage.render(stage.started)?;
        ensure_text(&buffer, "U Undo")?;
        ensure_text(&buffer, "1024")?;
        check(&format!("game2048_{theme}"), &stage, buffer)?;
    }
    select_theme(&mut stage, "neon");
    stage.state.game2048 = Game2048::restore(saved([11, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Enter to keep playing")?;
    check("game2048_won", &stage, buffer)?;
    stage.state.game2048 = Game2048::restore(saved([1, 2, 1, 2, 2, 1, 2, 1, 1, 2, 1, 2, 2, 1, 2, 1]));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "No moves")?;
    check("game2048_over", &stage, buffer)?;
    stage.state.game2048 = Game2048::restore(saved(cells));
    let before = stage.state.game2048.as_ref().unwrap().saved();
    stage.send(Msg::Resize { cols: NARROW.cols, rows: NARROW.rows });
    stage.emulator.resize(GridSize::console(NARROW.cols, NARROW.rows));
    let buffer = stage.render(stage.started)?;
    check("game2048_compact", &stage, buffer)?;
    for (cols, rows) in [(20, 5), (2, 2), (1, 1), (0, 0), (193, 40)] {
        stage.send(Msg::Resize { cols, rows });
        stage.emulator.resize(GridSize::console(cols, rows));
        stage.render(stage.started)?;
        assert_eq!(stage.state.game2048.as_ref().unwrap().saved(), before);
    }
    Ok(())
}

#[test]
fn all_overlays_survive_small_terminal_sizes() -> Result<()> {
    for (cols, rows) in [(193, 40), (64, 18), (20, 5), (2, 2), (1, 1), (0, 0)] {
        for code in [KeyCode::F(1), KeyCode::F(5), KeyCode::F(7), KeyCode::F(8), KeyCode::F(9)] {
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

fn collected_stage(size: GridSize) -> Result<Stage> {
    let mut stage = stage(size, Motion::STILL)?;
    let mut first = entry("config.toml", EntryKind::File, 1024, RECENT, false);
    first.source = Some("/workspace/api/config.toml".into());
    let mut second = entry("config.toml", EntryKind::File, 4096, OLD, false);
    second.source = Some("/workspace/web/config.toml".into());
    let mut missing = entry("draft.md", EntryKind::File, 0, OLD, false);
    missing.source = Some("/workspace/archive/draft.md".into());
    missing.missing = true;
    stage.state.result_history[0] = Some(stage.state.panels[0].clone());
    stage.state.panels[0].open_results(
        borg_core::ResultSource::Collection("Release bundle".into()),
        vec![first, second, missing],
        50,
    );
    stage.send(Msg::Resize { cols: size.cols, rows: size.rows });
    stage.state.panels[0].toggle_mark();
    Ok(stage)
}

#[test]
fn virtual_collection_expanded_and_grouped() -> Result<()> {
    let mut stage = collected_stage(GridSize { cols: 132, rows: 28 })?;
    stage.state.panels[0].results.as_mut().unwrap().expanded = true;
    let buffer = stage.render(stage.started)?;
    check("collection_expanded", &stage, buffer)?;
    stage.state.panels[0].group_results(borg_core::ResultGroup::Folder);
    let buffer = stage.render(stage.started)?;
    check("collection_grouped", &stage, buffer)?;
    stage.state.panels[0].filter_results("/web/".into());
    let buffer = stage.render(stage.started)?;
    check("collection_filtered_hidden_marks", &stage, buffer)
}

#[test]
fn virtual_collection_narrow_and_split() -> Result<()> {
    let mut stage = collected_stage(NARROW)?;
    let buffer = stage.render(stage.started)?;
    check("collection_narrow", &stage, buffer)?;
    stage.send(Msg::Resize { cols: SIZE.cols, rows: SIZE.rows });
    stage.state.panels[0].results.as_mut().unwrap().expanded = false;
    let buffer = stage.render(stage.started)?;
    check("collection_split", &stage, buffer)
}

#[test]
fn recursive_search_partial_and_cancelled() -> Result<()> {
    let mut stage = collected_stage(SIZE)?;
    let entries = stage.state.panels[0].entries().to_vec();
    stage.state.panels[0].open_results(
        borg_core::ResultSource::Search {
            request: borg_core::SearchRequest {
                root: "/workspace".into(),
                query: "*.toml".into(),
                hidden: false,
                options: Default::default(),
            },
            progress: borg_core::SearchProgress {
                visited: 1234,
                errors: 2,
                skipped: 4,
                limited: false,
                cancelled: false,
                ..Default::default()
            },
        },
        entries,
        51,
    );
    stage.state.panels[0].begin_search(51);
    let buffer = stage.render(stage.started)?;
    check("search_in_progress", &stage, buffer)?;
    stage.send(Msg::Pointer(borg_core::Pointer::Action(borg_core::Action::CancelSearch)));
    let buffer = stage.render(stage.started)?;
    check("search_cancelled", &stage, buffer)
}

#[test]
fn results_panel_resizes_without_buffer_overrun() -> Result<()> {
    let mut stage = collected_stage(SIZE)?;
    for (cols, rows) in [(1, 1), (4, 3), (20, 8), (80, 24), (193, 40)] {
        stage.send(Msg::Resize { cols, rows });
        stage.render(stage.started)?;
    }
    Ok(())
}

#[test]
fn mouse_sort_headers_and_group_rows_match_results_geometry() -> Result<()> {
    use borg_core::{Pointer, Side, SortKey};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut stage = collected_stage(GridSize { cols: 132, rows: 28 })?;
    let click = |x, y| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    let buffer = stage.render(stage.started)?;
    let screen = Snapshot::from_buffer(buffer, &stage.palette, None);
    let (x, y) = screen.find("Size").unwrap();
    let hit = borg_ui::mouse_input(&stage.state, click(x, y), false, stage.started).unwrap();
    assert!(matches!(hit, Msg::Pointer(Pointer::ResultSort { side: Side::Left, key: SortKey::Size })));
    stage.send(hit);
    assert_eq!(stage.state.active_panel().sort().key, SortKey::Size);
    assert!(stage.state.active_panel().sort().reverse);
    stage.state.panels[0].group_results(borg_core::ResultGroup::Folder);
    let before = stage.state.active_panel().len();
    let buffer = stage.render(stage.started)?;
    let screen = Snapshot::from_buffer(buffer, &stage.palette, None);
    let (x, y) = screen.find("/workspace/api · 1").unwrap();
    let hit = borg_ui::mouse_input(&stage.state, click(x, y), true, stage.started).unwrap();
    stage.send(hit);
    assert_eq!(stage.state.active_panel().len(), before - 1);
    stage.send(ctrl('q'));
    let preview = stage.state.layout(stage.started).panels[1];
    let scroll = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: preview.x + 1,
        row: preview.y + 1,
        modifiers: KeyModifiers::NONE,
    };
    assert!(matches!(
        borg_ui::mouse_input(&stage.state, scroll, false, stage.started),
        Some(Msg::Pointer(Pointer::ScrollDocument(_)))
    ));
    Ok(())
}

#[test]
fn discovery_windows_and_folder_totals() -> Result<()> {
    use borg_core::{
        Action, Collection, Pointer, ResultSource, SearchKind, SearchOptions, SearchProgress, SearchRequest,
    };
    let mut stage = collected_stage(GridSize { cols: 132, rows: 28 })?;
    stage.state.collections = (0..24)
        .map(|index| Collection {
            name: format!("Workspace bundle {index:02}"),
            entries: stage.state.panels[0].entries().to_vec(),
        })
        .collect();
    stage.send(Msg::Pointer(Pointer::Action(Action::OpenCollection)));
    stage.send(key(KeyCode::End));
    let buffer = stage.render(stage.started)?;
    check("collections_chooser", &stage, buffer)?;
    stage.send(key(KeyCode::F(7)));
    stage.send(Msg::Paste("Release notes".into()));
    let buffer = stage.render(stage.started)?;
    check("collection_create", &stage, buffer)?;
    stage.send(key(KeyCode::Esc)).send(key(KeyCode::Esc));
    stage.send(Msg::Pointer(Pointer::Action(Action::AnalyzeSpace)));
    let buffer = stage.render(stage.started)?;
    check("space_search_dialog", &stage, buffer)?;
    stage.send(key(KeyCode::Esc));
    let mut cache = entry("node_modules", EntryKind::Dir, 812_045_312, RECENT, false);
    cache.source = Some("/workspace/website/node_modules".into());
    cache.usage = Some(borg_vfs::FolderUsage { files: 72_419, folders: 3_128, partial: false, ..Default::default() });
    let mut build = entry("target", EntryKind::Dir, 4_521_148_416, RECENT, false);
    build.source = Some("/workspace/service/target".into());
    build.usage = Some(borg_vfs::FolderUsage { files: 12_804, folders: 294, partial: false, ..Default::default() });
    let mut partial = entry("cache", EntryKind::Dir, 123_456_789, RECENT, false);
    partial.source = Some("/workspace/archive/cache".into());
    partial.usage = Some(borg_vfs::FolderUsage { files: 9_240, folders: 129, partial: true, ..Default::default() });
    stage.state.panels[0].open_results(
        ResultSource::Search {
            request: SearchRequest {
                root: "/workspace".into(),
                query: "*".into(),
                hidden: true,
                options: SearchOptions { kind: SearchKind::FolderTotals, ..Default::default() },
            },
            progress: SearchProgress { measured: at(RECENT), visited: 99_104, errors: 1, ..Default::default() },
        },
        vec![cache, build, partial],
        80,
    );
    stage.state.panels[0].set_sort(borg_core::Sort { key: borg_core::SortKey::Size, reverse: true });
    stage.state.panels[0].results.as_mut().unwrap().expanded = true;
    let buffer = stage.render(stage.started)?;
    check("folder_sizes", &stage, buffer)?;
    stage.state.panels[0].cache_observations();
    let buffer = stage.render(stage.started)?;
    check("folder_sizes_cached", &stage, buffer)?;
    stage.send(key(KeyCode::F(8)));
    let buffer = stage.render(stage.started)?;
    check("folder_delete_confirm", &stage, buffer)?;
    stage.send(key(KeyCode::Esc)).send(key(KeyCode::F(10)));
    let buffer = stage.render(stage.started)?;
    check("quit_confirm", &stage, buffer)
}

#[test]
fn discovery_mouse_targets_and_tiny_windows() -> Result<()> {
    use borg_core::{Action, Overlay, Pointer};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut stage = collected_stage(GridSize { cols: 132, rows: 28 })?;
    let click = |column, row| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let bar = stage.state.layout(stage.started).keybar;
    assert!(matches!(
        borg_ui::mouse_input(&stage.state, click(bar.x, bar.y + 1), false, stage.started),
        Some(Msg::Pointer(Pointer::Action(Action::Search)))
    ));
    stage.send(Msg::Pointer(Pointer::Action(Action::Quit)));
    let buffer = stage.render(stage.started)?;
    let screen = Snapshot::from_buffer(buffer, &stage.palette, None);
    let (x, y) = screen.find("[ No ]").unwrap();
    assert!(matches!(
        borg_ui::mouse_input(&stage.state, click(x, y), false, stage.started),
        Some(Msg::Pointer(Pointer::Confirm(false)))
    ));
    stage.send(key(KeyCode::Esc));
    stage.state.collections.push(borg_core::Collection { name: "Bundle".into(), entries: Vec::new() });
    stage.send(Msg::Pointer(Pointer::Action(Action::OpenCollection)));
    let buffer = stage.render(stage.started)?;
    let screen = Snapshot::from_buffer(buffer, &stage.palette, None);
    let (x, y) = screen.find("F7: New collection").unwrap();
    assert!(matches!(
        borg_ui::mouse_input(&stage.state, click(x, y), false, stage.started),
        Some(Msg::Pointer(Pointer::NewCollection))
    ));
    stage.send(key(KeyCode::Esc));
    for (cols, rows) in [(1, 1), (4, 3), (20, 8), (44, 12), (193, 40)] {
        stage.send(Msg::Resize { cols, rows });
        stage.emulator.resize(GridSize::console(cols, rows));
        stage.send(Msg::Pointer(Pointer::Action(Action::AnalyzeSpace)));
        stage.render(stage.started)?;
        assert!(matches!(stage.state.overlay, Some(Overlay::Search(_))));
        stage.send(key(KeyCode::Esc));
        stage.send(Msg::Pointer(Pointer::Action(Action::OpenCollection)));
        stage.render(stage.started)?;
        stage.send(key(KeyCode::Esc));
        stage.state.overlay = None;
    }
    Ok(())
}

#[test]
fn search_controls_have_distinct_boundaries_and_labels_in_each_theme() -> Result<()> {
    use borg_core::{Action, Pointer};
    for theme in ["neon", "classic", "midnight"] {
        let mut stage = stage(GridSize { cols: 110, rows: 26 }, Motion::STILL)?;
        select_theme(&mut stage, theme);
        stage.send(Msg::Pointer(Pointer::Action(Action::Search)));
        stage
            .send(key(KeyCode::Tab))
            .send(key(KeyCode::Tab))
            .send(key(KeyCode::Tab))
            .send(key(KeyCode::Tab))
            .send(key(KeyCode::Tab));
        let buffer = stage.render(stage.started)?;
        let screen = Snapshot::from_buffer(buffer.clone(), &stage.palette, None);
        let (x, y) = screen.find("Name or glob").unwrap();
        let row = &screen.lines()[usize::from(y)];
        let field =
            row.chars().enumerate().skip(usize::from(x) + "Name or glob".len()).find(|(_, c)| *c == '[').unwrap().0
                as u16;
        assert_ne!(screen.style_at(x, y).bg, screen.style_at(field + 2, y).bg, "{theme}: field blends into label");
        assert!(row.chars().skip(usize::from(field)).any(|c| c == ']'));
        assert!(!screen.contains("guaranteed freed space"));
        assert!(!screen.contains("Folder totals find"));
        assert!(!screen.contains("folder totals only"));
        check(&format!("search_fields_{theme}"), &stage, buffer)?;
        let (x, y) = screen.find("[ Enter: Search ]").unwrap();
        let event = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        };
        let hit = borg_ui::mouse_input(&stage.state, event, false, stage.started).unwrap();
        assert!(matches!(hit, Msg::Pointer(Pointer::SearchStart)));
        stage.send(hit);
        assert!(stage.state.overlay.is_none());
        assert!(stage.state.active_panel().is_results());
    }
    Ok(())
}

#[test]
fn two_row_footer_keeps_actions_visible_and_mouse_targets_on_their_rows() -> Result<()> {
    use borg_core::{Action, Pointer};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut stage = stage(GridSize { cols: 132, rows: 28 }, Motion::STILL)?;
    let footer = stage.state.layout(stage.started).keybar;
    let click = |column, row| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let buffer = stage.render(stage.started)?;
    let screen = Snapshot::from_buffer(buffer.clone(), &stage.palette, None);
    assert_eq!(footer.height, 2);
    assert_eq!(footer.bottom(), stage.state.rows);
    assert!(screen.lines()[usize::from(footer.y)].contains("F1Help"));
    assert!(screen.lines()[usize::from(footer.y + 1)].starts_with("Alt-I Count sizes"));
    assert!(!screen.lines()[0].contains("Ctrl-F Search"));
    let (x, y) = screen.find("Ctrl-F Search").unwrap();
    assert!(matches!(
        borg_ui::mouse_input(&stage.state, click(x, y), false, stage.started),
        Some(Msg::Pointer(Pointer::Action(Action::Search)))
    ));
    check("two_row_shortcuts", &stage, buffer)?;
    stage.state.notice = Some("Copied 1 file".into());
    let buffer = stage.render(stage.started)?;
    let screen = Snapshot::from_buffer(buffer.clone(), &stage.palette, None);
    let notice = &screen.lines()[usize::from(footer.y + 1)];
    assert_eq!(notice.split("◈ Borg Commander").next().unwrap().trim(), "Copied 1 file");
    let (x, y) = screen.find("F10Quit").unwrap();
    assert!(matches!(
        borg_ui::mouse_input(&stage.state, click(x, y), false, stage.started),
        Some(Msg::Pointer(Pointer::Action(Action::Quit)))
    ));
    assert!(matches!(
        borg_ui::mouse_input(&stage.state, click(footer.x, footer.y + 1), false, stage.started),
        Some(Msg::Pointer(Pointer::DismissNotice))
    ));
    check("two_row_shortcuts_notice", &stage, buffer)
}

#[test]
fn result_title_controls_maximize_restore_and_fold_the_clicked_pane() -> Result<()> {
    use borg_core::{Action, Pointer, Side};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut stage = collected_stage(GridSize { cols: 132, rows: 28 })?;
    let click = |column, row| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let path = stage.state.active_panel().hovered_entry().unwrap().identity().to_path_buf();
    let buffer = stage.render(stage.started)?;
    let screen = Snapshot::from_buffer(buffer.clone(), &stage.palette, None);
    assert!(stage.state.layout(stage.started).panels.iter().all(|rect| !rect.is_empty()));
    let (x, y) = screen.find("[+]").unwrap();
    let hit = borg_ui::mouse_input(&stage.state, click(x, y), false, stage.started).unwrap();
    assert!(matches!(
        hit,
        Msg::Pointer(Pointer::ResultControl { side: Side::Left, action: Action::SetExpanded(true) })
    ));
    check("results_default_pane", &stage, buffer)?;
    stage.send(hit);
    let buffer = stage.render(stage.started)?;
    let screen = Snapshot::from_buffer(buffer.clone(), &stage.palette, None);
    assert!(stage.state.layout(stage.started).panels[1].is_empty());
    let (x, y) = screen.find("[=]").unwrap();
    let restore = borg_ui::mouse_input(&stage.state, click(x, y), false, stage.started).unwrap();
    check("results_maximized_controls", &stage, buffer)?;
    stage.send(restore);
    assert!(!stage.state.layout(stage.started).panels[1].is_empty());
    stage.state.active = Side::Right;
    let buffer = stage.render(stage.started)?;
    let screen = Snapshot::from_buffer(buffer, &stage.palette, None);
    let (x, y) = screen.find("[-]").unwrap();
    let hit = borg_ui::mouse_input(&stage.state, click(x, y), false, stage.started).unwrap();
    stage.send(hit);
    assert_eq!(stage.state.active, Side::Left);
    assert!(!stage.state.active_panel().is_results());
    stage.send(Msg::Pointer(Pointer::Action(Action::BackResults)));
    assert!(stage.state.active_panel().is_results());
    assert_eq!(stage.state.active_panel().hovered_entry().unwrap().identity(), path);
    assert_eq!(stage.state.active_panel().marked_count(), 1);
    Ok(())
}

fn size_tree_stage() -> Result<Stage> {
    use borg_core::{ResultSource, SearchKind, SearchOptions, SearchProgress, SearchRequest, Sort, SortKey};
    let mut stage = stage(GridSize { cols: 132, rows: 28 }, Motion::STILL)?;
    stage.state.settings.preferences.nerd_font = true;
    let theme = stage.state.appearance.choices().iter().position(|c| c.id == "neon").unwrap();
    stage.state.appearance.select(theme, stage.started, Duration::ZERO);
    let entries = [
        ("/workspace/project", 6_301_000_000, 84_310, true),
        ("/workspace/project/target", 4_600_000_000, 51_200, false),
        ("/workspace/project/target/debug", 4_200_000_000, 48_000, false),
        ("/workspace/project/target/debug/deps", 3_100_000_000, 30_000, false),
        ("/workspace/project/target/release", 400_000_000, 3_200, false),
        ("/workspace/project/assets", 1_700_000_000, 33_000, true),
        ("/workspace/project/src", 1_000_000, 110, false),
    ]
    .into_iter()
    .map(|(path, bytes, files, partial)| {
        let path = PathBuf::from(path);
        let mut e = entry(path.file_name().unwrap().to_str().unwrap(), EntryKind::Dir, bytes, RECENT, false);
        e.source = Some(path);
        let folders = match e.identity().to_str().unwrap() {
            "/workspace/project" => 6,
            "/workspace/project/target" => 3,
            "/workspace/project/target/debug" => 1,
            _ => 0,
        };
        e.usage = Some(borg_vfs::FolderUsage {
            files,
            folders,
            partial,
            measured: at(RECENT),
            direct_files: if e.identity().ends_with("debug") { 18_000 } else { 0 },
            direct_bytes: if e.identity().ends_with("debug") { 1_100_000_000 } else { 0 },
            ..Default::default()
        });
        e
    })
    .collect();
    stage.state.panels[0].open_results(
        ResultSource::Search {
            request: SearchRequest {
                root: "/workspace/project".into(),
                query: "*".into(),
                hidden: true,
                options: SearchOptions { kind: SearchKind::FolderTree, ..Default::default() },
            },
            progress: SearchProgress { measured: at(RECENT), visited: 84_422, errors: 1, ..Default::default() },
        },
        entries,
        80,
    );
    stage.state.panels[0].set_sort(Sort { key: SortKey::Size, reverse: true });
    stage.state.panels[0].move_by(1);
    stage.send(key(KeyCode::Right));
    Ok(stage)
}

#[test]
fn size_tree_fresh_cached_filtered_and_narrow() -> Result<()> {
    let mut stage = size_tree_stage()?;
    let buffer = stage.render(stage.started)?;
    check("size_tree", &stage, buffer)?;
    stage.state.panels[0].cache_observations();
    stage.send(key(KeyCode::Right)).send(key(KeyCode::Right));
    stage.send(key(KeyCode::Insert)).send(key(KeyCode::Left)).send(key(KeyCode::Left));
    let buffer = stage.render(stage.started)?;
    check("size_tree_cached", &stage, buffer)?;
    stage.state.panels[0].filter_results("deps".into());
    let buffer = stage.render(stage.started)?;
    check("size_tree_filtered", &stage, buffer)?;
    stage.state.panels[0].filter_results(String::new());
    stage.send(Msg::Resize { cols: 44, rows: 12 });
    let buffer = stage.render(stage.started)?;
    check("size_tree_narrow", &stage, buffer)?;
    for (cols, rows) in [(0, 0), (1, 1), (8, 4), (20, 6), (193, 40)] {
        stage.send(Msg::Resize { cols, rows });
        stage.render(stage.started)?;
    }
    Ok(())
}

#[test]
fn size_tree_arrows_and_column_headers_share_mouse_geometry() -> Result<()> {
    use borg_core::{Pointer, Side, SortKey};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut stage = size_tree_stage()?;
    let buffer = stage.render(stage.started)?;
    let row = (0..buffer.area.height)
        .find(|&y| (0..buffer.area.width).any(|x| buffer[(x, y)].symbol() == "▾" && x > 4))
        .unwrap();
    let column = (3..buffer.area.width).find(|&x| buffer[(x, row)].symbol() == "▾").unwrap();
    let event =
        MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE };
    let message = borg_ui::mouse_input(&stage.state, event, false, stage.started).unwrap();
    assert!(matches!(message, Msg::Pointer(Pointer::TreeNode { side: Side::Left, index: 1 })));
    stage.send(message);
    assert_eq!(stage.state.panels[0].len(), 4);
    let row =
        (0..buffer.area.height).find(|&y| (0..buffer.area.width).any(|x| buffer[(x, y)].symbol() == "↓")).unwrap();
    let column = (0..buffer.area.width).find(|&x| buffer[(x, row)].symbol() == "↓").unwrap();
    let message =
        borg_ui::mouse_input(&stage.state, MouseEvent { column, row, ..event }, false, stage.started).unwrap();
    assert!(matches!(message, Msg::Pointer(Pointer::ResultSort { side: Side::Left, key: SortKey::Size })));
    Ok(())
}

#[test]
fn size_tree_loading_and_cached_folder_listing() -> Result<()> {
    use borg_core::{ListingMode, Panel, ResultSource, SearchProgress, Side};
    let mut stage = size_tree_stage()?;
    let mut root = stage.state.panels[0]
        .entries()
        .iter()
        .find(|e| e.identity() == std::path::Path::new("/workspace/project"))
        .unwrap()
        .clone();
    root.usage = None;
    let ResultSource::Search { request, .. } = stage.state.panels[0].results.as_ref().unwrap().source.clone() else {
        unreachable!()
    };
    stage.state.panels[0].open_results(
        ResultSource::Search { request, progress: SearchProgress { visited: 100, ..Default::default() } },
        vec![root],
        81,
    );
    stage.state.panels[0].begin_search(81);
    let buffer = stage.render(stage.started)?;
    check("size_tree_loading", &stage, buffer)?;
    let mut entries = project();
    entries[0].size = 2_000_000;
    entries[0].usage = Some(borg_vfs::FolderUsage {
        files: 120,
        folders: 8,
        cached: true,
        measured: at(RECENT),
        ..Default::default()
    });
    stage.state.panels[0] = Panel::new("/home/me/projects/borg".into(), 20, ListingMode::Full, Default::default());
    let generation = stage.state.panels[0].generation();
    stage.send(Msg::Batch { side: Side::Left, generation, entries });
    stage.send(Msg::Listed { side: Side::Left, generation, error: None });
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down));
    let buffer = stage.render(stage.started)?;
    check("folder_sizes_cached_listing", &stage, buffer)
}

fn activity_stage() -> Result<Stage> {
    use borg_core::{ActivityData, ActivityRequest, ProcessInfo, ProcessKey, SystemSample};
    let mut stage = stage(GridSize { cols: 132, rows: 34 }, Motion::STILL)?;
    let theme = stage.state.appearance.choices().iter().position(|c| c.id == "neon").unwrap();
    stage.state.appearance.select(theme, stage.started, Duration::ZERO);
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT)));
    let base = [
        (410, "compiler", 2_600_000_000, 32000, None),
        (411, "language-server", 812_000_000, 8400, Some(410)),
        (412, "database", 644_000_000, 1100, None),
        (413, "borg", 42_000_000, 120, None),
        (414, "shell", 8_000_000, 0, Some(413)),
    ];
    for tick in 0..10 {
        let id = stage.state.activity.pending.as_ref().unwrap().0;
        let processes = base
            .iter()
            .map(|&(pid, name, ram, cpu, parent)| ProcessInfo {
                key: ProcessKey { pid, start: 100 },
                name: name.into(),
                parent,
                executable: Some(format!("/usr/local/bin/{name}").into()),
                resident: Some(ram + tick * 8_000_000),
                virtual_bytes: Some(ram * 3),
                cpu: Some(cpu),
                read: Some(1_200_000),
                written: Some(64_000),
            })
            .collect();
        stage.send(Msg::Activity {
            id,
            result: Ok(ActivityData::Sample(SystemSample {
                processes,
                total: 32_000_000_000,
                used: 18_000_000_000,
                swap_total: 4_000_000_000,
                swap_used: 256_000_000,
                cpu: vec![7500, 4400, 2100, 1200],
            })),
        });
        if tick < 9 {
            let next = stage.state.activity.pending.as_ref().map(|p| p.0).unwrap_or(id + 1);
            stage.state.activity.pending = Some((next, ActivityRequest::Sample));
        }
    }
    Ok(stage)
}

#[test]
fn activity_process_memory_map_snapshots_and_permissions() -> Result<()> {
    use borg_core::{ActivityData, ActivityPage, ActivityRequest, MemoryRegion, MemorySnapshot};
    let mut stage = activity_stage()?;
    let buffer = stage.render(stage.started)?;
    let event = crossterm::event::MouseEvent {
        column: 2,
        row: 0,
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        modifiers: KeyModifiers::NONE,
    };
    let msg = borg_ui::mouse_input(&stage.state, event, false, stage.started).unwrap();
    assert!(matches!(
        msg,
        Msg::Pointer(borg_core::Pointer::Action(borg_core::Action::OpenMenuBar) | borg_core::Pointer::Menu(_))
    ));
    check("activity_processes", &stage, buffer)?;
    stage.send(key(KeyCode::Char('/'))).send(key(KeyCode::Char('o')));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Esc clear")?;
    check("activity_filter_typing", &stage, buffer)?;
    stage.send(key(KeyCode::Enter));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "/ edit")?;
    check("activity_filter_applied", &stage, buffer)?;
    stage.send(key(KeyCode::Esc));
    stage.send(key(KeyCode::Tab));
    let buffer = stage.render(stage.started)?;
    check("activity_memory", &stage, buffer)?;
    let neon = stage.state.appearance.current();
    let ember = stage.state.appearance.choices().iter().position(|c| c.id == "ember").unwrap();
    stage.state.appearance.select(ember, stage.started, Duration::ZERO);
    let buffer = stage.render(stage.started)?;
    check("activity_memory_ember", &stage, buffer)?;
    stage.state.appearance.select(neon, stage.started, Duration::ZERO);
    stage.send(key(KeyCode::Enter));
    let id = stage.state.activity.pending.as_ref().unwrap().0;
    let process = stage.state.activity.selected.unwrap();
    let region = MemoryRegion {
        start: 0x100000000,
        end: 0x100040000,
        permissions: "rw-".into(),
        name: "Heap · application buffers".into(),
        resident: Some(163840),
        private: Some(163840),
        shared: Some(0),
        swap: Some(0),
        ..Default::default()
    };
    let mapped =
        |start: u64, size: u64, permissions: &str, kind: &str, path: Option<&str>, resident, dirty| MemoryRegion {
            start,
            end: start + size,
            permissions: permissions.into(),
            name: path.map_or(kind.into(), |p| format!("{kind} · {p}")),
            resident: Some(resident),
            swap: Some(0),
            dirty: Some(dirty),
            path: path.map(Into::into),
            ..Default::default()
        };
    let libsystem = Some("/usr/lib/libSystem.B.dylib");
    stage.send(Msg::Activity {
        id,
        result: Ok(ActivityData::Regions(vec![
            region.clone(),
            mapped(0x100040000, 0x400000, "rw-", "MALLOC_SMALL · DefaultMallocZone", None, 98304, 65536),
            mapped(0x100440000, 0x40000, "r-x", "__TEXT", Some("/usr/local/bin/compiler"), 131072, 0),
            mapped(0x100480000, 0x8000, "rw-", "__DATA", Some("/usr/local/bin/compiler"), 32768, 16384),
            mapped(0x16f000000, 0x800000, "rw-", "Stack · thread 0", None, 131072, 131072),
            mapped(0x16f800000, 0x4000, "---", "STACK GUARD · stack guard for thread 1", None, 0, 0),
            mapped(0x180000000, 0x400000, "r-x", "__TEXT", libsystem, 2097152, 0),
            mapped(0x180400000, 0x10000, "rw-", "__DATA", libsystem, 65536, 16384),
            mapped(0x180500000, 0x80000, "r-x", "__TEXT", Some("/usr/lib/libobjc.A.dylib"), 524288, 0),
            mapped(0x190000000, 0x40000000, "r--", "mapped file", Some("/System/Library/Fonts/SFNS.ttf"), 1048576, 0),
        ])),
    });
    let buffer = stage.render(stage.started)?;
    check("activity_map", &stage, buffer)?;
    let neon = stage.state.appearance.current();
    let classic = stage.state.appearance.choices().iter().position(|c| c.id == "classic").unwrap();
    stage.state.appearance.select(classic, stage.started, Duration::ZERO);
    let buffer = stage.render(stage.started)?;
    check("activity_map_classic", &stage, buffer)?;
    stage.state.appearance.select(neon, stage.started, Duration::ZERO);
    for code in [KeyCode::Down, KeyCode::Down, KeyCode::Right] {
        stage.send(key(code));
    }
    let buffer = stage.render(stage.started)?;
    check("activity_map_libraries", &stage, buffer)?;
    for code in [KeyCode::Left, KeyCode::Left, KeyCode::Home] {
        stage.send(key(code));
    }
    stage.send(key(KeyCode::Enter));
    stage.send(key(KeyCode::Enter));
    let id = stage.state.activity.pending.as_ref().unwrap().0;
    let bytes = b"Borg Commander\0user preferences\0theme=neon\n".repeat(16);
    stage.send(Msg::Activity {
        id,
        result: Ok(ActivityData::Snapshot(MemorySnapshot {
            process,
            address: region.start,
            bytes: bytes.clone(),
            captured: 123,
        })),
    });
    let buffer = stage.render(stage.started)?;
    check("activity_hex", &stage, buffer)?;
    stage.send(key(KeyCode::Char('h')));
    let buffer = stage.render(stage.started)?;
    check("activity_text", &stage, buffer)?;
    stage.send(key(KeyCode::Char('h'))).send(key(KeyCode::Char('r')));
    let id = stage.state.activity.pending.as_ref().unwrap().0;
    let mut changed = bytes;
    changed[5..14].copy_from_slice(b"Changed!!");
    stage.send(Msg::Activity {
        id,
        result: Ok(ActivityData::Snapshot(MemorySnapshot {
            process,
            address: region.start,
            bytes: changed,
            captured: 124,
        })),
    });
    stage.send(key(KeyCode::Char('d')));
    let buffer = stage.render(stage.started)?;
    check("activity_diff", &stage, buffer)?;
    stage.send(key(KeyCode::Esc));
    stage.state.activity.pending = Some((100, ActivityRequest::Regions { process, admin: false }));
    stage.send(Msg::Activity { id: 100, result: Err("Process protected by macOS".into()) });
    let buffer = stage.render(stage.started)?;
    check("activity_protected", &stage, buffer)?;
    for page in [ActivityPage::Processes, ActivityPage::Memory, ActivityPage::Regions, ActivityPage::Snapshot] {
        stage.state.activity.page = page;
        for (cols, rows) in [(0, 0), (1, 1), (12, 6), (44, 14), (193, 40)] {
            stage.send(Msg::Resize { cols, rows });
            stage.render(stage.started)?;
        }
    }
    Ok(())
}

#[test]
fn activity_files_tabs_and_scrolled_regions_keep_selection_visible() -> Result<()> {
    use borg_core::{ActivityData, MemoryRegion, Pointer, ProcessFile, ProcessFiles};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut stage = activity_stage()?;
    stage.send(key(KeyCode::Char('2')));
    let id = stage.state.activity.pending.as_ref().unwrap().0;
    stage.send(Msg::Activity {
        id,
        result: Ok(ActivityData::Files(ProcessFiles {
            entries: vec![
                ProcessFile {
                    descriptor: "3".into(),
                    access: "rw".into(),
                    kind: "REG".into(),
                    name: "/workspace/demo/session.log".into(),
                    size: Some(8192),
                    offset: Some(512),
                },
                ProcessFile {
                    descriptor: "4".into(),
                    access: "rw".into(),
                    kind: "IPv4".into(),
                    name: "127.0.0.1:8080 (LISTEN)".into(),
                    size: None,
                    offset: None,
                },
                ProcessFile {
                    descriptor: "cwd".into(),
                    access: String::new(),
                    kind: "DIR".into(),
                    name: "/workspace/demo".into(),
                    size: None,
                    offset: None,
                },
            ],
            skipped: 0,
        })),
    });
    let buffer = stage.render(stage.started)?;
    check("activity_files", &stage, buffer)?;
    let shot = Snapshot::from_buffer(stage.render(stage.started)?, &stage.palette, None);
    let (column, row) = shot.find("1 Memory").unwrap();
    let event =
        MouseEvent { column, row, kind: MouseEventKind::Down(MouseButton::Left), modifiers: KeyModifiers::NONE };
    let msg = borg_ui::mouse_input(&stage.state, event, false, stage.started).unwrap();
    stage.send(msg);
    let id = stage.state.activity.pending.as_ref().unwrap().0;
    stage.send(Msg::Activity {
        id,
        result: Ok(ActivityData::Regions(
            (0..60)
                .map(|i| MemoryRegion {
                    start: 0x1000 + i * 0x1000,
                    end: 0x2000 + i * 0x1000,
                    permissions: "rw-".into(),
                    name: format!("Heap allocation {i:03}"),
                    resident: Some(1024),
                    private: None,
                    shared: None,
                    swap: None,
                    ..Default::default()
                })
                .collect(),
        )),
    });
    stage.send(key(KeyCode::Char('g'))).send(key(KeyCode::End));
    let before = Snapshot::from_buffer(stage.render(stage.started)?, &stage.palette, None);
    let list_row = |shot: &Snapshot, name: &str| {
        shot.lines()
            .iter()
            .position(|line| line.chars().take(usize::from(shot.cols() / 2)).collect::<String>().contains(name))
            .unwrap()
    };
    let bottom = list_row(&before, "Heap allocation 059");
    stage.send(key(KeyCode::Up));
    let buffer = stage.render(stage.started)?;
    let after = Snapshot::from_buffer(buffer.clone(), &stage.palette, None);
    assert_eq!(list_row(&after, "Heap allocation 058"), bottom - 1);
    check("activity_scrolled_regions", &stage, buffer)?;
    let (column, row) = after.find("Heap allocation 057").unwrap();
    let event =
        MouseEvent { column, row, kind: MouseEventKind::Down(MouseButton::Left), modifiers: KeyModifiers::NONE };
    let msg = borg_ui::mouse_input(&stage.state, event, false, stage.started).unwrap();
    assert!(matches!(msg, Msg::Pointer(Pointer::ActivityRow { index: 57, .. })));
    stage.send(msg);
    assert_eq!(stage.state.activity.selected_region().unwrap().name, "Heap allocation 057");
    stage.send(key(KeyCode::Char('2')));
    assert_eq!(stage.state.activity.files[0].descriptor, "3");
    for (cols, rows) in [(0, 0), (1, 1), (12, 6), (44, 14)] {
        stage.send(Msg::Resize { cols, rows });
        stage.render(stage.started)?;
    }
    Ok(())
}

#[test]
fn activity_narrow_layout_mouse_sort_and_selection() -> Result<()> {
    use borg_core::{ActivitySort, Pointer};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut stage = activity_stage()?;
    let buffer = stage.render(stage.started)?;
    let shot = Snapshot::from_buffer(buffer, &stage.palette, None);
    let (column, row) = shot.find("RAM ↓").unwrap();
    let event =
        MouseEvent { column, row, kind: MouseEventKind::Down(MouseButton::Left), modifiers: KeyModifiers::NONE };
    let msg = borg_ui::mouse_input(&stage.state, event, false, stage.started).unwrap();
    assert!(matches!(msg, Msg::Pointer(Pointer::ActivitySort(ActivitySort::Memory))));
    stage.send(msg);
    assert!(!stage.state.activity.reverse);
    let buffer = stage.render(stage.started)?;
    let shot = Snapshot::from_buffer(buffer, &stage.palette, None);
    let (column, row) = shot.find("language-server").unwrap();
    let msg = borg_ui::mouse_input(&stage.state, MouseEvent { column, row, ..event }, true, stage.started).unwrap();
    stage.send(msg);
    assert_eq!(stage.state.activity.process().unwrap().name, "language-server");
    stage.send(key(KeyCode::Esc));
    stage.send(Msg::Resize { cols: 44, rows: 14 });
    let buffer = stage.render(stage.started)?;
    check("activity_narrow", &stage, buffer)
}

#[test]
fn borg_brand_is_visible_in_panels_console_viewer_and_overlays() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    let verify = |stage: &Stage, buffer: Buffer| {
        let shot = Snapshot::from_buffer(buffer, &stage.palette, None);
        assert!(shot.lines().last().unwrap().contains("Borg Commander"));
    };
    let buffer = stage.render(stage.started)?;
    verify(&stage, buffer);
    stage.send(key(KeyCode::F(1)));
    let buffer = stage.render(stage.started)?;
    verify(&stage, buffer);
    stage.send(key(KeyCode::Esc)).send(ctrl('o'));
    let buffer = stage.render(stage.started)?;
    verify(&stage, buffer);
    stage.send(ctrl('o'));
    stage.state.screensaver = true;
    let buffer = stage.render(stage.started)?;
    verify(&stage, buffer);
    stage.state.screensaver = false;
    let pager = borg_view::Pager::new(std::sync::Arc::new(b"Brand in the viewer".to_vec()), 88, 20)?;
    stage.send(Msg::Opened(borg_core::Viewer {
        path: "/workspace/sample.txt".into(),
        content: borg_core::Content::Text(pager),
    }));
    let buffer = stage.render(stage.started)?;
    verify(&stage, buffer);
    stage.send(key(KeyCode::Esc));
    for cols in [1, 4, 10, 44, 193] {
        stage.send(Msg::Resize { cols, rows: 20 });
        stage.render(stage.started)?;
    }
    Ok(())
}

#[test]
fn memory_search_form_results_and_match_preview_in_split_panes() -> Result<()> {
    use borg_core::{
        ActivityData, ActivityPage, ActivityRequest, MemoryHit, MemoryRegion, MemorySearchMode, MemorySearchReport,
        MemorySnapshot, Pointer,
    };
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut stage = activity_stage()?;
    stage.send(key(KeyCode::Tab)).send(key(KeyCode::Char('/')));
    stage.send(Msg::Paste("Borg".into()));
    stage.state.activity.search_form.as_mut().unwrap().mode = MemorySearchMode::Unicode;
    let buffer = stage.render(stage.started)?;
    check("memory_search_form", &stage, buffer)?;
    stage.send(key(KeyCode::Enter));
    let id = stage.state.activity.pending.as_ref().unwrap().0;
    let process = stage.state.activity.selected.unwrap();
    let region = MemoryRegion {
        start: 0x100000000,
        end: 0x100040000,
        permissions: "rw-".into(),
        name: "Heap · application buffers".into(),
        resident: None,
        private: None,
        shared: None,
        swap: None,
        ..Default::default()
    };
    stage.send(Msg::Activity {
        id,
        result: Ok(ActivityData::Search(MemorySearchReport {
            process,
            hits: vec![
                MemoryHit { address: region.start + 32, length: 4, encoding: "UTF-8".into(), region: region.clone() },
                MemoryHit {
                    address: region.start + 128,
                    length: 8,
                    encoding: "UTF-16LE".into(),
                    region: region.clone(),
                },
                MemoryHit {
                    address: region.start + 256,
                    length: 16,
                    encoding: "UTF-32BE".into(),
                    region: region.clone(),
                },
            ],
            scanned: 12_000_000,
            total: 120_000_000,
            skipped: 0,
            next: Some(region.end),
            cancelled: false,
        })),
    });
    let buffer = stage.render(stage.started)?;
    check("memory_search_results", &stage, buffer.clone())?;
    let shot = Snapshot::from_buffer(buffer, &stage.palette, None);
    let (column, row) = shot.find("UTF-16LE").unwrap();
    let msg = borg_ui::mouse_input(
        &stage.state,
        MouseEvent { column, row, kind: MouseEventKind::Down(MouseButton::Left), modifiers: KeyModifiers::NONE },
        false,
        stage.started,
    )
    .unwrap();
    assert!(matches!(msg, Msg::Pointer(Pointer::ActivityRow { index: 1, preview: false, .. })));
    stage.send(msg).send(key(KeyCode::Enter));
    let (id, ActivityRequest::Capture { region: window, .. }) = stage.state.activity.pending.clone().unwrap() else {
        panic!("match should request its surrounding bytes")
    };
    assert_eq!(window.start, region.start + 96);
    let mut bytes = vec![0; 200];
    bytes[32..40].copy_from_slice(&[0x42, 0, 0x6f, 0, 0x72, 0, 0x67, 0]);
    stage.send(Msg::Activity {
        id,
        result: Ok(ActivityData::Snapshot(MemorySnapshot { process, address: window.start, bytes, captured: 1 })),
    });
    let buffer = stage.render(stage.started)?;
    check("memory_search_preview", &stage, buffer)?;
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT)));
    let buffer = stage.render(stage.started)?;
    check("memory_search_maximized", &stage, buffer)?;
    assert_eq!(stage.state.activity.page, ActivityPage::Snapshot);
    for (cols, rows) in [(0, 0), (1, 1), (44, 14), (193, 40)] {
        stage.send(Msg::Resize { cols, rows });
        stage.render(stage.started)?;
    }
    Ok(())
}

#[test]
fn expanded_size_tree_accounts_for_files_outside_child_directories() -> Result<()> {
    let mut stage = size_tree_stage()?;
    stage.send(key(KeyCode::Right)).send(key(KeyCode::Right));
    let buffer = stage.render(stage.started)?;
    let shot = Snapshot::from_buffer(buffer.clone(), &stage.palette, None);
    assert!(shot.contains("Files in this folder"));
    check("size_tree_direct_files", &stage, buffer)
}

#[test]
fn background_sizes_stay_in_normal_panels_with_progress_and_stop_action() -> Result<()> {
    let mut stage = stage(GridSize { cols: 132, rows: 28 }, Motion::STILL)?;
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT)));
    let id = stage.state.sizing.as_ref().unwrap().id;
    stage.send(Msg::SizeProgress { id, progress: borg_core::SearchProgress { visited: 15238, ..Default::default() } });
    let mut measured = entry("src", EntryKind::Dir, 45200000, RECENT, false);
    measured.source = Some("/home/me/projects/borg/src".into());
    measured.usage =
        Some(borg_vfs::FolderUsage { files: 410, folders: 24, measured: at(RECENT), ..Default::default() });
    stage.send(Msg::Sized { id, entries: vec![measured] });
    assert_eq!(stage.state.active_panel().dir(), std::path::Path::new("/home/me/projects/borg"));
    assert_eq!(stage.state.active_panel().entries().iter().find(|e| e.name == "src").unwrap().size, 45200000);
    let buffer = stage.render(stage.started + Duration::from_secs(1))?;
    assert!(!stage.state.active_panel().is_results());
    assert!(stage.state.overlay.is_none());
    let shot = Snapshot::from_buffer(buffer.clone(), &stage.palette, None);
    assert!(shot.contains("43M"));
    assert!(shot.contains("Sizes · 15238 scanned"));
    assert!(shot.contains("Alt-I Stop sizes"));
    check("background_sizes", &stage, buffer)?;
    stage.send(Msg::Resize { cols: 44, rows: 14 });
    let buffer = stage.render(stage.started + Duration::from_secs(1))?;
    check("background_sizes_narrow", &stage, buffer)?;
    Ok(())
}
