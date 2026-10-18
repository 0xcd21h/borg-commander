use std::fs;

use anyhow::{Result, ensure};
use borg_testkit::{Fixture, Session, Shell, Snapshot, keys};
use crossterm::event::KeyCode;

const BORG: &str = env!("CARGO_BIN_EXE_borg");
const READY: &str = "4 items";
const CONFIG_FILE: &str = "config.toml";
const PROMPT_FROM_BOTTOM: u16 = 2;
const MENU_TITLES: &str = "Left    File    Command    Options    Right";
const LEFT_MENU: usize = 0;
const FILE_MENU: usize = 1;
const OPTIONS_MENU: usize = 3;

fn fixture() -> Result<Fixture> {
    let fixture = Fixture::new()?;
    fixture.dir("alpha/inner")?;
    fixture.file("alpha/one.txt", "1\n")?;
    fixture.dir("beta")?;
    fixture.file("notes.txt", "hello\n")?;
    fixture.executable("run.sh", "#!/bin/sh\necho ran-script\n")?;
    fixture.file(".secret", "x\n")?;
    fixture.freeze()?;
    Ok(fixture)
}

fn start(fixture: &Fixture, label: &str) -> Result<Session> {
    start_with(fixture, label, Shell::Bash)
}

fn start_with(fixture: &Fixture, label: &str, shell: Shell) -> Result<Session> {
    let mut session = Session::builder(BORG, fixture).shell(shell).label(label).spawn()?;
    session.wait_text(READY)?;
    session.wait_until("shell prompt", |s| prompt(s).starts_with('$'))?;
    Ok(session)
}

fn prompt(snapshot: &Snapshot) -> String {
    snapshot.lines()[usize::from(snapshot.rows() - PROMPT_FROM_BOTTOM)].clone()
}

fn open_theme_picker(app: &mut Session) -> Result<()> {
    open_menu(app, OPTIONS_MENU, "Theme…")?;
    app.send(keys::enter())?;
    app.wait_text("Enter keep")?;
    Ok(())
}

fn open_menu(app: &mut Session, index: usize, item: &str) -> Result<()> {
    app.send(keys::f(9))?;
    app.wait_text(MENU_TITLES)?;
    (0..index).try_for_each(|_| app.send(keys::right()).map(drop))?;
    app.wait_text(item)?;
    Ok(())
}

fn choose(app: &mut Session, index: usize, item: &str) -> Result<()> {
    const BORDER_OFFSET: u16 = 3;
    const MAX_ITEMS: usize = 20;
    open_menu(app, index, item)?;
    for _ in 0..MAX_ITEMS {
        let snapshot = app.settle()?;
        let (x, y) = snapshot.find(item).ok_or_else(|| anyhow::anyhow!("menu item {item:?} missing"))?;
        if snapshot.style_at(x, y).bg != snapshot.style_at(x.saturating_sub(BORDER_OFFSET), y).bg {
            app.send(keys::enter())?;
            return Ok(());
        }
        app.send(keys::down())?;
    }
    anyhow::bail!("could not select {item:?}")
}

fn panels_ready(s: &Snapshot) -> bool {
    s.lines().first().is_some_and(|top| top.starts_with('┌')) && prompt(s).starts_with('$')
}

fn left_panel(s: &Snapshot) -> String {
    let half = usize::from(s.cols() / 2);
    s.lines().iter().map(|line| line.chars().take(half).collect::<String>()).collect::<Vec<_>>().join("\n")
}

fn marked_footer(count: usize) -> String {
    format!("{count} marked")
}

fn highlighted(snapshot: &Snapshot, name: &str) -> bool {
    let Some((x, y)) = snapshot.find(name) else { return false };
    let row_bg = snapshot.style_at(x, y).bg;
    let panel_bg = snapshot.style_at(x, y + 1).bg;
    row_bg != panel_bg
}

#[test]
fn startup_lists_both_panels() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "startup")?;
    let screen = app.checkpoint("panels")?;

    for expected in ["/alpha", "/beta", " notes.txt", "*run.sh", "Mar  4  2021", "10Quit", "9Menu", "5Copy"] {
        ensure!(screen.contains(expected), "missing {expected:?}\n{}", screen.text());
    }
    ensure!(!screen.contains(".secret"), "hidden file shown");
    ensure!(highlighted(&screen, ".."), "cursor not on parent row");
    Ok(())
}

#[test]
fn navigation_enters_and_returns_to_folder() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "navigation")?;

    app.send(keys::down())?;
    app.wait_until("cursor on alpha", |s| highlighted(s, "/alpha"))?;
    app.send(keys::enter())?;
    app.wait_text("one.txt")?;
    app.checkpoint("inside-alpha")?;

    app.send(keys::left())?;
    let screen =
        app.wait_until("back in parent with alpha focused", |s| s.contains("/beta") && highlighted(s, "/alpha"))?;
    ensure!(!highlighted(&screen, "/beta"));
    Ok(())
}

#[test]
fn typed_command_runs_in_shell_and_ctrl_o_returns() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "command")?;

    app.send("echo borg-$((40+2))")?;
    app.wait_until("command in prompt row", |s| prompt(s) == "$ echo borg-$((40+2))")?;
    app.checkpoint("typing")?;

    app.send(keys::enter())?;
    app.wait_text("borg-42")?;
    app.wait_gone(READY)?;
    app.checkpoint("shell-view")?;

    app.send(keys::ctrl('o'))?;
    app.wait_text(READY)?;
    Ok(())
}

#[test]
fn shell_cd_moves_active_panel() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "shell-cd")?;

    app.send("cd alpha")?.send(keys::enter())?;
    app.wait_gone(READY)?;
    app.send(keys::ctrl('o'))?;
    app.wait_until("panel followed shell", |s| s.contains("one.txt") && s.contains("/inner"))?;
    Ok(())
}

#[test]
fn panel_navigation_moves_shell_before_next_command() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "panel-cd")?;

    app.send(keys::down())?.send(keys::enter())?;
    app.wait_text("one.txt")?;
    app.send("pwd")?.send(keys::enter())?;
    let expected = fixture.work().join("alpha").display().to_string();
    app.wait_until("pwd output", |s| s.lines().iter().any(|line| line == &expected))?;
    Ok(())
}

#[test]
fn tab_switches_panels_on_empty_line_and_completes_otherwise() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "tab")?;

    let borders = |s: &Snapshot| (s.style_at(0, 0).fg, s.style_at(s.cols() / 2, 0).fg);
    let (active, inactive) = borders(&app.settle()?);
    ensure!(active != inactive, "panels must differ when one is active");

    app.send(keys::tab())?;
    app.wait_until("right panel active", |s| borders(s) == (inactive, active))?;

    app.send("cat not")?.send(keys::tab())?;
    app.wait_until("completion in prompt", |s| prompt(s) == "$ cat notes.txt")?;
    Ok(())
}

#[test]
fn enter_runs_executables() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "executable")?;

    app.send(keys::key(KeyCode::End))?;
    app.wait_until("cursor on run.sh", |s| highlighted(s, "*run.sh"))?;
    app.send(keys::enter())?;
    app.wait_text("ran-script")?;
    Ok(())
}

#[test]
fn full_screen_programs_capture_every_key() -> Result<()> {
    const KEY_GRABBER: &str = "keygrab.sh";
    const SCRIPT: &str = r#"printf '\033[?1049h\033[Hwaiting-for-key'
stty raw -echo
key=$(dd bs=1 count=1 2>/dev/null)
stty sane
printf '\033[?1049l'
echo "got=$(printf '%s' "$key" | od -An -tu1 | tr -d ' ')"
"#;
    const CTRL_O_CODE: &str = "got=15";

    let fixture = fixture()?;
    let script = fixture.home().join(KEY_GRABBER);
    fs::write(&script, SCRIPT)?;
    let mut app = start(&fixture, "fullscreen")?;

    app.send(format!("sh '{}'", script.display()))?.send(keys::enter())?;
    app.wait_until("full-screen program running", |s| s.contains("waiting-for-key") && !s.contains(READY))?;
    app.send(keys::ctrl('o'))?;
    app.wait_until("ctrl-o delivered to the program", |s| s.contains(CTRL_O_CODE) && !s.contains(READY))?;

    app.send(keys::ctrl('o'))?;
    app.wait_text(READY)?;
    Ok(())
}

#[test]
fn hidden_files_toggle() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "hidden")?;

    app.send(keys::alt(KeyCode::Char('.')))?;
    app.wait_text(".secret")?;
    app.send(keys::alt(KeyCode::Char('.')))?;
    app.wait_gone(".secret")?;
    Ok(())
}

#[test]
fn theme_picker_previews_saves_and_restores_on_restart() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "theme-picker")?;
    let before = app.settle()?.style_of("notes.txt").map(|s| s.bg);

    open_theme_picker(&mut app)?;
    app.send(keys::down())?;
    app.wait_until("aurora preview", |s| s.style_of("notes.txt").map(|c| c.bg) != before)?;
    let classic_bg = app.checkpoint("picker")?.style_of("notes.txt").map(|s| s.bg);

    app.send(keys::enter())?;
    app.wait_gone("Enter keep")?;
    let saved = fs::read_to_string(fixture.config().join(CONFIG_FILE))?;
    ensure!(saved.contains("theme = \"aurora\""), "config was {saved:?}");

    app.send(keys::f(10))?;
    app.wait_exit()?;
    drop(app);

    let mut again = start(&fixture, "theme-restored")?;
    let restored = again.settle()?;
    ensure!(restored.style_of("notes.txt").map(|s| s.bg) == classic_bg, "theme not restored\n{}", restored.text());
    Ok(())
}

#[test]
fn theme_picker_escape_reverts_preview() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "theme-escape")?;
    let original = app.settle()?.style_of("notes.txt");

    open_theme_picker(&mut app)?;
    app.send(keys::down())?.send(keys::down())?;
    app.wait_until("preview changed", |s| s.style_of("notes.txt") != original)?;
    app.send(keys::esc())?;
    app.wait_until("original theme back", |s| !s.contains("Enter keep") && s.style_of("notes.txt") == original)?;
    ensure!(!fixture.config().join(CONFIG_FILE).exists(), "escape must not save");
    Ok(())
}

#[test]
fn large_directories_list_fully_and_jump_to_end() -> Result<()> {
    const COUNT: usize = 20_000;
    let fixture = fixture()?;
    let big = fixture.many("big", COUNT)?;
    let mut app = Session::builder(BORG, &fixture).cwd(&big).label("large").spawn()?;

    let footer = format!("{COUNT} items");
    app.wait_until("left panel fully listed", |s| left_panel(s).contains(&footer))?;
    app.send(keys::key(KeyCode::End))?;
    app.wait_until("last entry highlighted", |s| highlighted(s, "item_019999.dat"))?;
    app.checkpoint("end-of-large-dir")?;
    Ok(())
}

#[test]
fn resize_reflows_layout() -> Result<()> {
    const COLS: u16 = 64;
    const ROWS: u16 = 18;
    let fixture = fixture()?;
    let mut app = start(&fixture, "resize")?;

    app.resize(COLS, ROWS)?;
    app.wait_until("layout at new size", |s| {
        let lines = s.lines();
        let top_fits = lines.first().is_some_and(|top| top.chars().count() == usize::from(COLS) && top.ends_with('┐'));
        top_fits && lines[usize::from(ROWS - 1)].contains("Quit")
    })?;
    app.checkpoint("resized")?;
    Ok(())
}

#[test]
fn f10_quits_cleanly() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "quit")?;
    app.send(keys::f(10))?;
    ensure!(app.wait_exit()?.success(), "non-zero exit");
    Ok(())
}

#[test]
fn borg_refuses_to_start_inside_itself() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "nested")?;

    app.send(format!("'{BORG}'; echo nested-exit=$?"))?.send(keys::enter())?;
    app.wait_until("refusal with explanation", |s| {
        s.contains("nested-exit=1") && s.contains("borg is already running")
    })?;
    app.send(keys::ctrl('o'))?;
    app.wait_text(READY)?;
    Ok(())
}

#[test]
fn every_installed_shell_integrates() -> Result<()> {
    let shells = Shell::available();
    ensure!(!shells.is_empty(), "no supported shell installed");

    for (shell, _) in shells {
        let fixture = fixture()?;
        let label = format!("shell-{}", shell.program());
        let mut app = start_with(&fixture, &label, shell)?;

        app.send("echo integration-ok")?.send(keys::enter())?;
        app.wait_text("integration-ok")?;

        app.send("cd alpha")?.send(keys::enter())?;
        app.send(keys::ctrl('o'))?;
        app.wait_until(&format!("{} reported cwd", shell.program()), |s| s.contains("one.txt"))?;

        app.send(keys::down())?.send(keys::enter())?;
        app.wait_until(&format!("{} prompt detected as empty", shell.program()), |s| s.contains("0 items"))?;
    }
    Ok(())
}

#[test]
fn marks_files_with_keys_and_patterns() -> Result<()> {
    let fixture = fixture()?;
    fixture.file("b.rs", "")?;
    fixture.file("c.rs", "")?;
    fixture.freeze()?;
    let mut app = Session::builder(BORG, &fixture).label("marks").spawn()?;
    app.wait_text("6 items")?;

    app.send(keys::down())?.send(keys::key(KeyCode::Insert))?;
    app.wait_text(&marked_footer(1))?;
    app.send(keys::with(KeyCode::Down, crossterm::event::KeyModifiers::SHIFT))?;
    app.wait_text(&marked_footer(2))?;

    app.send("*")?;
    app.wait_text(&marked_footer(4))?;
    app.send("-")?.send(keys::enter())?;
    app.wait_gone("marked")?;

    app.send("+")?;
    app.wait_text("Files matching")?;
    app.send(keys::ctrl('u'))?.send("*.RS")?.send(keys::enter())?;
    let screen = app.wait_text(&marked_footer(2))?;
    ensure!(screen.contains("2 marked · 2 files"), "status line should summarise\n{}", screen.text());
    app.checkpoint("marked")?;
    Ok(())
}

#[test]
fn file_operations_run_through_the_shell() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "file-ops")?;
    let work = fixture.work();

    app.send(keys::f(7))?;
    app.wait_text("Make directory")?;
    app.send("made")?.send(keys::enter())?;
    app.wait_until("new folder focused", |s| highlighted(s, "/made") && panels_ready(s))?;
    ensure!(work.join("made").is_dir());

    app.send(keys::tab())?.send(keys::down())?.send(keys::down())?.send(keys::enter())?;
    app.wait_until("right panel inside beta", |s| s.contains("/beta ") || s.lines()[0].contains("beta"))?;
    app.send(keys::tab())?;

    app.send(keys::key(KeyCode::End))?;
    app.send(keys::up())?;
    app.wait_until("cursor on notes", |s| highlighted(s, " notes.txt"))?;
    app.send(keys::f(5))?;
    let dialog = app.wait_text("Copy \"notes.txt\" to")?;
    ensure!(dialog.contains("/work/beta"), "destination should default to other panel\n{}", dialog.text());
    app.checkpoint("copy-dialog")?;
    app.send(keys::enter())?;
    app.wait_until("copy finished", |s| work.join("beta/notes.txt").is_file() && panels_ready(s))?;

    app.send(keys::f(6))?;
    app.wait_text("Move \"notes.txt\" to")?;
    app.send(keys::ctrl('u'))?.send("made")?.send(keys::enter())?;
    app.wait_until("move finished", |s| {
        work.join("made/notes.txt").is_file() && !left_panel(s).contains("notes.txt") && panels_ready(s)
    })?;
    ensure!(highlighted(&app.settle()?, "*run.sh"), "cursor should move to the next file after a move");

    app.send(keys::key(KeyCode::Home))?.send(keys::down())?;
    app.wait_until("cursor on alpha", |s| highlighted(s, "/alpha"))?;
    app.send(keys::f(8))?;
    app.wait_text("Delete \"alpha\" permanently?")?;
    app.checkpoint("delete-confirm")?;
    app.send(keys::enter())?;
    app.wait_until("alpha deleted", |_| !work.join("alpha").exists())?;
    app.wait_gone("/alpha")?;
    Ok(())
}

#[test]
fn compresses_selection_and_extracts_archives() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "archives")?;
    let work = fixture.work();

    app.send(keys::down())?;
    app.wait_until("cursor on alpha", |s| highlighted(s, "/alpha"))?;
    choose(&mut app, FILE_MENU, "Compress…")?;
    app.wait_text("alpha.tar.gz")?;
    app.send(keys::enter())?;
    app.wait_until("archive created", |s| {
        work.join("alpha.tar.gz").is_file() && highlighted(s, "alpha.tar.gz") && panels_ready(s)
    })?;

    app.send(keys::tab())?.send(keys::down())?.send(keys::down())?.send(keys::enter())?;
    app.wait_until("right panel inside beta", |s| s.lines()[0].contains("beta"))?;
    app.send(keys::tab())?;
    choose(&mut app, FILE_MENU, "Extract to other panel")?;
    app.wait_until("extracted into beta", |_| work.join("beta/alpha/one.txt").is_file())?;
    Ok(())
}

#[test]
fn viewer_shows_files_wraps_and_handles_huge_files() -> Result<()> {
    const HUGE: u64 = 2 << 30;
    let fixture = fixture()?;
    let long_line = format!("{}END\n", "x".repeat(150));
    fixture.file("long.txt", &format!("first\n{long_line}last\n"))?;
    let huge = fixture.file("huge.log", "start of huge\n")?;
    let file = std::fs::OpenOptions::new().write(true).open(&huge)?;
    file.set_len(HUGE)?;
    std::os::unix::fs::FileExt::write_all_at(&file, b"\nthe very end\n", HUGE - 14)?;
    drop(file);
    let mut app = start_like(&fixture, "viewer", 6)?;

    app.send(keys::key(KeyCode::End))?;
    app.send(keys::up())?.send(keys::up())?;
    app.wait_until("cursor on long.txt", |s| highlighted(s, " long.txt"))?;
    app.send(keys::f(3))?;
    app.wait_until("wrapped view", |s| s.contains("first") && s.contains("END") && s.contains("· wrap"))?;
    app.checkpoint("wrapped")?;

    app.send("w")?;
    app.wait_until("unwrapped hides tail", |s| s.contains("nowrap") && !s.contains("END"))?;
    app.send(keys::esc())?;
    app.wait_text("6 items")?;

    app.send(keys::up())?;
    app.wait_until("cursor on huge.log", |s| highlighted(s, " huge.log"))?;
    app.send(keys::enter())?;
    app.wait_text("start of huge")?;
    app.send(keys::key(KeyCode::End))?;
    app.wait_until("end of huge file", |s| s.contains("the very end") && s.contains("100%"))?;
    app.checkpoint("huge-file-end")?;
    Ok(())
}

fn start_like(fixture: &Fixture, label: &str, items: usize) -> Result<Session> {
    let mut session = Session::builder(BORG, fixture).label(label).spawn()?;
    session.wait_text(&format!("{items} items"))?;
    session.wait_until("shell prompt", |s| prompt(s).starts_with('$'))?;
    Ok(session)
}

#[test]
fn status_line_shows_details_and_can_be_hidden() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "status-line")?;

    app.send(keys::key(KeyCode::End))?;
    app.wait_text("-rwxr-xr-x")?;
    app.checkpoint("status-line")?;

    choose(&mut app, OPTIONS_MENU, "Status line")?;
    app.wait_gone("-rwxr-xr-x")?;
    let saved = fs::read_to_string(fixture.config().join(CONFIG_FILE))?;
    ensure!(saved.contains("status-line = false"), "config was {saved:?}");
    Ok(())
}

#[test]
fn brief_listing_and_menu_bar() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "brief")?;

    app.send(keys::f(9))?;
    let menu = app.wait_text(MENU_TITLES)?;
    ensure!(menu.contains("Full listing") && menu.contains("Sort by size"), "left menu\n{}", menu.text());
    app.checkpoint("menu-open")?;
    app.send(keys::esc())?;
    app.wait_gone(MENU_TITLES)?;

    choose(&mut app, LEFT_MENU, "Brief listing")?;
    app.wait_until("brief listing without dates", |s| {
        let left = left_panel(s);
        left.contains("/alpha") && !left.contains("2021")
    })?;
    app.checkpoint("brief")?;
    Ok(())
}

#[test]
fn panels_resize_with_alt_shift_arrows() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "split")?;
    let corner = |s: &Snapshot| s.lines()[0].chars().position(|c| c == '┐');
    let before = corner(&app.settle()?);

    const PRESSES: usize = 5;
    const COLUMNS_PER_PRESS: usize = 2;
    let widen = keys::with(KeyCode::Right, crossterm::event::KeyModifiers::ALT | crossterm::event::KeyModifiers::SHIFT);
    (0..PRESSES).try_for_each(|_| app.send(&widen).map(drop))?;
    let target = before.map(|b| b + PRESSES * COLUMNS_PER_PRESS);
    app.wait_until("left panel ten columns wider", |s| corner(s) == target)?;
    app.checkpoint("wide-left")?;
    Ok(())
}

#[test]
fn session_and_shell_output_are_restored() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "session")?;

    app.send(keys::tab())?.send(keys::down())?.send(keys::enter())?;
    app.wait_until("right panel inside alpha", |s| s.contains("one.txt"))?;
    app.send(keys::tab())?;
    choose(&mut app, OPTIONS_MENU, "Restore shell output on start")?;
    app.send("echo remembered-$((6*7))")?.send(keys::enter())?;
    app.wait_text("remembered-42")?;
    app.send(keys::ctrl('o'))?;
    app.wait_text(READY)?;
    app.send(keys::f(10))?;
    app.wait_exit()?;
    drop(app);

    let mut again = start(&fixture, "session-restored")?;
    let panels = again.settle()?;
    ensure!(panels.contains("one.txt"), "right panel should reopen alpha\n{}", panels.text());
    again.send(keys::ctrl('o'))?;
    again.wait_until("restored output with marker", |s| {
        s.contains("remembered-42") && s.contains("restored from the previous session")
    })?;
    again.checkpoint("restored-shell")?;
    Ok(())
}

#[test]
fn modern_theme_uses_rounded_corners() -> Result<()> {
    let fixture = fixture()?;
    fs::write(fixture.config().join(CONFIG_FILE), "theme = \"aurora\"\n")?;
    let mut app = start(&fixture, "aurora")?;
    let screen = app.checkpoint("aurora")?;
    ensure!(screen.lines()[0].starts_with('╭'), "expected rounded corner\n{}", screen.text());
    Ok(())
}
