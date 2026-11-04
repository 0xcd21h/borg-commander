use std::{
    cmp::Ordering,
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use borg_core::{ListingMode, Panel, Row, Sort, SortKey, order};
use borg_vfs::{Entry, EntryKind};
use proptest::prelude::*;

const ROWS: usize = 7;
const GENERATION: u64 = 1;
const INSIDE: u64 = 2;
const RETURNED: u64 = 3;
const PARENT: &str = "/data";

#[derive(Debug, Clone)]
enum Op {
    Move(isize),
    Page(isize),
    Start,
    End,
    Settle,
}

fn entries() -> impl Strategy<Value = Vec<Entry>> {
    let name = "[a-cA-C._]{1,4}";
    proptest::collection::btree_map(name, any::<bool>(), 0..80).prop_map(|map: BTreeMap<String, bool>| {
        map.into_iter()
            .enumerate()
            .map(|(i, (name, dir))| Entry {
                source: None,
                missing: false,
                usage: None,
                size: (i as u64 * 7919) % 97,
                modified: None,
                name,
                kind: if dir { EntryKind::Dir } else { EntryKind::File },
                executable: false,
                permissions: 0o644,
            })
            .collect()
    })
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (-3isize..=3).prop_map(Op::Move),
        (-2isize..=2).prop_map(Op::Page),
        Just(Op::Start),
        Just(Op::End),
        Just(Op::Settle),
    ]
}

fn layout() -> impl Strategy<Value = (Sort, ListingMode)> {
    let key = prop_oneof![Just(SortKey::Name), Just(SortKey::Extension), Just(SortKey::Size), Just(SortKey::Modified)];
    let mode = prop_oneof![Just(ListingMode::Full), Just(ListingMode::Brief)];
    (key, any::<bool>(), mode).prop_map(|(key, reverse, mode)| (Sort { key, reverse }, mode))
}

fn apply(panel: &mut Panel, op: &Op) {
    match op {
        Op::Move(n) => panel.move_by(*n),
        Op::Page(n) => panel.page(*n),
        Op::Start => panel.to_start(),
        Op::End => panel.to_end(),
        Op::Settle => panel.settle(),
    }
}

fn check_invariants(panel: &Panel) -> Result<(), TestCaseError> {
    let sort = panel.sort();
    let entries = panel.entries();
    prop_assert!(entries.windows(2).all(|w| order(&w[0], &w[1], sort) != Ordering::Greater), "entries out of order");
    prop_assert!(entries.windows(2).all(|w| !w[1].is_dir() || w[0].is_dir()), "directories must come first");
    if panel.is_empty() {
        return Ok(());
    }
    prop_assert!(panel.cursor() < panel.len());
    prop_assert!(panel.scroll() <= panel.cursor());
    prop_assert!(panel.cursor() < panel.scroll() + panel.capacity());
    Ok(())
}

proptest! {
    #[test]
    fn listing_is_sorted_complete_and_cursor_stays_in_view(
        all in entries(),
        cuts in proptest::collection::vec(any::<prop::sample::Index>(), 0..6),
        ops in proptest::collection::vec(op(), 0..40),
        show_hidden in any::<bool>(),
        (sort, mode) in layout(),
    ) {
        let mut panel = Panel::new(PathBuf::from("/"), ROWS, mode, sort);
        panel.navigate(PathBuf::from("/data"), GENERATION, None);

        let mut points: Vec<usize> = cuts.iter().map(|c| c.index(all.len() + 1)).collect();
        points.sort_unstable();
        let mut start = 0;
        let mut ops = ops.iter();
        for end in points.into_iter().chain([all.len()]) {
            panel.receive(GENERATION, all[start..end].to_vec(), show_hidden);
            start = end;
            if let Some(op) = ops.next() {
                apply(&mut panel, op);
                check_invariants(&panel)?;
            }
        }
        panel.finish(GENERATION, None);
        panel.settle();
        ops.for_each(|op| apply(&mut panel, op));
        check_invariants(&panel)?;

        let visible = all.iter().filter(|e| show_hidden || !e.is_hidden()).count();
        prop_assert_eq!(panel.entries().len(), visible);
    }

    #[test]
    fn hovered_entry_survives_new_batches(
        first in entries(),
        second in entries(),
        steps in 0isize..20,
        (sort, mode) in layout(),
    ) {
        let mut panel = Panel::new(PathBuf::from("/"), ROWS, mode, sort);
        panel.navigate(PathBuf::from("/data"), GENERATION, None);
        panel.receive(GENERATION, first.clone(), true);
        panel.finish(GENERATION, None);
        panel.settle();
        panel.move_by(steps);

        let before = match panel.hovered() {
            Some(Row::Entry(entry)) => Some(entry.clone()),
            _ => None,
        };
        let names: Vec<_> = first.iter().map(|e| e.name.clone()).collect();
        let fresh: Vec<_> = second.into_iter().filter(|e| !names.contains(&e.name)).collect();
        panel.receive(GENERATION, fresh, true);
        panel.settle();

        if let Some(before) = before {
            prop_assert_eq!(panel.hovered(), Some(Row::Entry(&before)));
        }
    }

    #[test]
    fn returning_from_a_folder_keeps_the_cursor_on_its_row_as_far_as_the_list_allows(
        before in entries(),
        after in entries(),
        steps in 0isize..80,
        changed in any::<bool>(),
        rows in 1usize..2 * ROWS,
        (sort, mode) in layout(),
    ) {
        let mut panel = Panel::new(PathBuf::from("/"), ROWS, mode, sort);
        panel.navigate(PathBuf::from(PARENT), GENERATION, None);
        panel.receive(GENERATION, before.clone(), true);
        panel.finish(GENERATION, None);
        panel.settle();
        panel.move_by(steps);
        let entered = match panel.hovered() {
            Some(Row::Entry(entry)) if entry.is_dir() && Path::new(&entry.name).file_name().is_some() => entry.clone(),
            _ => return Ok(()),
        };
        let left = (panel.cursor(), panel.scroll());
        let row = left.0 - left.1;

        panel.navigate(PathBuf::from(PARENT).join(&entered.name), INSIDE, None);
        panel.finish(INSIDE, None);
        panel.settle();
        prop_assert_eq!((panel.cursor(), panel.scroll()), (0, 0));

        let listing = if changed {
            after.into_iter().filter(|entry| entry.name != entered.name).chain([entered.clone()]).collect()
        } else {
            before
        };
        let height = if changed { rows } else { ROWS };
        panel.set_rows(height);
        panel.navigate(PathBuf::from(PARENT), RETURNED, Some(entered.name.clone()));
        panel.receive(RETURNED, listing, true);
        panel.finish(RETURNED, None);
        panel.settle();

        prop_assert_eq!(panel.hovered(), Some(Row::Entry(&entered)));
        check_invariants(&panel)?;
        if !changed {
            prop_assert_eq!((panel.cursor(), panel.scroll()), left);
        }
        if mode == ListingMode::Full {
            let capacity = panel.capacity();
            let last = panel.len().saturating_sub(capacity);
            let wanted = panel.cursor().saturating_sub(row.min(capacity - 1)).min(last);
            prop_assert_eq!(panel.scroll(), wanted);
        }
    }
}
