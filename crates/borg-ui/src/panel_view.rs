use borg_core::{Listing, ListingMode, Panel, Row};
use borg_vfs::{Entry, EntryKind};
use jiff::Zoned;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Widget},
};

use crate::{Theme, format, theme::Styles};

const SIZE_WIDTH: u16 = 6;
const DATE_WIDTH: u16 = 12;
const MARKER_WIDTH: u16 = 1;
const GAP: u16 = 1;
const FIRST_META_CELL: usize = 2;
const META_STRIDE: usize = 2;
const SHOW_DATE_FROM: u16 = 40;
const SHOW_SIZE_FROM: u16 = 24;
const TITLE_PADDING: usize = 2;
const PARENT_NAME: &str = "..";
const PARENT_LABEL: &str = "UP";
const DIR_LABEL: &str = "DIR";
const COLUMN_SEPARATOR: &str = "│";

pub fn draw_panel(panel: &Panel, active: bool, area: Rect, theme: &Theme, clock: &Zoned, buf: &mut Buffer) {
    let styles = &theme.styles;
    let (border, title_style) =
        if active { (styles.border_active, styles.title_active) } else { (styles.border, styles.title) };
    let path = panel.dir().display().to_string();
    let title = format::truncate_start(&path, usize::from(area.width).saturating_sub(TITLE_PADDING * 2));
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.panel)
        .border_style(border)
        .title(Span::styled(format!(" {title} "), title_style))
        .title_bottom(footer(panel, styles));

    let inner = block.inner(area);
    block.render(area, buf);

    match panel.mode() {
        ListingMode::Full => draw_full(panel, active, inner, styles, clock, buf),
        ListingMode::Brief => draw_brief(panel, active, inner, styles, border, buf),
    }
}

fn draw_full(panel: &Panel, active: bool, inner: Rect, styles: &Styles<Style>, clock: &Zoned, buf: &mut Buffer) {
    let columns = Columns::fit(inner.width);
    for (index, row) in panel.visible() {
        let offset = u16::try_from(index - panel.scroll()).unwrap_or(u16::MAX);
        let line_area = Rect { y: inner.y + offset, height: 1, ..inner };
        let look = Look::of(panel, row, index, active, styles);
        draw_full_row(row, &columns, line_area, look, clock, buf);
    }
}

fn draw_brief(panel: &Panel, active: bool, inner: Rect, styles: &Styles<Style>, border: Style, buf: &mut Buffer) {
    let columns = panel.mode().columns();
    let constraints = (0..columns).map(|_| Constraint::Ratio(1, u32::try_from(columns).unwrap_or(1)));
    let cells = Layout::horizontal(constraints).split(inner);
    let rows = panel.rows().max(1);

    for (index, row) in panel.visible() {
        let slot = index - panel.scroll();
        let Some(column) = cells.get(slot / rows) else { continue };
        let separator = usize::from(slot / rows + 1 < columns);
        let width = column.width.saturating_sub(u16::try_from(separator).unwrap_or(0));
        let y = inner.y + u16::try_from(slot % rows).unwrap_or(u16::MAX);
        let area = Rect { x: column.x, y, width, height: 1 };
        let look = Look::of(panel, row, index, active, styles);
        draw_name(row, area, look, buf);
    }

    for column in cells.iter().take(columns.saturating_sub(1)) {
        let x = column.x + column.width.saturating_sub(1);
        (inner.y..inner.y + inner.height).for_each(|y| {
            buf[(x, y)].set_symbol(COLUMN_SEPARATOR).set_style(border);
        });
    }
}

#[derive(Clone, Copy)]
struct Look {
    name: Style,
    meta: Style,
    fill: Option<Style>,
}

impl Look {
    fn of(panel: &Panel, row: Row<'_>, index: usize, active: bool, styles: &Styles<Style>) -> Self {
        let marked = matches!(row, Row::Entry(entry) if panel.is_marked(entry));
        let base = match row {
            Row::Parent => styles.dir,
            Row::Entry(_) if marked => styles.marked,
            Row::Entry(entry) => style_of(entry, styles),
        };
        let fill = (index == panel.cursor()).then_some(match (active, marked) {
            (true, true) => styles.marked_cursor,
            (true, false) => styles.cursor,
            (false, _) => styles.cursor_inactive,
        });
        Self {
            name: fill.map_or(base, |f| base.patch(f)),
            meta: fill.map_or(styles.meta, |f| styles.meta.patch(f)),
            fill,
        }
    }
}

struct Columns {
    name: u16,
    size: Option<u16>,
    date: Option<u16>,
}

impl Columns {
    fn fit(width: u16) -> Self {
        let date = (width >= SHOW_DATE_FROM).then_some(DATE_WIDTH);
        let size = (width >= SHOW_SIZE_FROM).then_some(SIZE_WIDTH);
        let reserved = [size, date].into_iter().flatten().map(|w| w + GAP).sum::<u16>();
        Self { name: width.saturating_sub(reserved), size, date }
    }

    fn constraints(&self) -> Vec<Constraint> {
        let mut constraints = vec![Constraint::Length(self.name)];
        constraints.extend(self.size.into_iter().flat_map(|w| [Constraint::Length(GAP), Constraint::Length(w)]));
        constraints.extend(self.date.into_iter().flat_map(|w| [Constraint::Length(GAP), Constraint::Length(w)]));
        constraints
    }
}

fn draw_full_row(row: Row<'_>, columns: &Columns, area: Rect, look: Look, clock: &Zoned, buf: &mut Buffer) {
    let (size, date) = match row {
        Row::Parent => (PARENT_LABEL.to_owned(), String::new()),
        Row::Entry(entry) => (
            if entry.is_dir() { DIR_LABEL.to_owned() } else { format::size(entry.size) },
            format::modified(entry.modified, clock),
        ),
    };

    if let Some(fill) = look.fill {
        buf.set_style(area, fill);
    }
    let cells = Layout::horizontal(columns.constraints()).split(area);
    draw_name(row, cells[0], look, buf);
    let mut rest = cells.iter().skip(FIRST_META_CELL).step_by(META_STRIDE);
    if columns.size.is_some()
        && let Some(cell) = rest.next()
    {
        Line::styled(size, look.meta).right_aligned().render(*cell, buf);
    }
    if columns.date.is_some()
        && let Some(cell) = rest.next()
    {
        Line::styled(date, look.meta).render(*cell, buf);
    }
}

fn draw_name(row: Row<'_>, area: Rect, look: Look, buf: &mut Buffer) {
    if let Some(fill) = look.fill {
        buf.set_style(area, fill);
    }
    let (marker, name) = match row {
        Row::Parent => (' ', PARENT_NAME),
        Row::Entry(entry) => (marker(entry), entry.name.as_str()),
    };
    let width = usize::from(area.width.saturating_sub(MARKER_WIDTH));
    Line::styled(format!("{marker}{}", format::truncate_end(name, width)), look.name).render(area, buf);
}

fn marker(entry: &Entry) -> char {
    match entry.kind {
        EntryKind::Dir => '/',
        EntryKind::Symlink { to_dir: true } => '~',
        EntryKind::Symlink { to_dir: false } => '@',
        EntryKind::Other => '?',
        EntryKind::File if entry.executable => '*',
        EntryKind::File => ' ',
    }
}

fn style_of(entry: &Entry, styles: &Styles<Style>) -> Style {
    match entry.kind {
        EntryKind::Dir => styles.dir,
        EntryKind::Symlink { .. } => styles.link,
        EntryKind::Other => styles.other,
        EntryKind::File if entry.executable => styles.executable,
        EntryKind::File => styles.file,
    }
}

fn footer<'a>(panel: &Panel, styles: &Styles<Style>) -> Line<'a> {
    let marked = panel.marked_count();
    if marked > 0 {
        let bytes: u64 = panel.marked().filter(|e| !e.is_dir()).map(|e| e.size).sum();
        let text =
            if bytes > 0 { format!("{marked} marked · {}", format::size(bytes)) } else { format!("{marked} marked") };
        return Line::styled(format!(" {text} "), styles.marked);
    }
    let count = panel.received();
    match panel.listing() {
        Listing::Loading => Line::styled(format!(" loading… {count} "), styles.footer),
        Listing::Ready => Line::styled(format!(" {} ", format::count(count, "item")), styles.footer),
        Listing::Failed(error) => Line::styled(format!(" {error} "), styles.notice),
    }
}
