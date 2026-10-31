//! A file panel: its frame, title and footer, the find bar, the progress of what runs in it, and
//! the listing, or what an empty panel shows instead.

use borg_i18n::tr;
use std::{path::Path, time::Instant};

use borg_core::{Find, Listing, ListingMode, Panel, PanelMotion};
use jiff::Zoned;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Widget},
};

use unicode_width::UnicodeWidthStr;

use crate::{
    format,
    fx::{self, Gradient},
    look::Look,
};
mod rows;
pub(crate) use rows::style_of;
use rows::{draw_brief, draw_full};

const SIZE_WIDTH: u16 = 6;
const DATE_WIDTH: u16 = 12;
const GIT_WIDTH: u16 = 2;
const BACKUP_WIDTH: u16 = 1;
/// Marks an entry Time Machine leaves out of backups on its own account.
const BACKUP_MARK: &str = "⊘";
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
            let backup = if panel.backup.folder.excluded() {
                tr!(backup_footer_folder).into()
            } else if panel.backup.marks() {
                tr!(backup_footer_some, count = format::count(panel.backup.entries.len(), format::Count::Item))
            } else {
                String::new()
            };
            let items = format::count(panel.received(), format::Count::Item);
            Line::styled(format!(" {items}{git}{backup} "), styles.footer)
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
    if remembrance(scene.clock) {
        return draw_remembrance(scene, look, art, buf);
    }
    let t = scene.now.saturating_duration_since(scene.state.epoch).as_secs_f32();
    if let Some(progress) = borg_core::empty_glitch(t) {
        fx::draw_interference(buf, art, t, progress, look.background());
    }
    let caption = Rect { y: art.y + art.height / 2, height: 1, ..art };
    Line::styled(tr!(panel_empty), look.styles().meta).centered().render(caption, buf);
}

/// On the day Sir Patrick Stewart died, empty folders keep still and remember him: his
/// portrait where there is room for it, the dedication, and the photo's credit.
fn draw_remembrance(scene: &PanelScene<'_>, look: &Look<'_>, art: Rect, buf: &mut Buffer) {
    let width = usize::from(art.width.saturating_sub(2)).max(1);
    let styles = look.styles();
    let words =
        [(tr!(tribute_memory), styles.meta), (tr!(tribute_line), styles.footer), (tr!(tribute_credit), styles.footer)];
    let lines: Vec<(String, Style)> = words
        .into_iter()
        .flat_map(|(text, style)| crate::activity_view::wrap(text, width).into_iter().map(move |line| (line, style)))
        .collect();
    let text_rows = lines.len() as u16;
    let room = (art.width.saturating_sub(2), art.height.saturating_sub(text_rows + 1));
    let picture = Some(crate::portrait::size(room)).filter(|(_, rows)| *rows >= PORTRAIT_ROWS);
    let block = picture.map_or(0, |(_, rows)| rows + 1) + text_rows;
    let mut y = art.y + art.height.saturating_sub(block) / 2;
    if let Some((columns, rows)) = picture {
        let area = Rect { x: art.x + (art.width - columns) / 2, y, width: columns, height: rows };
        let plain = scene.state.appearance.current_id() == Some(PLAIN_THEME);
        crate::portrait::draw(buf, area, styles.meta, plain);
        y += rows + 1;
    }
    for (text, style) in lines {
        if y < art.bottom() {
            let row = Rect { y, height: 1, ..art };
            Line::styled(format::truncate_end(&text, width), style).centered().render(row, buf);
        }
        y += 1;
    }
}

/// Below this the portrait would not read as a face, so only the words show.
const PORTRAIT_ROWS: u16 = 12;
/// The retro theme draws the portrait with plain characters instead of braille.
const PLAIN_THEME: &str = "classic";

/// October 28, the day Sir Patrick Stewart died in 2026.
fn remembrance(clock: &Zoned) -> bool {
    (clock.month(), clock.day()) == (10, 28)
}
