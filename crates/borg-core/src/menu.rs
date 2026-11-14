use crate::{Action, AfterCommand, AppState, ListingMode, Side, SortKey};
use borg_i18n::tr;

pub const FILE_MENU: usize = 1;
pub const RIGHT_MENU: usize = 5;

/// An entry that does `action` when chosen: an action of the menu bar, or what an entry of a
/// context menu does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem<A = Action> {
    pub label: &'static str,
    pub hint: String,
    pub action: A,
    pub checked: Option<bool>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry<A = Action> {
    Item(MenuItem<A>),
    Separator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu<A = Action> {
    pub title: String,
    pub entries: Vec<MenuEntry<A>>,
}

impl<A> Menu<A> {
    pub fn item(&self, index: usize) -> Option<&MenuItem<A>> {
        match self.entries.get(index)? {
            MenuEntry::Item(item) => Some(item),
            MenuEntry::Separator => None,
        }
    }

    pub fn first_enabled(&self) -> Option<usize> {
        self.step(None, 1)
    }

    pub fn step(&self, from: Option<usize>, delta: isize) -> Option<usize> {
        let len = isize::try_from(self.entries.len()).ok()?;
        let start = from.map_or(if delta > 0 { -1 } else { len }, |i| isize::try_from(i).unwrap_or(len));
        (1..=len)
            .map(|offset| (start + offset * delta).rem_euclid(len))
            .filter_map(|i| usize::try_from(i).ok())
            .find(|&i| self.item(i).is_some_and(|item| item.enabled))
    }
}

/// An entry whose hint is the key bound to its action, when one is.
fn item(label: &'static str, action: Action) -> MenuEntry {
    let hint = crate::keymap::hint(action).unwrap_or_default();
    MenuEntry::Item(MenuItem { label, hint, action, checked: None, enabled: true })
}

fn check(label: &'static str, action: Action, checked: bool) -> MenuEntry {
    let hint = crate::keymap::hint(action).unwrap_or_default();
    MenuEntry::Item(MenuItem { label, hint, action, checked: Some(checked), enabled: true })
}

fn keyed_check(label: &'static str, hint: &'static str, action: Action, checked: bool) -> MenuEntry {
    MenuEntry::Item(MenuItem { label, hint: hint.to_owned(), action, checked: Some(checked), enabled: true })
}

/// An entry for a key that only the screen it belongs to takes, which the key table does not hold.
fn keyed(label: &'static str, hint: &'static str, action: Action) -> MenuEntry {
    MenuEntry::Item(MenuItem { label, hint: hint.to_owned(), action, checked: None, enabled: true })
}

pub(crate) fn when<A>(entry: MenuEntry<A>, enabled: bool) -> MenuEntry<A> {
    match entry {
        MenuEntry::Item(item) => MenuEntry::Item(MenuItem { enabled, ..item }),
        MenuEntry::Separator => MenuEntry::Separator,
    }
}

pub fn menus(state: &AppState) -> Vec<Menu> {
    borg_i18n::with_locale(state.settings.preferences.language, || localized_menus(state))
}

fn localized_menus(state: &AppState) -> Vec<Menu> {
    let mut menus = vec![
        panel_menu(tr!(left), state, Side::Left),
        file_menu(state),
        command_menu(state),
        remote_menu(state),
        options_menu(state),
        panel_menu(tr!(right), state, Side::Right),
    ];
    if state.settings.preferences.git_plugin {
        menus.push(git_menu(state));
    }
    if state.settings.preferences.docker_plugin {
        menus.push(docker_menu(state));
    }
    menus.push(results_menu(state));
    menus
}

fn docker_menu(state: &AppState) -> Menu {
    use borg_docker::Action as Docker;
    let can = |action| crate::docker::can(state, action);
    let pause = crate::docker::pause_action(state);
    Menu {
        title: tr!(docker_title).into(),
        entries: vec![
            check(tr!(docker_menu_containers), Action::Docker, state.docker.is_open()),
            keyed_check(
                tr!(docker_menu_disk),
                "D",
                Action::DockerDisk,
                state.docker.is_open() && state.docker.disk.open,
            ),
            MenuEntry::Separator,
            when(keyed(tr!(docker_menu_start), "S", Action::DockerAct(Docker::Start)), can(Docker::Start)),
            when(keyed(tr!(docker_menu_stop), "X", Action::DockerAct(Docker::Stop)), can(Docker::Stop)),
            when(keyed(tr!(docker_menu_restart), "R", Action::DockerAct(Docker::Restart)), can(Docker::Restart)),
            when(
                keyed(
                    if pause == Docker::Unpause { tr!(docker_menu_resume) } else { tr!(docker_menu_pause) },
                    "P",
                    Action::DockerAct(pause),
                ),
                can(pause),
            ),
            MenuEntry::Separator,
            when(keyed(tr!(docker_menu_files), "F", Action::DockerFiles), crate::docker::can_browse(state)),
            when(keyed(tr!(docker_menu_shell), "E", Action::DockerShell), crate::docker::can_open_shell(state)),
            when(
                keyed(
                    if state.docker.disk.open { tr!(docker_menu_filter_disk) } else { tr!(docker_menu_filter) },
                    "/",
                    Action::DockerFilter,
                ),
                state.docker.is_open(),
            ),
        ],
    }
}

fn git_menu(state: &AppState) -> Menu {
    use borg_git::Tool;
    let available =
        !state.monitor_open() && state.active_panel().git_repository().is_some() && state.plugins.git_task.is_none();
    let selected = available && !state.active_panel().selection().is_empty();
    Menu {
        title: tr!(git).into(),
        entries: vec![
            when(item(tr!(repository_status), Action::Git(Tool::Status)), available),
            when(item(tr!(diff_selection), Action::Git(Tool::Diff)), selected),
            when(item(tr!(staged_diff_selection), Action::Git(Tool::StagedDiff)), selected),
            when(item(tr!(recent_commits), Action::Git(Tool::Log)), available),
            MenuEntry::Separator,
            when(item(tr!(stage_selection), Action::Git(Tool::Stage)), selected),
            when(item(tr!(unstage_selection), Action::Git(Tool::Unstage)), selected),
            MenuEntry::Separator,
            item(tr!(refresh_status), Action::Rescan(state.active)),
        ],
    }
}

fn panel_menu(title: &'static str, state: &AppState, side: Side) -> Menu {
    let panel = state.panel(side);
    let sort = panel.sort();
    let sorted = |key: SortKey| sort.key == key;
    Menu {
        title: title.into(),
        entries: vec![
            check(tr!(full_listing), Action::SetListing(side, ListingMode::Full), panel.mode() == ListingMode::Full),
            check(tr!(brief_listing), Action::SetListing(side, ListingMode::Brief), panel.mode() == ListingMode::Brief),
            MenuEntry::Separator,
            check(tr!(sort_by_name), Action::SetSort(side, SortKey::Name), sorted(SortKey::Name)),
            check(tr!(sort_by_extension), Action::SetSort(side, SortKey::Extension), sorted(SortKey::Extension)),
            check(tr!(sort_by_size), Action::SetSort(side, SortKey::Size), sorted(SortKey::Size)),
            check(tr!(sort_by_modified), Action::SetSort(side, SortKey::Modified), sorted(SortKey::Modified)),
            check(tr!(reverse_order), Action::ToggleReverse(side), sort.reverse),
            MenuEntry::Separator,
            check(tr!(show_hidden_files), Action::ToggleHidden, state.settings.preferences.show_hidden),
            keyed(tr!(rescan), if side == state.active { "M-r" } else { "" }, Action::Rescan(side)),
        ],
    }
}

fn file_menu(state: &AppState) -> Menu {
    let panel = state.active_panel();
    // The files of a panel that holds an editor are out of sight, so nothing acts on them.
    let listed = state.docked[state.active.index()].is_none();
    let has_selection = listed && !panel.selection().is_empty();
    let hovered_file = listed && panel.hovered_entry().is_some_and(|entry| !entry.is_dir());
    let archive = listed && !panel.foreign() && panel.hovered_entry().is_some_and(crate::archive::opens);
    Menu {
        title: tr!(file).into(),
        entries: vec![
            when(item(tr!(view), Action::View), hovered_file),
            when(item(tr!(edit), Action::Edit), hovered_file),
            when(item(tr!(edit_in_panel), Action::EditInPanel), hovered_file),
            when(item(tr!(edit_external), Action::EditExternal), hovered_file),
            when(item(tr!(copy), Action::Copy), has_selection),
            when(item(tr!(move), Action::Move), has_selection),
            when(item(tr!(make_directory), Action::Mkdir), listed),
            when(
                item(if panel.is_results() { tr!(remove_actions) } else { tr!(delete) }, Action::Delete),
                has_selection,
            ),
            MenuEntry::Separator,
            when(item(tr!(select_group), Action::SelectGroup), listed),
            when(item(tr!(unselect_group), Action::UnselectGroup), listed),
            when(item(tr!(invert_selection), Action::InvertSelection), listed),
            MenuEntry::Separator,
            when(item(tr!(copy_names), Action::CopyNames), has_selection),
            when(item(tr!(copy_paths), Action::CopyPaths), has_selection),
            MenuEntry::Separator,
            when(item(tr!(menu_compress), Action::Compress), has_selection && !panel.is_results()),
            when(item(tr!(extract_here), Action::ExtractHere), archive && !panel.is_results()),
            when(item(tr!(extract_to_other_panel), Action::ExtractToOther), archive && !panel.is_results()),
            MenuEntry::Separator,
            item(tr!(quit), Action::Quit),
        ],
    }
}

fn command_menu(state: &AppState) -> Menu {
    let time_machine = state.backup.available.then(|| item(tr!(backup_title), Action::TimeMachine));
    Menu {
        title: tr!(command).into(),
        entries: [
            when(item(tr!(activity_title), Action::Activity), state.settings.preferences.system_plugin),
            when(item(tr!(network_title), Action::Network), state.settings.preferences.system_plugin),
            when(item(tr!(startup_title), Action::StartupItems), state.settings.preferences.system_plugin),
            when(item(tr!(watch_title), Action::Alerts), state.settings.preferences.system_plugin),
            when(item(tr!(docker_title), Action::Docker), state.settings.preferences.docker_plugin),
        ]
        .into_iter()
        .chain(time_machine)
        .chain([
            item(tr!(find_in_folder), Action::Find),
            item(tr!(folder_hotlist), Action::Folders),
            when(item(tr!(insert_new_line), Action::InsertNewline), !state.shell.busy()),
            check(tr!(quick_view), Action::QuickView, state.aside == Some(crate::Aside::QuickView)),
            check(tr!(info), Action::Info, state.aside == Some(crate::Aside::Info)),
            item(tr!(jobs), Action::Jobs),
            item(tr!(swap_panels), Action::SwapPanels),
            item(tr!(same_folder_in_other_panel), Action::SameFolder),
            item(tr!(equal_panel_widths), Action::EqualizePanels),
            item(tr!(widen_left_panel), Action::NudgeSplit(crate::keymap::SPLIT_STEP)),
            item(tr!(widen_right_panel), Action::NudgeSplit(-crate::keymap::SPLIT_STEP)),
            MenuEntry::Separator,
            item(tr!(show_shell), Action::ToggleShell),
            MenuEntry::Separator,
            item(tr!(about_borg), Action::About),
        ])
        .collect(),
    }
}

fn remote_menu(state: &AppState) -> Menu {
    Menu {
        title: tr!(remote_menu_title).into(),
        entries: vec![
            item(tr!(remote_borgs), Action::BrowseRemotes),
            when(
                item(tr!(remote_mirror_mounted), Action::MirrorRemote),
                state.panels.iter().any(|panel| panel.remote_mount.is_some()),
            ),
            MenuEntry::Separator,
            item(tr!(remote_share_this_borg), Action::ConnectClaude),
            item(tr!(remote_log), Action::AgentLog),
            item(tr!(remote_background_service), Action::BackgroundService),
        ],
    }
}

fn options_menu(state: &AppState) -> Menu {
    Menu {
        title: tr!(options).into(),
        entries: vec![
            item(tr!(theme), Action::PickTheme),
            item(tr!(settings), Action::Settings),
            item(tr!(plugins), Action::Plugins),
            item(tr!(language), Action::PickLanguage),
            MenuEntry::Separator,
            item(tr!(storage_menu), Action::Storage),
            when(item(tr!(ask_again_menu), Action::AskAgain), state.kept_answers > 0),
        ],
    }
}

pub fn settings_items(state: &AppState) -> Vec<MenuItem> {
    let prefs = &state.settings.preferences;
    [
        (tr!(show_hidden_files), Action::ToggleHidden, prefs.show_hidden),
        (tr!(status_line), Action::ToggleStatusLine, prefs.status_line),
        (tr!(menu_bar_always_visible), Action::ToggleMenuBar, prefs.menu_bar),
        (
            tr!(return_to_panels_after_command),
            Action::ToggleReturnToPanels,
            prefs.after_command == AfterCommand::ReturnToPanels,
        ),
        (tr!(restore_shell_output_on_start), Action::ToggleRestoreTerminal, prefs.restore_terminal),
        (tr!(screensaver_when_idle), Action::ToggleScreensaver, prefs.screensaver),
        (tr!(job_notifications), Action::ToggleJobNotifications, prefs.job_notifications),
    ]
    .into_iter()
    .map(|(label, action, checked)| MenuItem {
        label,
        hint: String::new(),
        action,
        checked: Some(checked),
        enabled: true,
    })
    .collect()
}

pub fn checkbox_items(state: &AppState) -> Vec<MenuItem> {
    if matches!(state.overlay, Some(crate::Overlay::Plugins { .. })) {
        vec![
            MenuItem {
                label: tr!(git_status_diffs_stage_unstage),
                hint: String::new(),
                action: Action::ToggleGitPlugin,
                checked: Some(state.settings.preferences.git_plugin),
                enabled: true,
            },
            MenuItem {
                label: tr!(system_plugin_label),
                hint: String::new(),
                action: Action::ToggleSystemPlugin,
                checked: Some(state.settings.preferences.system_plugin),
                enabled: true,
            },
            MenuItem {
                label: tr!(watch_plugin_label),
                hint: String::new(),
                action: Action::ToggleProcessWatch,
                checked: Some(state.settings.preferences.process_watch),
                enabled: state.settings.preferences.system_plugin,
            },
            MenuItem {
                label: tr!(docker_plugin_label),
                hint: String::new(),
                action: Action::ToggleDockerPlugin,
                checked: Some(state.settings.preferences.docker_plugin),
                enabled: true,
            },
            MenuItem {
                label: tr!(docker_console_plugin_label),
                hint: String::new(),
                action: Action::ToggleDockerConsole,
                checked: Some(state.settings.preferences.docker_console),
                enabled: true,
            },
            MenuItem {
                label: tr!(agent_log_plugin_label),
                hint: String::new(),
                action: Action::ToggleAgentLog,
                checked: Some(state.settings.preferences.agent_log),
                enabled: true,
            },
            MenuItem {
                label: tr!(agent_cloud_plugin_label),
                hint: String::new(),
                action: Action::ToggleAgentCloud,
                checked: Some(state.settings.preferences.agent_cloud),
                enabled: true,
            },
            MenuItem {
                label: tr!(agent_ask_reads_plugin_label),
                hint: String::new(),
                action: Action::ToggleAgentAskReads,
                checked: Some(state.settings.preferences.agent_ask_reads),
                enabled: state.settings.preferences.agent_cloud,
            },
        ]
    } else {
        settings_items(state)
    }
}

fn results_menu(state: &AppState) -> Menu {
    let panel = state.active_panel();
    let result = panel.is_results();
    let collection = matches!(panel.results.as_ref().map(|r| &r.source), Some(crate::ResultSource::Collection(_)));
    let group = panel.results.as_ref().map(|r| r.group);
    Menu {
        title: tr!(search).into(),
        entries: vec![
            item(tr!(search_recursively), Action::Search),
            item(tr!(analyze_folder_sizes), Action::AnalyzeSpace),
            item(tr!(folder_size_tree), Action::FolderTree),
            item(tr!(calculate_sizes), Action::CalculateSizes),
            when(
                item(
                    if state.sizing.is_some() { tr!(stop_background_sizes) } else { tr!(count_background_sizes) },
                    Action::MeasureSizes,
                ),
                state.sizing.is_some() || !result && !state.monitor_open(),
            ),
            when(keyed(tr!(refresh_sizes), "C-r", Action::RefreshSizes), panel.is_size_view()),
            when(
                check(
                    tr!(index_this_folder),
                    Action::ToggleIndex,
                    state.settings.index_roots.iter().any(|root| root == panel.dir()),
                ),
                !result && !panel.foreign(),
            ),
            item(tr!(open_create_collection), Action::OpenCollection),
            when(item(tr!(collect_in_other_panel), Action::Collect), !panel.selection().is_empty()),
            MenuEntry::Separator,
            when(keyed(tr!(filter_results), "C-s", Action::FilterResults), result),
            when(
                check(
                    tr!(group_by_folder),
                    Action::GroupResults(if group == Some(crate::ResultGroup::Folder) {
                        crate::ResultGroup::None
                    } else {
                        crate::ResultGroup::Folder
                    }),
                    group == Some(crate::ResultGroup::Folder),
                ),
                result && !panel.is_tree(),
            ),
            when(
                check(
                    tr!(group_by_type),
                    Action::GroupResults(if group == Some(crate::ResultGroup::Type) {
                        crate::ResultGroup::None
                    } else {
                        crate::ResultGroup::Type
                    }),
                    group == Some(crate::ResultGroup::Type),
                ),
                result && !panel.is_tree(),
            ),
            when(item(tr!(sort_by_source_folder), Action::SetSort(state.active, SortKey::Source)), result),
            when(check(tr!(expanded_results), Action::ToggleExpanded, state.results_expanded()), result),
            MenuEntry::Separator,
            when(item(tr!(reveal_original), Action::RevealOriginal), result && panel.hovered_entry().is_some()),
            when(keyed(tr!(remove_references), "F8", Action::RemoveReferences), collection),
            when(item(tr!(delete_originals), Action::DeleteOriginals), result && !panel.selection().is_empty()),
            when(
                keyed(tr!(replace_in_files), "C-r", Action::ReplaceInFiles),
                crate::replace::contents_query(panel).is_some(),
            ),
            when(
                keyed(tr!(cancel_search), "C-c", Action::CancelSearch),
                matches!(panel.results.as_ref().map(|r| &r.source), Some(crate::ResultSource::Search { .. }))
                    && panel.listing() == &crate::Listing::Loading,
            ),
            when(
                item(if result { tr!(return_to_folder) } else { tr!(resume_results) }, Action::BackResults),
                if result {
                    state.result_history[state.active.index()].is_some()
                } else {
                    state.last_results[state.active.index()].is_some()
                },
            ),
        ],
    }
}
