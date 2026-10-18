use std::time::SystemTime;

use jiff::{Timestamp, Zoned, tz::TimeZone};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const UNITS: [&str; 6] = ["K", "M", "G", "T", "P", "E"];
const STEP: f64 = 1024.0;
const EXACT_LIMIT: u64 = 10_000;
const RECENT_FORMAT: &str = "%b %e %H:%M";
const OLD_FORMAT: &str = "%b %e  %Y";
const ELLIPSIS: char = '…';

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

pub fn modified(time: Option<SystemTime>, now: &Zoned) -> String {
    let Some(stamp) = time.and_then(|t| Timestamp::try_from(t).ok()) else { return String::new() };
    let local = stamp.to_zoned(now.time_zone().clone());
    let format = if local.year() == now.year() { RECENT_FORMAT } else { OLD_FORMAT };
    local.strftime(format).to_string()
}

pub fn count(n: usize, noun: &str) -> String {
    if n == 1 { format!("{n} {noun}") } else { format!("{n} {noun}s") }
}

pub fn marked_summary(marked: usize, files: usize, bytes: u64, folders: usize) -> String {
    let mut parts = vec![format!("{marked} marked")];
    if files > 0 {
        parts.push(format!("{}, {}", count(files, "file"), size(bytes)));
    }
    if folders > 0 {
        parts.push(count(folders, "folder"));
    }
    parts.join(" · ")
}

pub fn now() -> Zoned {
    Zoned::now().with_time_zone(TimeZone::system())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes_compactly() {
        assert_eq!(size(512), "512");
        assert_eq!(size(9_999), "9999");
        assert_eq!(size(10_240), "10K");
        assert_eq!(size(1_572_864), "1.5M");
        assert_eq!(size(5 * 1024 * 1024 * 1024), "5.0G");
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
}
