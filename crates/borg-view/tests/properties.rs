use std::sync::Arc;

use borg_view::{Pager, Source};
use proptest::prelude::*;

const LINE_LIMIT: u64 = 5;
const SCAN_LIMIT: u64 = 64;
const WIDTH: usize = 80;

fn content() -> impl Strategy<Value = Vec<u8>> {
    let piece = prop_oneof![
        3 => proptest::collection::vec(b'a'..=b'z', 0..14),
        1 => Just(Vec::new()),
    ];
    (proptest::collection::vec(piece, 0..12), any::<bool>()).prop_map(|(lines, trailing)| {
        let mut bytes = lines.join(&b'\n');
        if trailing && !bytes.is_empty() {
            bytes.push(b'\n');
        }
        bytes
    })
}

fn unwrapped(bytes: Vec<u8>) -> Pager {
    let source: Arc<dyn Source> = Arc::new(bytes);
    let mut pager = Pager::with_limits(source, WIDTH, 1, LINE_LIMIT, SCAN_LIMIT).unwrap_or_else(|e| panic!("{e}"));
    pager.toggle_wrap().unwrap_or_else(|e| panic!("{e}"));
    pager
}

proptest! {
    #[test]
    fn walking_forward_and_backward_visits_the_same_lines(bytes in content()) {
        let mut pager = unwrapped(bytes.clone());
        let mut forward = vec![(pager.offset(), pager.rows().to_vec())];
        loop {
            pager.scroll_down(1).unwrap_or_else(|e| panic!("{e}"));
            if pager.offset() == forward.last().map_or(0, |(offset, _)| *offset) {
                break;
            }
            forward.push((pager.offset(), pager.rows().to_vec()));
        }

        let mut backward = vec![(pager.offset(), pager.rows().to_vec())];
        while pager.offset() > 0 {
            pager.scroll_up(1).unwrap_or_else(|e| panic!("{e}"));
            backward.push((pager.offset(), pager.rows().to_vec()));
        }
        backward.reverse();
        prop_assert_eq!(&forward, &backward);

        let shown: usize = forward.iter().flat_map(|(_, rows)| rows).map(String::len).sum();
        let content_bytes = bytes.iter().filter(|&&b| b != b'\n').count();
        prop_assert_eq!(shown, content_bytes);
    }

    #[test]
    fn rows_never_exceed_width_when_wrapped(bytes in content(), width in 1usize..12, height in 1usize..6) {
        let source: Arc<dyn Source> = Arc::new(bytes);
        let mut pager = Pager::with_limits(source, width, height, LINE_LIMIT, SCAN_LIMIT).unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..20 {
            prop_assert!(pager.rows().len() <= height);
            prop_assert!(pager.rows().iter().all(|row| row.chars().count() <= width));
            pager.scroll_down(1).unwrap_or_else(|e| panic!("{e}"));
        }
    }
}
