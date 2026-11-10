//! The dialog that offers to install a missing program: why Borg needs it, the exact command,
//! what running it does, and what may still be needed afterwards. The service dialog draws its
//! offer the same way.

use crate::{
    Theme,
    activity_view::dim,
    dialog_view::{centered, message_lines},
    format,
};
use borg_core::{InstallOffer, Tool};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Block, Clear, Paragraph, Widget},
};

const MAX_WIDTH: u16 = 64;
const MARGIN: u16 = 4;
const SIDE_PADDING: u16 = 2;
const BORDER: u16 = 2;
/// A blank row above the text, and one between its parts.
const TOP_ROWS: u16 = 1;
const GAP_ROWS: u16 = 1;
/// How far the command sits in from the edge of its row.
const COMMAND_INSET: u16 = 1;

fn question(offer: &InstallOffer) -> String {
    let manager = offer.installer.manager.name();
    match offer.tool {
        Tool::Tcpdump => tr!(install_tcpdump_question, manager = manager),
        Tool::Bsdtar => tr!(install_bsdtar_question, manager = manager),
    }
}

/// What running the command does: who it runs as, whether it asks for a password, and whom it
/// installs the program for.
fn consequence(offer: &InstallOffer) -> String {
    let (installer, program) = (offer.installer, offer.tool.program());
    match installer.elevation {
        Some(elevation) => tr!(install_with_password, elevation = elevation.program(), program = program),
        None if installer.manager.needs_root() => tr!(install_as_root, program = program),
        None => tr!(install_without_root, manager = installer.manager.name(), program = program),
    }
}

fn note(tool: Tool) -> &'static str {
    match tool {
        Tool::Tcpdump => tr!(install_tcpdump_note),
        Tool::Bsdtar => tr!(install_bsdtar_note),
    }
}

pub(crate) enum Part {
    Text(Vec<Line<'static>>),
    /// The command, a row for each of its parts.
    Command(Vec<String>),
    /// What may still be needed afterwards, dimmed.
    Note(Vec<Line<'static>>),
}

impl Part {
    fn rows(&self) -> u16 {
        match self {
            Self::Text(lines) | Self::Note(lines) => u16::try_from(lines.len()).unwrap_or(u16::MAX),
            Self::Command(rows) => u16::try_from(rows.len()).unwrap_or(u16::MAX),
        }
    }
}

fn parts(offer: &InstallOffer, width: u16) -> Vec<Part> {
    vec![
        Part::Text(message_lines(&question(offer), width)),
        Part::Command(vec![offer.installer.command(offer.tool)]),
        Part::Text(message_lines(&consequence(offer), width)),
        Part::Note(message_lines(note(offer.tool), width)),
    ]
}

fn text_width(screen: Rect) -> u16 {
    screen.width.saturating_sub(MARGIN).min(MAX_WIDTH).saturating_sub(BORDER + SIDE_PADDING * 2)
}

/// Where an offer sits on `screen`, and its parts, made for the width of its text.
pub(crate) fn layout(screen: Rect, parts: impl FnOnce(u16) -> Vec<Part>) -> (Rect, Vec<Part>) {
    let width = text_width(screen);
    let parts = parts(width);
    let gaps = GAP_ROWS * u16::try_from(parts.len().saturating_sub(1)).unwrap_or(u16::MAX);
    let rows = parts.iter().map(Part::rows).fold(0, u16::saturating_add);
    let height = rows.saturating_add(BORDER + TOP_ROWS + gaps);
    let rect = centered(screen, width.saturating_add(BORDER + SIDE_PADDING * 2), height.min(screen.height));
    (rect, parts)
}

pub(crate) fn rect(offer: &InstallOffer, screen: Rect) -> Rect {
    layout(screen, |width| parts(offer, width)).0
}

pub(crate) fn draw(offer: &InstallOffer, theme: &Theme, screen: Rect, buf: &mut Buffer) -> Rect {
    let (rect, parts) = layout(screen, |width| parts(offer, width));
    let title = tr!(install_title, program = offer.tool.program());
    draw_offer(&title, tr!(install_keys), rect, parts, theme, buf);
    rect
}

/// Draws an offer in `rect`: its title on top, its keys at the bottom, and its parts between.
pub(crate) fn draw_offer(title: &str, keys: &str, rect: Rect, parts: Vec<Part>, theme: &Theme, buf: &mut Buffer) {
    let styles = &theme.styles;
    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(title.to_owned(), styles.overlay_border).centered())
        .title_bottom(Line::styled(keys.to_owned(), styles.overlay).centered());
    let inner = block.inner(rect);
    block.render(rect, buf);
    let mut area = Rect {
        x: inner.x + SIDE_PADDING,
        y: inner.y + TOP_ROWS,
        width: inner.width.saturating_sub(SIDE_PADDING * 2),
        height: inner.height.saturating_sub(TOP_ROWS),
    };
    for part in parts {
        let rows = part.rows().min(area.height);
        let here = Rect { height: rows, ..area };
        match part {
            Part::Text(lines) => Paragraph::new(lines).style(styles.overlay).render(here, buf),
            Part::Note(lines) => Paragraph::new(lines).style(dim(theme)).render(here, buf),
            Part::Command(commands) => {
                buf.set_style(here, styles.input);
                let inset = Rect { x: here.x + COMMAND_INSET, width: here.width.saturating_sub(COMMAND_INSET), ..here };
                let shown: Vec<Line> = commands
                    .iter()
                    .map(|command| Line::styled(format::truncate_end(command, usize::from(inset.width)), styles.input))
                    .collect();
                Paragraph::new(shown).render(inset, buf);
            }
        }
        let used = rows.saturating_add(GAP_ROWS).min(area.height);
        area = Rect { y: area.y + used, height: area.height - used, ..area };
    }
}
