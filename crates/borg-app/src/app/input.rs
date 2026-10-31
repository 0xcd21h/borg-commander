//! Keys, mouse and the terminal's events, and the wheel's bursts.

use super::{App, Flow, Plain};
use anyhow::Result;
use borg_core::{Msg, View};
use borg_i18n::tr;
use borg_term::{Scroll, encode_mouse};
use borg_ui::mouse_input;
use crossterm::event::{Event, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use std::time::{Duration, Instant};

impl App {
    pub(super) fn on_local_event(&mut self, event: Event) -> Result<Flow> {
        let takes_control = match &event {
            Event::Key(key) => key.kind != KeyEventKind::Release,
            Event::Paste(_) => true,
            Event::Mouse(mouse) => !matches!(mouse.kind, MouseEventKind::Moved),
            _ => false,
        };
        if takes_control && self.native_mirror.local_input() {
            self.state.notice = Some(tr!(mirror_local_control).into());
        }
        self.on_terminal_event(event)
    }

    pub(super) fn on_mirror_input(&mut self, input: crate::native_mirror::Input) -> Result<Flow> {
        if input.events.is_empty() && input.reply.is_none() {
            self.state.notice = Some(match self.native_mirror.controller() {
                Some(origin) => {
                    tr!(mirror_remote_control, computer = origin.split(" [").next().unwrap_or(&origin))
                }
                None => tr!(mirror_control_released).into(),
            });
            return Ok(Flow::Continue);
        }
        if !self.native_mirror.controls(&input.origin) {
            if let Some(reply) = input.reply {
                let _ = reply.send(crate::agent::Reply::error("Mirror control was released"));
            }
            return Ok(Flow::Continue);
        }
        let mut flow = Flow::Continue;
        for event in input.events {
            flow = self.on_terminal_event(event)?;
            if matches!(flow, Flow::Quit) {
                break;
            }
        }
        if let Some(reply) = input.reply {
            let _ = reply.send(crate::agent::Reply::ok("Input sent"));
        }
        Ok(flow)
    }

    pub(super) fn on_terminal_event(&mut self, event: Event) -> Result<Flow> {
        // While text is shown plainly, a key brings borg back and does nothing else.
        if self.plain.is_some() {
            if let Event::Key(key) = event
                && key.kind == KeyEventKind::Press
            {
                self.plain = Some(Plain::Ending);
            }
            return Ok(Flow::Continue);
        }
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                self.last_click = None;
                self.observe(Msg::Observe)?;
                self.dispatch(Msg::Key(key))
            }
            Event::Paste(text) => {
                self.observe(Msg::Observe)?;
                self.dispatch(Msg::Paste(text))
            }
            Event::Resize(cols, rows) => self.resize(cols, rows),
            Event::Mouse(event) => {
                self.observe(Msg::Observe)?;
                self.on_mouse(event)
            }
            _ => Ok(Flow::Continue),
        }
    }

    pub(super) fn on_mouse(&mut self, event: MouseEvent) -> Result<Flow> {
        let now = Instant::now();
        self.state.idle_since = now;
        if self.state.screensaver {
            return self.dispatch(Msg::Pointer(borg_core::Pointer::DismissMenu));
        }
        if matches!(
            event.kind,
            MouseEventKind::ScrollUp
                | MouseEventKind::ScrollDown
                | MouseEventKind::ScrollLeft
                | MouseEventKind::ScrollRight
        ) {
            let target = if self.state.effective_view() == View::Shell && self.state.overlay.is_none() {
                WheelTarget::Console
            } else if self.state.viewer.is_some() {
                WheelTarget::Viewer
            } else if self.state.overlay.is_some() {
                WheelTarget::Dialog
            } else {
                WheelTarget::Panels
            };
            if !self.wheel.accepts(target, now) {
                return Ok(Flow::Continue);
            }
        }
        if self.state.effective_view() == View::Shell && self.state.overlay.is_none() {
            let captured = self.state.shell.captured();
            let emulator = self.console_emulator();
            if event.row >= emulator.screen().rows() {
                return Ok(Flow::Continue);
            }
            if captured {
                if let Some(bytes) = encode_mouse(event, emulator.mode()) {
                    self.write_console(bytes)?;
                }
            } else {
                let scroll = match event.kind {
                    MouseEventKind::ScrollUp => Some(Scroll::Delta(WHEEL_LINES)),
                    MouseEventKind::ScrollDown => Some(Scroll::Delta(-WHEEL_LINES)),
                    _ => None,
                };
                if let Some(scroll) = scroll {
                    emulator.scroll(scroll);
                }
            }
            return Ok(Flow::Continue);
        }
        let left = event.kind == MouseEventKind::Down(MouseButton::Left);
        let double = left
            && self.last_click.is_some_and(|(col, row, at)| {
                (col, row) == (event.column, event.row) && now.saturating_duration_since(at) <= DOUBLE_CLICK
            });
        let msg = mouse_input(&self.state, event, double, now);
        if left {
            self.last_click = match &msg {
                Some(Msg::Pointer(
                    borg_core::Pointer::Panel { mark: false, .. }
                    | borg_core::Pointer::EditorClick { select: false, word: false, .. },
                )) if !double => Some((event.column, event.row, now)),
                _ => None,
            };
        } else if matches!(event.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown) {
            self.last_click = None;
        }
        match msg {
            Some(msg) => self.dispatch(msg),
            None => Ok(Flow::Continue),
        }
    }
}

pub(super) const DOUBLE_CLICK: Duration = Duration::from_millis(400);

pub(super) const WHEEL_LINES: i32 = 3;

/// Wheel events closer together than this belong to one burst.
pub(super) const WHEEL_BURST: Duration = Duration::from_millis(200);

/// What a wheel event scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WheelTarget {
    Console,
    Viewer,
    Dialog,
    Panels,
}

/// Trackpads keep sending wheel events after the fingers lift. A burst stays with what it
/// started scrolling, so switching screens mid-burst never scrolls the new one.
#[derive(Debug, Default)]
pub(super) struct WheelBurst {
    pub(super) last: Option<(Instant, WheelTarget)>,
}

impl WheelBurst {
    pub(super) fn accepts(&mut self, target: WheelTarget, now: Instant) -> bool {
        let owner = match self.last {
            Some((at, owner)) if now.saturating_duration_since(at) <= WHEEL_BURST => owner,
            _ => target,
        };
        self.last = Some((now, owner));
        owner == target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wheel_burst_stays_with_what_it_started_scrolling() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut wheel = WheelBurst::default();
        assert!(wheel.accepts(WheelTarget::Console, at(0)));
        assert!(wheel.accepts(WheelTarget::Console, at(16)));
        assert!(!wheel.accepts(WheelTarget::Panels, at(32)), "momentum after switching screens is dropped");
        assert!(!wheel.accepts(WheelTarget::Panels, at(200)));
        assert!(wheel.accepts(WheelTarget::Panels, at(200) + WHEEL_BURST + Duration::from_millis(1)));
    }
}
