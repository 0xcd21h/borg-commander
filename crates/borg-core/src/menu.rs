use crate::{Action, AfterCommand, AppState, ListingMode, Side, SortKey, ops::ArchiveFormat};

pub const FILE_MENU: usize = 1;

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
    vec![
        panel_menu("Left", state, Side::Left),
        file_menu(state),
        command_menu(state),
        options_menu(state),
        panel_menu("Right", state, Side::Right),
    ]
}

fn panel_menu(title: &'static str, state: &AppState, side: Side) -> Menu {
    let panel = state.panel(side);
    let sort = panel.sort();
    let sorted = |key: SortKey| sort.key == key;
    Menu {
        title,
        entries: vec![
            check("Full listing", "", Action::SetListing(side, ListingMode::Full), panel.mode() == ListingMode::Full),
            check(
                "Brief listing",
                "",
                Action::SetListing(side, ListingMode::Brief),
                panel.mode() == ListingMode::Brief,
            ),
            MenuEntry::Separator,
            check("Sort by name", "", Action::SetSort(side, SortKey::Name), sorted(SortKey::Name)),
            check("Sort by extension", "", Action::SetSort(side, SortKey::Extension), sorted(SortKey::Extension)),
            check("Sort by size", "", Action::SetSort(side, SortKey::Size), sorted(SortKey::Size)),
            check("Sort by modified", "", Action::SetSort(side, SortKey::Modified), sorted(SortKey::Modified)),
            check("Reverse order", "", Action::ToggleReverse(side), sort.reverse),
            MenuEntry::Separator,
            check("Show hidden files", "M-.", Action::ToggleHidden, state.settings.preferences.show_hidden),
            item("Rescan", "", Action::Rescan(side)),
        ],
    }
}

fn file_menu(state: &AppState) -> Menu {
    let panel = state.active_panel();
    let has_selection = !panel.selection().is_empty();
    let hovered_file = panel.hovered_entry().is_some_and(|entry| !entry.is_dir());
    let archive = panel.hovered_entry().is_some_and(|entry| ArchiveFormat::detect(&entry.name).is_some());
    Menu {
        title: "File",
        entries: vec![
            when(item("View", "F3", Action::View), hovered_file),
            when(item("Edit", "F4", Action::Edit), hovered_file),
            when(item("Copy", "F5", Action::Copy), has_selection),
            when(item("Move", "F6", Action::Move), has_selection),
            item("Make directory", "F7", Action::Mkdir),
            when(item("Delete", "F8", Action::Delete), has_selection),
            MenuEntry::Separator,
            item("Select group", "+", Action::SelectGroup),
            item("Unselect group", "-", Action::UnselectGroup),
            item("Invert selection", "*", Action::InvertSelection),
            MenuEntry::Separator,
            when(item("Compress…", "", Action::Compress), has_selection),
            when(item("Extract here", "", Action::ExtractHere), archive),
            when(item("Extract to other panel", "", Action::ExtractToOther), archive),
            MenuEntry::Separator,
            item("Quit", "F10", Action::Quit),
        ],
    }
}

fn command_menu(state: &AppState) -> Menu {
    Menu {
        title: "Command",
        entries: vec![
            item("Find in folder", "C-s", Action::Find),
            check("Quick view", "C-q", Action::QuickView, state.aside == Some(crate::Aside::QuickView)),
            check("Info", "C-l", Action::Info, state.aside == Some(crate::Aside::Info)),
            when(item("Jobs…", "M-j", Action::Jobs), !state.jobs.is_empty()),
            item("Swap panels", "C-u", Action::SwapPanels),
            item("Equal panel widths", "", Action::EqualizePanels),
            item("Widen left panel", "M-S-→", Action::NudgeSplit(crate::keymap::SPLIT_STEP)),
            item("Widen right panel", "M-S-←", Action::NudgeSplit(-crate::keymap::SPLIT_STEP)),
            MenuEntry::Separator,
            item("Show shell", "C-o", Action::ToggleShell),
            MenuEntry::Separator,
            item("About borg", "F1", Action::About),
        ],
    }
}

fn options_menu(state: &AppState) -> Menu {
    let prefs = &state.settings.preferences;
    Menu {
        title: "Options",
        entries: vec![
            item("Theme…", "", Action::PickTheme),
            MenuEntry::Separator,
            check("File icons (Nerd Font)", "", Action::ToggleIcons, prefs.icons),
            check("Pixel-drawn panels (Kitty, Ghostty)", "", Action::TogglePixelChrome, prefs.pixel_chrome),
            check("Status line", "", Action::ToggleStatusLine, prefs.status_line),
            check("Menu bar always visible", "", Action::ToggleMenuBar, prefs.menu_bar),
            check("Animations", "", Action::ToggleAnimations, prefs.animations),
            check(
                "Return to panels after command",
                "",
                Action::ToggleReturnToPanels,
                prefs.after_command == AfterCommand::ReturnToPanels,
            ),
            check("Restore shell output on start", "", Action::ToggleRestoreTerminal, prefs.restore_terminal),
            MenuEntry::Separator,
            check("Startup animation", "", Action::ToggleSplash, prefs.splash),
            check("Screensaver when idle", "", Action::ToggleScreensaver, prefs.screensaver),
            check("Delight (easter eggs)", "", Action::ToggleDelight, prefs.delight),
        ],
    }
}
