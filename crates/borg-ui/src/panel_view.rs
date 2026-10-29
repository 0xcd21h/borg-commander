use borg_i18n::tr;
use std::{path::Path, time::Instant};

use borg_core::{Find, Listing, ListingMode, Panel, PanelMotion, Query, Row};
use borg_vfs::{Entry, EntryKind};
use jiff::Zoned;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Widget},
};

use unicode_width::UnicodeWidthStr;

use crate::{
    CursorShape, format,
    fx::{self, Gradient},
    icons, kinds,
    look::Look,
    theme::Styles,
};

const SIZE_WIDTH: u16 = 6;
const DATE_WIDTH: u16 = 12;
const GIT_WIDTH: u16 = 2;
const NO_MARKER: char = ' ';
const FIND_FIELD: usize = 18;
const FIND_PAD: usize = 1;
pub(crate) const ICON_GAP: char = ' ';
const GAP: u16 = 1;
const SHOW_DATE_FROM: u16 = 40;
const SHOW_SIZE_FROM: u16 = 24;
const SHOW_GIT_FROM: u16 = 30;
const SHOW_GIT_BRIEF_FROM: u16 = 20;
const PAD_FROM: u16 = 20;
const PADDING: u16 = 1;
const TITLE_PADDING: usize = 2;
const CRUMB_SEPARATOR: &str = " › ";
const CONTAINER_MARK: char = '▣';
const PARENT_NAME: &str = "..";
const COLUMN_SEPARATOR: &str = "│";
const LEFT_CAP: char = '▐';
const RIGHT_CAP: char = '▌';
const GLOW_FALLOFF: f32 = 0.5;
const COMET_WIDTH: u16 = 14;
const PROGRESS_FILL: char = '━';
const PROGRESS_TRACK: char = '─';
const PROGRESS_TRACK_FADE: f32 = 0.4;
const SEVEN: usize = 7;
const NINE: usize = 9;
const ASSIMILATE_FROM: usize = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    pub label: String,
    pub fraction: Option<f32>,
}

pub struct PanelScene<'a> {
    pub state: &'a borg_core::AppState,
    pub expanded: bool,
    pub panel: &'a Panel,
    pub client_of_remote: bool,
    pub active: bool,
    pub area: Rect,
    pub motion: &'a PanelMotion,
    pub activity: Option<Activity>,
    /// Shown once the panel's listing or footer activity has taken a while.
    pub spinner: Option<crate::spinner::Spinner>,
    pub icons: bool,
    pub icon_spacing: u8,
    pub find: Option<(&'a Find, (usize, usize))>,
    pub home: Option<&'a Path>,
    pub clock: &'a Zoned,
    pub now: Instant,
}

#[derive(Clone, Copy)]
struct Body {
    inner: Rect,
    content: Rect,
}

pub fn geometry(area: Rect) -> (Rect, Rect) {
    let body = Body::of(Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    });
    (body.inner, body.content)
}

impl Body {
    fn of(inner: Rect) -> Self {
        let gutter = if inner.width >= PAD_FROM { PADDING } else { 0 };
        let content = Rect { x: inner.x + gutter, width: inner.width.saturating_sub(gutter * 2), ..inner };
        Self { inner, content }
    }

    fn row(self, slot: usize) -> Option<(Rect, Rect)> {
        let offset = u16::try_from(slot).ok().filter(|offset| *offset < self.inner.height)?;
        let y = self.inner.y + offset;
        Some((Rect { y, height: 1, ..self.inner }, Rect { y, height: 1, ..self.content }))
    }

    fn padded(self) -> bool {
        self.content.width < self.inner.width
    }
}

pub fn draw_panel(scene: &PanelScene<'_>, look: &Look<'_>, buf: &mut Buffer) -> Option<Position> {
    if scene.panel.is_results() {
        crate::results_view::draw(scene, look, buf);
        return None;
    }
    let styles = look.styles();
    let area = scene.area;
    let (border, title_style) =
        if scene.active { (styles.border_active, styles.title_active) } else { (styles.border, styles.title) };
    let title = title(scene, title_style, styles.separator, styles.cursor);
    let (footer, shiny) = match scene.find {
        Some((find, counts)) => (find_bar(find, counts, usize::from(area.width).saturating_sub(4), look), None),
        None => footer(scene, look),
    };
    let title_width = title.width();
    let footer_width = footer.width();
    let block = Block::bordered()
        .border_type(look.theme.border.into())
        .style(styles.panel)
        .border_style(border)
        .title(title)
        .title_bottom(footer);

    let body = Body::of(block.inner(area));
    block.render(area, buf);

    draw_body(scene, look, body, border, buf);

    let edge = |y: u16, width: usize| Rect { x: area.x + 1, y, width: u16::try_from(width).unwrap_or(0), height: 1 };
    decorate(scene, look, edge(area.y, title_width), buf);
    if let Some(gradient) = shiny {
        fx::gradient_text(buf, edge(area.bottom().saturating_sub(1), footer_width), gradient);
    }
    if scene.spinner.is_some() {
        match scene.activity.as_ref().and_then(|activity| activity.fraction) {
            Some(fraction) => draw_progress(look, area, footer_width, fraction, buf),
            None => draw_comet(look, area, footer_width, buf),
        }
    }
    scene.find.map(|(find, _)| {
        let typed: String = find.input.text().chars().take(find.input.cursor_chars()).collect();
        let column = tr!(panel_find).width() + FIND_PAD + typed.width() + 1;
        Position::new(area.x + u16::try_from(column).unwrap_or(u16::MAX), area.bottom().saturating_sub(1))
    })
}

/// Quick find as an input in the bottom border: a highlighted label, a wide field holding
/// the query and the cursor, the match count, and the keys that work while finding.
fn find_bar<'a>(find: &Find, (current, total): (usize, usize), room: usize, look: &Look<'_>) -> Line<'a> {
    let styles = look.styles();
    let text = find.input.text();
    let count = if text.is_empty() { String::new() } else { format!(" {current}/{total} ") };
    let count_style = if total == 0 && !text.is_empty() { styles.notice } else { styles.footer };
    let field = (text.width() + FIND_PAD * 2 + 1)
        .max(FIND_FIELD)
        .min(room.saturating_sub(tr!(panel_find).width() + count.width()));
    let input = format!("{:pad$}{text}", "", pad = FIND_PAD);
    let mut spans = vec![
        Span::styled(tr!(panel_find), styles.cursor),
        Span::styled(format::pad_end(&input, field), styles.input),
        Span::styled(count, count_style),
    ];
    let used: usize = spans.iter().map(Span::width).sum();
    if used + tr!(panel_find_hint).width() <= room {
        spans.push(Span::styled(tr!(panel_find_hint), styles.footer));
    }
    Line::from(spans)
}

/// The folder path as breadcrumbs. Container and remote panels lead with their source,
/// so they never pass for a local folder.
fn title<'a>(scene: &PanelScene<'_>, style: Style, separator: Style, badge: Style) -> Line<'a> {
    let mount = scene.panel.mount.as_ref();
    let label = mount
        .map(|mount| format!(" {CONTAINER_MARK} {} ", mount.name))
        .or_else(|| {
            scene.panel.remote_mount.as_ref().map(|remote| {
                format!(
                    " ◈ Host {}:{} ",
                    scene.state.remote_host_name(&remote.machine),
                    scene.state.remote_session_name(&remote.machine, &remote.session, &remote.name)
                )
            })
        })
        .or_else(|| scene.client_of_remote.then(|| " ◈ Client ".to_owned()));
    let reserved = label.as_ref().map_or(0, |label| label.width() + 1);
    let width = usize::from(scene.area.width).saturating_sub(TITLE_PADDING * 2 + reserved);
    let home = if mount.is_some() || scene.panel.remote_mount.is_some() { None } else { scene.home };
    let crumbs = format::fit_crumbs(format::crumbs(scene.panel.dir(), home), width, CRUMB_SEPARATOR);
    let last = crumbs.len().saturating_sub(1);
    let separator = Style { fg: separator.fg, ..style.remove_modifier(Modifier::BOLD) };
    let spans = crumbs.into_iter().enumerate().flat_map(|(index, crumb)| {
        let lead = (index > 0).then(|| Span::styled(CRUMB_SEPARATOR, separator));
        let crumb_style = if index == last { style } else { style.remove_modifier(Modifier::BOLD) };
        lead.into_iter().chain(std::iter::once(Span::styled(crumb, crumb_style)))
    });
    let pad = || Span::styled(" ", style);
    let badge = label.into_iter().flat_map(|label| [Span::styled(label, badge), pad()]);
    Line::from(std::iter::once(pad()).chain(badge).chain(spans).chain(std::iter::once(pad())).collect::<Vec<_>>())
}

fn footer<'a>(scene: &PanelScene<'_>, look: &Look<'_>) -> (Line<'a>, Option<Gradient>) {
    let styles = look.styles();
    let panel = scene.panel;
    let marked = panel.marked_count();
    let total = panel.entries().len();
    if marked > 0 && scene.activity.is_none() {
        let bytes: u64 = panel.marked().filter(|e| !e.is_dir()).map(|e| e.size).sum();
        let summary = if bytes > 0 {
            tr!(marked, format::size(bytes), marked = marked)
        } else {
            tr!(marked_total, marked = marked)
        };
        let egg = match (marked, total) {
            (SEVEN, NINE) => Some(tr!(seven_of_nine).to_owned()),
            (marked, total) if marked == total && total >= ASSIMILATE_FROM => {
                Some(tr!(all_assimilated, format::size(bytes), total = total))
            }
            _ => None,
        };
        let shiny = egg.is_some().then(|| look.accent());
        return (Line::styled(format!(" {} ", egg.unwrap_or(summary)), styles.marked), shiny);
    }

    let spinning = |spinner: crate::spinner::Spinner, text: String| {
        let pad = || Span::styled(" ", styles.footer);
        Line::from([vec![pad()], spinner.spans(&text, look, styles.footer), vec![pad()]].concat())
    };
    let line = match (scene.spinner, &scene.activity, panel.listing()) {
        (Some(spinner), Some(activity), _) => spinning(spinner, activity.label.clone()),
        (Some(spinner), None, _) if panel.loading() => spinning(spinner, tr!(loading, panel.received())),
        (_, _, Listing::Ready | Listing::Loading) => {
            let git = if let Some(repository) = panel.git_repository() {
                tr!(panel_view_git, repository.branch)
            } else if panel.git.error.is_some() {
                tr!(git_unavailable).into()
            } else {
                String::new()
            };
            Line::styled(format!(" {}{git} ", format::count(panel.received(), format::Count::Item)), styles.footer)
        }
        (_, _, Listing::Failed(error)) => Line::styled(format!(" {error} "), styles.notice),
    };
    (line, None)
}

fn decorate(scene: &PanelScene<'_>, look: &Look<'_>, title: Rect, buf: &mut Buffer) {
    if !scene.active {
        return;
    }
    let effects = look.theme.effects;
    if let Some(pair) = effects.border_gradient {
        fx::gradient_border(buf, scene.area, look.gradient(pair).brightened(look.breath()));
    }
    if let Some(pair) = effects.title_gradient {
        fx::gradient_text(buf, title, look.gradient(pair));
    }
}

fn draw_comet(look: &Look<'_>, area: Rect, footer_width: usize, buf: &mut Buffer) {
    let start = area.x + 1 + u16::try_from(footer_width).unwrap_or(u16::MAX).saturating_add(GAP);
    let room = area.right().saturating_sub(start + 1).min(COMET_WIDTH);
    let y = area.bottom().saturating_sub(1);
    let gradient = look.accent();
    let span = f32::from(room.saturating_sub(1).max(1));
    for (x, (glyph, intensity)) in (start..start + room).zip(fx::comet(look.t, usize::from(room))) {
        if intensity <= 0.0 {
            continue;
        }
        let cell = &mut buf[(x, y)];
        let base = look.palette.foreground_of(cell.fg);
        let color = base.mix(gradient.at(f32::from(x - start) / span), intensity);
        cell.set_char(glyph).set_fg(color.into());
    }
}

fn draw_progress(look: &Look<'_>, area: Rect, footer_width: usize, fraction: f32, buf: &mut Buffer) {
    let start = area.x + 1 + u16::try_from(footer_width).unwrap_or(u16::MAX).saturating_add(GAP);
    let room = area.right().saturating_sub(start + GAP + 1);
    let y = area.bottom().saturating_sub(1);
    let filled = (fraction.clamp(0.0, 1.0) * f32::from(room)).round() as u16;
    let gradient = look.accent();
    let track = look.fg(look.styles().border).mix(look.background(), PROGRESS_TRACK_FADE);
    let span = f32::from(room.saturating_sub(1).max(1));
    for (offset, x) in (start..start + room).enumerate() {
        let lit = u16::try_from(offset).unwrap_or(u16::MAX) < filled;
        let (glyph, color) =
            if lit { (PROGRESS_FILL, gradient.at(offset as f32 / span)) } else { (PROGRESS_TRACK, track) };
        buf[(x, y)].set_char(glyph).set_fg(color.into());
    }
}

fn draw_body(scene: &PanelScene<'_>, look: &Look<'_>, body: Body, border: Style, buf: &mut Buffer) {
    let scroll = scene.motion.scroll(scene.now, scene.panel.scroll());
    match scene.panel.mode() {
        ListingMode::Full => draw_full(scene, look, body, scroll, buf),
        ListingMode::Brief => draw_brief(scene, look, body, scroll, border, buf),
    }
    if scene.panel.listing() == &Listing::Ready && scene.panel.entries().is_empty() {
        draw_empty(scene, look, body, buf);
    }
}

fn draw_empty(scene: &PanelScene<'_>, look: &Look<'_>, body: Body, buf: &mut Buffer) {
    let used = u16::try_from(scene.panel.len()).unwrap_or(u16::MAX);
    let art =
        Rect { y: body.inner.y.saturating_add(used), height: body.inner.height.saturating_sub(used), ..body.inner };
    if art.is_empty() {
        return;
    }
    let caption = Rect { y: art.y + art.height / 2, height: 1, ..art };
    if remembrance(scene.clock) {
        // On the day Sir Patrick Stewart died, empty folders keep still and remember him.
        let width = usize::from(art.width.saturating_sub(2)).max(1);
        let lines = [(tr!(tribute_memory), look.styles().meta), (tr!(tribute_line), look.styles().footer)]
            .into_iter()
            .flat_map(|(text, style)| {
                crate::activity_view::wrap(text, width).into_iter().map(move |line| (line, style))
            });
        for (offset, (text, style)) in (2..).zip(lines) {
            let row = Rect { y: caption.y.saturating_add(offset), ..caption };
            if row.y < art.bottom() {
                Line::styled(format::truncate_end(&text, width), style).centered().render(row, buf);
            }
        }
    } else {
        let t = scene.now.saturating_duration_since(scene.state.epoch).as_secs_f32();
        if let Some(progress) = borg_core::empty_glitch(t) {
            fx::draw_interference(buf, art, t, progress, look.background());
        }
    }
    Line::styled(tr!(panel_empty), look.styles().meta).centered().render(caption, buf);
}

/// October 28, the day Sir Patrick Stewart died in 2026.
fn remembrance(clock: &Zoned) -> bool {
    (clock.month(), clock.day()) == (10, 28)
}

fn visible<'p>(panel: &'p Panel, scroll: usize) -> impl Iterator<Item = (usize, usize, Row<'p>)> {
    let end = panel.len().min(scroll + panel.capacity());
    (scroll..end).filter_map(move |index| panel.row(index).map(|row| (index - scroll, index, row)))
}

fn draw_full(scene: &PanelScene<'_>, look: &Look<'_>, body: Body, scroll: usize, buf: &mut Buffer) {
    let panel = scene.panel;
    let columns = Columns::fit(body.content.width, panel.git_repository().is_some());
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
struct CursorRow {
    full: Rect,
    content: Rect,
    fill: Style,
    pill: bool,
}

fn paint_cursor(active: bool, cursor: CursorRow, body: Body, look: &Look<'_>, buf: &mut Buffer) {
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

fn draw_brief(scene: &PanelScene<'_>, look: &Look<'_>, body: Body, scroll: usize, border: Style, buf: &mut Buffer) {
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
        let status_width =
            if panel.git_repository().is_some() && area.width >= SHOW_GIT_BRIEF_FROM { GIT_WIDTH + GAP } else { 0 };
        draw_name(
            row,
            Rect { width: area.width.saturating_sub(status_width), ..area },
            tone,
            &Decor::of(scene, look),
            buf,
        );
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

fn draw_separators(panel: &Panel, body: Body, border: Style, buf: &mut Buffer) {
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
struct Tone {
    name: Style,
    meta: Style,
    fill: Option<Style>,
}

impl Tone {
    fn of(panel: &Panel, row: Row<'_>, index: usize, active: bool, styles: &Styles<Style>) -> Self {
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

struct Columns {
    name: u16,
    git: Option<u16>,
    size: Option<u16>,
    date: Option<u16>,
}

impl Columns {
    fn fit(width: u16, repository: bool) -> Self {
        let date = (width >= SHOW_DATE_FROM).then_some(DATE_WIDTH);
        let size = (width >= SHOW_SIZE_FROM).then_some(SIZE_WIDTH);
        let git = (repository && width >= SHOW_GIT_FROM).then_some(GIT_WIDTH);
        let reserved = [git, size, date].into_iter().flatten().map(|w| w + GAP).sum::<u16>();
        Self { name: width.saturating_sub(reserved), git, size, date }
    }

    fn split(&self, area: Rect) -> [Option<Rect>; 4] {
        let mut x = area.x;
        let mut next = |width: Option<u16>, gap: u16| {
            width.map(|width| {
                let rect = Rect { x: x + gap, width, ..area };
                x = rect.right();
                rect
            })
        };
        [next(Some(self.name), 0), next(self.git, GAP), next(self.size, GAP), next(self.date, GAP)]
    }
}

fn draw_full_row(
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

    let [name, git_cell, size_cell, date_cell] = columns.split(area);
    if let Some(cell) = name {
        draw_name(row, cell, tone, &Decor::of(scene, look), buf);
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

fn draw_git(row: Row<'_>, panel: &Panel, area: Rect, tone: Tone, look: &Look<'_>, buf: &mut Buffer) {
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

struct Decor {
    icons: bool,
    gap: String,
    query: Option<Query>,
    highlight: Style,
}

impl Decor {
    fn of(scene: &PanelScene<'_>, look: &Look<'_>) -> Self {
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

fn draw_name(row: Row<'_>, area: Rect, tone: Tone, decor: &Decor, buf: &mut Buffer) {
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

fn highlighted<'a>(text: &str, hits: &[std::ops::Range<usize>], plain: Style, lit: Style) -> Vec<Span<'a>> {
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

pub(crate) fn style_of(entry: &Entry, styles: &Styles<Style>) -> Style {
    match entry.kind {
        EntryKind::Dir => styles.dir,
        EntryKind::Symlink { .. } => styles.link,
        EntryKind::Other => styles.other,
        EntryKind::File if entry.executable => styles.executable,
        EntryKind::File => kinds::classify(&entry.name).map_or(styles.file, |kind| kinds::style(kind, styles)),
    }
}
