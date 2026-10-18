use std::{fmt::Write as _, path::PathBuf, time::Instant};

use borg_core::{AppState, EVEN_SPLIT, Effect, Msg, PanelSetup, Settings, ShellView, Side, Startup, update};
use borg_term::{Emulator, Foreground, GridSize};
use borg_ui::{Palette, Scene, ThemeSet, draw};
use borg_vfs::Entry;
use jiff::Zoned;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, style::Style};
use unicode_width::UnicodeWidthStr;

const SCROLLBACK: usize = 0;
const STYLE_HEADER: &str = "--- styles ---";

pub struct Stage {
    pub state: AppState,
    pub emulator: Emulator,
    pub themes: ThemeSet,
    pub palette: Palette,
    pub clock: Zoned,
    pub started: Instant,
    pub home: Option<PathBuf>,
}

impl Stage {
    pub fn new(
        dirs: [PathBuf; 2],
        listings: [Vec<Entry>; 2],
        size: GridSize,
        settings: Settings,
        clock: Zoned,
    ) -> anyhow::Result<Self> {
        let themes = ThemeSet::builtin()?;
        let started = Instant::now();
        let (mut state, effects) =
            AppState::new(startup(dirs), size.cols, size.rows, settings, themes.appearance(None, &[]), started);

        let mut listings = listings.into_iter();
        for effect in effects {
            let Effect::List { side, generation, .. } = effect else { continue };
            let entries = listings.next().unwrap_or_default();
            update(&mut state, Msg::Batch { side, generation, entries }, started);
            update(&mut state, Msg::Listed { side, generation, error: None }, started);
        }
        state.settle();

        let mut stage = Self {
            state,
            emulator: Emulator::new(size, SCROLLBACK),
            themes,
            palette: Palette::default(),
            clock,
            started,
            home: None,
        };
        stage.shell(b"\x1b]133;A\x07$ \x1b]133;B\x07");
        Ok(stage)
    }

    pub fn shell(&mut self, bytes: &[u8]) -> &mut Self {
        for signal in self.emulator.feed(bytes) {
            update(&mut self.state, Msg::Signal(signal), self.started);
        }
        let view = ShellView {
            prompt: self.emulator.prompt_state(),
            foreground: Foreground::Shell,
            mode: self.emulator.mode(),
        };
        update(&mut self.state, Msg::ShellOutput(view), self.started);
        self
    }

    pub fn send(&mut self, msg: Msg) -> &mut Self {
        update(&mut self.state, msg, self.started);
        self
    }

    pub fn render(&mut self, at: Instant) -> anyhow::Result<Buffer> {
        self.state.settle();
        let screen = self.emulator.screen();
        let size = (screen.cols(), screen.rows());
        let mut terminal = Terminal::new(TestBackend::new(size.0, size.1))?;
        let scene = Scene {
            state: &self.state,
            screen,
            palette: &self.palette,
            themes: &self.themes,
            clock: &self.clock,
            now: at,
            home: self.home.as_deref(),
        };
        terminal.draw(|frame| draw(frame, &scene))?;
        Ok(terminal.backend().buffer().clone())
    }
}

pub fn describe(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut out = String::new();
    for y in area.top()..area.bottom() {
        let mut covered = 0;
        let line: String = (area.left()..area.right())
            .filter_map(|x| {
                let symbol = buffer[(x, y)].symbol();
                if covered > 0 {
                    covered -= 1;
                    return None;
                }
                covered = symbol.width().saturating_sub(1);
                Some(symbol)
            })
            .collect();
        let _ = writeln!(out, "{}", line.trim_end());
    }

    let _ = writeln!(out, "{STYLE_HEADER}");
    for y in area.top()..area.bottom() {
        let runs = style_runs(buffer, y);
        let _ = writeln!(out, "{y:>3}: {}", runs.join(" | "));
    }
    out
}

fn style_runs(buffer: &Buffer, y: u16) -> Vec<String> {
    let area = buffer.area;
    let style_at = |x: u16| {
        let cell = &buffer[(x, y)];
        Style::new().fg(cell.fg).bg(cell.bg).add_modifier(cell.modifier)
    };
    let mut runs = Vec::new();
    let mut start = area.left();
    for x in area.left()..=area.right() {
        let boundary = x == area.right() || style_at(x) != style_at(start);
        if boundary {
            let style = style_at(start);
            runs.push(format!(
                "{start}-{} {:?} {:?} {:?}",
                x - 1,
                style.fg.unwrap_or_default(),
                style.bg.unwrap_or_default(),
                style.add_modifier
            ));
            start = x;
        }
    }
    runs
}

fn startup(dirs: [PathBuf; 2]) -> Startup {
    Startup { panels: dirs.map(PanelSetup::at), active: Side::Left, split: EVEN_SPLIT }
}
