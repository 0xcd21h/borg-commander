//! The listing's rows: the full and brief layouts, the cursor, and each entry's columns, name
//! and marks.

use super::{
    BACKUP_MARK, BACKUP_WIDTH, Body, COLUMN_SEPARATOR, DATE_WIDTH, GAP, GIT_WIDTH, GLOW_FALLOFF, ICON_GAP, LEFT_CAP,
    NO_MARKER, PARENT_NAME, PanelScene, RIGHT_CAP, SHOW_DATE_FROM, SHOW_GIT_BRIEF_FROM, SHOW_GIT_FROM, SHOW_SIZE_FROM,
    SIZE_WIDTH,
};
use crate::{CursorShape, format, fx, icons, kinds, look::Look, theme::Styles};
use borg_core::{Panel, Query, Row};
use borg_i18n::tr;
use borg_vfs::{Entry, EntryKind};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

pub(super) fn visible<'p>(panel: &'p Panel, scroll: usize) -> impl Iterator<Item = (usize, usize, Row<'p>)> {
    let end = panel.len().min(scroll + panel.capacity());
    (scroll..end).filter_map(move |index| panel.row(index).map(|row| (index - scroll, index, row)))
}

pub(super) fn draw_full(scene: &PanelScene<'_>, look: &Look<'_>, body: Body, scroll: usize, buf: &mut Buffer) {
    let panel = scene.panel;
    let columns = Columns::fit(body.content.width, panel.git_repository().is_some(), panel.backup.marks());
    let mut cursor_row = None;
    for (slot, index, row) in visible(panel, scroll) {
        let Some((full, content)) = body.row(slot) else { continue };
        let tone = Tone::of(panel, row, index, scene.active, look.styles());
        if let Some(fill) = tone.fill {
            let pill = look.theme.effects.cursor == CursorShape::Pill && body.padded();
            buf.set_style(if pill { content } else { full }, fill);
            cursor_row = Some(CursorRow { full, content, fill, pill });
        }
        draw_full_row(row, &columns, content, tone, scene, look, buf);
    }
    if let Some(cursor) = cursor_row {
        paint_cursor(scene.active, cursor, body, look, buf);
    }
}

/// The cursor row, kept while the other rows are drawn so its gradient, caps and glow go on top.
#[derive(Clone, Copy)]
pub(super) struct CursorRow {
    pub(super) full: Rect,
    pub(super) content: Rect,
    pub(super) fill: Style,
    pub(super) pill: bool,
}

pub(super) fn paint_cursor(active: bool, cursor: CursorRow, body: Body, look: &Look<'_>, buf: &mut Buffer) {
    let CursorRow { full, content, fill, pill } = cursor;
    let effects = look.theme.effects;
    let gradient = effects.cursor_gradient.filter(|_| active).map(|pair| look.gradient(pair));
    if let Some(gradient) = gradient {
        fx::gradient_fill(buf, if pill { content } else { full }, gradient);
    }
    if pill && fill.bg.is_some() {
        let ends = gradient.map_or_else(|| [look.bg(fill); 2], |g| [g.from, g.to]);
        let panel = look.styles().panel;
        let caps = [(full.x, LEFT_CAP, ends[0]), (full.right().saturating_sub(1), RIGHT_CAP, ends[1])];
        for (x, glyph, color) in caps {
            buf[(x, full.y)].set_char(glyph).set_style(panel).set_fg(color.into());
        }
    }
    if active && effects.glow > 0.0 {
        let color = gradient.map_or_else(|| look.bg(fill), |g| g.at(0.5));
        let strength = effects.glow * look.breath();
        let halo = |offset: i32| {
            let y = i32::from(full.y) + offset;
            u16::try_from(y)
                .ok()
                .filter(|y| *y >= body.inner.y && *y < body.inner.bottom())
                .map(|y| Rect { y, ..content })
        };
        [-1, 1]
            .into_iter()
            .filter_map(halo)
            .for_each(|rect| fx::glow(buf, rect, color, strength * GLOW_FALLOFF, look.palette));
    }
}

pub(super) fn draw_brief(
    scene: &PanelScene<'_>,
    look: &Look<'_>,
    body: Body,
    scroll: usize,
    border: Style,
    buf: &mut Buffer,
) {
    let panel = scene.panel;
    let columns = panel.mode().columns();
    let constraints = (0..columns).map(|_| Constraint::Ratio(1, u32::try_from(columns).unwrap_or(1)));
    let cells = Layout::horizontal(constraints).split(body.content);
    let rows = panel.rows().max(1);

    for (slot, index, row) in visible(panel, scroll) {
        let Some(column) = cells.get(slot / rows) else { continue };
        let separator = u16::from(slot / rows + 1 < columns);
        let y = body.content.y + u16::try_from(slot % rows).unwrap_or(u16::MAX);
        let area = Rect { x: column.x, y, width: column.width.saturating_sub(separator), height: 1 };
        let tone = Tone::of(panel, row, index, scene.active, look.styles());
        let roomy = area.width >= SHOW_GIT_BRIEF_FROM;
        let status_width = if panel.git_repository().is_some() && roomy { GIT_WIDTH + GAP } else { 0 };
        let backup_width = if panel.backup.marks() && roomy { BACKUP_WIDTH + GAP } else { 0 };
        draw_name(
            row,
            Rect { width: area.width.saturating_sub(status_width + backup_width), ..area },
            tone,
            &Decor::of(scene, look),
            buf,
        );
        if backup_width > 0 {
            let x = area.right() - status_width - BACKUP_WIDTH;
            draw_backup(row, panel, Rect { x, width: BACKUP_WIDTH, ..area }, tone, buf);
        }
        if status_width > 0 {
            draw_git(row, panel, Rect { x: area.right() - GIT_WIDTH, width: GIT_WIDTH, ..area }, tone, look, buf);
        }
        if tone.fill.is_some()
            && let Some(pair) = look.theme.effects.cursor_gradient.filter(|_| scene.active)
        {
            fx::gradient_fill(buf, area, look.gradient(pair));
        }
    }

    draw_separators(panel, body, border, buf);
}

pub(super) fn draw_separators(panel: &Panel, body: Body, border: Style, buf: &mut Buffer) {
    let columns = panel.mode().columns();
    let constraints = (0..columns).map(|_| Constraint::Ratio(1, u32::try_from(columns).unwrap_or(1)));
    let cells = Layout::horizontal(constraints).split(body.content);
    for column in cells.iter().take(columns.saturating_sub(1)) {
        let x = column.x + column.width.saturating_sub(1);
        (body.content.y..body.content.bottom()).for_each(|y| {
            buf[(x, y)].set_symbol(COLUMN_SEPARATOR).set_style(border);
        });
    }
}

#[derive(Clone, Copy)]
pub(super) struct Tone {
    pub(super) name: Style,
    pub(super) meta: Style,
    pub(super) fill: Option<Style>,
}

impl Tone {
    pub(super) fn of(panel: &Panel, row: Row<'_>, index: usize, active: bool, styles: &Styles<Style>) -> Self {
        let marked = matches!(row, Row::Entry(entry) if panel.is_marked(entry));
        let base = match row {
            Row::Parent | Row::Group { .. } => styles.dir,
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

pub(super) struct Columns {
    pub(super) name: u16,
    pub(super) backup: Option<u16>,
    pub(super) git: Option<u16>,
    pub(super) size: Option<u16>,
    pub(super) date: Option<u16>,
}

impl Columns {
    pub(super) fn fit(width: u16, repository: bool, backup: bool) -> Self {
        let date = (width >= SHOW_DATE_FROM).then_some(DATE_WIDTH);
        let size = (width >= SHOW_SIZE_FROM).then_some(SIZE_WIDTH);
        let git = (repository && width >= SHOW_GIT_FROM).then_some(GIT_WIDTH);
        let backup = (backup && width >= SHOW_GIT_FROM).then_some(BACKUP_WIDTH);
        let reserved = [backup, git, size, date].into_iter().flatten().map(|w| w + GAP).sum::<u16>();
        Self { name: width.saturating_sub(reserved), backup, git, size, date }
    }

    pub(super) fn split(&self, area: Rect) -> [Option<Rect>; 5] {
        let mut x = area.x;
        let mut next = |width: Option<u16>, gap: u16| {
            width.map(|width| {
                let rect = Rect { x: x + gap, width, ..area };
                x = rect.right();
                rect
            })
        };
        [
            next(Some(self.name), 0),
            next(self.backup, GAP),
            next(self.git, GAP),
            next(self.size, GAP),
            next(self.date, GAP),
        ]
    }
}

pub(super) fn draw_full_row(
    row: Row<'_>,
    columns: &Columns,
    area: Rect,
    tone: Tone,
    scene: &PanelScene<'_>,
    look: &Look<'_>,
    buf: &mut Buffer,
) {
    let (size, date) = match row {
        Row::Parent | Row::Group { .. } => (tr!(up).to_owned(), String::new()),
        Row::Entry(entry) => (
            if entry.is_dir() {
                entry.usage.as_ref().map_or_else(
                    || tr!(dir).to_owned(),
                    |u| {
                        format!(
                            "{}{}{}",
                            if u.cached { "~" } else { "" },
                            if u.partial { "≥" } else { "" },
                            format::size(entry.size)
                        )
                    },
                )
            } else {
                format::size(entry.size)
            },
            format::modified(entry.modified, scene.clock),
        ),
    };

    let [name, backup_cell, git_cell, size_cell, date_cell] = columns.split(area);
    if let Some(cell) = name {
        draw_name(row, cell, tone, &Decor::of(scene, look), buf);
    }
    if let Some(cell) = backup_cell {
        draw_backup(row, scene.panel, cell, tone, buf);
    }
    if let Some(cell) = git_cell {
        draw_git(row, scene.panel, cell, tone, look, buf);
    }
    if let Some(cell) = size_cell {
        Line::styled(size, tone.meta).right_aligned().render(cell, buf);
    }
    if let Some(cell) = date_cell {
        Line::styled(date, tone.meta).render(cell, buf);
    }
}

pub(super) fn draw_backup(row: Row<'_>, panel: &Panel, area: Rect, tone: Tone, buf: &mut Buffer) {
    if let Row::Entry(entry) = row
        && panel.backup.entry(&entry.name).is_some()
    {
        Line::styled(BACKUP_MARK, tone.meta).render(area, buf);
    }
}

pub(super) fn draw_git(row: Row<'_>, panel: &Panel, area: Rect, tone: Tone, look: &Look<'_>, buf: &mut Buffer) {
    let Row::Entry(entry) = row else { return };
    let Some(status) = panel.git_repository().and_then(|repository| repository.status(&entry.path(panel.dir()))) else {
        return;
    };
    let styles = look.styles();
    let span = |code: char, staged: bool| {
        let color = if status.conflict() || code == 'D' {
            styles.notice.fg
        } else if code == '?' {
            styles.meta.fg
        } else if staged {
            styles.executable.fg
        } else {
            styles.marked.fg
        };
        Span::styled(code.to_string(), Style { fg: color, ..tone.meta }.add_modifier(Modifier::BOLD))
    };
    Line::from(vec![span(status.index, true), span(status.worktree, false)]).render(area, buf);
}

pub(super) struct Decor {
    pub(super) icons: bool,
    pub(super) gap: String,
    pub(super) query: Option<Query>,
    pub(super) highlight: Style,
}

impl Decor {
    pub(super) fn of(scene: &PanelScene<'_>, look: &Look<'_>) -> Self {
        Self {
            icons: scene.icons,
            gap: ICON_GAP.to_string().repeat(usize::from(scene.icon_spacing)),
            query: scene.find.map(|(find, _)| Query::parse(find.input.text())),
            highlight: Style::new()
                .fg(look.fg(look.styles().marked).into())
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        }
    }
}

pub(super) fn draw_name(row: Row<'_>, area: Rect, tone: Tone, decor: &Decor, buf: &mut Buffer) {
    if let Some(fill) = tone.fill {
        buf.set_style(area, fill);
    }
    let name = match row {
        Row::Parent => PARENT_NAME,
        Row::Group { label, .. } => label,
        Row::Entry(entry) => entry.name.as_str(),
    };
    let lead = match (decor.icons, row) {
        (_, Row::Group { .. }) => None,
        (true, Row::Parent) => Some(icons::parent()),
        (true, Row::Entry(entry)) => Some(icons::icon(entry)),
        (false, Row::Parent) => Some(icons::portable_parent()),
        (false, Row::Entry(entry)) => Some(icons::portable(entry)),
    };
    let lead = match (lead, row) {
        (Some(icon), _) => {
            let color = icon.color.filter(|_| tone.fill.is_none());
            Span::styled(format!("{}{}", icon.glyph, decor.gap), color.map_or(tone.name, |fg| tone.name.fg(fg)))
        }
        (None, Row::Group { .. } | Row::Parent) => Span::styled(NO_MARKER.to_string(), tone.name),
        (None, Row::Entry(entry)) => Span::styled(marker(entry).to_string(), tone.name),
    };
    let width = usize::from(area.width).saturating_sub(lead.width());
    let shown = format::truncate_end(name, width);
    let hits = match row {
        Row::Entry(entry) => decor.query.as_ref().and_then(|query| query.hits(entry)).unwrap_or_default(),
        Row::Parent | Row::Group { .. } => Vec::new(),
    };
    let highlight = if tone.fill.is_some() {
        tone.name.add_modifier(decor.highlight.add_modifier)
    } else {
        tone.name.patch(decor.highlight)
    };
    let spans = std::iter::once(lead).chain(highlighted(&shown, &hits, tone.name, highlight));
    Line::from(spans.collect::<Vec<_>>()).render(area, buf);
}

pub(super) fn highlighted<'a>(text: &str, hits: &[std::ops::Range<usize>], plain: Style, lit: Style) -> Vec<Span<'a>> {
    let mut spans = Vec::new();
    let mut at = 0;
    for hit in hits
        .iter()
        .filter(|hit| hit.end <= text.len() && text.is_char_boundary(hit.start) && text.is_char_boundary(hit.end))
    {
        spans.push(Span::styled(text[at..hit.start].to_owned(), plain));
        spans.push(Span::styled(text[hit.start..hit.end].to_owned(), lit));
        at = hit.end;
    }
    spans.push(Span::styled(text[at..].to_owned(), plain));
    spans
}

pub(super) fn marker(entry: &Entry) -> char {
    match entry.kind {
        EntryKind::Dir => '/',
        EntryKind::Symlink { to_dir: true } => '~',
        EntryKind::Symlink { to_dir: false } => '@',
        EntryKind::Other => '?',
        EntryKind::File if entry.executable => '*',
        EntryKind::File => ' ',
    }
}

pub(crate) fn style_of(entry: &Entry, styles: &Styles<Style>) -> Style {
    match entry.kind {
        EntryKind::Dir => styles.dir,
        EntryKind::Symlink { .. } => styles.link,
        EntryKind::Other => styles.other,
        EntryKind::File if entry.executable => styles.executable,
        EntryKind::File => kinds::classify(&entry.name).map_or(styles.file, |kind| kinds::style(kind, styles)),
    }
}
