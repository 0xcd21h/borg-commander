use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone)]
pub struct Theme {
    pub panel: Style,
    pub border: Style,
    pub border_active: Style,
    pub title: Style,
    pub title_active: Style,
    pub dir: Style,
    pub file: Style,
    pub executable: Style,
    pub link: Style,
    pub other: Style,
    pub meta: Style,
    pub cursor: Style,
    pub cursor_inactive: Style,
    pub footer: Style,
    pub notice: Style,
    pub key: Style,
    pub key_label: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            panel: Style::new().bg(Color::Reset),
            border: Style::new().fg(Color::DarkGray),
            border_active: Style::new().fg(Color::Cyan),
            title: Style::new().fg(Color::Gray),
            title_active: Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            dir: Style::new().fg(Color::LightBlue).add_modifier(Modifier::BOLD),
            file: Style::new().fg(Color::Gray),
            executable: Style::new().fg(Color::LightGreen),
            link: Style::new().fg(Color::Cyan),
            other: Style::new().fg(Color::Yellow),
            meta: Style::new().fg(Color::DarkGray),
            cursor: Style::new().fg(Color::White).bg(Color::Indexed(24)),
            cursor_inactive: Style::new().bg(Color::Indexed(236)),
            footer: Style::new().fg(Color::DarkGray),
            notice: Style::new().fg(Color::LightRed),
            key: Style::new().fg(Color::White),
            key_label: Style::new().fg(Color::Black).bg(Color::Cyan),
        }
    }
}
