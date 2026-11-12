//! Disk use: a row per kind of what Docker keeps, their items below them, and in the other pane
//! what the selected row is and the command F8 types for it.

use super::{FILTER_ROW, HEADER_ROW, volumes};
use crate::{
    activity_view::{FOOTER_ROWS, body, dim, emphasis, heading, row, text, wrap},
    format,
    look::Look,
    spinner::Spinner,
};
use borg_core::{AppState, Disk, DiskGroup, DiskItem, DiskKind, DiskRow, Docker, NoCleanup};
use borg_docker::DiskUse;
use borg_i18n::tr;
use crossterm::event::KeyCode;
use jiff::Zoned;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use std::time::SystemTime;
use unicode_width::UnicodeWidthStr;

const COUNT_WIDTH: usize = 5;
const SIZE_WIDTH: usize = 7;
/// What a size Docker could not measure reads as.
const UNKNOWN: &str = "?";
const MARK: &str = "✓";
const FOLDED: char = '▸';
const UNFOLDED: char = '▾';
/// The widest a fact's label column grows before its value takes the rest.
const LABEL_LIMIT: usize = 14;
const SHORT_ID: usize = 12;

pub(super) fn kind_name(kind: DiskKind) -> &'static str {
    match kind {
        DiskKind::BuildCache => tr!(docker_disk_build_cache),
        DiskKind::DanglingImages => tr!(docker_disk_dangling_images),
        DiskKind::UnusedImages => tr!(docker_disk_unused_images),
        DiskKind::ImagesInUse => tr!(docker_disk_images_in_use),
        DiskKind::StoppedContainers => tr!(docker_disk_stopped_containers),
        DiskKind::AnonymousVolumes => tr!(docker_disk_anonymous_volumes),
        DiskKind::NamedVolumes => tr!(docker_disk_named_volumes),
        DiskKind::VolumesInUse => tr!(docker_disk_volumes_in_use),
        DiskKind::UnusedNetworks => tr!(docker_disk_unused_networks),
    }
}

fn about(kind: DiskKind) -> &'static str {
    match kind {
        DiskKind::BuildCache => tr!(docker_disk_about_build_cache),
        DiskKind::DanglingImages => tr!(docker_disk_about_dangling),
        DiskKind::UnusedImages => tr!(docker_disk_about_unused),
        DiskKind::ImagesInUse => tr!(docker_disk_about_images_in_use),
        DiskKind::StoppedContainers => tr!(docker_disk_about_stopped),
        DiskKind::AnonymousVolumes => tr!(docker_disk_about_anonymous),
        DiskKind::NamedVolumes => tr!(docker_disk_about_named),
        DiskKind::VolumesInUse => tr!(docker_disk_about_volumes_in_use),
        DiskKind::UnusedNetworks => tr!(docker_disk_about_networks),
    }
}

pub(super) fn size(value: Option<u64>) -> String {
    value.map_or_else(|| UNKNOWN.into(), format::size)
}

/// Name, count, size and reclaimable cells of a row; each number column is at least as wide
/// as its heading.
pub(super) fn columns(area: Rect) -> [Rect; 4] {
    let widths = [
        tr!(docker_disk_count).width().max(COUNT_WIDTH),
        tr!(docker_disk_size).width().max(SIZE_WIDTH),
        tr!(docker_disk_reclaimable).width().max(SIZE_WIDTH),
    ]
    .map(|width| width as u16);
    let name = area.width.saturating_sub(widths.iter().sum::<u16>() + 3);
    let mut x = area.x;
    [name, widths[0], widths[1], widths[2]].map(|width| {
        let cell = Rect { x, width: width.min(area.right().saturating_sub(x)), ..area };
        x = x.saturating_add(width + 1);
        cell
    })
}

/// The part of `area` from `start` cells in, `width` wide.
fn cell(area: Rect, start: usize, width: usize) -> Rect {
    let x = area.x.saturating_add(u16::try_from(start).unwrap_or(u16::MAX)).min(area.right());
    let width = u16::try_from(width).unwrap_or(u16::MAX).min(area.right() - x);
    Rect { x, width, ..area }
}

/// Text at the right edge of `area`, as numbers line up.
fn right(buf: &mut Buffer, area: Rect, value: &str, style: Style) {
    let pad = usize::from(area.width).saturating_sub(value.width());
    text(buf, area, format!("{}{value}", " ".repeat(pad)), style);
}

fn title(docker: &Docker) -> String {
    match &docker.context {
        Some(context) => format!("{} · {context}", tr!(docker_disk_title)),
        None => tr!(docker_disk_title).into(),
    }
}

/// A kind's row label: its fold mark and name.
fn kind_label(disk: &Disk, group: &DiskGroup) -> String {
    let fold = match (group.items.is_empty(), disk.unfolded(group.kind)) {
        (true, _) => ' ',
        (false, true) => UNFOLDED,
        (false, false) => FOLDED,
    };
    format!("{fold} {}", kind_name(group.kind))
}

fn item_label(disk: &Disk, usage: &DiskUse, item: DiskItem) -> String {
    let mark = if disk.marked(item) { MARK } else { " " };
    format!("  {mark} {}", item.label(usage))
}

pub(super) fn draw_list(
    state: &AppState,
    look: &Look<'_>,
    rect: Rect,
    spinner: Option<Spinner>,
    clock: &Zoned,
    buf: &mut Buffer,
) {
    let docker = &state.docker;
    let disk = &docker.disk;
    let theme = look.theme;
    let styles = &theme.styles;
    let inner = crate::results_view::pane_frame(docker.expanded, look, rect, &title(docker), true, buf);
    if inner.is_empty() {
        return;
    }
    if disk.usage.is_some() {
        let total = tr!(docker_disk_total, size = format::size(disk.reclaimable()));
        text(buf, row(inner, 0), total, emphasis(theme));
    }
    filter_row(disk, look, row(inner, FILTER_ROW), buf);
    let headings = [tr!(docker_name), tr!(docker_disk_count), tr!(docker_disk_size), tr!(docker_disk_reclaimable)];
    for (index, (cell, label)) in columns(row(inner, HEADER_ROW)).into_iter().zip(headings).enumerate() {
        if index == 0 { text(buf, cell, label, styles.menu) } else { right(buf, cell, label, styles.menu) }
    }
    let body = body(rect);
    let Some(usage) = &disk.usage else {
        if let Some(error) = &disk.error {
            for (offset, line) in wrap(error, usize::from(body.width)).into_iter().enumerate() {
                text(buf, row(body, offset as u16), line, styles.notice);
            }
        } else if let Some(spinner) = spinner {
            spinner.draw_centered(tr!(docker_disk_measuring), look, dim(theme), body, buf);
        }
        return;
    };
    if disk.rows.is_empty() {
        text(buf, row(body, 0), tr!(docker_disk_no_matches), dim(theme));
    }
    let name_width = usize::from(columns(row(body, 0))[0].width);
    let facts = volumes::list_layout(
        disk,
        usage,
        name_width,
        clock,
        |kind| disk.groups.iter().find(|group| group.kind == kind).map_or(0, |group| kind_label(disk, group).width()),
        |item| item_label(disk, usage, item).width(),
    );
    let start = disk.list_start(usize::from(body.height));
    for (offset, (index, &item)) in disk.rows.iter().enumerate().skip(start).take(usize::from(body.height)).enumerate()
    {
        let line = row(body, offset as u16);
        let selected = index == disk.cursor;
        let base = if selected { styles.cursor } else { styles.overlay };
        buf.set_style(line, base);
        let [name, count, bytes, frees] = columns(line);
        match item {
            DiskRow::Kind(group) => {
                let DiskGroup { kind, totals, ref items } = disk.groups[group];
                let style = if selected { base } else { heading(theme) };
                let label = kind_label(disk, &disk.groups[group]);
                let after = label.width() + 1;
                text(buf, name, label, style.add_modifier(Modifier::BOLD));
                if kind.holds_data() && disk.unfolded(kind) && !items.is_empty() {
                    let quiet = if selected { base } else { dim(theme) };
                    for (start, width, heading) in
                        volumes::headings(&facts).into_iter().filter(|(start, ..)| *start >= after)
                    {
                        text(buf, cell(name, start, width), format::truncate_end(heading, width), quiet);
                    }
                }
                right(buf, count, &totals.count.to_string(), base);
                if kind != DiskKind::UnusedNetworks {
                    right(buf, bytes, &format::size(totals.size), base);
                    let quiet = if selected || totals.reclaimable > 0 { base } else { dim(theme) };
                    right(buf, frees, &format::size(totals.reclaimable), quiet);
                }
            }
            DiskRow::Item(_, item) => {
                let marked = disk.marked(item);
                let style = if selected {
                    base
                } else if marked {
                    base.patch(styles.marked)
                } else {
                    base
                };
                let label = item_label(disk, usage, item);
                if let DiskItem::Volume(index) = item {
                    text(buf, name, format::truncate_end(&label, facts.label), style);
                    let quiet = if selected { base } else { dim(theme) };
                    for (start, width, value) in volumes::cells(&facts, disk, &usage.volumes[index], clock) {
                        text(buf, cell(name, start, width), format::truncate_end(&value, width), quiet);
                    }
                } else {
                    text(buf, name, format::truncate_end(&label, usize::from(name.width)), style);
                }
                right(buf, bytes, &size(item.size(usage)), style);
                right(buf, frees, &size(item.frees(usage)), if selected { base } else { dim(theme) });
            }
        }
    }
    let footer = Rect {
        y: inner.bottom().saturating_sub(FOOTER_ROWS.min(inner.height)),
        height: FOOTER_ROWS.min(inner.height),
        ..inner
    };
    let line = row(footer, 0);
    let opening = disk.opening.as_ref().map(|opening| {
        if opening.pull {
            tr!(docker_disk_pulling, image = borg_docker::helper::IMAGE_NAME)
        } else {
            tr!(docker_disk_starting).to_owned()
        }
    });
    if let Some(spinner) = spinner.filter(|_| disk.loading.is_some()) {
        spinner.line(tr!(docker_disk_measuring), look, dim(theme)).render(line, buf);
    } else if let Some((spinner, opening)) = spinner.zip(opening) {
        spinner.line(&opening, look, dim(theme)).render(line, buf);
    } else if let Some(spinner) = spinner.filter(|_| disk.reading.is_some()) {
        spinner.line(tr!(docker_disk_reading), look, dim(theme)).render(line, buf);
    } else if let Some(error) = &disk.error {
        text(buf, line, format::truncate_end(error, usize::from(line.width)), styles.notice);
    } else if let Some(notice) = &state.notice {
        text(buf, line, notice.as_str(), dim(theme));
    } else if disk.mark_count() > 0 {
        text(buf, line, tr!(marked_count, count = disk.mark_count()), dim(theme));
    }
}

/// The filter as the container list has it: an input while typing, a badge while it applies.
fn filter_row(disk: &Disk, look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let styles = &theme.styles;
    let label = tr!(activity_filter_label);
    let items = |groups: &[DiskGroup]| groups.iter().map(|group| group.items.len()).sum::<usize>();
    let shown = items(&disk.groups);
    let total = disk
        .usage
        .as_ref()
        .map_or(0, |usage| DiskKind::ALL.iter().map(|kind| kind.members(usage).len()).sum::<usize>());
    if disk.filtering {
        buf.set_style(area, styles.input);
        let typed = format!(" {}", disk.filter);
        Line::from(vec![Span::styled(label, styles.cursor), Span::styled(typed.clone(), styles.input)])
            .render(area, buf);
        let x = area.x.saturating_add((label.width() + typed.width()) as u16);
        if x < area.right() {
            buf[(x, area.y)].set_style(styles.input.add_modifier(Modifier::REVERSED));
        }
        let hint = tr!(activity_filter_typing, shown = shown, total = total);
        let start = area.right().saturating_sub(hint.width() as u16 + 1).max(x.saturating_add(2));
        text(buf, Rect { x: start, width: area.right().saturating_sub(start), ..area }, hint, styles.input);
    } else if !disk.filter.is_empty() {
        Line::from(vec![
            Span::styled(label, styles.cursor),
            Span::styled(format!(" {} ", disk.filter), heading(theme)),
            Span::styled(tr!(activity_filter_applied, shown = shown, total = total), dim(theme)),
        ])
        .render(area, buf);
    } else {
        text(buf, area, tr!(docker_disk_filter_placeholder), dim(theme));
    }
}

/// Lines down a pane: facts with their labels in a column, and wrapped paragraphs.
pub(super) struct Sheet<'a> {
    area: Rect,
    y: u16,
    pub label: u16,
    pub look: &'a Look<'a>,
}

impl Sheet<'_> {
    pub fn fact(&mut self, buf: &mut Buffer, label: &str, value: impl Into<String>) {
        let value = value.into();
        if value.is_empty() || self.y >= self.area.height {
            return;
        }
        let line = row(self.area, self.y);
        let width = self.label.min(line.width);
        text(buf, Rect { width, ..line }, label, dim(self.look.theme));
        let rest = Rect { x: line.x + width, width: line.width - width, ..line };
        text(buf, rest, format::truncate_end(&value, usize::from(rest.width)), self.look.theme.styles.overlay);
        self.y += 1;
    }

    pub fn paragraph(&mut self, buf: &mut Buffer, value: &str, style: Style) {
        for line in wrap(value, usize::from(self.area.width)) {
            if self.y >= self.area.height {
                return;
            }
            text(buf, row(self.area, self.y), line, style);
            self.y += 1;
        }
    }

    pub fn gap(&mut self) {
        if self.y > 0 {
            self.y += 1;
        }
    }
}

/// The label column for `labels`: the widest and a space, within reason.
pub(super) fn label_width(labels: &[&str]) -> u16 {
    (labels.iter().map(|label| label.width()).max().unwrap_or(0) + 2).min(LABEL_LIMIT) as u16
}

pub(super) fn draw_details(docker: &Docker, look: &Look<'_>, rect: Rect, clock: &Zoned, buf: &mut Buffer) {
    let disk = &docker.disk;
    let (Some(usage), Some(row), Some(group)) = (&disk.usage, disk.row(), disk.group()) else {
        crate::results_view::pane_frame(docker.expanded, look, rect, tr!(docker_disk_title), false, buf);
        return;
    };
    let name = match row {
        DiskRow::Kind(_) => kind_name(group.kind).to_owned(),
        DiskRow::Item(_, item) => item.label(usage),
    };
    let inner = crate::results_view::pane_frame(docker.expanded, look, rect, &name, false, buf);
    if inner.is_empty() {
        return;
    }
    let mut sheet = Sheet { area: inner, y: 0, label: 0, look };
    match row {
        DiskRow::Kind(_) => kind_details(&mut sheet, disk, usage, group, buf),
        DiskRow::Item(_, DiskItem::Volume(index)) => {
            volumes::details(&mut sheet, disk, &usage.volumes[index], DiskItem::Volume(index), clock, buf);
        }
        DiskRow::Item(_, item) => item_details(&mut sheet, usage, item, clock, buf),
    }
    sheet.gap();
    cleanup(&mut sheet, disk, group.kind, buf);
}

fn kind_details(sheet: &mut Sheet<'_>, disk: &Disk, usage: &DiskUse, group: &DiskGroup, buf: &mut Buffer) {
    let theme = sheet.look.theme;
    let totals = group.totals;
    let mut tiles = vec![(tr!(docker_disk_count), totals.count.to_string())];
    if group.kind != DiskKind::UnusedNetworks {
        tiles.push((tr!(docker_disk_size), format::size(totals.size)));
        tiles.push((tr!(docker_disk_reclaimable), format::size(totals.reclaimable)));
    }
    sheet.y += super::tiles(buf, sheet.area, &tiles, theme);
    sheet.paragraph(buf, about(group.kind), theme.styles.overlay);
    if group.kind == DiskKind::UnusedNetworks && !usage.networks.is_empty() {
        sheet.gap();
        sheet.label = label_width(&[tr!(docker_networks)]);
        sheet.fact(buf, tr!(docker_networks), usage.networks.join(", "));
    }
    let marked = disk.mark_count();
    if marked > 0 && group.kind.family().is_some() {
        sheet.gap();
        sheet.paragraph(buf, &tr!(marked_count, count = marked), dim(theme));
    }
}

fn item_details(sheet: &mut Sheet<'_>, usage: &DiskUse, item: DiskItem, clock: &Zoned, buf: &mut Buffer) {
    let theme = sheet.look.theme;
    let stamp = |time: Option<jiff::Timestamp>| format::stamp(time.map(SystemTime::from), clock);
    let frees = (tr!(docker_disk_reclaimable), size(item.frees(usage)));
    let facts: Vec<(&str, String)> = match item {
        DiskItem::Image(index) => {
            let image = &usage.images[index];
            vec![
                (tr!(docker_disk_tags), image.tags.join(", ")),
                (
                    tr!(docker_image),
                    if image.tags.is_empty() { image.repository.clone() } else { None }.unwrap_or_default(),
                ),
                (tr!(docker_disk_id), image.short_id().to_owned()),
                (tr!(docker_disk_size), size(image.size)),
                (tr!(docker_disk_shared), size(image.shared)),
                frees,
                (tr!(docker_created), stamp(image.created)),
                (tr!(docker_containers_tile), image.containers.map(|count| count.to_string()).unwrap_or_default()),
            ]
        }
        DiskItem::Container(index) => {
            let layer = &usage.containers[index];
            vec![
                (tr!(docker_image), layer.image.clone()),
                (tr!(docker_status), layer.status.clone()),
                (tr!(docker_disk_size), size(layer.size)),
                (tr!(docker_disk_id), layer.id.get(..SHORT_ID).unwrap_or(&layer.id).to_owned()),
            ]
        }
        DiskItem::Volume(_) => Vec::new(),
        DiskItem::Cache(index) => {
            let record = &usage.cache[index];
            let yes = |value: bool| if value { tr!(docker_disk_yes) } else { tr!(docker_disk_no) }.to_owned();
            vec![
                (tr!(docker_disk_type), record.kind.clone()),
                (tr!(docker_disk_size), size(record.size)),
                (tr!(docker_disk_shared), yes(record.shared)),
                (tr!(docker_disk_in_use_label), yes(record.in_use)),
                frees,
                (tr!(docker_disk_last_used), stamp(record.last_used)),
                (tr!(docker_disk_id), record.id.get(..SHORT_ID).unwrap_or(&record.id).to_owned()),
            ]
        }
    };
    sheet.label = label_width(&facts.iter().map(|(label, _)| *label).collect::<Vec<_>>());
    for (label, value) in facts {
        sheet.fact(buf, label, value);
    }
    if let DiskItem::Cache(index) = item
        && !usage.cache[index].description.is_empty()
    {
        sheet.gap();
        sheet.paragraph(buf, &usage.cache[index].description, theme.styles.overlay);
    }
}

/// The command F8 types for the selected row, or why it types none, and the warning that
/// removed volumes take their data along.
fn cleanup(sheet: &mut Sheet<'_>, disk: &Disk, kind: DiskKind, buf: &mut Buffer) {
    let theme = sheet.look.theme;
    match disk.cleanup() {
        Ok(line) => {
            sheet.paragraph(buf, tr!(docker_disk_types), dim(theme));
            sheet.paragraph(buf, &String::from_utf8_lossy(&line.line()), emphasis(theme));
            if kind.holds_data() {
                sheet.gap();
                sheet.paragraph(buf, tr!(docker_disk_volume_data), theme.styles.notice);
            }
        }
        Err(NoCleanup::InUse) => sheet.paragraph(buf, tr!(docker_disk_in_use), dim(theme)),
        Err(NoCleanup::CacheRecord) => sheet.paragraph(buf, tr!(docker_disk_cache_together), dim(theme)),
        Err(NoCleanup::ControlCharacter) => sheet.paragraph(buf, tr!(docker_disk_control_character), dim(theme)),
        Err(NoCleanup::Empty) => {}
    }
}

pub(super) fn buttons(disk: &Disk) -> Vec<(&'static str, KeyCode)> {
    if disk.filtering {
        return vec![
            (tr!(activity_filter_keep_button), KeyCode::Enter),
            (tr!(activity_filter_clear_button), KeyCode::Esc),
        ];
    }
    let files = disk.volume().map(|_| (tr!(docker_disk_files_button), KeyCode::Enter));
    files
        .into_iter()
        .chain([
            (tr!(docker_clean_button), KeyCode::F(8)),
            (tr!(ins_mark), KeyCode::Insert),
            (tr!(docker_refresh_button), KeyCode::Char('r')),
            (tr!(activity_filter_button), KeyCode::Char('/')),
            (tr!(docker_containers_button), KeyCode::Char('d')),
            (tr!(activity_back_button), KeyCode::Esc),
        ])
        .collect()
}

pub(super) fn hint(disk: &Disk) -> &'static str {
    if disk.filtering { tr!(activity_filter_hint) } else { tr!(docker_disk_hint) }
}

/// The status line: the selected kind, or the item with what tells it apart.
pub(super) fn status(disk: &Disk) -> String {
    let (Some(usage), Some(row), Some(group)) = (&disk.usage, disk.row(), disk.group()) else { return String::new() };
    let DiskRow::Item(_, item) = row else { return kind_name(group.kind).into() };
    match item {
        DiskItem::Image(index) => {
            let image = &usage.images[index];
            format!("{} · {}", image.label(), image.short_id())
        }
        DiskItem::Container(index) => {
            let layer = &usage.containers[index];
            format!("{} · {} · {}", layer.name, layer.image, layer.status)
        }
        DiskItem::Volume(index) => usage.volumes[index].name.clone(),
        DiskItem::Cache(index) => {
            let record = &usage.cache[index];
            format!("{} · {}", record.kind, record.id.get(..SHORT_ID).unwrap_or(&record.id))
        }
    }
}
