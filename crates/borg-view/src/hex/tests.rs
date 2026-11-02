use super::*;

#[test]
fn rows_hold_as_many_bytes_as_fit_and_offsets_as_many_digits_as_the_length_needs() {
    assert_eq!(HexLayout::fitting(80, 100).columns, 16);
    assert_eq!(HexLayout::fitting(71, 100).columns, 16);
    assert_eq!(HexLayout::fitting(70, 100).columns, 8);
    assert_eq!(HexLayout::fitting(30, 100).columns, 4);
    assert_eq!(HexLayout::fitting(0, 100).columns, 1);
    assert_eq!(HexLayout::fitting(80, 100).width(), 71);
    assert_eq!(HexLayout::full(0).digits, 4);
    assert_eq!(HexLayout::full(0x1_0000).digits, 4);
    assert_eq!(HexLayout::full(0x1_0001).digits, 5);
    assert_eq!(HexLayout::full(8 << 30).digits, 9);
    assert_eq!(HexLayout::fitting(79, 8 << 30).columns, 16);
    assert_eq!(HexLayout::fitting(75, 8 << 30).columns, 8, "longer offsets leave less room for bytes");
}

#[test]
fn a_row_shows_its_offset_its_bytes_in_hex_and_those_that_print() {
    let layout = HexLayout::full(0x1_0001);
    assert_eq!(layout.line(0x1_0000, b"A"), format!("10000  41{:45}  A", ""));
    let layout = HexLayout::fitting(40, 100);
    assert_eq!(layout.columns, 8);
    assert_eq!(layout.line(8, b"Hi \x00\x7f\xffok"), "0008  48 69 20 00 7f ff 6f 6b  Hi ...ok");
    assert_eq!(layout.rows(17), 3);
    assert_eq!(layout.row_start(17), 16);
}

#[test]
fn offsets_and_zero_bytes_are_faint_and_a_match_is_marked_in_both_columns() {
    let layout = HexLayout::fitting(40, 100);
    let mark = |at: u64| match at {
        1..=2 => Mark::Match,
        4 => Mark::Current,
        _ => Mark::None,
    };
    let runs = layout.runs(0, b"a\x00bcd", &mark);
    let shown: Vec<(&str, bool, Mark)> = runs.iter().map(|run| (run.text.as_str(), run.paint.dim, run.mark)).collect();
    assert_eq!(
        shown,
        [
            ("0000", true, Mark::None),
            ("  61 ", false, Mark::None),
            ("00", true, Mark::Match),
            (" 62", false, Mark::Match),
            (" 63 ", false, Mark::None),
            ("64", false, Mark::Current),
            ("           a", false, Mark::None),
            (".", true, Mark::Match),
            ("b", false, Mark::Match),
            ("c", false, Mark::None),
            ("d", false, Mark::Current),
        ]
    );
}

#[test]
fn a_dump_gives_every_row_and_a_narrow_one_keeps_the_text_beside_fewer_bytes() {
    let data = b"GET / HTTP/1.1\r\nHost";
    let layout = HexLayout::full(data.len() as u64);
    let lines = layout.lines(data);
    assert_eq!(
        lines,
        [
            "0000  47 45 54 20 2f 20 48 54 54 50 2f 31 2e 31 0d 0a  GET / HTTP/1.1..",
            &format!("0010  {:<47}  Host", "48 6f 73 74")
        ]
    );
    assert_eq!(lines[0].len(), layout.width());
    assert_eq!(
        HexLayout::fitting(64, 100).lines(b"GET / HTTP/1.1"),
        ["0000  47 45 54 20 2f 20 48 54  GET / HT", "0008  54 50 2f 31 2e 31        TP/1.1"]
    );
}
