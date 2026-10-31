//! Drawing frames: the scene, the terminal's size and what shows when Borg cannot draw.

use super::{App, Flow};
use crate::launch::Launch;
use anyhow::Result;
use borg_core::{Appearance, Image, Msg};
use borg_gfx::{Capabilities, Output, Protocol, TerminalColors, decode};
use borg_i18n::tr;
use borg_term::GridSize;
use borg_ui::{Palette, Pixels, Rgb, Scene, draw, local_now};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute, queue,
    terminal::{
        BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use jiff::Zoned;
use ratatui::DefaultTerminal;
use std::{io::Write as _, path::Path, time::Instant};

impl App {
    pub(super) fn render(&mut self, terminal: &mut DefaultTerminal, now: Instant) -> Result<()> {
        match self.plain.take() {
            Some(Plain::Waiting(text)) => {
                show_plain(terminal.backend_mut(), &text)?;
                self.native_mirror.plain(&text, self.state.cols, self.state.rows);
                self.plain = Some(Plain::Shown);
                return Ok(());
            }
            Some(Plain::Shown) => {
                self.plain = Some(Plain::Shown);
                return Ok(());
            }
            Some(Plain::Ending) => {
                // Cleared here and drawn whole: Terminal::clear would ask the terminal where
                // its cursor is, and not every terminal answers.
                execute!(terminal.backend_mut(), EnterAlternateScreen, EnableMouseCapture)?;
                clear_screen(terminal)?;
            }
            None => {}
        }
        self.state.settle();
        let clock = local_now();
        queue!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
        let mut output = self.draw_frame(terminal, &clock, now)?;
        if output.repaint {
            clear_screen(terminal)?;
            output = self.draw_frame(terminal, &clock, now)?;
        }
        let backend = terminal.backend_mut();
        backend.write_all(&output.bytes)?;
        if let Some(text) = self.clipboard.take() {
            // OSC 52 reaches the clipboard of the computer the user sits at, over SSH too.
            use base64::Engine as _;
            write!(backend, "\x1b]52;c;{}\x07", base64::engine::general_purpose::STANDARD.encode(text))?;
        }
        queue!(backend, EndSynchronizedUpdate)?;
        backend.flush()?;
        if let Some(error) = self.graphics.take_errors().first() {
            self.state.notice = Some(format!("{error}"));
        }
        Ok(())
    }

    pub(super) fn draw_frame(&mut self, terminal: &mut DefaultTerminal, clock: &Zoned, now: Instant) -> Result<Output> {
        let mut placements = Vec::new();
        let mut resized = Ok(());
        let mut mirror_cursor = None;
        let frame = terminal.draw(|frame| {
            let area = frame.area();
            if (self.state.cols, self.state.rows) != (area.width, area.height) {
                resized = self.resize(area.width, area.height).map(|_| ());
                if resized.is_err() {
                    return;
                }
            }
            (placements, mirror_cursor) = borg_ui::draw_with_cursor(frame, &self.scene(clock, now));
        })?;
        resized?;
        self.native_mirror.publish(frame.buffer, mirror_cursor);
        Ok(self.graphics.frame(&placements, frame.buffer))
    }

    pub(super) fn scene<'a>(&'a self, clock: &'a Zoned, now: Instant) -> Scene<'a> {
        Scene {
            state: &self.state,
            screen: match &self.console {
                borg_core::Console::Container(id) => self.shells.screen(id).unwrap_or_else(|| self.emulator.screen()),
                borg_core::Console::Host => self.emulator.screen(),
                borg_core::Console::Remote { machine, session } => self
                    .remote_consoles
                    .screen(&(machine.clone(), session.clone()))
                    .unwrap_or_else(|| self.emulator.screen()),
            },
            docker_shell: self.state.docker.visible_shell().and_then(|shell| self.shells.screen(&shell.container)),
            palette: &self.palette,
            themes: &self.themes,
            clock,
            now,
            home: self.home.as_deref(),
            pixels: pixels(self.graphics.capabilities()),
        }
    }

    /// The screen as text, drawn now as the next frame will show it, so an agent sees what
    /// its last call changed.
    pub(super) fn screen_text(&self) -> Vec<String> {
        let (clock, now) = (local_now(), Instant::now());
        let backend = ratatui::backend::TestBackend::new(self.state.cols, self.state.rows);
        let Ok(mut terminal) = ratatui::Terminal::new(backend);
        if terminal.draw(|frame| drop(draw(frame, &self.scene(&clock, now)))).is_err() {
            return Vec::new();
        }
        frame_text(terminal.backend().buffer())
    }

    pub(super) fn resize(&mut self, cols: u16, rows: u16) -> Result<Flow> {
        if (self.state.cols, self.state.rows) == (cols, rows) {
            return Ok(Flow::Continue);
        }
        let grid = GridSize::console(cols, rows);
        self.emulator.resize(grid);
        self.pty.resize(grid)?;
        self.last_click = None;
        self.dispatch(Msg::Resize { cols, rows })
    }
}

pub(super) fn clear_screen(terminal: &mut DefaultTerminal) -> Result<()> {
    execute!(terminal.backend_mut(), Clear(ClearType::All))?;
    terminal.swap_buffers();
    terminal.swap_buffers();
    Ok(())
}

/// Where an ask happens, with the computer that asked when it came through borg cloud.
pub(super) fn from_origin(place: String, origin: Option<&str>) -> String {
    match origin {
        None => place,
        Some(origin) if place.is_empty() => tr!(agent_asked_from).replace("{origin}", origin),
        Some(origin) => tr!(agent_from).replace("{place}", &place).replace("{origin}", origin),
    }
}

/// A drawn frame's text row by row, a wide character once, trailing spaces left out.
pub(super) fn frame_text(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
    let area = buffer.area;
    (area.top()..area.bottom())
        .map(|y| {
            let row: String =
                (area.left()..area.right()).filter_map(|x| buffer.cell((x, y))).map(|cell| cell.symbol()).collect();
            row.trim_end().to_owned()
        })
        .collect()
}

pub(super) fn palette(colors: &TerminalColors) -> Palette {
    let base = Palette::default();
    let rgb = |[r, g, b]: [u8; 3]| Rgb::new(r, g, b);
    Palette {
        ansi: std::array::from_fn(|index| colors.ansi[index].map_or(base.ansi[index], rgb)),
        foreground: colors.foreground.map_or(base.foreground, rgb),
        background: colors.background.map_or(base.background, rgb),
    }
}

pub(super) fn pixels(capabilities: Capabilities) -> Pixels {
    match capabilities.protocol {
        Protocol::Cells => Pixels::Off,
        Protocol::Kitty | Protocol::Sixel | Protocol::Iterm => Pixels::Pictures,
    }
}

pub(crate) fn span(cells: usize, cell: u16) -> u32 {
    u32::try_from(cells).unwrap_or(u32::MAX).saturating_mul(u32::from(cell))
}

pub(crate) fn picture(path: &Path, bounds: (u32, u32)) -> Result<Image> {
    let bytes = std::fs::metadata(path)?.len();
    let decoded = decode(path, bounds)?;
    Ok(Image { picture: decoded.picture, original: decoded.original, format: decoded.format, bytes })
}

pub(super) fn appearance(launch: &Launch) -> Appearance {
    launch.themes.appearance(launch.config.theme.as_deref(), launch.config.unlocked())
}

/// Where text shown plainly stands: waiting for the next frame, on the terminal, or about to
/// give way to borg's screen again.
pub(super) enum Plain {
    Waiting(String),
    Shown,
    Ending,
}

/// Leaves borg's screen and lets the mouse select again, then prints `text` as one line, which
/// the terminal wraps itself, so a selection copies it whole and without borg's frames.
pub(super) fn show_plain(out: &mut impl std::io::Write, text: &str) -> Result<()> {
    execute!(out, LeaveAlternateScreen, DisableMouseCapture)?;
    // Raw mode is on, so each line ends with a carriage return too.
    write!(out, "\r\n{}\r\n\r\n{text}\r\n\r\n{}\r\n", tr!(agent_plain_intro), tr!(agent_plain_return))?;
    out.flush()?;
    Ok(())
}
