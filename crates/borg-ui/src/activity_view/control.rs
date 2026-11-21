//! The dialog K opens on a process: the signals to send it and the steps of its priority.

use super::choices::Choices;
use crate::{Theme, format};
use borg_core::{CONTROL_CHOICES, ControlChoice, ControlDialog, ProcessControl, ProcessSignal};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
};
use unicode_width::UnicodeWidthStr;

const KEY_COLUMN: usize = 3;
const SIGNAL_COLUMN: usize = 6;
const LABEL_GAP: usize = 2;

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

fn choices(dialog: &ControlDialog) -> Choices<'static> {
    Choices {
        title: tr!(process_control_title).into(),
        subject: tr!(process_control_subject, name = dialog.name, pid = dialog.process.pid),
        keys: tr!(process_control_keys),
        rows: lines(dialog),
        selected: dialog.selected,
    }
}

/// The choice's row under `point`, when it is on one.
pub(crate) fn choice_at(dialog: &ControlDialog, area: Rect, point: Position) -> (Rect, Option<usize>) {
    super::choices::choice_at(&choices(dialog), area, point)
}

pub(crate) fn draw(dialog: &ControlDialog, theme: &Theme, area: Rect, buf: &mut Buffer) -> Rect {
    super::choices::draw(&choices(dialog), theme, area, buf)
}
