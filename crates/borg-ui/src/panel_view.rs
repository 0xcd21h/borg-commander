use std::{path::Path, time::Instant};

use borg_core::{EMPTY_ART_WINDOW, Find, Listing, ListingMode, Panel, PanelMotion, Query, Row, SlideDirection};
use borg_vfs::{Entry, EntryKind};
use jiff::Zoned;
use ratatui::{
    buffer::{Buffer, Cell},
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
const BAR_WIDTH: u16 = 5;
const NO_MARKER: char = ' ';
const FIND_LABEL: &str = " find: ";
const ICON_GAP: char = ' ';
const GAP: u16 = 1;
const SHOW_DATE_FROM: u16 = 40;
const SHOW_SIZE_FROM: u16 = 24;
const SHOW_BARS_FROM: u16 = 50;
const PAD_FROM: u16 = 20;
const PADDING: u16 = 1;
const TITLE_PADDING: usize = 2;
const CRUMB_SEPARATOR: &str = " › ";
const PARENT_NAME: &str = "..";
const PARENT_LABEL: &str = "UP";
const DIR_LABEL: &str = "DIR";
const COLUMN_SEPARATOR: &str = "│";
const EMPTY_CAPTION: &str = "empty";
const LEFT_CAP: char = '▐';
const RIGHT_CAP: char = '▌';
const BAR_FULL: char = '━';
const BAR_HALF: char = '╸';
const BAR_STEPS: usize = 2;
const BAR_VISIBILITY: f32 = 0.7;
const SLIDE_SHARE: f32 = 0.3;
const SLIDE_FADE: f32 = 0.8;
const GLOW_FALLOFF: f32 = 0.5;
const COMET_WIDTH: u16 = 14;
const STAR_PACE: f32 = 0.35;
const SEVEN: usize = 7;
const NINE: usize = 9;
const ASSIMILATE_FROM: usize = 2;
const SEVEN_OF_NINE: &str = "Seven of Nine";

pub struct PanelScene<'a> {
    pub panel: &'a Panel,
    pub active: bool,
    pub area: Rect,
    pub motion: &'a PanelMotion,
    pub activity: Option<&'a str>,
    pub delight: bool,
    pub icons: bool,
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
    let styles = look.styles();
    let area = scene.area;
    let (border, title_style) =
        if scene.active { (styles.border_active, styles.title_active) } else { (styles.border, styles.title) };
    let title = title(scene, title_style, styles.separator);
    let (footer, shiny) = match scene.find {
        Some((find, counts)) => (find_bar(find, counts, look), None),
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

    let scroll = scene.motion.scroll(scene.now, scene.panel.scroll());
    match scene.panel.mode() {
        ListingMode::Full => draw_full(scene, look, body, scroll, buf),
        ListingMode::Brief => draw_brief(scene, look, body, scroll, border, buf),
    }
    if scene.panel.listing() == &Listing::Ready && scene.panel.entries().is_empty() {
        draw_empty(scene, look, body, buf);
    }

    let edge = |y: u16, width: usize| Rect { x: area.x + 1, y, width: u16::try_from(width).unwrap_or(0), height: 1 };
    decorate(scene, look, edge(area.y, title_width), buf);
    if let Some(gradient) = shiny {
        fx::gradient_text(buf, edge(area.bottom().saturating_sub(1), footer_width), gradient);
    }
    if scene.activity.is_some() || scene.panel.listing() == &Listing::Loading {
        draw_comet(look, area, footer_width, buf);
    }
    if let Some((direction, progress)) = scene.motion.slide(scene.now) {
        slide(body.inner, direction, progress, look, buf);
    }
    scene.find.map(|(find, _)| {
        let typed: String = find.input.text().chars().take(find.input.cursor_chars()).collect();
        let column = FIND_LABEL.width() + typed.width() + 1;
        Position::new(area.x + u16::try_from(column).unwrap_or(u16::MAX), area.bottom().saturating_sub(1))
    })
}

fn find_bar<'a>(find: &Find, (current, total): (usize, usize), look: &Look<'_>) -> Line<'a> {
    let styles = look.styles();
    let count = if find.input.text().is_empty() { String::new() } else { format!(" {current}/{total} ") };
    let count_style = if total == 0 && !find.input.text().is_empty() { styles.notice } else { styles.footer };
    Line::from(vec![
        Span::styled(FIND_LABEL, styles.footer.fg(look.accent().from.into())),
        Span::styled(format!("{} ", find.input.text()), styles.input),
        Span::styled(count, count_style),
    ])
}

fn title<'a>(scene: &PanelScene<'_>, style: Style, separator: Style) -> Line<'a> {
    let width = usize::from(scene.area.width).saturating_sub(TITLE_PADDING * 2);
    let crumbs = format::fit_crumbs(format::crumbs(scene.panel.dir(), scene.home), width, CRUMB_SEPARATOR);
    let last = crumbs.len().saturating_sub(1);
    let spans = crumbs.into_iter().enumerate().flat_map(|(index, crumb)| {
        let lead = (index > 0).then(|| Span::styled(CRUMB_SEPARATOR, separator));
        let crumb_style = if index == last { style } else { style.remove_modifier(Modifier::BOLD) };
        lead.into_iter().chain(std::iter::once(Span::styled(crumb, crumb_style)))
    });
    let pad = || Span::styled(" ", style);
    Line::from(std::iter::once(pad()).chain(spans).chain(std::iter::once(pad())).collect::<Vec<_>>())
}

fn footer<'a>(scene: &PanelScene<'_>, look: &Look<'_>) -> (Line<'a>, Option<Gradient>) {
    let styles = look.styles();
    let panel = scene.panel;
    let marked = panel.marked_count();
    let total = panel.entries().len();
    if marked > 0 {
        let bytes: u64 = panel.marked().filter(|e| !e.is_dir()).map(|e| e.size).sum();
        let summary =
            if bytes > 0 { format!("{marked} marked · {}", format::size(bytes)) } else { format!("{marked} marked") };
        let egg = match (scene.delight, marked, total) {
            (true, SEVEN, NINE) => Some(SEVEN_OF_NINE.to_owned()),
            (true, marked, total) if marked == total && total >= ASSIMILATE_FROM => {
                Some(format!("all {total} assimilated · {}", format::size(bytes)))
            }
            _ => None,
        };
        let shiny = egg.is_some().then(|| look.accent());
        return (Line::styled(format!(" {} ", egg.unwrap_or(summary)), styles.marked), shiny);
    }

    let spinning = |text: String| {
        let glyph = Span::styled(fx::orbit(look.t).to_string(), styles.footer.fg(look.accent().from.into()));
        Line::from(vec![Span::styled(" ", styles.footer), glyph, Span::styled(format!(" {text} "), styles.footer)])
    };
    let line = match (scene.activity, panel.listing()) {
        (Some(label), _) => spinning(format!("{label}…")),
        (None, Listing::Loading) => spinning(format!("loading… {}", panel.received())),
        (None, Listing::Ready) => Line::styled(format!(" {} ", format::count(panel.received(), "item")), styles.footer),
        (None, Listing::Failed(error)) => Line::styled(format!(" {error} "), styles.notice),
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

fn slide(inner: Rect, direction: SlideDirection, progress: f32, look: &Look<'_>, buf: &mut Buffer) {
    let distance = f32::from(inner.width) * SLIDE_SHARE * (1.0 - progress);
    let dx = match direction {
        SlideDirection::Forward => distance.round() as i32,
        SlideDirection::Back => -(distance.round() as i32),
    };
    let mut fill = Cell::default();
    fill.set_style(look.styles().panel);
    fx::shift(buf, inner, dx, &fill);
    fx::fade_to_background(buf, inner, (1.0 - progress) * SLIDE_FADE, look.palette);
}

fn draw_empty(scene: &PanelScene<'_>, look: &Look<'_>, body: Body, buf: &mut Buffer) {
    let used = u16::try_from(scene.panel.len()).unwrap_or(u16::MAX);
    let art =
        Rect { y: body.inner.y.saturating_add(used), height: body.inner.height.saturating_sub(used), ..body.inner };
    if art.is_empty() {
        return;
    }
    if scene.delight {
        let drift = scene.now.saturating_duration_since(scene.motion.arrived).min(EMPTY_ART_WINDOW);
        fx::draw_stars(buf, art, drift.as_secs_f32() * STAR_PACE, look.background());
    }
    let caption = Rect { y: art.y + art.height / 2, height: 1, ..art };
    Line::styled(EMPTY_CAPTION, look.styles().meta).centered().render(caption, buf);
}

fn visible<'p>(panel: &'p Panel, scroll: usize) -> impl Iterator<Item = (usize, usize, Row<'p>)> {
    let end = panel.len().min(scroll + panel.capacity());
    (scroll..end).filter_map(move |index| panel.row(index).map(|row| (index - scroll, index, row)))
}

fn draw_full(scene: &PanelScene<'_>, look: &Look<'_>, body: Body, scroll: usize, buf: &mut Buffer) {
    let panel = scene.panel;
    let columns = Columns::fit(body.content.width);
    let mut cursor_row = None;
    for (slot, index, row) in visible(panel, scroll) {
        let Some((full, content)) = body.row(slot) else { continue };
        let tone = Tone::of(panel, row, index, scene.active, look.styles());
        if let Some(fill) = tone.fill {
            let pill = look.theme.effects.cursor == CursorShape::Pill && body.padded();
            buf.set_style(if pill { content } else { full }, fill);
            cursor_row = Some((full, content, fill, pill));
        }
        let bar = bar_fraction(row, panel.largest());
        draw_full_row(row, &columns, content, tone, scene, bar, look, buf);
    }
    if let Some((full, content, fill, pill)) = cursor_row {
        paint_cursor(scene.active, full, content, fill, pill, body, look, buf);
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_cursor(
    active: bool,
    full: Rect,
    content: Rect,
    fill: Style,
    pill: bool,
    body: Body,
    look: &Look<'_>,
    buf: &mut Buffer,
) {
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
        draw_name(row, area, tone, &Decor::of(scene, look), buf);
        if tone.fill.is_some()
            && let Some(pair) = look.theme.effects.cursor_gradient.filter(|_| scene.active)
        {
            fx::gradient_fill(buf, area, look.gradient(pair));
        }
    }

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
    bar: Option<u16>,
    size: Option<u16>,
    date: Option<u16>,
}

impl Columns {
    fn fit(width: u16) -> Self {
        let date = (width >= SHOW_DATE_FROM).then_some(DATE_WIDTH);
        let size = (width >= SHOW_SIZE_FROM).then_some(SIZE_WIDTH);
        let bar = (width >= SHOW_BARS_FROM).then_some(BAR_WIDTH);
        let reserved = [bar, size, date].into_iter().flatten().map(|w| w + GAP).sum::<u16>();
        Self { name: width.saturating_sub(reserved), bar, size, date }
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
        [next(Some(self.name), 0), next(self.bar, GAP), next(self.size, GAP), next(self.date, GAP)]
    }
}

fn bar_fraction(row: Row<'_>, largest: u64) -> Option<f32> {
    match row {
        Row::Entry(entry) if !entry.is_dir() && largest > 0 => {
            Some(((entry.size as f64).ln_1p() / (largest as f64).ln_1p()) as f32)
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_full_row(
    row: Row<'_>,
    columns: &Columns,
    area: Rect,
    tone: Tone,
    scene: &PanelScene<'_>,
    bar: Option<f32>,
    look: &Look<'_>,
    buf: &mut Buffer,
) {
    let (size, date) = match row {
        Row::Parent => (PARENT_LABEL.to_owned(), String::new()),
        Row::Entry(entry) => (
            if entry.is_dir() { DIR_LABEL.to_owned() } else { format::size(entry.size) },
            format::modified(entry.modified, scene.clock),
        ),
    };

    let [name, bar_cell, size_cell, date_cell] = columns.split(area);
    if let Some(cell) = name {
        draw_name(row, cell, tone, &Decor::of(scene, look), buf);
    }
    if let (Some(cell), Some(fraction)) = (bar_cell, bar) {
        draw_bar(cell, fraction, look, buf);
    }
    if let Some(cell) = size_cell {
        Line::styled(size, tone.meta).right_aligned().render(cell, buf);
    }
    if let Some(cell) = date_cell {
        Line::styled(date, tone.meta).render(cell, buf);
    }
}

fn draw_bar(area: Rect, fraction: f32, look: &Look<'_>, buf: &mut Buffer) {
    let steps = (fraction.clamp(0.0, 1.0) * f32::from(area.width) * BAR_STEPS as f32).round() as usize;
    let gradient = look.accent().dimmed(look.background(), BAR_VISIBILITY);
    let span = f32::from(area.width.saturating_sub(1).max(1));
    for (offset, x) in (area.left()..area.right()).enumerate() {
        let glyph = match steps.saturating_sub(offset * BAR_STEPS).min(BAR_STEPS) {
            0 => break,
            BAR_STEPS => BAR_FULL,
            _ => BAR_HALF,
        };
        buf[(x, area.y)].set_char(glyph).set_fg(gradient.at(offset as f32 / span).into());
    }
}

struct Decor {
    icons: bool,
    query: Option<Query>,
    highlight: Style,
}

impl Decor {
    fn of(scene: &PanelScene<'_>, look: &Look<'_>) -> Self {
        Self {
            icons: scene.icons,
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
        Row::Entry(entry) => entry.name.as_str(),
    };
    let lead = match (decor.icons, row) {
        (true, Row::Parent) => Some(icons::parent()),
        (true, Row::Entry(entry)) => Some(icons::icon(entry)),
        (false, _) => None,
    };
    let lead = match (lead, row) {
        (Some(icon), _) => {
            let color = icon.color.filter(|_| tone.fill.is_none());
            Span::styled(format!("{}{ICON_GAP}", icon.glyph), color.map_or(tone.name, |fg| tone.name.fg(fg)))
        }
        (None, Row::Parent) => Span::styled(NO_MARKER.to_string(), tone.name),
        (None, Row::Entry(entry)) => Span::styled(marker(entry).to_string(), tone.name),
    };
    let width = usize::from(area.width).saturating_sub(lead.width());
    let shown = format::truncate_end(name, width);
    let hits = match row {
        Row::Entry(entry) => decor.query.as_ref().and_then(|query| query.hits(entry)).unwrap_or_default(),
        Row::Parent => Vec::new(),
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

fn style_of(entry: &Entry, styles: &Styles<Style>) -> Style {
    match entry.kind {
        EntryKind::Dir => styles.dir,
        EntryKind::Symlink { .. } => styles.link,
        EntryKind::Other => styles.other,
        EntryKind::File if entry.executable => styles.executable,
        EntryKind::File => kinds::classify(&entry.name).map_or(styles.file, |kind| kinds::style(kind, styles)),
    }
}
