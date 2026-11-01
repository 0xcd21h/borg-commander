//! The dialog K opens on a process: the signals to send it and the steps of its priority.

use super::{dim, text};
use crate::{Theme, dialog_view::centered, format};
use borg_core::{CONTROL_CHOICES, ControlChoice, ControlDialog, ProcessControl, ProcessSignal};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    text::Line,
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

const KEY_COLUMN: usize = 3;
const SIGNAL_COLUMN: usize = 6;
const LABEL_GAP: usize = 2;
const SIDE_PADDING: u16 = 2;
const BORDER: u16 = 2;
const SUBJECT_ROWS: u16 = 2;

fn label(choice: ControlChoice) -> &'static str {
    match choice {
        ControlChoice::Signal(ProcessSignal::Terminate) => tr!(process_signal_terminate),
        ControlChoice::Signal(ProcessSignal::Interrupt) => tr!(process_signal_interrupt),
        ControlChoice::Signal(ProcessSignal::HangUp) => tr!(process_signal_hang_up),
        ControlChoice::Signal(ProcessSignal::Kill) => tr!(process_signal_kill),
        ControlChoice::Signal(ProcessSignal::Stop) => tr!(process_signal_stop),
        ControlChoice::Signal(ProcessSignal::Continue) => tr!(process_signal_continue),
        ControlChoice::LowerPriority => tr!(process_priority_lower),
        ControlChoice::RaisePriority => tr!(process_priority_raise),
    }
}

/// What the choice does, and whether it can be chosen now.
fn about(dialog: &ControlDialog, choice: ControlChoice) -> (String, bool) {
    match (choice, dialog.control(choice)) {
        (ControlChoice::Signal(signal), _) => (
            match signal {
                ProcessSignal::Terminate => tr!(process_signal_terminate_about),
                ProcessSignal::Interrupt => tr!(process_signal_interrupt_about),
                ProcessSignal::HangUp => tr!(process_signal_hang_up_about),
                ProcessSignal::Kill => tr!(process_signal_kill_about),
                ProcessSignal::Stop => tr!(process_signal_stop_about),
                ProcessSignal::Continue => tr!(process_signal_continue_about),
            }
            .to_owned(),
            true,
        ),
        (_, Some(ProcessControl::Priority(to))) => {
            (tr!(process_priority_change, from = dialog.nice.unwrap_or(to), to = to), true)
        }
        (_, _) => match dialog.nice {
            Some(nice) => (tr!(process_priority_limit, nice = nice), false),
            None => (tr!(process_priority_unknown).to_owned(), false),
        },
    }
}

fn signal_name(choice: ControlChoice) -> &'static str {
    match choice {
        ControlChoice::Signal(signal) => signal.name(),
        _ => "",
    }
}

fn lines(dialog: &ControlDialog) -> Vec<(String, String, bool)> {
    let widest = CONTROL_CHOICES.iter().map(|&choice| label(choice).width()).max().unwrap_or_default();
    CONTROL_CHOICES
        .iter()
        .map(|&choice| {
            let (about, enabled) = about(dialog, choice);
            let head = format!(
                "{}{}{}",
                format::pad_end(&choice.key().to_ascii_uppercase().to_string(), KEY_COLUMN),
                format::pad_end(label(choice), widest + LABEL_GAP),
                format::pad_end(signal_name(choice), SIGNAL_COLUMN),
            );
            (head, about, enabled)
        })
        .collect()
}

pub(crate) fn rect(dialog: &ControlDialog, area: Rect) -> Rect {
    let subject = tr!(process_control_subject, name = dialog.name, pid = dialog.process.pid);
    let widest = lines(dialog)
        .iter()
        .map(|(head, about, _)| head.width() + about.width())
        .chain([subject.width(), tr!(process_control_keys).width(), tr!(process_control_title).width()])
        .max()
        .unwrap_or_default();
    let width = u16::try_from(widest).unwrap_or(u16::MAX).saturating_add(BORDER + SIDE_PADDING * 2);
    let height = u16::try_from(CONTROL_CHOICES.len()).unwrap_or(u16::MAX).saturating_add(BORDER + SUBJECT_ROWS);
    centered(area, width.min(area.width), height.min(area.height))
}

/// The choice's row under `point`, when it is on one.
pub(crate) fn choice_at(dialog: &ControlDialog, area: Rect, point: Position) -> (Rect, Option<usize>) {
    let rect = rect(dialog, area);
    let top = rect.y + 1 + SUBJECT_ROWS;
    let inside = point.x > rect.x && point.x < rect.right().saturating_sub(1) && point.y >= top;
    let row = inside.then(|| usize::from(point.y - top)).filter(|row| *row < CONTROL_CHOICES.len());
    (rect, row)
}

pub(crate) fn draw(dialog: &ControlDialog, theme: &Theme, area: Rect, buf: &mut Buffer) -> Rect {
    let styles = &theme.styles;
    let rect = rect(dialog, area);
    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(tr!(process_control_title), styles.overlay_border).centered())
        .title_bottom(Line::styled(tr!(process_control_keys), styles.overlay).centered());
    let inner = block.inner(rect);
    block.render(rect, buf);
    let padded = Rect { x: inner.x + SIDE_PADDING, width: inner.width.saturating_sub(SIDE_PADDING * 2), ..inner };
    let line = |offset: u16| Rect { y: inner.y + offset, height: u16::from(offset < inner.height), ..padded };
    let subject = tr!(process_control_subject, name = dialog.name, pid = dialog.process.pid);
    text(buf, line(0), subject, styles.overlay);
    for (index, (head, about, enabled)) in lines(dialog).into_iter().enumerate() {
        let Ok(offset) = u16::try_from(index) else { break };
        let area = line(SUBJECT_ROWS + offset);
        if area.is_empty() {
            break;
        }
        let selected = index == dialog.selected;
        let style = if selected { styles.overlay_selected } else { styles.overlay };
        buf.set_style(Rect { x: inner.x, width: inner.width, ..area }, style);
        let head_style = if enabled || selected { style } else { dim(theme) };
        let about_style = if selected { style } else { dim(theme) };
        let head_width = u16::try_from(head.width()).unwrap_or(u16::MAX).min(area.width);
        text(buf, Rect { width: head_width, ..area }, head, head_style);
        text(buf, Rect { x: area.x + head_width, width: area.width - head_width, ..area }, about, about_style);
    }
    rect
}
