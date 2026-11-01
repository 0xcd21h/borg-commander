use borg_i18n::tr;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
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
    /// Files a contents search read, and those it left unread as binary or too large.
    pub read: usize,
    pub unread: usize,
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
    /// A contents search's first match in each file it found, by the file's path.
    pub lines: HashMap<PathBuf, crate::LineMatch>,
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
            lines: HashMap::new(),
        }
    }

    /// The text a contents search looked for, with its switches.
    pub fn contents(&self) -> Option<&borg_view::Query> {
        match &self.source {
            ResultSource::Search { request, .. } => request.options.contents.as_deref(),
            ResultSource::Collection(_) => None,
        }
    }

    /// The first match a contents search found in the file at `path`.
    pub fn line(&self, path: &Path) -> Option<&crate::LineMatch> {
        self.lines.get(path)
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
            ResultSource::Search { request, .. } if let Some(contents) = &request.options.contents => {
                tr!(contents_search_title, query = request.query, contents = contents.text)
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

/// Files a contents search found, each with its first match.
pub(crate) fn found(state: &mut AppState, side: Side, generation: u64, found: Vec<(Entry, crate::LineMatch)>) {
    let show_hidden = state.settings.preferences.show_hidden;
    let Some(panel) = state.receiving_panel(side, generation) else { return };
    let Some(results) = panel.results.as_mut() else { return };
    let entries = found
        .into_iter()
        .map(|(entry, line)| {
            results.lines.insert(entry.identity().to_path_buf(), line);
            entry
        })
        .collect();
    panel.receive(generation, entries, show_hidden);
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
mod tests;
