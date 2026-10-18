use std::{path::Path, time::Instant};

use borg_core::{Job, JobList, JobState, Question};
use borg_jobs::{Ask, Progress};
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
const CONFLICT_TITLE: &str = " Already exists ";
const FAILURE_TITLE: &str = " Something went wrong ";
const JOBS_TITLE: &str = " Jobs ";
const JOBS_HINT: &str = " p pause · c cancel · Esc close ";
const JOBS_WIDTH: u16 = 72;
const ROWS_PER_JOB: u16 = 2;
const BAR_WIDTH: usize = 24;
const BAR_FILL: char = '━';
const BAR_TRACK: char = '─';
const BORDER_ROWS: u16 = 2;
const LABEL_GAP: &str = " ";

fn percent(fraction: f32) -> u32 {
    (fraction * PERCENT).floor() as u32
}

fn clock(seconds: u64) -> String {
    let (hours, minutes, secs) =
        (seconds / SECONDS_PER_HOUR, seconds % SECONDS_PER_HOUR / SECONDS_PER_MINUTE, seconds % SECONDS_PER_MINUTE);
    if hours > 0 { format!("{hours}:{minutes:02}:{secs:02}") } else { format!("{minutes}:{secs:02}") }
}

fn pace(job: &Job, progress: &Progress, now: Instant) -> Option<(String, String)> {
    let elapsed = now.saturating_duration_since(job.started).as_secs_f64();
    if elapsed < MIN_ELAPSED || progress.done.bytes == 0 {
        return None;
    }
    let speed = progress.done.bytes as f64 / elapsed;
    let remaining = progress.total.bytes.saturating_sub(progress.done.bytes) as f64 / speed;
    Some((format!("{}/s", format::size(speed as u64)), clock(remaining.ceil() as u64)))
}

pub fn activity(job: &Job, now: Instant) -> Activity {
    let fraction = job.fraction();
    let label = match (&job.state, job.paused) {
        (_, true) => format!("Paused · {}%", percent(fraction.unwrap_or_default())),
        (JobState::Scanning(found), false) => format!("Scanning · {}", format::count(found.files as usize, "file")),
        (JobState::Asking(_), false) => "Waiting for you".to_owned(),
        (JobState::Running(progress), false) => {
            let share = percent(fraction.unwrap_or_default());
            match pace(job, progress, now) {
                Some((speed, eta)) => format!("{} {share}% · {speed} · {eta}", job.kind.doing()),
                None => format!("{} {share}%", job.kind.doing()),
            }
        }
    };
    Activity { label, fraction }
}

fn name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

fn frame(title: &str, hint: Option<&str>, look: &Look<'_>, rect: Rect, buf: &mut Buffer) -> Rect {
    let styles = look.styles();
    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(look.theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(title.to_owned(), styles.overlay_border).centered());
    let block = match hint {
        Some(hint) => block.title_bottom(Line::styled(hint.to_owned(), styles.overlay).centered()),
        None => block,
    };
    let inner = block.inner(rect);
    block.render(rect, buf);
    Rect { x: inner.x + PADDING, width: inner.width.saturating_sub(PADDING * 2), ..inner }
}

fn buttons<'a>(job: &Job, selected: usize, width: usize, look: &Look<'_>) -> Vec<Line<'a>> {
    let styles = look.styles();
    let mut rows: Vec<Vec<Span<'a>>> = vec![Vec::new()];
    let mut used = 0;
    for (index, (_, hotkey, label)) in job.choices().iter().enumerate() {
        let text = format!("[ {label} ]");
        let needed = text.width() + BUTTON_GAP.width();
        if used + needed > width && used > 0 {
            rows.push(Vec::new());
            used = 0;
        }
        let style = if index == selected { styles.overlay_selected } else { styles.overlay };
        let hot = label.chars().position(|c| c.to_ascii_lowercase() == *hotkey);
        let row = rows.last_mut().into_iter().next();
        if let Some(row) = row {
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
        used += needed;
    }
    rows.into_iter().map(|spans| Line::from(spans).centered()).collect()
}

pub fn draw_question(job: &Job, question: Question, look: &Look<'_>, screen: Rect, buf: &mut Buffer) -> Rect {
    let styles = look.styles();
    let JobState::Asking(ask) = &job.state else { return Rect::default() };
    let width = screen.width.saturating_sub(MARGIN).min(DIALOG_WIDTH);
    let text_width = usize::from(width.saturating_sub(PADDING * 2 + BORDER_ROWS));
    let (title, lines) = match ask {
        Ask::Conflict { target, sizes, newer, .. } => {
            let place = target.parent().map(|dir| dir.display().to_string()).unwrap_or_default();
            let fresher = if *newer { " · newer" } else { "" };
            (
                CONFLICT_TITLE,
                vec![
                    Line::styled(
                        format::truncate_end(&name(target), text_width),
                        styles.overlay.add_modifier(Modifier::BOLD),
                    ),
                    Line::styled(
                        format!("in {}", format::truncate_start(&place, text_width.saturating_sub(3))),
                        styles.overlay,
                    ),
                    Line::styled(
                        format!("Incoming {}{fresher}  ·  Existing {}", format::size(sizes.0), format::size(sizes.1)),
                        styles.overlay,
                    ),
                ],
            )
        }
        Ask::Failure { path, message } => (
            FAILURE_TITLE,
            vec![
                Line::styled(
                    format::truncate_end(&name(path), text_width),
                    styles.overlay.add_modifier(Modifier::BOLD),
                ),
                Line::styled(format::truncate_end(message, text_width), styles.notice),
            ],
        ),
    };
    let actions = buttons(job, question.choice, text_width, look);
    let height = u16::try_from(lines.len() + actions.len()).unwrap_or(u16::MAX) + BORDER_ROWS + 3;
    let rect = centered(screen, width, height);
    let inner = frame(title, None, look, rect, buf);
    for (row, line) in (inner.y + 1..).zip(lines.into_iter().chain(std::iter::once(Line::raw(""))).chain(actions)) {
        let centered = line.centered();
        centered.render(Rect { y: row, height: 1, ..inner }, buf);
    }
    rect
}

fn bar(fraction: f32) -> (String, String) {
    let filled = (fraction.clamp(0.0, 1.0) * BAR_WIDTH as f32).round() as usize;
    (BAR_FILL.to_string().repeat(filled), BAR_TRACK.to_string().repeat(BAR_WIDTH - filled))
}

pub fn draw_jobs(list: JobList, jobs: &[Job], look: &Look<'_>, now: Instant, screen: Rect, buf: &mut Buffer) -> Rect {
    let styles = look.styles();
    let width = screen.width.saturating_sub(MARGIN).min(JOBS_WIDTH);
    let height = u16::try_from(jobs.len()).unwrap_or(u16::MAX).saturating_mul(ROWS_PER_JOB) + BORDER_ROWS + 1;
    let rect = centered(screen, width, height.min(screen.height));
    let inner = frame(JOBS_TITLE, Some(JOBS_HINT), look, rect, buf);
    let text_width = usize::from(inner.width);
    let accent = look.accent();
    for (index, (job, y)) in jobs.iter().zip((inner.y..).step_by(usize::from(ROWS_PER_JOB))).enumerate() {
        if y + 1 >= inner.bottom() {
            break;
        }
        let selected = index == list.selected;
        let label_style = if selected { styles.overlay_selected } else { styles.overlay };
        let (fill, track) = bar(job.fraction().unwrap_or_default());
        let label = activity(job, now).label;
        let room = text_width.saturating_sub(BAR_WIDTH + LABEL_GAP.width() * 2);
        let head = Line::from(vec![
            Span::styled(fill, styles.overlay.fg(accent.from.into())),
            Span::styled(track, styles.overlay.add_modifier(Modifier::DIM)),
            Span::styled(LABEL_GAP, styles.overlay),
            Span::styled(format!("{}{LABEL_GAP}", format::truncate_end(&label, room)), label_style),
        ]);
        head.render(Rect { y, height: 1, ..inner }, buf);
        let current = job.progress.as_ref().map(|progress| progress.current.display().to_string()).unwrap_or_default();
        Line::styled(format::truncate_start(&current, text_width), styles.overlay.add_modifier(Modifier::DIM))
            .render(Rect { y: y + 1, height: 1, ..inner }, buf);
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
