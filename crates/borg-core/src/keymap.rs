use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{ListingMode, Side, SortKey, View};

pub const SPLIT_STEP: i16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Activity,
    ToggleSystemPlugin,
    Search,
    AnalyzeSpace,
    FolderTree,
    CalculateSizes,
    MeasureSizes,
    RefreshSizes,
    ToggleTreeNode,
    Collect,
    OpenCollection,
    FilterResults,
    GroupResults(crate::ResultGroup),
    ToggleExpanded,
    SetExpanded(bool),
    BackResults,
    RevealOriginal,
    RemoveReferences,
    DeleteOriginals,
    CancelSearch,
    Quit,
    ToggleShell,
    SwitchPanel,
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
    Open,
    Descend,
    Leave,
    InsertName,
    InsertNewline,
    QuickView,
    Info,
    InsertPath(PathOf),
    ToggleHidden,
    OpenMenuBar,
    About,
    OpenFileMenu,
    PickTheme,
    Settings,
    Plugins,
    ToggleGitPlugin,
    Git(borg_git::Tool),
    ToggleMark,
    MarkUp,
    MarkDown,
    SelectGroup,
    UnselectGroup,
    InvertSelection,
    View,
    Edit,
    Copy,
    Move,
    Mkdir,
    Delete,
    Compress,
    ExtractHere,
    ExtractToOther,
    SwapPanels,
    EqualizePanels,
    NudgeSplit(i16),
    ToggleStatusLine,
    ToggleMenuBar,
    ToggleRestoreTerminal,
    ToggleScreensaver,
    Jobs,
    Find,
    ToggleReturnToPanels,
    SetListing(Side, ListingMode),
    SetSort(Side, SortKey),
    ToggleReverse(Side),
    Rescan(Side),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Anywhere,
    Prompt,
    Panels,
    PanelsEmptyLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    pub view: View,
    pub line_empty: bool,
    pub at_prompt: bool,
}

impl Scope {
    fn applies(self, context: Context) -> bool {
        match self {
            Self::Anywhere => true,
            Self::Prompt => context.at_prompt,
            Self::Panels => context.view == View::Panels,
            Self::PanelsEmptyLine => context.view == View::Panels && context.line_empty,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub action: Action,
    pub scope: Scope,
    pub label: Option<&'static str>,
}

const fn bind(code: KeyCode, modifiers: KeyModifiers, action: Action, scope: Scope) -> Binding {
    Binding { code, modifiers, action, scope, label: None }
}

const fn labeled(binding: Binding, label: &'static str) -> Binding {
    Binding { label: Some(label), ..binding }
}

const NONE: KeyModifiers = KeyModifiers::NONE;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;
const ALT: KeyModifiers = KeyModifiers::ALT;
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;
const ALT_SHIFT: KeyModifiers = KeyModifiers::ALT.union(KeyModifiers::SHIFT);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathOf {
    Active,
    Other,
}

pub const KEYMAP: &[Binding] = &[
    bind(KeyCode::Char('i'), ALT, Action::MeasureSizes, Scope::PanelsEmptyLine),
    bind(KeyCode::Char('m'), ALT, Action::Activity, Scope::Panels),
    bind(KeyCode::Char('D'), ALT, Action::FolderTree, Scope::Panels),
    bind(KeyCode::Char('D'), ALT_SHIFT, Action::FolderTree, Scope::Panels),
    bind(KeyCode::Char('d'), ALT_SHIFT, Action::FolderTree, Scope::Panels),
    bind(KeyCode::Char('s'), ALT, Action::CalculateSizes, Scope::Panels),
    bind(KeyCode::Char('d'), ALT, Action::AnalyzeSpace, Scope::Panels),
    bind(KeyCode::Char('r'), ALT, Action::BackResults, Scope::Panels),
    bind(KeyCode::F(8), SHIFT, Action::DeleteOriginals, Scope::Panels),
    bind(KeyCode::Char('f'), CTRL, Action::Search, Scope::Panels),
    bind(KeyCode::Char('b'), ALT, Action::Collect, Scope::Panels),
    bind(KeyCode::Char('v'), ALT, Action::OpenCollection, Scope::Panels),
    bind(KeyCode::Char('w'), ALT, Action::ToggleExpanded, Scope::Panels),
    labeled(bind(KeyCode::F(1), NONE, Action::About, Scope::Panels), tr!(help)),
    labeled(bind(KeyCode::F(2), NONE, Action::OpenFileMenu, Scope::Panels), tr!(file)),
    labeled(bind(KeyCode::F(3), NONE, Action::View, Scope::Panels), tr!(view)),
    labeled(bind(KeyCode::F(4), NONE, Action::Edit, Scope::Panels), tr!(edit)),
    labeled(bind(KeyCode::F(5), NONE, Action::Copy, Scope::Panels), tr!(copy)),
    labeled(bind(KeyCode::F(6), NONE, Action::Move, Scope::Panels), tr!(move)),
    labeled(bind(KeyCode::F(7), NONE, Action::Mkdir, Scope::Panels), tr!(mkdir)),
    labeled(bind(KeyCode::F(8), NONE, Action::Delete, Scope::Panels), tr!(delete)),
    labeled(bind(KeyCode::F(9), NONE, Action::OpenMenuBar, Scope::Panels), tr!(menu)),
    labeled(bind(KeyCode::F(10), NONE, Action::Quit, Scope::Panels), tr!(quit)),
    labeled(bind(KeyCode::Char('o'), CTRL, Action::ToggleShell, Scope::Anywhere), tr!(shell)),
    bind(KeyCode::Tab, NONE, Action::SwitchPanel, Scope::PanelsEmptyLine),
    bind(KeyCode::Up, NONE, Action::Up, Scope::Panels),
    bind(KeyCode::Down, NONE, Action::Down, Scope::Panels),
    bind(KeyCode::PageUp, NONE, Action::PageUp, Scope::Panels),
    bind(KeyCode::PageDown, NONE, Action::PageDown, Scope::Panels),
    bind(KeyCode::Home, NONE, Action::Top, Scope::PanelsEmptyLine),
    bind(KeyCode::End, NONE, Action::Bottom, Scope::PanelsEmptyLine),
    bind(KeyCode::Enter, NONE, Action::Open, Scope::PanelsEmptyLine),
    bind(KeyCode::Right, NONE, Action::Descend, Scope::PanelsEmptyLine),
    bind(KeyCode::Left, NONE, Action::Leave, Scope::PanelsEmptyLine),
    bind(KeyCode::Enter, ALT, Action::InsertName, Scope::Panels),
    bind(KeyCode::Enter, SHIFT, Action::InsertNewline, Scope::Prompt),
    bind(KeyCode::Char('j'), CTRL, Action::InsertNewline, Scope::Prompt),
    bind(KeyCode::Char('a'), ALT, Action::InsertPath(PathOf::Active), Scope::Panels),
    bind(KeyCode::Char('A'), ALT, Action::InsertPath(PathOf::Other), Scope::Panels),
    bind(KeyCode::Char('.'), ALT, Action::ToggleHidden, Scope::PanelsEmptyLine),
    bind(KeyCode::Insert, NONE, Action::ToggleMark, Scope::Panels),
    bind(KeyCode::Char('t'), CTRL, Action::ToggleMark, Scope::PanelsEmptyLine),
    bind(KeyCode::Up, SHIFT, Action::MarkUp, Scope::Panels),
    bind(KeyCode::Down, SHIFT, Action::MarkDown, Scope::Panels),
    bind(KeyCode::Char('+'), NONE, Action::SelectGroup, Scope::PanelsEmptyLine),
    bind(KeyCode::Char('-'), NONE, Action::UnselectGroup, Scope::PanelsEmptyLine),
    bind(KeyCode::Char('*'), NONE, Action::InvertSelection, Scope::PanelsEmptyLine),
    bind(KeyCode::Char('u'), CTRL, Action::SwapPanels, Scope::PanelsEmptyLine),
    bind(KeyCode::Char('s'), CTRL, Action::Find, Scope::PanelsEmptyLine),
    bind(KeyCode::Char('q'), CTRL, Action::QuickView, Scope::PanelsEmptyLine),
    bind(KeyCode::Char('l'), CTRL, Action::Info, Scope::PanelsEmptyLine),
    bind(KeyCode::Char('j'), ALT, Action::Jobs, Scope::Panels),
    bind(KeyCode::Left, ALT_SHIFT, Action::NudgeSplit(-SPLIT_STEP), Scope::Panels),
    bind(KeyCode::Right, ALT_SHIFT, Action::NudgeSplit(SPLIT_STEP), Scope::Panels),
];

pub fn lookup(key: &KeyEvent, context: Context) -> Option<Action> {
    let modifiers = match key.code {
        KeyCode::Char(_) => key.modifiers - KeyModifiers::SHIFT,
        _ => key.modifiers,
    };
    KEYMAP.iter().find(|b| b.code == key.code && b.modifiers == modifiers && b.scope.applies(context)).map(|b| b.action)
}
