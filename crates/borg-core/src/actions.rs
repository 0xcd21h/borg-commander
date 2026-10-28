use borg_i18n::tr;
use std::{path::Path, time::Instant};

use crate::{
    Action, AfterCommand, AppState, Confirm, ConfirmPurpose, Effect, LineInput, ListingMode, MenuCursor, Operation,
    Overlay, Panel, PathOf, Picker, Prompt, PromptPurpose, Row, Side, Sort, View,
    command::CommandLine,
    layout::EVEN_SPLIT,
    menu::{FILE_MENU, RIGHT_MENU, menus},
    ops::{self, ArchiveFormat, DEFAULT_ARCHIVE_SUFFIX},
    state::REVEAL_DELAY,
    update::{prepare_shell, shell_ready},
};

const LINE_UP: isize = -1;
const LINE_DOWN: isize = 1;
const PAGE_UP: isize = -1;
const PAGE_DOWN: isize = 1;
const SELECT_ALL: &str = "*";

pub(crate) fn perform(state: &mut AppState, action: Action, now: Instant) -> Vec<Effect> {
    let side = state.active;
    if matches!(
        action,
        Action::Search
            | Action::AnalyzeSpace
            | Action::FolderTree
            | Action::CalculateSizes
            | Action::OpenCollection
            | Action::BackResults
    ) {
        state.activity.side = None;
        crate::docker::close(state);
    }

    if (state.activity.side.is_some() || state.docker.is_open())
        && matches!(
            action,
            Action::Copy
                | Action::Move
                | Action::Delete
                | Action::DeleteOriginals
                | Action::View
                | Action::Edit
                | Action::Mkdir
                | Action::Compress
                | Action::ExtractHere
                | Action::ExtractToOther
                | Action::Collect
                | Action::Git(_)
        )
    {
        state.notice = Some(
            if state.docker.is_open() { tr!(docker_close_for_files) } else { tr!(activity_close_for_files) }.into(),
        );
        return Vec::new();
    }

    let mounted = state.panel(side).mount.is_some();
    let other_mounted = state.panel(side.other()).mount.is_some();
    let remote = state.panel(side).remote_mount.is_some();
    let other_remote = state.panel(side.other()).remote_mount.is_some();
    if (remote || other_remote)
        && matches!(
            action,
            Action::Move
                | Action::Delete
                | Action::DeleteOriginals
                | Action::Edit
                | Action::Mkdir
                | Action::Compress
                | Action::ExtractHere
                | Action::ExtractToOther
                | Action::Collect
                | Action::Git(_)
                | Action::QuickView
                | Action::Info
                | Action::Search
                | Action::AnalyzeSpace
                | Action::FolderTree
                | Action::CalculateSizes
                | Action::OpenCollection
                | Action::InsertName
                | Action::InsertPath(_)
                | Action::MeasureSizes
                | Action::RefreshSizes
                | Action::RevealOriginal
                | Action::RemoveReferences
        )
    {
        state.notice = Some(tr!(remote_operation_unavailable).into());
        return Vec::new();
    }
    if mounted
        && matches!(
            action,
            Action::Edit
                | Action::QuickView
                | Action::Info
                | Action::Compress
                | Action::ExtractHere
                | Action::ExtractToOther
                | Action::Search
                | Action::AnalyzeSpace
                | Action::FolderTree
                | Action::CalculateSizes
                | Action::Collect
                | Action::OpenCollection
                | Action::Git(_)
        )
        || other_mounted && action == Action::ExtractToOther
    {
        state.notice = Some(tr!(docker_not_in_container).into());
        return Vec::new();
    }
    if (mounted || other_mounted) && action == Action::Move {
        state.notice = Some(tr!(docker_move_unsupported).into());
        return Vec::new();
    }

    if state.active_panel().is_results()
        && matches!(action, Action::Compress | Action::ExtractHere | Action::ExtractToOther | Action::Mkdir)
    {
        state.notice = Some(tr!(reveal_the_original_folder_to_create_directories).into());
        return Vec::new();
    }
    match action {
        Action::MeasureSizes => crate::measure::toggle(state),
        Action::Activity => crate::activity::open(state),
        Action::ToggleSystemPlugin => {
            state.settings.preferences.system_plugin = !state.settings.preferences.system_plugin;
            vec![Effect::SaveConfig]
        }
        Action::Docker => crate::docker::toggle(state, now),
        Action::DockerAct(action) => crate::docker::act(state, action),
        Action::DockerFiles => crate::docker::browse(state),
        Action::DockerShell => crate::docker::open_shell(state, now),
        Action::AgentLog => crate::agent::open_log(state),
        Action::ConnectClaude => crate::agent::open_connect(state),
        Action::DockerFilter => {
            crate::docker::filter(state);
            Vec::new()
        }
        Action::ToggleDockerPlugin => {
            state.settings.preferences.docker_plugin = !state.settings.preferences.docker_plugin;
            vec![Effect::SaveConfig]
        }
        Action::ToggleDockerConsole => {
            state.settings.preferences.docker_console = !state.settings.preferences.docker_console;
            vec![Effect::SaveConfig]
        }
        Action::ToggleAgentCloud => {
            state.settings.preferences.agent_cloud = !state.settings.preferences.agent_cloud;
            vec![Effect::SaveConfig]
        }
        Action::ToggleAgentAskReads => {
            state.settings.preferences.agent_ask_reads = !state.settings.preferences.agent_ask_reads;
            vec![Effect::SaveConfig]
        }
        Action::ToggleAgentLog => {
            state.settings.preferences.agent_log = !state.settings.preferences.agent_log;
            if !state.settings.preferences.agent_log {
                state.agent.log.clear();
            }
            vec![Effect::SaveConfig]
        }
        Action::Search
        | Action::FolderTree
        | Action::CalculateSizes
        | Action::RefreshSizes
        | Action::ToggleTreeNode
        | Action::AnalyzeSpace
        | Action::Collect
        | Action::OpenCollection
        | Action::FilterResults
        | Action::GroupResults(_)
        | Action::ToggleExpanded
        | Action::SetExpanded(_)
        | Action::BackResults
        | Action::RevealOriginal
        | Action::RemoveReferences
        | Action::CancelSearch => crate::results::perform(state, action),
        Action::DeleteOriginals => confirm_delete(state),
        Action::Quit => {
            state.overlay = Some(Overlay::Confirm(Confirm {
                title: tr!(quit),
                message: if state.jobs.is_empty() && !state.docker.shells.is_empty() {
                    match state.docker.shells.len() {
                        1 => tr!(quit_shells_one).into(),
                        count => tr!(quit_shells_many, count = count),
                    }
                } else if state.jobs.is_empty() {
                    tr!(quit_borg_commander).into()
                } else {
                    if state.jobs.len() == 1 {
                        tr!(quit_jobs_one).into()
                    } else {
                        tr!(quit_jobs_many, count = state.jobs.len())
                    }
                },
                purpose: ConfirmPurpose::Quit,
                accept: false,
            }));
            Vec::new()
        }
        Action::Jobs => crate::jobs::open_list(state),
        Action::ToggleShell => toggle_shell(state),
        Action::MirrorRemote => {
            let mounted = state.panels.iter().find_map(|panel| panel.remote_mount.as_ref());
            match mounted {
                Some(mount) => crate::remote::mirror(state, mount.machine.clone(), mount.session.clone()),
                None => {
                    state.notice = Some("Open Remote Borgs with Alt-R, then choose a session to mirror.".into());
                    Vec::new()
                }
            }
        }
        Action::BrowseRemotes => {
            state.overlay = Some(Overlay::Remotes(crate::RemoteBrowser {
                side: state.active,
                choices: Vec::new(),
                selected: 0,
                loading: true,
                connecting: false,
                error: None,
                clients: None,
                show_clients: !state.agent.computers.is_empty(),
                client_selected: 0,
                deleting: None,
                forgetting: None,
            }));
            vec![Effect::OpenRemotes]
        }
        Action::ApprovalModeOff => vec![Effect::ApprovalModeOff],
        Action::SwitchPanel if state.aside.is_some() => {
            state.preview_focused = !state.preview_focused;
            Vec::new()
        }
        Action::SwitchPanel => {
            state.active = side.other();
            Vec::new()
        }
        Action::InsertNewline => {
            let mut effects = prepare_shell(state);
            effects.push(Effect::Write(borg_term::encode_newline(state.shell.observed.mode)));
            state.shell.mark_typed();
            effects
        }
        Action::QuickView => crate::aside::toggle(state, crate::Aside::QuickView),
        Action::Info => crate::aside::toggle(state, crate::Aside::Info),
        Action::Up => moved(state, |panel| panel.move_by(LINE_UP)),
        Action::Down => moved(state, |panel| panel.move_by(LINE_DOWN)),
        Action::PageUp => moved(state, |panel| panel.page(PAGE_UP)),
        Action::PageDown => moved(state, |panel| panel.page(PAGE_DOWN)),
        Action::Top => moved(state, Panel::to_start),
        Action::Bottom => moved(state, Panel::to_end),
        Action::Open => open(state, side),
        Action::Descend if state.panel(side).is_tree() => {
            state.panel_mut(side).tree_step(true);
            Vec::new()
        }
        Action::Leave if state.panel(side).is_tree() => {
            state.panel_mut(side).tree_step(false);
            Vec::new()
        }
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
        Action::Plugins => {
            state.overlay = Some(Overlay::Plugins { selected: 0 });
            Vec::new()
        }
        Action::ToggleGitPlugin => {
            state.settings.preferences.git_plugin = !state.settings.preferences.git_plugin;
            vec![Effect::SaveConfig]
        }
        Action::Git(tool) => crate::plugins::run(state, tool),
        Action::PickLanguage => {
            let current = state.settings.preferences.language as usize;
            state.overlay = Some(Overlay::Languages(Picker { selected: current, original: current }));
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
        Action::SelectGroup => prompt(state, tr!(select_group), tr!(files_matching), SELECT_ALL, PromptPurpose::Select),
        Action::UnselectGroup => {
            prompt(state, tr!(unselect_group), tr!(files_matching), SELECT_ALL, PromptPurpose::Unselect)
        }
        Action::InvertSelection => moved(state, Panel::invert_marks),
        Action::View if mounted => {
            hovered_file(state).map(|path| crate::docker::view(state, &path)).unwrap_or_default()
        }
        Action::View if remote => remote_view(state, side),
        Action::View => hovered_file(state).map(|path| crate::viewer::open(state, path)).unwrap_or_default(),
        Action::Edit => edit(state),
        Action::Copy if remote || other_remote => {
            if mounted || other_mounted || state.panel(side.other()).is_results() {
                state.notice = Some(tr!(remote_copy_destination).into());
                return Vec::new();
            }
            let from = state.panel(side).remote_mount.clone();
            let to = state.panel(side.other()).remote_mount.clone();
            let destination = to
                .as_ref()
                .map(|mount| {
                    tr!(
                        remote_copy_host,
                        machine = state.remote_host_name(&mount.machine),
                        session = state.remote_session_name(&mount.machine, &mount.session, &mount.name)
                    )
                })
                .unwrap_or_else(|| tr!(remote_copy_client).into());
            let effects = transfer(state, tr!(copy), |sources| PromptPurpose::RemoteCopy { sources, from, to });
            if let Some(Overlay::Prompt(prompt)) = &mut state.overlay {
                prompt.label = tr!(remote_copy_label, label = prompt.label, destination = destination);
            }
            effects
        }
        Action::Copy if mounted || other_mounted => {
            transfer(state, tr!(copy), |sources| PromptPurpose::DockerCopy { sources })
        }
        Action::Copy if state.panel(side.other()).is_results() => {
            if matches!(
                state.panel(side.other()).results.as_ref().map(|r| &r.source),
                Some(crate::ResultSource::Collection(_))
            ) {
                crate::results::collect(state)
            } else {
                state.notice = Some(tr!(choose_a_real_folder_or_collection_as).into());
                Vec::new()
            }
        }
        Action::Move if state.panel(side.other()).is_results() => {
            state.notice = Some(tr!(move_needs_a_real_destination_folder).into());
            Vec::new()
        }
        Action::Copy => transfer(state, tr!(copy), |sources| PromptPurpose::Copy { sources, optimize_hdd: false }),
        Action::Move => transfer(state, tr!(move), |sources| PromptPurpose::Move { sources }),
        Action::Mkdir => prompt(state, tr!(make_directory), tr!(name), "", PromptPurpose::Mkdir),
        Action::Delete if state.active_panel().is_results() => {
            if matches!(
                state.active_panel().results.as_ref().map(|r| &r.source),
                Some(crate::ResultSource::Collection(_))
            ) {
                crate::results::perform(state, Action::RemoveReferences)
            } else {
                confirm_delete(state)
            }
        }
        Action::Delete => confirm_delete(state),
        Action::Compress => compress(state),
        Action::ExtractHere => extract(state, state.active_panel().dir().to_path_buf().as_path(), now),
        Action::ExtractToOther => extract(state, state.panel(side.other()).dir().to_path_buf().as_path(), now),
        Action::SwapPanels => {
            state.panels.swap(0, 1);
            state.result_history.swap(0, 1);
            state.last_results.swap(0, 1);
            state.panels.iter_mut().for_each(|panel| panel.git = crate::GitPanel::default());
            if state.panels.iter().any(Panel::is_results) { state.reload_all() } else { Vec::new() }
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
        Action::Find if state.active_panel().is_results() => crate::results::filter_prompt(state),
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
        None => open_menu(state, if side == Side::Left { 0 } else { RIGHT_MENU }),
    }
}

fn prompt(state: &mut AppState, title: &'static str, label: &str, value: &str, purpose: PromptPurpose) -> Vec<Effect> {
    state.overlay = Some(Overlay::Prompt(Prompt {
        title,
        label: label.to_owned(),
        input: LineInput::new(value),
        purpose,
        option_focused: false,
    }));
    Vec::new()
}

fn toggle_shell(state: &mut AppState) -> Vec<Effect> {
    if state.view == View::Panels && state.active_panel().remote_mount.is_some() && !state.console_owns_panel() {
        state.notice = Some(crate::remote::blocked_console_notice(state));
        return Vec::new();
    }
    match state.view {
        View::Panels => {
            state.view = View::Shell;
            state.shell.return_view = View::Shell;
            prepare_shell(state)
        }
        View::Shell => {
            state.view = View::Panels;
            state.shell.return_view = View::Panels;
            vec![Effect::ScrollConsole(borg_term::Scroll::Bottom)]
        }
    }
}

fn open(state: &mut AppState, side: Side) -> Vec<Effect> {
    let panel = state.panel(side);
    match panel.hovered() {
        Some(Row::Group { .. }) => {
            state.panel_mut(side).toggle_group();
            Vec::new()
        }
        Some(Row::Parent) => leave(state, side).into_iter().collect(),
        Some(Row::Entry(entry)) if entry.is_dir() => descend(state, side).into_iter().collect(),
        Some(Row::Entry(_)) if panel.remote_mount.is_some() => remote_view(state, side),
        Some(Row::Entry(entry)) if panel.mount.is_some() => {
            let path = entry.path(panel.dir());
            crate::docker::view(state, &path)
        }
        Some(Row::Entry(entry)) if entry.executable => {
            let command = if panel.is_results() {
                CommandLine::default().path(&entry.path(panel.dir()))
            } else {
                ops::run(&entry.name)
            };
            interactive(state, &command)
        }
        Some(Row::Entry(entry)) => crate::viewer::open(state, entry.path(panel.dir())),
        None => Vec::new(),
    }
}

fn descend(state: &mut AppState, side: Side) -> Option<Effect> {
    if matches!(state.panel(side).hovered(), Some(Row::Group { .. })) {
        state.panel_mut(side).toggle_group();
        return None;
    }
    let panel = state.panel(side);
    let entry = panel.hovered_entry().filter(|entry| entry.is_dir())?;
    let dir = entry.path(panel.dir());
    Some(state.navigate(side, dir, None))
}

fn leave(state: &mut AppState, side: Side) -> Option<Effect> {
    if state.panel(side).remote_mount.is_some() && state.panel(side).dir().parent().is_none() {
        return crate::remote::unmount(state, side);
    }
    if state.panel(side).mount.is_some() && state.panel(side).dir().parent().is_none() {
        return crate::docker::unmount(state, side);
    }
    if state.panel(side).is_results() {
        return crate::results::back(state).into_iter().next();
    }
    let current = state.panel(side).dir();
    let parent = current.parent()?.to_path_buf();
    let child = current.file_name().map(|name| name.to_string_lossy().into_owned());
    Some(state.navigate(side, parent, child))
}

fn insert_names(state: &mut AppState, side: Side) -> Vec<Effect> {
    let panel = state.panel(side);
    let selected = panel.selection();
    if selected.is_empty() {
        return Vec::new();
    }
    let words = selected.iter().fold(CommandLine::default(), |line, entry| {
        let path = entry.path(panel.dir());
        if panel.is_results() {
            line.path(&path)
        } else {
            line.path(std::path::Path::new(path.file_name().unwrap_or_default()))
        }
    });
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
    panel.hovered_entry().filter(|entry| !entry.is_dir()).map(|entry| entry.path(panel.dir()))
}

fn remote_view(state: &AppState, side: Side) -> Vec<Effect> {
    let panel = state.panel(side);
    let Some(mount) = &panel.remote_mount else { return Vec::new() };
    panel
        .hovered_entry()
        .filter(|entry| !entry.is_dir())
        .map(|entry| Effect::RemoteView {
            machine: mount.machine.clone(),
            session: mount.session.clone(),
            path: entry.path(panel.dir()),
        })
        .into_iter()
        .collect()
}

fn edit(state: &mut AppState) -> Vec<Effect> {
    let Some(entry) = state.active_panel().hovered_entry().filter(|entry| !entry.is_dir()) else { return Vec::new() };
    let panel = state.active_panel();
    let command = if panel.is_results() { ops::edit_path(&entry.path(panel.dir())) } else { ops::edit(&entry.name) };
    interactive(state, &command)
}

fn interactive(state: &mut AppState, command: &CommandLine) -> Vec<Effect> {
    if !shell_ready(state) {
        state.notice = Some(tr!(shell_is_busy_finish_or_clear_the).to_owned());
        return Vec::new();
    }
    let mut effects = prepare_shell(state);
    effects.push(Effect::Write(command.submit()));
    state.shell.return_view = state.view;
    state.shell.submit();
    state.view = View::Shell;
    effects
}

fn transfer(
    state: &mut AppState,
    title: &'static str,
    purpose: impl FnOnce(Vec<std::path::PathBuf>) -> PromptPurpose,
) -> Vec<Effect> {
    let sources = crate::selected_paths(state.active_panel());
    if sources.is_empty() {
        state.notice = Some(tr!(nothing_selected).to_owned());
        return Vec::new();
    }
    let label = tr!(
        transfer_to,
        title = title,
        selection = if state.active_panel().is_results() {
            describe_result_selection(state.active_panel(), &sources)
        } else {
            describe(&selected_names(state.active_panel()))
        }
    );
    let dest = state.panel(state.active.other()).dir().display().to_string();
    prompt(state, title, &label, &dest, purpose(sources))
}

fn confirm_delete(state: &mut AppState) -> Vec<Effect> {
    let sources = crate::selected_paths(state.active_panel());
    if sources.is_empty() {
        state.notice = Some(tr!(nothing_selected).to_owned());
        return Vec::new();
    }
    let message = tr!(
        delete_permanently,
        if state.active_panel().is_results() {
            describe_result_selection(state.active_panel(), &sources)
        } else {
            describe(&selected_names(state.active_panel()))
        }
    );
    state.overlay = Some(Overlay::Confirm(Confirm {
        title: tr!(delete),
        message,
        purpose: ConfirmPurpose::Delete { sources },
        accept: !state.active_panel().is_results(),
    }));
    Vec::new()
}

fn compress(state: &mut AppState) -> Vec<Effect> {
    let sources = selected_names(state.active_panel());
    let stem = match sources.as_slice() {
        [] => {
            state.notice = Some(tr!(nothing_selected).to_owned());
            return Vec::new();
        }
        [single] => single.clone(),
        _ => state.active_panel().dir().file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
    };
    let default = format!("{stem}{DEFAULT_ARCHIVE_SUFFIX}");
    prompt(state, tr!(compress), &tr!(archive_as, describe(&sources)), &default, PromptPurpose::Compress { sources })
}

fn extract(state: &mut AppState, dest: &Path, now: Instant) -> Vec<Effect> {
    let Some(entry) = state.active_panel().hovered_entry() else { return Vec::new() };
    let Some(format) = ArchiveFormat::detect(&entry.name) else { return Vec::new() };
    let command = ops::extract(&entry.name, format, dest);
    run_operation(state, tr!(extract), &command, None, now)
}

pub(crate) fn run_operation(
    state: &mut AppState,
    label: &'static str,
    command: &CommandLine,
    focus: Option<String>,
    now: Instant,
) -> Vec<Effect> {
    if !shell_ready(state) {
        state.notice = Some(tr!(shell_is_busy_finish_or_clear_the).to_owned());
        return Vec::new();
    }
    let mut effects = prepare_shell(state);
    effects.push(Effect::Write(command.submit_hidden()));
    state.shell.return_view = state.view;

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
        many => tr!(items, many.len()),
    }
}

fn describe_paths(paths: &[std::path::PathBuf]) -> String {
    describe(&paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>())
}

fn describe_result_selection(panel: &Panel, paths: &[std::path::PathBuf]) -> String {
    let description = describe_paths(paths);
    let hidden =
        panel.results.as_ref().map_or(0, |results| panel.marked().filter(|entry| !results.matches(entry)).count());
    if hidden > 0 { tr!(hidden_by_filter, description = description, hidden = hidden) } else { description }
}
