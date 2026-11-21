use borg_core::{Action, MenuItem};

use super::*;

fn menu(title: &str, count: usize) -> Menu {
    let entry = MenuItem { label: "Entry", hint: String::new(), action: Action::Reload, checked: None, enabled: true };
    Menu { title: title.into(), entries: vec![MenuEntry::Item(entry); count] }
}

/// Titles that start at columns 2, 10, 18 and 29 and end at column 43.
fn bar() -> Vec<Menu> {
    ["Left", "File", "Command", "Encryption"].into_iter().map(|title| menu(title, 1)).collect()
}

#[test]
fn a_menu_that_fits_shows_from_its_first_entry() {
    assert_eq!(first_entry(3, 4, Some(2)), 0);
    assert_eq!(first_entry(10, 4, None), 0);
}

#[test]
fn a_tall_menu_keeps_the_chosen_entry_mid_menu_as_far_as_its_ends_allow() {
    assert_eq!(first_entry(10, 4, Some(0)), 0);
    assert_eq!(first_entry(10, 4, Some(3)), 1);
    assert_eq!(first_entry(10, 4, Some(7)), 5);
    assert_eq!(first_entry(10, 4, Some(9)), 6);
}

#[test]
fn a_click_on_a_scrolled_menu_takes_the_entry_shown_there() {
    let tall = menu("Tall", 10);
    let rect = Rect { x: 0, y: 1, width: 20, height: 6 };
    assert_eq!(entry_at(&tall, Some(9), rect, Position::new(3, 2)), Some(6));
    assert_eq!(entry_at(&tall, Some(9), rect, Position::new(3, 5)), Some(9));
    assert_eq!(entry_at(&tall, Some(0), rect, Position::new(3, 2)), Some(0));
    assert_eq!(entry_at(&tall, Some(9), rect, Position::new(3, 1)), None, "the border");
}

#[test]
fn the_menu_bar_stays_put_while_its_titles_fit() {
    assert_eq!(bar_shift(&bar(), Some(3), 43), 0);
    assert_eq!(bar_shift(&bar(), Some(3), 80), 0);
}

#[test]
fn the_menu_bar_moves_so_the_open_title_shows_whole_with_a_mark_after_it() {
    let bar = bar();
    assert_eq!(bar_shift(&bar, None, 30), 0);
    assert_eq!(bar_shift(&bar, Some(0), 30), 0);
    assert_eq!(bar_shift(&bar, Some(2), 30), 0);
    assert_eq!(bar_shift(&bar, Some(3), 30), 14);
    assert_eq!(dropdown_left(&bar, 3, 30), 15);
    assert_eq!(title_at(&bar, 15, 14), Some(3));
    assert_eq!(title_at(&bar, 28, 14), Some(3));
    assert_eq!(title_at(&bar, 4, 14), Some(2));
    assert_eq!(title_at(&bar, 1, 0), None);
}
