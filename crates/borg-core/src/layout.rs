use ratatui::layout::{Constraint, Layout as Split, Rect};

const PROMPT_ROWS: u16 = 1;
const KEYBAR_ROWS: u16 = 1;
const MENU_ROWS: u16 = 1;
const STATUS_ROWS: u16 = 1;
const BORDER_ROWS: u16 = 2;
pub const MIN_PANEL_WIDTH: u16 = 16;
pub const EVEN_SPLIT: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chrome {
    pub menu_bar: bool,
    pub status_line: bool,
    pub prompt_rows: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub screen: Rect,
    pub menu: Option<Rect>,
    pub panels: [Rect; 2],
    pub status: Option<Rect>,
    pub prompt: Rect,
    pub keybar: Rect,
}

impl Layout {
    pub fn new(cols: u16, rows: u16, chrome: Chrome, split: f32) -> Self {
        let screen = Rect::new(0, 0, cols, rows);
        let menu_rows = if chrome.menu_bar { MENU_ROWS } else { 0 };
        let status_rows = if chrome.status_line { STATUS_ROWS } else { 0 };
        let [menu, body, status, prompt, keybar] = Split::vertical([
            Constraint::Length(menu_rows),
            Constraint::Fill(1),
            Constraint::Length(status_rows),
            Constraint::Length(chrome.prompt_rows.max(PROMPT_ROWS)),
            Constraint::Length(KEYBAR_ROWS),
        ])
        .areas(screen);

        let left_width = split_width(body.width, split);
        let left = Rect { width: left_width, ..body };
        let right = Rect { x: body.x + left_width, width: body.width - left_width, ..body };

        Self {
            screen,
            menu: chrome.menu_bar.then_some(menu),
            panels: [left, right],
            status: chrome.status_line.then_some(status),
            prompt,
            keybar,
        }
    }

    pub fn panel_rows(&self) -> usize {
        usize::from(self.panels[0].height.saturating_sub(BORDER_ROWS))
    }
}

pub fn clamp_split(split: f32, width: u16) -> f32 {
    let width = f32::from(width.max(1));
    let min = (f32::from(MIN_PANEL_WIDTH) / width).min(EVEN_SPLIT);
    split.clamp(min, 1.0 - min)
}

fn split_width(width: u16, split: f32) -> u16 {
    let wanted = (f32::from(width) * clamp_split(split, width)).round();
    (wanted as u16).min(width)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BARE: Chrome = Chrome { menu_bar: false, status_line: false, prompt_rows: 1 };
    const FULL: Chrome = Chrome { menu_bar: true, status_line: true, prompt_rows: 1 };

    #[test]
    fn even_split_divides_width() {
        let layout = Layout::new(100, 30, BARE, EVEN_SPLIT);
        assert_eq!((layout.panels[0].width, layout.panels[1].width), (50, 50));
        assert_eq!(layout.panel_rows(), 30 - 2 - 2);
    }

    #[test]
    fn optional_rows_shrink_panels() {
        let layout = Layout::new(100, 30, FULL, EVEN_SPLIT);
        assert_eq!(layout.menu.map(|r| r.y), Some(0));
        assert_eq!(layout.panels[0].y, 1);
        assert_eq!(layout.status.map(|r| r.y), Some(27));
        assert_eq!(layout.panel_rows(), 30 - 4 - 2);
    }

    #[test]
    fn uneven_split_is_clamped_to_minimum_width() {
        let layout = Layout::new(100, 30, BARE, 0.0);
        assert_eq!(layout.panels[0].width, MIN_PANEL_WIDTH);
        assert_eq!(layout.panels[1].x + layout.panels[1].width, 100);

        let wide = Layout::new(100, 30, BARE, 0.7);
        assert_eq!((wide.panels[0].width, wide.panels[1].width), (70, 30));
    }
}
