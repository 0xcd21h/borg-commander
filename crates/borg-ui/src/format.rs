use borg_drives::{DriveKind, Facts};
use borg_i18n::tr;
use std::{
    path::Path,
    time::{Duration, SystemTime},
};

use jiff::{Timestamp, Zoned, tz::TimeZone};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const UNITS: [&str; 6] = ["K", "M", "G", "T", "P", "E"];
const STEP: f64 = 1024.0;
const EXACT_LIMIT: u64 = 10_000;
const RECENT_FORMAT: &str = "%b %e %H:%M";
const OLD_FORMAT: &str = "%b %e  %Y";
const STAMP_FORMAT: &str = "%Y-%m-%d %H:%M";
const DAY_FORMAT: &str = "%Y-%m-%d";
const EXACT_STAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S %Z";
const ELLIPSIS: char = '…';
const HOME_MARK: &str = "~";
const ROOT_MARK: &str = "/";
const MINUTE: u64 = 60;
const HOUR: u64 = 3600;
/// A span reads in a smaller unit up to this many of it: 89 seconds, then 2 minutes.
const SPAN_STEP: u64 = 90;
const FACT_GAP: &str = " · ";

pub fn size(bytes: u64) -> String {
    if bytes < EXACT_LIMIT {
        return bytes.to_string();
    }
    let mut value = bytes as f64 / STEP;
    let mut unit = 0;
    while value >= STEP && unit + 1 < UNITS.len() {
        value /= STEP;
        unit += 1;
    }
    let precision = usize::from(value < 10.0);
    format!("{value:.precision$}{}", UNITS[unit])
}

/// Bytes a second, or none.
/// Bits per byte, to two decimals at most: 7.94, 7.2, 4.
pub fn bits(entropy: borg_view::entropy::Entropy) -> String {
    let shown = format!("{:.2}", entropy.bits());
    shown.trim_end_matches('0').trim_end_matches('.').to_owned()
}

pub fn rate(bytes: u64) -> String {
    if bytes == 0 { size(0) } else { tr!(network_rate, size = size(bytes)) }
}

pub fn modified(time: Option<SystemTime>, now: &Zoned) -> String {
    let Some(stamp) = time.and_then(|t| Timestamp::try_from(t).ok()) else { return String::new() };
    let local = stamp.to_zoned(now.time_zone().clone());
    let format = if local.year() == now.year() { RECENT_FORMAT } else { OLD_FORMAT };
    local.strftime(format).to_string()
}

/// A time to the minute with its year, in `now`'s time zone, for lists that sort by time.
pub fn stamp(time: Option<SystemTime>, now: &Zoned) -> String {
    zoned(time, now).map_or_else(String::new, |local| local.strftime(STAMP_FORMAT).to_string())
}

/// The day of a time, in the zone of `now`.
pub fn day(time: Option<SystemTime>, now: &Zoned) -> String {
    zoned(time, now).map_or_else(String::new, |local| local.strftime(DAY_FORMAT).to_string())
}

/// A time to the second with its zone.
pub fn exact_stamp(time: Option<SystemTime>, now: &Zoned) -> String {
    zoned(time, now).map_or_else(String::new, |local| local.strftime(EXACT_STAMP_FORMAT).to_string())
}

fn zoned(time: Option<SystemTime>, now: &Zoned) -> Option<Zoned> {
    let stamp = time.and_then(|t| Timestamp::try_from(t).ok())?;
    Some(stamp.to_zoned(now.time_zone().clone()))
}

pub use borg_i18n::{Count, count};

/// A span of time in the unit that suits it, rounded: 45 seconds, 3 minutes, 2 hours.
pub fn span(duration: Duration) -> String {
    let seconds = duration.as_secs_f64().ceil() as u64;
    let rounded = |unit: u64| (seconds + unit / 2) / unit;
    let (amount, unit) = if seconds < SPAN_STEP {
        (seconds.max(1), Count::Second)
    } else if seconds < SPAN_STEP * MINUTE {
        (rounded(MINUTE), Count::Minute)
    } else {
        (rounded(HOUR), Count::Hour)
    };
    count(usize::try_from(amount).unwrap_or(usize::MAX), unit)
}

pub fn drive(kind: DriveKind) -> &'static str {
    match kind {
        DriveKind::HardDrive => tr!(drive_hard_drive),
        DriveKind::SolidState => tr!(drive_solid_state),
        DriveKind::Memory => tr!(drive_memory),
        DriveKind::Network => tr!(drive_network),
        DriveKind::Remote => tr!(drive_remote),
        DriveKind::Unknown => tr!(drive_unknown),
    }
}

/// Where work happens, as the end of a sentence: "on a hard drive".
pub fn on_drive(kind: DriveKind) -> &'static str {
    match kind {
        DriveKind::HardDrive => tr!(slow_on_hard_drive),
        DriveKind::SolidState => tr!(slow_on_solid_state),
        DriveKind::Memory => tr!(slow_on_memory),
        DriveKind::Network => tr!(slow_on_network),
        DriveKind::Remote => tr!(slow_on_remote),
        DriveKind::Unknown => tr!(slow_on_unknown),
    }
}

/// The drive's kind and filesystem, and whether Borg measured its speed.
pub fn drive_facts(facts: &Facts) -> String {
    let source = if facts.measured { tr!(drive_measured) } else { tr!(drive_typical) };
    [drive(facts.drive.kind), &facts.drive.filesystem, source]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(FACT_GAP)
}

pub fn drive_speeds(facts: &Facts) -> String {
    tr!(drive_speeds, read = size(facts.rates.read), write = size(facts.rates.write))
}

pub fn marked_summary(marked: usize, files: usize, bytes: u64, folders: usize) -> String {
    let mut parts = vec![tr!(marked_total, marked = marked)];
    if files > 0 {
        parts.push(format!("{}, {}", count(files, Count::File), size(bytes)));
    }
    if folders > 0 {
        parts.push(count(folders, Count::Folder));
    }
    parts.join(" · ")
}

pub fn now() -> Zoned {
    Zoned::now().with_time_zone(TimeZone::system())
}

pub fn pad_end(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(text.width())))
}

pub fn pad_start(text: &str, width: usize) -> String {
    format!("{}{text}", " ".repeat(width.saturating_sub(text.width())))
}

pub fn truncate_end(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let budget = width.saturating_sub(ELLIPSIS.width().unwrap_or(1));
    let mut used = 0;
    let kept: String = text
        .chars()
        .take_while(|c| {
            used += c.width().unwrap_or(0);
            used <= budget
        })
        .collect();
    format!("{kept}{ELLIPSIS}")
}

pub fn truncate_start(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let budget = width.saturating_sub(ELLIPSIS.width().unwrap_or(1));
    let mut used = 0;
    let mut kept: Vec<char> = text
        .chars()
        .rev()
        .take_while(|c| {
            used += c.width().unwrap_or(0);
            used <= budget
        })
        .collect();
    kept.reverse();
    std::iter::once(ELLIPSIS).chain(kept).collect()
}

pub fn crumbs(path: &Path, home: Option<&Path>) -> Vec<String> {
    let (head, rest) = match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => (HOME_MARK, rest),
        None => (ROOT_MARK, path.strip_prefix(ROOT_MARK).unwrap_or(path)),
    };
    std::iter::once(head.to_owned())
        .chain(rest.components().map(|part| part.as_os_str().to_string_lossy().into_owned()))
        .collect()
}

pub fn fit_crumbs(mut crumbs: Vec<String>, width: usize, separator: &str) -> Vec<String> {
    let total = |crumbs: &[String]| {
        crumbs.iter().map(|crumb| crumb.width()).sum::<usize>() + separator.width() * crumbs.len().saturating_sub(1)
    };
    let ellipsis = ELLIPSIS.to_string();
    while total(&crumbs) > width && crumbs.len() > 2 {
        match crumbs[1] == ellipsis {
            false => crumbs[1] = ellipsis.clone(),
            true if crumbs.len() > 3 => drop(crumbs.remove(2)),
            true => break,
        }
    }
    let overflow = total(&crumbs).saturating_sub(width);
    if let Some(last) = crumbs.last_mut().filter(|_| overflow > 0) {
        *last = truncate_start(last, last.width().saturating_sub(overflow));
    }
    crumbs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_uses_terminal_columns_for_wide_labels() {
        assert_eq!(pad_end("复制", 6), "复制  ");
        assert_eq!(pad_start("复制", 6), "  复制");
        assert_eq!(pad_end("Copiar", 6), "Copiar");
        assert_eq!(pad_end("Русский", 3), "Русский");
    }

    #[test]
    fn formats_sizes_compactly() {
        assert_eq!(size(512), "512");
        assert_eq!(size(9_999), "9999");
        assert_eq!(size(10_240), "10K");
        assert_eq!(size(1_572_864), "1.5M");
        assert_eq!(size(5 * 1024 * 1024 * 1024), "5.0G");
    }

    #[test]
    fn spans_read_in_the_unit_that_suits_them() {
        assert_eq!(span(Duration::from_millis(300)), "1 second");
        assert_eq!(span(Duration::from_secs(45)), "45 seconds");
        assert_eq!(span(Duration::from_secs(90)), "2 minutes");
        assert_eq!(span(Duration::from_secs(61 * 60)), "61 minutes");
        assert_eq!(span(Duration::from_secs(5 * 3600)), "5 hours");
    }

    #[test]
    fn summarises_marked_entries() {
        assert_eq!(marked_summary(2, 0, 0, 2), "2 marked · 2 folders");
        assert_eq!(marked_summary(3, 2, 12_288, 1), "3 marked · 2 files, 12K · 1 folder");
        assert_eq!(marked_summary(1, 1, 310, 0), "1 marked · 1 file, 310");
    }

    #[test]
    fn truncates_by_display_width() {
        assert_eq!(truncate_end("abcdef", 4), "abc…");
        assert_eq!(truncate_end("abc", 4), "abc");
        assert_eq!(truncate_start("/home/user/projects", 10), "…/projects");
        assert_eq!(truncate_end("日本語テキスト", 5), "日本…");
    }

    #[test]
    fn builds_breadcrumbs_relative_to_home() {
        let home = Path::new("/home/me");
        assert_eq!(crumbs(Path::new("/home/me/projects/borg"), Some(home)), ["~", "projects", "borg"]);
        assert_eq!(crumbs(Path::new("/home/me"), Some(home)), ["~"]);
        assert_eq!(crumbs(Path::new("/usr/lib"), Some(home)), ["/", "usr", "lib"]);
        assert_eq!(crumbs(Path::new("/"), None), ["/"]);
    }

    #[test]
    fn collapses_breadcrumbs_from_the_middle() {
        let all = crumbs(Path::new("/home/me/a/bbbb/cccc/dddd"), Some(Path::new("/home/me")));
        assert_eq!(fit_crumbs(all.clone(), 40, " › "), all);
        assert_eq!(fit_crumbs(all.clone(), 19, " › "), ["~", "…", "cccc", "dddd"]);
        assert_eq!(fit_crumbs(all.clone(), 16, " › "), ["~", "…", "dddd"]);
        assert_eq!(fit_crumbs(all, 10, " › "), ["~", "…", "…d"]);
    }
}
