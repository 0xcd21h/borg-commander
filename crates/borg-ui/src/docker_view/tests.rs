use super::*;
use crate::docker_view::details::summed;
use ratatui::layout::Rect;

#[test]
fn histories_add_up_from_their_latest_samples() {
    assert_eq!(summed(&[&[1, 2, 3], &[10, 20]]), [1, 12, 23]);
    assert!(summed(&[]).is_empty());
}

#[test]
fn narrow_lists_drop_the_status_column() {
    let wide = columns(Rect::new(0, 0, 80, 1));
    assert!(wide[3].width > 0 && wide.iter().map(|c| c.width).sum::<u16>() <= 80);
    assert_eq!(columns(Rect::new(0, 0, 40, 1))[3].width, 0);
}
