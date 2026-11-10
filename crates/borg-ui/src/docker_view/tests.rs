use super::*;
use crate::docker_view::details::summed;
use ratatui::layout::Rect;

#[test]
fn histories_add_up_from_their_latest_samples() {
    assert_eq!(summed(&[&[1, 2, 3], &[10, 20]]), [1, 12, 23]);
    assert!(summed(&[]).is_empty());
}

#[test]
fn narrow_lists_drop_the_momentary_cpu_and_then_the_status() {
    let widths = |width| columns(Rect::new(0, 0, width, 1)).map(|cell| cell.width);
    let [name, now, average, memory, status] = widths(80);
    assert!(now > 0 && status > 0, "a wide list shows every column");
    assert!(name + now + average + memory + status + 4 <= 80);
    let [_, now, _, _, status] = widths(58);
    assert_eq!(now, 0, "the momentary CPU goes first");
    assert!(status > 0);
    let [_, now, average, _, status] = widths(40);
    assert_eq!((now, status), (0, 0));
    assert!(average > 0, "the average stays");
    for width in 0..160 {
        let [name, now, _, _, status] = widths(width);
        assert!(name >= NAME_MIN || (now, status) == (0, 0), "names keep their room at {width}");
    }
}
