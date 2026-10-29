use borg_i18n::tr;
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};

use borg_vfs::Entry;

use crate::{Action, AppState, Effect, LineInput, Overlay, Panel, Prompt, PromptPurpose, Side};

pub const RESULT_LIMIT: usize = 20_000;
pub const COLLECTION_LIMIT: usize = 64;
const COLLECTION_NAME_LIMIT: usize = 80;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ResultGroup {
    #[default]
    None,
    Folder,
    Type,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchProgress {
    pub measured: Option<std::time::SystemTime>,
    pub cached: bool,
    pub visited: usize,
    pub errors: usize,
    pub skipped: usize,
    /// Generated folders, ignored files and configured globs the search left out.
    pub excluded: usize,
    /// When the index that answered was built; `None` for a live search.
    pub indexed: Option<std::time::SystemTime>,
    pub limited: bool,
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    pub root: PathBuf,
    pub query: String,
    pub hidden: bool,
    pub options: crate::SearchOptions,
    /// What the search leaves out; `None` searches everything.
    pub skip: Option<crate::SearchSettings>,
}

/// A name search for `query` below `root`, after the query's own `hidden:` and `excluded:`
/// switches. Size scans never skip anything, so their totals stay true.
pub(crate) fn name_search(
    state: &AppState,
    root: PathBuf,
    query: &str,
    hidden: bool,
    skip: bool,
    options: crate::SearchOptions,
) -> Result<SearchRequest, crate::QueryError> {
    let parsed = crate::SearchQuery::parse(query)?;
    let sizes = matches!(options.kind, crate::SearchKind::FolderTotals | crate::SearchKind::FolderTree);
    let skip = parsed.excluded.map_or(skip, |excluded| !excluded) && !sizes;
    Ok(SearchRequest {
        root,
        query: query.into(),
        hidden: parsed.hidden.unwrap_or(hidden),
        options,
        skip: skip.then(|| state.settings.search.clone()),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultSource {
    Collection(String),
    Search { request: SearchRequest, progress: SearchProgress },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultRow {
    Entry(usize),
    Group { label: String, count: usize },
}

#[derive(Debug, Clone)]
pub struct Results {
    pub tree: Option<TreeState>,
    pub expanded: bool,
    pub source: ResultSource,
    pub filter: String,
    pub group: ResultGroup,
    pub rows: Vec<ResultRow>,
    pub collapsed: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct TreeState {
    pub root: PathBuf,
    pub open: HashSet<PathBuf>,
    pub branches: HashSet<PathBuf>,
    pub loaded: HashSet<PathBuf>,
    pub pending: HashSet<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    pub name: String,
    pub entries: Vec<Entry>,
}

impl Results {
    pub fn new(source: ResultSource) -> Self {
        let tree = match &source {
            ResultSource::Search { request, .. } if request.options.kind == crate::SearchKind::FolderTree => {
                Some(TreeState {
                    root: request.root.clone(),
                    open: HashSet::from([request.root.clone()]),
                    branches: HashSet::new(),
                    loaded: HashSet::from([request.root.clone()]),
                    pending: HashSet::new(),
                })
            }
            _ => None,
        };
        Self {
            tree,
            expanded: false,
            source,
            filter: String::new(),
            group: ResultGroup::None,
            rows: Vec::new(),
            collapsed: HashSet::new(),
        }
    }

    pub fn title(&self) -> String {
        match &self.source {
            ResultSource::Search { request, .. } if self.tree.is_some() => {
                tr!(folder_tree_title, root = request.root.display())
            }
            ResultSource::Collection(name) => tr!(collection_title, name = name),
            ResultSource::Search { request, .. } if request.options.kind == crate::SearchKind::FolderTotals => {
                tr!(folder_sizes, request.query)
            }
            ResultSource::Search { request, .. } => tr!(search_title, request.query),
        }
    }

    pub fn matches(&self, entry: &Entry) -> bool {
        if self.filter.is_empty() {
            return true;
        }
        let text = format!("{} {}", entry.name, entry.identity().display());
        if self.filter.chars().any(char::is_uppercase) {
            text.contains(&self.filter)
        } else {
            text.to_lowercase().contains(&self.filter.to_lowercase())
        }
    }

    pub fn group_label(&self, entry: &Entry) -> String {
        match self.group {
            ResultGroup::None => String::new(),
            ResultGroup::Folder => entry.identity().parent().map_or_else(String::new, |p| p.display().to_string()),
            ResultGroup::Type if entry.is_dir() => tr!(folders).into(),
            ResultGroup::Type => entry
                .identity()
                .extension()
                .map_or_else(|| tr!(no_extension).into(), |ext| ext.to_string_lossy().to_lowercase()),
        }
    }

    pub fn rebuild(&mut self, entries: &[Entry]) {
        self.rows.clear();
        if let Some(tree) = self.tree.as_mut() {
            tree.branches = entries
                .iter()
                .filter(|e| e.identity() != tree.root)
                .filter_map(|e| e.identity().parent().map(std::path::Path::to_path_buf))
                .collect();
            tree.branches.extend(
                entries
                    .iter()
                    .filter(|e| e.usage.as_ref().is_some_and(|u| u.folders > 0))
                    .map(|e| e.identity().to_path_buf()),
            );
            self.rebuild_tree(entries);
            return;
        }
        if self.group == ResultGroup::None {
            self.rows.extend(
                entries
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| self.matches(entry))
                    .map(|(index, _)| ResultRow::Entry(index))
                    .collect::<Vec<_>>(),
            );
            return;
        }
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, entry) in entries.iter().enumerate().filter(|(_, entry)| self.matches(entry)) {
            groups.entry(self.group_label(entry)).or_default().push(index);
        }
        for (label, indices) in groups {
            let collapsed = self.collapsed.contains(&label);
            self.rows.push(ResultRow::Group { label, count: indices.len() });
            if !collapsed {
                self.rows.extend(indices.into_iter().map(ResultRow::Entry));
            }
        }
    }

    fn rebuild_tree(&mut self, entries: &[Entry]) {
        let Some(tree) = &self.tree else { return };
        let mut children: BTreeMap<&std::path::Path, Vec<usize>> = BTreeMap::new();
        let mut root = None;
        for (index, entry) in entries.iter().enumerate() {
            if entry.identity() == tree.root {
                root = Some(index);
            } else if entry.identity().starts_with(&tree.root)
                && let Some(parent) = entry.identity().parent()
            {
                children.entry(parent).or_default().push(index);
            }
        }
        let Some(root) = root else { return };
        let mut included = HashSet::new();
        let mut filtering = !self.filter.is_empty();
        if let ResultSource::Search { request, .. } = &self.source {
            filtering |= request.query != "*" || request.options.minimum > 0 || request.options.minimum_files > 0;
            if filtering {
                let query = crate::Query::parse(&request.query);
                for entry in entries.iter().filter(|entry| {
                    self.matches(entry)
                        && query.accepts(entry)
                        && entry.size >= request.options.minimum
                        && entry.usage.as_ref().is_some_and(|u| u.files >= request.options.minimum_files)
                }) {
                    let mut path = Some(entry.identity());
                    while let Some(current) = path.filter(|p| p.starts_with(&tree.root)) {
                        included.insert(current);
                        if current == tree.root {
                            break;
                        }
                        path = current.parent();
                    }
                }
            }
        }
        let mut stack = vec![root];
        while let Some(index) = stack.pop() {
            let path = entries[index].identity();
            if filtering && path != tree.root && !included.contains(path) {
                continue;
            }
            self.rows.push(ResultRow::Entry(index));
            if tree.open.contains(path)
                && !filtering
                && let Some(usage) = &entries[index].usage
                && usage.direct_files > 0
            {
                self.rows.push(ResultRow::Group {
                    label: tr!(tree_direct_files, bytes = usage.direct_bytes),
                    count: usage.direct_files as usize,
                });
            }

            if (tree.open.contains(path) || filtering)
                && let Some(descendants) = children.get(path)
            {
                stack.extend(descendants.iter().rev().copied());
            }
        }
    }
}

pub(crate) fn prompt(
    state: &mut AppState,
    title: &'static str,
    label: String,
    value: String,
    purpose: PromptPurpose,
) -> Vec<Effect> {
    state.overlay =
        Some(Overlay::Prompt(Prompt { title, label, input: LineInput::new(value), purpose, option_focused: false }));
    Vec::new()
}

pub(crate) fn search_prompt(state: &mut AppState) -> Vec<Effect> {
    state.overlay = Some(Overlay::Search(crate::SearchForm::new(state, false)));
    Vec::new()
}

pub(crate) fn start_search(state: &mut AppState, request: SearchRequest) -> Vec<Effect> {
    let side = state.active;
    let generation = state.next_generation();
    remember(state, side);
    state.panel_mut(side).open_results(
        ResultSource::Search { request: request.clone(), progress: SearchProgress::default() },
        Vec::new(),
        generation,
    );
    state.panel_mut(side).begin_search(generation);
    if matches!(request.options.kind, crate::SearchKind::FolderTotals | crate::SearchKind::FolderTree) {
        state.panel_mut(side).set_sort(crate::Sort { key: crate::SortKey::Size, reverse: true });
    }
    state.refit();
    vec![Effect::Search { side, generation, request }]
}

fn remember(state: &mut AppState, side: Side) {
    if !state.panel(side).is_results() {
        state.result_history[side.index()] = Some(state.panel(side).clone());
    }
}

pub(crate) fn collection(state: &mut AppState, name: &str, side: Side) -> Vec<Effect> {
    let name = name.chars().filter(|c| !c.is_control()).take(COLLECTION_NAME_LIMIT).collect::<String>();
    if name.is_empty() {
        return Vec::new();
    }
    if !state.collections.iter().any(|c| c.name == name) {
        if state.collections.len() >= COLLECTION_LIMIT {
            state.notice = Some(tr!(collection_limit_reached).into());
            return Vec::new();
        }
        state.collections.push(Collection { name: name.clone(), entries: Vec::new() });
    }
    let entries = state.collections.iter().find(|c| c.name == name).map(|c| c.entries.clone()).unwrap_or_default();
    remember(state, side);
    let generation = state.next_generation();
    state.panel_mut(side).open_results(ResultSource::Collection(name), entries.clone(), generation);
    state.panel_mut(side).begin_results_refresh(generation);
    state.refit();
    vec![Effect::RefreshCollection { side, generation, entries }, Effect::SaveCollections]
}

pub(crate) fn collect(state: &mut AppState) -> Vec<Effect> {
    let side = state.active.other();
    let entries: Vec<_> = state
        .active_panel()
        .selection()
        .into_iter()
        .map(|entry| {
            let mut entry = entry.clone();
            entry.source = Some(entry.path(state.active_panel().dir()));
            entry
        })
        .collect();
    if entries.is_empty() {
        state.notice = Some(tr!(nothing_selected).into());
        return Vec::new();
    }
    let name = match state.panel(side).results.as_ref().map(|r| &r.source) {
        Some(ResultSource::Collection(name)) => name.clone(),
        _ => tr!(collection).into(),
    };
    let existing: HashSet<_> = state
        .collections
        .iter()
        .find(|c| c.name == name)
        .into_iter()
        .flat_map(|c| &c.entries)
        .map(|entry| entry.identity())
        .collect();
    let added: HashSet<_> =
        entries.iter().map(|entry| entry.identity()).filter(|path| !existing.contains(path)).collect();
    if existing.len().saturating_add(added.len()) > RESULT_LIMIT {
        state.notice = Some(tr!(a_collection_holds_up_to_references, RESULT_LIMIT = RESULT_LIMIT));
        return Vec::new();
    }
    let mut effects = if matches!(state.panel(side).results.as_ref().map(|r| &r.source), Some(ResultSource::Collection(current)) if *current == name)
    {
        Vec::new()
    } else {
        collection(state, &name, side)
    };
    if !matches!(state.panel(side).results.as_ref().map(|r| &r.source), Some(ResultSource::Collection(current)) if *current == name)
    {
        return effects;
    }
    effects.retain(|effect| !matches!(effect, Effect::RefreshCollection { .. }));
    let generation = state.next_generation();
    state.panel_mut(side).finish_results_edit(generation);
    state.panel_mut(side).append_results(entries);
    effects.push(Effect::CancelSearch(side));
    if let Some(other) = save_current(state, side) {
        effects.push(Effect::CancelSearch(other));
    }
    state.notice = Some(tr!(collected_in_tab_opens_the_other_panel, name = name));
    effects.push(Effect::SaveCollections);
    effects
}

pub(crate) fn save_current(state: &mut AppState, side: Side) -> Option<Side> {
    let Some(ResultSource::Collection(name)) = state.panel(side).results.as_ref().map(|r| &r.source) else {
        return None;
    };
    let name = name.clone();
    let entries = state.panel(side).entries().to_vec();
    if let Some(collection) = state.collections.iter_mut().find(|c| c.name == name) {
        collection.entries = entries.clone();
    }
    let other = side.other();
    if matches!(state.panel(other).results.as_ref().map(|r| &r.source), Some(ResultSource::Collection(current)) if *current == name)
    {
        let generation = state.next_generation();
        state.panel_mut(other).finish_results_edit(generation);
        state.panel_mut(other).replace_collection(entries.clone());
        return Some(other);
    }
    if state.last_results[other.index()].as_ref().is_some_and(|panel| matches!(panel.results.as_ref().map(|r| &r.source), Some(ResultSource::Collection(current)) if *current == name)) {
        let generation = state.next_generation();
        if let Some(panel) = state.last_results[other.index()].as_mut() {
            panel.finish_results_edit(generation);
            panel.replace_collection(entries);
        }
        return Some(other);
    }
    None
}

pub(crate) fn back(state: &mut AppState) -> Vec<Effect> {
    let side = state.active;
    let sizes = state.panel(side).is_size_view();
    state.settle();
    state.panel_mut(side).cache_observations();
    if state.panel(side).is_results() {
        if let Some(folder) = state.result_history[side.index()].take() {
            let results = std::mem::replace(&mut state.panels[side.index()], folder);
            state.last_results[side.index()] = Some(results);
        }
    } else if let Some(results) = state.last_results[side.index()].take() {
        let folder = std::mem::replace(&mut state.panels[side.index()], results);
        state.result_history[side.index()] = Some(folder);
    }
    state.refit();
    if sizes && !state.panel(side).is_results() { vec![state.reload(side)] } else { Vec::new() }
}

pub(crate) fn reveal(state: &mut AppState) -> Vec<Effect> {
    let panel = state.active_panel();
    let Some(entry) = panel.hovered_entry() else { return Vec::new() };
    let path = entry.path(panel.dir());
    let Some(parent) = path.parent() else { return Vec::new() };
    let side = state.active.other();
    state.aside = None;
    state.active = side;
    state.panel_mut(side).mount = None;
    vec![state.navigate(side, parent.to_path_buf(), path.file_name().map(|n| n.to_string_lossy().into_owned()))]
}

pub(crate) fn open_collection_prompt(state: &mut AppState) -> Vec<Effect> {
    state.overlay = Some(Overlay::Collections(crate::CollectionPicker {
        selected: 0,
        creating: state.collections.is_empty().then(|| LineInput::new(tr!(collection))),
    }));
    Vec::new()
}

pub(crate) fn filter_prompt(state: &mut AppState) -> Vec<Effect> {
    let value = state.active_panel().results.as_ref().map(|r| r.filter.clone()).unwrap_or_default();
    prompt(
        state,
        tr!(results_filter_results),
        tr!(name_or_source_path_empty_clears_filter).into(),
        value,
        PromptPurpose::ResultsFilter,
    )
}

pub(crate) fn perform(state: &mut AppState, action: Action) -> Vec<Effect> {
    match action {
        Action::FolderTree | Action::CalculateSizes => {
            let root = if action == Action::CalculateSizes {
                state
                    .active_panel()
                    .hovered_entry()
                    .filter(|e| e.kind == borg_vfs::EntryKind::Dir)
                    .map(|e| e.path(state.active_panel().dir()))
                    .unwrap_or_else(|| state.active_panel().dir().to_path_buf())
            } else {
                state.active_panel().dir().to_path_buf()
            };
            start_search(
                state,
                SearchRequest {
                    skip: None,
                    root,
                    query: "*".into(),
                    hidden: true,
                    options: crate::SearchOptions { kind: crate::SearchKind::FolderTree, ..Default::default() },
                },
            )
        }
        Action::RefreshSizes => vec![state.reload(state.active)],
        Action::ToggleTreeNode => {
            state.panel_mut(state.active).toggle_tree_node();
            Vec::new()
        }
        Action::Search => search_prompt(state),
        Action::AnalyzeSpace => {
            state.overlay = Some(Overlay::Search(crate::SearchForm::new(state, true)));
            Vec::new()
        }
        Action::Collect => collect(state),
        Action::OpenCollection => open_collection_prompt(state),
        Action::FilterResults => filter_prompt(state),
        Action::GroupResults(group) => {
            let side = state.active;
            state.panel_mut(side).group_results(group);
            Vec::new()
        }
        Action::ToggleExpanded => perform(state, Action::SetExpanded(!state.results_expanded())),
        Action::SetExpanded(expanded) => {
            if let Some(results) = &mut state.panels[state.active.index()].results {
                results.expanded = expanded;
                state.refit();
            }
            Vec::new()
        }
        Action::BackResults => back(state),
        Action::RevealOriginal => reveal(state),
        Action::CancelSearch => {
            let side = state.active;
            if !matches!(state.panel(side).results.as_ref().map(|r| &r.source), Some(ResultSource::Search { .. })) {
                return Vec::new();
            }
            if let Some(crate::Results { source: ResultSource::Search { progress, .. }, .. }) =
                &mut state.panel_mut(side).results
            {
                progress.cancelled = true;
            }
            let old = state.panel(side).generation();
            state.panel_mut(side).cancel_refresh();
            state.panel_mut(side).finish(old, None);
            state.panel_mut(side).settle();
            let generation = state.next_generation();
            state.panel_mut(side).finish_results_edit(generation);
            vec![Effect::CancelSearch(side)]
        }
        Action::RemoveReferences => {
            let side = state.active;
            let generation = state.next_generation();
            state.panel_mut(side).finish_results_edit(generation);
            state.panel_mut(side).remove_selected();
            let mut effects = vec![Effect::CancelSearch(side), Effect::SaveCollections];
            if let Some(other) = save_current(state, side) {
                effects.push(Effect::CancelSearch(other));
            }
            effects
        }
        _ => Vec::new(),
    }
}

pub(crate) fn progress(state: &mut AppState, side: Side, generation: u64, update: SearchProgress) {
    let Some(panel) = state.receiving_panel(side, generation) else { return };
    if let Some(Results { source: ResultSource::Search { progress, .. }, .. }) = &mut panel.results
        && !progress.cancelled
    {
        *progress = update;
    }
}

pub fn selected_paths(panel: &Panel) -> Vec<PathBuf> {
    let mut paths: Vec<_> = panel.selection().into_iter().filter(|e| !e.missing).map(|e| e.path(panel.dir())).collect();
    paths.sort();
    paths.dedup();
    let directories: HashSet<_> = panel
        .selection()
        .into_iter()
        .filter(|e| e.kind == borg_vfs::EntryKind::Dir)
        .map(|e| e.path(panel.dir()))
        .collect();
    paths.retain(|path| !path.ancestors().skip(1).any(|parent| directories.contains(parent)));
    paths
}

pub(crate) fn sync_sizes(state: &mut AppState) -> Vec<Effect> {
    let mut effects = Vec::new();
    for side in Side::ALL {
        let panel = state.panel_mut(side);
        if panel.listing() != &crate::Listing::Ready {
            continue;
        }
        let generation = panel.generation();
        if let Some(Results { tree: Some(tree), source: ResultSource::Search { request, .. }, .. }) = &mut panel.results
        {
            for parent in tree.open.clone() {
                if tree.branches.contains(&parent)
                    && !tree.loaded.contains(&parent)
                    && tree.pending.insert(parent.clone())
                {
                    effects.push(Effect::SizeChildren {
                        side,
                        generation,
                        root: request.root.clone(),
                        hidden: request.hidden,
                        parent,
                    });
                }
            }
        }
    }
    effects
}
pub(crate) fn size_children(
    state: &mut AppState,
    side: Side,
    generation: u64,
    parent: PathBuf,
    result: Result<(Vec<Entry>, bool), String>,
) {
    let Some(panel) = state.receiving_panel(side, generation) else { return };
    let Some(tree) = panel.results.as_mut().and_then(|r| r.tree.as_mut()) else { return };
    if !tree.pending.remove(&parent) {
        return;
    }
    tree.loaded.insert(parent);
    match result {
        Ok((mut entries, limited)) => {
            let cached = matches!(&panel.results, Some(Results { source: ResultSource::Search { progress, .. }, .. }) if progress.cached);
            for entry in &mut entries {
                if let Some(usage) = &mut entry.usage {
                    usage.cached = cached;
                }
            }
            panel.append_results(entries);
            if limited && let Some(Results { source: ResultSource::Search { progress, .. }, .. }) = &mut panel.results {
                progress.limited = true;
            }
        }
        Err(error) => state.notice = Some(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Appearance, ListingMode, Msg, Pointer, Settings, Sort, SortKey, Startup, ThemeChoice, update};
    use std::{path::Path, time::Instant};

    fn file(path: &str, size: u64) -> Entry {
        Entry {
            source: Some(path.into()),
            missing: false,
            usage: None,
            name: Path::new(path).file_name().unwrap().to_string_lossy().into_owned(),
            kind: borg_vfs::EntryKind::File,
            size,
            modified: None,
            executable: false,
            permissions: 0o644,
        }
    }

    fn panel() -> Panel {
        let mut panel = Panel::new("/work".into(), 10, ListingMode::Full, Sort::default());
        panel.open_results(
            ResultSource::Collection("Bundle".into()),
            vec![file("/a/config.toml", 10), file("/b/config.toml", 20), file("/a/notes.md", 30)],
            10,
        );
        panel
    }

    fn state() -> (AppState, Instant) {
        let now = Instant::now();
        let (mut state, _) = AppState::new(
            Startup::at("/work".into()),
            100,
            30,
            Settings::default(),
            Appearance::new(vec![ThemeChoice { id: "midnight".into(), name: "Midnight".into() }], 0),
            now,
        );
        state.panels[0] = panel();
        state.collections.push(Collection { name: "Bundle".into(), entries: state.panels[0].entries().to_vec() });
        (state, now)
    }

    #[test]
    fn new_searches_and_collections_open_in_their_pane_with_independent_expansion() {
        let (mut state, now) = state();
        assert!(!state.results_expanded());
        assert!(state.layout(now).panels.iter().all(|rect| !rect.is_empty()));
        perform(&mut state, Action::SetExpanded(true));
        assert!(state.results_expanded());
        state.active = Side::Right;
        collection(&mut state, "Other", Side::Right);
        assert!(!state.results_expanded());
        assert!(state.panels[0].results.as_ref().unwrap().expanded);
        assert!(state.layout(now).panels.iter().all(|rect| !rect.is_empty()));
        update(
            &mut state,
            Msg::Pointer(Pointer::ResultControl { side: Side::Left, action: Action::SetExpanded(true) }),
            now,
        );
        assert_eq!(state.active, Side::Left);
        assert!(state.results_expanded());
        start_search(
            &mut state,
            SearchRequest {
                skip: None,
                root: "/work".into(),
                query: "*".into(),
                hidden: false,
                options: Default::default(),
            },
        );
        assert!(!state.results_expanded());
        assert!(state.layout(now).panels.iter().all(|rect| !rect.is_empty()));
    }

    #[test]
    fn fold_and_resume_keep_expansion_cursor_and_marks() {
        let (mut state, now) = state();
        state.result_history[0] = Some(Panel::new("/work".into(), 10, ListingMode::Full, Sort::default()));
        state.panels[0].move_by(1);
        state.panels[0].toggle_mark();
        let selected = state.panels[0].hovered_entry().unwrap().identity().to_path_buf();
        perform(&mut state, Action::SetExpanded(true));
        update(&mut state, Msg::Pointer(Pointer::ResultControl { side: Side::Left, action: Action::BackResults }), now);
        assert!(!state.panels[0].is_results());
        back(&mut state);
        assert!(state.results_expanded());
        assert_eq!(state.panels[0].hovered_entry().unwrap().identity(), selected);
        assert_eq!(state.panels[0].marked_count(), 1);
    }

    #[test]
    fn cleanup_targets_include_a_marked_folder_once_without_dropping_similarly_named_siblings() {
        let mut panel = panel();
        let mut folder = file("/work/cache", 1);
        folder.kind = borg_vfs::EntryKind::Dir;
        panel.open_results(
            ResultSource::Collection("Cleanup".into()),
            vec![folder, file("/work/cache/nested/item", 2), file("/work/cache-other/item", 3)],
            20,
        );
        panel.mark_where(|_| true, true);
        assert_eq!(selected_paths(&panel), vec![PathBuf::from("/work/cache"), PathBuf::from("/work/cache-other/item")]);
    }

    #[test]
    fn stopping_a_search_never_discards_an_inflight_collection_refresh() {
        let (mut state, _) = state();
        let generation = state.panels[0].generation();
        state.panels[0].begin_results_refresh(generation);
        assert!(perform(&mut state, Action::CancelSearch).is_empty());
        assert_eq!(state.panels[0].entries().len(), 3);
        assert_eq!(state.panels[0].listing(), &crate::Listing::Loading);
    }

    #[test]
    fn editing_a_collection_updates_a_hidden_copy_without_resurrecting_removed_references() {
        let (mut state, _) = state();
        state.panels[1] = state.panels[0].clone();
        state.result_history[0] = Some(Panel::new("/work".into(), 10, ListingMode::Full, Sort::default()));
        back(&mut state);
        assert!(!state.panels[0].is_results());
        state.active = Side::Right;
        perform(&mut state, Action::RemoveReferences);
        assert_eq!(state.collections[0].entries.len(), 2);
        state.active = Side::Left;
        back(&mut state);
        assert_eq!(state.panels[0].entries().len(), 2);
    }

    #[test]
    fn hidden_search_keeps_its_position_marks_filter_and_receives_late_results() {
        let (mut state, now) = state();
        state.panels[0] = Panel::new("/work".into(), 10, ListingMode::Full, Sort::default());
        let effects = start_search(
            &mut state,
            SearchRequest {
                skip: None,
                root: "/work".into(),
                query: "*".into(),
                hidden: true,
                options: Default::default(),
            },
        );
        let generation = match &effects[0] {
            Effect::Search { generation, .. } => *generation,
            _ => panic!("search"),
        };
        update(
            &mut state,
            Msg::Batch { side: Side::Left, generation, entries: vec![file("/work/a", 10), file("/work/b", 20)] },
            now,
        );
        state.settle();
        state.panels[0].move_by(1);
        state.panels[0].toggle_mark();
        state.panels[0].filter_results("/work".into());
        let path = state.panels[0].hovered_entry().unwrap().identity().to_path_buf();
        update(&mut state, Msg::Key(crossterm::event::KeyCode::Esc.into()), now);
        assert!(!state.panels[0].is_results());
        update(&mut state, Msg::Batch { side: Side::Left, generation, entries: vec![file("/work/c", 30)] }, now);
        update(&mut state, Msg::Listed { side: Side::Left, generation, error: None }, now);
        assert!(back(&mut state).is_empty());
        assert_eq!(state.panels[0].entries().len(), 3);
        assert_eq!(state.panels[0].hovered_entry().unwrap().identity(), path);
        assert_eq!(state.panels[0].marked_count(), 1);
        assert_eq!(state.panels[0].results.as_ref().unwrap().filter, "/work");
        assert!(back(&mut state).is_empty());
        assert!(back(&mut state).is_empty());
        assert_eq!(state.panels[0].hovered_entry().unwrap().identity(), path);
    }

    #[test]
    fn virtual_panels_own_tab_even_when_shell_reports_a_typed_line() {
        let (mut state, now) = state();
        state.panels[1] = state.panels[0].clone();
        state.shell.mark_typed();
        let effects = update(&mut state, Msg::Key(crossterm::event::KeyCode::Tab.into()), now);
        assert_eq!(state.active, Side::Right);
        assert!(!effects.iter().any(|e| matches!(e, Effect::Write(_))));
        update(&mut state, Msg::Key(crossterm::event::KeyCode::Tab.into()), now);
        assert_eq!(state.active, Side::Left);
    }

    #[test]
    fn quit_always_confirms_and_defaults_to_staying() {
        let (mut state, now) = state();
        assert!(
            update(&mut state, Msg::Key(crossterm::event::KeyCode::F(10).into()), now)
                .iter()
                .all(|e| *e != Effect::Quit)
        );
        assert!(matches!(state.overlay, Some(Overlay::Confirm(crate::Confirm { accept: false, .. }))));
        assert!(!update(&mut state, Msg::Key(crossterm::event::KeyCode::Enter.into()), now).contains(&Effect::Quit));
        update(&mut state, Msg::Key(crossterm::event::KeyCode::F(10).into()), now);
        assert!(update(&mut state, Msg::Pointer(Pointer::Confirm(true)), now).contains(&Effect::Quit));
    }

    #[test]
    fn collection_picker_scrolls_creates_and_opens_without_typing_saved_names() {
        let (mut state, now) = state();
        state.collections =
            (0..30).map(|index| Collection { name: format!("Batch {index}"), entries: Vec::new() }).collect();
        open_collection_prompt(&mut state);
        update(&mut state, Msg::Key(crossterm::event::KeyCode::End.into()), now);
        assert!(matches!(state.overlay, Some(Overlay::Collections(crate::CollectionPicker { selected: 29, .. }))));
        update(&mut state, Msg::Key(crossterm::event::KeyCode::Enter.into()), now);
        assert!(
            matches!(state.active_panel().results.as_ref().unwrap().source, ResultSource::Collection(ref name) if name == "Batch 29")
        );
        open_collection_prompt(&mut state);
        update(&mut state, Msg::Pointer(Pointer::NewCollection), now);
        update(&mut state, Msg::Paste("Cleanup".into()), now);
        update(&mut state, Msg::Key(crossterm::event::KeyCode::Enter.into()), now);
        assert!(state.collections.iter().any(|c| c.name == "Cleanup"));
    }

    #[test]
    fn same_names_keep_distinct_marks_and_focus_through_sort_and_filter() {
        let mut panel = panel();
        panel.toggle_mark();
        let selected = panel.hovered_entry().unwrap().identity().to_path_buf();
        panel.set_sort(Sort { key: SortKey::Size, reverse: true });
        assert_eq!(panel.hovered_entry().unwrap().identity(), selected);
        assert_eq!(panel.marked_count(), 1);
        panel.filter_results("/b/".into());
        assert_eq!(panel.len(), 1);
        assert_eq!(panel.hovered_entry().unwrap().identity(), Path::new("/b/config.toml"));
        assert_eq!(panel.marked().next().unwrap().identity(), selected);
        assert_eq!(selected_paths(&panel), vec![selected]);
        panel.filter_results(String::new());
        assert_eq!(panel.entries().len(), 3);
    }

    #[test]
    fn grouped_headers_fold_but_never_become_file_targets() {
        let mut panel = panel();
        panel.group_results(ResultGroup::Folder);
        panel.to_start();
        assert!(matches!(panel.hovered(), Some(crate::Row::Group { count: 2, .. })));
        assert!(selected_paths(&panel).is_empty());
        panel.toggle_mark();
        assert_eq!(panel.marked_count(), 2);
        assert_eq!(selected_paths(&panel).len(), 2);
        panel.clear_marks();
        let before = panel.len();
        panel.toggle_group();
        assert_eq!(panel.len(), before - 2);
        panel.toggle_group();
        assert_eq!(panel.len(), before);
    }

    #[test]
    fn f8_removes_only_references_and_preserves_other_same_named_member() {
        let (mut state, _) = state();
        let effects = perform(&mut state, Action::RemoveReferences);
        assert!(effects.contains(&Effect::SaveCollections));
        assert!(!effects.iter().any(|e| matches!(e, Effect::StartJob { .. })));
        assert_eq!(state.collections[0].entries.len(), 2);
        assert!(state.collections[0].entries.iter().any(|e| e.identity() == Path::new("/b/config.toml")));
    }

    #[test]
    fn cancelled_search_rejects_already_queued_batches() {
        let (mut state, now) = state();
        let effects = start_search(
            &mut state,
            SearchRequest {
                skip: None,
                root: "/work".into(),
                query: "*".into(),
                hidden: false,
                options: Default::default(),
            },
        );
        let generation = effects
            .iter()
            .find_map(|e| match e {
                Effect::Search { generation, .. } => Some(*generation),
                _ => None,
            })
            .unwrap();
        update(&mut state, Msg::Batch { side: Side::Left, generation, entries: vec![file("/work/first", 1)] }, now);
        state.settle();
        assert_eq!(state.active_panel().entries().len(), 1);
        perform(&mut state, Action::CancelSearch);
        update(&mut state, Msg::Batch { side: Side::Left, generation, entries: vec![file("/work/late", 1)] }, now);
        state.settle();
        assert_eq!(state.active_panel().entries().len(), 1);
        assert!(
            matches!(state.active_panel().results.as_ref().unwrap().source, ResultSource::Search { ref progress, .. } if progress.cancelled)
        );
    }

    #[test]
    fn collecting_into_a_new_panel_cannot_be_replaced_by_its_initial_refresh() {
        let (mut state, now) = state();
        let effects = collect(&mut state);
        assert!(!effects.iter().any(|e| matches!(e, Effect::RefreshCollection { .. })));
        state.settle();
        assert_eq!(state.panel(Side::Right).entries().len(), 1);
        update(&mut state, Msg::Listed { side: Side::Right, generation: 0, error: None }, now);
        state.settle();
        assert_eq!(state.panel(Side::Right).entries().len(), 1);
    }

    #[test]
    fn column_sorting_keeps_selection_and_expanded_layout_inside_screen() {
        let (mut state, now) = state();
        state.panels[0].results.as_mut().unwrap().expanded = true;
        let path = state.active_panel().hovered_entry().unwrap().identity().to_path_buf();
        update(&mut state, Msg::Pointer(Pointer::ResultSort { side: Side::Left, key: SortKey::Size }), now);
        assert_eq!(state.active_panel().hovered_entry().unwrap().identity(), path);
        assert!(state.active_panel().sort().reverse);
        assert_eq!(state.layout(now).panels[0].width, state.cols);
        assert!(state.layout(now).panels[1].is_empty());
        state.aside = Some(crate::Aside::QuickView);
        let layout = state.layout(now);
        assert_eq!(layout.panels[0].bottom(), layout.panels[1].y);
        assert!(layout.panels[1].bottom() <= layout.prompt.y);
        assert_eq!(state.active_panel().dir(), Path::new("/work"));
    }
    #[test]
    fn removing_a_reference_invalidates_inflight_refreshes_on_both_panels() {
        let (mut state, now) = state();
        state.panels[1] = state.panels[0].clone();
        let old = state.panels[0].generation();
        state.panels[0].begin_results_refresh(old);
        state.panels[1].begin_results_refresh(old);
        perform(&mut state, Action::RemoveReferences);
        for side in Side::ALL {
            update(&mut state, Msg::Batch { side, generation: old, entries: vec![file("/a/config.toml", 10)] }, now);
            update(&mut state, Msg::Listed { side, generation: old, error: None }, now);
        }
        state.settle();
        assert_eq!(state.panels[0].entries().len(), 2);
        assert_eq!(state.panels[1].entries().len(), 2);
        assert!(state.collections[0].entries.iter().all(|entry| entry.identity() != Path::new("/a/config.toml")));
    }

    #[test]
    fn collection_limit_does_not_append_references_to_a_real_directory() {
        let (mut state, _) = state();
        state.collections = (0..COLLECTION_LIMIT)
            .map(|index| Collection { name: format!("Saved {index}"), entries: Vec::new() })
            .collect();
        let before = state.panel(Side::Right).entries().to_vec();
        collect(&mut state);
        assert!(!state.panel(Side::Right).is_results());
        assert_eq!(state.panel(Side::Right).entries(), before);
        assert!(state.notice.as_ref().unwrap().contains("limit"));
    }

    #[test]
    fn refreshing_a_search_preserves_filter_grouping_and_marks() {
        let (mut state, _) = state();
        let entries = state.active_panel().entries().to_vec();
        state.panels[0].open_results(
            ResultSource::Search {
                request: SearchRequest {
                    skip: None,
                    root: "/work".into(),
                    query: "*.toml".into(),
                    hidden: false,
                    options: Default::default(),
                },
                progress: SearchProgress::default(),
            },
            entries,
            10,
        );
        state.panels[0].toggle_mark();
        state.panels[0].filter_results("config".into());
        state.panels[0].group_results(ResultGroup::Folder);
        state.settings.preferences.show_hidden = true;
        let effect = state.reload(Side::Left);
        assert!(matches!(effect, Effect::Search { request, .. } if request.hidden));
        assert_eq!(state.active_panel().marked_count(), 1);
        let results = state.active_panel().results.as_ref().unwrap();
        assert_eq!(results.filter, "config");
        assert_eq!(results.group, ResultGroup::Folder);
    }
    fn tree_panel() -> Panel {
        let mut panel = Panel::new("/work".into(), 12, ListingMode::Full, Sort::default());
        let entries = [("/work", 120), ("/work/cache", 100), ("/work/cache/deep", 80), ("/work/src", 20)]
            .into_iter()
            .map(|(path, size)| {
                let mut entry = file(path, size);
                entry.kind = borg_vfs::EntryKind::Dir;
                entry.usage = Some(borg_vfs::FolderUsage { files: size, ..Default::default() });
                entry
            })
            .collect();
        panel.open_results(
            ResultSource::Search {
                request: SearchRequest {
                    skip: None,
                    root: "/work".into(),
                    query: "*".into(),
                    hidden: true,
                    options: crate::SearchOptions { kind: crate::SearchKind::FolderTree, ..Default::default() },
                },
                progress: SearchProgress::default(),
            },
            entries,
            10,
        );
        panel.set_sort(Sort { key: SortKey::Size, reverse: true });
        panel
    }

    fn paths(panel: &Panel) -> Vec<PathBuf> {
        (0..panel.len())
            .filter_map(|i| match panel.row(i) {
                Some(crate::Row::Entry(e)) => Some(e.identity().to_path_buf()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn tree_navigation_keeps_root_first_sorts_siblings_and_preserves_hidden_marks() {
        let mut panel = tree_panel();
        assert_eq!(paths(&panel), ["/work", "/work/cache", "/work/src"].map(PathBuf::from));
        panel.move_by(1);
        panel.tree_step(true);
        assert_eq!(paths(&panel), ["/work", "/work/cache", "/work/cache/deep", "/work/src"].map(PathBuf::from));
        panel.tree_step(true);
        assert_eq!(panel.hovered_entry().unwrap().identity(), Path::new("/work/cache/deep"));
        panel.toggle_mark();
        panel.tree_step(false);
        assert_eq!(panel.hovered_entry().unwrap().identity(), Path::new("/work/cache"));
        panel.tree_step(false);
        assert_eq!(panel.marked_count(), 1);
        assert_eq!(panel.visible_entries().filter(|e| panel.is_marked(e)).count(), 0);
        panel.toggle_mark();
        assert_eq!(selected_paths(&panel), vec![PathBuf::from("/work/cache")]);
        panel.filter_results("deep".into());
        assert_eq!(paths(&panel), ["/work", "/work/cache", "/work/cache/deep"].map(PathBuf::from));
        panel.filter_results(String::new());
        assert_eq!(panel.len(), 3);
        panel.set_sort(Sort { key: SortKey::Size, reverse: false });
        assert_eq!(paths(&panel), ["/work", "/work/src", "/work/cache"].map(PathBuf::from));
    }

    #[test]
    fn tree_stream_updates_replace_by_identity_and_cancelled_refresh_preserves_previous_tree() {
        let mut panel = tree_panel();
        let mut root = panel.entries().iter().find(|e| e.identity() == Path::new("/work")).unwrap().clone();
        root.size = 200;
        panel.receive(10, vec![root.clone(), root], true);
        panel.settle();
        assert_eq!(panel.entries().len(), 4);
        assert_eq!(panel.entries().iter().find(|e| e.identity() == Path::new("/work")).unwrap().size, 200);
        panel.begin_results_refresh(11);
        panel.receive(11, vec![file("/work/new", 50)], true);
        panel.settle();
        assert_eq!(panel.entries().len(), 4);
        panel.cancel_refresh();
        panel.finish(11, None);
        panel.settle();
        assert_eq!(panel.entries().len(), 4);
        assert!(panel.entries().iter().all(|e| e.usage.as_ref().unwrap().cached));
        panel.begin_results_refresh(12);
        panel.receive(12, vec![file("/work/new", 60)], true);
        panel.finish(12, Some("unavailable".into()));
        panel.settle();
        assert_eq!(panel.entries().len(), 4);
    }

    #[test]
    fn hiding_a_tree_reloads_normal_totals_and_resumes_branches_cursor_and_marks() {
        let (mut state, now) = state();
        state.panels[0] = tree_panel();
        state.result_history[0] = Some(Panel::new("/work".into(), 10, ListingMode::Full, Sort::default()));
        state.panels[0].move_by(1);
        state.panels[0].tree_step(true);
        state.panels[0].tree_step(true);
        state.panels[0].toggle_mark();
        let effects = update(
            &mut state,
            Msg::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            )),
            now,
        );
        assert!(effects.iter().any(|e| matches!(e, Effect::List { .. })));
        assert!(!state.active_panel().is_results());
        back(&mut state);
        assert_eq!(state.active_panel().len(), 4);
        assert_eq!(state.active_panel().hovered_entry().unwrap().identity(), Path::new("/work/cache/deep"));
        assert_eq!(state.active_panel().marked_count(), 1);
        let effect = state.reload(Side::Left);
        assert!(matches!(effect, Effect::Search { request, .. } if request.options.refresh && request.hidden));
    }
    #[test]
    fn flat_size_results_expose_the_same_rescan_and_cached_folder_handoff() {
        let (mut state, now) = state();
        state.panels[0] = tree_panel();
        let Some(Results { tree, source: ResultSource::Search { request, .. }, .. }) = &mut state.panels[0].results
        else {
            unreachable!()
        };
        *tree = None;
        request.options.kind = crate::SearchKind::FolderTotals;
        state.result_history[0] = Some(Panel::new("/work".into(), 10, ListingMode::Full, Sort::default()));
        let effects = update(
            &mut state,
            Msg::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('r'),
                crossterm::event::KeyModifiers::CONTROL,
            )),
            now,
        );
        assert!(effects.iter().any(|e| matches!(e, Effect::Search { request, side: Side::Left, .. } if request.options.refresh && request.hidden)));
        perform(&mut state, Action::CancelSearch);
        let effects = back(&mut state);
        assert!(matches!(&effects[..], [Effect::List { .. }]));
        back(&mut state);
        assert!(state.active_panel().entries().iter().all(|e| e.usage.as_ref().unwrap().cached));
    }
    #[test]
    fn lazy_tree_children_reject_old_generations_and_keep_fresh_scan_status() {
        let (mut state, now) = state();
        let mut panel = tree_panel();
        let mut entries = panel
            .entries()
            .iter()
            .filter(|e| e.identity() != Path::new("/work/cache/deep"))
            .cloned()
            .collect::<Vec<_>>();
        let branch = entries.iter_mut().find(|e| e.identity() == Path::new("/work/cache")).unwrap();
        branch.usage.as_mut().unwrap().folders = 1;
        branch.usage.as_mut().unwrap().direct_bytes = 20;
        branch.usage.as_mut().unwrap().direct_files = 2;
        panel.replace_collection(entries);
        state.panels[0] = panel;
        state.panels[0].move_by(1);
        state.panels[0].tree_step(true);
        let effects = sync_sizes(&mut state);
        let generation = state.panels[0].generation();
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::SizeChildren { parent, .. } if parent == Path::new("/work/cache")))
        );
        let mut child = file("/work/cache/deep", 80);
        child.kind = borg_vfs::EntryKind::Dir;
        child.usage = Some(borg_vfs::FolderUsage { cached: true, ..Default::default() });
        update(
            &mut state,
            Msg::SizeChildren {
                side: Side::Left,
                generation: generation + 1,
                parent: "/work/cache".into(),
                result: Ok((vec![child.clone()], false)),
            },
            now,
        );
        assert_eq!(state.panels[0].entries().len(), 3);
        update(
            &mut state,
            Msg::SizeChildren {
                side: Side::Left,
                generation,
                parent: "/work/cache".into(),
                result: Ok((vec![child], false)),
            },
            now,
        );
        assert_eq!(state.panels[0].entries().len(), 4);
        assert!(state.panels[0].entries().iter().all(|e| !e.usage.as_ref().unwrap().cached));
        assert!(matches!(state.panels[0].row(2), Some(crate::Row::Group { count: 2, .. })));
        assert!(
            matches!(&state.panels[0].results.as_ref().unwrap().source, ResultSource::Search { progress, .. } if !progress.cached)
        );
    }
}
