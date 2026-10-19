use std::{path::Path, time::Instant};

use crate::{
    Action, AfterCommand, AppState, Confirm, ConfirmPurpose, Effect, LineInput, ListingMode, MenuCursor, Operation,
    Overlay, Panel, PathOf, Picker, Prompt, PromptPurpose, Row, Side, Sort, View,
    command::CommandLine,
    layout::EVEN_SPLIT,
    menu::{FILE_MENU, menus},
    ops::{self, ArchiveFormat, DEFAULT_ARCHIVE_SUFFIX},
    state::REVEAL_DELAY,
    update::{prepare_shell, shell_ready},
};

const LINE_UP: isize = -1;
const LINE_DOWN: isize = 1;
const PAGE_UP: isize = -1;
const PAGE_DOWN: isize = 1;
const SELECT_ALL: &str = "*";
const SHELL_NOT_READY: &str = "Shell is busy · finish or clear the command line first";
const NOTHING_SELECTED: &str = "Nothing selected";
const QUIT_TITLE: &str = "Quit";

fn plural(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun} is") } else { format!("{count} {noun}s are") }
}

pub(crate) fn perform(state: &mut AppState, action: Action, now: Instant) -> Vec<Effect> {
    let side = state.active;
    match action {
        Action::Quit if !state.jobs.is_empty() => {
            state.overlay = Some(Overlay::Confirm(Confirm {
                title: QUIT_TITLE,
                message: format!("{} still running. Quit and cancel?", plural(state.jobs.len(), "job")),
                purpose: ConfirmPurpose::Quit,
                accept: false,
            }));
            Vec::new()
        }
        Action::Quit => vec![Effect::Quit],
        Action::Jobs => crate::jobs::open_list(state),
        Action::ToggleShell => toggle_shell(state),
        Action::SwitchPanel if state.aside.is_some() => {
            state.preview_focused = !state.preview_focused;
            Vec::new()
        }
        Action::SwitchPanel => {
            state.active = side.other();
            Vec::new()
        }
        Action::Compose => vec![Effect::ReadCommandLine],
        Action::QuickView => crate::aside::toggle(state, crate::Aside::QuickView),
        Action::Info => crate::aside::toggle(state, crate::Aside::Info),
        Action::Up => moved(state, |panel| panel.move_by(LINE_UP)),
        Action::Down => moved(state, |panel| panel.move_by(LINE_DOWN)),
        Action::PageUp => moved(state, |panel| panel.page(PAGE_UP)),
        Action::PageDown => moved(state, |panel| panel.page(PAGE_DOWN)),
        Action::Top => moved(state, Panel::to_start),
        Action::Bottom => moved(state, Panel::to_end),
        Action::Open => open(state, side),
        Action::Descend if state.panel(side).mode() == ListingMode::Brief => {
            moved(state, |panel| panel.move_column(LINE_DOWN))
        }
        Action::Leave if state.panel(side).mode() == ListingMode::Brief => {
            moved(state, |panel| panel.move_column(LINE_UP))
        }
        Action::Descend => descend(state, side).into_iter().collect(),
        Action::Leave => leave(state, side).into_iter().collect(),
        Action::InsertName => insert_names(state, side),
        Action::InsertPath(which) => {
            let side = match which {
                PathOf::Active => side,
                PathOf::Other => side.other(),
            };
            let dir = state.panel(side).dir().to_path_buf();
            type_words(state, CommandLine::default().path(&dir))
        }
        Action::ToggleHidden => {
            let prefs = &mut state.settings.preferences;
            prefs.show_hidden = !prefs.show_hidden;
            let mut effects = state.reload_all();
            effects.push(Effect::SaveConfig);
            effects
        }
        Action::OpenMenuBar => open_menu_bar(state, side),
        Action::OpenFileMenu => open_menu(state, FILE_MENU),
        Action::About => {
            state.overlay = Some(Overlay::About(crate::About::default()));
            Vec::new()
        }
        Action::Settings => {
            state.overlay = Some(Overlay::Settings { selected: 0 });
            Vec::new()
        }
        Action::PickTheme => {
            let current = state.appearance.current();
            state.overlay = Some(Overlay::Themes(Picker { selected: current, original: current }));
            Vec::new()
        }
        Action::ToggleMark => mark_and_move(state, LINE_DOWN),
        Action::MarkUp => mark_and_move(state, LINE_UP),
        Action::MarkDown => mark_and_move(state, LINE_DOWN),
        Action::SelectGroup => prompt(state, "Select group", "Files matching", SELECT_ALL, PromptPurpose::Select),
        Action::UnselectGroup => prompt(state, "Unselect group", "Files matching", SELECT_ALL, PromptPurpose::Unselect),
        Action::InvertSelection => moved(state, Panel::invert_marks),
        Action::View => hovered_file(state).map(|path| vec![Effect::View(path)]).unwrap_or_default(),
        Action::Edit => edit(state),
        Action::Copy => transfer(state, "Copy", |sources| PromptPurpose::Copy { sources }),
        Action::Move => transfer(state, "Move", |sources| PromptPurpose::Move { sources }),
        Action::Mkdir => prompt(state, "Make directory", "Name", "", PromptPurpose::Mkdir),
        Action::Delete => confirm_delete(state),
        Action::Compress => compress(state),
        Action::ExtractHere => extract(state, state.active_panel().dir().to_path_buf().as_path(), now),
        Action::ExtractToOther => extract(state, state.panel(side.other()).dir().to_path_buf().as_path(), now),
        Action::SwapPanels => {
            state.panels.swap(0, 1);
            Vec::new()
        }
        Action::EqualizePanels => {
            state.set_split(EVEN_SPLIT, now);
            Vec::new()
        }
        Action::NudgeSplit(columns) => {
            let step = f32::from(columns) / f32::from(state.cols.max(1));
            state.set_split(state.split_target() + step, now);
            Vec::new()
        }
        Action::ToggleStatusLine => preference(state, |prefs| prefs.status_line = !prefs.status_line),
        Action::ToggleMenuBar => preference(state, |prefs| prefs.menu_bar = !prefs.menu_bar),
        Action::ToggleRestoreTerminal => preference(state, |prefs| prefs.restore_terminal = !prefs.restore_terminal),
        Action::ToggleScreensaver => {
            state.idle_since = now;
            preference(state, |prefs| prefs.screensaver = !prefs.screensaver)
        }
        Action::Find => crate::find::open(state),
        Action::ToggleReturnToPanels => preference(state, |prefs| {
            prefs.after_command = match prefs.after_command {
                AfterCommand::ShowShell => AfterCommand::ReturnToPanels,
                AfterCommand::ReturnToPanels => AfterCommand::ShowShell,
            }
        }),
        Action::SetListing(target, mode) => {
            state.panel_mut(target).set_mode(mode);
            Vec::new()
        }
        Action::SetSort(target, key) => {
            let reverse = state.panel(target).sort().reverse;
            state.panel_mut(target).set_sort(Sort { key, reverse });
            Vec::new()
        }
        Action::ToggleReverse(target) => {
            let sort = state.panel(target).sort();
            state.panel_mut(target).set_sort(Sort { reverse: !sort.reverse, ..sort });
            Vec::new()
        }
        Action::Rescan(target) => vec![state.reload(target)],
    }
}

fn moved(state: &mut AppState, motion: impl FnOnce(&mut Panel)) -> Vec<Effect> {
    motion(state.panel_mut(state.active));
    Vec::new()
}

fn mark_and_move(state: &mut AppState, step: isize) -> Vec<Effect> {
    moved(state, |panel| {
        panel.toggle_mark();
        panel.move_by(step);
    })
}

fn preference(state: &mut AppState, change: impl FnOnce(&mut crate::Preferences)) -> Vec<Effect> {
    change(&mut state.settings.preferences);
    state.refit();
    vec![Effect::SaveConfig]
}

fn open_menu(state: &mut AppState, menu: usize) -> Vec<Effect> {
    let item = menus(state).get(menu).and_then(crate::Menu::first_enabled);
    state.overlay = Some(Overlay::Menu(MenuCursor { menu, item }));
    Vec::new()
}

fn open_menu_bar(state: &mut AppState, side: Side) -> Vec<Effect> {
    let all = menus(state);
    let resumed = state.last_menu.and_then(|last| {
        let menu = all.get(last.menu)?;
        let usable = |index: &usize| menu.item(*index).is_some_and(|item| item.enabled);
        Some(MenuCursor { item: last.item.filter(usable).or_else(|| menu.first_enabled()), ..last })
    });
    match resumed {
        Some(cursor) => {
            state.overlay = Some(Overlay::Menu(cursor));
            Vec::new()
        }
        None => open_menu(state, if side == Side::Left { 0 } else { all.len() - 1 }),
    }
}

fn prompt(state: &mut AppState, title: &'static str, label: &str, value: &str, purpose: PromptPurpose) -> Vec<Effect> {
    state.overlay =
        Some(Overlay::Prompt(Prompt { title, label: label.to_owned(), input: LineInput::new(value), purpose }));
    Vec::new()
}

fn toggle_shell(state: &mut AppState) -> Vec<Effect> {
    match state.view {
        View::Panels => {
            state.view = View::Shell;
            prepare_shell(state)
        }
        View::Shell => {
            state.view = View::Panels;
            vec![Effect::ScrollConsole(borg_term::Scroll::Bottom)]
        }
    }
}

fn open(state: &mut AppState, side: Side) -> Vec<Effect> {
    let panel = state.panel(side);
    match panel.hovered() {
        Some(Row::Parent) => leave(state, side).into_iter().collect(),
        Some(Row::Entry(entry)) if entry.is_dir() => descend(state, side).into_iter().collect(),
        Some(Row::Entry(entry)) if entry.executable => {
            let command = ops::run(&entry.name);
            interactive(state, &command)
        }
        Some(Row::Entry(entry)) => vec![Effect::View(panel.dir().join(&entry.name))],
        None => Vec::new(),
    }
}

fn descend(state: &mut AppState, side: Side) -> Option<Effect> {
    let panel = state.panel(side);
    let entry = panel.hovered_entry().filter(|entry| entry.is_dir())?;
    let dir = panel.dir().join(&entry.name);
    Some(state.navigate(side, dir, None))
}

fn leave(state: &mut AppState, side: Side) -> Option<Effect> {
    let current = state.panel(side).dir();
    let parent = current.parent()?.to_path_buf();
    let child = current.file_name().map(|name| name.to_string_lossy().into_owned());
    Some(state.navigate(side, parent, child))
}

fn insert_names(state: &mut AppState, side: Side) -> Vec<Effect> {
    let names = selected_names(state.panel(side));
    if names.is_empty() {
        return Vec::new();
    }
    let words = names.iter().fold(CommandLine::default(), |line, name| line.arg(name.as_bytes()));
    type_words(state, words)
}

fn type_words(state: &mut AppState, words: CommandLine) -> Vec<Effect> {
    let mut effects = prepare_shell(state);
    effects.push(Effect::Write([&words.line()[..], b" "].concat()));
    state.shell.mark_typed();
    effects
}

fn selected_names(panel: &Panel) -> Vec<String> {
    panel.selection().iter().map(|entry| entry.name.clone()).collect()
}

fn hovered_file(state: &AppState) -> Option<std::path::PathBuf> {
    let panel = state.active_panel();
    panel.hovered_entry().filter(|entry| !entry.is_dir()).map(|entry| panel.dir().join(&entry.name))
}

fn edit(state: &mut AppState) -> Vec<Effect> {
    let Some(entry) = state.active_panel().hovered_entry().filter(|entry| !entry.is_dir()) else { return Vec::new() };
    let command = ops::edit(&entry.name);
    interactive(state, &command)
}

fn interactive(state: &mut AppState, command: &CommandLine) -> Vec<Effect> {
    if !shell_ready(state) {
        state.notice = Some(SHELL_NOT_READY.to_owned());
        return Vec::new();
    }
    let mut effects = prepare_shell(state);
    effects.push(Effect::Write(command.submit()));
    state.shell.submit();
    state.view = View::Shell;
    effects
}

fn transfer(
    state: &mut AppState,
    title: &'static str,
    purpose: impl FnOnce(Vec<String>) -> PromptPurpose,
) -> Vec<Effect> {
    let sources = selected_names(state.active_panel());
    if sources.is_empty() {
        state.notice = Some(NOTHING_SELECTED.to_owned());
        return Vec::new();
    }
    let label = format!("{title} {} to", describe(&sources));
    let dest = state.panel(state.active.other()).dir().display().to_string();
    prompt(state, title, &label, &dest, purpose(sources))
}

fn confirm_delete(state: &mut AppState) -> Vec<Effect> {
    let sources = selected_names(state.active_panel());
    if sources.is_empty() {
        state.notice = Some(NOTHING_SELECTED.to_owned());
        return Vec::new();
    }
    let message = format!("Delete {} permanently?", describe(&sources));
    state.overlay = Some(Overlay::Confirm(Confirm {
        title: "Delete",
        message,
        purpose: ConfirmPurpose::Delete { sources },
        accept: true,
    }));
    Vec::new()
}

fn compress(state: &mut AppState) -> Vec<Effect> {
    let sources = selected_names(state.active_panel());
    let stem = match sources.as_slice() {
        [] => {
            state.notice = Some(NOTHING_SELECTED.to_owned());
            return Vec::new();
        }
        [single] => single.clone(),
        _ => state.active_panel().dir().file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
    };
    let default = format!("{stem}{DEFAULT_ARCHIVE_SUFFIX}");
    prompt(
        state,
        "Compress",
        &format!("Archive {} as", describe(&sources)),
        &default,
        PromptPurpose::Compress { sources },
    )
}

fn extract(state: &mut AppState, dest: &Path, now: Instant) -> Vec<Effect> {
    let Some(entry) = state.active_panel().hovered_entry() else { return Vec::new() };
    let Some(format) = ArchiveFormat::detect(&entry.name) else { return Vec::new() };
    let command = ops::extract(&entry.name, format, dest);
    run_operation(state, "extract", &command, None, now)
}

pub(crate) fn run_operation(
    state: &mut AppState,
    label: &'static str,
    command: &CommandLine,
    focus: Option<String>,
    now: Instant,
) -> Vec<Effect> {
    if !shell_ready(state) {
        state.notice = Some(SHELL_NOT_READY.to_owned());
        return Vec::new();
    }
    let mut effects = prepare_shell(state);
    effects.push(Effect::Write(command.submit_hidden()));

    if state.shell.integrated {
        state.shell.running = true;
        state.operation =
            Some(Operation { label, side: state.active, reveal_at: now + REVEAL_DELAY, revealed: false, focus });
    } else {
        state.view = View::Shell;
    }
    effects
}

pub(crate) fn describe(names: &[String]) -> String {
    match names {
        [single] => format!("\"{single}\""),
        many => format!("{} items", many.len()),
    }
}
