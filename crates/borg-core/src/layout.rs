use ratatui::layout::{Constraint, Layout as Split, Rect};

const PROMPT_ROWS: u16 = 1;
const KEYBAR_ROWS: u16 = 1;
const BORDER_ROWS: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub screen: Rect,
    pub panels: [Rect; 2],
    pub prompt: Rect,
    pub keybar: Rect,
}

impl Layout {
    pub fn new(cols: u16, rows: u16) -> Self {
        let screen = Rect::new(0, 0, cols, rows);
        let [body, prompt, keybar] =
            Split::vertical([Constraint::Fill(1), Constraint::Length(PROMPT_ROWS), Constraint::Length(KEYBAR_ROWS)])
                .areas(screen);
        let [left, right] = Split::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).areas(body);
        Self { screen, panels: [left, right], prompt, keybar }
    }

    pub fn viewport(&self) -> usize {
        usize::from(self.panels[0].height.saturating_sub(BORDER_ROWS))
    }
}
