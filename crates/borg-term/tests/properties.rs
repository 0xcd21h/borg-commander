use borg_term::{OscScanner, ShellSignal, TermMode, encode_key};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use proptest::prelude::*;

const MARKS: [(&[u8], ShellSignal); 4] = [
    (b"\x1b]133;A\x07", ShellSignal::PromptStart),
    (b"\x1b]133;B\x1b\\", ShellSignal::InputStart),
    (b"\x1b]133;C\x07", ShellSignal::CommandStart),
    (b"\x1b]133;D;7\x07", ShellSignal::CommandEnd { exit: Some(7) }),
];

#[derive(Debug, Clone)]
enum Piece {
    Noise(Vec<u8>),
    Mark(usize),
}

fn piece() -> impl Strategy<Value = Piece> {
    let noise = proptest::collection::vec(any::<u8>().prop_filter("no escapes", |b| *b != 0x1b), 0..40);
    prop_oneof![noise.prop_map(Piece::Noise), (0..MARKS.len()).prop_map(Piece::Mark)]
}

fn assemble(pieces: &[Piece]) -> (Vec<u8>, Vec<ShellSignal>) {
    pieces.iter().fold((Vec::new(), Vec::new()), |(mut bytes, mut signals), piece| {
        match piece {
            Piece::Noise(noise) => bytes.extend(noise),
            Piece::Mark(i) => {
                bytes.extend(MARKS[*i].0);
                signals.push(MARKS[*i].1.clone());
            }
        }
        (bytes, signals)
    })
}

proptest! {
    #[test]
    fn scanner_finds_every_mark_regardless_of_chunking(
        pieces in proptest::collection::vec(piece(), 0..30),
        cuts in proptest::collection::vec(any::<prop::sample::Index>(), 0..12),
    ) {
        let (bytes, expected) = assemble(&pieces);
        let mut points: Vec<usize> = cuts.iter().map(|c| c.index(bytes.len() + 1)).collect();
        points.sort_unstable();
        points.dedup();

        let mut scanner = OscScanner::default();
        let mut start = 0;
        let mut found = Vec::new();
        for end in points.into_iter().chain([bytes.len()]) {
            if end < start { continue; }
            found.extend(scanner.scan(&bytes[start..end]).into_iter().map(|(_, signal)| signal));
            start = end;
        }
        prop_assert_eq!(found, expected);
    }

    #[test]
    fn scanner_offsets_point_just_past_each_mark(pieces in proptest::collection::vec(piece(), 0..30)) {
        let (bytes, _) = assemble(&pieces);
        for (end, _) in OscScanner::default().scan(&bytes) {
            prop_assert!(end <= bytes.len());
            prop_assert!(matches!(bytes[end - 1], 0x07 | b'\\'));
        }
    }

    #[test]
    fn ctrl_letters_encode_to_single_control_byte(c in proptest::char::range('a', 'z')) {
        let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        let bytes = encode_key(&key, TermMode::empty()).unwrap_or_default();
        prop_assert_eq!(bytes, vec![u8::try_from(c).unwrap_or_default() - b'a' + 1]);
    }

    #[test]
    fn printable_text_encodes_as_utf8(c in any::<char>().prop_filter("printable", |c| !c.is_control())) {
        let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        prop_assert_eq!(encode_key(&key, TermMode::empty()).unwrap_or_default(), c.to_string().into_bytes());
    }
}
