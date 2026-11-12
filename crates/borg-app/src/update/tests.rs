use super::*;

const DAY: Duration = CHECK_EVERY;

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn version(text: &str) -> Version {
    text.parse().unwrap()
}

#[test]
fn the_check_reads_the_time_and_the_skipped_version_a_line_each() {
    let check = UpdateCheck { checked: Some(at(1_800_000_000)), skipped: Some(version("0.11.0")) };
    assert_eq!(check.text(), "1800000000\n0.11.0\n");
    assert_eq!(UpdateCheck::parse(&check.text()), check);
    let unskipped = UpdateCheck { skipped: None, ..check };
    assert_eq!(unskipped.text(), "1800000000\n");
    assert_eq!(UpdateCheck::parse("1800000000"), unskipped, "the file an older Borg wrote");
    assert_eq!(UpdateCheck::parse("garbage\nnot a version\n"), UpdateCheck::default());
}

#[test]
fn a_check_is_due_once_a_day_and_when_the_clock_went_back() {
    let now = at(1_800_000_000);
    assert!(UpdateCheck::default().due(now), "never checked");
    let check = UpdateCheck { checked: Some(now), skipped: None };
    assert!(!check.due(now + DAY / 2));
    assert!(check.due(now + DAY));
    assert!(check.due(now - DAY), "a clock set back does not stop the check");
}

#[test]
fn every_release_but_the_skipped_one_is_offered() {
    let check = UpdateCheck { checked: None, skipped: Some(version("0.11.0")) };
    assert!(!check.offers(version("0.11.0")));
    assert!(check.offers(version("0.11.1")), "a newer release is offered again");
    assert!(UpdateCheck::default().offers(version("0.11.0")));
}
