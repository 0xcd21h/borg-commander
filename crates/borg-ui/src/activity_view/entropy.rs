//! The entropy scan of every process: the dialog that picks what it reads, its hits ranked
//! with packed code first, and the selected hit's details.

use super::choices::Choices;
use super::{displayable, emphasis, row, text, wrap};
use crate::{Theme, format};
use borg_core::{Activity, ActivityPage, EntropyChoice, EntropyDialog, EntropyHit, EntropyScan, HitKind};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

const KEY_COLUMN: usize = 3;
const LABEL_GAP: usize = 2;
/// Cells the share of dense pages and the densest page take in a hit's row.
const SHARE_COLUMN: usize = 24;

fn label(choice: EntropyChoice) -> &'static str {
    match choice {
        EntropyChoice::Scan(scope) => scope.label(),
        EntropyChoice::Results => tr!(entropy_results_choice),
    }
}

fn about(choice: EntropyChoice) -> &'static str {
    match choice {
        EntropyChoice::Scan(borg_core::EntropyScope::Code) => tr!(entropy_scope_code_about),
        EntropyChoice::Scan(borg_core::EntropyScope::Memory) => tr!(entropy_scope_memory_about),
        EntropyChoice::Results => tr!(entropy_results_about),
    }
}

fn choices(dialog: &EntropyDialog) -> Choices<'static> {
    let all = dialog.choices();
    let widest = all.iter().map(|&choice| label(choice).width()).max().unwrap_or_default();
    let rows = all
        .into_iter()
        .map(|choice| {
            let key = choice.key().to_ascii_uppercase().to_string();
            let head =
                format!("{}{}", format::pad_end(&key, KEY_COLUMN), format::pad_end(label(choice), widest + LABEL_GAP));
            (head, about(choice).to_owned(), true)
        })
        .collect();
    Choices {
        title: format!(" {} ", tr!(entropy_title)),
        subject: tr!(entropy_dialog_subject).into(),
        keys: tr!(entropy_dialog_keys),
        rows,
        selected: dialog.selected,
    }
}

/// The choice's row under `point`, when it is on one.
pub(crate) fn choice_at(dialog: &EntropyDialog, area: Rect, point: Position) -> (Rect, Option<usize>) {
    super::choices::choice_at(&choices(dialog), area, point)
}

pub(crate) fn draw_dialog(dialog: &EntropyDialog, theme: &Theme, area: Rect, buf: &mut Buffer) -> Rect {
    super::choices::draw(&choices(dialog), theme, area, buf)
}

/// The title of the hits: the scan and what it reads.
pub(super) fn title(activity: &Activity) -> String {
    match &activity.entropy {
        Some(scan) => format!("{} · {}", tr!(entropy_title), scan.scope.label()),
        None => tr!(entropy_title).into(),
    }
}

/// Where the scan stands: whether it runs, and how many processes it read and hits it found.
pub(super) fn summary(scan: &EntropyScan) -> String {
    let state = if scan.running {
        tr!(entropy_running)
    } else if scan.report.remaining.is_empty() {
        tr!(entropy_finished)
    } else {
        tr!(entropy_stopped)
    };
    let report = &scan.report;
    tr!(
        entropy_summary,
        state = state,
        scanned = report.scanned,
        total = scan.total,
        hits = report.hits.len() + report.dropped
    )
}

/// What the scan left out: processes it could not read or did not reach, and weaker hits.
pub(super) fn left_out(scan: &EntropyScan) -> String {
    let report = &scan.report;
    let left = tr!(
        entropy_left_out,
        unreadable = report.unreadable.len(),
        protected = report.protected,
        remaining = report.remaining.len()
    );
    if report.dropped == 0 { left } else { format!("{left} · {}", tr!(entropy_dropped, dropped = report.dropped)) }
}

/// The share of a hit's sampled pages that are dense, and its densest page.
fn share(hit: &EntropyHit) -> String {
    tr!(packed_match, share = format::percent(hit.tally.share()), bits = format::bits(hit.tally.highest))
}

pub(super) fn hits(activity: &Activity, theme: &Theme, body: Rect, buf: &mut Buffer) {
    let Some(scan) = &activity.entropy else { return };
    let hits = &scan.report.hits;
    if hits.is_empty() {
        if !scan.running {
            text(buf, row(body, 0), tr!(entropy_none), theme.styles.overlay);
        }
        return;
    }
    let kind_width = [HitKind::Code, HitKind::Own, HitKind::File].map(|kind| kind.label().width()).into_iter().max();
    let kind_width = kind_width.unwrap_or_default() + 1;
    let start = activity.list_start(ActivityPage::Entropy, usize::from(body.height));
    for (i, hit) in hits.iter().enumerate().skip(start).take(usize::from(body.height)) {
        let line = row(body, (i - start) as u16);
        let selected = i == scan.cursor;
        let style = if selected { theme.styles.cursor } else { theme.styles.overlay };
        let mark = if selected {
            style
        } else if hit.packed() {
            theme.styles.notice.add_modifier(Modifier::BOLD)
        } else if hit.kind == HitKind::Code {
            emphasis(theme)
        } else {
            style
        };
        buf.set_style(line, style);
        let place = format!("{} · {} · {}", hit.name, hit.process.pid, hit.region.name);
        Line::from(vec![
            Span::styled(format::pad_end(hit.kind.label(), kind_width), mark),
            Span::styled(format::pad_end(&share(hit), SHARE_COLUMN), mark),
            Span::styled(displayable(&place), style),
        ])
        .render(line, buf);
    }
}

/// The selected hit's process, as the details' first row.
pub(super) fn process_line(activity: &Activity) -> String {
    let Some(hit) = activity.entropy.as_ref().and_then(EntropyScan::selected) else { return String::new() };
    let executable = activity
        .sample
        .processes
        .iter()
        .find(|process| process.key == hit.process)
        .and_then(|process| process.executable.as_ref())
        .map_or_else(String::new, |path| path.display().to_string());
    tr!(activity_process_label, name = hit.name, pid = hit.process.pid, executable = executable)
}

/// The selected hit's region, its pages, and what dense bytes there tend to mean.
pub(super) fn details(activity: &Activity, theme: &Theme, body: Rect, buf: &mut Buffer) {
    let Some(hit) = activity.entropy.as_ref().and_then(EntropyScan::selected) else { return };
    let region = &hit.region;
    let tally = &hit.tally;
    let width = usize::from(body.width).max(1);
    let range = format!(
        "{:016x}-{:016x} · {} · {}",
        region.start,
        region.end,
        format::size(region.mapped()),
        region.permissions
    );
    let pages = tr!(entropy_hit_pages, high = tally.high, blocks = tally.blocks, bits = format::bits(tally.highest));
    let first = tally.first_high.map(|address| tr!(entropy_hit_first, address = format!("{address:016x}")));
    let about = match hit.kind {
        _ if hit.packed() => tr!(entropy_about_packed),
        HitKind::Code => tr!(entropy_about_code),
        HitKind::Own => tr!(entropy_about_own),
        HitKind::File => tr!(entropy_about_file),
    };
    let heading = format!("{} · {}", hit.kind.label(), share(hit));
    let facts = [heading, range].into_iter().map(|line| (line, emphasis(theme)));
    let counts = std::iter::once(pages)
        .chain(first)
        .flat_map(|line| wrap(&line, width))
        .map(|line| (line, theme.styles.overlay));
    let note = std::iter::once(String::new()).chain(wrap(about, width)).map(|line| (line, super::dim(theme)));
    for (y, (line, style)) in facts.chain(counts).chain(note).enumerate() {
        let Ok(y) = u16::try_from(y) else { break };
        text(buf, row(body, y), line, style);
    }
}
