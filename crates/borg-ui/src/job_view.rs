//! A job's progress in a few words for the panel footer and the shortcut bar, and the dialog a
//! job's question opens.

use borg_i18n::tr;
use std::{path::Path, time::Instant};

use borg_core::{Job, Phase, Question, Unit};
use borg_jobs::Ask;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{dialog_view::centered, format, look::Look, panel_view::Activity};

const PERCENT: f32 = 100.0;
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3600;
const MIN_ELAPSED: f64 = 0.25;
const DIALOG_WIDTH: u16 = 64;
const MARGIN: u16 = 4;
const PADDING: u16 = 2;
const BUTTON_GAP: &str = "  ";
const BORDER_ROWS: u16 = 2;

pub(crate) fn percent(fraction: f32) -> u32 {
    (fraction * PERCENT).floor() as u32
}

pub(crate) fn clock(seconds: u64) -> String {
    let (hours, minutes, secs) =
        (seconds / SECONDS_PER_HOUR, seconds % SECONDS_PER_HOUR / SECONDS_PER_MINUTE, seconds % SECONDS_PER_MINUTE);
    if hours > 0 { format!("{hours}:{minutes:02}:{secs:02}") } else { format!("{minutes}:{secs:02}") }
}

/// How fast a job that counts bytes goes, and how long it has left: its speed in bytes a
/// second and the seconds to go.
pub(crate) fn pace(job: &Job, now: Instant) -> Option<(u64, u64)> {
    let elapsed = now.saturating_duration_since(job.started).as_secs_f64();
    let gauge = job.gauge;
    if elapsed < MIN_ELAPSED || gauge.done == 0 || gauge.unit != Some(Unit::Bytes) || job.phase != Phase::Running {
        return None;
    }
    let speed = gauge.done as f64 / elapsed;
    let remaining = gauge.total?.saturating_sub(gauge.done) as f64 / speed;
    Some((speed as u64, remaining.ceil() as u64))
}

/// How long job `id` has worked, or worked before it ended.
pub(crate) fn took(job: &Job, now: Instant) -> String {
    clock(job.until(now).saturating_duration_since(job.started).as_secs())
}

/// What a gauge counted so far, such as `9,812 folders`.
pub(crate) fn counted(done: u64, unit: Unit) -> String {
    let done = usize::try_from(done).unwrap_or(usize::MAX);
    match unit {
        Unit::Bytes => format::size(done as u64),
        Unit::Files => format::count(done, format::Count::File),
        Unit::Folders => format::count(done, format::Count::Folder),
        Unit::Processes => format::count(done, format::Count::Process),
    }
}

pub fn activity(job: &Job, now: Instant) -> Activity {
    let fraction = job.fraction();
    let share = percent(fraction.unwrap_or_default());
    let label = match (&job.phase, job.paused) {
        (_, true) => tr!(paused, share),
        (Phase::Preparing, false) => match job.gauge.unit {
            Some(unit) => tr!(scanning, counted(job.gauge.done, unit)),
            None => format!("{}…", job.kind.doing()),
        },
        (Phase::Asking(_), false) => tr!(waiting_for_you).to_owned(),
        (Phase::Running | Phase::Ended(_), false) => match pace(job, now) {
            Some((speed, left)) => {
                format!("{} {share}% · {}/s · {}", job.kind.doing(), format::size(speed), clock(left))
            }
            None => format!("{} {share}%", job.kind.doing()),
        },
    };
    Activity { label, fraction }
}

/// A job's state in a few words, for places with little room such as the shortcut bar.
pub(crate) fn status(job: &Job) -> String {
    let share = percent(job.fraction().unwrap_or_default());
    match (&job.phase, job.paused) {
        (_, true) => tr!(paused, share),
        (Phase::Asking(_), false) => tr!(waiting_for_you).to_owned(),
        (Phase::Preparing | Phase::Ended(_), false) => format!("{}…", job.kind.doing()),
        (Phase::Running, false) if job.fraction().is_none() => format!("{}…", job.kind.doing()),
        (Phase::Running, false) => format!("{} {share}%", job.kind.doing()),
    }
}

fn name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

fn frame(title: &str, look: &Look<'_>, rect: Rect, buf: &mut Buffer) -> Rect {
    let styles = look.styles();
    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(look.theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(title.to_owned(), styles.overlay_border).centered());
    let inner = block.inner(rect);
    block.render(rect, buf);
    Rect { x: inner.x + PADDING, width: inner.width.saturating_sub(PADDING * 2), ..inner }
}

/// A job's answers as buttons in rows that fit `width`: each one's label and hotkey.
fn button_rows(job: &Job, width: usize) -> Vec<Vec<(String, char)>> {
    let mut rows = vec![Vec::new()];
    let mut used = 0;
    for (_, hotkey, label) in job.choices().iter() {
        let hotkey = *hotkey;
        let translated = label.text();
        let label = if translated.chars().any(|c| c.to_ascii_lowercase() == hotkey) {
            translated.to_owned()
        } else {
            format!("{hotkey} {translated}")
        };
        let needed = format!("[ {label} ]").width() + BUTTON_GAP.width();
        if used + needed > width && used > 0 {
            rows.push(Vec::new());
            used = 0;
        }
        if let Some(row) = rows.last_mut() {
            row.push((label, hotkey));
        }
        used += needed;
    }
    rows
}

fn buttons<'a>(job: &Job, selected: usize, width: usize, look: &Look<'_>) -> Vec<Line<'a>> {
    let styles = look.styles();
    let mut rows: Vec<Vec<Span<'a>>> = Vec::new();
    let mut index = 0;
    for row_labels in button_rows(job, width) {
        rows.push(Vec::new());
        for (label, hotkey) in row_labels {
            let hotkey = &hotkey;
            let style = if index == selected { styles.overlay_selected } else { styles.overlay };
            index += 1;
            let hot = label.char_indices().find(|(_, c)| c.to_ascii_lowercase() == *hotkey).map(|(offset, _)| offset);
            if let Some(row) = rows.last_mut() {
                let (before, rest) = label.split_at(hot.unwrap_or(label.len()));
                let mut chars = rest.chars();
                let key = chars.next().map(String::from).unwrap_or_default();
                row.extend([
                    Span::styled(format!("[ {before}"), style),
                    Span::styled(key, style.add_modifier(Modifier::UNDERLINED)),
                    Span::styled(format!("{} ]", chars.as_str()), style),
                    Span::styled(BUTTON_GAP, styles.overlay),
                ]);
            }
        }
    }
    rows.into_iter().map(|spans| Line::from(spans).centered()).collect()
}

/// The question's dialog and its buttons, each with the hotkey that answers with it.
pub(crate) fn question_buttons(job: &Job, screen: Rect) -> (Rect, Vec<(Rect, char)>) {
    let width = screen.width.saturating_sub(MARGIN).min(DIALOG_WIDTH);
    let text_width = usize::from(width.saturating_sub(PADDING * 2 + BORDER_ROWS));
    let lines = match &job.phase {
        Phase::Asking(Ask::Conflict { .. } | Ask::Expand { .. } | Ask::Slow { .. }) => 3,
        Phase::Asking(Ask::Failure { .. } | Ask::Room { .. }) => 2,
        _ => return (Rect::default(), Vec::new()),
    };
    let rows = button_rows(job, text_width);
    let height = u16::try_from(lines + rows.len()).unwrap_or(u16::MAX) + BORDER_ROWS + 3;
    let rect = centered(screen, width, height);
    let inner = Rect { x: rect.x + 1 + PADDING, width: rect.width.saturating_sub(2 + PADDING * 2), ..rect };
    let mut buttons = Vec::new();
    for (row, labels) in rows.iter().enumerate() {
        let widths: Vec<u16> = labels.iter().map(|(label, _)| format!("[ {label} ]").width() as u16).collect();
        let total = widths.iter().map(|width| width + BUTTON_GAP.width() as u16).sum::<u16>();
        let mut x = inner.x + inner.width.saturating_sub(total) / 2;
        let y = rect.y + 2 + lines as u16 + 1 + row as u16;
        for ((_, hotkey), width) in labels.iter().zip(widths) {
            buttons.push((Rect { x, y, width, height: 1 }, *hotkey));
            x += width + BUTTON_GAP.width() as u16;
        }
    }
    (rect, buttons)
}

pub fn draw_question(job: &Job, question: Question, look: &Look<'_>, screen: Rect, buf: &mut Buffer) -> Rect {
    let styles = look.styles();
    let Phase::Asking(ask) = &job.phase else { return Rect::default() };
    let width = screen.width.saturating_sub(MARGIN).min(DIALOG_WIDTH);
    let text_width = usize::from(width.saturating_sub(PADDING * 2 + BORDER_ROWS));
    let (title, lines) = match ask {
        Ask::Conflict { target, sizes, newer, .. } => {
            let place = target.parent().map(|dir| dir.display().to_string()).unwrap_or_default();
            let fresher = if *newer { tr!(newer) } else { "" };
            (
                tr!(already_exists),
                vec![
                    Line::styled(
                        format::truncate_end(&name(target), text_width),
                        styles.overlay.add_modifier(Modifier::BOLD),
                    ),
                    Line::styled(
                        tr!(job_source_folder, format::truncate_start(&place, text_width.saturating_sub(3))),
                        styles.overlay,
                    ),
                    Line::styled(
                        tr!(incoming_existing, format::size(sizes.0), format::size(sizes.1), fresher = fresher),
                        styles.overlay,
                    ),
                ],
            )
        }
        Ask::Failure { path, message } => (
            tr!(something_went_wrong),
            vec![
                Line::styled(
                    format::truncate_end(&name(path), text_width),
                    styles.overlay.add_modifier(Modifier::BOLD),
                ),
                Line::styled(format::truncate_end(message, text_width), styles.notice),
            ],
        ),
        Ask::Expand { archive, packed, unpacked, files } => {
            let files = borg_i18n::count(usize::try_from(*files).unwrap_or(usize::MAX), borg_i18n::Count::File);
            let detail = tr!(
                archive_expand_detail,
                unpacked = format::size(*unpacked),
                files = files,
                packed = format::size(*packed)
            );
            (
                tr!(archive_expand_title),
                vec![
                    Line::styled(
                        format::truncate_end(&name(archive), text_width),
                        styles.overlay.add_modifier(Modifier::BOLD),
                    ),
                    Line::styled(format::truncate_end(&detail, text_width), styles.overlay),
                    Line::styled(format::truncate_end(tr!(archive_expand_warning), text_width), styles.notice),
                ],
            )
        }
        Ask::Room { target, needed, free } => {
            let detail = tr!(archive_room_detail, needed = format::size(*needed), free = format::size(*free));
            (
                tr!(archive_room_title),
                vec![
                    Line::styled(
                        format::truncate_start(&target.display().to_string(), text_width),
                        styles.overlay.add_modifier(Modifier::BOLD),
                    ),
                    Line::styled(format::truncate_end(&detail, text_width), styles.notice),
                ],
            )
        }
        Ask::Slow { path, bytes, estimate, kind, .. } => {
            let detail = tr!(
                slow_detail,
                time = format::span(*estimate),
                bytes = format::size(*bytes),
                place = format::on_drive(*kind)
            );
            (
                tr!(slow_title),
                vec![
                    Line::styled(
                        format::truncate_end(&name(path), text_width),
                        styles.overlay.add_modifier(Modifier::BOLD),
                    ),
                    Line::styled(format::truncate_end(&detail, text_width), styles.overlay),
                    Line::styled(format::truncate_end(tr!(slow_note), text_width), styles.notice),
                ],
            )
        }
    };
    let actions = buttons(job, question.choice, text_width, look);
    let height = u16::try_from(lines.len() + actions.len()).unwrap_or(u16::MAX) + BORDER_ROWS + 3;
    let rect = centered(screen, width, height);
    let inner = frame(title, look, rect, buf);
    for (row, line) in (inner.y + 1..).zip(lines.into_iter().chain(std::iter::once(Line::raw(""))).chain(actions)) {
        let centered = line.centered();
        centered.render(Rect { y: row, height: 1, ..inner }, buf);
    }
    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_clocks_and_percentages() {
        assert_eq!(clock(5), "0:05");
        assert_eq!(clock(125), "2:05");
        assert_eq!(clock(3_725), "1:02:05");
        assert_eq!(percent(0.456), 45);
    }
}
