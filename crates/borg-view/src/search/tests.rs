use super::*;
use proptest::prelude::*;
use std::ops::Range;

fn query(text: &str) -> Search {
    Query { text: text.into(), ..Query::default() }.compile().unwrap().unwrap()
}

fn all(search: &Search, text: &str) -> Vec<Range<u64>> {
    let mut found = Vec::new();
    search
        .each(&text.as_bytes().to_vec(), 0, text.len() as u64, &|_| true, &mut |range| {
            found.push(range);
            true
        })
        .unwrap();
    found
}

/// Every match as its start and end.
fn spans(search: &Search, text: &str) -> Vec<(u64, u64)> {
    all(search, text).into_iter().map(|range| (range.start, range.end)).collect()
}

/// Every match, line by line, as highlighting finds them.
fn by_lines(search: &Search, text: &str) -> Vec<Range<u64>> {
    let mut found = Vec::new();
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let bare = line.strip_suffix('\n').unwrap_or(line);
        found.extend(search.in_line(bare.as_bytes()).into_iter().map(|r| start + r.start as u64..start + r.end as u64));
        start += line.len() as u64;
    }
    found
}

#[test]
fn a_query_without_capitals_ignores_case_and_words_match_whole() {
    let text = "Borg borg BORG cyborg borgs\n";
    assert_eq!(all(&query("borg"), text).len(), 5);
    assert_eq!(spans(&query("Borg"), text), [(0, 4)]);
    let exact = Query { text: "borg".into(), case: true, ..Query::default() }.compile().unwrap().unwrap();
    assert_eq!(all(&exact, text).len(), 3);
    let words = Query { text: "borg".into(), words: true, ..Query::default() }.compile().unwrap().unwrap();
    assert_eq!(spans(&words, text), [(0, 4), (5, 9), (10, 14)]);
    let regex = Query { text: r"\bb\w+s\b".into(), regex: true, ..Query::default() }.compile().unwrap().unwrap();
    assert_eq!(spans(&regex, text), [(22, 27)]);
    assert!(!capitals(r"\S+\p{Lu}", true) && capitals(r"\S+A", true));
    assert_eq!(Query { text: "(".into(), regex: true, ..Query::default() }.compile().unwrap_err(), "unclosed group");
    assert!(Query::default().compile().unwrap().is_none());
}

#[test]
fn matches_stay_within_their_lines() {
    let text = "end  \n  start\nnone\n";
    let spaces = Query { text: r"\s+".into(), regex: true, ..Query::default() }.compile().unwrap().unwrap();
    assert_eq!(spans(&spaces, text), [(3, 5), (6, 8)]);
    let line_end = Query { text: "e$".into(), regex: true, ..Query::default() }.compile().unwrap().unwrap();
    assert_eq!(spans(&line_end, text), [(17, 18)]);
    let crlf = Query { text: "d$".into(), regex: true, ..Query::default() }.compile().unwrap().unwrap();
    assert_eq!(spans(&crlf, "end\r\nmore"), [(2, 3)]);
}

#[test]
fn next_and_previous_wrap_nowhere_and_regroup_replacements() {
    let text: Vec<u8> = "one two\nthree two\n".into();
    let two = query("two");
    assert_eq!(two.next(&text, 0, &|_| true).unwrap(), Some(4..7));
    assert_eq!(two.next(&text, 5, &|_| true).unwrap(), Some(14..17));
    assert_eq!(two.next(&text, 15, &|_| true).unwrap(), None);
    assert_eq!(two.previous(&text, 14, &|_| true).unwrap(), Some(4..7));
    assert_eq!(two.previous(&text, 15, &|_| true).unwrap(), Some(14..17));
    assert_eq!(two.previous(&text, 4, &|_| true).unwrap(), None);
    assert_eq!(two.count(&text, 14, &|_| true).unwrap(), Count { index: Some(2), total: 2, more: false });
    assert!(two.next(&text, 0, &|_| false).is_err(), "a stopped search says so");
    let swap = Query { text: r"(\w+) (\w+)".into(), regex: true, ..Query::default() }.compile().unwrap().unwrap();
    assert_eq!(swap.replacement(b"one two", 0..7, "$2 $1"), "two one");
}

#[test]
fn searches_cross_chunks_and_long_lines() {
    let mut text = "x".repeat(CHUNK as usize * 2 + 10);
    text.push_str("needle");
    text.push('\n');
    text.push_str(&"y\n".repeat(CHUNK as usize));
    text.push_str("needle\n");
    let source = text.clone().into_bytes();
    let needle = query("needle");
    let first = CHUNK * 2 + 10;
    assert_eq!(needle.next(&source, 0, &|_| true).unwrap(), Some(first..first + 6));
    let second = text.rfind("needle").unwrap() as u64;
    assert_eq!(needle.next(&source, first + 1, &|_| true).unwrap(), Some(second..second + 6));
    assert_eq!(needle.previous(&source, second, &|_| true).unwrap(), Some(first..first + 6));
    assert_eq!(needle.previous(&source, source.len() as u64, &|_| true).unwrap(), Some(second..second + 6));
}

proptest! {
    #[test]
    fn a_whole_search_finds_what_each_line_holds(
        text in "[ab \n]{0,64}",
        needle in prop_oneof![Just("a"), Just("ab"), Just("b a"), Just("a+"), Just(r"\s"), Just("^b"), Just("a$")],
    ) {
        let search = Query { text: needle.into(), regex: true, ..Query::default() }.compile().unwrap().unwrap();
        let expected = by_lines(&search, &text);
        prop_assert_eq!(all(&search, &text), expected.clone());
        let source = text.clone().into_bytes();
        let mut stepped = Vec::new();
        let mut from = 0;
        while let Some(range) = search.next(&source, from, &|_| true).unwrap() {
            from = range.end;
            stepped.push(range);
        }
        prop_assert_eq!(&stepped, &expected);
        let mut back = Vec::new();
        let mut before = source.len() as u64;
        while let Some(range) = search.previous(&source, before, &|_| true).unwrap() {
            before = range.start;
            back.push(range);
        }
        back.reverse();
        prop_assert_eq!(back, expected);
    }
}
