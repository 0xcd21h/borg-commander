//! The system summary above the process list: each core's CPU use, then the system's CPU,
//! memory, swap and disks beside its tasks, load and uptime.

use super::{dim, memory::meter, percent, text};
use crate::{format, look::Look};
use borg_core::{
    Activity, CORE_PERCENT, CoreRows, SUMMARY_GAP, SummaryLayout, bar_labels, core_number_width, figure_labels,
    label_width, load_text, strip_cells, tasks_text, uptime_text,
};
use borg_i18n::tr;
use ratatui::{buffer::Buffer, layout::Rect};
use unicode_width::UnicodeWidthStr;

/// The narrowest bar beside a label and its value; narrower, the value stands alone.
const MIN_METER: u16 = 4;
/// A whole core or the whole system, in hundredths of a percent.
const WHOLE: f64 = 10_000.0;
const HUNDREDTHS: u32 = 100;
/// The strip's cells from idle to fully busy.
const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const UNKNOWN: &str = "—";

/// A row of the summary with a bar: its label, how full the bar is, and the value after it.
/// Without a fraction the value stands where the bar would.
struct Gauge {
    label: &'static str,
    fraction: Option<f64>,
    value: String,
}

/// A row of the summary with a label and a value.
struct Figure {
    label: &'static str,
    value: String,
}

fn share(part: u64, whole: u64) -> f64 {
    if whole == 0 { 0.0 } else { part as f64 / whole as f64 }
}

fn of_whole(hundredths: u32) -> f64 {
    f64::from(hundredths) / WHOLE
}

pub(super) fn draw(activity: &Activity, look: &Look<'_>, area: Rect, layout: SummaryLayout, buf: &mut Buffer) {
    let cores = activity.core_cpu();
    match layout.cores {
        CoreRows::Bars { columns, rows } => core_bars(&cores, (columns, rows), look, area, buf),
        CoreRows::Strip { .. } => core_strip(&cores, look, area, buf),
        CoreRows::Hidden => {}
    }
    let rest = Rect { y: area.y + layout.cores.rows(), height: layout.figure_rows, ..area };
    let (gauges, figures) = (gauges(activity), figures(activity, layout.two_columns));
    if layout.two_columns {
        let (bars, labels) = (label_width(&bar_labels()), label_width(&figure_labels()));
        let room = rest.width.saturating_sub(SUMMARY_GAP);
        let longest = figures.iter().map(|figure| figure.value.width()).max().unwrap_or(0);
        let needed = labels.saturating_add(1).saturating_add(u16::try_from(longest).unwrap_or(u16::MAX));
        let least = bars + 1 + MIN_METER + 1 + value_width(&gauges);
        let right = needed.max(room / 2).min(room.saturating_sub(least));
        let left = room - right;
        draw_gauges(&gauges, bars, look, Rect { width: left, ..rest }, buf);
        draw_figures(&figures, labels, look, Rect { x: rest.x + left + SUMMARY_GAP, width: right, ..rest }, buf);
    } else {
        let labels = label_width(&bar_labels()).max(label_width(&figure_labels()));
        let under = gauges.len() as u16;
        draw_gauges(&gauges, labels, look, rest, buf);
        draw_figures(
            &figures,
            labels,
            look,
            Rect { y: rest.y + under, height: rest.height.saturating_sub(under), ..rest },
            buf,
        );
    }
}

/// A bar and a percentage for each core, numbered down each column.
fn core_bars(cores: &[u32], (columns, rows): (u16, u16), look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let number = core_number_width(cores.len());
    let width = area.width.saturating_sub(columns.saturating_sub(1) * SUMMARY_GAP) / columns.max(1);
    let meter_width = width.saturating_sub(number + 1 + CORE_PERCENT);
    for (index, &cpu) in cores.iter().enumerate() {
        let (column, row) = (index / usize::from(rows.max(1)), index % usize::from(rows.max(1)));
        let (Ok(column), Ok(row)) = (u16::try_from(column), u16::try_from(row)) else { continue };
        let line = Rect { x: area.x + column * (width + SUMMARY_GAP), y: area.y + row, width, height: 1 };
        text(
            buf,
            Rect { width: number, ..line },
            format::pad_start(&(index + 1).to_string(), usize::from(number)),
            dim(theme),
        );
        meter(buf, Rect { x: line.x + number + 1, width: meter_width, ..line }, of_whole(cpu), look);
        let percent = format!("{}%", (cpu + HUNDREDTHS / 2) / HUNDREDTHS);
        let value = Rect { x: line.x + number + 1 + meter_width, width: CORE_PERCENT, ..line };
        text(buf, value, format::pad_start(&percent, usize::from(CORE_PERCENT)), theme.styles.overlay);
    }
}

/// One cell for each core beside the strip's label, as tall as the core's use.
fn core_strip(cores: &[u32], look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let accent = look.accent();
    let label = label_width(&bar_labels());
    text(buf, Rect { width: label, height: 1, ..area }, tr!(summary_cores), dim(theme));
    let cells = usize::from(strip_cells(area.width));
    let last = LEVELS.len() - 1;
    for (index, &cpu) in cores.iter().enumerate() {
        let (Ok(row), Ok(column)) = (u16::try_from(index / cells), u16::try_from(index % cells)) else { continue };
        let (x, y) = (area.x + label + 1 + column, area.y + row);
        if x >= area.right() || y >= area.bottom() {
            continue;
        }
        let fraction = of_whole(cpu).clamp(0.0, 1.0);
        let level = (fraction * last as f64).round() as usize;
        let cell = &mut buf[(x, y)];
        if cpu == 0 {
            cell.set_char(LEVELS[0]).set_style(dim(theme));
        } else {
            let color = accent.at(fraction as f32);
            cell.set_char(LEVELS[level.max(1)]).set_style(theme.styles.overlay.fg(color.into()));
        }
    }
}

fn gauges(activity: &Activity) -> Vec<Gauge> {
    let sample = &activity.sample;
    let [cpu, ram, swap, disk, _] = bar_labels();
    let pair = |used: u64, total: u64| format!("{}/{}", format::size(used), format::size(total));
    let mut gauges = vec![
        Gauge {
            label: cpu,
            fraction: Some(activity.system_cpu().map_or(0.0, of_whole)),
            value: percent(activity.system_cpu()),
        },
        Gauge { label: ram, fraction: Some(share(sample.used, sample.total)), value: pair(sample.used, sample.total) },
        Gauge {
            label: swap,
            fraction: Some(share(sample.swap_used, sample.swap_total)),
            value: pair(sample.swap_used, sample.swap_total),
        },
    ];
    if let Some(busy) = activity.disk_busy() {
        gauges.push(Gauge { label: disk, fraction: Some(of_whole(busy)), value: whole_percent(busy) });
    }
    gauges
}

/// `value` shortened with an ellipsis to fit `area`.
fn fitted(value: &str, area: Rect) -> String {
    format::truncate_end(value, usize::from(area.width))
}

fn whole_percent(hundredths: u32) -> String {
    format!("{}%", (hundredths + HUNDREDTHS / 2) / HUNDREDTHS)
}

/// What the disks read and write per second.
fn rates(activity: &Activity) -> String {
    activity.sample.disk.map_or_else(
        || UNKNOWN.to_owned(),
        |disk| tr!(summary_disk_rates, read = format::size(disk.read), write = format::size(disk.written)),
    )
}

/// The tasks, load, uptime and the disks' rates, which stand beside the disk bar without a label
/// of their own when there is one `beside` them.
fn figures(activity: &Activity, beside: bool) -> Vec<Figure> {
    let sample = &activity.sample;
    let [tasks, load, uptime, disk] = figure_labels();
    vec![
        Figure { label: tasks, value: tasks_text(sample.processes.len(), sample.threads) },
        Figure { label: load, value: sample.load.map_or_else(|| UNKNOWN.to_owned(), load_text) },
        Figure { label: uptime, value: sample.uptime.map_or_else(|| UNKNOWN.to_owned(), uptime_text) },
        Figure { label: if beside && activity.disk_busy().is_some() { "" } else { disk }, value: rates(activity) },
    ]
}

/// Cells the widest value after a bar takes.
fn value_width(gauges: &[Gauge]) -> u16 {
    let widest = gauges.iter().filter(|gauge| gauge.fraction.is_some()).map(|gauge| gauge.value.width()).max();
    u16::try_from(widest.unwrap_or(0)).unwrap_or(u16::MAX)
}

fn draw_gauges(gauges: &[Gauge], labels: u16, look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let values = value_width(gauges);
    for (offset, gauge) in (0..area.height).zip(gauges) {
        let line = Rect { y: area.y + offset, height: 1, ..area };
        text(buf, Rect { width: labels, ..line }, gauge.label, dim(theme));
        let rest = Rect { x: line.x + labels + 1, width: line.width.saturating_sub(labels + 1), ..line };
        let meter_width = rest.width.saturating_sub(values + 1);
        match gauge.fraction.filter(|_| meter_width >= MIN_METER) {
            Some(fraction) => {
                meter(buf, Rect { width: meter_width, ..rest }, fraction, look);
                let value = Rect { x: rest.x + meter_width + 1, width: values, ..rest };
                text(buf, value, format::pad_start(&gauge.value, usize::from(values)), theme.styles.overlay);
            }
            None => text(buf, rest, fitted(&gauge.value, rest), theme.styles.overlay),
        }
    }
}

fn draw_figures(figures: &[Figure], labels: u16, look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    for (offset, figure) in (0..area.height).zip(figures) {
        let line = Rect { y: area.y + offset, height: 1, ..area };
        text(buf, Rect { width: labels, ..line }, figure.label, dim(theme));
        let value = Rect { x: line.x + labels + 1, width: line.width.saturating_sub(labels + 1), ..line };
        text(buf, value, fitted(&figure.value, value), theme.styles.overlay);
    }
}
