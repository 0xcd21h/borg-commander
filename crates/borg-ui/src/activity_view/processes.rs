//! The process list: its sortable columns and the filter row above them.

use super::{dim, heading, row, size, text};
use crate::{Theme, format, look::Look};
use borg_core::{Activity, ActivityPage, ActivitySort};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

fn cpu(value: Option<u32>) -> String {
    value.map_or_else(|| "—".into(), |v| format!("{:.1}", f64::from(v) / 100.0))
}

fn growth(value: i128) -> String {
    format!(
        "{}{}",
        if value > 0 {
            "+"
        } else if value < 0 {
            "−"
        } else {
            ""
        },
        format::size(value.unsigned_abs().min(u128::from(u64::MAX)) as u64)
    )
}

pub(super) fn columns(area: Rect) -> Vec<(ActivitySort, Rect)> {
    let wide = area.width >= 100;
    let growth = area.width >= 72;
    let mut x = area.x;
    let mut out = Vec::new();
    for (key, width) in [
        (ActivitySort::Pid, 7),
        (ActivitySort::Cpu, if area.width >= 44 { 7 } else { 0 }),
        (ActivitySort::Memory, 10),
        (ActivitySort::Growth, if growth { 10 } else { 0 }),
        (ActivitySort::Read, if wide { 10 } else { 0 }),
        (ActivitySort::Write, if wide { 10 } else { 0 }),
        (ActivitySort::Name, u16::MAX),
    ] {
        let width = width.min(area.right().saturating_sub(x));
        if width > 0 {
            out.push((key, Rect { x, width, ..area }));
            x += width;
        }
    }
    out
}

fn sort_label(sort: ActivitySort) -> &'static str {
    match sort {
        ActivitySort::Pid => tr!(activity_pid),
        ActivitySort::Cpu => tr!(activity_cpu),
        ActivitySort::Memory => tr!(activity_ram),
        ActivitySort::Growth => tr!(activity_growth),
        ActivitySort::Read => tr!(activity_read),
        ActivitySort::Write => tr!(activity_write),
        ActivitySort::Name => tr!(name),
    }
}

/// The process filter: an input with a cursor while typing, a badge while a filter applies,
/// and a hint otherwise, so it is always clear whether keys type or move.
pub(super) fn filter_row(activity: &Activity, look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let styles = &theme.styles;
    let label = tr!(activity_filter_label);
    let (shown, total) = (activity.rows.len(), activity.sample.processes.len());
    if activity.filtering {
        buf.set_style(area, styles.input);
        let typed = format!(" {}", activity.filter);
        Line::from(vec![Span::styled(label, styles.cursor), Span::styled(typed.clone(), styles.input)])
            .render(area, buf);
        let x = area.x.saturating_add((label.width() + typed.width()) as u16);
        if x < area.right() {
            buf[(x, area.y)].set_style(styles.input.add_modifier(Modifier::REVERSED));
        }
        let hint = tr!(activity_filter_typing, shown = shown, total = total);
        let start = area.right().saturating_sub(hint.width() as u16 + 1).max(x.saturating_add(2));
        text(buf, Rect { x: start, width: area.right().saturating_sub(start), ..area }, hint, styles.input);
    } else if !activity.filter.is_empty() {
        let hint = tr!(activity_filter_applied, shown = shown, total = total);
        Line::from(vec![
            Span::styled(label, styles.cursor),
            Span::styled(format!(" {} ", activity.filter), heading(theme)),
            Span::styled(hint, dim(theme)),
        ])
        .render(area, buf);
    } else {
        text(buf, area, tr!(activity_filter_placeholder), dim(theme));
    }
}

/// The process tree under its sortable column headings, the selected process marked while the
/// list has focus.
pub(super) fn list(activity: &Activity, theme: &Theme, header: Rect, body: Rect, focused: bool, buf: &mut Buffer) {
    let styles = &theme.styles;
    for (sort, cell) in columns(header) {
        let arrow = if activity.sort == sort { if activity.reverse { " ↓" } else { " ↑" } } else { "" };
        text(buf, cell, format!("{}{arrow}", sort_label(sort)), styles.menu);
    }
    for (offset, &(i, depth)) in activity
        .rows
        .iter()
        .skip(activity.list_start(ActivityPage::Processes, usize::from(body.height)))
        .take(usize::from(body.height))
        .enumerate()
    {
        let p = &activity.sample.processes[i];
        let line = row(body, offset as u16);
        let selected = focused && Some(p.key) == activity.selected;
        let style = if selected { styles.cursor } else { styles.overlay };
        buf.set_style(line, style);
        for (sort, cell) in columns(line) {
            let value = match sort {
                ActivitySort::Pid => p.key.pid.to_string(),
                ActivitySort::Cpu => cpu(p.cpu),
                ActivitySort::Memory => size(p.resident),
                ActivitySort::Growth if p.resident.is_some() => growth(activity.growth(p.key)),
                ActivitySort::Growth => "—".into(),
                ActivitySort::Read => size(p.read),
                ActivitySort::Write => size(p.written),
                ActivitySort::Name => {
                    format!("{}{}", " ".repeat(depth.saturating_mul(2).min(usize::from(cell.width / 2))), p.name)
                }
            };
            text(buf, cell, value, style);
        }
    }
    if activity.rows.is_empty() {
        text(buf, row(body, 0), tr!(activity_no_processes), styles.overlay);
    }
}
