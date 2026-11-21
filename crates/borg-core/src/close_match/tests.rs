use super::*;

fn rank(typed: &str, name: &str) -> Option<MatchRank> {
    let needle = Needle::new(typed, true);
    let found = needle.rank(name, true);
    let quick = needle.found_in(name) || needle.close_to(name);
    assert_eq!(found.is_some(), quick, "the quick check and the full one disagree on {typed:?} in {name:?}");
    found
}

fn tier(typed: &str, name: &str) -> Option<Tier> {
    rank(typed, name).map(|rank| rank.tier)
}

fn edits(typed: &str, name: &str) -> Option<u8> {
    rank(typed, name).filter(|rank| rank.tier == Tier::Typo).map(|rank| rank.edits)
}

fn marked(typed: &str, name: &str) -> Vec<String> {
    let (_, spans) = Needle::new(typed, true).locate(name, true).unwrap();
    spans.into_iter().map(|span| name[span].to_owned()).collect()
}

#[test]
fn finds_names_as_typed_first_then_by_where_the_word_starts() {
    assert_eq!(tier("budget", "budget.xlsx"), Some(Tier::Exact));
    assert_eq!(tier("budget", "budget"), Some(Tier::Exact));
    assert_eq!(tier("budg", "Budget 2024.xlsx"), Some(Tier::Prefix));
    assert_eq!(tier("report", "2024_report_final.pdf"), Some(Tier::WordStart));
    assert_eq!(tier("parser", "HTMLParser.java"), Some(Tier::WordStart));
    assert_eq!(tier("port", "export.csv"), Some(Tier::Inside));
}

#[test]
fn allows_one_typo_from_four_letters_and_two_from_nine() {
    assert_eq!(edits("reciept", "receipt-march.pdf"), Some(1));
    assert_eq!(edits("recipt", "receipt.pdf"), Some(1));
    assert_eq!(edits("receeipt", "receipt.pdf"), Some(1));
    assert_eq!(edits("budegt", "budget.xlsx"), Some(1));
    assert_eq!(edits("vacaton", "Vacation Photos"), Some(1));
    assert_eq!(edits("mian", "main.rs"), Some(1));
    assert_eq!(edits("main", "mail.app"), Some(1));
    assert_eq!(edits("note", "nose.txt"), Some(1));
    assert_eq!(edits("reciep", "receipts-2023.pdf"), Some(1));
    assert_eq!(edits("acommodation", "accommodation-booking.pdf"), Some(1));
    assert_eq!(edits("acomodation", "accommodation.pdf"), Some(2));
    assert_eq!(edits("quartelry", "Quarterly Report.docx"), Some(1));
    assert_eq!(edits("meetingnotes", "Meeting Notes.md"), Some(0));
}

#[test]
fn finds_numbers_with_their_digits_in_another_order() {
    assert_eq!(tier("1986", "scan_8619.pdf"), Some(Tier::Shuffled));
    assert_eq!(tier("1986", "photos 1968"), Some(Tier::Shuffled));
    assert_eq!(tier("invoice8619", "Invoice 1986.pdf"), Some(Tier::Shuffled));
}

#[test]
fn finds_letters_scattered_over_the_words_of_a_name() {
    assert_eq!(tier("bdgt", "budget.xlsx"), Some(Tier::Scattered));
    assert_eq!(tier("qtrly", "quarterly-sales.csv"), Some(Tier::Scattered));
    assert_eq!(tier("mtgnotes", "meeting-notes.txt"), Some(Tier::Scattered));
}

#[test]
fn leaves_out_names_that_only_look_alike() {
    let unrelated = [
        ("cat", "chat.log"),
        ("test", "best.txt"),
        ("budget", "gadget.txt"),
        ("notes", "votes.csv"),
        ("note", "notification.log"),
        ("receipt", "recent.txt"),
        ("report", "import.rs"),
        ("report", "rapport.docx"),
        ("2024", "2023 taxes.pdf"),
        ("1986", "1987 budget.xlsx"),
        ("1986", "IMG_86190512.jpg"),
        ("1986", "198.pdf"),
        ("135", "scan_513.pdf"),
        ("abc", "a_big_cat.txt"),
        ("mtng", "monitoring.log"),
    ];
    for (typed, name) in unrelated {
        assert_eq!(rank(typed, name), None, "{typed:?} should not find {name:?}");
    }
}

#[test]
fn ranks_exact_matches_above_typos_and_typos_above_scattered_letters() {
    let names = ["mtgnotes.txt", "old-mtgnotes.txt", "mtgnote.txt", "meeting-notes.txt"];
    let ranks = names.map(|name| rank("mtgnotes", name));
    assert!(ranks.iter().all(Option::is_some), "{ranks:?}");
    assert!(ranks.windows(2).all(|pair| pair[0] < pair[1]), "{ranks:?}");
    assert!(rank("reciept", "reciept.pdf") < rank("reciept", "receipt.pdf"));
    assert!(rank("acomodation", "accomodation.pdf") < rank("acomodation", "accommodation.pdf"));
}

#[test]
fn a_capital_keeps_case_and_accents_are_ignored_in_close_matches() {
    assert_eq!(rank("README", "readme.md"), None);
    assert_eq!(tier("Vacaton", "Vacation Photos"), Some(Tier::Typo));
    assert_eq!(rank("Vacaton", "vacation photos"), None);
    assert_eq!(tier("resume", "Résumé.pdf"), Some(Tier::Typo));
    assert_eq!(tier("resume", "Re\u{301}sume\u{301}.pdf"), Some(Tier::Typo));
    assert_eq!(tier("результат", "Результаты.txt"), Some(Tier::Prefix));
    assert_eq!(edits("резултат", "результат.txt"), Some(1));
}

#[test]
fn scripts_without_spelling_only_match_exactly() {
    assert_eq!(tier("报告", "年度报告.pdf"), Some(Tier::Inside));
    assert_eq!(rank("报吿", "年度报告.pdf"), None);
    assert!(!Needle::new("報告書", true).can_be_close());
}

#[test]
fn close_matching_waits_to_be_allowed() {
    let needle = Needle::new("reciept", false);
    assert!(!needle.can_be_close() && !needle.close_to("receipt.pdf"));
    assert_eq!(needle.rank("receipt.pdf", true), None);
    assert_eq!(Needle::new("reciept", true).rank("receipt.pdf", false), None);
}

#[test]
fn marks_the_letters_that_matched() {
    assert_eq!(marked("report", "2024_report_final.pdf"), ["report"]);
    assert_eq!(marked("reciept", "receipt-march.pdf"), ["receipt"]);
    assert_eq!(marked("1986", "scan_8619.pdf"), ["8619"]);
    assert_eq!(marked("bdgt", "budget.xlsx"), ["b", "dg", "t"]);
    assert_eq!(marked("meetingnotes", "Meeting Notes.md"), ["Meeting Notes"]);
    assert_eq!(marked("invoice8619", "Invoice 1986.pdf"), ["Invoice", "1986"]);
    assert_eq!(marked("resume", "Résumé.pdf"), ["Résumé"]);
}

#[test]
fn short_words_and_mixed_words_under_four_letters_stay_exact() {
    assert_eq!(rank("mp3", "MP-3 notes.txt"), None);
    assert_eq!(rank("v2", "video_2.mp4"), None);
    assert_eq!(tier("v2", "draft v2.md"), Some(Tier::WordStart));
}
