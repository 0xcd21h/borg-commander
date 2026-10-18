use std::{
    env,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use borg_core::{Motion, Msg, Preferences, SPLASH_DURATION, Settings};
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
const FIRST_CONTACT_CLOCK: &str = "2026-04-05T12:00:00+00:00[UTC]";
const QUIET: Preferences = Preferences { splash: false, ..Preferences::DEFAULT };

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
fn splash_screen_midway() -> Result<()> {
    let mut stage = staged(Motion::LOCAL, Preferences::DEFAULT, project(), CLOCK)?;
    let buffer = stage.render(stage.started + SPLASH_DURATION / 2)?;
    ensure_text(&buffer, "c o m m a n d e r")?;
    check("splash", &stage, buffer)
}

#[test]
fn splash_greets_on_first_contact_day() -> Result<()> {
    let mut stage = staged(Motion::LOCAL, Preferences::DEFAULT, project(), FIRST_CONTACT_CLOCK)?;
    let buffer = stage.render(stage.started + SPLASH_DURATION / 4)?;
    ensure_text(&buffer, "First contact.")
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
fn delight_switch_hides_easter_eggs() -> Result<()> {
    let mut stage = staged(Motion::STILL, Preferences { delight: false, ..QUIET }, crew(), CLOCK)?;
    stage.send(key(KeyCode::Down));
    (0..7).for_each(|_| {
        stage.send(key(KeyCode::Insert));
    });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "7 marked")
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
fn entering_a_folder_slides_the_listing() -> Result<()> {
    let mut stage = stage(SIZE, Motion::LOCAL)?;
    stage.send(key(KeyCode::Down));
    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), stage.started);
    let Some(borg_core::Effect::List { side, generation, .. }) = effects.into_iter().next() else {
        anyhow::bail!("entering a folder must list it");
    };
    stage.send(Msg::Batch { side, generation, entries: logs() });
    stage.send(Msg::Listed { side, generation, error: None });
    let buffer = stage.render(stage.started + Motion::LOCAL.fade / 3)?;
    check("slide_forward", &stage, buffer)
}

#[test]
fn menu_unfolds() -> Result<()> {
    let mut stage = stage(SIZE, Motion::LOCAL)?;
    stage.send(key(KeyCode::F(2)));
    let buffer = stage.render(stage.started + Motion::LOCAL.fade / 3)?;
    check("menu_unfolding", &stage, buffer)
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
fn dialogs_fade_out() -> Result<()> {
    let mut stage = stage(SIZE, Motion::LOCAL)?;
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down)).send(key(KeyCode::F(8)));
    let closing = stage.started + Motion::LOCAL.fade;
    borg_core::update(&mut stage.state, key(KeyCode::Esc), closing);
    let buffer = stage.render(closing + Motion::LOCAL.fade / 3)?;
    check("dialog_fading_out", &stage, buffer)
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
    stage.pixels = true;
    open_picture(&mut stage)?;
    let (buffer, placements) = stage.compose(stage.started)?;
    anyhow::ensure!(placements.len() == 1, "expected one placement, got {placements:?}");
    anyhow::ensure!(!describe(&buffer).contains('▀'), "pixel mode must leave the picture area to the terminal");
    check_with("picture_pixels", &stage, buffer, placements)
}

#[test]
fn nerd_font_icons() -> Result<()> {
    let mut stage = staged(Motion::STILL, Preferences { icons: true, ..QUIET }, project(), CLOCK)?;
    let neon = stage.state.appearance.choices().iter().position(|c| c.id == "neon").unwrap_or_default();
    stage.state.appearance.select(neon, stage.started, Duration::ZERO);
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down));
    let buffer = stage.render(stage.started)?;
    check("icons", &stage, buffer)
}

#[test]
fn quick_find_highlights_matches() -> Result<()> {
    let mut stage = staged(Motion::STILL, Preferences { icons: true, ..QUIET }, project(), CLOCK)?;
    stage.send(ctrl('s'));
    chars("o").for_each(|msg| {
        stage.send(msg);
    });
    stage.send(key(KeyCode::Down));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "find: o  2/4")?;
    check("quick_find", &stage, buffer)
}
