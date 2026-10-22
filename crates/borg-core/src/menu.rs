use crate::{Action, AfterCommand, AppState, ListingMode, Side, SortKey, ops::ArchiveFormat};
use borg_i18n::tr;

pub const FILE_MENU: usize = 1;
pub const RIGHT_MENU: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub label: &'static str,
    pub hint: &'static str,
    pub action: Action,
    pub checked: Option<bool>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    Item(MenuItem),
    Separator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub title: &'static str,
    pub entries: Vec<MenuEntry>,
}

impl Menu {
    pub fn item(&self, index: usize) -> Option<&MenuItem> {
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

fn item(label: &'static str, hint: &'static str, action: Action) -> MenuEntry {
    MenuEntry::Item(MenuItem { label, hint, action, checked: None, enabled: true })
}

fn check(label: &'static str, hint: &'static str, action: Action, checked: bool) -> MenuEntry {
    MenuEntry::Item(MenuItem { label, hint, action, checked: Some(checked), enabled: true })
}

fn when(entry: MenuEntry, enabled: bool) -> MenuEntry {
    match entry {
        MenuEntry::Item(item) => MenuEntry::Item(MenuItem { enabled, ..item }),
        MenuEntry::Separator => MenuEntry::Separator,
    }
}

pub fn menus(state: &AppState) -> Vec<Menu> {
    let mut menus = vec![
        panel_menu(tr!(left), state, Side::Left),
        file_menu(state),
        command_menu(state),
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
        title: tr!(docker_title),
        entries: vec![
            check(tr!(docker_menu_containers), "M-c", Action::Docker, state.docker.is_open()),
            MenuEntry::Separator,
            when(item(tr!(docker_menu_start), "S", Action::DockerAct(Docker::Start)), can(Docker::Start)),
            when(item(tr!(docker_menu_stop), "X", Action::DockerAct(Docker::Stop)), can(Docker::Stop)),
            when(item(tr!(docker_menu_restart), "R", Action::DockerAct(Docker::Restart)), can(Docker::Restart)),
            when(
                item(
                    if pause == Docker::Unpause { tr!(docker_menu_resume) } else { tr!(docker_menu_pause) },
                    "P",
                    Action::DockerAct(pause),
                ),
                can(pause),
            ),
            MenuEntry::Separator,
            when(item(tr!(docker_menu_files), "F", Action::DockerFiles), crate::docker::can_browse(state)),
            when(item(tr!(docker_menu_shell), "E", Action::DockerShell), crate::docker::can_open_shell(state)),
            when(item(tr!(docker_menu_filter), "/", Action::DockerFilter), state.docker.is_open()),
        ],
    }
}

fn git_menu(state: &AppState) -> Menu {
    use borg_git::Tool;
    let available = state.activity.side.is_none()
        && !state.docker.is_open()
        && state.active_panel().git_repository().is_some()
        && state.plugins.git_task.is_none();
    let selected = available && !state.active_panel().selection().is_empty();
    Menu {
        title: tr!(git),
        entries: vec![
            when(item(tr!(repository_status), "", Action::Git(Tool::Status)), available),
            when(item(tr!(diff_selection), "", Action::Git(Tool::Diff)), selected),
            when(item(tr!(staged_diff_selection), "", Action::Git(Tool::StagedDiff)), selected),
            when(item(tr!(recent_commits), "", Action::Git(Tool::Log)), available),
            MenuEntry::Separator,
            when(item(tr!(stage_selection), "", Action::Git(Tool::Stage)), selected),
            when(item(tr!(unstage_selection), "", Action::Git(Tool::Unstage)), selected),
            MenuEntry::Separator,
            item(tr!(refresh_status), "", Action::Rescan(state.active)),
        ],
    }
}

fn panel_menu(title: &'static str, state: &AppState, side: Side) -> Menu {
    let panel = state.panel(side);
    let sort = panel.sort();
    let sorted = |key: SortKey| sort.key == key;
    Menu {
        title,
        entries: vec![
            check(
                tr!(full_listing),
                "",
                Action::SetListing(side, ListingMode::Full),
                panel.mode() == ListingMode::Full,
            ),
            check(
                tr!(brief_listing),
                "",
                Action::SetListing(side, ListingMode::Brief),
                panel.mode() == ListingMode::Brief,
            ),
            MenuEntry::Separator,
            check(tr!(sort_by_name), "", Action::SetSort(side, SortKey::Name), sorted(SortKey::Name)),
            check(tr!(sort_by_extension), "", Action::SetSort(side, SortKey::Extension), sorted(SortKey::Extension)),
            check(tr!(sort_by_size), "", Action::SetSort(side, SortKey::Size), sorted(SortKey::Size)),
            check(tr!(sort_by_modified), "", Action::SetSort(side, SortKey::Modified), sorted(SortKey::Modified)),
            check(tr!(reverse_order), "", Action::ToggleReverse(side), sort.reverse),
            MenuEntry::Separator,
            check(tr!(show_hidden_files), "M-.", Action::ToggleHidden, state.settings.preferences.show_hidden),
            item(tr!(rescan), "", Action::Rescan(side)),
        ],
    }
}

fn file_menu(state: &AppState) -> Menu {
    let panel = state.active_panel();
    let has_selection = !panel.selection().is_empty();
    let hovered_file = panel.hovered_entry().is_some_and(|entry| !entry.is_dir());
    let archive = panel.hovered_entry().is_some_and(|entry| ArchiveFormat::detect(&entry.name).is_some());
    Menu {
        title: tr!(file),
        entries: vec![
            when(item(tr!(view), "F3", Action::View), hovered_file),
            when(item(tr!(edit), "F4", Action::Edit), hovered_file),
            when(item(tr!(copy), "F5", Action::Copy), has_selection),
            when(item(tr!(move), "F6", Action::Move), has_selection),
            item(tr!(make_directory), "F7", Action::Mkdir),
            when(
                item(if panel.is_results() { tr!(remove_actions) } else { tr!(delete) }, "F8", Action::Delete),
                has_selection,
            ),
            MenuEntry::Separator,
            item(tr!(select_group), "+", Action::SelectGroup),
            item(tr!(unselect_group), "-", Action::UnselectGroup),
            item(tr!(invert_selection), "*", Action::InvertSelection),
            MenuEntry::Separator,
            when(item(tr!(menu_compress), "", Action::Compress), has_selection && !panel.is_results()),
            when(item(tr!(extract_here), "", Action::ExtractHere), archive && !panel.is_results()),
            when(item(tr!(extract_to_other_panel), "", Action::ExtractToOther), archive && !panel.is_results()),
            MenuEntry::Separator,
            item(tr!(quit), "F10", Action::Quit),
        ],
    }
}

fn command_menu(state: &AppState) -> Menu {
    Menu {
        title: tr!(command),
        entries: vec![
            when(item(tr!(activity_title), "M-m", Action::Activity), state.settings.preferences.system_plugin),
            when(item(tr!(docker_title), "M-c", Action::Docker), state.settings.preferences.docker_plugin),
            item(tr!(find_in_folder), "C-s", Action::Find),
            when(item(tr!(insert_new_line), "S-Enter", Action::InsertNewline), !state.shell.busy()),
            check(tr!(quick_view), "C-q", Action::QuickView, state.aside == Some(crate::Aside::QuickView)),
            check(tr!(info), "C-l", Action::Info, state.aside == Some(crate::Aside::Info)),
            when(item(tr!(jobs), "M-j", Action::Jobs), !state.jobs.is_empty()),
            item(tr!(swap_panels), "C-u", Action::SwapPanels),
            item(tr!(equal_panel_widths), "", Action::EqualizePanels),
            item(tr!(widen_left_panel), "M-S-→", Action::NudgeSplit(crate::keymap::SPLIT_STEP)),
            item(tr!(widen_right_panel), "M-S-←", Action::NudgeSplit(-crate::keymap::SPLIT_STEP)),
            MenuEntry::Separator,
            item(tr!(show_shell), "C-o", Action::ToggleShell),
            MenuEntry::Separator,
            item(tr!(about_borg), "F1", Action::About),
        ],
    }
}

fn options_menu(_state: &AppState) -> Menu {
    Menu {
        title: tr!(options),
        entries: vec![
            item(tr!(theme), "", Action::PickTheme),
            item(tr!(settings), "", Action::Settings),
            item(tr!(plugins), "", Action::Plugins),
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
    ]
    .into_iter()
    .map(|(label, action, checked)| MenuItem { label, hint: "", action, checked: Some(checked), enabled: true })
    .collect()
}

pub fn checkbox_items(state: &AppState) -> Vec<MenuItem> {
    if matches!(state.overlay, Some(crate::Overlay::Plugins { .. })) {
        vec![
            MenuItem {
                label: tr!(git_status_diffs_stage_unstage),
                hint: "",
                action: Action::ToggleGitPlugin,
                checked: Some(state.settings.preferences.git_plugin),
                enabled: true,
            },
            MenuItem {
                label: tr!(system_plugin_label),
                hint: "",
                action: Action::ToggleSystemPlugin,
                checked: Some(state.settings.preferences.system_plugin),
                enabled: true,
            },
            MenuItem {
                label: tr!(docker_plugin_label),
                hint: "",
                action: Action::ToggleDockerPlugin,
                checked: Some(state.settings.preferences.docker_plugin),
                enabled: true,
            },
            MenuItem {
                label: tr!(docker_console_plugin_label),
                hint: "",
                action: Action::ToggleDockerConsole,
                checked: Some(state.settings.preferences.docker_console),
                enabled: state.settings.preferences.docker_plugin,
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
        title: tr!(search),
        entries: vec![
            item(tr!(search_recursively), "C-f", Action::Search),
            item(tr!(analyze_folder_sizes), "M-d", Action::AnalyzeSpace),
            item(tr!(folder_size_tree), "M-S-d", Action::FolderTree),
            item(tr!(calculate_sizes), "M-s", Action::CalculateSizes),
            when(
                item(
                    if state.sizing.is_some() { tr!(stop_background_sizes) } else { tr!(count_background_sizes) },
                    "M-i",
                    Action::MeasureSizes,
                ),
                state.sizing.is_some() || !result && state.activity.side.is_none() && !state.docker.is_open(),
            ),
            when(item(tr!(refresh_sizes), "C-r", Action::RefreshSizes), panel.is_size_view()),
            item(tr!(open_create_collection), "M-v", Action::OpenCollection),
            when(item(tr!(collect_in_other_panel), "M-b", Action::Collect), !panel.selection().is_empty()),
            MenuEntry::Separator,
            when(item(tr!(filter_results), "C-s", Action::FilterResults), result),
            when(
                check(
                    tr!(group_by_folder),
                    "",
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
                    "",
                    Action::GroupResults(if group == Some(crate::ResultGroup::Type) {
                        crate::ResultGroup::None
                    } else {
                        crate::ResultGroup::Type
                    }),
                    group == Some(crate::ResultGroup::Type),
                ),
                result && !panel.is_tree(),
            ),
            when(item(tr!(sort_by_source_folder), "", Action::SetSort(state.active, SortKey::Source)), result),
            when(check(tr!(expanded_results), "M-w", Action::ToggleExpanded, state.results_expanded()), result),
            MenuEntry::Separator,
            when(item(tr!(reveal_original), "", Action::RevealOriginal), result && panel.hovered_entry().is_some()),
            when(item(tr!(remove_references), "F8", Action::RemoveReferences), collection),
            when(item(tr!(delete_originals), "S-F8", Action::DeleteOriginals), result && !panel.selection().is_empty()),
            when(
                item(tr!(cancel_search), "C-c", Action::CancelSearch),
                matches!(panel.results.as_ref().map(|r| &r.source), Some(crate::ResultSource::Search { .. }))
                    && panel.listing() == &crate::Listing::Loading,
            ),
            when(
                item(if result { tr!(return_to_folder) } else { tr!(resume_results) }, "M-r", Action::BackResults),
                if result {
                    state.result_history[state.active.index()].is_some()
                } else {
                    state.last_results[state.active.index()].is_some()
                },
            ),
        ],
    }
}
