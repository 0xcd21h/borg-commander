//! The activity monitor: the process list, a process's memory, files and regions, and memory
//! searches, each page in a module of its own; the drawing they share is here.

use crate::{Rgb, Theme, format, look::Look, spinner::Spinner};
use borg_core::{Activity, ActivityPage, ActivityRequest, AppState};
use borg_i18n::tr;
use crossterm::event::KeyCode;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

mod controls;
mod files;
mod memory;
mod processes;
mod regions;
mod search;

pub(crate) use controls::{keybar, panel_mouse};
use files::{file_details, files};
pub(crate) use memory::{Unit, chart};
use memory::{memory, metrics, sized_line, sizes};
use processes::filter_row;
use regions::{Page, regions, snapshot};
use search::{findings, search_form, search_results};

pub(crate) const HEADER_ROWS: u16 = 3;
pub(crate) const FOOTER_ROWS: u16 = 1;
const SIZE_WIDTH: usize = 9;
/// Summary rows above search results: what was found, then what was searched or the strings' tally.
const SEARCH_HEADER: u16 = 2;

fn tabs(area: Rect) -> Vec<(Rect, &'static str, KeyCode)> {
    let mut x = area.x;
    [(tr!(process_memory_tab), '1'), (tr!(process_files_tab), '2')]
        .into_iter()
        .map(|(label, key)| {
            let width = (label.width() as u16 + 2).min(area.right().saturating_sub(x));
            let rect = Rect { x, width, ..area };
            x += width;
            (rect, label, KeyCode::Char(key))
        })
        .collect()
}

/// Secondary text should stay readable; accents need less, since they repeat what text says.
const READABLE: f32 = 3.0;
const DIM_SHARE: f32 = 0.4;

/// The pane's colours. Text roles borrowed from other parts of a theme, such as the dialog
/// accent and the footer colour, fall back to the pane's own text colour when they would
/// be hard to read on its background (white on Classic's grey, for example).
fn colors(theme: &Theme) -> (Rgb, Rgb) {
    let palette = crate::Palette::default();
    let base = theme.styles.overlay;
    (palette.foreground_of(base.fg.unwrap_or(Color::Reset)), palette.background_of(base.bg.unwrap_or(Color::Reset)))
}

pub(crate) fn readable(theme: &Theme, color: Option<Color>) -> Option<Color> {
    let (_, background) = colors(theme);
    color.filter(|&c| crate::Palette::default().foreground_of(c).contrast(background) >= READABLE)
}

/// Accented text: values and summaries that should stand out from body text.
pub(crate) fn emphasis(theme: &Theme) -> Style {
    let base = theme.styles.overlay;
    readable(theme, theme.styles.overlay_border.fg).map_or(base, |color| base.fg(color))
}

pub(crate) fn heading(theme: &Theme) -> Style {
    emphasis(theme).add_modifier(Modifier::BOLD)
}

pub(crate) fn dim(theme: &Theme) -> Style {
    let (text, background) = colors(theme);
    let fallback = text.mix(background, DIM_SHARE).into();
    theme.styles.overlay.fg(readable(theme, theme.styles.footer.fg).unwrap_or(fallback))
}

/// Splits text into lines of at most `width` cells, breaking between words.
pub(crate) fn wrap(value: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in value.split_whitespace() {
        let last = lines.last_mut().unwrap();
        if !last.is_empty() && last.width() + 1 + word.width() > width {
            lines.push(String::new());
        }
        let last = lines.last_mut().unwrap();
        if !last.is_empty() {
            last.push(' ');
        }
        last.push_str(word);
    }
    lines
}

pub(crate) fn inner(screen: Rect) -> Rect {
    Block::bordered().inner(screen)
}
pub(crate) fn body(screen: Rect) -> Rect {
    let inner = inner(screen);
    Rect {
        y: inner.y.saturating_add(HEADER_ROWS).min(inner.bottom()),
        height: inner.height.saturating_sub(HEADER_ROWS + FOOTER_ROWS),
        ..inner
    }
}
pub(crate) fn text(buf: &mut Buffer, area: Rect, value: impl Into<String>, style: Style) {
    if !area.is_empty() {
        Line::styled(displayable(&value.into()), style).render(area, buf);
    }
}
/// Text with control characters, which would garble the terminal, shown as replacements.
fn displayable(value: &str) -> String {
    value.chars().map(|c| if c.is_control() { '�' } else { c }).collect()
}
pub(crate) fn row(area: Rect, offset: u16) -> Rect {
    Rect { y: area.y.saturating_add(offset).min(area.bottom()), height: u16::from(offset < area.height), ..area }
}
fn size(value: Option<u64>) -> String {
    value.map_or_else(|| "—".into(), format::size)
}
/// Draws the activity monitor. Inspecting a process opens a window over it, holding the
/// inspection list and its preview, while the process list and overview stay beneath.
pub(crate) fn draw(
    state: &AppState,
    look: &Look<'_>,
    panels: [Rect; 2],
    monitor: [Rect; 2],
    now: std::time::Instant,
    buf: &mut Buffer,
) {
    let activity = &state.activity;
    let Some(side) = activity.side else { return };
    let spinner = Spinner::of(&state.busy, borg_core::Task::Activity, now);
    if activity.inspecting() {
        for (pane, page) in [(side, ActivityPage::Processes), (side.other(), ActivityPage::Memory)] {
            if !monitor[pane.index()].is_empty() {
                let pane = Pane { page, processes: pane == side, focused: false, spinner: None };
                draw_panel(state, look, monitor[pane_index(side, pane.processes)], pane, buf);
            }
        }
        let [left, right] = monitor;
        let whole = Rect { width: right.right().saturating_sub(left.x), ..left };
        crate::fx::fade_to_background(buf, whole, crate::draw::BACKDROP, look.palette);
    }
    for pane in [side, side.other()] {
        let rect = panels[pane.index()];
        if rect.is_empty() {
            continue;
        }
        let list = pane == side;
        let (page, focused) =
            if list { (activity.list, !activity.preview_focused) } else { (activity.page, activity.preview_focused) };
        draw_panel(state, look, rect, Pane { page, processes: list, focused, spinner }, buf);
    }
    if activity.inspecting() {
        let window = panels.into_iter().filter(|r| !r.is_empty()).reduce(|a, b| a.union(b)).unwrap_or_default();
        crate::fx::shadow(buf, window, look.palette);
    }
}

/// What one activity pane shows: its page, whether it is the list pane, focus, and the
/// spinner for work that has taken a while.
struct Pane {
    page: ActivityPage,
    processes: bool,
    focused: bool,
    spinner: Option<Spinner>,
}

fn pane_index(side: borg_core::Side, processes: bool) -> usize {
    if processes { side.index() } else { side.other().index() }
}

fn draw_panel(state: &AppState, look: &Look<'_>, screen: Rect, pane: Pane, buf: &mut Buffer) -> Rect {
    let Pane { page, processes, focused, spinner } = pane;
    let activity = &state.activity;
    // The pane showing the current list or preview, rather than the monitor beneath an
    // inspection window.
    let live = if processes { page == activity.list } else { page == activity.page };
    let searching = !processes && live && activity.search_form.is_some();
    let inner = frame(activity, look.theme, screen, page, searching, focused, buf);
    if inner.is_empty() {
        return screen;
    }
    text(buf, row(inner, 0), summary(activity, page, processes), emphasis(look.theme));
    subtitle(activity, look, inner, page, processes, buf);
    let body = body(screen);
    match activity.search_form.as_ref().filter(|_| searching) {
        Some(form) => search_form(form, look.theme, body, buf),
        None => draw_page(activity, look, page, (inner, body), focused, spinner, buf),
    }
    footer(state, look, inner, live, spinner, buf);
    screen
}

/// Clears the pane and draws its border, with the page's title, whether the monitor is live,
/// and the expand control; returns what is inside.
fn frame(
    activity: &Activity,
    theme: &Theme,
    rect: Rect,
    page: ActivityPage,
    searching: bool,
    focused: bool,
    buf: &mut Buffer,
) -> Rect {
    let styles = &theme.styles;
    Clear.render(rect, buf);
    let title = if searching {
        tr!(memory_search_title)
    } else {
        match page {
            ActivityPage::Processes => tr!(activity_title),
            ActivityPage::Memory => tr!(activity_memory),
            ActivityPage::Regions => tr!(activity_map),
            ActivityPage::Snapshot => tr!(activity_snapshot),
            ActivityPage::Search => tr!(memory_search_results),
            ActivityPage::Integrity => tr!(integrity_title),
            ActivityPage::Files => tr!(process_files_title),
            ActivityPage::FileDetails => tr!(process_file_details),
        }
    };
    let status = if activity.pending.as_ref().is_some_and(|(_, r)| matches!(r, ActivityRequest::Search { .. })) {
        tr!(memory_search_running)
    } else if activity.paused {
        tr!(activity_paused)
    } else {
        tr!(activity_live)
    };
    let title = if matches!(page, ActivityPage::Processes | ActivityPage::Memory) {
        format!(" {title} · {status} ")
    } else {
        format!(" {title} ")
    };
    let controls = crate::results_view::controls(rect, activity.expanded);
    let room = controls.first().map_or(rect.width.saturating_sub(2), |(r, _, _)| r.x.saturating_sub(rect.x + 2));
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(if focused { styles.border_active } else { styles.border })
        .title(Line::styled(
            format::truncate_end(&title, usize::from(room)),
            if focused { styles.title_active } else { styles.title },
        ));
    let inner = block.inner(rect);
    block.render(rect, buf);
    for (r, label, _) in controls {
        text(buf, r, label, styles.menu);
    }
    inner
}

/// The pane's first row: the machine's load, or what the page shows of the process or region.
fn summary(activity: &Activity, page: ActivityPage, processes: bool) -> String {
    if processes && page != ActivityPage::Processes {
        let pid = activity.selected.map_or(0, |p| p.pid);
        return if activity.regions().is_empty() {
            tr!(
                process_memory_sampled,
                pid = pid,
                ram = size(activity.process().and_then(|p| p.resident)),
                mapped = size(activity.process().and_then(|p| p.virtual_bytes))
            )
        } else {
            let totals = activity.region_totals();
            tr!(
                process_memory_summary,
                pid = pid,
                footprint = size(totals.footprint),
                resident = size(totals.resident),
                mapped = format::size(totals.mapped)
            )
        };
    }
    if page == ActivityPage::FileDetails {
        return tr!(process_files_count, count = activity.files.len(), skipped = activity.files_skipped);
    }
    if page == ActivityPage::Snapshot && activity.search_form.is_none() {
        return activity.shown_region().map_or_else(String::new, |r| {
            tr!(
                activity_region_summary,
                access = r.permissions,
                bytes = format::size(r.mapped()),
                resident = size(r.resident),
                footprint = size(r.footprint())
            )
        });
    }
    let cores = activity.sample.cpu.len().max(1) as u64;
    let cpu_total = activity.sample.cpu.iter().map(|&c| u64::from(c)).sum::<u64>() / cores;
    tr!(
        activity_overview,
        cpu = format_args!("{:.1}%", cpu_total as f64 / 100.0),
        used = format::size(activity.sample.used),
        total = format::size(activity.sample.total),
        swap = format::size(activity.sample.swap_used),
        swap_total = format::size(activity.sample.swap_total),
        count = activity.sample.processes.len()
    )
}

/// The pane's second row: the process or region it is about, the process filter, or the tabs
/// between a process's memory and its files.
fn subtitle(activity: &Activity, look: &Look<'_>, inner: Rect, page: ActivityPage, processes: bool, buf: &mut Buffer) {
    let styles = &look.theme.styles;
    let line = row(inner, 1);
    let subtitle = if page == ActivityPage::Snapshot && activity.search_form.is_none() {
        activity.shown_region().map(|r| r.name.clone()).unwrap_or_default()
    } else if page == ActivityPage::Processes {
        String::new()
    } else if let Some(p) = activity.process() {
        tr!(
            activity_process_label,
            name = p.name,
            pid = p.key.pid,
            executable = p.executable.as_ref().map_or_else(String::new, |p| p.display().to_string())
        )
    } else {
        tr!(activity_process_gone).into()
    };
    text(buf, line, subtitle, styles.overlay);
    if page == ActivityPage::Processes {
        filter_row(activity, look, line, buf);
    }
    if processes && page != ActivityPage::Processes {
        Clear.render(line, buf);
        buf.set_style(line, styles.overlay);
        for (rect, label, code) in tabs(line) {
            let active = if code == KeyCode::Char('2') {
                activity.list == ActivityPage::Files
            } else {
                activity.list != ActivityPage::Files
            };
            let style = if active { styles.cursor } else { styles.menu };
            buf.set_style(rect, style);
            text(buf, rect, label, style);
        }
    }
}

fn draw_page(
    activity: &Activity,
    look: &Look<'_>,
    page: ActivityPage,
    (inner, body): (Rect, Rect),
    focused: bool,
    spinner: Option<Spinner>,
    buf: &mut Buffer,
) {
    let theme = look.theme;
    match page {
        ActivityPage::Search => search_results(activity, theme, body, buf),
        ActivityPage::Integrity => {
            if let Some(report) = &activity.integrity {
                findings(activity, report, theme, body, buf);
            }
        }
        ActivityPage::Processes => processes::list(activity, theme, row(inner, 2), body, focused, buf),
        ActivityPage::Memory => memory(activity, look, body, buf),
        ActivityPage::Regions => regions(activity, theme, inner, body, buf),
        ActivityPage::Files => files(activity, look, inner, body, spinner, buf),
        ActivityPage::FileDetails => file_details(activity, look, body, buf),
        ActivityPage::Snapshot => {
            // Group and file previews have no region header, so they start at the top.
            let whole = Rect { height: inner.height.saturating_sub(FOOTER_ROWS), ..inner };
            let summary = activity.list != ActivityPage::Regions
                || (activity.selected_group().is_none() && activity.selected_file().is_none());
            snapshot(activity, look, if summary { body } else { whole }, spinner, buf)
        }
    }
}

/// The pane's last row: an error, the progress of a search or check, a notice or the files'
/// tally, or a spinner while the monitor waits for an answer.
fn footer(state: &AppState, look: &Look<'_>, inner: Rect, live: bool, spinner: Option<Spinner>, buf: &mut Buffer) {
    let activity = &state.activity;
    let theme = look.theme;
    let footer = Rect {
        y: inner.bottom().saturating_sub(FOOTER_ROWS.min(inner.height)),
        height: FOOTER_ROWS.min(inner.height),
        ..inner
    };
    let checking = activity.pending.as_ref().is_some_and(|(_, r)| matches!(r, ActivityRequest::Integrity { .. }));
    let progress = activity.search_progress.map(|(scanned, total, hits)| {
        let (scanned, total) = (format::size(scanned), format::size(total));
        if checking {
            tr!(integrity_progress, checked = scanned, total = total, findings = hits)
        } else {
            tr!(memory_search_progress, scanned = scanned, total = total, hits = hits)
        }
    });
    let files_note = (activity.list == ActivityPage::Files && activity.files_loaded)
        .then(|| tr!(process_files_count, count = activity.files.len(), skipped = activity.files_skipped));
    let note = if !live {
        ""
    } else {
        activity
            .error
            .as_deref()
            .or(progress.as_deref())
            .or(state.notice.as_deref())
            .or(files_note.as_deref())
            .unwrap_or("")
    };
    match spinner.filter(|_| live && note.is_empty() && activity.pending.is_some()) {
        Some(spinner) => spinner.line(tr!(activity_loading), look, dim(theme)).render(row(footer, 0), buf),
        None => {
            text(buf, row(footer, 0), note, if activity.error.is_some() { theme.styles.notice } else { dim(theme) })
        }
    }
}
