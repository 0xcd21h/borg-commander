use std::path::Path;

use crate::{Theme, dialog_view::centered, format};
use borg_core::{
    AppState, CONTENTS_FIELD, CollectionPicker, EXCLUSION_SWITCHES, ExclusionEditor, FILES_FIELD, HIDDEN_FIELD,
    LineInput, MINIMUM_FIELD, PATTERN_FIELD, PastSearch, ROOT_FIELD, SKIP_FIELD, SearchForm,
};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};

const MAX_WIDTH: u16 = 82;
const SEARCH_HEIGHT: u16 = 14;
/// The contents field's row, which it shares with the switches for case, words and regular
/// expressions.
const CONTENTS_ROW: u16 = 2;
const HIDDEN_ROW: u16 = 7;
const SKIP_ROW: u16 = 8;
const ERROR_ROW: u16 = 9;
const SEARCH_BUTTON_ROW: u16 = 10;
const HINT_ROW: u16 = 11;
/// The heading over recent searches, a row below the hint.
const RECENT_ROW: u16 = 13;
/// The first pattern's row in the exclusion list, below the switches and a heading.
const PATTERNS_ROW: u16 = 5;
/// The exclusion list's rows besides its patterns: borders, switches, headings, the row for
/// adding a pattern, the hint, the note and the error.
const EXCLUSIONS_CHROME: u16 = 12;
const FIELD_GAP: u16 = 2;
const FIELD_PADDING: u16 = 2;
const LIST_ROWS: u16 = 10;
const COLLECTION_CHROME: u16 = 10;

pub(crate) fn collection_area(screen: Rect) -> Rect {
    centered(
        screen,
        screen.width.saturating_sub(4).min(MAX_WIDTH),
        screen.height.saturating_sub(2).min(LIST_ROWS + COLLECTION_CHROME),
    )
}

pub(crate) fn collection_list(screen: Rect) -> Rect {
    let area = collection_area(screen);
    Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(3),
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(COLLECTION_CHROME).max(1).min(area.height.saturating_sub(4)),
    }
}

pub(crate) fn collection_start(picker: &CollectionPicker, screen: Rect) -> usize {
    picker.selected.saturating_sub(usize::from(collection_list(screen).height).saturating_sub(1))
}

fn write(buf: &mut Buffer, area: Rect, row: u16, value: String, style: ratatui::style::Style) {
    if row < area.height && area.width > 0 {
        Line::styled(value, style).render(Rect { y: area.y + row, height: 1, ..area }, buf);
    }
}

pub(crate) fn collections(
    state: &AppState,
    picker: &CollectionPicker,
    theme: &Theme,
    screen: Rect,
    buf: &mut Buffer,
) -> Rect {
    let area = collection_area(screen);
    Clear.render(area, buf);
    Block::bordered()
        .style(theme.styles.overlay)
        .border_style(theme.styles.overlay_border)
        .title(tr!(collections))
        .title_bottom(tr!(pgup_pgdn_enter_open_f7_n_new))
        .render(area, buf);
    let inner = area.inner(ratatui::layout::Margin::new(2, 1));
    write(buf, inner, 0, tr!(collection_count, state.collections.len()), theme.styles.overlay);
    write(
        buf,
        inner,
        1,
        format!("{} {}   {}", format::pad_end(tr!(collection), 42), format::pad_start(tr!(entries), 9), tr!(file_size)),
        theme.styles.menu,
    );
    let list = collection_list(screen);
    let start = collection_start(picker, screen);
    for (offset, (index, collection)) in
        state.collections.iter().enumerate().skip(start).take(list.height as usize).enumerate()
    {
        let bytes = collection.entries.iter().filter(|e| !e.is_dir()).fold(0u64, |sum, e| sum.saturating_add(e.size));
        let style = if index == picker.selected { theme.styles.overlay_selected } else { theme.styles.overlay };
        let row = Rect { y: list.y + offset as u16, height: 1, ..list };
        buf.set_style(row, style);
        write(
            buf,
            row,
            0,
            format!(
                "{:<42} {:>9}   {}",
                format::truncate_end(&collection.name, 42),
                collection.entries.len(),
                if collection.entries.iter().any(|e| e.missing) {
                    format!("≥{}", format::size(bytes))
                } else {
                    format::size(bytes)
                }
            ),
            style,
        );
    }
    if state.collections.is_empty() {
        write(buf, list, 0, tr!(collections_empty).into(), theme.styles.overlay);
    }
    let details = list.bottom().saturating_sub(inner.y).saturating_add(1);
    if let Some(collection) = state.collections.get(picker.selected) {
        let missing = collection.entries.iter().filter(|e| e.missing).count();
        write(
            buf,
            inner,
            details,
            tr!(
                references_folders_missing_or_not_checked,
                collection.entries.len(),
                collection.entries.iter().filter(|e| e.is_dir()).count(),
                missing
            ),
            theme.styles.overlay,
        );
    }
    if let Some(input) = &picker.creating {
        use unicode_width::UnicodeWidthStr;
        let label = tr!(name);
        let offset = (label.width() as u16).saturating_add(FIELD_GAP).min(inner.width);
        let row = details.saturating_add(2);
        write(buf, Rect { width: offset, ..inner }, row, label.into(), theme.styles.overlay);
        if row < inner.height {
            let field_area = Rect {
                x: inner.x.saturating_add(offset),
                y: inner.y.saturating_add(row),
                width: inner.width.saturating_sub(offset),
                height: 1,
            };
            field(
                buf,
                field_area,
                &input_text(input, true, usize::from(field_area.width.saturating_sub(FIELD_PADDING * 2))),
                true,
                theme,
            );
        }
        write(buf, inner, details.saturating_add(3), tr!(enter_create_esc_return_to_list).into(), theme.styles.overlay);
    } else {
        write(
            buf,
            inner,
            details.saturating_add(2),
            format!("{}     {}", tr!(collections_open_button), tr!(collections_new_button)),
            theme.styles.menu,
        );
    }
    area
}

/// The search dialog, taller by a heading, a gap and a row for each of `recent` searches.
pub(crate) fn search_area(screen: Rect, recent: usize) -> Rect {
    let rows = if recent == 0 { 0 } else { recent as u16 + 2 };
    centered(screen, screen.width.saturating_sub(4).min(MAX_WIDTH), (SEARCH_HEIGHT + rows).min(screen.height))
}

pub(crate) fn search_field(screen: Rect, recent: usize, point: Position) -> Option<usize> {
    let area = search_area(screen, recent).inner(ratatui::layout::Margin::new(2, 1));
    (area.contains(point) && point.y > area.y && point.y <= area.y + SKIP_ROW)
        .then(|| usize::from(point.y - area.y - 1))
}

/// The search dialog's labels, and where its fields start.
fn search_fields(screen: Rect, recent: usize) -> (Rect, u16) {
    use unicode_width::UnicodeWidthStr;
    let inner = search_area(screen, recent).inner(ratatui::layout::Margin::new(2, 1));
    let labels =
        [tr!(name_or_glob), tr!(contents_label), tr!(search_in), tr!(look_for), tr!(minimum_size), tr!(minimum_files)];
    let label_width = labels.iter().map(|label| label.width()).max().unwrap_or(0) as u16;
    let label_width = label_width.min(inner.width / 2);
    (inner, inner.x.saturating_add(label_width).saturating_add(FIELD_GAP))
}

/// The contents field's switches, at the end of its row.
fn contents_switches(screen: Rect, recent: usize) -> [Rect; 3] {
    let (inner, field_x) = search_fields(screen, recent);
    let row = Rect { y: inner.y + CONTENTS_ROW, height: u16::from(inner.height > CONTENTS_ROW), ..inner };
    let end = inner.right().saturating_sub(crate::find_view::SWITCHES_WIDTH).max(field_x);
    crate::find_view::switch_areas(Rect { x: field_x, width: end - field_x, ..row }, row)
}

/// The key a click on one of the contents field's switches presses.
pub(crate) fn contents_switch(screen: Rect, recent: usize, point: Position) -> Option<crossterm::event::KeyEvent> {
    crate::find_view::switch_key(contents_switches(screen, recent), point)
}

/// The label on the leave-out row that opens what is left out.
pub(crate) fn edit_exclusions(screen: Rect, recent: usize) -> Rect {
    use unicode_width::UnicodeWidthStr;
    let inner = search_area(screen, recent).inner(ratatui::layout::Margin::new(2, 1));
    let width = (tr!(edit_exclusions).width() as u16).min(inner.width);
    Rect { x: inner.right() - width, y: inner.y + SKIP_ROW, width, height: u16::from(inner.height > SKIP_ROW) }
}

/// The exclusion list, the first pattern it shows and how many fit.
fn exclusions_window(editor: &ExclusionEditor, state: &AppState, screen: Rect) -> (Rect, usize, usize) {
    let patterns = state.settings.search.exclude.len();
    let height = (EXCLUSIONS_CHROME + patterns as u16).min(screen.height.saturating_sub(2));
    let area = centered(screen, screen.width.saturating_sub(4).min(MAX_WIDTH), height);
    let shown = usize::from(height.saturating_sub(EXCLUSIONS_CHROME)).min(patterns);
    let chosen = editor.selected.saturating_sub(EXCLUSION_SWITCHES).min(patterns.saturating_sub(1));
    (area, (chosen + 1).saturating_sub(shown.max(1)).min(patterns - shown), shown)
}

pub(crate) fn exclusions_area(editor: &ExclusionEditor, state: &AppState, screen: Rect) -> Rect {
    exclusions_window(editor, state, screen).0
}

/// The row of the exclusion list at `point`: a switch, a pattern or the row for adding one.
pub(crate) fn exclusion_row(
    editor: &ExclusionEditor,
    state: &AppState,
    screen: Rect,
    point: Position,
) -> Option<usize> {
    let (area, first, shown) = exclusions_window(editor, state, screen);
    let inner = area.inner(ratatui::layout::Margin::new(2, 1));
    let row = point.y.checked_sub(inner.y).filter(|_| inner.contains(point))?;
    match row {
        1 | 2 => Some(usize::from(row - 1)),
        row if row >= PATTERNS_ROW && usize::from(row - PATTERNS_ROW) < shown => {
            Some(EXCLUSION_SWITCHES + first + usize::from(row - PATTERNS_ROW))
        }
        row if usize::from(row) == usize::from(PATTERNS_ROW) + shown => Some(ExclusionEditor::rows(state) - 1),
        _ => None,
    }
}

pub(crate) fn exclusions(
    editor: &ExclusionEditor,
    state: &AppState,
    theme: &Theme,
    screen: Rect,
    buf: &mut Buffer,
) -> Rect {
    let (area, first, shown) = exclusions_window(editor, state, screen);
    Clear.render(area, buf);
    Block::bordered()
        .style(theme.styles.overlay)
        .border_style(theme.styles.overlay_border)
        .title(tr!(exclusions_title))
        .title_bottom(tr!(exclusions_keys))
        .render(area, buf);
    let inner = area.inner(ratatui::layout::Margin::new(2, 1));
    let search = &state.settings.search;
    let switches =
        [(search.artifacts, tr!(exclude_generated_folders)), (search.ignore_files, tr!(exclude_ignored_files))];
    for (row, (on, label)) in switches.into_iter().enumerate() {
        let style = if editor.selected == row { theme.styles.overlay_selected } else { theme.styles.input };
        write(
            buf,
            Rect { width: 3.min(inner.width), ..inner },
            row as u16 + 1,
            if on { "[x]" } else { "[ ]" }.into(),
            style,
        );
        let label_area = Rect { x: inner.x.saturating_add(4), width: inner.width.saturating_sub(4), ..inner };
        write(buf, label_area, row as u16 + 1, label.into(), theme.styles.overlay);
    }
    write(buf, inner, PATTERNS_ROW - 1, tr!(exclusion_patterns).into(), theme.styles.footer);
    let width = usize::from(inner.width.saturating_sub(FIELD_PADDING * 2));
    let editing = editor.editing.as_ref();
    let add = ExclusionEditor::rows(state) - 1;
    let rows = (first..first + shown)
        .map(|at| (EXCLUSION_SWITCHES + at, search.exclude[at].clone()))
        .chain([(add, tr!(add_exclusion).into())]);
    for (y, (row, text)) in (PATTERNS_ROW..).zip(rows) {
        let line = Rect { y: inner.y + y, height: 1, ..inner };
        if y >= inner.height {
            break;
        }
        let at = (row != add).then(|| row - EXCLUSION_SWITCHES);
        match editing.filter(|(editing, _)| *editing == at) {
            Some((_, input)) => field(buf, line, &input_text(input, true, width), true, theme),
            None => {
                let style = if editor.selected == row { theme.styles.overlay_selected } else { theme.styles.overlay };
                buf.set_style(line, style);
                write(buf, line, 0, format::truncate_end(&text, usize::from(inner.width)), style);
            }
        }
    }
    let below = PATTERNS_ROW + shown as u16 + 2;
    write(
        buf,
        inner,
        below,
        format::truncate_end(tr!(exclusion_pattern_hint), usize::from(inner.width)),
        theme.styles.footer,
    );
    if cfg!(target_os = "macos") {
        write(
            buf,
            inner,
            below + 1,
            format::truncate_end(tr!(exclusion_other_apps), usize::from(inner.width)),
            theme.styles.footer,
        );
    }
    if let Some(error) = &editor.error {
        write(buf, inner, below + 2, error.clone(), theme.styles.notice);
    }
    area
}

/// The place in the history of the recent search at `point`.
pub(crate) fn search_recent(screen: Rect, recent: &[(usize, &PastSearch)], point: Position) -> Option<usize> {
    let area = search_area(screen, recent.len()).inner(ratatui::layout::Margin::new(2, 1));
    let row = point.y.checked_sub(area.y + RECENT_ROW + 1).filter(|_| area.contains(point))?;
    recent.get(usize::from(row)).map(|(at, _)| *at)
}

/// `path` as the panels title it, below `~` when it is in the home folder.
fn place(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

fn input_text(input: &LineInput, focused: bool, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    if !focused {
        return format::truncate_end(input.text(), width);
    }
    let before: String = input.text().chars().take(input.cursor_chars()).collect();
    let after: String = input.text().chars().skip(input.cursor_chars()).collect();
    let shift = before.width().saturating_sub(width.saturating_sub(1));
    crate::dialog_view::skip_columns(&format!("{before}▏{after}"), shift)
}

pub(crate) fn search_button(screen: Rect, recent: usize) -> Rect {
    use unicode_width::UnicodeWidthStr;
    let inner = search_area(screen, recent).inner(ratatui::layout::Margin::new(2, 1));
    Rect {
        y: inner.y.saturating_add(SEARCH_BUTTON_ROW),
        width: inner.width.min(tr!(search_start_button).width() as u16),
        height: u16::from(inner.height > SEARCH_BUTTON_ROW),
        ..inner
    }
}

fn field(buf: &mut Buffer, area: Rect, input: &str, focused: bool, theme: &Theme) {
    if area.is_empty() {
        return;
    }
    let style = if focused { theme.styles.overlay_selected } else { theme.styles.input };
    buf.set_style(area, style);
    write(buf, area, 0, "[".into(), style);
    if area.width > 1 {
        write(buf, Rect { x: area.right() - 1, width: 1, ..area }, 0, "]".into(), style);
    }
    let contents =
        Rect { x: area.x.saturating_add(FIELD_PADDING), width: area.width.saturating_sub(FIELD_PADDING * 2), ..area };
    write(buf, contents, 0, input.into(), style);
}

pub(crate) fn search(
    form: &SearchForm,
    state: &AppState,
    home: Option<&Path>,
    theme: &Theme,
    screen: Rect,
    buf: &mut Buffer,
) -> Rect {
    use unicode_width::UnicodeWidthStr;
    let recent = form.recent(state);
    let area = search_area(screen, recent.len());
    Clear.render(area, buf);
    Block::bordered()
        .style(theme.styles.overlay)
        .border_style(theme.styles.overlay_border)
        .title(if matches!(form.kind, borg_core::SearchKind::FolderTotals | borg_core::SearchKind::FolderTree) {
            tr!(size_search_dialog_title)
        } else {
            tr!(search_dialog_title)
        })
        .title_bottom(tr!(tab_fields_mode_space_toggle_esc_cancel))
        .render(area, buf);
    let (inner, field_x) = search_fields(screen, recent.len());
    let field_width = inner.right().saturating_sub(field_x);
    let width = usize::from(field_width.saturating_sub(FIELD_PADDING * 2));
    let switches = contents_switches(screen, recent.len());
    let contents_width = switches[0].x.saturating_sub(1).saturating_sub(field_x);
    let contents_room = usize::from(contents_width.saturating_sub(FIELD_PADDING * 2));
    let any = |input: &LineInput, field: usize| {
        if input.text().is_empty() && form.selected != field {
            tr!(any).into()
        } else {
            input_text(input, form.selected == field, width)
        }
    };
    let fields = [
        (tr!(name_or_glob), input_text(&form.pattern, form.selected == PATTERN_FIELD, width), field_width),
        (
            tr!(contents_label),
            input_text(&form.contents.input, form.selected == CONTENTS_FIELD, contents_room),
            contents_width,
        ),
        (tr!(search_in), input_text(&form.root, form.selected == ROOT_FIELD, width), field_width),
        (tr!(look_for), format!("‹ {} ›", form.kind.label()), field_width),
        (tr!(minimum_size), any(&form.minimum, MINIMUM_FIELD), field_width),
        (tr!(minimum_files), any(&form.files, FILES_FIELD), field_width),
    ];
    let label_width = field_x.saturating_sub(inner.x).saturating_sub(FIELD_GAP);
    for (index, (label, value, field_width)) in fields.into_iter().enumerate() {
        let row = index as u16 + 1;
        if row >= inner.height || !form.shows(index) {
            continue;
        }
        write(buf, Rect { width: label_width, ..inner }, row, label.into(), theme.styles.overlay);
        field(
            buf,
            Rect { x: field_x, y: inner.y + row, width: field_width, height: 1 },
            &value,
            form.selected == index,
            theme,
        );
    }
    if form.shows(CONTENTS_FIELD) {
        let off = theme.styles.overlay.add_modifier(ratatui::style::Modifier::DIM);
        crate::find_view::draw_switches(&form.contents, switches, theme.styles.cursor, off, buf);
    }
    let sizes = matches!(form.kind, borg_core::SearchKind::FolderTotals | borg_core::SearchKind::FolderTree);
    let checks = [
        (HIDDEN_ROW, form.hidden, HIDDEN_FIELD, tr!(include_hidden_files_and_folders)),
        (SKIP_ROW, form.skip, SKIP_FIELD, tr!(skip_generated_files)),
    ];
    for (row, checked, index, label) in checks.into_iter().filter(|(_, _, index, _)| form.shows(*index)) {
        write(
            buf,
            Rect { width: 3.min(inner.width), ..inner },
            row,
            if checked { "[x]" } else { "[ ]" }.into(),
            if form.selected == index { theme.styles.overlay_selected } else { theme.styles.input },
        );
        let label_area = Rect { x: inner.x.saturating_add(4), width: inner.width.saturating_sub(4), ..inner };
        write(buf, label_area, row, label.into(), theme.styles.overlay);
    }
    if !sizes {
        write(buf, edit_exclusions(screen, recent.len()), 0, tr!(edit_exclusions).into(), theme.styles.footer);
        let hint = if form.selected == CONTENTS_FIELD { tr!(contents_hint) } else { tr!(search_query_hint) };
        write(buf, inner, HINT_ROW, format::truncate_end(hint, usize::from(inner.width)), theme.styles.footer);
    }
    if let Some(error) = &form.error {
        write(buf, inner, ERROR_ROW, error.clone(), theme.styles.notice);
    }
    write(buf, search_button(screen, recent.len()), 0, tr!(search_start_button).into(), theme.styles.menu);
    if !recent.is_empty() {
        write(buf, inner, RECENT_ROW, tr!(recent_searches).into(), theme.styles.footer);
    }
    let root = form.root(state);
    let recalled = form.recalled.as_ref().map(|(at, _)| *at);
    for (row, (at, past)) in (RECENT_ROW + 1..inner.height).zip(&recent) {
        let style = if recalled == Some(*at) { theme.styles.overlay_selected } else { theme.styles.overlay };
        let line = Rect { y: inner.y + row, height: 1, ..inner };
        buf.set_style(line, style);
        let width = usize::from(inner.width);
        // Searches made elsewhere say where, so a query that only makes sense there is not taken for this one.
        let place = (past.root != root)
            .then(|| format!("  {}", tr!(recent_search_in, place(&past.root, home))))
            .map(|place| format::truncate_start(&place, width / 2))
            .unwrap_or_default();
        let query = format::truncate_end(&past.query, width.saturating_sub(place.width()));
        Line::from(vec![Span::styled(query, style), Span::styled(place, style.patch(theme.styles.footer))])
            .render(line, buf);
    }
    area
}
