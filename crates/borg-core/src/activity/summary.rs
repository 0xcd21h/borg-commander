//! The system summary above the process list: each core's CPU use, then the system's CPU, load,
//! memory, swap and disks beside its processes, uptime and disk rates, laid out to fit the pane.

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
/// The bars under the cores: CPU, load, RAM and swap, and a disk bar where the system says how
/// busy its disks are.
const BAR_ROWS: u16 = 4;
/// The figures beside or under the bars: processes, uptime, and what the disks read and write,
/// and a row for threads where the system counts them. Beside the bars, reading and writing
/// take a row each.
const FIGURE_ROWS: u16 = 3;
/// How far the last minute's load must stand from the last 15 minutes' to count as rising or
/// falling, in percent of what the cores can run.
const TREND_SHARE: u32 = 10;
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

/// What the system reports beyond what every system does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SummaryExtras {
    /// How busy the disks are, for a bar of their own.
    pub disk_busy: bool,
    /// How many threads there are and how many run, for a row of their own.
    pub threads: bool,
}

/// Whether the load over the last minute is above, below or close to the load over the last
/// 15 minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadTrend {
    Rising,
    Falling,
    Steady,
}

/// The load measured against what the cores can run at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadReading {
    /// The last minute's load as a share of the cores, in hundredths of a percent. Above 10,000
    /// the cores cannot keep up.
    pub share: u32,
    pub trend: LoadTrend,
    /// About how many processes waited for a core over the last minute, when the cores could not
    /// keep up.
    pub waiting: Option<u32>,
}

/// The labels in front of the bars and the strip of cores, which share one column.
pub fn bar_labels() -> [&'static str; 6] {
    [tr!(summary_cpu), tr!(summary_load), tr!(summary_ram), tr!(summary_swap), tr!(summary_disk), tr!(summary_cores)]
}

/// The labels in front of the processes, the threads, the uptime and the disks' rates.
pub fn figure_labels() -> [&'static str; 4] {
    [tr!(summary_processes), tr!(summary_threads), tr!(summary_uptime), tr!(summary_disk)]
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
pub fn summary_layout(cores: usize, extras: SummaryExtras, width: u16, height: u16) -> Option<SummaryLayout> {
    if width < SUMMARY_MIN_WIDTH {
        return None;
    }
    let two_columns = width >= SUMMARY_TWO_COLUMNS;
    let bars = BAR_ROWS + u16::from(extras.disk_busy);
    let figures = FIGURE_ROWS + u16::from(extras.threads) + u16::from(two_columns);
    let figure_rows = if two_columns { bars.max(figures) } else { bars + figures };
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

/// The load over the last minute against what `cores` cores run at once, or none before the
/// first sample counts the cores.
pub fn load_reading(load: LoadAverage, cores: usize) -> Option<LoadReading> {
    let cores = u32::try_from(cores).ok().filter(|&cores| cores > 0)?;
    let capacity = cores * HUNDREDTHS;
    let margin = cores * TREND_SHARE;
    let trend = if load.one > load.fifteen.saturating_add(margin) {
        LoadTrend::Rising
    } else if load.one.saturating_add(margin) < load.fifteen {
        LoadTrend::Falling
    } else {
        LoadTrend::Steady
    };
    let waiting = (load.one > capacity).then(|| (load.one - capacity).div_ceil(HUNDREDTHS));
    Some(LoadReading { share: load.one.saturating_mul(HUNDREDTHS) / cores, trend, waiting })
}

/// The load's share of the cores and its trend: "88%, rising".
pub fn load_text(reading: LoadReading) -> String {
    let share = format!("{}%", (reading.share + HUNDREDTHS / 2) / HUNDREDTHS);
    let trend = match reading.trend {
        LoadTrend::Rising => tr!(summary_load_rising),
        LoadTrend::Falling => tr!(summary_load_falling),
        LoadTrend::Steady => tr!(summary_load_steady),
    };
    tr!(summary_load_value, share = share, trend = trend)
}

/// About how many processes wait for a core: "~4 waiting".
pub fn waiting_text(waiting: u32) -> String {
    tr!(summary_load_waiting, waiting = waiting)
}

/// How long the system has been up, in its two largest units: "3 days, 4 hours".
pub fn uptime_text(uptime: Duration) -> String {
    let seconds = uptime.as_secs();
    let days = seconds / DAY_SECONDS;
    let hours = seconds % DAY_SECONDS / HOUR_SECONDS;
    let minutes = seconds % HOUR_SECONDS / MINUTE_SECONDS;
    let units =
        [(days, Count::Day), (hours, Count::Hour), (minutes, Count::Minute), (seconds % MINUTE_SECONDS, Count::Second)];
    let first = units.iter().position(|&(amount, _)| amount > 0).unwrap_or(units.len() - 1);
    let amount = |(amount, noun): (u64, Count)| count(usize::try_from(amount).unwrap_or(usize::MAX), noun);
    match units.get(first + 1).filter(|&&(amount, noun)| amount > 0 && noun != Count::Second) {
        Some(&second) => tr!(summary_uptime_pair, larger = amount(units[first]), smaller = amount(second)),
        None => amount(units[first]),
    }
}

/// The threads and how many of them run: "517 · 3 running".
pub fn threads_text(threads: Threads) -> String {
    tr!(summary_threads_running, threads = threads.total, running = threads.running)
}

impl Activity {
    /// The summary as it fits a pane `width` by `height` cells inside its border, or none when
    /// it is hidden or does not fit.
    pub fn summary_layout(&self, width: u16, height: u16) -> Option<SummaryLayout> {
        if self.hide_summary {
            return None;
        }
        let extras = SummaryExtras { disk_busy: self.disk_busy().is_some(), threads: self.sample.threads.is_some() };
        summary_layout(self.sample.cpu.len(), extras, width, height)
    }

    /// Rows the summary takes from the process list in `pane`, beyond the overview's one row.
    pub fn summary_rows(&self, pane: Rect) -> u16 {
        let (width, height) = (pane.width.saturating_sub(PANE_BORDER), pane.height.saturating_sub(PANE_BORDER));
        self.summary_layout(width, height).map_or(0, |layout| layout.rows().saturating_sub(1))
    }

    /// The load against the cores, once a sample has both.
    pub fn load(&self) -> Option<LoadReading> {
        self.sample.load.and_then(|load| load_reading(load, self.sample.cpu.len()))
    }

    /// How busy the busiest disk was, in hundredths of a percent, where the system says.
    pub fn disk_busy(&self) -> Option<u32> {
        self.sample.disk.and_then(|disk| disk.busy)
    }
}
