//! The memory overview: the meters, the history chart, the largest processes, and the
//! inspected process's place in the process tree.

use super::{Page, SIZE_WIDTH, dim, emphasis, heading, percent, row, size, text};
use crate::{Rgb, format, look::Look};
use borg_core::{Activity, DEFAULT_NICE, HISTORY, Kin, MemoryProtection, MemoryRegion, MemoryTotals, Relative};
use borg_i18n::{Count, count, tr};
use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};
use unicode_width::UnicodeWidthStr;

const TILE_WIDTH: u16 = 12;
const METER_WIDTH: u16 = 20;
const CHART_ROWS: u16 = 20;
/// Rows the chart's title and time axis take around its plot.
const CHART_FRAME: u16 = 2;
/// Rows above the process tree's processes: a blank one after the chart, then the title.
const FAMILY_HEADER: u16 = 2;
/// Cells each level of the process tree indents by.
const FAMILY_INDENT: usize = 2;
const BLOCKS: [char; 7] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇'];
const LEFT_BLOCKS: [char; 7] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉'];
const BYTE_UNITS: [&str; 6] = ["K", "M", "G", "T", "P", "E"];

/// How many of the Footprint, Resident and Mapped columns fit beside a name.
pub(super) fn size_columns(width: u16) -> usize {
    match width {
        56.. => 3,
        40.. => 2,
        26.. => 1,
        _ => 0,
    }
}

pub(super) fn sized_line(name: &str, values: &[String], width: u16) -> String {
    let shown = size_columns(width).min(values.len());
    let name_width = usize::from(width).saturating_sub(shown * (SIZE_WIDTH + 1)).max(1);
    let name = format::truncate_end(name, name_width);
    let pad = name_width.saturating_sub(name.width());
    values[..shown]
        .iter()
        .fold(format!("{name}{}", " ".repeat(pad)), |line, value| format!("{line} {value:>SIZE_WIDTH$}"))
}

pub(super) fn sizes(totals: &MemoryTotals) -> [String; 3] {
    [size(totals.footprint), size(totals.resident), format::size(totals.mapped)]
}

/// A horizontal bar filled to `fraction`, shaded along the theme accent.
/// How a meter colors its filled cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fill {
    /// Along the theme's accent, from the empty end to the full one.
    Accent,
    /// In the notice color, for something that runs short.
    Warning,
}

pub(super) fn meter(buf: &mut Buffer, area: Rect, fraction: f64, tone: Fill, look: &Look<'_>) {
    let theme = look.theme;
    let accent = look.accent();
    let eighths = (fraction.clamp(0.0, 1.0) * f64::from(area.width) * 8.0).round() as u32;
    for x in 0..area.width {
        let fill = eighths.saturating_sub(u32::from(x) * 8).min(8);
        let cell = &mut buf[(area.x + x, area.y)];
        match fill {
            0 => {
                cell.set_char('─').set_style(dim(theme));
            }
            _ => {
                let symbol = if fill == 8 { '█' } else { LEFT_BLOCKS[fill as usize - 1] };
                let color = match tone {
                    Fill::Accent => accent.at(f32::from(x) / f32::from(area.width.max(2) - 1)),
                    Fill::Warning => look.fg(theme.styles.notice),
                };
                cell.set_char(symbol).set_style(theme.styles.overlay.fg(color.into()));
            }
        }
    }
}

/// Footprint, Resident and Mapped rows, with the footprint's share of the process.
pub(super) fn metrics(page: &mut Page<'_>, totals: &MemoryTotals, whole: Option<u64>, look: &Look<'_>) {
    let theme = look.theme;
    let labels = [tr!(memory_footprint_column), tr!(memory_resident_column), tr!(memory_mapped_column)];
    let label_width = labels.iter().map(|l| l.width()).max().unwrap_or(0);
    for (i, (label, value)) in labels.into_iter().zip(sizes(totals)).enumerate() {
        let line = page.line(format!("{}  {value:>8}", format::pad_end(label, label_width)), theme.styles.overlay);
        text(page.buf, Rect { width: label_width as u16, ..line }, label, dim(theme));
        let (Some(part), Some(whole)) = (totals.footprint, whole.filter(|&w| w > 0)) else { continue };
        if i != 0 {
            continue;
        }
        let fraction = part as f64 / whole as f64;
        let x = line.x.saturating_add(label_width as u16 + 12);
        let percent = match (fraction * 100.0).round() as u64 {
            0 if part > 0 => "<1".to_owned(),
            percent => percent.to_string(),
        };
        let note = tr!(memory_footprint_share, percent = percent);
        let width = METER_WIDTH.min(line.right().saturating_sub(x).saturating_sub(note.width() as u16 + 1));
        if width >= 4 {
            meter(page.buf, Rect { x, width, ..line }, fraction, Fill::Accent, look);
            let note_area = Rect { x: x + width + 1, width: line.right().saturating_sub(x + width + 1), ..line };
            text(page.buf, note_area, note, dim(theme));
        }
    }
}

pub(super) fn change(value: i128) -> String {
    let amount = format::size(value.unsigned_abs().min(u128::from(u64::MAX)) as u64);
    match value.signum() {
        1 => format!("▲ {amount}"),
        -1 => format!("▼ {amount}"),
        _ => amount,
    }
}

pub(super) fn memory(activity: &Activity, look: &Look<'_>, body: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let Some(p) = activity.process() else {
        text(buf, row(body, 0), tr!(activity_process_gone), theme.styles.notice);
        return;
    };
    let history = activity.histories.get(&p.key);
    let summed = |value: fn(&MemoryRegion) -> Option<u64>| {
        activity.regions().iter().filter_map(value).reduce(u64::saturating_add)
    };
    let mut tiles = vec![
        (tr!(activity_resident), size(p.resident)),
        (tr!(activity_peak), size(history.map(|h| h.peak))),
        (tr!(activity_since), change(activity.growth(p.key))),
        (tr!(activity_cpu_tile), percent(activity.cpu(p.key))),
        (tr!(activity_virtual), size(p.virtual_bytes)),
    ];
    if let Some(nice) = p.nice.filter(|&nice| nice != DEFAULT_NICE) {
        tiles.push((tr!(activity_nice_tile), nice.to_string()));
    }
    let footprint = activity.region_totals().footprint;
    for (label, value) in [
        (tr!(activity_footprint), footprint),
        (tr!(activity_swapped), summed(|r| r.swap)),
        (tr!(activity_private), summed(|r| r.private)),
        (tr!(activity_shared), summed(|r| r.shared)),
    ] {
        if let Some(value) = value {
            tiles.push((label, format::size(value)));
        }
    }
    let per_row = (body.width / TILE_WIDTH).clamp(1, tiles.len() as u16);
    let tile_width = body.width / per_row;
    let mut y = 0;
    for line in tiles.chunks(usize::from(per_row)) {
        for (i, (label, value)) in line.iter().enumerate() {
            let x = body.x + i as u16 * tile_width;
            let area = |dy: u16| Rect { x, width: tile_width.saturating_sub(1), ..row(body, y + dy) };
            text(buf, area(0), *label, dim(theme));
            let accent =
                if i == 0 && y == 0 { heading(theme) } else { theme.styles.overlay.add_modifier(Modifier::BOLD) };
            text(buf, area(1), value.clone(), accent);
        }
        y += 3;
    }
    if p.stopped {
        text(buf, row(body, y), tr!(process_stopped_note), emphasis(theme));
        y += 2;
    }
    if let Some(protection) = p.protection {
        let note = match protection {
            MemoryProtection::System => tr!(process_memory_protected_system),
            MemoryProtection::HardenedRuntime => tr!(process_memory_protected_runtime),
        };
        text(buf, row(body, y), note, emphasis(theme));
        y += 2;
    }
    let rest = Rect { y: body.y.saturating_add(y).min(body.bottom()), height: body.height.saturating_sub(y), ..body };
    let family = activity.family();
    let family_rows = family_rows(&family, rest.height);
    let chart_area = Rect { height: rest.height - family_rows, ..rest };
    chart(tr!(memory_chart_title), history.map_or(&[][..], |h| &h.values[..]), Unit::Bytes, look, chart_area, buf);
    let family_area = Rect { y: chart_area.bottom(), height: family_rows, ..rest };
    family_tree(activity, &family, look, family_area, buf);
}

/// Rows the process tree takes below the chart: all it needs where the chart leaves room, and
/// otherwise up to half of what is left. A process alone in its tree gets none.
fn family_rows(family: &[Relative], height: u16) -> u16 {
    if family.len() < 2 {
        return 0;
    }
    let needed = u16::try_from(family.len()).unwrap_or(u16::MAX).saturating_add(FAMILY_HEADER);
    let room = height.saturating_sub(CHART_ROWS + CHART_FRAME).max(height / 2);
    if room <= FAMILY_HEADER { 0 } else { needed.min(room) }
}

/// The processes the inspected one descends from, the process itself, and everything it
/// started, each indented under the process that started it. When they do not all fit, the
/// process and the ones closest above it stay, and a last line counts the ones left out.
fn family_tree(activity: &Activity, family: &[Relative], look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    if area.height <= FAMILY_HEADER {
        return;
    }
    text(buf, row(area, 1), tr!(activity_family_title), theme.styles.overlay.add_modifier(Modifier::BOLD));
    let lines = usize::from(area.height - FAMILY_HEADER);
    let selected = family.iter().position(|relative| relative.kin == Kin::Selected).unwrap_or_default();
    let (start, shown) = if family.len() <= lines {
        (0, lines)
    } else {
        let shown = lines - 1;
        ((selected + 1).saturating_sub(shown), shown)
    };
    let indent_limit = usize::from(area.width / 2);
    for (offset, relative) in family.iter().skip(start).take(shown).enumerate() {
        let process = &activity.sample.processes[relative.index];
        let line = row(area, FAMILY_HEADER + offset as u16);
        let indent = " ".repeat(relative.depth.saturating_mul(FAMILY_INDENT).min(indent_limit));
        let style = if relative.kin == Kin::Selected { heading(theme) } else { theme.styles.overlay };
        let resident = size(process.resident);
        let size_x = line.right().saturating_sub(resident.width() as u16);
        let name_width = size_x.saturating_sub(line.x).saturating_sub(1);
        text(buf, Rect { width: name_width, ..line }, format!("{indent}{} {}", process.key.pid, process.name), style);
        text(buf, Rect { x: size_x, width: line.right() - size_x, ..line }, resident, dim(theme));
    }
    let left_out = family.len().saturating_sub(start + shown);
    if left_out > 0 {
        let line = row(area, FAMILY_HEADER + shown as u16);
        text(buf, line, tr!(activity_family_more, processes = count(left_out, Count::Process)), dim(theme));
    }
}

/// What a chart's values measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unit {
    Bytes,
    /// Bytes a second.
    Rate,
    /// Hundredths of a percent.
    Percent,
}

impl Unit {
    pub(super) fn format(self, value: u64) -> String {
        match self {
            Self::Bytes => format::size(value),
            Self::Rate => format::rate(value),
            Self::Percent => format!("{:.1}%", value as f64 / 100.0),
        }
    }
}

/// Picks a round axis step and range covering `values`, in the largest unit below them.
pub(super) fn scale(values: &[u64], unit: Unit) -> (f64, f64, [String; 3]) {
    let min = values.iter().copied().min().unwrap_or(0) as f64;
    let max = values.iter().copied().max().unwrap_or(0) as f64;
    let (base, suffix, least) = match unit {
        Unit::Bytes | Unit::Rate => {
            let mut base = 1024.0;
            let mut suffix = 0;
            while max / base >= 1024.0 && suffix + 1 < BYTE_UNITS.len() {
                base *= 1024.0;
                suffix += 1;
            }
            (base, BYTE_UNITS[suffix], 0.01)
        }
        // An idle container's CPU stays on a scale of at least one percent.
        Unit::Percent => (100.0, "%", 1.0),
    };
    let (low, high) = (min / base, max / base);
    let span = (high - low).max(high * 0.04).max(least);
    let rough = span / 2.0;
    let magnitude = 10f64.powf(rough.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0].into_iter().map(|n| n * magnitude).find(|&s| s >= rough).unwrap_or(rough);
    let lo = ((low / step).floor() * step).max(0.0);
    let mut hi = lo + step * ((high - lo) / step).ceil().max(1.0);
    if hi - high < step * 0.1 {
        hi += step;
    }
    let mid = (lo + hi) / 2.0;
    let decimals = (0..3)
        .find(|&d| {
            let shifted = (mid - lo) * 10f64.powi(d);
            (shifted - shifted.round()).abs() < 1e-6
        })
        .unwrap_or(3) as usize;
    let label = |v: f64| format!("{v:.decimals$}{suffix}");
    (lo * base, hi * base, [label(hi), label(mid), label(lo)])
}

/// The value at plot column `x` on a fixed window of `HISTORY` samples ending now.
pub(super) fn sample_at(values: &[u64], x: u16, width: u16) -> Option<f64> {
    let values = &values[values.len().saturating_sub(HISTORY)..];
    let slots = (HISTORY - 1) as f64;
    let slot = if width <= 1 { slots } else { f64::from(x) * slots / f64::from(width - 1) };
    let position = slot - (HISTORY - values.len()) as f64;
    if position < -1e-9 {
        return None;
    }
    let index = (position.max(0.0).floor() as usize).min(values.len() - 1);
    let from = values[index] as f64;
    let to = values.get(index + 1).map_or(from, |&v| v as f64);
    Some(from + (to - from) * (position - index as f64))
}

/// An area chart of the last minute: round axis labels, faint grid lines, and columns
/// shaded from the background up to a bright accent edge.
pub(crate) fn chart(title: &str, values: &[u64], unit: Unit, look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    if area.height < 2 {
        return;
    }
    text(buf, row(area, 0), title, theme.styles.overlay.add_modifier(Modifier::BOLD));
    if let Some(&latest) = values.last() {
        let latest = unit.format(latest);
        let x = area.right().saturating_sub(latest.width() as u16);
        text(buf, Rect { x, width: area.right() - x, ..row(area, 0) }, latest, heading(theme));
    }
    if values.len() < 2 {
        let note = Rect { x: area.x + title.width() as u16 + 2, ..row(area, 0) };
        text(
            buf,
            Rect { width: area.right().saturating_sub(note.x), ..note },
            tr!(memory_chart_collecting),
            dim(theme),
        );
    }
    let plot_height = area.height.saturating_sub(2).min(CHART_ROWS);
    if plot_height < 2 || values.is_empty() {
        return;
    }
    let (lo, hi, labels) = scale(values, unit);
    let axis = labels.iter().map(|l| l.width()).max().unwrap_or(0) as u16 + 1;
    let plot = Rect {
        x: area.x.saturating_add(axis),
        y: area.y + 1,
        width: area.width.saturating_sub(axis),
        height: plot_height,
    };
    if plot.width < 4 {
        return;
    }
    let background = look.bg(theme.styles.overlay);
    let grid: Rgb = background.mix(look.fg(theme.styles.border), 0.55);
    for (label, y) in labels.iter().zip([0, plot.height / 2, plot.height - 1]) {
        let line = row(plot, y);
        text(
            buf,
            Rect { x: area.x, width: axis - 1, ..line },
            format!("{label:>width$}", width = usize::from(axis - 1)),
            dim(theme),
        );
        for x in line.left()..line.right() {
            buf[(x, line.y)].set_char('┈').set_style(theme.styles.overlay.fg(grid.into()));
        }
    }
    let accent = look.accent();
    let levels = f64::from(plot.height) * 8.0;
    for x in 0..plot.width {
        let Some(value) = sample_at(values, x, plot.width) else { continue };
        let fill = (((value - lo) / (hi - lo)) * levels).round().clamp(1.0, levels) as u32;
        let top = (fill - 1) / 8;
        for level in 0..=top {
            let eighths = (fill - level * 8).min(8);
            let symbol = if eighths == 8 { '█' } else { BLOCKS[eighths as usize - 1] };
            let height = (level + 1) as f32 / f32::from(plot.height);
            let shade = accent.to.mix(accent.from, height);
            let color = if level == top { shade } else { background.mix(shade, 0.3 + 0.55 * height) };
            let y = plot.bottom() - 1 - level as u16;
            buf[(plot.x + x, y)].set_char(symbol).set_style(theme.styles.overlay.fg(color.into()));
        }
    }
    let axis_row = Rect { y: plot.bottom(), height: u16::from(plot.bottom() < area.bottom()), ..plot };
    text(buf, axis_row, tr!(memory_chart_span, seconds = HISTORY), dim(theme));
    let now = tr!(memory_chart_now);
    let x = axis_row.right().saturating_sub(now.width() as u16);
    text(buf, Rect { x, width: axis_row.right() - x, ..axis_row }, now, dim(theme));
}
