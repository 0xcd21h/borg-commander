//! The files a process holds open, and the details and preview of one.

use super::{Page, dim, emphasis, heading, metrics, row, size, sized_line, sizes, text};
use crate::{
    format::{self, Count},
    look::Look,
    spinner::Spinner,
};
use borg_core::{Activity, ActivityPage, FILE_PREVIEW_TOP, MemoryTotals, ProcessFile};
use borg_i18n::tr;
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

pub(super) fn file_preview(activity: &Activity, first: usize, look: &Look<'_>, body: Rect, buf: &mut Buffer) {
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

pub(super) fn files(
    activity: &Activity,
    look: &Look<'_>,
    inner: Rect,
    body: Rect,
    spinner: Option<Spinner>,
    buf: &mut Buffer,
) {
    let theme = look.theme;
    text(buf, row(inner, 2), tr!(process_files_header), theme.styles.menu);
    let top = activity.list_start(ActivityPage::Files, usize::from(body.height));
    for (offset, file) in activity.files.iter().skip(top).take(usize::from(body.height)).enumerate() {
        let style = if offset + top == activity.file_cursor { theme.styles.cursor } else { theme.styles.overlay };
        let line = row(body, offset as u16);
        buf.set_style(line, style);
        text(buf, line, format!("{:<5} {:<3} {:<6} {}", file.descriptor, file.access, file.kind, file.name), style);
    }
    if activity.files.is_empty() {
        if activity.pending.is_none() {
            text(buf, row(body, 0), tr!(process_files_empty), dim(theme));
        } else if let Some(spinner) = spinner {
            spinner.line(tr!(activity_loading), look, dim(theme)).render(row(body, 0), buf);
        }
    }
}

pub(super) fn file_details(activity: &Activity, look: &Look<'_>, body: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let Some(file) = activity.files.get(activity.file_cursor) else {
        return;
    };
    if activity.previewable_file().is_some() {
        return open_file(activity, file, look, body, buf);
    }
    let mut y = 0;
    for (label, value) in [
        (tr!(process_file_path), file.name.clone()),
        (tr!(process_file_descriptor), file.descriptor.clone()),
        (tr!(process_file_access), file.access.clone()),
        (tr!(process_file_kind), file.kind.clone()),
        (tr!(process_file_size), size(file.size)),
        (tr!(process_file_offset), file.offset.map_or_else(|| "—".into(), |v| v.to_string())),
    ] {
        text(buf, row(body, y), label, emphasis(theme));
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

/// A regular file: its details in a few rows, then what it holds.
pub(super) fn open_file(activity: &Activity, file: &ProcessFile, look: &Look<'_>, body: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let width = usize::from(body.width).max(1);
    // Two rows of path; a longer one keeps its end, which names the file.
    let path: Vec<char> = format::truncate_start(&file.name, width * 2).chars().collect();
    for (y, chunk) in path.chunks(width).enumerate() {
        text(buf, row(body, y as u16), chunk.iter().collect::<String>(), emphasis(theme));
    }
    let facts = tr!(
        process_file_facts,
        descriptor = file.descriptor,
        access = file.access,
        kind = file.kind,
        size = size(file.size),
        offset = file.offset.map_or_else(|| "—".into(), |v| v.to_string())
    );
    text(buf, row(body, 2), facts, theme.styles.overlay);
    text(buf, row(body, FILE_PREVIEW_TOP - 1), tr!(process_file_preview), heading(theme));
    let preview = Rect {
        y: body.y.saturating_add(FILE_PREVIEW_TOP).min(body.bottom()),
        height: body.height.saturating_sub(FILE_PREVIEW_TOP),
        ..body
    };
    if let Some(peeked) = &activity.file_preview {
        crate::aside_view::draw_file(&peeked.peek, look, theme.styles.overlay, preview, buf);
    }
}
