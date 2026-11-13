//! The context menu of a panel's files and of results: the marked items, or the one under the
//! cursor.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use borg_i18n::{Count, tr};

use super::{ContextCommand, copy, copy_lines, perform};
use crate::menu::when;
use crate::{Action, AppState, Menu, MenuEntry, Panel};

pub(super) fn entries(state: &AppState) -> Option<Menu<ContextCommand>> {
    let panel = state.active_panel();
    let selection = panel.selection();
    let paths: Vec<PathBuf> = selection.iter().map(|entry| entry.path(panel.dir())).collect();
    let title = match selection.as_slice() {
        [] => return None,
        [one] => one.name.clone(),
        many => borg_i18n::count(many.len(), Count::Item),
    };
    let hovered_file = panel.hovered_entry().is_some_and(|entry| !entry.is_dir());
    let mut entries = copies(panel, &paths);
    entries.extend([
        MenuEntry::Separator,
        when(perform(tr!(view), Action::View), hovered_file),
        when(perform(tr!(edit), Action::Edit), hovered_file),
        perform(tr!(copy), Action::Copy),
        perform(tr!(move), Action::Move),
        perform(if panel.is_results() { tr!(remove_actions) } else { tr!(delete) }, Action::Delete),
    ]);
    if let Some(entry) = panel.hovered_entry() {
        let label = if panel.is_marked(entry) { tr!(context_unmark) } else { tr!(context_mark) };
        entries.extend([MenuEntry::Separator, perform(label, Action::ToggleMark)]);
        if panel.is_results() {
            entries.push(perform(tr!(reveal_original), Action::RevealOriginal));
        }
    }
    Some(Menu { title, entries })
}

/// The paths and names to copy, the folder they are in when they share one, and the line a
/// contents search found in a single file.
fn copies(panel: &Panel, paths: &[PathBuf]) -> Vec<MenuEntry<ContextCommand>> {
    let shown = |path: &Path| path.to_string_lossy().into_owned();
    let names: Vec<String> = paths
        .iter()
        .map(|path| path.file_name().map_or_else(|| shown(path), |name| name.to_string_lossy().into_owned()))
        .collect();
    let full: Vec<String> = paths.iter().map(|path| shown(path)).collect();
    let folders: BTreeSet<&Path> = paths.iter().filter_map(|path| path.parent()).collect();
    let mut entries = match (full.as_slice(), names.as_slice()) {
        ([path], [name]) => {
            vec![copy(tr!(context_copy_path), path.as_str()), copy(tr!(context_copy_name), name.as_str())]
        }
        _ => vec![
            copy_lines(tr!(context_copy_path), &full, tr!(paths_copied)),
            copy_lines(tr!(context_copy_name), &names, tr!(names_copied)),
        ],
    };
    if let [folder] = folders.into_iter().collect::<Vec<_>>().as_slice() {
        entries.push(copy(tr!(context_copy_folder), shown(folder)));
    }
    if let [path] = paths
        && let Some(found) = panel.results.as_ref().and_then(|results| results.lines.get(path))
    {
        entries.push(copy(tr!(context_copy_line), format!("{}:{}", shown(path), found.line)));
    }
    entries
}
