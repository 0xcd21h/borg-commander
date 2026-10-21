use borg_core::{Panel, Row};
use borg_i18n::tr;
use borg_vfs::{Entry, EntryKind};
use jiff::{Timestamp, Zoned};
use ratatui::{buffer::Buffer, layout::Rect, style::Style, text::Line, widgets::Widget};

use crate::{format, theme::Styles};

const PERMISSION_BITS: [(u32, char); 9] = [
    (0o400, 'r'),
    (0o200, 'w'),
    (0o100, 'x'),
    (0o040, 'r'),
    (0o020, 'w'),
    (0o010, 'x'),
    (0o004, 'r'),
    (0o002, 'w'),
    (0o001, 'x'),
];
const UNSET: char = '-';
const FULL_DATE: &str = "%Y-%m-%d %H:%M";
const PARENT_TEXT: &str = tr!(up_to_the_parent_directory);

pub fn draw_status(panel: &Panel, area: Rect, styles: &Styles<Style>, clock: &Zoned, buf: &mut Buffer) {
    buf.set_style(area, styles.status);
    if panel.is_results() {
        Line::styled(
            format!(" {}", crate::results_view::status(panel, usize::from(area.width.saturating_sub(1)), clock)),
            styles.status,
        )
        .render(area, buf);
        return;
    }
    let text = match (panel.marked_count(), panel.hovered()) {
        (0, Some(Row::Entry(entry))) => describe(entry, clock),
        (0, Some(Row::Group { .. })) => String::new(),
        (0, Some(Row::Parent)) => PARENT_TEXT.to_owned(),
        (0, None) => String::new(),
        (marked, _) => {
            let (dirs, files): (Vec<&Entry>, Vec<&Entry>) = panel.marked().partition(|e| e.is_dir());
            let bytes: u64 = files.iter().map(|e| e.size).sum();
            format::marked_summary(marked, files.len(), bytes, dirs.len())
        }
    };
    Line::styled(format!(" {text}"), styles.status).render(area, buf);
}

fn describe(entry: &Entry, clock: &Zoned) -> String {
    let size = if entry.is_dir() && entry.usage.is_none() {
        String::from(tr!(dir))
    } else {
        format!(
            "{}{}{}",
            if entry.usage.as_ref().is_some_and(|u| u.cached) { "~" } else { "" },
            if entry.usage.as_ref().is_some_and(|u| u.partial) { "≥" } else { "" },
            format::size(entry.size)
        )
    };
    let modified = entry
        .modified
        .and_then(|t| Timestamp::try_from(t).ok())
        .map(|t| t.to_zoned(clock.time_zone().clone()).strftime(FULL_DATE).to_string())
        .unwrap_or_default();
    let cached = entry.usage.as_ref().filter(|u| u.measured.is_some()).map_or_else(String::new, |u| {
        let time = format::modified(u.measured, clock);
        format!(tr!(sizes_cached_status), time = time)
    });
    format!("{}  {size:>6}  {modified}  {}{cached}", permissions(entry), entry.name)
}

pub fn permissions(entry: &Entry) -> String {
    let kind = match entry.kind {
        EntryKind::Dir => 'd',
        EntryKind::Symlink { .. } => 'l',
        EntryKind::Other => 's',
        EntryKind::File => UNSET,
    };
    std::iter::once(kind).chain(rwx(entry.permissions).chars()).collect()
}

pub fn rwx(bits: u32) -> String {
    PERMISSION_BITS.iter().map(|(bit, c)| if bits & bit != 0 { *c } else { UNSET }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_unix_permission_strings() {
        let entry = |kind, permissions| Entry {
            source: None,
            missing: false,
            usage: None,
            name: String::new(),
            kind,
            size: 0,
            modified: None,
            executable: false,
            permissions,
        };
        assert_eq!(permissions(&entry(EntryKind::Dir, 0o755)), "drwxr-xr-x");
        assert_eq!(permissions(&entry(EntryKind::File, 0o640)), "-rw-r-----");
        assert_eq!(permissions(&entry(EntryKind::Symlink { to_dir: false }, 0o777)), "lrwxrwxrwx");
    }
}
