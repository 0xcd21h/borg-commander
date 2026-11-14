//! Toasts: small notes in the bottom-right corner that a job started, ended or waits for an
//! answer. They slide in from the right, stack with the newest at the bottom, and fade out.

use std::time::Instant;

use borg_core::{AppState, How, Job, Toast, ToastEvent, View};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{blend::crossfade, format, job_view, jobs_sheet, look::Look};

const BOX_WIDTH: u16 = 40;
const BOX_ROWS: u16 = 3;
/// Screens this wide get toasts in boxes; narrower ones down to `LINE_SCREEN` get one line each.
const BOX_SCREEN: u16 = 60;
const LINE_SCREEN: u16 = 40;
const LINES_MOST: usize = 2;
/// Screens with fewer rows get no toasts.
const LEAST_ROWS: u16 = 14;
/// Rows the full-screen console, editor or viewer keeps at its bottom.
const FULL_SCREEN_FOOTER: u16 = 2;
const PADDING: u16 = 1;
const STARTED: &str = "▶";
const ASKING: &str = "!";
const DONE: &str = "✓";
const CANCELLED: &str = "■";
const FAILED: &str = "✗";
const SEPARATOR: &str = " · ";
const FULLY_SHOWN: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Accent,
    Warn,
    Dim,
}

struct Content {
    title: String,
    body: String,
    key: Option<&'static str>,
    tone: Tone,
}

/// The finished job Alt-G opens, whose toast offers it.
fn opened_by_key(state: &AppState) -> Option<u64> {
    borg_core::shown_jobs(&state.jobs).into_iter().find(|job| !job.running() && job.reveal.is_some()).map(|job| job.id)
}

fn content(toast: &Toast, job: &Job, keys: bool, opens: Option<u64>, now: Instant) -> Option<Content> {
    let (icon, title, tone) = match toast.event {
        ToastEvent::Started => (STARTED, job.kind.title().to_owned(), Tone::Accent),
        ToastEvent::Asking => (ASKING, tr!(job_waits, job = job.kind.title()), Tone::Warn),
        ToastEvent::Ended(how) => {
            let summary = job.ending()?.summary.clone();
            match how {
                How::Done => (DONE, summary, Tone::Accent),
                How::Cancelled => (CANCELLED, summary, Tone::Dim),
                How::Failed => (FAILED, summary, Tone::Warn),
            }
        }
    };
    let body = match toast.event {
        ToastEvent::Ended(_) => format!("{}{SEPARATOR}{}", job.subject, job_view::took(job, now)),
        ToastEvent::Started | ToastEvent::Asking => job.subject.clone(),
    };
    let key = match toast.event {
        ToastEvent::Asking => tr!(job_toast_answer),
        ToastEvent::Ended(_) if opens == Some(job.id) => tr!(job_toast_open),
        ToastEvent::Started | ToastEvent::Ended(_) => tr!(job_toast_jobs),
    };
    Some(Content { title: format!("{icon} {title}"), body, key: keys.then_some(key), tone })
}

fn tone_style(tone: Tone, look: &Look<'_>) -> Style {
    let styles = look.styles();
    match tone {
        Tone::Accent => styles.overlay.fg(look.accent().from.into()),
        Tone::Warn => styles.overlay.fg(look.fg(styles.notice).into()),
        Tone::Dim => styles.overlay.add_modifier(Modifier::DIM),
    }
}

fn boxed(content: &Content, width: u16, look: &Look<'_>) -> Buffer {
    let styles = look.styles();
    let rect = Rect::new(0, 0, width, BOX_ROWS);
    let mut buf = Buffer::empty(rect);
    let tone = tone_style(content.tone, look);
    let room = usize::from(width.saturating_sub(4));
    let mut block =
        Block::bordered().border_type(look.theme.border.into()).style(styles.overlay).border_style(tone).title(
            Line::styled(
                format!(" {} ", format::truncate_end(&content.title, room)),
                tone.add_modifier(Modifier::BOLD),
            ),
        );
    if let Some(key) = content.key {
        block = block.title_bottom(Line::styled(format!(" {key} "), styles.overlay).right_aligned());
    }
    let inner = block.inner(rect);
    block.render(rect, &mut buf);
    let body = Rect { x: inner.x + PADDING, width: inner.width.saturating_sub(PADDING * 2), ..inner };
    Line::styled(format::truncate_end(&content.body, usize::from(body.width)), styles.overlay).render(body, &mut buf);
    buf
}

fn one_line(content: &Content, room: u16, look: &Look<'_>) -> Buffer {
    let key = content.key.map(|key| format!("{SEPARATOR}{key}")).unwrap_or_default();
    let text = format!(" {}{key} ", content.title);
    let width = (text.width() as u16).min(room);
    let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
    let style = tone_style(content.tone, look);
    Line::styled(format::truncate_end(&text, usize::from(width)), style).render(buf.area, &mut buf);
    buf
}

/// Where toasts stack: inside the panels above their footers, or above the bottom lines of
/// the full-screen console, editor or viewer.
fn area(state: &AppState, now: Instant) -> Rect {
    let layout = state.layout(now);
    let in_panels = state.effective_view() == View::Panels
        && state.editor.is_none()
        && state.viewer.is_none()
        && state.panel_opacity(now) >= FULLY_SHOWN;
    if in_panels {
        let panels = jobs_sheet::panels_area(&layout);
        Rect {
            x: panels.x + 1,
            y: panels.y + 1,
            width: panels.width.saturating_sub(2),
            height: panels.height.saturating_sub(2),
        }
    } else {
        let screen = layout.screen;
        Rect { height: screen.height.saturating_sub(FULL_SCREEN_FOOTER), ..screen }
    }
}

/// Copies `toast` to `buf` with its left edge at `x`, cut off at the right of `area`.
fn place(toast: &Buffer, x: u16, y: u16, area: Rect, buf: &mut Buffer) {
    for row in 0..toast.area.height {
        for column in 0..toast.area.width {
            let at = (x + column, y + row);
            if at.0 < area.right() && buf.area.contains(at.into()) {
                buf[at] = toast[(column, row)].clone();
            }
        }
    }
}

pub(crate) fn draw(state: &AppState, look: &Look<'_>, now: Instant, buf: &mut Buffer) {
    if state.overlay.is_some() || state.screensaver || state.intro.is_some() {
        return;
    }
    let screen = state.layout(now).screen;
    if screen.width < LINE_SCREEN || screen.height < LEAST_ROWS {
        return;
    }
    let toasts = borg_core::visible_toasts(state, now);
    let area = area(state, now);
    let keys = borg_core::toast_keys_work(state);
    let opens = opened_by_key(state);
    let boxes = screen.width >= BOX_SCREEN;
    let (most, rows) = if boxes { (borg_core::MOST_TOASTS, BOX_ROWS) } else { (LINES_MOST, 1) };
    let motion = state.motion();
    let mut bottom = area.bottom();
    for toast in toasts.iter().rev().take(most) {
        let Some(content) =
            state.jobs.iter().find(|job| job.id == toast.job).and_then(|job| content(toast, job, keys, opens, now))
        else {
            continue;
        };
        if bottom < area.y + rows {
            break;
        }
        bottom -= rows;
        let painted =
            if boxes { boxed(&content, BOX_WIDTH.min(area.width), look) } else { one_line(&content, area.width, look) };
        let arrival = toast.arrival(now, motion);
        let width = painted.area.width;
        let slide = ((FULLY_SHOWN - arrival) * f32::from(width)).round() as u16;
        let x = area.right().saturating_sub(width) + slide;
        let opacity = arrival * toast.presence(now, motion);
        if opacity >= FULLY_SHOWN {
            place(&painted, x, bottom, area, buf);
        } else {
            let under = buf.clone();
            let mut over = buf.clone();
            place(&painted, x, bottom, area, &mut over);
            crossfade(&over, &under, opacity, look.palette, buf);
        }
    }
}
