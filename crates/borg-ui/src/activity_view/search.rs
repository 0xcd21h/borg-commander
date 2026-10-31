//! Searching a process's memory: the form, its results and the integrity findings.

use super::{SEARCH_HEADER, dim, displayable, emphasis, row, text, wrap};
use crate::{Theme, format};
use borg_core::{Activity, ActivityPage, ActivityRequest, FINDING_NOTE, IntegrityReport, Severity};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Widget},
};
use unicode_width::UnicodeWidthStr;

/// Marks a string no compared file holds.
const MEMORY_ONLY: &str = "•";

pub(super) fn form_field(body: Rect, index: usize) -> Rect {
    Rect {
        y: body.y.saturating_add((index * 3) as u16).min(body.bottom()),
        height: body.height.saturating_sub((index * 3) as u16).min(3),
        ..body
    }
}

pub(super) fn mode_label(mode: borg_core::MemorySearchMode) -> &'static str {
    use borg_core::MemorySearchMode;
    match mode {
        MemorySearchMode::Text => tr!(memory_mode_text),
        MemorySearchMode::Hex => tr!(memory_mode_hex),
        MemorySearchMode::Unicode => tr!(memory_mode_unicode),
        MemorySearchMode::Strings => tr!(memory_mode_strings),
        MemorySearchMode::Indicators => tr!(memory_mode_indicators),
    }
}

pub(super) fn search_form(form: &borg_core::MemorySearchForm, theme: &Theme, body: Rect, buf: &mut Buffer) {
    let mode = mode_label(form.mode);
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

pub(super) fn search_results(activity: &Activity, theme: &Theme, body: Rect, buf: &mut Buffer) {
    let Some(report) = &activity.search_report else { return };
    let more = if report.cancelled {
        tr!(memory_search_cancelled)
    } else if report.next.is_some() {
        tr!(memory_search_more)
    } else {
        tr!(memory_search_complete)
    };
    let (scanned, total, skipped) =
        (format::size(report.scanned), format::size(report.total), format::size(report.skipped));
    let (summary, details) = match &report.strings {
        Some(tally) => {
            let summary = tr!(
                memory_strings_summary,
                distinct = tally.distinct,
                found = tally.found,
                scanned = scanned,
                total = total,
                skipped = skipped,
                more = more
            );
            let compared = if tally.files > 0 {
                tr!(memory_strings_compared, only = tally.memory_only, files = tally.files)
            } else {
                tr!(memory_strings_uncompared).to_owned()
            };
            let kinds = tally.kinds.iter().map(|(kind, count)| format!("{} {count}", kind.label()));
            (summary, std::iter::once(compared).chain(kinds).collect::<Vec<_>>().join(" · "))
        }
        None => {
            let summary = tr!(
                memory_search_summary,
                hits = report.hits.len(),
                scanned = scanned,
                total = total,
                skipped = skipped,
                more = more
            );
            let searched = match &activity.last_search {
                Some(ActivityRequest::Search { mode, query, .. }) => format!("{} · {query}", mode_label(*mode)),
                _ => String::new(),
            };
            (summary, searched)
        }
    };
    text(buf, row(body, 0), summary, dim(theme));
    text(buf, row(body, 1), details, dim(theme));
    let list = results_list(body, SEARCH_HEADER, 0);
    let start = activity.list_start(ActivityPage::Search, usize::from(list.height));
    for (i, hit) in report.hits.iter().enumerate().skip(start).take(usize::from(list.height)) {
        let style = if i == activity.search_cursor { theme.styles.cursor } else { theme.styles.overlay };
        let line = row(list, (i - start) as u16);
        buf.set_style(line, style);
        let shown = match &hit.text {
            Some(found) if report.strings.is_some() => {
                let copies = if hit.copies > 1 { format!("×{}", hit.copies) } else { String::new() };
                let only = if hit.in_file == Some(false) { MEMORY_ONLY } else { " " };
                format!("{:<8} {copies:>6} {only} {found}", hit.indicator.map_or("", |kind| kind.label()))
            }
            Some(found) => format!("{:<9} {:<8} {found}", hit.encoding, hit.indicator.map_or("", |kind| kind.label())),
            None => format!("{:<9} {}", hit.encoding, hit.region.name),
        };
        text(buf, line, displayable(&format!("{:016x} {shown}", hit.address)), style);
    }
}

/// The rows of a results list: below `header` summary rows, and above `note` rows that
/// explain the selected result.
pub(super) fn results_list(body: Rect, header: u16, note: u16) -> Rect {
    Rect {
        y: body.y.saturating_add(header).min(body.bottom()),
        height: body.height.saturating_sub(header + note),
        ..body
    }
}

/// Integrity findings, worst first, with what the selected one means below them.
pub(super) fn findings(activity: &Activity, report: &IntegrityReport, theme: &Theme, body: Rect, buf: &mut Buffer) {
    let summary = tr!(
        integrity_summary,
        findings = report.findings.len(),
        compared = format::size(report.compared),
        scanned = format::size(report.scanned),
        unbacked = format::size(report.unbacked),
        unreadable = format::size(report.unreadable),
        incomplete = if report.complete { "" } else { tr!(integrity_incomplete) }
    );
    text(buf, row(body, 0), summary, dim(theme));
    let list = results_list(body, 1, FINDING_NOTE);
    if report.findings.is_empty() {
        text(buf, row(list, 0), tr!(integrity_none), theme.styles.overlay);
        return;
    }
    let start = activity.list_start(ActivityPage::Integrity, usize::from(list.height));
    for (i, finding) in report.findings.iter().enumerate().skip(start).take(usize::from(list.height)) {
        let line = row(list, (i - start) as u16);
        let selected = i == activity.finding_cursor;
        let style = if selected { theme.styles.cursor } else { theme.styles.overlay };
        let severity = match finding.severity {
            _ if selected => style,
            Severity::Alert => theme.styles.notice.add_modifier(Modifier::BOLD),
            Severity::Warning => emphasis(theme),
            Severity::Notice => dim(theme),
        };
        let address = finding.address.map_or_else(String::new, |address| format!("{address:016x}"));
        buf.set_style(line, style);
        Line::from(vec![
            Span::styled(format!("{:<8}", finding.severity.label()), severity),
            Span::styled(displayable(&format!(" {address:<16} {} · {}", finding.kind.label(), finding.detail)), style),
        ])
        .render(line, buf);
    }
    let Some(finding) = activity.selected_finding() else { return };
    let note = Rect { y: list.bottom(), height: FINDING_NOTE.min(body.bottom().saturating_sub(list.bottom())), ..body };
    let width = usize::from(note.width).max(1);
    let detail = wrap(&finding.detail, width).into_iter().take(3).map(|line| (line, emphasis(theme)));
    let caveat = wrap(finding.kind.caveat(), width).into_iter().map(|line| (line, dim(theme)));
    for (y, (line, style)) in detail.chain(caveat).enumerate() {
        text(buf, row(note, y as u16), line, style);
    }
}
