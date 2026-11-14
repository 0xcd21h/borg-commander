//! The jobs panel Alt-J opens: a sheet on the right with every job, the running ones first.
//! Each job folds to one line and unfolds to its details, and the selected one shows its buttons.

use std::time::{Duration, Instant};

use borg_core::{How, Job, JobList, Layout, Phase, Unit, Value};
use borg_i18n::tr;
use crossterm::event::KeyEvent;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{format, job_view, look::Look};

/// Terminals this wide get the sheet at its own width on the right; narrower ones get it across
/// the panels.
const WIDE_SCREEN: u16 = 100;
const SHEET_WIDTH: u16 = 58;
const BAR_WIDTH: usize = 14;
/// Sheets with less room than this get a short bar, and those with less than `BARLESS` none.
const NARROW: usize = 50;
const NARROW_BAR: usize = 8;
const BARLESS: usize = 34;
const BAR_FILL: &str = "━";
const BAR_TRACK: &str = "─";
/// The lit part of a bar with no total, and how long it stays on a cell.
const SWEEP: usize = 3;
const SWEEP_STEP: Duration = Duration::from_millis(150);
const INDENT: &str = "    ";
const LABEL_GAP: usize = 2;
const SUBJECT_GAP: &str = "  ";
const BUTTON_GAP: &str = "  ";
const SEPARATOR: &str = " · ";
const PADDING: u16 = 1;
const FOLDED: &str = "▸";
const UNFOLDED: &str = "▾";
const ASKING: &str = "!";
const DONE: &str = "✓";
const CANCELLED: &str = "■";
const FAILED: &str = "✗";

/// One row of a job's details, as the sheet shows it.
struct Fact {
    label: String,
    value: Value,
    warn: bool,
}

/// One line of the sheet.
enum Piece {
    Head(usize),
    Fact { job: usize, fact: Fact, column: usize },
    Buttons { job: usize, labels: Vec<&'static str> },
    Empty,
}

impl Piece {
    fn job(&self) -> Option<usize> {
        match self {
            Self::Head(job) | Self::Fact { job, .. } | Self::Buttons { job, .. } => Some(*job),
            Self::Empty => None,
        }
    }
}

/// What the mouse is over in the sheet.
pub(crate) enum At {
    Row(usize),
    Button(KeyEvent),
    /// Elsewhere: the border's keys, or outside the sheet.
    Frame(Rect),
}

/// The part of the screen the panels take, which the sheet and the toasts sit in.
pub(crate) fn panels_area(layout: &Layout) -> Rect {
    let [left, right] = layout.panels;
    let area = left.union(right);
    if area.is_empty() { layout.screen } else { area }
}

/// The sheet: over the right panel on a wide screen, unless that panel is narrower than the
/// sheet, and across the panels on a narrow one.
fn rect(layout: &Layout) -> Rect {
    let area = panels_area(layout);
    let right = layout.panels[1];
    let beside = right.x > area.x && right.right() == area.right() && right.width >= SHEET_WIDTH;
    let width = match layout.screen.width >= WIDE_SCREEN {
        true if beside => right.width,
        true => SHEET_WIDTH.min(area.width),
        false => area.width,
    };
    Rect { x: area.right().saturating_sub(width), width, ..area }
}

fn inner(rect: Rect) -> Rect {
    let inner = Block::bordered().inner(rect);
    Rect { x: inner.x + PADDING, width: inner.width.saturating_sub(PADDING * 2), ..inner }
}

/// The keys the bottom border lists: all of them, or the fewest when the sheet is narrow.
pub(crate) fn hint(rect: Rect) -> &'static str {
    let room = usize::from(rect.width.saturating_sub(2));
    if tr!(jobs_hint).width() <= room { tr!(jobs_hint) } else { tr!(jobs_hint_short) }
}

fn title(jobs: &[&Job]) -> String {
    let running = jobs.iter().filter(|job| job.running()).count();
    let ended = jobs.len() - running;
    let mut parts = vec![tr!(jobs_title).to_owned()];
    if running > 0 {
        parts.push(tr!(jobs_running_count, count = running));
    }
    if ended > 0 {
        parts.push(tr!(jobs_done_count, count = ended));
    }
    format!(" {} ", parts.join(SEPARATOR))
}

fn icon(job: &Job) -> &'static str {
    match &job.phase {
        Phase::Ended(ending) => match ending.how {
            How::Done => DONE,
            How::Cancelled => CANCELLED,
            How::Failed => FAILED,
        },
        Phase::Asking(_) => ASKING,
        Phase::Preparing | Phase::Running if job.folded => FOLDED,
        Phase::Preparing | Phase::Running => UNFOLDED,
    }
}

fn bar_width(width: usize) -> usize {
    if width >= NARROW {
        BAR_WIDTH
    } else if width >= BARLESS {
        NARROW_BAR
    } else {
        0
    }
}

/// A running job's bar as runs of cells, each lit or not: filled up to its share, or a short
/// lit part that moves along with the time it ran when the total is not known.
fn bar(fraction: Option<f32>, ran: Duration, width: usize) -> Vec<(usize, bool)> {
    match fraction {
        Some(fraction) => {
            let filled = ((fraction * width as f32).round() as usize).min(width);
            vec![(filled, true), (width - filled, false)]
        }
        None => {
            let steps = ran.as_millis() / SWEEP_STEP.as_millis();
            let at = usize::try_from(steps).unwrap_or_default() % width.max(1);
            let lit = SWEEP.min(width - at);
            vec![(at, false), (lit, true), (width - at - lit, false)]
        }
    }
}

/// What a job's line says on its right: how far it got, or how it ended and how long it took.
fn progress(job: &Job, now: Instant) -> String {
    let share = job_view::percent(job.fraction().unwrap_or_default());
    match (&job.phase, job.paused) {
        (Phase::Ended(ending), _) => format!("{}{SEPARATOR}{}", ending.summary, job_view::took(job, now)),
        (_, true) => tr!(paused, share),
        (Phase::Asking(_), false) => tr!(waiting_for_you).to_owned(),
        (Phase::Preparing | Phase::Running, false) => match (job.gauge.total, job.gauge.unit) {
            (Some(_), _) => match job_view::pace(job, now) {
                Some((_, left)) => format!("{share}% {}", job_view::clock(left)),
                None => format!("{share}%"),
            },
            (None, Some(unit)) => job_view::counted(job.gauge.done, unit),
            (None, None) => format!("{}…", job.kind.doing()),
        },
    }
}

fn facts(job: &Job, now: Instant) -> Vec<Fact> {
    let mut facts: Vec<Fact> = job
        .details
        .iter()
        .map(|detail| Fact { label: detail.label.text().to_owned(), value: detail.value.clone(), warn: false })
        .collect();
    if let Some((speed, _)) = job_view::pace(job, now) {
        let value = Value::Text(format!("{}/s", format::size(speed)));
        facts.push(Fact { label: tr!(job_detail_speed).to_owned(), value, warn: false });
    }
    if let Phase::Ended(ending) = &job.phase {
        let summary = Value::Text(ending.summary.clone());
        facts.push(Fact { label: tr!(job_detail_ended).to_owned(), value: summary, warn: ending.how == How::Failed });
        let took = Value::Text(job_view::took(job, now));
        facts.push(Fact { label: tr!(job_detail_took).to_owned(), value: took, warn: false });
    }
    facts
}

/// The buttons the selected job shows, each starting with the key it presses.
fn buttons(job: &Job) -> Vec<&'static str> {
    let pause = if job.paused { tr!(job_button_resume) } else { tr!(job_button_pause) };
    let choices = match &job.phase {
        Phase::Ended(_) => [job.reveal.is_some().then_some(tr!(job_button_open)), Some(tr!(job_button_dismiss))],
        Phase::Asking(_) => [Some(tr!(job_button_answer)), job.kind.cancels().then_some(tr!(job_button_cancel))],
        Phase::Preparing | Phase::Running => {
            [job.kind.pauses().then_some(pause), job.kind.cancels().then_some(tr!(job_button_cancel))]
        }
    };
    choices.into_iter().flatten().collect()
}

fn pieces(jobs: &[&Job], list: JobList, now: Instant) -> Vec<Piece> {
    if jobs.is_empty() {
        return vec![Piece::Empty];
    }
    let mut pieces = Vec::new();
    for (index, job) in jobs.iter().enumerate() {
        pieces.push(Piece::Head(index));
        if job.folded {
            continue;
        }
        let facts = facts(job, now);
        let column = facts.iter().map(|fact| fact.label.width()).max().unwrap_or_default() + LABEL_GAP;
        pieces.extend(facts.into_iter().map(|fact| Piece::Fact { job: index, fact, column }));
        let labels = buttons(job);
        if index == list.selected && !labels.is_empty() {
            pieces.push(Piece::Buttons { job: index, labels });
        }
    }
    pieces
}

/// The first line shown: the top, or far enough down that the selected job shows, its head
/// first when it is taller than the sheet.
fn scroll(pieces: &[Piece], selected: usize, height: u16) -> usize {
    let start = pieces.iter().position(|piece| piece.job() == Some(selected)).unwrap_or_default();
    let end = pieces.iter().rposition(|piece| piece.job() == Some(selected)).map_or(start, |end| end + 1);
    end.saturating_sub(usize::from(height)).min(start)
}

fn head<'a>(job: &Job, selected: bool, width: usize, look: &Look<'_>, now: Instant) -> Line<'a> {
    let styles = look.styles();
    let base = if selected { styles.overlay_selected } else { styles.overlay };
    let tint = |style: Style| if selected { base } else { style };
    let accent = styles.overlay.fg(look.accent().from.into());
    let dim = styles.overlay.add_modifier(Modifier::DIM);
    let warn = styles.overlay.fg(look.fg(styles.notice).into());
    let icon_style = match &job.phase {
        Phase::Ended(ending) if ending.how == How::Done => accent,
        Phase::Ended(ending) if ending.how == How::Cancelled => dim,
        Phase::Ended(_) | Phase::Asking(_) => warn,
        Phase::Preparing | Phase::Running => base,
    };
    let lit = if job.running() { bar_width(width) } else { 0 };
    let right = format::truncate_end(&progress(job, now), width / 2);
    let tail = right.width() + if lit > 0 { lit + 1 } else { 0 };
    let lead = format!("{} ", icon(job));
    let room = width.saturating_sub(tail + 1 + lead.width());
    let name = format::truncate_end(job.kind.title(), room);
    let subject_room = room.saturating_sub(name.width() + SUBJECT_GAP.width());
    let subject = if subject_room > 0 {
        format!("{SUBJECT_GAP}{}", format::truncate_end(&job.subject, subject_room))
    } else {
        String::new()
    };
    let used = lead.width() + name.width() + subject.width() + tail;
    let mut spans = vec![
        Span::styled(lead, tint(icon_style)),
        Span::styled(name, base.add_modifier(Modifier::BOLD)),
        Span::styled(subject, base),
        Span::styled(" ".repeat(width.saturating_sub(used)), base),
    ];
    if lit > 0 {
        spans.extend(bar(job.fraction(), now.saturating_duration_since(job.started), lit).into_iter().map(
            |(cells, on)| {
                let (glyph, style) = if on { (BAR_FILL, accent) } else { (BAR_TRACK, dim) };
                Span::styled(glyph.repeat(cells), tint(style))
            },
        ));
        spans.push(Span::styled(" ", base));
    }
    let ended_badly = job.ending().is_some_and(|ending| ending.how == How::Failed);
    spans.push(Span::styled(right, if ended_badly { tint(warn) } else { base }));
    Line::from(spans)
}

fn fact_line<'a>(fact: &Fact, column: usize, width: usize, look: &Look<'_>) -> Line<'a> {
    let styles = look.styles();
    let room = width.saturating_sub(INDENT.width() + column);
    let value = match &fact.value {
        Value::Text(text) => format::truncate_end(text, room),
        Value::Path(path) => format::truncate_start(&path.display().to_string(), room),
        Value::Number(number) => number.to_string(),
        Value::Of { done, total, unit } => {
            let done = if *unit == Unit::Bytes { format::size(*done) } else { done.to_string() };
            format::truncate_end(&tr!(job_of, done = done, total = job_view::counted(*total, *unit)), room)
        }
    };
    Line::from(vec![
        Span::styled(INDENT, styles.overlay),
        Span::styled(format::pad_end(&fact.label, column), styles.overlay.add_modifier(Modifier::DIM)),
        Span::styled(value, if fact.warn { styles.overlay.fg(look.fg(styles.notice).into()) } else { styles.overlay }),
    ])
}

/// The buttons in a row, each with the columns it takes from the row's start.
fn button_spans(labels: &[&'static str]) -> Vec<(String, u16, u16)> {
    let mut x = INDENT.width() as u16;
    labels
        .iter()
        .map(|label| {
            let text = format!("[ {label} ]");
            let width = text.width() as u16;
            let placed = (text, x, width);
            x += width + BUTTON_GAP.width() as u16;
            placed
        })
        .collect()
}

pub fn draw(list: JobList, jobs: &[Job], look: &Look<'_>, now: Instant, layout: &Layout, buf: &mut Buffer) -> Rect {
    let styles = look.styles();
    let shown = borg_core::shown_jobs(jobs);
    let rect = rect(layout);
    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(look.theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(title(&shown), styles.overlay_border).right_aligned())
        .title_bottom(Line::styled(hint(rect), styles.overlay).centered());
    block.render(rect, buf);
    let inner = inner(rect);
    let width = usize::from(inner.width);
    let pieces = pieces(&shown, list, now);
    let top = scroll(&pieces, list.selected, inner.height);
    for (piece, y) in pieces.iter().skip(top).zip(inner.y..inner.bottom()) {
        let row = Rect { y, height: 1, ..inner };
        let line = match piece {
            Piece::Head(index) => {
                let selected = *index == list.selected;
                if selected {
                    buf.set_style(
                        Rect { x: rect.x + 1, width: rect.width.saturating_sub(2), ..row },
                        styles.overlay_selected,
                    );
                }
                shown.get(*index).map(|job| head(job, selected, width, look, now))
            }
            Piece::Fact { fact, column, .. } => Some(fact_line(fact, *column, width, look)),
            Piece::Buttons { labels, .. } => {
                let mut spans = vec![Span::styled(INDENT, styles.overlay)];
                for (text, _, _) in button_spans(labels) {
                    spans.push(Span::styled(text, styles.overlay_selected));
                    spans.push(Span::styled(BUTTON_GAP, styles.overlay));
                }
                Some(Line::from(spans))
            }
            Piece::Empty => Some(Line::styled(
                format::truncate_end(tr!(jobs_empty), width),
                styles.overlay.add_modifier(Modifier::DIM),
            )),
        };
        if let Some(line) = line {
            line.render(row, buf);
        }
    }
    rect
}

/// What is under `point`: a job's line, one of the selected job's buttons, or the frame.
pub(crate) fn at(list: JobList, jobs: &[Job], layout: &Layout, now: Instant, point: Position) -> At {
    let shown = borg_core::shown_jobs(jobs);
    let rect = rect(layout);
    let inner = inner(rect);
    if !inner.contains(point) {
        return At::Frame(rect);
    }
    let pieces = pieces(&shown, list, now);
    let top = scroll(&pieces, list.selected, inner.height);
    let Some(piece) = pieces.get(top + usize::from(point.y - inner.y)) else { return At::Frame(rect) };
    match piece {
        Piece::Buttons { labels, .. } => {
            let column = point.x - inner.x;
            let pressed =
                labels.iter().zip(button_spans(labels)).find(|(_, (_, x, width))| (*x..x + width).contains(&column));
            match pressed.and_then(|(label, _)| crate::hints::key(label.split_whitespace().next()?)) {
                Some(key) => At::Button(key),
                None => At::Frame(rect),
            }
        }
        piece => piece.job().map_or(At::Frame(rect), At::Row),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_fills_to_its_share_or_moves_along_without_a_total() {
        assert_eq!(bar(Some(0.5), Duration::ZERO, 14), vec![(7, true), (7, false)]);
        assert_eq!(bar(None, Duration::ZERO, 14), vec![(0, false), (3, true), (11, false)]);
        assert_eq!(bar(None, SWEEP_STEP * 2, 14), vec![(2, false), (3, true), (9, false)]);
        assert_eq!(bar(None, SWEEP_STEP * 13, 14), vec![(13, false), (1, true), (0, false)]);
    }

    #[test]
    fn the_sheet_scrolls_until_the_selected_job_shows() {
        let pieces = [
            Piece::Head(0),
            Piece::Head(1),
            Piece::Buttons { job: 1, labels: Vec::new() },
            Piece::Buttons { job: 1, labels: Vec::new() },
            Piece::Head(2),
        ];
        assert_eq!(scroll(&pieces, 0, 2), 0);
        assert_eq!(scroll(&pieces, 1, 2), 1);
        assert_eq!(scroll(&pieces, 2, 2), 3);
        assert_eq!(scroll(&pieces, 1, 10), 0);
    }
}
