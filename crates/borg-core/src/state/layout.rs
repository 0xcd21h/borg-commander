//! Where things go on the screen: the panels' split, their frames and fading, and fitting them to the terminal.

use super::{
    AppState, OPAQUE, PANEL_FRAME_ROWS, RESULT_FRAME_ROWS, RESULT_PREVIEW_DIVISOR, RESULT_PREVIEW_MIN_ROWS, Side,
    TRANSPARENT, View,
};
use crate::{
    Easing, Fade, Layout, Motion,
    layout::{Chrome, clamp_split},
};
use std::time::Instant;

impl AppState {
    pub fn chrome(&self) -> Chrome {
        let prefs = &self.settings.preferences;
        Chrome {
            menu_bar: prefs.menu_bar,
            status_line: prefs.status_line,
            prompt_rows: self.shell.observed.command_rows.max(1).min((self.rows / 2).max(1)),
        }
    }

    pub fn results_expanded(&self) -> bool {
        self.active_panel().results.as_ref().is_some_and(|results| results.expanded)
    }

    pub fn layout(&self, now: Instant) -> Layout {
        let mut layout = Layout::new(self.cols, self.rows, self.chrome(), self.split.value(now));
        if let Some(side) = self.activity.side {
            if self.activity.inspecting() {
                layout.panels = inspection_panels(layout.panels);
            }
            if self.activity.expanded {
                let left = layout.panels[0];
                let right = layout.panels[1];
                let focus = if self.activity.preview_focused { side.other() } else { side };
                layout.panels[focus.index()] =
                    ratatui::layout::Rect { width: right.right().saturating_sub(left.x), ..left };
                layout.panels[focus.other().index()] = Default::default();
            }
            return layout;
        }
        if let Some(side) = self.network.side
            && self.network.expanded
        {
            let left = layout.panels[0];
            let right = layout.panels[1];
            let focus = if self.network.details_focused { side.other() } else { side };
            layout.panels[focus.index()] =
                ratatui::layout::Rect { width: right.right().saturating_sub(left.x), ..left };
            layout.panels[focus.other().index()] = Default::default();
            return layout;
        }
        if let Some(side) = self.docker.side
            && self.docker.expanded
        {
            let left = layout.panels[0];
            let right = layout.panels[1];
            let focus = if self.docker.details_focused { side.other() } else { side };
            layout.panels[focus.index()] =
                ratatui::layout::Rect { width: right.right().saturating_sub(left.x), ..left };
            layout.panels[focus.other().index()] = Default::default();
            return layout;
        }
        if self.results_expanded() {
            let left = layout.panels[0];
            let right = layout.panels[1];
            let mut whole = ratatui::layout::Rect { width: right.right().saturating_sub(left.x), ..left };
            let other = if self.aside.is_some() && whole.height >= RESULT_PREVIEW_MIN_ROWS {
                let height = whole.height / RESULT_PREVIEW_DIVISOR;
                whole.height -= height;
                ratatui::layout::Rect { y: whole.bottom(), height, ..whole }
            } else {
                ratatui::layout::Rect::default()
            };
            layout.panels[self.active.index()] = whole;
            layout.panels[self.active.other().index()] = other;
        }
        layout
    }

    /// The activity monitor's own panes, which stay beneath the inspection window.
    pub fn monitor_panels(&self, now: Instant) -> [ratatui::layout::Rect; 2] {
        Layout::new(self.cols, self.rows, self.chrome(), self.split.value(now)).panels
    }

    pub fn split_target(&self) -> f32 {
        self.split.target()
    }

    pub fn motion(&self) -> Motion {
        self.settings.effective_motion()
    }

    pub fn panel_opacity(&self, now: Instant) -> f32 {
        self.fade.value(now)
    }

    pub(crate) fn sync_fade(&mut self, now: Instant) {
        let target = match self.effective_view() {
            View::Panels => OPAQUE,
            View::Shell => TRANSPARENT,
        };
        if (self.fade.target() - target).abs() > f32::EPSILON {
            self.fade = self.fade.toward(target, now, self.motion().fade, Easing::InOutCubic);
        }
    }

    pub(crate) fn set_split(&mut self, split: f32, now: Instant) {
        let target = clamp_split(split, self.cols);
        if (self.split.target() - target).abs() > f32::EPSILON {
            self.split = self.split.toward(target, now, self.motion().fade, Easing::OutCubic);
        }
    }

    pub(crate) fn resize(&mut self, cols: u16, rows: u16, now: Instant) {
        self.cols = cols;
        self.rows = rows;
        self.split = Fade::settled(clamp_split(self.split.target(), cols), now);
        self.refit();
        crate::viewer::resize(self);
        crate::editor::resize(self);
    }

    pub(crate) fn refit(&mut self) {
        let layout = self.layout(self.epoch);
        for side in Side::ALL {
            let panel = self.panel_mut(side);
            let reserved = if panel.is_results() { RESULT_FRAME_ROWS } else { PANEL_FRAME_ROWS };
            panel.set_rows(usize::from(layout.panels[side.index()].height.saturating_sub(reserved)));
        }
        crate::editor::repaint(self);
    }
}

/// Columns and rows left around the process inspection window, which opens over the
/// activity monitor. The overlay shadow fills the right and bottom margins.
pub(super) const INSPECTION_MARGIN: (u16, u16) = (2, 1);

/// The list and preview panes of the inspection window, splitting it like the panels.
pub(super) fn inspection_panels([left, right]: [ratatui::layout::Rect; 2]) -> [ratatui::layout::Rect; 2] {
    let whole = ratatui::layout::Rect { width: right.right().saturating_sub(left.x), ..left };
    let (columns, rows) = INSPECTION_MARGIN;
    let outer = ratatui::layout::Rect {
        x: whole.x.saturating_add(columns),
        y: whole.y.saturating_add(rows),
        width: whole.width.saturating_sub(columns * 2),
        height: whole.height.saturating_sub(rows * 2),
    };
    let split = (u32::from(outer.width) * u32::from(left.width) / u32::from(whole.width.max(1))) as u16;
    [
        ratatui::layout::Rect { width: split, ..outer },
        ratatui::layout::Rect { x: outer.x + split, width: outer.width - split, ..outer },
    ]
}
