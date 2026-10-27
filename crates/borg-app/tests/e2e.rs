use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use borg_testkit::{Fixture, Session, Shell, Snapshot, keys};
use crossterm::event::KeyCode;

const BORG: &str = env!("CARGO_BIN_EXE_borg");
const READY: &str = "4 items";
const PROMPT_FROM_BOTTOM: u16 = 3;
const MENU_TITLES: &str = "Left    File    Command    Remote    Options    Right";
const MENU_NAMES: [&str; 9] = ["Left", "File", "Command", "Remote", "Options", "Right", "Git", "Docker", "Search"];
const LEFT_MENU: usize = 0;
const FILE_MENU: usize = 1;
const OPTIONS_MENU: usize = 4;
const FIRST_KEY: &str = "F1He";
const SPLASH_SUBTITLE: &str = "c o m m a n d e r";
const UNLOCK: &str = "borg";
const SEVEN: usize = 7;
const CREW: [&str; 9] =
    ["1.drone", "2.drone", "3.drone", "4.drone", "5.drone", "6.drone", "7.drone", "8.drone", "9.drone"];
const TOP_LEFT_CORNERS: [char; 4] = ['┌', '╭', '┏', '╔'];
const TOP_RIGHT_CORNERS: [char; 4] = ['┐', '╮', '┓', '╗'];

fn fixture() -> Result<Fixture> {
    let fixture = Fixture::new()?;
    fixture.dir("alpha/inner")?;
    fixture.file("alpha/one.txt", "1\n")?;
    fixture.dir("beta")?;
    fixture.file("notes.txt", "hello\n")?;
    fixture.executable("run.sh", "#!/bin/sh\necho ran-script\n")?;
    fixture.file(".secret", "x\n")?;
    fixture.freeze()?;
    fixture.configure("return-to-panels = false\n")?;
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

/// Approves an agent's ask once its dialog takes Yes: one that appears right after a key
/// ignores Yes for a moment.
fn approve(app: &mut Session) -> Result<()> {
    app.pump(std::time::Duration::from_millis(600))?;
    app.send("y")?;
    Ok(())
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

fn open_menu_index(snapshot: &Snapshot) -> Option<usize> {
    let bar = snapshot.style_at(0, 0).bg;
    MENU_NAMES.iter().position(|name| snapshot.find(name).is_some_and(|(x, y)| snapshot.style_at(x, y).bg != bar))
}

fn open_menu(app: &mut Session, index: usize, item: &str) -> Result<()> {
    app.send(keys::f(9))?;
    app.wait_text(MENU_TITLES)?;
    let current = open_menu_index(&app.settle()?).ok_or_else(|| anyhow::anyhow!("no menu is open"))?;
    let steps = (index + MENU_NAMES.len() - current) % MENU_NAMES.len();
    (0..steps).try_for_each(|_| app.send(keys::right()).map(drop))?;
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

fn toggle_setting(app: &mut Session, label: &str) -> Result<()> {
    choose(app, OPTIONS_MENU, "Settings…")?;
    let screen = app.wait_text("saved immediately")?;
    let (x, y) = screen.find(label).ok_or_else(|| anyhow::anyhow!("setting missing: {label}"))?;
    app.send(mouse(0, x, y))?;
    app.send(keys::esc())?;
    app.wait_gone("saved immediately")?;
    Ok(())
}

fn panels_ready(s: &Snapshot) -> bool {
    s.lines().first().is_some_and(|top| top.starts_with(TOP_LEFT_CORNERS)) && prompt(s).starts_with('$')
}

fn status(snapshot: &Snapshot) -> String {
    snapshot.lines()[usize::from(snapshot.rows() - PROMPT_FROM_BOTTOM - 1)].trim_end().to_owned()
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

    for expected in ["/ alpha", "/ beta", " notes.txt", "* run.sh", "Mar  4  2021", "10Quit", "9Menu", "5Copy"] {
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
    app.wait_until("cursor on alpha", |s| highlighted(s, "/ alpha"))?;
    app.send(keys::enter())?;
    app.wait_text("one.txt")?;
    app.checkpoint("inside-alpha")?;

    app.send(keys::left())?;
    let screen =
        app.wait_until("back in parent with alpha focused", |s| s.contains("/ beta") && highlighted(s, "/ alpha"))?;
    ensure!(!highlighted(&screen, "/ beta"));
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
fn command_output_with_tabs_renders_in_the_console() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "tab-output")?;
    app.send("printf 'tab\\tseparated\\n'")?.send(keys::enter())?;
    let screen = app.wait_text("separated")?;
    ensure!(screen.lines().iter().any(|line| line.contains("tab") && line.contains("separated")));
    app.send(keys::ctrl('o'))?;
    app.wait_text(READY)?;
    Ok(())
}

#[test]
fn shift_enter_edits_the_shell_buffer_in_place_in_each_shell() -> Result<()> {
    const FIRST: &str = "touch first-ran;";
    const SECOND: &str = "touch second-ran";
    for (shell, _) in Shell::available() {
        let fixture = fixture()?;
        fixture.configure("")?;
        let mut app = start_with(&fixture, &format!("inline-newline-{}", shell.program()), shell)?;
        let baseline = app.wait_text(READY)?.find(READY).unwrap().1;
        app.send(format!("{FIRST}{SECOND}"))?;
        app.wait_text(SECOND)?;
        app.send(keys::ctrl('a'))?.send(keys::right().repeat(FIRST.len()))?;
        app.send(keys::with(KeyCode::Enter, crossterm::event::KeyModifiers::SHIFT))?;
        let screen = app.wait_until("newline splits the command at its cursor", |s| {
            s.find(FIRST).zip(s.find(SECOND)).is_some_and(|((_, first), (_, second))| second > first)
        })?;
        ensure!(screen.find(READY).unwrap().1 < baseline, "panels should shrink above the command");
        ensure!(!screen.contains("Borg console"), "editing should keep the panels visible");
        ensure!(!screen.contains("Alt-Enter run"), "newlines should use the shell prompt");
        ensure!(!fixture.work().join("first-ran").exists(), "newline must not execute the first line");
        ensure!(!fixture.work().join("second-ran").exists());
        app.send(keys::key(KeyCode::Backspace))?;
        app.wait_until("backspace joins the lines", |s| {
            s.contains(&format!("{FIRST}{SECOND}")) && s.find(READY).is_some_and(|(_, y)| y == baseline)
        })?;
        app.send(keys::ctrl('j'))?;
        app.wait_until("Ctrl-J inserts the same newline", |s| {
            s.find(FIRST).zip(s.find(SECOND)).is_some_and(|((_, a), (_, b))| b > a)
        })?;
        app.send("printf 'INSERTED\\n'; ")?.send(keys::enter())?;
        app.wait_until("both lines executed and panels restored", |s| {
            panels_ready(s) && left_panel(s).contains("6 items")
        })?;
        ensure!(fixture.work().join("first-ran").exists());
        ensure!(fixture.work().join("second-ran").exists());
        let screen = app.snapshot();
        ensure!(screen.find("6 items").unwrap().1 == baseline, "prompt should collapse after execution");
    }
    Ok(())
}

#[test]
fn shell_cd_moves_active_panel() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "shell-cd")?;

    app.send("cd alpha")?.send(keys::enter())?;
    app.wait_gone(READY)?;
    app.send(keys::ctrl('o'))?;
    app.wait_until("panel followed shell", |s| s.contains("one.txt") && s.contains("/ inner"))?;
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
fn zsh_follows_panel_folders_without_echoing_cd_commands() -> Result<()> {
    if Shell::Zsh.locate().is_none() {
        return Ok(());
    }
    let fixture = fixture()?;
    fs::write(fixture.home().join(".zshrc"), "PROMPT='$ %1~ '\nRPROMPT=''\nPROMPT_EOL_MARK=''\n")?;
    let mut app = start_with(&fixture, "zsh-silent-cd", Shell::Zsh)?;
    app.send(keys::down())?.send(keys::enter())?;
    app.wait_text("one.txt")?;
    app.send(keys::ctrl('o'))?;
    let console = app.wait_until("prompt redrawn in the panel folder", |s| s.contains("$ alpha"))?;
    ensure!(!console.contains("cd '"), "the folder change was echoed:\n{}", console.text());
    app.send("pwd")?.send(keys::enter())?;
    let expected = fixture.work().join("alpha").display().to_string();
    let output = app.wait_until("pwd output", |s| s.lines().iter().any(|line| line == &expected))?;
    ensure!(!output.contains("cd '"), "the folder change was echoed:\n{}", output.text());
    app.send(keys::ctrl('o'))?;
    app.wait_text("one.txt")?;
    app.send(keys::down())?.send(keys::enter())?;
    app.wait_text("0 items")?;
    app.send("pwd")?.send(keys::enter())?;
    let inner = fixture.work().join("alpha/inner").display().to_string();
    let typed = app.wait_until("pwd typed from the panels", |s| s.lines().iter().any(|line| line == &inner))?;
    ensure!(!typed.contains("cd '"), "the folder change was echoed:\n{}", typed.text());
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
    app.wait_until("cursor on run.sh", |s| highlighted(s, "* run.sh"))?;
    app.send(keys::enter())?;
    app.wait_text("ran-script")?;
    Ok(())
}

#[test]
fn full_screen_programs_capture_every_key() -> Result<()> {
    const KEY_GRABBER: &str = "keygrab.sh";
    const SCRIPT: &str = r#"stty raw -echo
printf '\033[?1049h\033[Hwaiting-for-key'
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
    app.wait_until("panels back once the program exits", |s| s.contains(READY) && !s.contains("waiting-for-key"))?;

    app.send(keys::ctrl('o'))?;
    app.wait_until("ctrl-o was delivered to the program", |s| s.contains(CTRL_O_CODE) && !s.contains(READY))?;
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
    app.checkpoint("picker")?;

    app.send(keys::enter())?;
    app.wait_gone("Enter keep")?;
    let chosen_bg = app.settle()?.style_of("notes.txt").map(|s| s.bg);
    let saved = fs::read_to_string(fixture.config_file())?;
    ensure!(saved.contains("theme = \"aurora\""), "config was {saved:?}");

    app.send(keys::f(10))?;
    app.wait_text("Quit Borg Commander?")?;
    app.send("y")?;
    app.wait_exit()?;
    drop(app);

    let mut again = start(&fixture, "theme-restored")?;
    let restored = again.settle()?;
    ensure!(restored.style_of("notes.txt").map(|s| s.bg) == chosen_bg, "theme not restored\n{}", restored.text());
    Ok(())
}

#[test]
fn theme_picker_escape_reverts_preview() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "theme-escape")?;
    let config = fs::read_to_string(fixture.config_file())?;
    let original = app.settle()?.style_of("notes.txt");

    open_theme_picker(&mut app)?;
    app.send(keys::down())?.send(keys::down())?;
    app.wait_until("preview changed", |s| s.style_of("notes.txt") != original)?;
    app.send(keys::esc())?;
    app.wait_until("original theme back", |s| !s.contains("Enter keep") && s.style_of("notes.txt") == original)?;
    ensure!(fs::read_to_string(fixture.config_file())? == config, "escape must not save");
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
        let top_fits = lines
            .first()
            .is_some_and(|top| top.chars().count() == usize::from(COLS) && top.ends_with(TOP_RIGHT_CORNERS));
        top_fits && lines[usize::from(ROWS - 2)].starts_with(FIRST_KEY)
    })?;
    app.checkpoint("resized")?;
    Ok(())
}

#[test]
fn hidden_2048_modal_plays_undoes_resizes_and_restores_after_restart() -> Result<()> {
    use borg_core::SavedGame2048;
    let fixture = fixture()?;
    let path = fixture.state().join("2048.json");
    fs::create_dir_all(fixture.state())?;
    let initial = SavedGame2048 {
        version: 1,
        cells: [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        score: 0,
        best: 0,
        random: 42,
        continued: false,
    };
    fs::write(&path, serde_json::to_vec(&initial)?)?;
    let load = || fs::read(&path).ok().and_then(|bytes| serde_json::from_slice::<SavedGame2048>(&bytes).ok());
    let mut app = start(&fixture, "2048")?;
    app.send(keys::f(1))?;
    app.wait_text(&format!("version {}", env!("CARGO_PKG_VERSION")))?;
    app.send("2048")?;
    app.wait_text("Score 0    Best 0")?;
    app.wait_text("U Undo")?;
    app.send(keys::left())?;
    app.wait_text("Score 4    Best 4")?;
    app.send("u")?;
    app.wait_text("Score 0    Best 4")?;
    ensure!(load().is_some_and(|saved| saved.cells == initial.cells && saved.random == initial.random));
    app.send(keys::left())?;
    app.wait_text("Score 4    Best 4")?;
    app.send("r")?;
    app.wait_text("Score 0    Best 4")?;
    let restarted = load().unwrap();
    ensure!(restarted.cells.iter().filter(|&&cell| cell > 0).count() == 2);
    app.resize(1, 1)?;
    app.resize(64, 18)?;
    app.wait_text("Score 0    Best 4")?;
    ensure!(load() == Some(restarted.clone()));
    app.checkpoint("2048-modal")?;
    app.send(keys::esc())?;
    app.wait_gone("Join matching tiles")?;
    app.send(keys::f(1))?;
    app.wait_text(&format!("version {}", env!("CARGO_PKG_VERSION")))?;
    app.send("2048")?;
    app.wait_text("Score 0    Best 4")?;
    ensure!(load() == Some(restarted.clone()));
    app.send(keys::esc())?;
    app.wait_gone("Join matching tiles")?;
    app.send(keys::f(10))?;
    app.wait_text("Quit Borg Commander?")?;
    app.send("y")?;
    app.wait_exit()?;
    drop(app);
    let mut again = start(&fixture, "2048-restored")?;
    again.send(keys::f(1))?;
    again.wait_text(&format!("version {}", env!("CARGO_PKG_VERSION")))?;
    again.send("2048")?;
    again.wait_text("Score 0    Best 4")?;
    ensure!(load() == Some(restarted));
    Ok(())
}

#[test]
fn f10_quits_cleanly() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "quit")?;
    app.send(keys::f(10))?;
    app.wait_text("Quit Borg Commander?")?;
    app.send("y")?;
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
    app.wait_until("new folder focused", |s| highlighted(s, "/ made") && panels_ready(s))?;
    ensure!(work.join("made").is_dir());

    app.send(keys::tab())?.send(keys::down())?.send(keys::down())?.send(keys::enter())?;
    app.wait_until("right panel inside beta", |s| s.contains("/beta ") || s.lines()[0].contains("beta"))?;
    app.send(keys::tab())?;

    app.send(keys::key(KeyCode::End))?;
    app.send(keys::up())?;
    app.wait_until("cursor on notes", |s| highlighted(s, " notes.txt"))?;
    app.send(keys::f(5))?;
    app.wait_until("destination defaults to other panel", |s| {
        s.contains("Copy \"notes.txt\" to") && s.contains("/work/beta")
    })?;
    app.checkpoint("copy-dialog")?;
    let screen = app.wait_text("[ ] Optimize copying for hard drives")?;
    let (x, y) = screen.find("[ ] Optimize copying for hard drives").unwrap();
    app.send(mouse(0, x, y))?;
    app.wait_text("[x] Optimize copying for hard drives")?;
    app.send(keys::tab())?;
    app.send(keys::tab())?.send(" ")?;
    app.wait_text("[ ] Optimize copying for hard drives")?;
    app.send(" ")?;
    app.wait_text("[x] Optimize copying for hard drives")?;
    app.send(keys::enter())?;
    app.wait_until("copy finished", |s| work.join("beta/notes.txt").is_file() && panels_ready(s))?;

    app.send(keys::f(6))?;
    app.wait_text("Move \"notes.txt\" to")?;
    app.send(keys::ctrl('u'))?.send("made")?.send(keys::enter())?;
    app.wait_until("move finished", |s| {
        work.join("made/notes.txt").is_file() && !left_panel(s).contains("notes.txt") && panels_ready(s)
    })?;
    ensure!(highlighted(&app.settle()?, "* run.sh"), "cursor should move to the next file after a move");

    app.send(keys::key(KeyCode::Home))?.send(keys::down())?;
    app.wait_until("cursor on alpha", |s| highlighted(s, "/ alpha"))?;
    app.send(keys::f(8))?;
    app.wait_text("Delete \"alpha\" permanently?")?;
    app.checkpoint("delete-confirm")?;
    app.send(keys::enter())?;
    app.wait_until("alpha deleted", |_| !work.join("alpha").exists())?;
    app.wait_gone("/ alpha")?;
    Ok(())
}

#[test]
fn compresses_selection_and_extracts_archives() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "archives")?;
    let work = fixture.work();

    app.send(keys::down())?;
    app.wait_until("cursor on alpha", |s| highlighted(s, "/ alpha"))?;
    choose(&mut app, FILE_MENU, "Compress…")?;
    app.wait_text("alpha.tar.gz")?;
    app.send(keys::enter())?;
    app.wait_until("archive created", |s| {
        work.join("alpha.tar.gz").is_file()
            && left_panel(s).contains("5 items")
            && highlighted(s, "alpha.tar.gz")
            && panels_ready(s)
            && prompt(s).trim() == "$"
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

    toggle_setting(&mut app, "Status line")?;
    app.wait_gone("-rwxr-xr-x")?;
    let saved = fs::read_to_string(fixture.config_file())?;
    ensure!(saved.contains("status-line = false"), "config was {saved:?}");
    Ok(())
}

#[test]
fn brief_listing_and_menu_bar() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "brief")?;

    app.send(keys::f(9))?;
    let menu = app.wait_until("left menu unfolded", |s| {
        s.contains(MENU_TITLES) && s.contains("Full listing") && s.contains("Sort by size")
    })?;
    ensure!(menu.contains("Brief listing"), "left menu\n{}", menu.text());
    app.checkpoint("menu-open")?;
    app.send(keys::esc())?;
    app.wait_gone(MENU_TITLES)?;

    choose(&mut app, LEFT_MENU, "Brief listing")?;
    app.wait_until("brief listing without dates", |s| {
        let left = left_panel(s);
        left.contains("/ alpha") && !left.contains("2021")
    })?;
    app.checkpoint("brief")?;
    Ok(())
}

#[test]
fn panels_resize_with_alt_shift_arrows() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "split")?;
    let corner = |s: &Snapshot| s.lines()[0].chars().position(|c| TOP_RIGHT_CORNERS.contains(&c));
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
    toggle_setting(&mut app, "Restore shell output on start")?;
    app.send("echo remembered-$((6*7))")?.send(keys::enter())?;
    app.wait_text("remembered-42")?;
    app.send(keys::ctrl('o'))?;
    app.wait_text(READY)?;
    app.send(keys::f(10))?;
    app.wait_text("Quit Borg Commander?")?;
    app.send("y")?;
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
    fixture.configure("theme = \"aurora\"\n")?;
    let mut app = start(&fixture, "aurora")?;
    let screen = app.checkpoint("aurora")?;
    ensure!(screen.lines()[0].starts_with('╭'), "expected rounded corner\n{}", screen.text());
    Ok(())
}

#[test]
fn retired_animation_option_opens_panels_immediately() -> Result<()> {
    let fixture = fixture()?;
    fixture.configure("startup-animation = true\n")?;
    let mut app = Session::builder(BORG, &fixture).label("startup-direct").spawn()?;
    let panels = app.wait_text(READY)?;
    ensure!(!panels.contains(SPLASH_SUBTITLE), "startup must show panels");
    Ok(())
}

#[test]
fn about_screen_unlocks_the_cube_theme() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "about")?;
    app.send(keys::f(1))?;
    app.wait_text("Resistance is futile.")?;
    app.send(UNLOCK)?;
    app.wait_text("Cube theme unlocked")?;
    app.send(keys::esc())?;
    app.wait_gone("Resistance is futile.")?;

    let saved = fs::read_to_string(fixture.config_file())?;
    ensure!(saved.contains("unlocked = [\"cube\"]"), "config was {saved:?}");
    open_theme_picker(&mut app)?;
    app.wait_text("Cube")?;
    Ok(())
}

#[test]
fn screensaver_starts_when_idle_and_a_key_wakes_it() -> Result<()> {
    let fixture = fixture()?;
    fixture.configure("screensaver = true\nscreensaver-after = 1\n")?;
    let mut app = start(&fixture, "screensaver")?;
    app.wait_gone(READY)?;
    app.send(keys::down())?;
    let panels = app.wait_text(READY)?;
    ensure!(highlighted(&panels, ".."), "the waking key must not move the cursor\n{}", panels.text());
    Ok(())
}

#[test]
fn marking_seven_of_nine() -> Result<()> {
    let fixture = Fixture::new()?;
    CREW.iter().try_for_each(|name| fixture.file(name, "drone\n").map(drop))?;
    let mut app = Session::builder(BORG, &fixture).label("seven-of-nine").spawn()?;
    app.wait_text("9 items")?;
    app.send(keys::down())?;
    (0..SEVEN).try_for_each(|_| app.send(keys::key(KeyCode::Insert)).map(drop))?;
    app.wait_text("Seven of Nine")?;
    app.send(keys::key(KeyCode::Insert))?.send(keys::key(KeyCode::Insert))?;
    app.wait_text("all 9 assimilated")?;
    app.checkpoint("assimilated")?;
    Ok(())
}

#[test]
fn f3_on_a_picture_shows_it_and_steps_to_the_next() -> Result<()> {
    let fixture = Fixture::new()?;
    for (name, shade) in [("a.png", 40u8), ("b.png", 200u8)] {
        let rgba = (0..32 * 16).flat_map(|_| [shade, 90, 255 - shade, 255]).collect();
        let picture = borg_gfx::Picture::from_rgba(32, 16, rgba)?;
        fs::write(fixture.work().join(name), picture.png()?)?;
    }
    let mut app = Session::builder(BORG, &fixture).label("picture").spawn()?;
    app.wait_text("2 items")?;
    app.send(keys::down())?.send(keys::f(3))?;
    let first = app.wait_text("32×16 · PNG")?;
    ensure!(first.contains("▀") && first.contains("a.png"), "picture not drawn\n{}", first.text());
    app.send(keys::right())?;
    app.wait_until("second picture", |s| s.contains("b.png · 32×16"))?;
    app.send(keys::esc())?;
    let panels = app.wait_text("2 items")?;
    ensure!(highlighted(&panels, "b.png"), "panel cursor should follow the viewer\n{}", panels.text());
    Ok(())
}

#[test]
fn ctrl_s_finds_files_as_you_type() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "find")?;
    app.send(keys::ctrl('s'))?;
    app.wait_text(" Find ")?;
    app.send("note")?;
    app.wait_until("cursor on notes", |s| highlighted(s, "notes.txt") && s.contains("1/1"))?;
    app.send(keys::enter())?;
    app.wait_gone(" Find ")?;
    app.send(keys::f(3))?;
    app.wait_text("hello")?;
    Ok(())
}

#[test]
fn quick_view_and_info_replace_the_other_panel() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "aside")?;
    app.send(keys::ctrl('s'))?.send("note")?.send(keys::enter())?;
    app.wait_until("cursor on notes", |s| status(s).ends_with("notes.txt"))?;
    app.send(keys::ctrl('q'))?;
    app.wait_until("quick view of notes", |s| s.contains("Quick view · notes.txt") && s.contains("hello"))?;
    app.send(keys::ctrl('l'))?;
    app.wait_until("info about notes", |s| s.contains("Info · notes.txt") && s.contains("6 bytes"))?;
    app.send(keys::ctrl('l'))?;
    app.wait_gone("Info · notes.txt")?;
    Ok(())
}

#[test]
fn copying_onto_an_existing_file_asks_and_can_keep_both() -> Result<()> {
    let fixture = fixture()?;
    fixture.file("beta/notes.txt", "older\n")?;
    let mut app = start(&fixture, "conflict")?;
    app.send(keys::tab())?.send(keys::down())?.send(keys::down())?.send(keys::enter())?;
    app.wait_until("right panel inside beta", |s| s.lines()[0].contains("beta") && s.contains("1 item"))?;
    app.send(keys::tab())?;
    app.send(keys::key(KeyCode::End))?.send(keys::up())?;
    app.wait_until("cursor on notes", |s| status(s).ends_with("notes.txt"))?;
    app.send(keys::f(5))?;
    app.wait_until("destination prompt", |s| s.contains("Copy \"notes.txt\" to") && s.contains("/work/beta"))?;
    app.send(keys::enter())?;
    app.wait_text("Already exists")?;
    app.send("k")?;
    app.wait_until("copied with a new name", |_| fixture.work().join("beta/notes (2).txt").is_file())?;
    app.wait_text("Copied 1 file")?;
    ensure!(fs::read_to_string(fixture.work().join("beta/notes.txt"))? == "older\n", "the existing file must stay");
    ensure!(fs::read_to_string(fixture.work().join("beta/notes (2).txt"))? == "hello\n", "the copy keeps its content");
    Ok(())
}

fn mouse(kind: u8, x: u16, y: u16) -> String {
    format!("\x1b[<{kind};{};{}M", x + 1, y + 1)
}

#[test]
fn wheel_momentum_after_leaving_the_console_does_not_move_the_panel() -> Result<()> {
    const WHEEL_UP: u8 = 64;
    const WHEEL_DOWN: u8 = 65;
    let fixture = fixture()?;
    let mut app = start(&fixture, "wheel-momentum")?;
    app.send("printf 'line-%03d\\n' {1..60}")?.send(keys::enter())?;
    app.wait_text("line-060")?;
    app.wait_text("Borg console")?;
    let burst = |direction| (0..6).map(|_| mouse(direction, 10, 5)).collect::<String>();
    app.send(burst(WHEEL_UP))?.send(keys::ctrl('o'))?.send(burst(WHEEL_DOWN))?;
    app.wait_until("panels back", panels_ready)?;
    app.pump(std::time::Duration::from_millis(300))?;
    let after = app.settle()?;
    ensure!(
        status(&after).contains("parent directory"),
        "the console's wheel burst moved the panel: {}",
        status(&after)
    );
    app.pump(std::time::Duration::from_millis(400))?;
    app.send(mouse(WHEEL_DOWN, 10, 5))?;
    app.wait_until("a new wheel gesture moves the panel", |s| !status(s).contains("parent directory"))?;
    Ok(())
}

#[test]
fn mouse_opens_menus_and_activates_items() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "mouse-menus")?;
    let screen = app.settle()?;
    let (x, y) = screen.find("9Menu").ok_or_else(|| anyhow::anyhow!("menu action missing"))?;
    app.send(mouse(0, x, y))?;
    let screen = app.wait_text(MENU_TITLES)?;
    let (x, y) = screen.find("Options").ok_or_else(|| anyhow::anyhow!("Options missing"))?;
    app.send(mouse(0, x, y))?;
    let screen = app.wait_text("Theme…")?;
    let (x, y) = screen.find("Theme…").ok_or_else(|| anyhow::anyhow!("Theme missing"))?;
    app.send(mouse(0, x, y))?;
    app.wait_text("Enter keep")?;
    Ok(())
}

#[test]
fn mouse_wheel_scrolls_console_output_and_preserves_typed_command() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "console-history")?;
    app.send("printf 'BORG-HISTORY-%03d\\n' {1..100}")?.send(keys::enter())?;
    app.wait_text("BORG-HISTORY-100")?;
    app.wait_text("Borg console")?;
    app.send("echo wheel-preserved")?;
    app.wait_text("$ echo wheel-preserved")?;
    app.send(mouse(64, 5, 5))?;
    app.wait_until("console history above the prompt", |s| {
        s.contains("↑3 lines") && !s.contains("$ echo wheel-preserved")
    })?;
    app.send(keys::with(KeyCode::Home, crossterm::event::KeyModifiers::SHIFT))?;
    app.wait_text("BORG-HISTORY-001")?;
    app.send(mouse(65, 5, 5))?;
    app.wait_gone("BORG-HISTORY-001")?;
    app.send(keys::with(KeyCode::End, crossterm::event::KeyModifiers::SHIFT))?;
    app.wait_text("$ echo wheel-preserved")?;
    app.send(keys::enter())?;
    app.wait_until("typed command survived scrolling", |s| {
        s.lines().iter().any(|line| line.trim() == "wheel-preserved")
    })?;
    Ok(())
}

#[test]
fn rapid_resizes_and_tiny_windows_recover_with_an_open_menu() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "resize-race")?;
    app.send(keys::f(9))?;
    app.wait_text(MENU_TITLES)?;
    for _ in 0..3 {
        for (cols, rows) in [(194, 41), (193, 40), (40, 12), (80, 24)] {
            app.resize(cols, rows)?;
        }
    }
    app.resize(1, 1)?;
    app.settle()?;
    app.resize(100, 30)?;
    app.wait_text(MENU_TITLES)?;
    app.send(keys::key(KeyCode::Esc))?;
    app.wait_until("panels recovered after rapid resizing", panels_ready)?;
    app.send(keys::ctrl('o'))?;
    app.wait_text("Borg console")?;
    app.resize(193, 40)?;
    app.resize(64, 18)?;
    app.wait_text("Borg console")?;
    Ok(())
}

#[test]
fn mouse_marks_files_and_double_click_opens_folders() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "mouse-panels")?;
    let screen = app.settle()?;
    let (x, y) = screen.find("notes.txt").unwrap();
    app.send(mouse(2, x, y))?;
    app.wait_text("1 marked")?;
    let screen = app.settle()?;
    let (x, y) = screen.find("alpha").unwrap();
    app.send(mouse(0, x, y))?;
    app.send(format!("\x1b[<0;{};{}m", x + 1, y + 1))?;
    app.send(mouse(0, x, y))?;
    app.wait_text("one.txt")?;
    Ok(())
}

#[test]
fn completed_commands_return_to_panels_by_default_and_keep_console_output() -> Result<()> {
    let fixture = fixture()?;
    fixture.configure("")?;
    let mut app = start(&fixture, "return-default")?;
    app.send("printf 'BORG-DEFAULT-%s\\n' done; touch command-finished")?.send(keys::enter())?;
    app.wait_until("returned to panels", |s| {
        panels_ready(s) && left_panel(s).contains("5 items") && prompt(s).trim() == "$"
    })?;
    app.send(keys::ctrl('o'))?;
    app.wait_text("BORG-DEFAULT-done")?;
    Ok(())
}

#[test]
fn markdown_preview_scrolls_with_focus_and_toggles_source() -> Result<()> {
    let fixture = fixture()?;
    let text = format!(
        "# Preview title\n\n{}\nEnd of document\n",
        (0..80).map(|n| format!("- item {n}\n")).collect::<String>()
    );
    fixture.file("guide.md", &text)?;
    let mut app = start_like(&fixture, "markdown-preview", 5)?;
    app.send(keys::ctrl('s'))?.send("guide")?.send(keys::enter())?.send(keys::ctrl('q'))?;
    let screen = app.wait_text("Quick view · guide.md")?;
    ensure!(!screen.contains("# Preview title"), "Markdown should open rendered");
    app.wait_text("Preview title")?;
    app.send(keys::tab())?;
    app.wait_text("Focused · Tab back")?;
    app.send(keys::key(KeyCode::End))?;
    app.wait_text("End of document")?;
    app.send("s")?;
    app.wait_text("# Preview title")?;
    app.send("s")?;
    app.wait_text("End of document")?;
    app.send(keys::tab())?.send(keys::f(3))?;
    app.wait_text("rendered")?;
    app.wait_text("End of document")?;
    app.send("s")?;
    app.wait_text("# Preview title")?;
    app.send(keys::key(KeyCode::End))?;
    app.wait_text("End of document")?;
    app.send(keys::esc())?;
    let screen = app.wait_text("Quick view · guide.md")?;
    ensure!(!screen.contains("# Preview title"), "closing the viewer should preserve the scrolled position");
    app.wait_text("End of document")?;
    Ok(())
}

#[test]
fn recalling_a_wrapped_command_expands_the_prompt_above_the_keybar() -> Result<()> {
    let fixture = fixture()?;
    fixture.configure("")?;
    let mut app = Session::builder(BORG, &fixture).size(50, 22).label("history-prompt").spawn()?;
    app.wait_text(READY)?;
    app.wait_until("shell ready", |s| prompt(s).trim() == "$")?;
    let command = format!("printf '%s\\n' 'BEGIN-RECALL {} END-RECALL'", "history text ".repeat(10));
    app.send(&command)?.send(keys::enter())?;
    app.wait_until("command finished", |s| panels_ready(s) && prompt(s).trim() == "$")?;
    app.send(keys::ctrl('p'))?;
    let screen = app.wait_until("full recalled command", |s| s.contains("BEGIN-RECALL") && s.contains("END-RECALL"))?;
    let first = screen.find("BEGIN-RECALL").unwrap().1;
    let last = screen.find("END-RECALL").unwrap().1;
    ensure!(last > first, "recalled command should occupy multiple rows");
    ensure!(last < screen.rows() - 2, "command should stay above the keybar");
    app.send(keys::ctrl('c'))?;
    app.wait_until("prompt collapsed", |s| prompt(s).trim() == "$")?;
    Ok(())
}

#[test]
fn recalling_multiline_history_keeps_the_whole_command_visible_in_each_shell() -> Result<()> {
    for (shell, _) in Shell::available() {
        let fixture = fixture()?;
        fixture.configure("")?;
        let mut app = start_with(&fixture, &format!("multiline-{}", shell.program()), shell)?;
        app.send(if shell == Shell::Fish { "begin" } else { "{" })?;
        for line in ["printf 'FIRST-HISTORY\\n';", "printf 'SECOND-HISTORY\\n';", "touch history-ready;"] {
            app.send(keys::ctrl('j'))?.send(line)?;
        }
        app.send(keys::ctrl('j'))?.send(if shell == Shell::Fish { "end" } else { "}" })?;
        app.wait_until("five lines typed in the prompt", |s| {
            s.contains("FIRST-HISTORY") && s.contains("SECOND-HISTORY") && s.contains("touch history-ready")
        })?;
        app.send(keys::enter())?;
        app.wait_until(&format!("multiline command completed in {}", shell.program()), |s| {
            panels_ready(s) && left_panel(s).contains("5 items") && prompt(s).trim() == "$"
        })?;
        app.send(keys::ctrl('p'))?;
        let recalled = app.wait_until("multiline command recalled", |s| {
            s.contains("FIRST-HISTORY") && s.contains("SECOND-HISTORY") && s.contains("history-ready")
        })?;
        ensure!(!recalled.contains("Borg console"), "history recall should keep the panels visible");
        app.send(keys::ctrl('c'))?;
        app.wait_until("command prompt cleared", |s| prompt(s).trim() == "$")?;
    }
    Ok(())
}

#[test]
fn ghostty_icons_stay_rich_and_saved_settings_keep_portable_terminal_fallback() -> Result<()> {
    const FOLDER_ICON: &str = "\u{f07b}";
    let fixture = fixture()?;
    fixture.configure("nerd-font = false\n")?;
    let mut ghostty = Session::builder(BORG, &fixture).terminal_program("ghostty").label("ghostty-icons").spawn()?;
    let screen = ghostty.wait_text(READY)?;
    ensure!(screen.contains(FOLDER_ICON), "Ghostty should get rich folder icons\n{}", screen.text());
    ghostty.wait_until("shell prompt", |s| prompt(s).starts_with('$'))?;
    toggle_setting(&mut ghostty, "Status line")?;
    let saved = fs::read_to_string(fixture.config_file())?;
    ensure!(saved.contains("icon-style = \"auto\""), "automatic detection must stay automatic: {saved}");
    ensure!(!saved.contains("nerd-font"), "obsolete detected preference should be removed: {saved}");
    ghostty.send(keys::f(10))?;
    ghostty.wait_text("Quit Borg Commander?")?;
    ghostty.send("y")?;
    ghostty.wait_exit()?;
    drop(ghostty);
    let mut portable =
        Session::builder(BORG, &fixture).terminal_program("Apple_Terminal").label("portable-icons").spawn()?;
    let screen = portable.wait_text(READY)?;
    ensure!(!screen.contains(FOLDER_ICON), "Apple Terminal should keep portable icons");
    ensure!(screen.contains("/ alpha"), "portable folder marker missing\n{}", screen.text());
    Ok(())
}

fn git(fixture: &Fixture, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(fixture.work())
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()?;
    ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[test]
fn git_plugin_views_diffs_stages_unstages_and_persists_disabling() -> Result<()> {
    const GIT_MENU: usize = 6;
    let fixture = fixture()?;
    git(&fixture, &["init", "-b", "main"])?;
    git(&fixture, &["config", "user.name", "Borg Test"])?;
    git(&fixture, &["config", "user.email", "borg@example.test"])?;
    git(&fixture, &["add", "notes.txt"])?;
    git(&fixture, &["commit", "-qm", "Initial fixture"])?;
    let contents = "changed by plugin test\n";
    fs::write(fixture.work().join("notes.txt"), contents)?;
    let mut app = start(&fixture, "git-plugin")?;
    app.wait_text("Git main")?;
    app.send(keys::ctrl('s'))?.send("notes")?.send(keys::enter())?;
    choose(&mut app, GIT_MENU, "Diff selection")?;
    app.wait_text("+changed by plugin test")?;
    app.send(keys::esc())?;
    app.wait_until("panels after Git viewer", panels_ready)?;
    choose(&mut app, GIT_MENU, "Stage selection")?;
    app.wait_text("Git index updated")?;
    ensure!(git(&fixture, &["diff", "--cached"])?.contains("+changed by plugin test"));
    choose(&mut app, GIT_MENU, "Staged diff selection")?;
    app.wait_text("+changed by plugin test")?;
    app.send(keys::esc())?;
    app.wait_until("panels after Git viewer", panels_ready)?;
    choose(&mut app, GIT_MENU, "Unstage selection")?;
    app.wait_text("Git index updated")?;
    ensure!(git(&fixture, &["diff", "--cached"])?.is_empty());
    ensure!(fs::read_to_string(fixture.work().join("notes.txt"))? == contents);
    choose(&mut app, GIT_MENU, "Recent commits")?;
    app.wait_text("Initial fixture")?;
    app.send(keys::esc())?;
    app.wait_until("panels after Git viewer", panels_ready)?;
    choose(&mut app, OPTIONS_MENU, "Plugins…")?;
    let screen = app.wait_text("Git — status, diffs, stage/unstage")?;
    let (x, y) = screen.find("Git — status, diffs, stage/unstage").unwrap();
    app.send(mouse(0, x, y))?;
    app.wait_text("[ ] Git")?;
    app.send(keys::esc())?;
    app.wait_gone("Git main")?;
    ensure!(fs::read_to_string(fixture.config_file())?.contains("git = false"));
    drop(app);
    let mut app = start(&fixture, "git-plugin-disabled")?;
    app.send(keys::f(9))?;
    let screen = app.wait_text(MENU_TITLES)?;
    ensure!(!screen.lines()[0].contains("Git"), "disabled plugin should remove the menu");
    ensure!(!screen.contains("Git main"), "disabled plugin should remove branch and status columns");
    Ok(())
}

#[test]
fn search_collect_preview_copy_remove_and_restore() -> Result<()> {
    let fixture = fixture()?;
    fixture.file("beta/one.txt", "BETA source\n")?;
    let mut app = start(&fixture, "results-workflow")?;
    app.send(keys::ctrl('f'))?;
    app.wait_text("Name or glob")?;
    app.send(keys::ctrl('u'))?;
    app.send("one.txt")?;
    app.send(keys::enter())?;
    let found = app.wait_until("both recursive matches", |s| {
        s.contains("Search: one.txt") && s.contains("2/2 items") && s.contains("finished")
    })?;
    ensure!(found.contains("Source folder"), "source column missing\n{}", found.text());
    let (x, y) = found.find("[+]").ok_or_else(|| anyhow::anyhow!("search must open in its pane"))?;
    ensure!(x < found.cols() / 2);
    app.send(mouse(0, x, y))?;
    let expanded = app.wait_text("[=]")?;
    let (x, y) = expanded.find("[=]").unwrap();
    ensure!(x > expanded.cols() / 2);
    app.send(mouse(0, x, y))?;
    app.wait_text("[+]")?;
    app.send(keys::key(KeyCode::Home))?;
    app.send(keys::down())?;
    let screen = app.wait_text("[-]")?;
    let (x, y) = screen.find("[-]").unwrap();
    app.send(mouse(0, x, y))?;
    app.wait_gone("Search: one.txt")?;
    app.send(keys::alt(KeyCode::Char('R')))?;
    app.wait_text("Search: one.txt")?;
    app.send(keys::ctrl('q'))?;
    app.wait_text("BETA source")?;
    app.send(keys::ctrl('q'))?;
    app.send(keys::key(KeyCode::Home))?;
    app.send(keys::ctrl('t'))?;
    app.send(keys::ctrl('t'))?;
    app.wait_text("2 marked")?;
    app.send(keys::alt(KeyCode::Char('k')))?;
    app.wait_text("Collected in Collection")?;
    app.send(keys::tab())?;
    app.wait_text("Collection: Collection")?;
    app.wait_text("2/2 items")?;
    app.send(keys::f(8))?;
    app.wait_text("1/1 items")?;
    ensure!(fixture.work().join("alpha/one.txt").exists());
    ensure!(fixture.work().join("beta/one.txt").exists());
    app.send(keys::tab())?;
    app.wait_text("Search: one.txt")?;
    app.send(keys::left())?;
    app.wait_gone("Search: one.txt")?;
    app.send(keys::tab())?;
    app.wait_text("Collection: Collection")?;
    app.send(keys::f(5))?;
    app.wait_text("Copy \"")?;
    app.send(keys::enter())?;
    app.wait_until("copied original from collection", |_| fixture.work().join("one.txt").exists())?;
    app.wait_text("Copied 1 file")?;
    ensure!(fs::read_to_string(fixture.work().join("one.txt"))? == "BETA source\n");
    app.send(keys::f(10))?;
    app.wait_text("Quit Borg Commander?")?;
    app.send("y")?;
    app.wait_exit()?;
    let mut app = Session::builder(BORG, &fixture).label("results-restored").spawn()?;
    app.wait_text("5 items")?;
    app.send(keys::alt(KeyCode::Char('v')))?;
    app.wait_text("Collections")?;
    app.send(keys::enter())?;
    let restored = app.wait_until("saved collection restored", |s| {
        s.contains("Collection: Collection") && s.contains("1/1 items") && !s.contains("[missing]")
    })?;
    ensure!(restored.contains("one.txt"));
    Ok(())
}

#[test]
fn folder_analysis_finds_small_file_accumulations_and_confirms_deletion() -> Result<()> {
    let fixture = fixture()?;
    for index in 0..40 {
        fixture.file(&format!("alpha/node_modules/.cache/file-{index}"), "1234567890")?;
    }
    let mut app = start(&fixture, "space-analysis")?;
    app.send(keys::alt(KeyCode::Char('d')))?;
    app.wait_text("Folder totals")?;
    app.send(keys::ctrl('u'))?.send("node_modules")?;
    app.send(keys::tab().repeat(3))?.send("300")?;
    app.send(keys::tab())?.send("30")?;
    app.send(keys::enter())?;
    let results = app.wait_until("measured cache including hidden files", |s| {
        s.contains("Folder sizes: node_modules") && s.contains("finished") && s.contains("1/1 items")
    })?;
    ensure!(results.contains("40 files"), "file count missing: {}", results.text());
    ensure!(results.contains("400 content bytes"));
    fs::write(fixture.work().join("alpha/node_modules/.cache/file-0"), "x".repeat(100))?;
    app.send(keys::esc())?;
    app.wait_gone("Folder sizes: node_modules")?;
    app.send(keys::alt(KeyCode::Char('d')))?;
    app.wait_text("Folder totals")?;
    app.send(keys::ctrl('u'))?.send("node_modules")?.send(keys::enter())?;
    let cached = app.wait_until("Space analysis reuses the saved totals", |s| {
        s.contains("Cached") && s.contains("400 content bytes")
    })?;
    ensure!(cached.contains("Ctrl-R Rescan"));
    app.send(keys::ctrl('r'))?;
    app.wait_until("Space analysis rescans on request", |s| s.contains("finished") && s.contains("490 content bytes"))?;
    app.send(keys::esc())?;
    app.wait_gone("Folder sizes: node_modules")?;
    app.send(keys::alt(KeyCode::Char('R')))?;
    app.wait_text("Folder sizes: node_modules")?;
    app.send(keys::f(8))?;
    app.wait_text("permanently?")?;
    app.send(keys::enter())?;
    app.wait_gone("permanently?")?;
    ensure!(fixture.work().join("alpha/node_modules").is_dir(), "No should be selected initially");
    app.send(keys::f(8))?;
    app.wait_text("permanently?")?;
    app.send("y")?;
    app.wait_until("only selected folder deleted", |_| !fixture.work().join("alpha/node_modules").exists())?;
    ensure!(fixture.work().join("alpha/one.txt").exists());
    Ok(())
}

#[test]
fn folder_tree_expands_hides_refreshes_copies_and_reopens_saved_measurements() -> Result<()> {
    let fixture = fixture()?;
    fixture.file("alpha/cache/nested/data", &"x".repeat(100))?;
    let mut app = start(&fixture, "size-tree")?;
    app.send(keys::ctrl('s'))?.send("alpha")?.send(keys::enter())?;
    app.send(keys::alt(KeyCode::Char('s')))?;
    let screen = app.wait_until("selected folder measured", |s| {
        s.contains("Size tree:") && s.contains("finished") && s.contains("102 content bytes")
    })?;
    ensure!(screen.contains("←→ Branch") && screen.contains("Ctrl-R Rescan"));
    app.send(keys::down())?.send(keys::down())?.send(keys::right())?;
    app.wait_until("cache branch expanded", |s| left_panel(s).contains("nested"))?;
    app.send(keys::esc())?;
    let normal = app.wait_until("cached total in regular folder listing", |s| {
        !s.contains("Size tree:") && left_panel(s).contains("~102")
    })?;
    ensure!(normal.contains("cached"));
    app.send(keys::alt(KeyCode::Char('R')))?;
    let resumed = app.wait_text("Cached")?;
    ensure!(left_panel(&resumed).contains("nested"));
    ensure!(highlighted(&resumed, "cache"));
    fs::write(fixture.work().join("alpha/cache/nested/data"), "x".repeat(200))?;
    app.send(keys::ctrl('r'))?;
    app.wait_until("refreshed totals", |s| s.contains("finished") && s.contains("200 content bytes"))?;
    app.send(keys::f(5))?;
    app.wait_text("Copy")?;
    app.send(keys::ctrl('u'))?.send(fixture.work().join("beta").to_string_lossy().as_bytes())?;
    app.send(keys::enter())?;
    app.wait_until("copy from a real tree path", |_| fixture.work().join("beta/cache/nested/data").exists())?;
    ensure!(fs::read(fixture.work().join("beta/cache/nested/data"))?.len() == 200);
    ensure!(fixture.work().join("alpha/cache/nested/data").exists());
    app.send(keys::esc())?;
    app.wait_gone("Size tree:")?;
    app.send(keys::f(10))?.send("y")?;
    app.wait_exit()?;
    let mut app = start(&fixture, "size-tree-restored")?;
    app.send(keys::ctrl('s'))?.send("alpha")?.send(keys::enter())?;
    app.send(keys::alt(KeyCode::Char('s')))?;
    let saved =
        app.wait_until("saved scan reused after restart", |s| s.contains("Cached") && s.contains("202 content bytes"))?;
    ensure!(saved.contains("~202"));
    app.send(keys::down())?.send(keys::down())?.send(keys::f(8))?;
    app.wait_text("permanently?")?;
    app.send("y")?;
    app.wait_until("real directory deleted and size cache invalidated", |_| {
        !fixture.work().join("alpha/cache").exists()
    })?;
    app.wait_until("tree refreshed after delete", |s| s.contains("finished") && !left_panel(s).contains("nested"))?;
    app.send(keys::key(KeyCode::Home))?;
    app.wait_text("2 content bytes")?;
    ensure!(fixture.work().join("alpha/one.txt").exists());
    Ok(())
}

#[test]
fn activity_monitor_drills_into_process_memory_pauses_and_respects_plugin_toggle() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "activity-monitor")?;
    let pid = app.process_id().unwrap();
    app.wait_text("Borg Commander")?;
    app.send(keys::alt(KeyCode::Char('m')))?;
    app.wait_text("Activity monitor")?;
    app.send("/")?.send(pid.to_string())?.send(keys::enter())?;
    let list = app.wait_until("own process in activity list", |s| {
        s.contains("borg") && s.contains(&format!(" {pid} ")) && s.contains("/ edit")
    })?;
    ensure!(list.contains("RAM") && list.contains("Borg Commander"));
    app.send(keys::tab())?;
    app.wait_text("Process memory")?;
    app.wait_text("Resident memory")?;
    app.wait_text("Peak")?;
    app.send(keys::tab())?.send(keys::tab())?;
    app.send("/")?.send("Borg")?.send(keys::tab())?.send(keys::right())?.send(keys::right())?;
    app.wait_text("Unicode")?;
    app.send(keys::tab())?.send(keys::tab())?.send(" ")?;
    let form = app.wait_text("Current snapshot")?;
    ensure!(left_panel(&form).contains("Activity monitor"));
    app.send(keys::enter())?;
    app.wait_text("Capture memory first")?;
    app.send(keys::esc())?;
    app.wait_gone("Search memory")?;
    app.send(" ")?;
    app.wait_text("Paused")?;
    app.send(" ")?;
    app.wait_text("Live")?;
    app.send(keys::enter())?;
    app.wait_text("Memory map")?;
    let map = app.wait_until("memory map or a clear platform restriction", |s| {
        s.contains("Cannot inspect") || s.contains("G Raw")
    })?;
    ensure!(map.contains("Borg Commander"));
    ensure!(map.find("Memory map").unwrap().0 < map.cols() / 2);
    ensure!(map.find("Memory preview").unwrap().0 >= map.cols() / 2);
    app.send(keys::tab())?;
    app.wait_text("↑↓ Scroll")?;
    app.send(keys::esc())?;
    app.wait_text("Esc Processes")?;
    if map.contains("Cannot inspect") {
        ensure!(map.contains("Administrator"));
    }
    app.send(keys::esc())?;
    app.wait_text("Activity monitor")?;
    app.wait_until("process details ready", |s| s.contains("2 Open files") && !s.contains("Memory preview"))?;
    app.send("2")?;
    let files = app.wait_until("open files loaded for own process", |s| {
        s.contains("Open files") && s.contains("Descriptor") && s.contains("File details")
    })?;
    ensure!(files.contains("File details") && files.contains("FD    Use Type"));
    app.send(keys::down())?;
    app.send("1")?;
    app.wait_text("Memory map")?;
    app.send("2")?;
    app.wait_text("File details")?;
    app.send(keys::esc())?;
    app.wait_text("Activity monitor")?;
    app.send(keys::esc())?;
    app.wait_text("/ Filter processes")?;
    app.send(keys::esc())?;
    app.wait_until("file panels after closing activity monitor", |s| {
        panels_ready(s) && !s.contains("Activity monitor")
    })?;
    choose(&mut app, OPTIONS_MENU, "Plugins…")?;
    let plugin = app.wait_text("System — processes and memory")?;
    let (x, y) = plugin.find("System — processes and memory").unwrap();
    app.send(mouse(0, x, y))?;
    app.wait_text("[ ] System")?;
    app.send(keys::esc())?;
    app.wait_until("system plugin setting saved", |_| {
        fs::read_to_string(fixture.config_file()).is_ok_and(|s| s.contains("system = false"))
    })?;
    app.send(keys::alt(KeyCode::Char('m')))?;
    app.wait_text("Enable System")?;
    app.send(keys::f(10))?.send("y")?;
    app.wait_exit()?;
    let mut app = start(&fixture, "activity-disabled")?;
    app.send(keys::alt(KeyCode::Char('m')))?;
    app.wait_text("Enable System")?;
    Ok(())
}

const BOTTOM_LEFT_CORNERS: [char; 4] = ['└', '╰', '┗', '╚'];
const ACTIVITY_FIRST_ROW: u16 = 4;

fn activity_rows(s: &Snapshot) -> std::ops::Range<u16> {
    let bottom = s.lines().iter().skip(1).position(|l| l.starts_with(BOTTOM_LEFT_CORNERS)).unwrap_or(0) as u16 + 1;
    ACTIVITY_FIRST_ROW..bottom.saturating_sub(1)
}

fn activity_cursor(s: &Snapshot) -> Option<u16> {
    let rows: Vec<_> = activity_rows(s).map(|y| (y, s.style_at(2, y).bg)).collect();
    (rows.len() > 2).then_some(())?;
    rows.iter().find(|(_, bg)| rows.iter().filter(|(_, other)| other == bg).count() == 1).map(|(y, _)| *y)
}

struct Sleepers(Vec<std::process::Child>);

impl Drop for Sleepers {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn activity_cursor_moves_up_from_the_bottom_without_dragging_the_viewport() -> Result<()> {
    const STEPS: u16 = 3;
    let _sleepers = Sleepers(
        (0..24).map(|_| std::process::Command::new("sleep").arg("60").spawn()).collect::<std::io::Result<_>>()?,
    );
    let fixture = fixture()?;
    let mut app = start(&fixture, "activity-scroll")?;
    app.send(keys::alt(KeyCode::Char('m')))?;
    app.wait_until("process rows", |s| s.contains("Activity monitor") && activity_cursor(s).is_some())?;
    app.send(" ")?;
    app.wait_text("Paused")?;
    app.send(keys::key(KeyCode::End))?;
    let bottom = app.wait_until("cursor on the last visible row", |s| {
        activity_cursor(s).is_some_and(|y| y + 1 == activity_rows(s).end)
    })?;
    let last = activity_rows(&bottom).end - 1;
    let top = left_panel(&bottom).lines().nth(usize::from(ACTIVITY_FIRST_ROW)).unwrap_or_default().to_owned();
    for _ in 0..STEPS {
        app.send(keys::up())?;
    }
    let moved = app.wait_until("cursor moved up inside the viewport", |s| activity_cursor(s) == Some(last - STEPS))?;
    let still = left_panel(&moved).lines().nth(usize::from(ACTIVITY_FIRST_ROW)).unwrap_or_default().to_owned();
    ensure!(still == top, "viewport scrolled while the cursor moved up: {top:?} became {still:?}");
    Ok(())
}

#[test]
fn console_remembers_its_view_and_option_arrows_edit_words_in_each_shell() -> Result<()> {
    let console_prompt =
        |s: &Snapshot| !panels_ready(s) && s.cursor().is_some_and(|(_, y)| s.lines()[usize::from(y)].trim() == "$");
    for (shell, _) in Shell::available() {
        let fixture = fixture()?;
        fixture.configure("return-to-panels = true\n")?;
        let mut app = start_with(&fixture, &format!("console-view-{}", shell.program()), shell)?;
        app.send("touch from-panels")?.send(keys::enter())?;
        app.wait_until("panel command finished", |s| {
            fixture.work().join("from-panels").exists() && panels_ready(s) && left_panel(s).contains("from-panels")
        })?;
        ensure!(panels_ready(&app.settle()?), "panel command has not returned to panels");
        app.send(keys::ctrl('o'))?;
        app.wait_until("console opened explicitly", |s| !panels_ready(s))?;
        for name in ["console-one", "console-two"] {
            app.send(format!("touch {name}"))?.send(keys::enter())?;
            app.wait_until("console command ran", |s| {
                fixture.work().join(name).exists() && s.contains(&format!("touch {name}")) && console_prompt(s)
            })?;
            ensure!(!panels_ready(&app.settle()?), "{} returned to panels after a console command", shell.program());
        }
        app.send("touch word left")?
            .send(keys::alt(KeyCode::Left))?
            .send("previous-")?
            .send(keys::alt(KeyCode::Right))?
            .send("-next")?
            .send(keys::enter())?;
        app.wait_until("Option arrows moved between words", |s| {
            fixture.work().join("previous-left-next").exists()
                && s.contains("touch word previous-left-next")
                && console_prompt(s)
        })?;
        ensure!(!fixture.work().join("left").exists());
        ensure!(!panels_ready(&app.settle()?));
        app.send("touch cancelled")?;
        app.wait_until("command typed before cancelling", |s| {
            s.cursor().is_some_and(|(_, y)| s.lines()[usize::from(y)].trim() == "$ touch cancelled")
        })?;
        app.send(keys::ctrl('c'))?;
        app.wait_until("cancelled command returned to prompt", console_prompt)?;
        ensure!(!panels_ready(&app.settle()?), "Ctrl-C at the console prompt changed the view");
        ensure!(!fixture.work().join("cancelled").exists());
        app.send("touch running; sleep 30")?.send(keys::enter())?;
        app.wait_until("interruptible command started", |s| {
            fixture.work().join("running").exists() && s.contains("touch running; sleep 30")
        })?;
        app.send(keys::ctrl('c'))?;
        app.wait_until("shell prompt returned after Ctrl-C", console_prompt)?;
        app.send("touch after-interrupt")?.send(keys::enter())?;
        app.wait_until("console accepted command after Ctrl-C", |_| fixture.work().join("after-interrupt").exists())?;
        ensure!(!panels_ready(&app.settle()?), "Ctrl-C on a running command changed the view");
        app.send(keys::ctrl('o'))?;
        app.wait_until("back to panels by request", panels_ready)?;
        app.send("touch panels-again")?.send(keys::enter())?;
        app.wait_until("panel return preference still applies", |s| {
            fixture.work().join("panels-again").exists() && panels_ready(s)
        })?;
    }
    Ok(())
}

#[test]
fn background_folder_sizes_keep_panels_usable_and_reuse_the_tree_cache() -> Result<()> {
    let fixture = fixture()?;
    fixture.file("alpha/inner/data", &"x".repeat(2_000_000))?;
    fixture.file("beta/data", &"x".repeat(1000))?;
    let mut app = start(&fixture, "background-sizes")?;
    let screen = app.wait_text("Alt-I Count sizes")?;
    ensure!(panels_ready(&screen));
    app.send(keys::alt(KeyCode::Char('i')))?;
    let screen = app.wait_text("Sizes measured")?;
    ensure!(panels_ready(&screen));
    ensure!(!screen.contains("Size tree:"));
    let screen = app.wait_until("directory rows contain measured sizes", |s| {
        left_panel(s).contains("~1.9M") && left_panel(s).contains("~1000")
    })?;
    ensure!(!screen.contains("Alt-I Stop sizes"));
    app.send(keys::ctrl('s'))?.send("alpha")?.send(keys::enter())?.send(keys::enter())?;
    app.wait_text("inner")?;
    app.wait_until("nested directory totals also persisted", |s| left_panel(s).contains("~1.9M"))?;
    app.send(keys::left())?;
    app.wait_until("returned to scan root", |s| panels_ready(s) && left_panel(s).contains("beta"))?;
    app.send(keys::alt(KeyCode::Char('D')))?;
    let screen = app.wait_text("Cached")?;
    ensure!(screen.contains("Size tree:"));
    Ok(())
}

/// A stand-in for the `docker` CLI: one Compose project with one container, a stats
/// stream, and a stop that the next listing reflects. It logs every call.
const FAKE_DOCKER: &str = r#"#!/bin/sh
here="$(dirname "$0")"
echo "$*" >> "$here/calls.log"
web=f00d000000000000000000000000000000000000000000000000000000000001
case "$1" in
context) echo fake-engine ;;
ps)
    if [ -f "$here/stopped" ]; then state=exited; status="Exited (0) 1 second ago"; else state=running; status="Up 5 minutes"; fi
    printf '{"id":"%s","name":"shop-web-1","image":"nginx:1.27","command":"\\"nginx\\"","created":"2026-10-22 09:00:00 +0000 UTC","state":"%s","status":"%s","ports":"","networks":"shop_default","project":"shop","service":"web","dir":"/srv/shop","files":"/srv/shop/compose.yaml"}\n' "$web" "$state" "$status"
    ;;
stats)
    while :; do
        printf '\033[H{"BlockIO":"1MB / 0B","CPUPerc":"12.50%%","Container":"%s","ID":"%s","MemPerc":"2%%","MemUsage":"200MiB / 8GiB","Name":"shop-web-1","NetIO":"1kB / 2kB","PIDs":"9"}\033[K\n\033[K\n\033[J' "$web" "$web"
        sleep 0.3
    done
    ;;
inspect) echo '{"started":"2026-10-22T09:00:01Z","finished":"0001-01-01T00:00:00Z","exit":0,"oom":false,"error":"","health":null,"restarts":0,"policy":"always","entrypoint":null,"cmd":["nginx"],"workdir":"","platform":"linux","mounts":[],"networks":{"shop_default":{"IPAddress":"172.18.0.4"}}}' ;;
stop) touch "$here/stopped" ;;
exec)
    if [ "$2" = "-i" ]; then
        # Writes a file in the container's filesystem, which lives in root/ beside this script.
        sh -c "$6" sh "$here/root$8"
        exit
    fi
    if [ "$2" = "-it" ]; then
        # Run Borg's own start-up script with its environment, as a container would.
        shift 2
        while [ "$1" = "-e" ] || [ "$1" = "-w" ]; do
            if [ "$1" = "-e" ]; then export "$2"; else cd "$2" || exit 1; fi
            shift 2
        done
        shift
        PS1='box$ '
        export PS1
        exec "$@"
    fi
    case "$3" in
    sh)
        case "$5" in
        "cd --"*)
            case "$7" in
            /) printf '41ed/4096/1729600000/etc\n81a4/12/1729600001/motd\n/\n' ;;
            *) printf '/\n' ;;
            esac
            ;;
        "for p;"*)
            shift 6
            for p; do if [ "$p" = /motd ]; then printf '12/1729600001//motd\n'; fi; done
            ;;
        '[ -d'*) sh -c "$5" sh "$here/root$7" "$8" "$9" ;;
        esac
        ;;
    rm|mkdir) ;;
    *) echo "unexpected: $*" >&2; exit 1 ;;
    esac
    ;;
cp)
    case "$2" in
    "$web":*) if [ -d "$3" ]; then echo "welcome aboard" > "$3/$(basename "${2#*:}")"; else echo "welcome aboard" > "$3"; fi ;;
    esac
    ;;
*) echo "unexpected: $*" >&2; exit 1 ;;
esac
"#;

#[test]
fn docker_view_lists_containers_streams_stats_and_stops_one() -> Result<()> {
    let fixture = fixture()?;
    let bin = fixture.home().join("bin");
    fs::create_dir_all(&bin)?;
    let docker = bin.join("docker");
    fs::write(&docker, FAKE_DOCKER)?;
    fs::set_permissions(&docker, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    let mut app = Session::builder(BORG, &fixture).path(&bin).label("docker").spawn()?;
    app.wait_text(READY)?;
    app.send(keys::alt(KeyCode::Char('c')))?;
    app.wait_text("Docker · fake-engine")?;
    app.wait_text("12.5")?;
    app.send(keys::down())?;
    app.wait_text("shop › web")?;
    app.wait_text("172.18.0.4")?;
    app.send(keys::key(KeyCode::Char('x')))?;
    app.wait_text("Stop web?")?;
    app.send(keys::enter())?;
    app.wait_text("Exited (0)")?;
    let calls = fs::read_to_string(bin.join("calls.log"))?;
    ensure!(calls.lines().any(|line| line.starts_with("stop f00d")), "the stop reached docker:\n{calls}");
    app.send(keys::ctrl('o'))?;
    app.wait_text("Borg console")?;
    app.send(keys::ctrl('o'))?;
    app.wait_text("Docker · fake-engine")?;
    app.send(keys::esc())?;
    app.wait_text(READY)?;
    Ok(())
}

#[test]
fn docker_files_open_in_a_panel_and_copy_both_ways() -> Result<()> {
    let fixture = fixture()?;
    let bin = fixture.home().join("bin");
    fs::create_dir_all(&bin)?;
    let docker = bin.join("docker");
    fs::write(&docker, FAKE_DOCKER)?;
    fs::set_permissions(&docker, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    let mut app = Session::builder(BORG, &fixture).path(&bin).label("docker-files").spawn()?;
    app.wait_text(READY)?;
    app.send(keys::alt(KeyCode::Char('c')))?;
    app.wait_text("shop")?;
    app.send(keys::down())?;
    app.wait_text("shop › web")?;
    app.send(keys::key(KeyCode::Char('f')))?;
    app.wait_text("▣ shop-web-1")?;
    app.wait_text("motd")?;
    app.send(keys::down())?.send(keys::down())?.send(keys::f(5))?;
    app.wait_text("Copy")?;
    app.send(keys::enter())?;
    app.wait_text("Copied 1 file")?;
    ensure!(fs::read_to_string(fixture.work().join("motd"))? == "welcome aboard\n", "the file reached the host");
    app.send(keys::tab())?.send(keys::key(KeyCode::End))?.send(keys::f(5))?;
    app.wait_text("Copy")?;
    app.send(keys::enter())?;
    app.wait_until("copied into the container", |_| {
        fs::read_to_string(bin.join("calls.log"))
            .is_ok_and(|calls| calls.lines().any(|l| l.starts_with("cp ") && l.ends_with(":/")))
    })?;
    app.wait_text("Copied 1 file")?;

    // motd is in both places now: copying it in asks, and Skip leaves the container's copy.
    app.send(keys::key(KeyCode::Home))?.send(keys::ctrl('s'))?.send("motd")?.send(keys::enter())?;
    app.send(keys::f(5))?;
    app.wait_text("Copy")?;
    app.send(keys::enter())?;
    app.wait_text("Already exists")?;
    app.send("s")?;
    app.wait_text("1 skipped")?;
    let calls = fs::read_to_string(bin.join("calls.log"))?;
    ensure!(
        !calls.lines().any(|l| l.starts_with("cp ") && l.contains("/motd f00d")),
        "the skipped file stayed:\n{calls}"
    );

    // Copying it out again asks too, and Keep both saves it under a new name.
    app.send(keys::tab())?.send(keys::key(KeyCode::Home))?.send(keys::down())?.send(keys::down())?.send(keys::f(5))?;
    app.wait_text("Copy")?;
    app.send(keys::enter())?;
    app.wait_text("Already exists")?;
    app.send("k")?;
    app.wait_until("kept both", |_| fixture.work().join("motd (2)").exists())?;
    ensure!(fs::read_to_string(fixture.work().join("motd"))? == "welcome aboard\n");

    app.send(keys::key(KeyCode::Home))?.send(keys::enter())?;
    app.wait_gone("▣ shop-web-1")?;
    app.wait_text("6 items")?;
    Ok(())
}

#[test]
fn docker_shell_runs_beside_the_list_and_stays_open_in_the_background() -> Result<()> {
    let fixture = fixture()?;
    let bin = fixture.home().join("bin");
    fs::create_dir_all(&bin)?;
    let docker = bin.join("docker");
    fs::write(&docker, FAKE_DOCKER)?;
    fs::set_permissions(&docker, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    let mut app = Session::builder(BORG, &fixture).path(&bin).size(140, 30).label("docker-shell").spawn()?;
    app.wait_text(READY)?;
    app.send(keys::alt(KeyCode::Char('c')))?;
    app.wait_text("shop")?;
    app.send(keys::down())?;
    app.wait_text("shop › web")?;
    app.send(keys::key(KeyCode::Char('e')))?;
    app.wait_text("shop-web-1  $")?;
    app.wait_text("Keys go to the shell in web")?;
    app.send("echo inside-$((6*7))\r")?;
    app.wait_text("inside-42")?;
    app.send(keys::alt(KeyCode::Char('1')))?;
    app.wait_text("nginx:1.27")?;
    app.send(keys::alt(KeyCode::Char('2')))?;
    app.wait_text("inside-42")?;
    app.wait_text("Keys go to the shell in web")?;
    app.send(keys::alt(KeyCode::Char('c')))?;
    app.wait_text("S Start")?;
    app.wait_text("›_")?;
    app.send(keys::esc())?;
    app.wait_text("Alt-C Docker · 1 shell")?;
    app.send(keys::f(10))?;
    app.wait_text("A container shell is open")?;
    Ok(())
}

fn fake_docker(fixture: &Fixture) -> Result<std::path::PathBuf> {
    let bin = fixture.home().join("bin");
    fs::create_dir_all(&bin)?;
    let docker = bin.join("docker");
    fs::write(&docker, FAKE_DOCKER)?;
    fs::set_permissions(&docker, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    Ok(bin)
}

fn browse_web(app: &mut Session) -> Result<()> {
    app.send(keys::alt(KeyCode::Char('c')))?;
    app.wait_text("shop")?;
    app.send(keys::down())?;
    app.wait_text("shop › web")?;
    app.send(keys::key(KeyCode::Char('f')))?;
    app.wait_text("▣ shop-web-1")?;
    Ok(())
}

#[test]
fn the_command_line_follows_a_container_panel_when_enabled() -> Result<()> {
    let fixture = fixture()?;
    fixture.configure("return-to-panels = true\n")?;
    let bin = fake_docker(&fixture)?;
    let mut app = Session::builder(BORG, &fixture).path(&bin).size(120, 30).label("docker-console").spawn()?;
    app.wait_text(READY)?;
    browse_web(&mut app)?;
    app.wait_until("the container's prompt", |s| prompt(s).contains(" shop-web-1  $"))?;
    app.send("echo inside-$((6*7))\r")?;
    app.pump(std::time::Duration::from_millis(800))?;
    app.wait_until("the command finishes and the panels return", |s| {
        s.contains("▣ shop-web-1") && s.contains("motd") && prompt(s).trim_end().ends_with('$')
    })?;
    app.send(keys::ctrl('o'))?;
    let console = app.wait_text("shop-web-1 console")?;
    ensure!(console.contains("inside-42"), "Ctrl-O shows the container's output\n{}", console.text());
    app.send(keys::ctrl('o'))?.send(keys::tab())?;
    app.wait_until("the host's prompt", |s| prompt(s).starts_with('$'))?;
    app.send(keys::ctrl('o'))?;
    let host = app.wait_text("Borg console")?;
    ensure!(!host.contains("inside-42"), "the host's console has its own output\n{}", host.text());
    Ok(())
}

#[test]
fn the_host_command_line_waits_for_a_host_panel_when_it_does_not_follow() -> Result<()> {
    let fixture = fixture()?;
    fixture.configure("[plugins]\ndocker-console = false\n")?;
    let bin = fake_docker(&fixture)?;
    let mut app = Session::builder(BORG, &fixture).path(&bin).size(120, 30).label("docker-host-line").spawn()?;
    app.wait_text(READY)?;
    browse_web(&mut app)?;
    app.wait_text("⌂ host")?;
    app.send("echo nope\r")?;
    app.wait_text("This command line runs on the client")?;
    ensure!(!app.snapshot().contains("Borg console"), "nothing ran");
    Ok(())
}

/// `borg mcp`, as an agent starts it, talking MCP over stdio to the borg open on the machine.
struct Mcp {
    child: std::process::Child,
    input: std::process::ChildStdin,
    output: std::io::BufReader<std::process::ChildStdout>,
    next: u64,
}

impl Mcp {
    fn start(fixture: &Fixture) -> Result<Self> {
        Self::start_with_path(fixture, Path::new(""))
    }

    /// Starts `borg mcp` with `dir` ahead of `PATH`, where a fake `docker` may be.
    fn start_with_path(fixture: &Fixture, dir: &Path) -> Result<Self> {
        let path = std::env::join_paths(
            std::iter::once(dir.to_path_buf())
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())),
        )?;
        let mut child = std::process::Command::new(BORG)
            .arg("mcp")
            .env("PATH", path)
            .env("XDG_RUNTIME_DIR", fixture.runtime())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()?;
        let input = child.stdin.take().unwrap();
        let output = std::io::BufReader::new(child.stdout.take().unwrap());
        Ok(Self { child, input, output, next: 0 })
    }

    fn request(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        use std::io::{BufRead, Write};
        self.next += 1;
        let message = serde_json::json!({ "jsonrpc": "2.0", "id": self.next, "method": method, "params": params });
        writeln!(self.input, "{message}")?;
        let mut line = String::new();
        self.output.read_line(&mut line)?;
        let reply: serde_json::Value = serde_json::from_str(&line)?;
        ensure!(reply["id"] == self.next, "answers come back in order: {reply}");
        Ok(reply["result"].clone())
    }

    fn call(&mut self, tool: &str, arguments: serde_json::Value) -> Result<(bool, String)> {
        let result = self.request("tools/call", serde_json::json!({ "name": tool, "arguments": arguments }))?;
        Ok((result["isError"] == true, result["content"][0]["text"].as_str().unwrap_or_default().to_owned()))
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn an_agent_sees_reads_shows_and_changes_only_what_the_user_approves() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "agent")?;
    let mut mcp = Mcp::start(&fixture)?;
    let init = mcp.request("initialize", serde_json::json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } }))?;
    ensure!(init["serverInfo"]["name"] == "borg", "{init}");
    let tools = mcp.request("tools/list", serde_json::json!({}))?;
    let names: Vec<&str> = tools["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    ensure!(
        [
            "screen",
            "read",
            "show",
            "write",
            "edit",
            "run",
            "type",
            "list_sessions",
            "use_session",
            "list_remote_sessions",
            "approval_mode"
        ]
        .iter()
        .all(|name| names.contains(name)),
        "{names:?}"
    );
    app.wait_text("Remote connected to Borg")?;

    let (error, screen) = mcp.call("screen", serde_json::json!({}))?;
    ensure!(!error && screen.contains("Active panel: left") && screen.contains("highlighted: .."), "{screen}");
    let (_, notes) = mcp.call("read", serde_json::json!({ "path": "notes.txt" }))?;
    ensure!(notes.ends_with("lines 1–1 of 1\n     1\thello\n"), "{notes}");
    let (_, listing) = mcp.call("read", serde_json::json!({ "path": "alpha" }))?;
    ensure!(listing.contains("inner/") && listing.contains("one.txt  2 bytes"), "{listing}");
    let (_, here) = mcp.call("read", serde_json::json!({}))?;
    ensure!(here.lines().next().is_some_and(|line| line.contains("/work (")), "the active folder: {here}");
    let (error, processes) =
        mcp.call("read", serde_json::json!({ "source": "processes", "sort": "cpu", "limit": 3 }))?;
    let rows = processes
        .lines()
        .skip(2)
        .filter(|line| line.split_whitespace().next().is_some_and(|pid| pid.parse::<u32>().is_ok()));
    ensure!(!error && processes.starts_with("CPU ") && rows.count() == 3, "{processes}");
    let (error, text) = mcp.call("read", serde_json::json!({ "path": "notes.txt", "lines": "x" }))?;
    ensure!(error && text.contains("not like 120-180"), "{text}");

    mcp.call("show", serde_json::json!({ "path": "alpha/one.txt" }))?;
    let (_, screen) = mcp.call("screen", serde_json::json!({}))?;
    ensure!(
        screen.contains("right panel: ") && screen.contains("/work/alpha\n  highlighted: one.txt"),
        "the reveal is done when it answers:\n{screen}"
    );

    // A new file shows whole, scrolling through its lines, before the user approves it.
    let content: String = (1..=100).map(|n| format!("step {n}\n")).collect();
    let asking = std::thread::spawn(move || {
        let created = mcp.call("write", serde_json::json!({ "path": "plan.txt", "content": content }));
        (mcp, created)
    });
    app.wait_text("Remote wants to create")?;
    app.wait_text("of 100")?;
    app.send(keys::key(KeyCode::End))?;
    app.wait_text("100 + step 100")?;
    approve(&mut app)?;
    let (mut mcp, created) = asking.join().unwrap();
    let (error, text) = created?;
    ensure!(!error && text.contains("Created") && text.contains("plan.txt (100 lines)"), "{text}");
    ensure!(fs::read_to_string(fixture.work().join("plan.txt"))?.ends_with("step 100\n"));

    // An edit shows as a diff; one whose text is not in the file fails without asking.
    let (error, text) = mcp.call("edit", serde_json::json!({ "path": "plan.txt", "old": "step 999", "new": "x" }))?;
    ensure!(error && text.contains("was not found"), "{text}");
    let (error, text) = mcp.call("edit", serde_json::json!({ "path": "plan.txt", "old": "step 5", "new": "x" }))?;
    ensure!(error && text.contains("matches 11 places"), "{text}");
    let asking = std::thread::spawn(move || {
        let edited =
            mcp.call("edit", serde_json::json!({ "path": "plan.txt", "old": "step 50\n", "new": "step fifty\n" }));
        (mcp, edited)
    });
    app.wait_text("Remote wants to edit")?;
    app.wait_until("the diff", |s| s.contains("50 - step 50") && s.contains("50 + step fifty"))?;
    approve(&mut app)?;
    let (mut mcp, edited) = asking.join().unwrap();
    ensure!(!edited?.0, "the edit is written");
    ensure!(fs::read_to_string(fixture.work().join("plan.txt"))?.contains("step 49\nstep fifty\nstep 51\n"));

    // Typed text waits at the prompt: only the user's Enter runs it.
    let (error, text) = mcp.call("type", serde_json::json!({ "text": "touch typed-by-claude" }))?;
    ensure!(!error && text.contains("nothing ran"), "{text}");
    app.wait_until("the text at the prompt", |s| prompt(s).contains("touch typed-by-claude"))?;
    let (_, screen) = mcp.call("screen", serde_json::json!({}))?;
    ensure!(
        screen.contains("looking at the panels")
            && screen.contains("at its prompt, with \"touch typed-by-claude\" typed there")
            && screen.contains("The screen as the user sees it")
            && screen.contains("notes.txt"),
        "screen shows what is typed and the screen itself:\n{screen}"
    );
    app.pump(std::time::Duration::from_millis(300))?;
    ensure!(!fixture.work().join("typed-by-claude").exists(), "typing runs nothing");
    app.send(keys::enter())?;
    app.wait_until("the user ran it", |_| fixture.work().join("typed-by-claude").exists())?;
    app.send(keys::ctrl('o'))?;
    app.wait_text("6 items")?;

    // A command waits for the user: n declines it, y types it into Borg's console. The
    // dialog shows over the console too, where the user may be.
    app.send(keys::ctrl('o'))?;
    app.wait_text("Borg console")?;
    let (_, screen) = mcp.call("screen", serde_json::json!({}))?;
    ensure!(
        screen.contains("looking at Borg's console") && screen.contains("$ touch typed-by-claude"),
        "screen knows the console shows, and what it shows:\n{screen}"
    );
    let mut declined = Mcp::start(&fixture)?;
    declined.request("initialize", serde_json::json!({ "protocolVersion": "2025-06-18" }))?;
    let asking = std::thread::spawn(move || declined.call("run", serde_json::json!({ "command": "touch nope" })));
    app.wait_until("the dialog over the console", |s| s.contains("Remote wants to run") && s.contains("Borg console"))?;
    app.checkpoint("approval-over-console")?;
    app.send("n")?;
    let (error, text) = asking.join().unwrap()?;
    ensure!(error && text.contains("declined") && !fixture.work().join("nope").exists(), "{text}");

    let asking = std::thread::spawn(move || {
        mcp.call("run", serde_json::json!({ "command": "echo from-claude > made.txt; echo done-$((6*7))" }))
    });
    app.wait_text("echo from-claude > made.txt; echo done-$((6*7))")?;
    approve(&mut app)?;
    let (error, text) = asking.join().unwrap()?;
    ensure!(!error && text == "exit 0\ndone-42\n", "{text:?}");
    ensure!(fs::read_to_string(fixture.work().join("made.txt"))? == "from-claude\n");
    app.wait_until("the command ran in the console the user sees", |s| {
        s.contains(" echo from-claude > made.txt") && s.contains("done-42")
    })?;
    Ok(())
}

#[test]
fn the_connect_dialog_copies_a_command_that_starts_claude_on_this_borg() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "agent-connect")?;
    app.send(keys::f(1))?;
    app.wait_text("C  Remote access to this borg")?;
    app.send("c")?;
    app.wait_text("Start Claude Code connected to this borg")?;
    app.wait_text("claude --mcp-config")?;
    // Without a relay, borg cloud says what is missing, and Enter copies nothing there.
    app.wait_text("No relay is set")?;
    app.send(keys::tab())?;
    app.send(keys::enter())?;
    app.wait_text("Copied; paste it into a terminal on your computer")?;
    Ok(())
}

#[test]
fn a_relay_typed_into_the_connect_dialog_is_saved_and_gives_a_pairing_link() -> Result<()> {
    let fixture = fixture()?;
    let relay = relay()?;
    let mut app = start(&fixture, "agent-relay")?;
    app.send(keys::f(1))?;
    app.wait_text("C  Remote access to this borg")?;
    app.send("c")?;
    app.wait_text("No relay is set")?;
    app.send(&relay)?;
    app.wait_text("Enter saves the relay")?;
    app.send(keys::enter())?;
    let shown = app.wait_until("the pairing link", |s| pairing_link(s).is_some())?;
    let link = pairing_link(&shown).unwrap();
    ensure!(link.starts_with(&format!("borg+{relay}/")), "{link}");
    let config = fs::read_to_string(fixture.config_file())?;
    ensure!(config.contains(&format!("relay = \"{relay}\"")), "the relay is saved: {config}");
    Ok(())
}

#[test]
fn f3_shows_the_pairing_command_alone_to_select_where_the_clipboard_does_not_work() -> Result<()> {
    let fixture = fixture()?;
    let relay = relay()?;
    fixture.configure(&format!("return-to-panels = false\nrelay = \"{relay}\"\n"))?;
    let mut app = start(&fixture, "agent-plain")?;
    app.send(keys::f(1))?;
    app.wait_text("C  Remote access to this borg")?;
    app.send("c")?;
    let shown = app.wait_until("the pairing link", |s| pairing_link(s).is_some())?;
    let link = pairing_link(&shown).unwrap();
    app.send(keys::f(3))?;
    let plain = app.wait_until("the command alone", |s| s.contains("Press any key to return to borg"))?;
    // The terminal wraps the command itself, so its rows join into the link again.
    let lines = plain.lines();
    let first = lines.iter().position(|line| line.starts_with("borg pair '")).context("the command is shown")?;
    let command: String =
        lines[first..].iter().take_while(|line| !line.is_empty()).map(|line| line.trim_end()).collect();
    ensure!(command == format!("borg pair '{link}'"), "{command}\n{}", lines.join("\n"));
    ensure!(!lines.iter().any(|line| line.contains('│')), "nothing of borg's screen: {}", lines.join("\n"));
    app.send("x")?;
    app.wait_text("Remote access")?;
    app.wait_text("Tab picks")?;
    Ok(())
}

#[test]
fn an_agent_edits_container_files_and_works_in_a_container_shell_the_user_watches() -> Result<()> {
    let fixture = fixture()?;
    let bin = fake_docker(&fixture)?;
    fs::create_dir_all(bin.join("root/etc"))?;
    fs::write(bin.join("root/etc/app.conf"), "name = shop\nport = 80\n")?;
    let mut app = Session::builder(BORG, &fixture).path(&bin).size(130, 30).label("agent-docker").spawn()?;
    app.wait_text(READY)?;
    let mut mcp = Mcp::start_with_path(&fixture, &bin)?;
    mcp.request("initialize", serde_json::json!({ "protocolVersion": "2025-06-18" }))?;
    let (_, containers) = mcp.call("read", serde_json::json!({ "source": "containers" }))?;
    ensure!(containers.contains("shop-web-1") && containers.contains("Running"), "{containers}");

    let (error, text) = mcp.call("read", serde_json::json!({ "path": "docker://web/etc/app.conf", "find": "port" }))?;
    ensure!(!error && text.contains("     2\tport = 80"), "{text}");
    let asking = std::thread::spawn(move || {
        let edited = mcp.call(
            "edit",
            serde_json::json!({ "path": "docker://web/etc/app.conf", "old": "port = 80", "new": "port = 8080" }),
        );
        (mcp, edited)
    });
    app.wait_text("Remote wants to edit")?;
    app.wait_text("/etc/app.conf in the shop-web-1 container")?;
    approve(&mut app)?;
    let (mut mcp, edited) = asking.join().unwrap();
    let (error, text) = edited?;
    ensure!(!error && text.contains("docker://shop-web-1/etc/app.conf"), "{text}");
    ensure!(fs::read_to_string(bin.join("root/etc/app.conf"))? == "name = shop\nport = 8080\n");

    let asking = std::thread::spawn(move || {
        let first =
            mcp.call("run", serde_json::json!({ "at": "docker://web", "command": "cd /tmp && echo in-$((6*7))" }));
        (mcp, first)
    });
    app.wait_text("cd /tmp && echo in-$((6*7))")?;
    approve(&mut app)?;
    let (mut mcp, first) = asking.join().unwrap();
    let (error, text) = first?;
    ensure!(!error && text == "exit 0\nin-42\n", "{text:?}");
    app.wait_until("the user watches it in the Docker view's shell", |s| {
        s.contains("Alt-2 Shell ●") && s.contains("in-42")
    })?;

    let asking = std::thread::spawn(move || {
        mcp.call("run", serde_json::json!({ "at": "docker://shop-web-1", "command": "pwd" }))
    });
    app.wait_text("Remote wants to run")?;
    approve(&mut app)?;
    let (error, text) = asking.join().unwrap()?;
    ensure!(!error && text.trim_end().ends_with("/tmp"), "the shell keeps its folder between commands: {text:?}");
    Ok(())
}

#[test]
fn option_left_and_right_move_by_words_on_the_command_line() -> Result<()> {
    let fixture = fixture()?;
    let mut app = start(&fixture, "word-moves")?;
    app.send("echo one two")?;
    app.wait_until("typed", |s| prompt(s).contains("echo one two"))?;
    // Option-Left and Option-Right as terminals send them: Esc b and Esc f.
    app.send("\x1bb")?;
    app.send("X")?;
    app.wait_until("the cursor moved a word left", |s| prompt(s).contains("echo one Xtwo"))?;
    app.send("\x1bb\x1bb\x1bf")?;
    app.send("Y")?;
    app.wait_until("and a word right", |s| prompt(s).contains("echo oneY Xtwo"))?;
    ensure!(!app.snapshot().contains("Collection"), "no collections panel opened");
    Ok(())
}

/// Runs a relay without TLS in this process, on a free port, for the test's lifetime.
fn relay() -> Result<String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    listener.set_nonblocking(true)?;
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("a runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("the listener");
            borg_relay::serve(listener, borg_relay::Options::default()).await;
        });
    });
    Ok(format!("ws://{address}"))
}

fn relay_with_idle_proxy(idle: std::time::Duration) -> Result<String> {
    let target = relay()?.trim_start_matches("ws://").to_owned();
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    listener.set_nonblocking(true)?;
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("a runtime");
        runtime.block_on(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let listener = tokio::net::TcpListener::from_std(listener).expect("the proxy listener");
            while let Ok((client, _)) = listener.accept().await {
                let target = target.clone();
                tokio::spawn(async move {
                    let Ok(server) = tokio::net::TcpStream::connect(target).await else { return };
                    let (mut client_read, mut client_write) = client.into_split();
                    let (mut server_read, mut server_write) = server.into_split();
                    let mut outbound = [0; 64 * 1024];
                    let mut inbound = [0; 64 * 1024];
                    let mut active = tokio::time::Instant::now();
                    loop {
                        tokio::select! {
                            next = client_read.read(&mut outbound) => {
                                let Ok(count) = next else { break };
                                if count == 0 || server_write.write_all(&outbound[..count]).await.is_err() { break; }
                                active = tokio::time::Instant::now();
                            }
                            next = server_read.read(&mut inbound) => {
                                let Ok(count) = next else { break };
                                if count == 0 || client_write.write_all(&inbound[..count]).await.is_err() { break; }
                            }
                            () = tokio::time::sleep_until(active + idle) => break,
                        }
                    }
                });
            }
        });
    });
    Ok(format!("ws://{address}"))
}

/// The pairing link in the Connect dialog: the dialog wraps it over rows that start at the
/// same column as the command.
fn pairing_link(snapshot: &Snapshot) -> Option<String> {
    const COMMAND: &str = "borg pair '";
    let lines: Vec<Vec<char>> = snapshot.lines().iter().map(|line| line.chars().collect()).collect();
    let (row, column) = lines.iter().enumerate().find_map(|(row, line)| {
        let text: String = line.iter().collect();
        text.find(COMMAND).map(|byte| (row, text[..byte].chars().count()))
    })?;
    let mut command = String::new();
    for line in &lines[row..] {
        let piece: String = line.iter().skip(column).take_while(|&&c| c != '│').collect();
        command.push_str(piece.trim_end());
        if command.len() > COMMAND.len() && command.ends_with('\'') {
            return Some(command[COMMAND.len()..command.len() - 1].to_owned());
        }
    }
    None
}

/// Pairs a computer, whose borg keeps its key in a folder of the fixture, with the borg in
/// `app` through the Connect dialog's link, approving it there: the computer's folder and the
/// name it knows the server by.
fn pair_computer(app: &mut Session, fixture: &Fixture) -> Result<(std::path::PathBuf, String)> {
    // The server shows a link; the computer pairs with it, and the user approves it here.
    app.send(keys::f(1))?;
    app.wait_text("Remote access to this borg")?;
    app.send("c")?;
    let shown = app.wait_until("the pairing link", |s| pairing_link(s).is_some())?;
    let link = pairing_link(&shown).unwrap();
    ensure!(link.starts_with("borg+ws://127.0.0.1:"), "{link}");
    app.send(keys::esc())?;
    let target = borg_link::PairingLink::parse(&link).expect("the dialog shows a valid pairing link");
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let registration_deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        match runtime.block_on(borg_link::relay::dial(&target.relay, &target.server)) {
            Ok(socket) => {
                drop(socket);
                break;
            }
            Err(_) if std::time::Instant::now() < registration_deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(error) => {
                anyhow::bail!("the test server did not register at its relay: {error:#}\n{}", app.snapshot().text())
            }
        }
    }
    let computer = fixture.root().join("computer");
    let mut pairing = std::process::Command::new(BORG)
        .args(["pair", &link])
        .env("BORG_STATE_DIR", &computer)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if app.snapshot().contains("Pair this computer with borg?") {
            break;
        }
        if let Some(status) = pairing.try_wait()? {
            let output = pairing.wait_with_output()?;
            anyhow::bail!(
                "borg pair exited {status} before asking for approval: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "timed out waiting for pairing approval\n{}",
            app.snapshot().lines().join("\n")
        );
        app.pump(std::time::Duration::from_millis(100))?;
    }
    let dialog = app.wait_until("the pairing code", |s| s.contains("shows this code"))?.lines().join("\n");
    approve(app)?;
    let paired = pairing.wait_with_output()?;
    let said = format!("{}{}", String::from_utf8_lossy(&paired.stdout), String::from_utf8_lossy(&paired.stderr));
    ensure!(paired.status.success() && said.contains("Paired with"), "{said}");
    // Both ends showed one code: the names `borg pair` printed are in the dialog, in order.
    let names: Vec<&str> = said
        .lines()
        .skip_while(|line| !line.starts_with("The pairing code is"))
        .skip(2)
        .take(2)
        .flat_map(|row| row.split(" · ").filter_map(|symbol| symbol.split_whitespace().last()))
        .collect();
    ensure!(names.len() == 6, "borg pair prints six symbols: {said}");
    let (_, mut rest) = dialog.split_once("Computer:").context("the dialog names the computer")?;
    rest = rest.split_once('\n').map_or("", |(_, after)| after);
    for name in &names {
        let at = rest.find(name).with_context(|| format!("{name} of {names:?} is not in the dialog:\n{dialog}"))?;
        rest = &rest[at + name.len()..];
    }
    let name =
        said.lines().find_map(|line| line.strip_prefix("Paired with ")).unwrap().trim_end_matches('.').to_owned();
    ensure!(said.contains(&format!("mcp remote {name}")), "it shows the line for Claude Code: {said}");

    Ok((computer, name))
}

/// `borg mcp remote NAME` on a computer whose borg keeps its key in `state`.
fn remote_mcp(state: &Path, name: &str) -> Result<Mcp> {
    let mut child = std::process::Command::new(BORG)
        .args(["mcp", "remote", name])
        .env("BORG_STATE_DIR", state)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    let input = child.stdin.take().unwrap();
    let output = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut mcp = Mcp { child, input, output, next: 0 };
    mcp.request("initialize", serde_json::json!({ "protocolVersion": "2025-06-18" }))?;
    Ok(mcp)
}

#[test]
fn a_computer_pairs_through_borg_cloud_and_claude_works_there_end_to_end_encrypted() -> Result<()> {
    let relay = relay()?;
    let fixture = fixture()?;
    fixture.configure(&format!("return-to-panels = false\nrelay = \"{relay}\"\n"))?;
    let mut app = Session::builder(BORG, &fixture).size(130, 34).label("cloud").spawn()?;
    app.wait_text(READY)?;

    let (computer, name) = pair_computer(&mut app, &fixture)?;

    // Claude on the computer works in the borg on the server.
    let mut mcp = remote_mcp(&computer, &name)?;
    let tools = mcp.request("tools/list", serde_json::json!({}))?;
    let names: Vec<&str> = tools["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    ensure!(names.contains(&"download") && names.contains(&"upload") && names.contains(&"run"), "{names:?}");
    let (error, screen) = mcp.call("screen", serde_json::json!({}))?;
    ensure!(!error && screen.contains("The user is looking at the panels"), "{screen}");
    let (_, notes) = mcp.call("read", serde_json::json!({ "path": "notes.txt" }))?;
    ensure!(notes.ends_with("     1\thello\n"), "{notes}");

    let downloads = fixture.root().join("downloads");
    fs::create_dir_all(&downloads)?;
    let (error, text) =
        mcp.call("download", serde_json::json!({ "path": "notes.txt", "to": downloads.to_string_lossy() }))?;
    ensure!(!error && fs::read_to_string(downloads.join("notes.txt"))? == "hello\n", "{text}");

    let big: Vec<u8> = (0..3_000_000u32).map(|n| (n % 253) as u8).collect();
    let local = fixture.root().join("big.bin");
    fs::write(&local, &big)?;
    let asking = std::thread::spawn(move || {
        let uploaded = mcp.call("upload", serde_json::json!({ "from": local.to_string_lossy(), "to": "big.bin" }));
        (mcp, uploaded)
    });
    app.wait_text("Remote wants to upload a file")?;
    app.wait_text("asked from")?;
    approve(&mut app)?;
    let (mut mcp, uploaded) = asking.join().unwrap();
    let (error, text) = uploaded?;
    ensure!(!error && fs::read(fixture.work().join("big.bin"))? == big, "{text}");

    let asking = std::thread::spawn(move || mcp.call("run", serde_json::json!({ "command": "echo remote-$((6*7))" })));
    app.wait_text("echo remote-$((6*7))")?;
    approve(&mut app)?;
    let (error, text) = asking.join().unwrap()?;
    ensure!(!error && text == "exit 0\nremote-42\n", "{text:?}");

    // Another computer, which knows the server but holds no paired key, is refused.
    let stranger = fixture.root().join("stranger");
    fs::create_dir_all(stranger.join("link"))?;
    fs::copy(computer.join("link/servers.json"), stranger.join("link/servers.json"))?;
    let mut intruder = remote_mcp(&stranger, &name)?;
    let (error, text) = intruder.call("screen", serde_json::json!({}))?;
    ensure!(error && text.contains("not paired"), "{text}");
    Ok(())
}

#[test]
fn reads_from_another_computer_wait_for_approval_when_the_user_asks_for_that() -> Result<()> {
    let relay = relay()?;
    let fixture = fixture()?;
    fixture
        .configure(&format!("return-to-panels = false\nrelay = \"{relay}\"\n[plugins]\nagent-ask-reads = true\n"))?;
    let mut app = Session::builder(BORG, &fixture).size(130, 34).label("cloud-reads").spawn()?;
    app.wait_text(READY)?;
    let (computer, name) = pair_computer(&mut app, &fixture)?;
    let mut mcp = remote_mcp(&computer, &name)?;
    let asking = std::thread::spawn(move || {
        let read = mcp.call("read", serde_json::json!({ "path": "notes.txt" }));
        (mcp, read)
    });
    app.wait_text("Remote wants to read")?;
    app.send("n")?;
    let (mut mcp, read) = asking.join().unwrap();
    let (error, text) = read?;
    ensure!(error && text.contains("declined"), "{text}");
    let asking = std::thread::spawn(move || mcp.call("read", serde_json::json!({ "path": "notes.txt" })));
    app.wait_text("Remote wants to read")?;
    approve(&mut app)?;
    let (error, text) = asking.join().unwrap()?;
    ensure!(!error && text.ends_with("     1\thello\n"), "{text}");
    Ok(())
}

#[test]
fn remote_host_reports_its_version_and_streams_large_folders_without_duplicate_pages() -> Result<()> {
    let relay_url = relay()?;
    let fixture = fixture()?;
    fixture.configure(&format!("return-to-panels = false\nrelay = \"{relay_url}\"\n"))?;
    let mut app = Session::builder(BORG, &fixture).size(130, 34).label("remote-pages").spawn()?;
    app.wait_text(READY)?;
    let (computer, _) = pair_computer(&mut app, &fixture)?;
    let directory = fixture.work().join("large");
    fs::create_dir(&directory)?;
    for n in 0..2048 {
        fs::write(directory.join(format!("entry-{n}")), b"")?;
    }
    let servers: serde_json::Value = serde_json::from_slice(&fs::read(computer.join("link/servers.json"))?)?;
    let server = &servers[0];
    let relay = borg_link::Relay::parse(server["relay"].as_str().unwrap()).unwrap();
    let key = borg_link::unhex(server["key"].as_str().unwrap()).unwrap();
    let identity = borg_link::Identity::load_or_create(&computer.join("link/key"))?;
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    runtime.block_on(async {
        let socket = borg_link::relay::dial(&relay, &key).await?;
        let mut channel = borg_link::initiate(socket, &identity, &key, br#"{"kind":"session"}"#).await?;
        let answer: serde_json::Value = serde_json::from_slice(&channel.recv().await?.context("hello")?)?;
        ensure!(answer["ok"] == true);
        async fn call(
            channel: &mut borg_link::Channel,
            tool: &str,
            args: serde_json::Value,
        ) -> Result<serde_json::Value> {
            channel.send(&serde_json::to_vec(&serde_json::json!({"tool":tool,"args":args}))?).await?;
            receive(channel).await
        }
        async fn receive(channel: &mut borg_link::Channel) -> Result<serde_json::Value> {
            let bytes =
                tokio::time::timeout(std::time::Duration::from_secs(10), channel.recv()).await??.context("reply")?;
            let reply: serde_json::Value = serde_json::from_slice(&bytes)?;
            ensure!(reply["ok"] == true, "{reply}");
            Ok(serde_json::from_str(reply["text"].as_str().unwrap())?)
        }
        let host = call(&mut channel, "host_info", serde_json::json!({})).await?;
        ensure!(host["version"] == env!("CARGO_PKG_VERSION"));
        ensure!(host["os"] == std::env::consts::OS && host["service"] == false);
        ensure!(host["sessions"].as_array().is_some_and(|sessions| !sessions.is_empty()));
        let mut cursor = serde_json::Value::Null;
        let mut names = std::collections::HashSet::new();
        let mut chunks = 0;
        loop {
            let page = call(
                &mut channel,
                "fs_list",
                serde_json::json!({
                    "session":"default", "path":directory, "stream":true, "cursor":cursor, "limit":128
                }),
            )
            .await?;
            ensure!(page["streaming"] == true);
            let entries = page["entries"].as_array().context("entries")?;
            ensure!(entries.len() <= 128);
            for entry in entries {
                ensure!(names.insert(entry["name"].as_str().unwrap().to_owned()), "duplicate entry");
            }
            chunks += 1;
            cursor = page["cursor"].clone();
            if cursor.is_null() {
                break;
            }
        }
        ensure!(names.len() == 2048 && chunks >= 16, "{} entries, {chunks} chunks", names.len());
        names.clear();
        let start = std::time::Instant::now();
        let mut page = call(
            &mut channel,
            "fs_list",
            serde_json::json!({
                "session":"default", "path":directory, "stream":true, "push":true, "limit":128
            }),
        )
        .await?;
        let first = start.elapsed();
        let mut chunks = 0;
        loop {
            ensure!(page["pushed"] == true);
            let entries = page["entries"].as_array().context("pushed entries")?;
            ensure!(entries.len() <= if chunks == 0 { 128 } else { 512 });
            for entry in entries {
                ensure!(names.insert(entry["name"].as_str().unwrap().to_owned()), "duplicate pushed entry");
            }
            chunks += 1;
            if page["cursor"].is_null() {
                break;
            }
            // No new client request: the host sends the remaining chunks over this stream.
            page = receive(&mut channel).await?;
        }
        ensure!(names.len() == 2048 && chunks > 1);
        eprintln!(
            "encrypted localhost listing: {} entries / {chunks} pushed chunks; first {:?}, complete {:?}",
            names.len(),
            first,
            start.elapsed()
        );
        Ok::<_, anyhow::Error>(())
    })?;
    Ok(())
}

#[test]
fn remote_panel_console_preserves_its_shell_while_idle_and_parked() -> Result<()> {
    let relay = relay_with_idle_proxy(std::time::Duration::from_secs(25))?;
    let host = fixture()?;
    host.configure(&format!("return-to-panels = false\nrelay = \"{relay}\"\n"))?;
    let mut remote = Session::builder(BORG, &host).size(130, 34).label("idle-console-host").spawn()?;
    remote.wait_text(READY)?;
    let (computer, name) = pair_computer(&mut remote, &host)?;
    let client = fixture()?;
    client.configure(&format!("return-to-panels = false\nrelay = \"{relay}\"\n"))?;
    fs::create_dir_all(client.state().join("link"))?;
    for file in ["key", "servers.json"] {
        fs::copy(computer.join("link").join(file), client.state().join("link").join(file))?;
    }
    let mut local = Session::builder(BORG, &client).size(150, 34).label("idle-console-client").spawn()?;
    local.wait_text(READY)?;
    local.send(keys::alt(KeyCode::Char('r')))?;
    local.wait_text(&format!("Host {name} · Default"))?;
    local.send(keys::enter())?;
    local.wait_text(&format!("Host {name}:Default"))?;
    local.send(keys::ctrl('o'))?;
    local.wait_text(&format!("Host {name}:Default console"))?;
    local.send("keep_console=alive")?.send(keys::enter())?;
    local.wait_until("remote shell ready before waiting", |screen| {
        screen.cursor().is_some_and(|(_, row)| screen.lines()[usize::from(row)].trim() == "$")
    })?;
    local.send("printf '%s' \"$keep_console\"")?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(35);
    while std::time::Instant::now() < until {
        local.pump(std::time::Duration::from_millis(100))?;
        ensure!(local.snapshot().contains(&format!("Host {name}:Default console")), "the idle console stays open");
    }
    local.send(keys::enter())?;
    local.wait_text("alive")?;
    local.send(keys::ctrl('o'))?;
    local.wait_text("F1Help")?;
    local.send(keys::tab())?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(35);
    while std::time::Instant::now() < until {
        local.pump(std::time::Duration::from_millis(100))?;
        ensure!(!local.snapshot().contains("Remote console is closed"), "the parked console stays open");
    }
    local.send(keys::tab())?.send(keys::ctrl('o'))?;
    local.wait_text(&format!("Host {name}:Default console"))?;
    local.send("printf '%s' \"$keep_console\" > \"$HOME/idle-console-proof\"")?.send(keys::enter())?;
    local.wait_until("the same remote shell survived idle and parked time", |_| {
        fs::read(host.home().join("idle-console-proof")).is_ok_and(|data| data == b"alive")
    })?;
    ensure!(!client.home().join("idle-console-proof").exists());
    Ok(())
}

#[test]
fn remote_panel_console_follows_the_right_panel_and_ctrl_o_stays_remote() -> Result<()> {
    let relay = relay()?;
    let host = fixture()?;
    host.configure(&format!("return-to-panels = false\nrelay = \"{relay}\"\n"))?;
    let grabber = host.home().join("remote-grabber.sh");
    fs::write(
        &grabber,
        r#"mode="$1"
stty raw -echo -iexten min 0 time 1
if [ "$mode" = alternate ]; then printf '\033[?1049h'; fi
draw() { printf '\033[H\033[2Jremote-fullscreen-%s\033[2;1Hremote-size=%s' "$mode" "$(stty size)"; }
trap draw WINCH
draw
while :; do
    key=$(dd bs=1 count=1 2>/dev/null)
    [ -z "$key" ] && continue
    code=$(printf '%s' "$key" | od -An -tu1 | tr -d ' ')
    [ "$code" = 15 ] && break
    printf '\033[4;1Hremote-key=%s' "$code"
done
trap - WINCH
stty sane
if [ "$mode" = alternate ]; then printf '\033[?1049l'; fi
printf '\r\nremote-got=%s\r\n' "$code"
"#,
    )?;
    let mut remote = Session::builder(BORG, &host).size(130, 34).label("console-host").spawn()?;
    remote.wait_text(READY)?;
    let (computer, name) = pair_computer(&mut remote, &host)?;
    let client = fixture()?;
    client.configure(&format!("return-to-panels = false\nrelay = \"{relay}\"\n"))?;
    fs::create_dir_all(client.state().join("link"))?;
    for file in ["key", "servers.json"] {
        fs::copy(computer.join("link").join(file), client.state().join("link").join(file))?;
    }
    let mut local = Session::builder(BORG, &client).size(150, 34).label("console-client").spawn()?;
    local.wait_text(READY)?;
    local.send(keys::tab())?;
    local.send(keys::alt(KeyCode::Char('r')))?;
    local.wait_text(&format!("Host {name} · Default"))?;
    local.send(keys::enter())?;
    local.wait_text(&format!("Host {name}:Default"))?;
    local.send(keys::ctrl('o'))?;
    local.wait_text(&format!("Host {name}:Default console"))?;
    local.send("printf 'remote-proof' > \"$HOME/remote-console-proof\"")?;
    local.send(keys::enter())?;
    local.wait_until("command executed on the host", |_| host.home().join("remote-console-proof").exists())?;
    ensure!(fs::read_to_string(host.home().join("remote-console-proof"))? == "remote-proof");
    ensure!(!client.home().join("remote-console-proof").exists());
    local.send("sleep 30")?;
    local.send(keys::enter())?;
    local.wait_text("sleep 30")?;
    local.send(keys::ctrl('c'))?;
    local.send("printf 'interrupt-proof' > \"$HOME/interrupt-proof\"")?;
    local.send(keys::enter())?;
    local.wait_until("Ctrl-C interrupts the remote command", |_| host.home().join("interrupt-proof").exists())?;
    local.wait_text("printf 'interrupt-proof'")?;
    local.wait_until("remote prompt is ready to switch panels", |screen| {
        screen.cursor().is_some_and(|(_, row)| screen.lines()[usize::from(row)].trim() == "$")
    })?;

    local.send(keys::ctrl('o'))?;
    local.wait_text(&format!("Host {name}:Default"))?;
    local.send(keys::tab())?;
    local.send(keys::ctrl('o'))?;
    local.wait_text("Borg console")?;
    local.send("printf 'local-proof' > \"$HOME/local-console-proof\"")?;
    local.send(keys::enter())?;
    local.wait_until("local console restored", |_| client.home().join("local-console-proof").exists())?;
    ensure!(!host.home().join("local-console-proof").exists());
    local.wait_text("printf 'local-proof'")?;
    local.wait_until("local prompt is ready to switch panels", |screen| {
        screen.cursor().is_some_and(|(_, row)| screen.lines()[usize::from(row)].trim() == "$")
    })?;
    local.send(keys::ctrl('o'))?;
    local.send(keys::tab())?;
    local.send(keys::ctrl('o'))?;
    local.wait_text(&format!("Host {name}:Default console"))?;
    for (mode, cols, rows) in [("normal", 90, 27), ("alternate", 120, 32)] {
        local.send(keys::ctrl('o'))?;
        local.wait_text("F1He")?;
        local.send(format!("bash '{}' {mode}", grabber.display()))?;
        local.send(keys::enter())?;
        local.wait_text(&format!("remote-fullscreen-{mode}"))?;
        local.send(keys::f(1))?;
        local.wait_text("remote-key=80")?;
        ensure!(!local.snapshot().contains("Remote access to this borg"));
        local.resize(cols, rows)?;
        local.wait_text(&format!("remote-size={} {cols}", rows - 1))?;
        local.send(keys::ctrl('o'))?;
        local.wait_until("panels return after the remote app exits", |screen| {
            screen.contains("F1He") && !screen.contains("remote-fullscreen-")
        })?;
        local.send(keys::ctrl('o'))?;
        local.wait_text("remote-got=15")?;
    }
    local.send("cd ")?;
    local.send(format!("'{}'", host.work().join("alpha").display()))?;
    local.send(keys::enter())?;
    local.send(keys::ctrl('o'))?;
    local.wait_text("one.txt")?;
    ensure!(remote.snapshot().contains("4 items"), "the remote Borg's own panels stay in their original folder");
    Ok(())
}

#[test]
#[ignore = "requires a paired Linux host with procps top and BORG_TEST_REMOTE_* settings"]
fn paired_linux_top_captures_the_remote_console_resizes_and_restores_the_prompt() -> Result<()> {
    let state = std::path::PathBuf::from(std::env::var("BORG_TEST_REMOTE_STATE")?);
    let machine = std::env::var("BORG_TEST_REMOTE_MACHINE")?;
    let session = std::env::var("BORG_TEST_REMOTE_SESSION")?;
    let servers: serde_json::Value = serde_json::from_slice(&fs::read(state.join("link/servers.json"))?)?;
    let server = servers
        .as_array()
        .context("paired servers")?
        .iter()
        .find(|server| server["name"].as_str() == Some(machine.as_str()))
        .context("paired test machine")?;
    let client = fixture()?;
    client.configure(&format!(
        "return-to-panels = false\nrelay = {:?}\n[plugins]\nagent-cloud = false\n",
        server["relay"].as_str().context("relay")?
    ))?;
    fs::create_dir_all(client.state().join("link"))?;
    fs::copy(state.join("link/key"), client.state().join("link/key"))?;
    fs::write(client.state().join("link/servers.json"), serde_json::to_vec(&serde_json::json!([server]))?)?;
    let mut local = Session::builder(BORG, &client).size(140, 34).label("live-remote-top").spawn()?;
    local.wait_text(READY)?;
    local.send(keys::alt(KeyCode::Char('r')))?;
    let label = format!("Host {machine} · {session} ·");
    local.wait_text(&label)?;
    local.send(keys::key(KeyCode::Home))?;
    let mut selected = false;
    for _ in 0..64 {
        let screen = local.settle()?;
        let (x, y) = screen.find(&label).context("session in picker")?;
        let (base_x, base_y) = screen.find("This Borg:").context("picker body")?;
        if screen.style_at(x, y).bg != screen.style_at(base_x, base_y).bg {
            selected = true;
            break;
        }
        local.send(keys::down())?;
    }
    ensure!(selected, "test session could not be selected");
    local.send(keys::enter())?;
    local.wait_text(&format!("Host {machine}:{session}"))?;
    local.send("top -d 0.5")?.send(keys::enter())?;
    local.wait_text("load average")?;
    let screen = local.wait_text("program running")?;
    ensure!(!screen.contains("F1Help"));
    let images = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/screenshots/remote-top");
    fs::create_dir_all(&images)?;
    screen.save_png(&images.join("initial.png"))?;
    local.send("h")?;
    let help = local.wait_text("Help for Interactive Commands")?;
    help.save_png(&images.join("help.png"))?;
    local.send("q")?;
    local.wait_text("load average")?;
    local.resize(100, 26)?;
    let resized = local.wait_until("top resized with the remote console", |screen| {
        screen.cols() == 100
            && screen.rows() == 26
            && screen.contains("load average")
            && screen.lines()[25].contains("program running")
    })?;
    resized.save_png(&images.join("resized.png"))?;
    local.send("q")?;
    local.wait_text("F1Help")?;
    local.send(keys::ctrl('o'))?;
    local.wait_text(&format!("Host {machine}:{session} console"))?;
    local.send("printf '__TOP_RETURN__:%s:%s\\n' \"$(stty size)\" \"$(uname -n)\"")?.send(keys::enter())?;
    local.wait_text(&format!("__TOP_RETURN__:25 100:{machine}"))?;
    local.send(keys::ctrl('o'))?;
    let restored = local.wait_text("F1Help")?;
    restored.save_png(&images.join("restored.png"))?;
    Ok(())
}

#[test]
fn remote_host_and_session_labels_can_be_renamed_reset_and_reloaded() -> Result<()> {
    let relay = relay()?;
    let host = fixture()?;
    host.configure(&format!("relay = \"{relay}\"\n"))?;
    let mut remote = Session::builder(BORG, &host).size(130, 34).label("rename-host").spawn()?;
    remote.wait_text(READY)?;
    let (computer, machine) = pair_computer(&mut remote, &host)?;
    let client = fixture()?;
    client.configure(&format!("relay = \"{relay}\"\n[plugins]\nagent-cloud = false\n"))?;
    fs::create_dir_all(client.state().join("link"))?;
    for file in ["key", "servers.json"] {
        fs::copy(computer.join("link").join(file), client.state().join("link").join(file))?;
    }
    let mut local = Session::builder(BORG, &client).size(140, 34).label("rename-client").spawn()?;
    local.wait_text(READY)?;
    local.send(keys::alt(KeyCode::Char('r')))?;
    local.wait_text(&format!("Host {machine} · Default"))?;
    local.send(keys::f(2))?;
    local.wait_text("Rename host label")?;
    local.send(keys::ctrl('u'))?.send("Production")?.send(keys::enter())?;
    local.wait_text("Host Production · Default")?;
    local.send(keys::with(KeyCode::F(2), crossterm::event::KeyModifiers::SHIFT))?;
    local.wait_text("Rename session label")?;
    local.send(keys::ctrl('u'))?.send("Deploy")?.send(keys::enter())?;
    local.wait_text("Host Production · Deploy")?;
    local.send("r")?;
    local.wait_text("Host Production · Deploy")?;
    local.send(keys::enter())?;
    local.wait_text("Host Production:Deploy")?;
    local.send(keys::ctrl('o'))?;
    local.wait_text("Host Production:Deploy console")?;
    drop(local);
    let mut reopened = Session::builder(BORG, &client).size(140, 34).label("renamed-reopened").spawn()?;
    reopened.wait_text(READY)?;
    reopened.send(keys::alt(KeyCode::Char('r')))?;
    reopened.wait_text("Host Production · Deploy")?;
    reopened.wait_text(&format!("Original host: {machine} · Session: Default"))?;
    reopened.send(keys::f(2))?;
    reopened.wait_text("Rename host label")?;
    reopened.send(keys::ctrl('u'))?.send(keys::enter())?;
    reopened.wait_text(&format!("Host {machine} · Deploy"))?;
    reopened.send(keys::with(KeyCode::F(2), crossterm::event::KeyModifiers::SHIFT))?;
    reopened.wait_text("Rename session label")?;
    reopened.send(keys::ctrl('u'))?.send(keys::enter())?;
    reopened.wait_text(&format!("Host {machine} · Default"))?;
    let names: serde_json::Value = serde_json::from_slice(&fs::read(client.state().join("remote-names.json"))?)?;
    ensure!(names.as_object().is_some_and(|names| names.is_empty()));
    Ok(())
}

#[test]
fn f5_copies_folders_to_a_paired_host_and_files_back_with_keep_both() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let relay = relay()?;
    let host = fixture()?;
    host.configure(&format!("return-to-panels = false\nrelay = \"{relay}\"\n"))?;
    let mut remote = Session::builder(BORG, &host).size(130, 34).label("copy-host").spawn()?;
    remote.wait_text(READY)?;
    let (computer, name) = pair_computer(&mut remote, &host)?;
    let client = fixture()?;
    client.configure(&format!("return-to-panels = false\nrelay = \"{relay}\"\n[plugins]\nagent-cloud = false\n"))?;
    fs::create_dir_all(client.state().join("link"))?;
    for file in ["key", "servers.json"] {
        fs::copy(computer.join("link").join(file), client.state().join("link").join(file))?;
    }
    let contents: Vec<u8> = (0..2_100_007).map(|n| (n % 251) as u8).collect();
    fs::create_dir(client.work().join("bundle"))?;
    fs::write(client.work().join("bundle/large.dat"), &contents)?;
    fs::create_dir(client.work().join("bundle/empty"))?;
    fs::set_permissions(client.work().join("bundle/large.dat"), fs::Permissions::from_mode(0o751))?;
    std::os::unix::fs::symlink("large.dat", client.work().join("bundle/link"))?;
    let mut local = Session::builder(BORG, &client).size(150, 34).label("copy-client").spawn()?;
    local.wait_text("5 items")?;
    local.send(keys::tab())?.send(keys::alt(KeyCode::Char('r')))?;
    local.wait_text(&format!("Host {name} · Default"))?;
    local.send(keys::enter())?;
    local.wait_text(&format!("Host {name}:Default"))?;
    local.send(keys::tab())?;
    local.send(keys::ctrl('s'))?.send("bundle")?.send(keys::enter())?;
    local.send(keys::f(5))?;
    local.wait_text(&format!("Copy \"bundle\" to (host {}", name.chars().take(12).collect::<String>()))?;
    local.send(keys::ctrl('u'))?.send(host.work().to_string_lossy().as_bytes())?.send(keys::enter())?;
    local.wait_text("Copied 2 files")?;
    let uploaded = host.work().join("bundle");
    ensure!(fs::read(uploaded.join("large.dat"))? == contents);
    ensure!(uploaded.join("empty").is_dir());
    ensure!(fs::read_link(uploaded.join("link"))? == Path::new("large.dat"));
    ensure!(fs::metadata(uploaded.join("large.dat"))?.permissions().mode() & 0o777 == 0o751);

    local.send(keys::tab())?.send(keys::ctrl('o'))?;
    local.wait_text(&format!("Host {name}:Default console"))?;
    local.send(format!("cd '{}'; printf '__COPY_READY__\\n'", uploaded.display()))?.send(keys::enter())?;
    local.wait_text("__COPY_READY__")?;
    local.wait_until("remote shell prompt after cd", |screen| {
        screen.cursor().is_some_and(|(_, row)| screen.lines()[usize::from(row)].trim() == "$")
    })?;
    local.send(keys::ctrl('o'))?;
    local.wait_until("remote panel changed directory", |screen| {
        screen.contains("F1He")
            && screen
                .lines()
                .iter()
                .any(|line| line.chars().skip(usize::from(screen.cols() / 2)).collect::<String>().contains("large.dat"))
    })?;
    local.send(keys::ctrl('s'))?.send("large.dat")?.send(keys::enter())?;
    local.send(keys::f(5))?;
    local.wait_text("client filesystem")?;
    local.send(keys::enter())?;
    local.wait_text("Copied 1 file")?;
    ensure!(fs::read(client.work().join("large.dat"))? == contents);
    local.send(keys::f(5))?;
    local.wait_text("client filesystem")?;
    local.send(keys::enter())?;
    local.wait_text("Keep both")?;
    local.send("k")?;
    local.wait_until("download kept both files", |_| client.work().join("large (2).dat").exists())?;
    ensure!(fs::read(client.work().join("large (2).dat"))? == contents);
    use base64::Engine;
    use std::os::unix::ffi::OsStrExt;
    let interrupted = host.work().join("interrupted.bin");
    let encoded = base64::engine::general_purpose::STANDARD.encode(interrupted.as_os_str().as_bytes());
    let mut transfer = remote_mcp(&computer, &name)?;
    let (error, reply) = transfer.call(
        "file_transfer",
        serde_json::json!({"session":"default","op":"begin","path":encoded,"size":1000,"overwrite":false}),
    )?;
    ensure!(!error, "{reply}");
    let (error, _) =
        transfer.call("file_transfer", serde_json::json!({"session":"missing","op":"write","data":"eA=="}))?;
    ensure!(error, "invalid sessions cannot continue an upload");
    let (error, reply) =
        transfer.call("file_transfer", serde_json::json!({"session":"default","op":"write","data":"eA=="}))?;
    ensure!(!error, "{reply}");
    ensure!(
        fs::read_dir(host.work())?
            .any(|entry| entry.is_ok_and(|entry| entry.file_name().to_string_lossy().starts_with(".borg-copy-")))
    );
    drop(transfer);
    local.wait_until("disconnect removes the incomplete host file", |_| {
        fs::read_dir(host.work()).is_ok_and(|mut entries| {
            !entries
                .any(|entry| entry.is_ok_and(|entry| entry.file_name().to_string_lossy().starts_with(".borg-copy-")))
        })
    })?;
    ensure!(!interrupted.exists());
    ensure!(remote.snapshot().contains("4 items"), "host UI remains independent of local file copies");
    Ok(())
}

#[test]
fn a_regular_named_borg_can_be_mirrored_and_controlled_without_a_background_service() -> Result<()> {
    let relay = relay()?;
    let host = fixture()?;
    host.configure(&format!("return-to-panels = false\nrelay = \"{relay}\"\n"))?;
    let mut broker = Session::builder(BORG, &host).size(110, 30).label("native-mirror-broker").spawn()?;
    broker.wait_text(READY)?;
    let (computer, name) = pair_computer(&mut broker, &host)?;
    let made = std::process::Command::new(BORG)
        .args(["sessions", "create", "Second"])
        .env("BORG_STATE_DIR", host.state())
        .output()?;
    ensure!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
    let mut second = Session::builder(BORG, &host)
        .args(&["--session", "Second"])
        .size(100, 28)
        .label("native-mirror-second")
        .spawn()?;
    second.wait_text(READY)?;
    let client = fixture()?;
    fs::create_dir_all(client.state().join("link"))?;
    for file in ["key", "servers.json"] {
        fs::copy(computer.join("link").join(file), client.state().join("link").join(file))?;
    }
    let target = format!("{name}:Second");
    let mut mirror = Session::builder(BORG, &client)
        .args(&["mirror", &target])
        .size(140, 34)
        .label("native-mirror-client")
        .spawn()?;
    mirror.wait_text("REMOTE MIRROR - CONTROL")?;
    mirror.wait_text("hold Control and press G")?;
    mirror.wait_text("notes.txt")?;
    mirror.send(keys::f(1))?;
    mirror.wait_text("Remote access to this borg")?;
    second.wait_text("Remote access to this borg")?;
    ensure!(!broker.snapshot().contains("Remote access to this borg"), "the selected session receives the keys");
    mirror.send(keys::esc())?;
    mirror.wait_gone("Remote access to this borg")?;
    let shown = mirror.wait_text("notes.txt")?;
    let (x, y) = shown.find("notes.txt").context("notes on mirrored screen")?;
    mirror.send(mouse(0, x, y))?;
    second.wait_until("mouse selected the remote file", |screen| highlighted(screen, "notes.txt"))?;
    let command = "printf 'native-proof' > \"$HOME/native-mirror-proof\"";
    mirror.send(format!("\x1b[200~{command}\x1b[201~"))?.send(keys::enter())?;
    mirror.wait_until("pasted command ran in the selected remote Borg", |_| {
        host.home().join("native-mirror-proof").exists()
    })?;
    ensure!(fs::read_to_string(host.home().join("native-mirror-proof"))? == "native-proof");
    ensure!(!client.home().join("native-mirror-proof").exists());
    second.wait_until("remote shell returns to its prompt", |screen| {
        screen.cursor().is_some_and(|(_, row)| screen.lines()[usize::from(row)].trim() == "$")
    })?;
    mirror.wait_text("Borg console")?;
    mirror.send(keys::ctrl('o'))?;
    second.wait_until("remote panels before local takeover", panels_ready)?;
    mirror.wait_until("mirrored panels before local takeover", |screen| {
        screen.contains("notes.txt") && !screen.contains("Borg console")
    })?;
    let grabber = host.home().join("native-grabber.sh");
    fs::write(
        &grabber,
        r#"stty raw -echo -iexten
printf '\033[?1049h\033[2J\033[Hnative-fullscreen-ready'
key=$(dd bs=1 count=1 2>/dev/null)
code=$(printf '%s' "$key" | od -An -tu1 | tr -d ' ')
stty sane
printf '\033[?1049l\r\nnative-key=%s\r\n' "$code"
"#,
    )?;
    mirror.send(format!("bash '{}'", grabber.display()))?.send(keys::enter())?;
    mirror.wait_text("native-fullscreen-ready")?;
    mirror.send("q")?;
    second.wait_until("remote panels after app exits", panels_ready)?;
    mirror.wait_until("mirrored panels after app exits", |screen| {
        screen.contains("notes.txt") && !screen.contains("Borg console")
    })?;
    mirror.send(keys::ctrl('o'))?;
    mirror.wait_text("native-key=113")?;
    second.wait_until("remote prompt after full-screen app", |screen| {
        screen.cursor().is_some_and(|(_, row)| screen.lines()[usize::from(row)].trim() == "$")
    })?;
    mirror.send(keys::ctrl('o'))?;
    second.wait_until("remote panels after full-screen app", panels_ready)?;
    mirror.wait_until("mirrored panels after full-screen app", |screen| {
        screen.contains("notes.txt") && !screen.contains("Borg console")
    })?;
    second.send(keys::f(1))?;
    mirror.wait_text("VIEW ONLY")?;
    mirror.wait_text("Remote access to this borg")?;
    mirror.send(keys::esc())?;
    mirror.wait_until("view-only key did not close the host help", |screen| {
        screen.contains("Remote access to this borg") && screen.contains("VIEW ONLY")
    })?;
    mirror.send(keys::ctrl('t'))?;
    mirror.wait_text("REMOTE MIRROR - CONTROL")?;
    mirror.send(keys::esc())?;
    mirror.wait_gone("Remote access to this borg")?;
    mirror.send("exit")?.send(keys::enter())?;
    mirror.wait_text("No keeps the current shell running.")?;
    second.wait_text("No keeps the current shell running.")?;
    mirror.send(keys::enter())?;
    mirror.wait_gone("No keeps the current shell running.")?;
    second.wait_until("remote prompt after declining exit", |screen| {
        screen.cursor().is_some_and(|(_, row)| screen.lines()[usize::from(row)].trim() == "$")
    })?;
    mirror.send(keys::ctrl('o'))?;
    second.wait_until("remote panels after declining exit", panels_ready)?;
    second.resize(90, 26)?;
    second.wait_until("host resized", |screen| {
        screen.cols() == 90 && screen.rows() == 26 && screen.contains("notes.txt")
    })?;
    mirror.resize(70, 20)?;
    mirror.wait_text("EXIT MIRROR")?;
    mirror.resize(140, 34)?;
    mirror.wait_text("notes.txt")?;
    mirror.send(keys::ctrl('g'))?;
    ensure!(mirror.wait_exit()?.success());
    second.send(keys::f(1))?;
    second.wait_text("Remote access to this borg")?;
    Ok(())
}

#[test]
fn mirror_client_still_controls_background_host_sessions_using_pty_input() -> Result<()> {
    use base64::Engine;
    let relay = relay()?;
    let host = fixture()?;
    host.configure(&format!("return-to-panels = false\nrelay = \"{relay}\"\n"))?;
    let mut regular = Session::builder(BORG, &host).size(130, 34).label("service-mirror-pairing").spawn()?;
    regular.wait_text(READY)?;
    let (computer, name) = pair_computer(&mut regular, &host)?;
    regular.send(keys::f(10))?.send("y")?;
    regular.wait_exit()?;
    drop(regular);
    let mut service =
        Session::builder(BORG, &host).args(&["service", "run"]).size(130, 34).label("service-mirror-host").spawn()?;
    let mut probe = remote_mcp(&computer, &name)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let (error, text) = probe.call("mirror_frame", serde_json::json!({"session":"default","after":0}))?;
        let ready = (!error)
            .then(|| serde_json::from_str::<serde_json::Value>(&text).ok())
            .flatten()
            .and_then(|value| {
                value["data"].as_str().and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok())
            })
            .is_some_and(|data| String::from_utf8_lossy(&data).contains("notes.txt"));
        if ready {
            break;
        }
        ensure!(std::time::Instant::now() < deadline, "background session did not become ready: {text}");
        service.pump(std::time::Duration::from_millis(100))?;
    }
    let (error, text) = probe.call("host_info", serde_json::json!({}))?;
    ensure!(!error && serde_json::from_str::<serde_json::Value>(&text)?["service"] == true);
    drop(probe);
    let client = fixture()?;
    fs::create_dir_all(client.state().join("link"))?;
    for file in ["key", "servers.json"] {
        fs::copy(computer.join("link").join(file), client.state().join("link").join(file))?;
    }
    let target = format!("{name}:Default");
    let mut mirror = Session::builder(BORG, &client)
        .args(&["mirror", &target])
        .size(140, 36)
        .label("service-mirror-client")
        .spawn()?;
    mirror.wait_text("REMOTE MIRROR - CONTROL")?;
    mirror.wait_text("notes.txt")?;
    mirror.send(keys::alt(KeyCode::Char('r')))?;
    mirror.wait_text("This Borg is the host for the selected client.")?;
    mirror.wait_text("Connections: 3")?;
    mirror.send(keys::esc())?;
    mirror.wait_gone("Remote colony")?;
    mirror.send(keys::f(1))?;
    mirror.wait_text("Remote access to this borg")?;
    mirror.send(keys::esc())?;
    mirror.wait_gone("Remote access to this borg")?;
    mirror.send("exit")?.send(keys::enter())?;
    mirror.wait_text("No keeps the current shell running.")?;
    mirror.send(keys::esc())?;
    mirror.wait_gone("No keeps the current shell running.")?;
    mirror.send(keys::ctrl('g'))?;
    ensure!(mirror.wait_exit()?.success());
    Ok(())
}

#[test]
fn remote_sessions_can_be_removed_from_colony_without_deleting_host_files() -> Result<()> {
    for background in [false, true] {
        let relay = relay()?;
        let host = fixture()?;
        host.configure(&format!("return-to-panels = false\nrelay = \"{relay}\"\n"))?;
        let mut broker = Session::builder(BORG, &host).size(130, 34).label("remove-session-broker").spawn()?;
        broker.wait_text(READY)?;
        let (computer, machine) = pair_computer(&mut broker, &host)?;
        for name in ["Throwaway", "Keep"] {
            let output = std::process::Command::new(BORG)
                .args(["sessions", "create", name])
                .env("BORG_STATE_DIR", host.state())
                .output()?;
            ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        }
        let registry: serde_json::Value = serde_json::from_slice(&fs::read(host.state().join("sessions.json"))?)?;
        let entries = registry.as_array().context("session registry")?;
        let id = entries
            .iter()
            .find(|entry| entry["name"] == "Throwaway")
            .and_then(|entry| entry["id"].as_str())
            .context("Throwaway ID")?;
        let keep = entries
            .iter()
            .find(|entry| entry["name"] == "Keep")
            .and_then(|entry| entry["id"].as_str())
            .context("Keep ID")?;
        let workspace = host.state().join("sessions").join(id);
        let kept_workspace = host.state().join("sessions").join(keep);
        fs::create_dir_all(&workspace)?;
        fs::create_dir_all(&kept_workspace)?;
        fs::write(workspace.join("saved-proof"), b"saved workspace")?;
        fs::write(kept_workspace.join("saved-proof"), b"keep workspace")?;
        fs::write(host.home().join("keep-machine-file"), b"keep machine file")?;
        let mut target = if background {
            broker.send(keys::f(10))?.send("y")?;
            ensure!(broker.wait_exit()?.success());
            drop(broker);
            Session::builder(BORG, &host)
                .args(&["service", "run"])
                .size(130, 34)
                .label("remove-session-service")
                .spawn()?
        } else {
            Session::builder(BORG, &host)
                .args(&["--session", id])
                .size(130, 34)
                .label("remove-session-target")
                .spawn()?
        };
        if !background {
            target.wait_text(READY)?;
        }
        let client = fixture()?;
        client
            .configure(&format!("return-to-panels = false\nrelay = \"{relay}\"\n[plugins]\nagent-cloud = false\n"))?;
        fs::create_dir_all(client.state().join("link"))?;
        for file in ["key", "servers.json"] {
            fs::copy(computer.join("link").join(file), client.state().join("link").join(file))?;
        }
        let mut local = Session::builder(BORG, &client).size(150, 36).label("remove-session-client").spawn()?;
        local.wait_text(READY)?;
        local.wait_until("target session holds its running lock", |_| {
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(workspace.join(".lock"))
                .is_ok_and(|file| file.try_lock().is_err())
        })?;
        local.send(keys::alt(KeyCode::Char('r')))?;
        local.wait_text(&format!("Host {machine} · Throwaway"))?;
        local.send(keys::down())?.send(keys::enter())?;
        local.wait_text(&format!("Host {machine}:Throwaway"))?;
        local.send(keys::ctrl('o'))?;
        local.wait_text(&format!("Host {machine}:Throwaway console"))?;
        local.send("printf 'remote console' > \"$HOME/remove-console-proof\"")?.send(keys::enter())?;
        local
            .wait_until("target remote console is connected", |_| host.home().join("remove-console-proof").exists())?;
        local.wait_until("remote console prompt ready", |screen| {
            screen.cursor().is_some_and(|(_, row)| screen.lines()[usize::from(row)].trim() == "$")
        })?;
        local.send(keys::ctrl('o'))?.send(keys::alt(KeyCode::Char('r')))?;
        local.wait_text(&format!("Host {machine} · Throwaway"))?;
        local.send(keys::down())?.send(keys::f(8))?;
        local.wait_text("Remove remote session")?;
        local.wait_text("Files on the machine are kept.")?;
        local.send(keys::enter())?;
        local.wait_gone("Remove remote session")?;
        ensure!(workspace.exists(), "No keeps the session and its saved workspace");
        local.send(keys::f(8))?;
        local.wait_text("Remove remote session")?;
        local.send("y")?;
        local.wait_gone(&format!("Host {machine} · Throwaway"))?;
        local.wait_text(&format!("Host {machine} · Keep"))?;
        local.wait_until("saved workspace removed after the session exits", |_| !workspace.exists())?;
        ensure!(fs::read(kept_workspace.join("saved-proof"))? == b"keep workspace");
        ensure!(fs::read(host.home().join("keep-machine-file"))? == b"keep machine file");
        if !background {
            ensure!(target.wait_exit()?.success());
        }
        local.send(keys::key(KeyCode::Home))?.send(keys::f(8))?;
        local.wait_text("Default session is protected")?;
        local.send(keys::esc())?;
        local.wait_gone("Remote colony")?;
        local.send(keys::ctrl('o'))?;
        local.wait_text("Borg console")?;
        local.send("printf 'local console' > \"$HOME/remove-local-proof\"")?.send(keys::enter())?;
        local.wait_until("client console restored after removal", |_| {
            client.home().join("remove-local-proof").exists()
        })?;
        ensure!(!host.home().join("remove-local-proof").exists());
        if background {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while std::time::Instant::now() < until {
                local.pump(std::time::Duration::from_millis(100))?;
                ensure!(!workspace.exists(), "the service must not recreate a removed session");
            }
        }
    }
    Ok(())
}

#[test]
fn host_colony_shows_paired_clients_and_updates_connection_counts() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let relay = relay()?;
    let host = fixture()?;
    host.configure(&format!("relay = \"{relay}\"\n"))?;
    let mut remote = Session::builder(BORG, &host).size(140, 36).label("colony-clients-host").spawn()?;
    remote.wait_text(READY)?;
    let (computer, name) = pair_computer(&mut remote, &host)?;
    let paired: serde_json::Value = serde_json::from_slice(&fs::read(host.state().join("link/computers.json"))?)?;
    let client_name = paired[0]["name"].as_str().context("paired client name")?;
    let target = format!("{name}:Default");
    let client = fixture()?;
    fs::create_dir_all(client.state().join("link"))?;
    for file in ["key", "servers.json"] {
        fs::copy(computer.join("link").join(file), client.state().join("link").join(file))?;
    }
    let mut mirror = Session::builder(BORG, &client)
        .args(&["mirror", &target])
        .size(140, 36)
        .label("colony-clients-viewer")
        .spawn()?;
    mirror.wait_text("REMOTE MIRROR - CONTROL")?;
    mirror.wait_text("notes.txt")?;
    remote.send(keys::alt(KeyCode::Char('r')))?;
    remote.wait_text(&format!("Client {client_name} · Connected"))?;
    let screen = remote.wait_text("Connections: 3")?;
    ensure!(screen.lines().iter().filter(|line| line.contains(&format!("Client {client_name}"))).count() == 1);
    ensure!(!screen.contains("This Borg: client"));
    ensure!(fs::metadata(host.state().join("link/clients.sock"))?.permissions().mode() & 0o777 == 0o600);
    remote.send(keys::tab())?;
    remote.wait_text("No paired hosts")?;
    remote.send(keys::tab())?;
    remote.wait_text(&format!("Client {client_name} · Connected"))?;
    mirror.send(keys::ctrl('g'))?;
    ensure!(mirror.wait_exit()?.success());
    remote.wait_text(&format!("Client {client_name} · Not connected"))?;
    remote.wait_text("Connections: 0")?;
    remote.send(keys::esc())?;
    remote.wait_gone("Remote colony")?;
    Ok(())
}

#[test]
fn remote_mirror_connection_can_be_cancelled_times_out_and_retries() -> Result<()> {
    struct Paused(u32);
    impl Drop for Paused {
        fn drop(&mut self) {
            let _ = std::process::Command::new("kill").args(["-CONT", &self.0.to_string()]).status();
        }
    }
    let relay = relay()?;
    let host = fixture()?;
    host.configure(&format!("relay = \"{relay}\"\n"))?;
    let mut remote = Session::builder(BORG, &host).size(130, 34).label("stalled-mirror-host").spawn()?;
    remote.wait_text(READY)?;
    let (computer, name) = pair_computer(&mut remote, &host)?;
    let client = fixture()?;
    client.configure(&format!("relay = \"{relay}\"\n[plugins]\nagent-cloud = false\n"))?;
    fs::create_dir_all(client.state().join("link"))?;
    for file in ["key", "servers.json"] {
        fs::copy(computer.join("link").join(file), client.state().join("link").join(file))?;
    }
    let mut local = Session::builder(BORG, &client).size(140, 34).label("stalled-mirror-client").spawn()?;
    local.wait_text(READY)?;
    local.send(keys::alt(KeyCode::Char('r')))?;
    local.wait_text(&format!("Host {name} · Default"))?;
    let pid = remote.process_id().context("test host PID")?;
    ensure!(std::process::Command::new("kill").args(["-STOP", &pid.to_string()]).status()?.success());
    let paused = Paused(pid);
    local.send("m")?;
    let first = local.wait_text(&format!("Connecting mirror to {name}:Default"))?;
    local.wait_text("Esc: cancel connection")?;
    local.wait_until("the connection spinner keeps animating", |screen| {
        screen.text() != first.text() && screen.contains("Connecting mirror to")
    })?;
    let cancelled = std::time::Instant::now();
    local.send(keys::esc())?;
    local.wait_gone("Connecting mirror to")?;
    ensure!(cancelled.elapsed() < std::time::Duration::from_secs(2), "Esc must cancel promptly");
    local.wait_text(&format!("Host {name} · Default"))?;
    local.send("m")?;
    local.wait_text("Connecting mirror to")?;
    local.wait_text("Connection timed out after 10 seconds")?;
    local.wait_text("M: retry mirror")?;
    local.wait_gone("Esc: cancel connection")?;
    drop(paused);
    remote.send(keys::f(1))?;
    remote.wait_text("Remote access to this borg")?;
    remote.send(keys::esc())?;
    remote.wait_gone("Remote access to this borg")?;
    local.send("m")?;
    local.wait_text("REMOTE MIRROR - CONTROL")?;
    local.wait_text("notes.txt")?;
    local.send(keys::ctrl('g'))?;
    local.wait_text("F1Help")?;
    local.send(keys::f(1))?;
    local.wait_text("Remote access to this borg")?;
    Ok(())
}

#[test]
fn mirror_keeps_connections_alive_while_quiet_and_while_typing() -> Result<()> {
    let relay = relay_with_idle_proxy(std::time::Duration::from_secs(25))?;
    let host = fixture()?;
    host.configure(&format!("relay = \"{relay}\"\n"))?;
    let mut remote = Session::builder(BORG, &host).size(130, 34).label("idle-mirror-host").spawn()?;
    remote.wait_text(READY)?;
    let (computer, name) = pair_computer(&mut remote, &host)?;
    let client = fixture()?;
    fs::create_dir_all(client.state().join("link"))?;
    for file in ["key", "servers.json"] {
        fs::copy(computer.join("link").join(file), client.state().join("link").join(file))?;
    }
    let target = format!("{name}:Default");
    let mut mirror =
        Session::builder(BORG, &client).args(&["mirror", &target]).size(140, 36).label("idle-mirror-client").spawn()?;
    mirror.wait_text("REMOTE MIRROR - CONTROL")?;
    mirror.wait_text("notes.txt")?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(50);
    while std::time::Instant::now() < until {
        mirror.pump(std::time::Duration::from_millis(100))?;
        ensure!(mirror.snapshot().contains("REMOTE MIRROR - CONTROL"), "the quiet mirror stays connected");
    }
    ensure!(!remote.snapshot().contains("Remote access to this borg"), "keepalives do not send host keys");
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < until {
        mirror.send(keys::f(1))?;
        mirror.wait_text("Remote access to this borg")?;
        mirror.send(keys::esc())?;
        mirror.wait_gone("Remote access to this borg")?;
        mirror.pump(std::time::Duration::from_millis(500))?;
        ensure!(mirror.snapshot().contains("REMOTE MIRROR - CONTROL"), "the active mirror stays connected");
    }
    mirror.send(keys::ctrl('t'))?;
    mirror.send(keys::f(1))?;
    mirror.wait_text("Remote access to this borg")?;
    remote.wait_text("Remote access to this borg")?;
    mirror.send(keys::esc())?;
    mirror.wait_gone("Remote access to this borg")?;
    mirror.send(keys::ctrl('g'))?;
    ensure!(mirror.wait_exit()?.success());
    Ok(())
}

#[test]
fn shell_exit_confirmation_preserves_the_current_shell_when_declined() -> Result<()> {
    for (shell, _) in Shell::available() {
        let fixture = fixture()?;
        fixture.configure("return-to-panels = false\n")?;
        let mut app = start_with(&fixture, &format!("exit-confirm-{}", shell.program()), shell)?;
        let command =
            if shell == Shell::Fish { "set --global keep_exit alive; cd alpha" } else { "keep_exit=alive; cd alpha" };
        app.send(command)?.send(keys::enter())?;
        app.wait_until("shell ready before exit", |screen| {
            screen.cursor().is_some_and(|(_, row)| screen.lines()[usize::from(row)].trim() == "$")
        })?;
        app.send("exit")?.send(keys::enter())?;
        app.wait_text("No keeps the current shell running.")?;
        app.send(keys::enter())?;
        app.wait_gone("No keeps the current shell running.")?;
        app.wait_until("same shell resumes after No", |screen| {
            screen.cursor().is_some_and(|(_, row)| screen.lines()[usize::from(row)].trim() == "$")
        })?;
        let nested = if shell == Shell::Fish { "fish -c 'exit 0'" } else { "(exit 0)" };
        app.send(format!(
            "{nested}; bash -c 'exit 0'; printf '%s' \"$keep_exit\" > \"$HOME/kept-exit\"; pwd > \"$HOME/kept-cwd\""
        ))?
        .send(keys::enter())?;
        app.wait_until("variables and cwd survive; nested exits do not ask", |_| {
            fs::read(fixture.home().join("kept-exit")).is_ok_and(|data| data == b"alive")
                && fixture.home().join("kept-cwd").exists()
        })?;
        let cwd = fs::read_to_string(fixture.home().join("kept-cwd"))?;
        ensure!(Path::new(cwd.trim()).canonicalize()? == fixture.work().join("alpha").canonicalize()?);
        app.wait_until("shell ready before accepting exit", |screen| {
            screen.cursor().is_some_and(|(_, row)| screen.lines()[usize::from(row)].trim() == "$")
        })?;
        app.send("exit")?.send(keys::enter())?;
        app.wait_text("No keeps the current shell running.")?;
        app.send("y")?;
        ensure!(app.wait_exit()?.success());
    }
    Ok(())
}

#[test]
fn mirror_handoffs_and_disconnects_work_without_terminal_cursor_replies() -> Result<()> {
    let relay = relay()?;
    let host = fixture()?;
    host.configure(&format!("relay = \"{relay}\"\n"))?;
    let mut remote = Session::builder(BORG, &host).size(130, 34).label("mirror-handoff-host").spawn()?;
    remote.wait_text(READY)?;
    let (computer, name) = pair_computer(&mut remote, &host)?;
    let client = fixture()?;
    client.configure(&format!("relay = \"{relay}\"\n[plugins]\nagent-cloud = false\n"))?;
    fs::create_dir_all(client.state().join("link"))?;
    for file in ["key", "servers.json"] {
        fs::copy(computer.join("link").join(file), client.state().join("link").join(file))?;
    }
    let mut local =
        Session::builder(BORG, &client).size(150, 36).terminal_replies(false).label("mirror-handoff-client").spawn()?;
    local.wait_text(READY)?;
    for _ in 0..3 {
        local.send(keys::alt(KeyCode::Char('r')))?;
        local.wait_text(&format!("Host {name} · Default"))?;
        local.send("m")?;
        local.wait_text("REMOTE MIRROR - CONTROL")?;
        local.send(keys::f(1))?;
        local.wait_text("Remote access to this borg")?;
        local.send(keys::esc())?;
        local.wait_gone("Remote access to this borg")?;
        local.send(keys::ctrl('g'))?;
        local.wait_until("local panels after mirror without cursor replies", |screen| {
            panels_ready(screen) && !screen.contains("REMOTE MIRROR")
        })?;
        local.send(keys::f(1))?;
        local.wait_text("Remote access to this borg")?;
        local.send(keys::esc())?;
        local.wait_gone("Remote access to this borg")?;
    }
    local.send(keys::alt(KeyCode::Char('r')))?;
    local.wait_text(&format!("Host {name} · Default"))?;
    local.send("m")?;
    local.wait_text("REMOTE MIRROR - CONTROL")?;
    remote.send(keys::f(10))?.send("y")?;
    ensure!(remote.wait_exit()?.success());
    local.wait_text("Remote mirror:")?;
    local.wait_until("local panels after remote disconnect", |screen| {
        panels_ready(screen) && !screen.contains("REMOTE MIRROR")
    })?;
    let shown = local.wait_text("notes.txt")?;
    let (x, y) = shown.find("notes.txt").context("local file on screen")?;
    local.send(mouse(0, x, y))?;
    local.wait_until("mouse still selects local files after mirror", |screen| highlighted(screen, "notes.txt"))?;
    local.send("echo handoff-ready")?;
    local
        .wait_until("local command line works after mirror", |screen| prompt(screen).contains("echo handoff-ready"))?;
    local.send(keys::ctrl('u'))?;
    local.send(keys::f(10))?.send("y")?;
    ensure!(local.wait_exit()?.success());
    Ok(())
}
