use crate::{
    Rgb, Theme,
    format::{self, Count},
    look::Look,
};
use borg_core::{
    Activity, ActivityPage, ActivitySort, AppState, HISTORY, MemoryGroup, MemoryRegion, MemoryTotals, Msg, Pointer,
    RegionRow,
};
use borg_i18n::tr;
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

const HEADER_ROWS: u16 = 3;
const FOOTER_ROWS: u16 = 1;
const SIZE_WIDTH: usize = 9;
const TILE_WIDTH: u16 = 12;
const LARGEST: usize = 6;
const METER_WIDTH: u16 = 20;
const CHART_ROWS: u16 = 20;
const BLOCKS: [char; 7] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇'];
const LEFT_BLOCKS: [char; 7] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉'];
const BYTE_UNITS: [&str; 6] = ["K", "M", "G", "T", "P", "E"];

fn tabs(area: Rect) -> Vec<(Rect, &'static str, KeyCode)> {
    let mut x = area.x;
    [(tr!(process_memory_tab), '1'), (tr!(process_files_tab), '2')]
        .into_iter()
        .map(|(label, key)| {
            let width = (label.width() as u16 + 2).min(area.right().saturating_sub(x));
            let rect = Rect { x, width, ..area };
            x += width;
            (rect, label, KeyCode::Char(key))
        })
        .collect()
}

fn dim(theme: &Theme) -> Style {
    theme.styles.overlay.fg(theme.styles.footer.fg.unwrap_or(Color::DarkGray))
}

fn heading(theme: &Theme) -> Style {
    theme.styles.overlay_border.add_modifier(Modifier::BOLD)
}

/// How many of the Footprint, Resident and Mapped columns fit beside a name.
fn size_columns(width: u16) -> usize {
    match width {
        56.. => 3,
        40.. => 2,
        26.. => 1,
        _ => 0,
    }
}

fn sized_line(name: &str, values: &[String], width: u16) -> String {
    let shown = size_columns(width).min(values.len());
    let name_width = usize::from(width).saturating_sub(shown * (SIZE_WIDTH + 1)).max(1);
    let name = format::truncate_end(name, name_width);
    let pad = name_width.saturating_sub(name.width());
    values[..shown]
        .iter()
        .fold(format!("{name}{}", " ".repeat(pad)), |line, value| format!("{line} {value:>SIZE_WIDTH$}"))
}

fn sizes(totals: &MemoryTotals) -> [String; 3] {
    [size(totals.footprint), size(totals.resident), format::size(totals.mapped)]
}

fn regions(activity: &Activity, theme: &Theme, inner: Rect, body: Rect, buf: &mut Buffer) {
    let columns = [tr!(memory_footprint_column), tr!(memory_resident_column), tr!(memory_mapped_column)];
    text(
        buf,
        row(inner, 2),
        sized_line(tr!(memory_region_name), &columns.map(String::from), inner.width),
        theme.styles.menu,
    );
    let top = activity.list_start(ActivityPage::Regions, usize::from(body.height));
    for (index, item) in activity.region_tree().iter().enumerate().skip(top).take(usize::from(body.height)) {
        let style = if index == activity.region_cursor {
            theme.styles.cursor
        } else if matches!(item.row, RegionRow::Group(_)) {
            heading(theme)
        } else {
            theme.styles.overlay
        };
        let line = row(body, (index - top) as u16);
        buf.set_style(line, style);
        let indent = "  ".repeat(item.depth);
        let arrow = if item.expanded { "▾" } else { "▸" };
        let name = match item.row {
            RegionRow::Group(group) => {
                format!("{arrow} {} ({})", group.label(), if item.files > 0 { item.files } else { item.totals.regions })
            }
            RegionRow::File(first) => format!("{indent}{arrow} {}", activity.file_name(first)),
            RegionRow::Region(i) => {
                let region = &activity.regions()[i];
                let label = if activity.grouped() { region.kind() } else { region.name.as_str() };
                format!("{indent}{} {label}", region.permissions)
            }
        };
        text(buf, line, sized_line(&name, &sizes(&item.totals), line.width), style);
    }
}

/// Splits text into lines of at most `width` cells, breaking between words.
fn wrap(value: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in value.split_whitespace() {
        let last = lines.last_mut().unwrap();
        if !last.is_empty() && last.width() + 1 + word.width() > width {
            lines.push(String::new());
        }
        let last = lines.last_mut().unwrap();
        if !last.is_empty() {
            last.push(' ');
        }
        last.push_str(word);
    }
    lines
}

/// Writes preview lines top to bottom and drops what does not fit.
struct Page<'a> {
    buf: &'a mut Buffer,
    area: Rect,
    y: u16,
}

impl Page<'_> {
    fn room(&self) -> u16 {
        self.area.height.saturating_sub(self.y)
    }
    fn line(&mut self, value: impl Into<String>, style: Style) -> Rect {
        let line = row(self.area, self.y);
        text(self.buf, line, value, style);
        self.y = self.y.saturating_add(1);
        line
    }
    fn wrapped(&mut self, value: &str, style: Style) {
        for line in wrap(value, usize::from(self.area.width).max(1)) {
            self.line(line, style);
        }
    }
    fn gap(&mut self) {
        self.y = self.y.saturating_add(1);
    }
}

/// A horizontal bar filled to `fraction`, shaded along the theme accent.
fn meter(buf: &mut Buffer, area: Rect, fraction: f64, look: &Look<'_>) {
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
                let color = accent.at(f32::from(x) / f32::from(area.width.max(2) - 1));
                cell.set_char(symbol).set_style(theme.styles.overlay.fg(color.into()));
            }
        }
    }
}

/// Footprint, Resident and Mapped rows, with the footprint's share of the process.
fn metrics(page: &mut Page<'_>, totals: &MemoryTotals, whole: Option<u64>, look: &Look<'_>) {
    let theme = look.theme;
    let labels = [tr!(memory_footprint_column), tr!(memory_resident_column), tr!(memory_mapped_column)];
    let label_width = labels.iter().map(|l| l.width()).max().unwrap_or(0);
    for (i, (label, value)) in labels.into_iter().zip(sizes(totals)).enumerate() {
        let line = page.line(format!("{label:<label_width$}  {value:>8}"), theme.styles.overlay);
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
        let note = format!(tr!(memory_footprint_share), percent = percent);
        let width = METER_WIDTH.min(line.right().saturating_sub(x).saturating_sub(note.width() as u16 + 1));
        if width >= 4 {
            meter(page.buf, Rect { x, width, ..line }, fraction, look);
            let note_area = Rect { x: x + width + 1, width: line.right().saturating_sub(x + width + 1), ..line };
            text(page.buf, note_area, note, dim(theme));
        }
    }
}

fn group_preview(activity: &Activity, group: MemoryGroup, look: &Look<'_>, body: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let members: Vec<_> = activity.group_members(group).take(LARGEST).collect();
    let totals = activity.group_totals(group);
    let whole = activity.region_totals().footprint;
    let mut page = Page { buf, area: body, y: 0 };
    page.line(group.label(), heading(theme));
    let count = format::count(totals.regions, Count::Region);
    page.line(
        if group.by_file() {
            format!("{} · {count}", format::count(activity.group_members(group).count(), Count::File))
        } else {
            count
        },
        dim(theme),
    );
    page.gap();
    metrics(&mut page, &totals, whole, look);
    page.gap();
    page.wrapped(group.about(), theme.styles.overlay);
    if group != MemoryGroup::Reserved && page.room() > 3 {
        page.gap();
        // Members rank by footprint, then by resident size, so a group that costs nothing
        // still shows what occupies the most RAM.
        let by_footprint = totals.footprint.unwrap_or(0) > 0;
        page.line(if by_footprint { tr!(memory_largest) } else { tr!(memory_largest_resident) }, heading(theme));
        let width = page.area.width;
        for (row, totals) in members.iter().take(LARGEST.min(usize::from(page.room().saturating_sub(5)).max(1))) {
            let name = match *row {
                RegionRow::File(first) => activity.file_name(first).to_owned(),
                RegionRow::Region(i) => activity.regions()[i].name.clone(),
                RegionRow::Group(_) => continue,
            };
            let value = if by_footprint { size(totals.footprint) } else { size(totals.resident) };
            page.line(sized_line(&format!("  {name}"), &[value], width), theme.styles.overlay);
        }
    }
    if page.room() > 4 {
        page.gap();
        for note in [tr!(memory_footprint_note), tr!(memory_resident_note), tr!(memory_mapped_note)] {
            page.wrapped(note, dim(theme));
        }
    }
}

fn file_preview(activity: &Activity, first: usize, look: &Look<'_>, body: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let regions = activity.file_regions(first);
    let Some(region) = regions.first() else { return };
    let totals = MemoryTotals::of(regions.iter().copied());
    let whole = activity.region_totals().footprint;
    let mut page = Page { buf, area: body, y: 0 };
    page.line(activity.file_name(first), heading(theme));
    if let Some(path) = &region.path {
        page.wrapped(path, dim(theme));
    }
    page.line(
        format!(
            "{} · {}",
            activity.file_group(first).unwrap_or_else(|| activity.region_group(region)).label(),
            format::count(regions.len(), Count::Region)
        ),
        dim(theme),
    );
    page.gap();
    metrics(&mut page, &totals, whole, look);
    page.gap();
    page.line(tr!(memory_file_regions), heading(theme));
    let width = page.area.width;
    for region in regions {
        let name = format!("  {} {}", region.permissions, region.kind());
        page.line(sized_line(&name, &sizes(&MemoryTotals::of([region])), width), theme.styles.overlay);
    }
}

fn files(activity: &Activity, theme: &Theme, inner: Rect, body: Rect, buf: &mut Buffer) {
    text(buf, row(inner, 2), tr!(process_files_header), theme.styles.menu);
    let top = activity.list_start(ActivityPage::Files, usize::from(body.height));
    for (offset, file) in activity.files.iter().skip(top).take(usize::from(body.height)).enumerate() {
        let style = if offset + top == activity.file_cursor { theme.styles.cursor } else { theme.styles.overlay };
        let line = row(body, offset as u16);
        buf.set_style(line, style);
        text(buf, line, format!("{:<5} {:<3} {:<6} {}", file.descriptor, file.access, file.kind, file.name), style);
    }
    if activity.files.is_empty() {
        text(
            buf,
            row(body, 0),
            if activity.pending.is_some() { tr!(activity_loading) } else { tr!(process_files_empty) },
            theme.styles.footer,
        );
    }
}

fn file_details(activity: &Activity, theme: &Theme, body: Rect, buf: &mut Buffer) {
    let Some(file) = activity.files.get(activity.file_cursor) else {
        return;
    };
    let mut y = 0;
    for (label, value) in [
        (tr!(process_file_path), file.name.clone()),
        (tr!(process_file_descriptor), file.descriptor.clone()),
        (tr!(process_file_access), file.access.clone()),
        (tr!(process_file_kind), file.kind.clone()),
        (tr!(process_file_size), size(file.size)),
        (tr!(process_file_offset), file.offset.map_or_else(|| "—".into(), |v| v.to_string())),
    ] {
        text(buf, row(body, y), label, theme.styles.overlay_border);
        let width = usize::from(body.width).max(1);
        let chars: Vec<_> = value.chars().collect();
        for chunk in chars.chunks(width) {
            y = y.saturating_add(1);
            if y >= body.height {
                return;
            }
            text(buf, row(body, y), chunk.iter().collect::<String>(), theme.styles.overlay);
        }
        y = y.saturating_add(2);
    }
}

fn inner(screen: Rect) -> Rect {
    Block::bordered().inner(screen)
}
fn body(screen: Rect) -> Rect {
    let inner = inner(screen);
    Rect {
        y: inner.y.saturating_add(HEADER_ROWS).min(inner.bottom()),
        height: inner.height.saturating_sub(HEADER_ROWS + FOOTER_ROWS),
        ..inner
    }
}
fn text(buf: &mut Buffer, area: Rect, value: impl Into<String>, style: Style) {
    if !area.is_empty() {
        Line::styled(value.into().chars().map(|c| if c.is_control() { '�' } else { c }).collect::<String>(), style)
            .render(area, buf);
    }
}
fn row(area: Rect, offset: u16) -> Rect {
    Rect { y: area.y.saturating_add(offset).min(area.bottom()), height: u16::from(offset < area.height), ..area }
}
fn size(value: Option<u64>) -> String {
    value.map_or_else(|| "—".into(), format::size)
}
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
fn columns(area: Rect) -> Vec<(ActivitySort, Rect)> {
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
fn buttons(activity: &Activity, page: ActivityPage, area: Rect) -> Vec<(Rect, &'static str, KeyCode)> {
    let mut specs: Vec<_> = if activity.retry.is_some() && activity.error.is_some() {
        vec![(tr!(activity_admin_button), KeyCode::Char('a')), (tr!(activity_back_button), KeyCode::Esc)]
    } else {
        match page {
            ActivityPage::Processes => vec![
                (tr!(activity_memory_button), KeyCode::Enter),
                (tr!(activity_filter_button), KeyCode::Char('/')),
                (tr!(activity_pause_button), KeyCode::Char(' ')),
                (tr!(activity_close_button), KeyCode::Esc),
            ],
            ActivityPage::Memory => {
                vec![
                    (tr!(activity_map_button), KeyCode::Enter),
                    (tr!(memory_search_shortcut), KeyCode::Char('/')),
                    (tr!(activity_back_button), KeyCode::Esc),
                ]
            }
            ActivityPage::Regions => vec![
                (tr!(memory_search_shortcut), KeyCode::Char('/')),
                (tr!(activity_capture_button), KeyCode::Enter),
                (tr!(activity_refresh_button), KeyCode::Char('r')),
                (tr!(activity_back_button), KeyCode::Esc),
            ],
            ActivityPage::Files | ActivityPage::FileDetails => {
                vec![(tr!(activity_refresh_button), KeyCode::Char('r')), (tr!(activity_back_button), KeyCode::Esc)]
            }
            ActivityPage::Search => vec![
                (tr!(memory_preview_button), KeyCode::Enter),
                (tr!(memory_search_shortcut), KeyCode::Char('/')),
                (tr!(memory_continue_button), KeyCode::Char('n')),
                (tr!(activity_back_button), KeyCode::Esc),
            ],
            ActivityPage::Snapshot => vec![
                (tr!(memory_search_shortcut), KeyCode::Char('/')),
                (tr!(activity_mode_button), KeyCode::Char('h')),
                (tr!(activity_diff_button), KeyCode::Char('d')),
                (tr!(activity_refresh_button), KeyCode::Char('r')),
                (tr!(activity_export_button), KeyCode::Char('e')),
                (tr!(activity_back_button), KeyCode::Esc),
            ],
        }
    };
    if activity.retry.is_some() && activity.error.is_none() {
        specs.insert(0, (tr!(activity_admin_button), KeyCode::Char('a')));
    }
    let mut x = area.x;
    specs
        .into_iter()
        .filter_map(|(label, key)| {
            let width = label.width() as u16 + 2;
            if x.saturating_add(width) > area.right() {
                return None;
            }
            let cell = Rect { x, width, ..area };
            x += width;
            Some((cell, label, key))
        })
        .collect()
}
/// Draws the activity monitor. Inspecting a process opens a window over it, holding the
/// inspection list and its preview, while the process list and overview stay beneath.
pub(crate) fn draw(state: &AppState, look: &Look<'_>, panels: [Rect; 2], monitor: [Rect; 2], buf: &mut Buffer) {
    let activity = &state.activity;
    let Some(side) = activity.side else { return };
    if activity.inspecting() {
        for (pane, page) in [(side, ActivityPage::Processes), (side.other(), ActivityPage::Memory)] {
            if !monitor[pane.index()].is_empty() {
                draw_panel(state, look, monitor[pane.index()], page, pane == side, false, buf);
            }
        }
        let [left, right] = monitor;
        let whole = Rect { width: right.right().saturating_sub(left.x), ..left };
        crate::fx::fade_to_background(buf, whole, crate::draw::BACKDROP, look.palette);
    }
    for pane in [side, side.other()] {
        let rect = panels[pane.index()];
        if rect.is_empty() {
            continue;
        }
        let list = pane == side;
        let (page, focused) =
            if list { (activity.list, !activity.preview_focused) } else { (activity.page, activity.preview_focused) };
        draw_panel(state, look, rect, page, list, focused, buf);
    }
    if activity.inspecting() {
        let window = panels.into_iter().filter(|r| !r.is_empty()).reduce(|a, b| a.union(b)).unwrap_or_default();
        crate::fx::shadow(buf, window, look.palette);
    }
}

fn draw_panel(
    state: &AppState,
    look: &Look<'_>,
    screen: Rect,
    page: ActivityPage,
    processes: bool,
    focused: bool,
    buf: &mut Buffer,
) -> Rect {
    let theme = look.theme;
    let activity = &state.activity;
    let rect = screen;
    let styles = &theme.styles;
    // The pane showing the current list or preview, rather than the monitor beneath an
    // inspection window.
    let live = if processes { page == activity.list } else { page == activity.page };
    Clear.render(rect, buf);
    let title = if !processes && live && activity.search_form.is_some() {
        tr!(memory_search_title)
    } else {
        match page {
            ActivityPage::Processes => tr!(activity_title),
            ActivityPage::Memory => tr!(activity_memory),
            ActivityPage::Regions => tr!(activity_map),
            ActivityPage::Snapshot => tr!(activity_snapshot),
            ActivityPage::Search => tr!(memory_search_results),
            ActivityPage::Files => tr!(process_files_title),
            ActivityPage::FileDetails => tr!(process_file_details),
        }
    };
    let status =
        if activity.pending.as_ref().is_some_and(|(_, r)| matches!(r, borg_core::ActivityRequest::Search { .. })) {
            tr!(memory_search_running)
        } else if activity.paused {
            tr!(activity_paused)
        } else {
            tr!(activity_live)
        };
    let controls = crate::results_view::controls(rect, state.activity.expanded);
    let room = controls.first().map_or(rect.width.saturating_sub(2), |(r, _, _)| r.x.saturating_sub(rect.x + 2));
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(if focused { styles.border_active } else { styles.border })
        .title(Line::styled(
            format::truncate_end(
                &if matches!(page, ActivityPage::Processes | ActivityPage::Memory) {
                    format!(" {title} · {status} ")
                } else {
                    format!(" {title} ")
                },
                usize::from(room),
            ),
            if focused { styles.title_active } else { styles.title },
        ));
    let inner = block.inner(rect);
    block.render(rect, buf);
    for (r, label, _) in controls {
        text(buf, r, label, styles.menu);
    }
    if inner.is_empty() {
        return rect;
    }
    let cores = activity.sample.cpu.len().max(1) as u64;
    let cpu_total = activity.sample.cpu.iter().map(|&c| u64::from(c)).sum::<u64>() / cores;
    let summary = format!(
        tr!(activity_overview),
        cpu = format_args!("{:.1}%", cpu_total as f64 / 100.0),
        used = format::size(activity.sample.used),
        total = format::size(activity.sample.total),
        swap = format::size(activity.sample.swap_used),
        swap_total = format::size(activity.sample.swap_total),
        count = activity.sample.processes.len()
    );
    let summary = if page == ActivityPage::FileDetails {
        format!(tr!(process_files_count), count = activity.files.len(), skipped = activity.files_skipped)
    } else if page == ActivityPage::Snapshot && activity.search_form.is_none() {
        let region = if activity.list == ActivityPage::Search {
            activity.search_report.as_ref().and_then(|r| r.hits.get(activity.search_cursor)).map(|h| &h.region)
        } else {
            activity.selected_region()
        };
        region.map_or_else(String::new, |r| {
            format!(
                tr!(activity_region_summary),
                access = r.permissions,
                bytes = format::size(r.mapped()),
                resident = size(r.resident),
                footprint = size(r.footprint())
            )
        })
    } else {
        summary
    };
    let summary = if processes && page != ActivityPage::Processes {
        let pid = activity.selected.map_or(0, |p| p.pid);
        if activity.regions().is_empty() {
            format!(
                tr!(process_memory_sampled),
                pid = pid,
                ram = size(activity.process().and_then(|p| p.resident)),
                mapped = size(activity.process().and_then(|p| p.virtual_bytes))
            )
        } else {
            let totals = activity.region_totals();
            format!(
                tr!(process_memory_summary),
                pid = pid,
                footprint = size(totals.footprint),
                resident = size(totals.resident),
                mapped = format::size(totals.mapped)
            )
        }
    } else {
        summary
    };
    text(buf, row(inner, 0), summary, styles.overlay_border);
    let subtitle = if page == ActivityPage::Snapshot && activity.search_form.is_none() {
        if activity.list == ActivityPage::Search {
            activity
                .search_report
                .as_ref()
                .and_then(|r| r.hits.get(activity.search_cursor))
                .map(|h| h.region.name.clone())
                .unwrap_or_default()
        } else {
            activity.selected_region().map(|r| r.name.clone()).unwrap_or_default()
        }
    } else if page == ActivityPage::Processes {
        format!(tr!(activity_filter), filter = activity.filter)
    } else if let Some(p) = activity.process() {
        format!(
            tr!(activity_process_label),
            name = p.name,
            pid = p.key.pid,
            executable = p.executable.as_ref().map_or_else(String::new, |p| p.display().to_string())
        )
    } else {
        tr!(activity_process_gone).into()
    };
    text(buf, row(inner, 1), subtitle, styles.overlay);
    if processes && page != ActivityPage::Processes {
        Clear.render(row(inner, 1), buf);
        buf.set_style(row(inner, 1), styles.overlay);
        for (rect, label, code) in tabs(row(inner, 1)) {
            let active = if code == KeyCode::Char('2') {
                activity.list == ActivityPage::Files
            } else {
                activity.list != ActivityPage::Files
            };
            let style = if active { styles.cursor } else { styles.menu };
            buf.set_style(rect, style);
            text(buf, rect, label, style);
        }
    }
    let body = body(screen);
    if !processes
        && live
        && let Some(form) = &activity.search_form
    {
        search_form(form, theme, body, buf);
    } else {
        match page {
            ActivityPage::Search => search_results(activity, theme, body, buf),
            ActivityPage::Processes => {
                let header = row(inner, 2);
                for (sort, cell) in columns(header) {
                    text(
                        buf,
                        cell,
                        format!(
                            "{}{}",
                            sort_label(sort),
                            if activity.sort == sort { if activity.reverse { " ↓" } else { " ↑" } } else { "" }
                        ),
                        styles.menu,
                    );
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
                            ActivitySort::Growth => {
                                if p.resident.is_some() {
                                    growth(activity.growth(p.key))
                                } else {
                                    "—".into()
                                }
                            }
                            ActivitySort::Read => size(p.read),
                            ActivitySort::Write => size(p.written),
                            ActivitySort::Name => format!(
                                "{}{}",
                                " ".repeat(depth.saturating_mul(2).min(usize::from(cell.width / 2))),
                                p.name
                            ),
                        };
                        text(buf, cell, value, style);
                    }
                }
                if activity.rows.is_empty() {
                    text(buf, row(body, 0), tr!(activity_no_processes), styles.overlay);
                }
            }
            ActivityPage::Memory => memory(activity, look, body, buf),
            ActivityPage::Regions => regions(activity, theme, inner, body, buf),
            ActivityPage::Files => files(activity, theme, inner, body, buf),
            ActivityPage::FileDetails => file_details(activity, theme, body, buf),
            ActivityPage::Snapshot => {
                // Group and file previews have no region header, so they start at the top.
                let whole = Rect { height: inner.height.saturating_sub(FOOTER_ROWS), ..inner };
                let summary = activity.list != ActivityPage::Regions
                    || (activity.selected_group().is_none() && activity.selected_file().is_none());
                snapshot(activity, look, if summary { body } else { whole }, buf)
            }
        }
    }
    let footer = Rect {
        y: inner.bottom().saturating_sub(FOOTER_ROWS.min(inner.height)),
        height: FOOTER_ROWS.min(inner.height),
        ..inner
    };
    let progress = activity.search_progress.map(|(scanned, total, hits)| {
        format!(tr!(memory_search_progress), scanned = format::size(scanned), total = format::size(total), hits = hits)
    });
    let files_note = (activity.list == ActivityPage::Files && activity.files_loaded)
        .then(|| format!(tr!(process_files_count), count = activity.files.len(), skipped = activity.files_skipped));
    let note = if !live {
        ""
    } else {
        activity
            .error
            .as_deref()
            .or(progress.as_deref())
            .or(state.notice.as_deref())
            .or(files_note.as_deref())
            .unwrap_or(if activity.pending.is_some() { tr!(activity_loading) } else { "" })
    };
    text(buf, row(footer, 0), note, if activity.error.is_some() { styles.notice } else { styles.footer });
    rect
}
fn change(value: i128) -> String {
    let amount = format::size(value.unsigned_abs().min(u128::from(u64::MAX)) as u64);
    match value.signum() {
        1 => format!("▲ {amount}"),
        -1 => format!("▼ {amount}"),
        _ => amount,
    }
}

fn memory(activity: &Activity, look: &Look<'_>, body: Rect, buf: &mut Buffer) {
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
        (tr!(activity_cpu_tile), p.cpu.map_or_else(|| "—".into(), |v| format!("{:.1}%", f64::from(v) / 100.0))),
        (tr!(activity_virtual), size(p.virtual_bytes)),
    ];
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
    let chart_area =
        Rect { y: body.y.saturating_add(y).min(body.bottom()), height: body.height.saturating_sub(y), ..body };
    chart(history.map_or(&[][..], |h| &h.values[..]), look, chart_area, buf);
}

/// Picks a round axis step and range covering `values`, in the largest unit below them.
fn scale(values: &[u64]) -> (f64, f64, [String; 3]) {
    let min = values.iter().copied().min().unwrap_or(0) as f64;
    let max = values.iter().copied().max().unwrap_or(0) as f64;
    let mut unit = 1024.0;
    let mut suffix = 0;
    while max / unit >= 1024.0 && suffix + 1 < BYTE_UNITS.len() {
        unit *= 1024.0;
        suffix += 1;
    }
    let (low, high) = (min / unit, max / unit);
    let span = (high - low).max(high * 0.04).max(0.01);
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
    let label = |v: f64| format!("{v:.decimals$}{}", BYTE_UNITS[suffix]);
    (lo * unit, hi * unit, [label(hi), label(mid), label(lo)])
}

/// The value at plot column `x` on a fixed window of `HISTORY` samples ending now.
fn sample_at(values: &[u64], x: u16, width: u16) -> Option<f64> {
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

/// An area chart of resident memory: round axis labels, faint grid lines, and columns
/// shaded from the background up to a bright accent edge.
fn chart(values: &[u64], look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    if area.height < 2 {
        return;
    }
    text(buf, row(area, 0), tr!(memory_chart_title), theme.styles.overlay.add_modifier(Modifier::BOLD));
    if let Some(&latest) = values.last() {
        let latest = format::size(latest);
        let x = area.right().saturating_sub(latest.width() as u16);
        text(buf, Rect { x, width: area.right() - x, ..row(area, 0) }, latest, heading(theme));
    }
    if values.len() < 2 {
        let note = Rect { x: area.x + tr!(memory_chart_title).width() as u16 + 2, ..row(area, 0) };
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
    let (lo, hi, labels) = scale(values);
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
    text(buf, axis_row, format!(tr!(memory_chart_span), seconds = HISTORY), dim(theme));
    let now = tr!(memory_chart_now);
    let x = axis_row.right().saturating_sub(now.width() as u16);
    text(buf, Rect { x, width: axis_row.right() - x, ..axis_row }, now, dim(theme));
}
fn snapshot(activity: &Activity, look: &Look<'_>, body: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    if activity.list == ActivityPage::Regions {
        if let Some(group) = activity.selected_group() {
            return group_preview(activity, group, look, body, buf);
        }
        if let Some(first) = activity.selected_file() {
            return file_preview(activity, first, look, body, buf);
        }
    }
    let Some(snapshot) = &activity.snapshot else {
        let note = if activity.error.is_some() {
            tr!(activity_preview_failed)
        } else if activity.preview_waiting() {
            tr!(activity_loading)
        } else if activity.list == ActivityPage::Regions && activity.selected_region().is_some_and(|r| !r.readable()) {
            tr!(activity_region_unreadable)
        } else if activity.list == ActivityPage::Search {
            tr!(activity_no_matches)
        } else {
            tr!(activity_region_choose)
        };
        text(buf, row(body, 0), note, theme.styles.footer);
        return;
    };
    let columns = activity.snapshot_columns(usize::from(body.width));
    let details = format!(
        tr!(activity_capture_label),
        pid = snapshot.process.pid,
        address = snapshot.address,
        bytes = snapshot.bytes.len(),
        mode = if activity.text { tr!(activity_text) } else { tr!(activity_hex) }
    );
    text(buf, row(body, 0), details, theme.styles.overlay_border);
    let old = activity.previous.as_ref();
    if activity.diff {
        text(
            buf,
            row(body, 1),
            if old.is_some() { tr!(activity_compare) } else { tr!(activity_no_previous) },
            theme.styles.footer,
        );
    }
    for (line_index, chunk) in snapshot
        .bytes
        .chunks(columns)
        .enumerate()
        .skip(activity.offset)
        .take(usize::from(body.height.saturating_sub(2)))
    {
        let offset = line_index * columns;
        let mut spans = vec![Span::styled(format!("{:016x}  ", snapshot.address + offset as u64), theme.styles.footer)];
        for (i, &byte) in chunk.iter().enumerate() {
            let changed = activity.diff && old.is_some_and(|old| old.bytes.get(offset + i) != Some(&byte));
            let address = snapshot.address.saturating_add((offset + i) as u64);
            let matched = activity.from_search
                && activity
                    .search_report
                    .as_ref()
                    .and_then(|r| r.hits.get(activity.search_cursor))
                    .is_some_and(|h| address >= h.address && address < h.address.saturating_add(h.length as u64));
            let style = if matched {
                theme.styles.cursor.add_modifier(Modifier::BOLD)
            } else if changed {
                theme.styles.marked.add_modifier(Modifier::BOLD)
            } else {
                theme.styles.overlay
            };
            spans.push(Span::styled(
                if activity.text {
                    if byte.is_ascii_graphic() || byte == b' ' { char::from(byte).to_string() } else { ".".into() }
                } else {
                    format!("{byte:02x} ")
                },
                style,
            ));
        }
        if !activity.text {
            spans.push(Span::styled(
                format!(
                    " {:width$}",
                    chunk
                        .iter()
                        .map(|&b| if b.is_ascii_graphic() || b == b' ' { char::from(b) } else { '.' })
                        .collect::<String>(),
                    width = columns
                ),
                theme.styles.overlay,
            ));
        }
        let y = line_index - activity.offset + 2;
        if y < usize::from(body.height) {
            Line::from(spans).render(row(body, y as u16), buf);
        }
    }
}
pub(crate) fn keybar(state: &AppState, area: Rect, theme: &Theme, buf: &mut Buffer) {
    let activity = &state.activity;
    let page = activity.focused_page();
    buf.set_style(area, theme.styles.key_label);
    let controls = if activity.search_form.is_some() {
        vec![(row(area, 0), tr!(memory_submit_button), KeyCode::Enter)]
    } else {
        buttons(activity, page, row(area, 0))
    };
    for (rect, label, _) in controls {
        text(buf, rect, label, theme.styles.key);
    }
    let hint = if activity.search_form.is_some() {
        tr!(memory_search_hint)
    } else {
        match page {
            ActivityPage::Processes => tr!(activity_list_hint),
            ActivityPage::Memory => tr!(activity_memory_hint),
            ActivityPage::Regions => tr!(activity_map_hint),
            ActivityPage::Snapshot => tr!(activity_snapshot_hint),
            ActivityPage::Search => tr!(memory_results_hint),
            ActivityPage::Files | ActivityPage::FileDetails => tr!(process_files_hint),
        }
    };
    text(buf, crate::branding::content(row(area, 1)), hint, theme.styles.key_label);
}
pub(crate) fn panel_mouse(
    state: &AppState,
    event: MouseEvent,
    double: bool,
    layout: &borg_core::Layout,
) -> Option<Msg> {
    let activity = &state.activity;
    let preview = activity.preview_focused;
    let point = Position::new(event.column, event.row);
    if layout.keybar.contains(point) {
        if activity.search_form.is_some()
            && row(layout.keybar, 0).contains(point)
            && event.kind == MouseEventKind::Down(MouseButton::Left)
        {
            return Some(Msg::Pointer(Pointer::ActivityControl { preview: true, code: KeyCode::Enter }));
        }
        let page = if preview { activity.page } else { activity.list };
        return buttons(activity, page, row(layout.keybar, 0))
            .into_iter()
            .find(|(rect, _, _)| rect.contains(point))
            .map(|(_, _, code)| Msg::Pointer(Pointer::ActivityControl { preview, code }));
    }
    let side = activity.side?;
    for panel in [side, side.other()] {
        let rect = layout.panels[panel.index()];
        if !rect.contains(point) {
            continue;
        }
        let preview = panel != side;
        if event.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some((_, _, action)) =
                crate::results_view::controls(rect, activity.expanded).into_iter().find(|(r, _, _)| r.contains(point))
        {
            return Some(Msg::Pointer(match action {
                borg_core::Action::SetExpanded(expand) => Pointer::ActivityExpand { preview, expand },
                _ => Pointer::ActivityHide,
            }));
        }

        let code = match event.kind {
            MouseEventKind::ScrollUp => Some(KeyCode::Up),
            MouseEventKind::ScrollDown => Some(KeyCode::Down),
            _ => None,
        };
        if let Some(code) = code {
            return Some(Msg::Pointer(Pointer::ActivityControl { preview, code }));
        }
        if event.kind != MouseEventKind::Down(MouseButton::Left) {
            return None;
        }
        let body = body(rect);
        if !preview && activity.list != ActivityPage::Processes && row(inner(rect), 1).contains(point) {
            return tabs(row(inner(rect), 1))
                .into_iter()
                .find(|(r, _, _)| r.contains(point))
                .map(|(_, _, code)| Msg::Pointer(Pointer::ActivityControl { preview: false, code }));
        }
        if !preview && activity.list == ActivityPage::Processes && row(inner(rect), 2).contains(point) {
            return columns(row(inner(rect), 2))
                .into_iter()
                .find(|(_, r)| r.contains(point))
                .map(|(sort, _)| Msg::Pointer(Pointer::ActivitySort(sort)));
        }
        if preview && activity.search_form.is_some() {
            return (0..4)
                .find(|&i| form_field(body, i).contains(point))
                .map(|i| Msg::Pointer(Pointer::ActivitySearchField(i)));
        }
        let body = if !preview && activity.list == ActivityPage::Search {
            Rect { y: body.y.saturating_add(1).min(body.bottom()), height: body.height.saturating_sub(1), ..body }
        } else {
            body
        };
        if body.contains(point) && !preview {
            let index = activity.list_start(activity.list, usize::from(body.height)) + usize::from(point.y - body.y);
            return Some(Msg::Pointer(Pointer::ActivityRow { index, open: double, preview }));
        }
        return Some(Msg::Pointer(Pointer::ActivityControl { preview, code: KeyCode::Null }));
    }
    None
}

fn form_field(body: Rect, index: usize) -> Rect {
    Rect {
        y: body.y.saturating_add((index * 3) as u16).min(body.bottom()),
        height: body.height.saturating_sub((index * 3) as u16).min(3),
        ..body
    }
}
fn search_form(form: &borg_core::MemorySearchForm, theme: &Theme, body: Rect, buf: &mut Buffer) {
    use borg_core::MemorySearchMode;
    let mode = match form.mode {
        MemorySearchMode::Text => tr!(memory_mode_text),
        MemorySearchMode::Hex => tr!(memory_mode_hex),
        MemorySearchMode::Unicode => tr!(memory_mode_unicode),
    };
    let fields = [
        (tr!(memory_search_query), form.input.text().to_owned()),
        (tr!(memory_search_mode), format!("‹ {mode} ›")),
        (tr!(memory_search_case), if form.match_case { "[x]".into() } else { "[ ]".into() }),
        (
            tr!(memory_search_scope),
            if form.snapshot_only { tr!(memory_search_snapshot).into() } else { tr!(memory_search_process).into() },
        ),
    ];
    for (i, (label, value)) in fields.into_iter().enumerate() {
        let area = form_field(body, i);
        if area.is_empty() {
            continue;
        }
        let style = if form.selected == i { theme.styles.cursor } else { theme.styles.input };
        let block = Block::bordered()
            .title(Line::styled(format!(" {label} "), theme.styles.overlay))
            .style(style)
            .border_style(if form.selected == i { theme.styles.overlay_border } else { theme.styles.border });
        let inner = block.inner(area);
        block.render(area, buf);
        if i == 0 && form.selected == 0 && !inner.is_empty() {
            let before: String = form.input.text().chars().take(form.input.cursor_chars()).collect();
            let shift = before.width().saturating_sub(usize::from(inner.width.saturating_sub(1)));
            text(buf, inner, crate::dialog_view::skip_columns(&value, shift), style);
            let x = inner.x + (before.width() - shift) as u16;
            buf[(x, inner.y)].set_style(style.add_modifier(Modifier::REVERSED));
        } else {
            text(buf, inner, value, style);
        }
    }
}
fn search_results(activity: &Activity, theme: &Theme, body: Rect, buf: &mut Buffer) {
    let Some(report) = &activity.search_report else { return };
    let more = if report.cancelled {
        tr!(memory_search_cancelled)
    } else if report.next.is_some() {
        tr!(memory_search_more)
    } else {
        tr!(memory_search_complete)
    };
    text(
        buf,
        row(body, 0),
        format!(
            tr!(memory_search_summary),
            hits = report.hits.len(),
            scanned = format::size(report.scanned),
            total = format::size(report.total),
            skipped = format::size(report.skipped),
            more = more
        ),
        theme.styles.footer,
    );
    let list = Rect { y: body.y.saturating_add(1).min(body.bottom()), height: body.height.saturating_sub(1), ..body };
    for (i, hit) in report
        .hits
        .iter()
        .skip(activity.list_start(ActivityPage::Search, usize::from(list.height)))
        .take(usize::from(list.height))
        .enumerate()
    {
        let selected =
            i + activity.list_start(ActivityPage::Search, usize::from(list.height)) == activity.search_cursor;
        let style = if selected { theme.styles.cursor } else { theme.styles.overlay };
        let line = row(list, i as u16);
        buf.set_style(line, style);
        text(buf, line, format!("{:016x} {:<9} {}", hit.address, hit.encoding, hit.region.name), style);
    }
}
