use std::fs;

use anyhow::{Result, ensure};
use borg_testkit::{Fixture, Session, Shell, Snapshot, keys};
use crossterm::event::KeyCode;

const BORG: &str = env!("CARGO_BIN_EXE_borg");
const READY: &str = "4 items";
const CONFIG_FILE: &str = "config.toml";
const PROMPT_FROM_BOTTOM: u16 = 2;

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

    for expected in ["/alpha", "/beta", " notes.txt", "*run.sh", "Mar  4  2021", "10Quit", "9Theme"] {
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
    let fixture = fixture()?;
    let mut app = start(&fixture, "fullscreen")?;

    app.send("less notes.txt")?.send(keys::enter())?;
    app.wait_text("(END)")?;
    app.send(keys::ctrl('o'))?;
    let screen = app.settle()?;
    ensure!(screen.contains("hello") && !screen.contains(READY), "ctrl-o escaped less\n{}", screen.text());

    app.send("q")?;
    app.wait_gone("(END)")?;
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

    app.send(keys::f(9))?;
    app.wait_text("Enter keep")?;
    app.send(keys::down())?;
    app.wait_until("classic preview", |s| s.style_of("notes.txt").map(|c| c.bg) != before)?;
    let classic_bg = app.checkpoint("picker")?.style_of("notes.txt").map(|s| s.bg);

    app.send(keys::enter())?;
    app.wait_gone("Enter keep")?;
    let saved = fs::read_to_string(fixture.config().join(CONFIG_FILE))?;
    ensure!(saved.contains("theme = \"classic\""), "config was {saved:?}");

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

    app.send(keys::f(9))?;
    app.wait_text("Enter keep")?;
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

    app.wait_text(&format!("{COUNT} items"))?;
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
