//! The system summary above the process list: each core's CPU use, then the system's CPU,
//! memory, swap and disks beside its tasks, load and uptime, laid out to fit the pane.

use super::{Activity, LoadAverage, Threads};
use borg_i18n::{Count, count, tr};
use ratatui::layout::Rect;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

/// The narrowest pane that shows the summary; a narrower one keeps the one-line overview.
pub const SUMMARY_MIN_WIDTH: u16 = 30;
/// The narrowest pane that shows the figures beside the bars rather than under them.
pub const SUMMARY_TWO_COLUMNS: u16 = 56;
/// The summary takes at most one row of the pane in this many, leaving the rest to the list.
const SUMMARY_SHARE: u16 = 3;
/// The most rows the cores' bars take; more cores show as a strip of one cell each.
const CORE_BAR_ROWS: usize = 8;
/// The narrowest bar of a core.
pub const CORE_METER: u16 = 6;
/// Cells between the summary's columns.
pub const SUMMARY_GAP: u16 = 2;
/// Cells a core's percentage takes after its bar, with the space before it: " 100%".
pub const CORE_PERCENT: u16 = 5;
/// The bars under the cores: CPU, RAM and swap, and a disk bar where the system says how busy
/// its disks are.
const BAR_ROWS: u16 = 3;
/// The figures beside or under the bars: tasks, load, uptime and the disks' rates.
const FIGURE_ROWS: u16 = 4;
/// Cells the pane's border takes across and down.
const PANE_BORDER: u16 = 2;
const MINUTE_SECONDS: u64 = 60;
const HOUR_SECONDS: u64 = 60 * MINUTE_SECONDS;
const DAY_SECONDS: u64 = 24 * HOUR_SECONDS;
const HUNDREDTHS: u32 = 100;

/// How the summary shows each core's CPU use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreRows {
    /// A bar and a percentage for each core, numbered down each column.
    Bars { columns: u16, rows: u16 },
    /// One cell for each core, as tall as its use.
    Strip { rows: u16 },
    /// No room for the cores.
    Hidden,
}
impl CoreRows {
    pub fn rows(self) -> u16 {
        match self {
            Self::Bars { rows, .. } | Self::Strip { rows } => rows,
            Self::Hidden => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummaryLayout {
    pub cores: CoreRows,
    /// The figures stand beside the bars rather than under them.
    pub two_columns: bool,
    /// Rows the bars and figures take under the cores.
    pub figure_rows: u16,
}
impl SummaryLayout {
    pub fn rows(self) -> u16 {
        self.cores.rows() + self.figure_rows
    }
}

/// The labels in front of the bars and the strip of cores, which share one column.
pub fn bar_labels() -> [&'static str; 5] {
    [tr!(summary_cpu), tr!(summary_ram), tr!(summary_swap), tr!(summary_disk), tr!(summary_cores)]
}

/// The labels in front of the tasks, the load, the uptime and the disks' rates.
pub fn figure_labels() -> [&'static str; 4] {
    [tr!(summary_tasks), tr!(summary_load), tr!(summary_uptime), tr!(summary_disk)]
}

/// Cells the widest of `labels` takes.
pub fn label_width(labels: &[&str]) -> u16 {
    labels.iter().map(|label| label.width()).max().map_or(0, |width| u16::try_from(width).unwrap_or(u16::MAX))
}

/// Cells a core's number takes: as many as the highest number has digits.
pub fn core_number_width(cores: usize) -> u16 {
    u16::try_from(cores.to_string().len()).unwrap_or(u16::MAX)
}

/// The columns and rows of the cores' bars across `width` cells: as many columns as fit, then
/// as few as the same rows need, so the last column is the only one that ends early.
fn core_bars(cores: usize, width: u16) -> (usize, usize) {
    let cell = core_number_width(cores) + 1 + CORE_METER + CORE_PERCENT;
    let fit = usize::from((width + SUMMARY_GAP) / (cell + SUMMARY_GAP)).max(1);
    let rows = cores.div_ceil(fit);
    (cores.div_ceil(rows.max(1)), rows)
}

/// Cells in each row of the strip of cores, beside its label.
pub fn strip_cells(width: u16) -> u16 {
    width.saturating_sub(label_width(&bar_labels()) + 1).max(1)
}

/// The summary for `cores` cores in a pane `width` by `height` cells inside its border, or none
/// when it does not fit and the pane keeps the one-line overview. The cores' bars give way to
/// a strip, and the strip to nothing, before the summary takes more than its share of the pane.
/// `disk_bar` adds a bar for how busy the disks are.
pub fn summary_layout(cores: usize, disk_bar: bool, width: u16, height: u16) -> Option<SummaryLayout> {
    if width < SUMMARY_MIN_WIDTH {
        return None;
    }
    let two_columns = width >= SUMMARY_TWO_COLUMNS;
    let bars = BAR_ROWS + u16::from(disk_bar);
    let figure_rows = if two_columns { bars.max(FIGURE_ROWS) } else { bars + FIGURE_ROWS };
    let (columns, rows) = core_bars(cores, width);
    let core_bars = (cores > 0 && rows <= CORE_BAR_ROWS).then(|| CoreRows::Bars {
        columns: u16::try_from(columns).unwrap_or(u16::MAX),
        rows: u16::try_from(rows).unwrap_or(u16::MAX),
    });
    let strip = (cores > 0).then(|| CoreRows::Strip {
        rows: u16::try_from(cores.div_ceil(usize::from(strip_cells(width)))).unwrap_or(u16::MAX),
    });
    let room = height / SUMMARY_SHARE;
    [core_bars, strip, Some(CoreRows::Hidden)]
        .into_iter()
        .flatten()
        .map(|cores| SummaryLayout { cores, two_columns, figure_rows })
        .find(|layout| layout.rows() <= room)
}

/// A figure in hundredths, with two decimals.
fn hundredths(value: u32) -> String {
    format!("{}.{:02}", value / HUNDREDTHS, value % HUNDREDTHS)
}

/// The load averages over one, five and fifteen minutes.
pub fn load_text(load: LoadAverage) -> String {
    [load.one, load.five, load.fifteen].map(hundredths).join(" ")
}

/// How long the system has been up: days, then hours, minutes and seconds.
pub fn uptime_text(uptime: Duration) -> String {
    let seconds = uptime.as_secs();
    let clock = format!(
        "{:02}:{:02}:{:02}",
        seconds % DAY_SECONDS / HOUR_SECONDS,
        seconds % HOUR_SECONDS / MINUTE_SECONDS,
        seconds % MINUTE_SECONDS
    );
    match usize::try_from(seconds / DAY_SECONDS).unwrap_or(usize::MAX) {
        0 => clock,
        days => tr!(summary_uptime_days, days = count(days, Count::Day), time = clock),
    }
}

/// The number of processes, then the threads and how many of them run, where the system
/// counts them.
pub fn tasks_text(processes: usize, threads: Option<Threads>) -> String {
    let processes = processes.to_string();
    match threads {
        Some(threads) => tr!(
            summary_tasks_threads,
            processes = processes,
            threads = count(usize::try_from(threads.total).unwrap_or(usize::MAX), Count::Thread),
            running = threads.running
        ),
        None => processes,
    }
}

impl Activity {
    /// The summary as it fits a pane `width` by `height` cells inside its border, or none when
    /// it is hidden or does not fit.
    pub fn summary_layout(&self, width: u16, height: u16) -> Option<SummaryLayout> {
        if self.hide_summary {
            return None;
        }
        summary_layout(self.sample.cpu.len(), self.disk_busy().is_some(), width, height)
    }

    /// Rows the summary takes from the process list in `pane`, beyond the overview's one row.
    pub fn summary_rows(&self, pane: Rect) -> u16 {
        let (width, height) = (pane.width.saturating_sub(PANE_BORDER), pane.height.saturating_sub(PANE_BORDER));
        self.summary_layout(width, height).map_or(0, |layout| layout.rows().saturating_sub(1))
    }

    /// How busy the busiest disk was, in hundredths of a percent, where the system says.
    pub fn disk_busy(&self) -> Option<u32> {
        self.sample.disk.and_then(|disk| disk.busy)
    }
}
