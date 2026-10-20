use crate::{Theme, dialog_view::centered, format};
use borg_core::{AppState, CollectionPicker, LineInput, SearchForm};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    text::Line,
    widgets::{Block, Clear, Widget},
};

const MAX_WIDTH: u16 = 82;
const SEARCH_HEIGHT: u16 = 13;
const SEARCH_BUTTON_ROW: u16 = 9;
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
    write(buf, inner, 0, format!(tr!(collection_count), state.collections.len()), theme.styles.overlay);
    write(
        buf,
        inner,
        1,
        format!("{:<42} {:>9}   {}", tr!(collection), tr!(entries), tr!(file_size)),
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
            format!(
                tr!(references_folders_missing_or_not_checked),
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

pub(crate) fn search_area(screen: Rect) -> Rect {
    centered(screen, screen.width.saturating_sub(4).min(MAX_WIDTH), SEARCH_HEIGHT.min(screen.height))
}

pub(crate) fn search_field(screen: Rect, point: Position) -> Option<usize> {
    let area = search_area(screen).inner(ratatui::layout::Margin::new(2, 1));
    (area.contains(point) && point.y > area.y && point.y <= area.y + 6).then(|| usize::from(point.y - area.y - 1))
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

pub(crate) fn search_button(screen: Rect) -> Rect {
    use unicode_width::UnicodeWidthStr;
    let inner = search_area(screen).inner(ratatui::layout::Margin::new(2, 1));
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

pub(crate) fn search(form: &SearchForm, theme: &Theme, screen: Rect, buf: &mut Buffer) -> Rect {
    use unicode_width::UnicodeWidthStr;
    let area = search_area(screen);
    Clear.render(area, buf);
    Block::bordered()
        .style(theme.styles.overlay)
        .border_style(theme.styles.overlay_border)
        .title(tr!(search_dialog_title))
        .title_bottom(tr!(tab_fields_mode_space_toggle_esc_cancel))
        .render(area, buf);
    let inner = area.inner(ratatui::layout::Margin::new(2, 1));
    let labels = [tr!(name_or_glob), tr!(search_in), tr!(look_for), tr!(minimum_size), tr!(minimum_files)];
    let label_width = labels.iter().map(|label| label.width()).max().unwrap_or(0) as u16;
    let label_width = label_width.min(inner.width / 2);
    let field_x = inner.x.saturating_add(label_width).saturating_add(FIELD_GAP);
    let field_width = inner.right().saturating_sub(field_x);
    let width = usize::from(field_width.saturating_sub(FIELD_PADDING * 2));
    let fields = [
        input_text(&form.pattern, form.selected == 0, width),
        input_text(&form.root, form.selected == 1, width),
        format!("‹ {} ›", form.kind.label()),
        if form.minimum.text().is_empty() && form.selected != 3 {
            tr!(any).into()
        } else {
            input_text(&form.minimum, form.selected == 3, width)
        },
        if form.files.text().is_empty() && form.selected != 4 {
            tr!(any).into()
        } else {
            input_text(&form.files, form.selected == 4, width)
        },
    ];
    for (index, (label, value)) in labels.into_iter().zip(fields).enumerate() {
        let row = index as u16 + 1;
        if row >= inner.height {
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
    let checked = if form.hidden { "[x]" } else { "[ ]" };
    write(
        buf,
        Rect { width: 3.min(inner.width), ..inner },
        6,
        checked.into(),
        if form.selected == 5 { theme.styles.overlay_selected } else { theme.styles.input },
    );
    write(
        buf,
        Rect { x: inner.x.saturating_add(4), width: inner.width.saturating_sub(4), ..inner },
        6,
        tr!(include_hidden_files_and_folders).into(),
        theme.styles.overlay,
    );
    if let Some(error) = &form.error {
        write(buf, inner, 8, error.clone(), theme.styles.notice);
    }
    write(buf, search_button(screen), 0, tr!(search_start_button).into(), theme.styles.menu);
    area
}
