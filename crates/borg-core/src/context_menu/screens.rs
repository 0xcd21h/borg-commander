//! The context menus of the full-width screens: a process, an open file, a container, a
//! network row, an alert and a startup item. A screen that is taking typed text has none.

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{ContextCommand, copy, press};
use crate::menu::when;
use crate::{
    Action, Activity, ActivityPage, Alerts, AppState, Menu, MenuEntry, Neighbor, NeighborEntry, NeighborRow, Network,
    NetworkPage, Origin, Presence, StartupItems,
};

const SIGNAL_KEY: char = 'k';
const REGIONS_KEY: char = '1';
const FILES_KEY: char = '2';
const TRAFFIC_KEY: char = 'n';
const RESUME_KEY: char = 'c';
const ALLOW_KEY: char = 'a';
const SCAN_KEY: char = 's';
const FORGET_KEY: u8 = 8;
const PORTS_KEY: char = 'p';
const SCANS_KEY: char = 'o';
const DISCOVER_KEY: char = 'd';

fn alt(letter: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(letter), KeyModifiers::ALT)
}

/// The selected process, or the selected open file of the process being inspected.
pub(super) fn activity(activity: &Activity) -> Option<Menu<ContextCommand>> {
    if activity.filtering || activity.search_form.is_some() {
        return None;
    }
    match activity.list {
        ActivityPage::Processes => process(activity),
        ActivityPage::Files => open_file(activity),
        _ => None,
    }
}

fn process(activity: &Activity) -> Option<Menu<ContextCommand>> {
    let process = activity.process()?;
    let pid = process.key.pid;
    let program = process.executable.as_ref().map(|path| path.to_string_lossy().into_owned());
    let own = activity.sample.own == Some(pid);
    Some(Menu {
        title: format!("{} {pid}", process.name),
        entries: vec![
            copy(tr!(context_copy_pid), pid.to_string()),
            copy(tr!(context_copy_name), process.name.as_str()),
            when(copy(tr!(context_copy_program), program.clone().unwrap_or_default()), program.is_some()),
            MenuEntry::Separator,
            when(press(tr!(context_signal), "K", KeyCode::Char(SIGNAL_KEY)), !own),
            press(tr!(context_open_files), "2", KeyCode::Char(FILES_KEY)),
            press(tr!(context_memory_map), "1", KeyCode::Char(REGIONS_KEY)),
            press(tr!(context_traffic), "M-n", alt(TRAFFIC_KEY)),
        ],
    })
}

fn open_file(activity: &Activity) -> Option<Menu<ContextCommand>> {
    let file = activity.files.get(activity.file_cursor)?;
    Some(Menu {
        title: file.name.clone(),
        entries: vec![
            copy(tr!(context_copy_path), file.name.as_str()),
            MenuEntry::Separator,
            when(press(tr!(view), "F3", KeyCode::F(3)), activity.previewable_file().is_some()),
        ],
    })
}

/// The selected container, with what the Docker menu does to it.
pub(super) fn container(state: &AppState) -> Option<Menu<ContextCommand>> {
    use borg_docker::Action as Docker;
    let container = state.docker.container()?;
    let act = |label, hint, action| {
        let docker = Action::DockerAct(action);
        when(
            super::entry(label, String::from(hint), ContextCommand::Perform(docker)),
            crate::docker::can(state, action),
        )
    };
    let pause = crate::docker::pause_action(state);
    let pause_label = if pause == Docker::Unpause { tr!(docker_menu_resume) } else { tr!(docker_menu_pause) };
    Some(Menu {
        title: container.name.clone(),
        entries: vec![
            copy(tr!(context_copy_id), container.id.as_str()),
            copy(tr!(context_copy_name), container.name.as_str()),
            MenuEntry::Separator,
            act(tr!(docker_menu_start), "S", Docker::Start),
            act(tr!(docker_menu_stop), "X", Docker::Stop),
            act(tr!(docker_menu_restart), "R", Docker::Restart),
            act(pause_label, "P", pause),
            MenuEntry::Separator,
            when(docker_key(tr!(docker_menu_files), "F", Action::DockerFiles), crate::docker::can_browse(state)),
            when(docker_key(tr!(docker_menu_shell), "E", Action::DockerShell), crate::docker::can_open_shell(state)),
        ],
    })
}

fn docker_key(label: &'static str, hint: &'static str, action: Action) -> MenuEntry<ContextCommand> {
    super::entry(label, hint.into(), ContextCommand::Perform(action))
}

/// What the selected row of the network screen is reached by, and its process. `scanned` says
/// whether the selected neighbor has port scans to show.
pub(super) fn network(network: &Network, scanned: bool) -> Option<Menu<ContextCommand>> {
    if network.typing() || network.viewer.is_some() {
        return None;
    }
    if let Some((neighbor, row)) = network.neighbor() {
        return Some(neighbor_menu(neighbor, row, scanned));
    }
    if let Some(entry) = network.entry() {
        return heading_menu(network, entry);
    }
    let text = crate::network::copied_text(network)?;
    let label = match network.page {
        NetworkPage::Ports | NetworkPage::Interfaces | NetworkPage::Neighbors => tr!(context_copy_address),
        NetworkPage::Processes => tr!(context_copy_pid),
        NetworkPage::Connections => tr!(context_copy_remote),
    };
    let mut entries = vec![copy(label, text.as_str())];
    if let Some(pid) = network.selected_pid() {
        if network.page != NetworkPage::Processes {
            entries.push(copy(tr!(context_copy_pid), pid.to_string()));
        }
        entries.extend([
            MenuEntry::Separator,
            press(tr!(context_show_in_activity), "Enter", KeyCode::Enter),
            press(tr!(context_signal), "K", KeyCode::Char(SIGNAL_KEY)),
        ]);
    }
    Some(Menu { title: text, entries })
}

fn neighbor_menu(neighbor: &Neighbor, row: NeighborRow, scanned: bool) -> Menu<ContextCommand> {
    let address = neighbor.address.to_string();
    let mac = neighbor.mac.map(|mac| mac.to_string());
    let offline = matches!(row.presence, Presence::Offline(_));
    let forget = offline.then(|| press(tr!(context_forget_host), "F8", KeyCode::F(FORGET_KEY)));
    let save = KeyEvent::new(KeyCode::Char(crate::port_scan::SAVE_KEY), KeyModifiers::CONTROL);
    Menu {
        title: address.clone(),
        entries: [
            copy(tr!(context_copy_ip), address),
            when(copy(tr!(context_copy_mac), mac.clone().unwrap_or_default()), mac.is_some()),
            MenuEntry::Separator,
            press(tr!(context_scan_ports), "P", KeyCode::Char(PORTS_KEY)),
            when(press(tr!(context_port_scans), "O", KeyCode::Char(SCANS_KEY)), scanned),
            press(tr!(context_scan_network), "S", KeyCode::Char(SCAN_KEY)),
            press(tr!(context_discover), "D", KeyCode::Char(DISCOVER_KEY)),
            press(tr!(context_save_report), "C-s", save),
        ]
        .into_iter()
        .chain(forget)
        .collect(),
    }
}

/// A heading of the Neighbors page: what it names to copy, and the scans that start from it.
fn heading_menu(network: &Network, entry: NeighborEntry) -> Option<Menu<ContextCommand>> {
    let text = crate::network::copied_text(network)?;
    let routed = match entry {
        NeighborEntry::Network(index) => network.routed.get(index),
        NeighborEntry::Interface(_) | NeighborEntry::Internet(_) | NeighborEntry::Host(_) => None,
    };
    let label = if routed.is_some() { tr!(context_copy_network) } else { tr!(context_copy_address) };
    let forgettable = routed.is_some_and(|routed| routed.remembered.is_some() && routed.origin != Origin::Table);
    let save = KeyEvent::new(KeyCode::Char(crate::port_scan::SAVE_KEY), KeyModifiers::CONTROL);
    let entries = vec![
        copy(label, text.as_str()),
        MenuEntry::Separator,
        press(tr!(context_scan_network), "S", KeyCode::Char(SCAN_KEY)),
        press(tr!(context_discover), "D", KeyCode::Char(DISCOVER_KEY)),
        press(tr!(context_save_report), "C-s", save),
    ]
    .into_iter()
    .chain(forgettable.then(|| press(tr!(context_forget_network), "F8", KeyCode::F(FORGET_KEY))))
    .collect();
    Some(Menu { title: text, entries })
}

/// The selected alert's process and what can be done about it.
pub(super) fn alert(alerts: &Alerts) -> Option<Menu<ContextCommand>> {
    if alerts.typing() {
        return None;
    }
    let alert = alerts.selected()?;
    let process = &alert.process;
    let pid = process.key.pid;
    let program = process.program.as_ref().map(|path| path.to_string_lossy().into_owned());
    let command = process.command_line();
    Some(Menu {
        title: format!("{} {pid}", process.name),
        entries: vec![
            copy(tr!(context_copy_pid), pid.to_string()),
            when(copy(tr!(context_copy_program), program.clone().unwrap_or_default()), program.is_some()),
            when(copy(tr!(context_copy_command), command.as_str()), !command.is_empty()),
            MenuEntry::Separator,
            press(tr!(context_show_in_activity), "Enter", KeyCode::Enter),
            when(press(tr!(context_resume), "C", KeyCode::Char(RESUME_KEY)), alert.resumable()),
            press(tr!(context_kill), "K", KeyCode::Char(SIGNAL_KEY)),
            press(tr!(context_allow), "A", KeyCode::Char(ALLOW_KEY)),
        ],
    })
}

/// The selected startup item's file, the program it runs, and the keys that open them.
pub(super) fn startup_item(startup: &StartupItems) -> Option<Menu<ContextCommand>> {
    if startup.typing() {
        return None;
    }
    let item = startup.selected()?;
    let program = item.resolved.as_ref().map(|path| path.to_string_lossy().into_owned());
    let runs = &item.found.runs;
    let title = item.file.file_name().unwrap_or(item.file.as_os_str()).to_string_lossy().into_owned();
    Some(Menu {
        title,
        entries: vec![
            copy(tr!(context_copy_path), item.file.to_string_lossy()),
            when(copy(tr!(context_copy_program), program.clone().unwrap_or_default()), program.is_some()),
            when(copy(tr!(context_copy_command), runs.as_str()), !runs.is_empty()),
            MenuEntry::Separator,
            press(tr!(context_show_in_panel), "Enter", KeyCode::Enter),
            press(tr!(view), "F3", KeyCode::F(3)),
            press(tr!(edit), "F4", KeyCode::F(4)),
        ],
    })
}
