mod actions;
mod activity;
mod agent;
mod anim;
mod appearance;
mod aside;
mod backup;
mod busy;
mod clipboard;
mod command;
mod contents;
mod delight;
mod discovery;
mod docker;
mod editor;
mod exclude;
mod exclusions;
mod find;
mod find_bar;
mod game2048;
mod history;
mod input;
mod jobs;
mod keymap;
mod layout;
mod measure;
mod menu;
mod motion;
mod msg;
mod ops;
mod overlays;
mod panel;
mod pattern;
mod plugins;
mod pointer;
mod query;
mod remote;
mod results;
mod settings;
mod state;
mod update;
mod viewer;

#[cfg(test)]
mod update_tests;

pub use agent::{
    Agent, AgentEvent, Approval, Ask, AskKind, CONNECT_CLOUD, CONNECT_KEEP, CONNECT_START, Change, Connect,
    ConnectInfo, DiffLine, approval_layout, approval_rows, connect_commands, host_console_ready, log_layout, log_rows,
    pair_command, shell_quote, typing_refusal,
};
pub use anim::{Easing, Fade};
pub use appearance::{Appearance, Picker, ThemeChoice};
pub use aside::{Aside, Facts, Nature, Peek, Peeked, land_on_match};
pub use backup::{
    Backup, BackupChange, BackupEvent, BackupItem, BackupList, BackupListing, BackupPanel, BackupRequest, BackupTab,
    BackupWindow, Exclusion, list_rows as backup_list_rows, name_of as backup_name, rank as backup_rank,
    window_height as backup_window_height,
};
pub use busy::{Busy, SPINNER_DELAY, Task};
pub use clipboard::Clipboard;
pub use contents::{CONTENTS_SIZE_LIMIT, LineMatch, Scan, scan as scan_contents};
pub use delight::{About, RAIN_DURATION, UNLOCK_WORD};
pub use discovery::{
    CONTENTS_FIELD, CollectionPicker, FILES_FIELD, HIDDEN_FIELD, KIND_FIELD, MINIMUM_FIELD, PATTERN_FIELD, ROOT_FIELD,
    SKIP_FIELD, SearchForm, SearchKind, SearchOptions,
};
pub use docker::{
    ContainerShell, Docker, DockerData, DockerGroup, DockerRequest, DockerRow, FileOp, FileWork, Mount, ShellRequest,
    Usage, Working, container_shell_busy, shell_rect,
};
pub use editor::{
    EDITOR_CHROME, Editor, EditorDialog, EditorFile, EditorPrompt, EditorSlot, FileStamp, OpenError, SaveError,
    dialog as editor_dialog, focused as focused_editor, status as editor_status,
};
pub use exclude::{ARTIFACTS, CACHEDIR_SIGNATURE, CACHEDIR_TAG, Marker, is_artifact};
pub use exclusions::{EXCLUSION_SWITCHES, ExclusionEditor};
pub use find::{Find, Query, summary as find_summary};
pub use find_bar::{Direction, FindBar, FindNote, summary as find_bar_summary};
pub use game2048::{Game2048, GameDirection, GameStatus, SavedGame2048};
pub use history::{HISTORY_LIMIT, PastSearch, SearchHistory};
pub use input::LineInput;
pub use jobs::{Job, JobCommand, JobKind, JobList, JobState, Question};
pub use keymap::{Action, Binding, KEYMAP, PathOf, Scope};
pub use layout::{Chrome, EVEN_SPLIT, Layout};
pub use menu::{Menu, MenuEntry, MenuItem, checkbox_items, menus, settings_items};
pub use motion::{Pace, PanelMotion, empty_glitch};
pub use msg::{Effect, Msg};
pub use panel::{Listing, ListingMode, Panel, RemoteMount, Row, Seek, Sort, SortKey, order};
pub use pattern::{Pattern, PatternError, composed};
pub use plugins::{GitPanel, Plugins};
pub use pointer::Pointer;
pub use query::{Candidate, Problem as QueryProblem, QueryError, SearchQuery};
pub use settings::{
    AMBIENT_FRAME, AfterCommand, DEFAULT_SCREENSAVER_AFTER, ICON_SPACING, Motion, Preferences, SearchSettings, Settings,
};
pub use state::{
    AppState, Confirm, ConfirmPurpose, Console, INTRO, Leaving, MenuCursor, Operation, Overlay, PanelSetup, Prompt,
    PromptPurpose, Saver, ShellStatus, ShellView, Side, Startup, View,
};
pub use update::update;
pub use viewer::{Content, FoundInViewer, Image, Viewer, find_in, viewer_size};

pub use remote::{RemoteBrowser, RemoteChoice, RemoteClient, RemoteHostInfo, RemoteHostNames, RemoteNames};
pub use remote::{mount as mount_remote, unmount as unmount_remote};
pub use results::{
    COLLECTION_LIMIT, Collection, FirstMatch, RESULT_LIMIT, ResultGroup, ResultRow, ResultSource, Results,
    SearchProgress, SearchRequest, selected_paths,
};

pub use activity::{
    Activity, ActivityData, ActivityPage, ActivityRequest, ActivitySort, HISTORY, MemoryHistory, MemoryProtection,
    MemoryRegion, MemorySnapshot, PROCESS_LIMIT, ProcessInfo, ProcessKey, SNAPSHOT_LIMIT, SystemSample,
};

pub use activity::{CENSUS_LEGEND, Indicator, MemoryCensus, PageClass, census_grid};
pub use activity::{FILE_PREVIEW_TOP, FINDING_NOTE, Finding, FindingKind, IntegrityReport, Severity, chrome_rows};
pub use activity::{MemoryGroup, MemoryTotals, ProcessFile, ProcessFiles, RegionItem, RegionRow, SnapshotRow};
pub use activity::{MemoryHit, MemorySearchForm, MemorySearchMode, MemorySearchReport, StringsTally};
pub use activity::{STRING_PLACES, junk_string, rank_strings, string_score};

pub use measure::SizeScan;
