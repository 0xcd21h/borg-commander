//! Carrying out an `Action`, whichever key, menu item or click asked for it. The panels' places
//! rule some out first; the rest go to the module that owns them.

use std::time::Instant;

use borg_i18n::tr;

use crate::{Action, AppState, Effect, Overlay, Panel, PromptPurpose, layout::EVEN_SPLIT, menu::FILE_MENU};

mod dialogs;
mod files;
mod panels;
mod refusals;
mod settings;
mod shell;

pub(crate) use shell::run_operation;

const LINE_UP: isize = -1;
const LINE_DOWN: isize = 1;
const PAGE_UP: isize = -1;
const PAGE_DOWN: isize = 1;
const SELECT_ALL: &str = "*";

pub(crate) fn perform(state: &mut AppState, action: Action, now: Instant) -> Vec<Effect> {
    // Results take their panel back from the activity monitor and the container list.
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
    if let Some(notice) = refusals::refusal(state, action) {
        state.notice = Some(notice);
        return Vec::new();
    }
    let side = state.active;
    match action {
        Action::MeasureSizes => crate::measure::toggle(state),
        Action::Activity => crate::activity::open(state),
        Action::ToggleSystemPlugin => settings::flip(state, |prefs| &mut prefs.system_plugin),
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
        Action::ToggleDockerPlugin => settings::flip(state, |prefs| &mut prefs.docker_plugin),
        Action::ToggleDockerConsole => settings::flip(state, |prefs| &mut prefs.docker_console),
        Action::ToggleAgentCloud => settings::flip(state, |prefs| &mut prefs.agent_cloud),
        Action::ToggleAgentAskReads => settings::flip(state, |prefs| &mut prefs.agent_ask_reads),
        Action::ToggleAgentLog => settings::toggle_agent_log(state),
        Action::ToggleIndex => settings::toggle_index(state),
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
        Action::DeleteOriginals => files::confirm_delete(state),
        Action::Quit => dialogs::confirm_quit(state),
        Action::Jobs => crate::jobs::open_list(state),
        Action::ToggleShell => shell::toggle_shell(state),
        Action::MirrorRemote => crate::remote::mirror_mounted(state),
        Action::BrowseRemotes => crate::remote::browse(state),
        Action::ApprovalModeOff => vec![Effect::ApprovalModeOff],
        Action::SwitchPanel => panels::switch(state),
        Action::InsertNewline => shell::insert_newline(state),
        Action::QuickView => crate::aside::toggle(state, crate::Aside::QuickView),
        Action::Info => crate::aside::toggle(state, crate::Aside::Info),
        Action::TimeMachine => crate::backup::open(state),
        Action::Up => panels::moved(state, |panel| panel.move_by(LINE_UP)),
        Action::Down => panels::moved(state, |panel| panel.move_by(LINE_DOWN)),
        Action::PageUp => panels::moved(state, |panel| panel.page(PAGE_UP)),
        Action::PageDown => panels::moved(state, |panel| panel.page(PAGE_DOWN)),
        Action::Top => panels::moved(state, Panel::to_start),
        Action::Bottom => panels::moved(state, Panel::to_end),
        Action::Open => panels::open(state, side),
        Action::Descend => panels::step_in(state, side),
        Action::Leave => panels::step_out(state, side),
        Action::InsertName => shell::insert_names(state, side),
        Action::CopyNames => shell::copy_selection(state, side, false),
        Action::CopyPaths => shell::copy_selection(state, side, true),
        Action::InsertPath(which) => shell::insert_path(state, which),
        Action::ToggleHidden => settings::toggle_hidden(state),
        Action::OpenMenuBar => dialogs::open_menu_bar(state, side),
        Action::OpenFileMenu => dialogs::open_menu(state, FILE_MENU),
        Action::About => dialogs::show(state, Overlay::About(crate::About::default())),
        Action::Settings => dialogs::show(state, Overlay::Settings { selected: 0 }),
        Action::Plugins => dialogs::show(state, Overlay::Plugins { selected: 0 }),
        Action::ToggleGitPlugin => settings::flip(state, |prefs| &mut prefs.git_plugin),
        Action::Git(tool) => crate::plugins::run(state, tool),
        Action::PickLanguage => dialogs::pick_language(state),
        Action::PickTheme => dialogs::pick_theme(state),
        Action::ToggleMark => panels::mark_and_move(state, LINE_DOWN),
        Action::MarkUp => panels::mark_and_move(state, LINE_UP),
        Action::MarkDown => panels::mark_and_move(state, LINE_DOWN),
        Action::SelectGroup => {
            dialogs::prompt(state, tr!(select_group), tr!(files_matching), SELECT_ALL, PromptPurpose::Select)
        }
        Action::UnselectGroup => {
            dialogs::prompt(state, tr!(unselect_group), tr!(files_matching), SELECT_ALL, PromptPurpose::Unselect)
        }
        Action::InvertSelection => panels::moved(state, Panel::invert_marks),
        Action::View => files::view(state, side),
        Action::Edit => files::edit_hovered(state, crate::EditorSlot::Screen),
        Action::EditInPanel => files::edit_hovered(state, crate::EditorSlot::Panel(side)),
        Action::EditExternal => files::edit_external(state),
        Action::Copy => files::copy(state, side),
        Action::Move => files::move_selection(state, side),
        Action::Mkdir => dialogs::prompt(state, tr!(make_directory), tr!(name), "", PromptPurpose::Mkdir),
        Action::Delete => files::delete(state),
        Action::Compress => files::compress(state),
        Action::ExtractHere => files::extract(state, side, now),
        Action::ExtractToOther => files::extract(state, side.other(), now),
        Action::SwapPanels => panels::swap(state),
        Action::EqualizePanels => panels::set_split(state, EVEN_SPLIT, now),
        Action::NudgeSplit(columns) => panels::nudge_split(state, columns, now),
        Action::ToggleStatusLine => settings::preference(state, |prefs| prefs.status_line = !prefs.status_line),
        Action::ToggleMenuBar => settings::preference(state, |prefs| prefs.menu_bar = !prefs.menu_bar),
        Action::ToggleRestoreTerminal => {
            settings::preference(state, |prefs| prefs.restore_terminal = !prefs.restore_terminal)
        }
        Action::ToggleScreensaver => settings::toggle_screensaver(state, now),
        Action::Find if state.active_panel().is_results() => crate::results::filter_prompt(state),
        Action::Find => crate::find::open(state),
        Action::ToggleReturnToPanels => settings::toggle_after_command(state),
        Action::SetListing(target, mode) => panels::set_listing(state, target, mode),
        Action::SetSort(target, key) => panels::set_sort(state, target, key),
        Action::ToggleReverse(target) => panels::toggle_reverse(state, target),
        Action::Rescan(target) => vec![state.reload(target)],
        Action::Reload => vec![state.reload(state.active)],
    }
}
