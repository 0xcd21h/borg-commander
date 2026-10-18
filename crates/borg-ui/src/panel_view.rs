use borg_core::{Listing, Panel, Row};
use borg_vfs::{Entry, EntryKind};
use jiff::Zoned;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Widget},
};

use crate::{format, theme::Theme};

const SIZE_WIDTH: u16 = 6;
const DATE_WIDTH: u16 = 12;
const MARKER_WIDTH: u16 = 1;
const GAP: u16 = 1;
const FIRST_META_CELL: usize = 3;
const META_STRIDE: usize = 2;
const SHOW_DATE_FROM: u16 = 40;
const SHOW_SIZE_FROM: u16 = 24;
const TITLE_PADDING: usize = 2;
const PARENT_NAME: &str = "..";
const PARENT_LABEL: &str = "UP";
const DIR_LABEL: &str = "DIR";

pub fn draw_panel(panel: &Panel, active: bool, area: Rect, theme: &Theme, now: &Zoned, buf: &mut Buffer) {
    let (border, title_style) =
        if active { (theme.border_active, theme.title_active) } else { (theme.border, theme.title) };
    let path = panel.dir().display().to_string();
    let title = format::truncate_start(&path, usize::from(area.width).saturating_sub(TITLE_PADDING * 2));
    let block = Block::bordered()
        .style(theme.panel)
        .border_style(border)
        .title(Span::styled(format!(" {title} "), title_style))
        .title_bottom(footer(panel, theme));

    let inner = block.inner(area);
    block.render(area, buf);

    let columns = Columns::fit(inner.width);
    for (index, row) in panel.visible() {
        let offset = u16::try_from(index - panel.scroll()).unwrap_or(u16::MAX);
        let line_area = Rect { y: inner.y + offset, height: 1, ..inner };
        let highlight = (index == panel.cursor()).then_some(if active { theme.cursor } else { theme.cursor_inactive });
        draw_row(row, &columns, line_area, theme, highlight, now, buf);
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
        let reserved = [size, date].into_iter().flatten().map(|w| w + GAP).sum::<u16>() + MARKER_WIDTH;
        Self { name: width.saturating_sub(reserved), size, date }
    }

    fn constraints(&self) -> Vec<Constraint> {
        let mut constraints = vec![Constraint::Length(MARKER_WIDTH), Constraint::Length(self.name)];
        constraints.extend(self.size.into_iter().flat_map(|w| [Constraint::Length(GAP), Constraint::Length(w)]));
        constraints.extend(self.date.into_iter().flat_map(|w| [Constraint::Length(GAP), Constraint::Length(w)]));
        constraints
    }
}

fn draw_row(
    row: Row<'_>,
    columns: &Columns,
    area: Rect,
    theme: &Theme,
    highlight: Option<Style>,
    now: &Zoned,
    buf: &mut Buffer,
) {
    let (marker, name, size, date, style) = match row {
        Row::Parent => (' ', PARENT_NAME.to_owned(), PARENT_LABEL.to_owned(), String::new(), theme.dir),
        Row::Entry(entry) => (
            marker(entry),
            entry.name.clone(),
            if entry.is_dir() { DIR_LABEL.to_owned() } else { format::size(entry.size) },
            format::modified(entry.modified, now),
            style_of(entry, theme),
        ),
    };

    let cells = Layout::horizontal(columns.constraints()).split(area);
    let style = highlight.map_or(style, |h| style.patch(h));
    let meta = highlight.map_or(theme.meta, |h| theme.meta.patch(h));

    if let Some(h) = highlight {
        buf.set_style(area, h);
    }
    Line::styled(marker.to_string(), style).render(cells[0], buf);
    Line::styled(format::truncate_end(&name, usize::from(columns.name)), style).render(cells[1], buf);
    let mut rest = cells.iter().skip(FIRST_META_CELL).step_by(META_STRIDE);
    if columns.size.is_some()
        && let Some(cell) = rest.next()
    {
        Line::styled(size, meta).right_aligned().render(*cell, buf);
    }
    if columns.date.is_some()
        && let Some(cell) = rest.next()
    {
        Line::styled(date, meta).render(*cell, buf);
    }
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

fn style_of(entry: &Entry, theme: &Theme) -> Style {
    match entry.kind {
        EntryKind::Dir => theme.dir,
        EntryKind::Symlink { .. } => theme.link,
        EntryKind::Other => theme.other,
        EntryKind::File if entry.executable => theme.executable,
        EntryKind::File => theme.file,
    }
}

fn footer<'a>(panel: &Panel, theme: &Theme) -> Line<'a> {
    let count = panel.received();
    match panel.listing() {
        Listing::Loading => Line::styled(format!(" loading… {count} "), theme.footer),
        Listing::Ready => Line::styled(format!(" {count} items "), theme.footer),
        Listing::Failed(error) => Line::styled(format!(" {error} "), theme.notice),
    }
}
