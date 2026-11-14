//! Toasts: small notes in the bottom-right corner that a job started, ended or waits for an
//! answer. They slide in from the right, stack with the newest at the bottom, and fade out. A
//! click on a toast's close mark closes it.

use std::time::Instant;

use borg_core::{AppState, How, Job, Msg, Pointer, Toast, ToastEvent, View};
use borg_i18n::tr;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
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
/// The borders and spaces around a box's title.
const TITLE_FRAME: u16 = 4;
/// The border between a box's title and its close mark.
const MARK_GAP: u16 = 1;
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
    keys: Vec<&'static str>,
    tone: Tone,
}

enum Shape {
    Boxed,
    /// One line, with the text before its close mark.
    Line(String),
}

/// A toast that shows, and where.
struct Placed<'a> {
    toast: &'a Toast,
    content: Content,
    shape: Shape,
    rect: Rect,
}

impl Placed<'_> {
    /// Where its close mark is: at the right end of a box's top border, or of its line.
    fn mark(&self) -> Rect {
        let width = tr!(job_toast_close_mark).width() as u16;
        let corner = u16::from(matches!(self.shape, Shape::Boxed));
        Rect { x: self.rect.right().saturating_sub(width + corner), y: self.rect.y, width, height: 1 }
    }
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
    let keys = if keys { vec![key, tr!(job_toast_close)] } else { Vec::new() };
    Some(Content { title: format!("{icon} {title}"), body, keys, tone })
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
    let mark = tr!(job_toast_close_mark);
    let room = usize::from(width.saturating_sub(TITLE_FRAME + MARK_GAP + mark.width() as u16));
    let mut block = Block::bordered()
        .border_type(look.theme.border.into())
        .style(styles.overlay)
        .border_style(tone)
        .title(Line::styled(
            format!(" {} ", format::truncate_end(&content.title, room)),
            tone.add_modifier(Modifier::BOLD),
        ))
        .title_top(Line::styled(mark, tone).right_aligned());
    if !content.keys.is_empty() {
        let keys = content.keys.join(SEPARATOR);
        block = block.title_bottom(Line::styled(format!(" {keys} "), styles.overlay).right_aligned());
    }
    let inner = block.inner(rect);
    block.render(rect, &mut buf);
    let body = Rect { x: inner.x + PADDING, width: inner.width.saturating_sub(PADDING * 2), ..inner };
    Line::styled(format::truncate_end(&content.body, usize::from(body.width)), styles.overlay).render(body, &mut buf);
    buf
}

/// A one-line toast's text before its close mark, within `room` cells: the title, then the keys
/// that fit whole.
fn line_text(content: &Content, room: usize) -> String {
    let room = room.saturating_sub(tr!(job_toast_close_mark).width());
    let mut text = format!(" {}", content.title);
    for key in &content.keys {
        let more = format!("{SEPARATOR}{key}");
        if text.width() + more.width() + usize::from(PADDING) > room {
            break;
        }
        text.push_str(&more);
    }
    text.push(' ');
    format::truncate_end(&text, room)
}

fn one_line(content: &Content, text: &str, width: u16, look: &Look<'_>) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
    let style = tone_style(content.tone, look);
    Line::from(vec![Span::styled(text, style), Span::styled(tr!(job_toast_close_mark), style)])
        .render(buf.area, &mut buf);
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

/// The toasts that show and where, newest at the bottom, each slid in as far as it arrived, with
/// the area they stack in.
fn placed(state: &AppState, now: Instant) -> (Rect, Vec<Placed<'_>>) {
    let hidden = state.overlay.is_some() || state.approval.is_some() || state.screensaver || state.intro.is_some();
    let screen = state.layout(now).screen;
    if hidden || screen.width < LINE_SCREEN || screen.height < LEAST_ROWS {
        return (Rect::default(), Vec::new());
    }
    let area = area(state, now);
    let keys = borg_core::toast_keys_work(state);
    let opens = opened_by_key(state);
    let boxes = screen.width >= BOX_SCREEN;
    let (most, rows) = if boxes { (borg_core::MOST_TOASTS, BOX_ROWS) } else { (LINES_MOST, 1) };
    let motion = state.motion();
    let mut bottom = area.bottom();
    let mut placed = Vec::new();
    for toast in borg_core::visible_toasts(state, now).into_iter().rev().take(most) {
        let Some(content) =
            state.jobs.iter().find(|job| job.id == toast.job).and_then(|job| content(toast, job, keys, opens, now))
        else {
            continue;
        };
        if bottom < area.y + rows {
            break;
        }
        bottom -= rows;
        let (shape, width) = if boxes {
            (Shape::Boxed, BOX_WIDTH.min(area.width))
        } else {
            let text = line_text(&content, usize::from(area.width));
            let width = (text.width() + tr!(job_toast_close_mark).width()) as u16;
            (Shape::Line(text), width.min(area.width))
        };
        let slide = ((FULLY_SHOWN - toast.arrival(now, motion)) * f32::from(width)).round() as u16;
        let x = area.right().saturating_sub(width) + slide;
        placed.push(Placed { toast, content, shape, rect: Rect::new(x, bottom, width, rows) });
    }
    (area, placed)
}

pub(crate) fn draw(state: &AppState, look: &Look<'_>, now: Instant, buf: &mut Buffer) {
    let (area, placed) = placed(state, now);
    let motion = state.motion();
    for one in placed {
        let painted = match &one.shape {
            Shape::Boxed => boxed(&one.content, one.rect.width, look),
            Shape::Line(text) => one_line(&one.content, text, one.rect.width, look),
        };
        let opacity = one.toast.arrival(now, motion) * one.toast.presence(now, motion);
        if opacity >= FULLY_SHOWN {
            place(&painted, one.rect.x, one.rect.y, area, buf);
        } else {
            let under = buf.clone();
            let mut over = buf.clone();
            place(&painted, one.rect.x, one.rect.y, area, &mut over);
            crossfade(&over, &under, opacity, look.palette, buf);
        }
    }
}

/// A left click on a toast's close mark closes that toast, wherever toasts show.
pub fn toast_mouse(state: &AppState, event: MouseEvent, now: Instant) -> Option<Msg> {
    if event.kind != MouseEventKind::Down(MouseButton::Left) {
        return None;
    }
    let point = Position::new(event.column, event.row);
    borg_i18n::with_locale(state.settings.preferences.language, || {
        let (area, placed) = placed(state, now);
        let hit = placed.iter().find(|one| area.contains(point) && one.mark().contains(point))?;
        Some(Msg::Pointer(Pointer::CloseToast(hit.toast.job)))
    })
}
