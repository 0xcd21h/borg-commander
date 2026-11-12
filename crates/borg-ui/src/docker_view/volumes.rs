//! What a volume is, beside its name: when it was last used and made, its Compose project and
//! the containers that mount it, in as many columns as the Name column has room for, and in
//! full in the other pane.

use super::disk::{Sheet, label_width, size};
use crate::{activity_view::dim, format};
use borg_core::{Disk, DiskGroup, DiskItem, DiskKind, LastUsed};
use borg_docker::{DiskUse, Volume, VolumeUser};
use borg_i18n::tr;
use jiff::{Timestamp, Zoned};
use ratatui::buffer::Buffer;
use std::time::SystemTime;
use unicode_width::UnicodeWidthStr;

/// Marks a date that is an estimate.
const ABOUT: &str = "~";
const GAP: usize = 2;
/// The fewest cells a name keeps when the Name column is too narrow for it and the last use.
const NAME_FLOOR: usize = 20;
/// The widest a project's column grows.
const PROJECT_LIMIT: usize = 24;
/// The fewest cells the containers' column needs to be worth showing.
const USERS_MIN: usize = 10;
const LIST_SEPARATOR: &str = ", ";
const FACT_SEPARATOR: &str = " · ";
const OPTION_VALUE: &str = "=";
/// The driver Docker uses unless told otherwise, which needs no mention.
const LOCAL_DRIVER: &str = "local";

/// A fact the list shows beside a volume's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fact {
    LastUsed,
    Created,
    Project,
    Users,
}

impl Fact {
    /// Left to right.
    const SHOWN: [Self; 4] = [Self::LastUsed, Self::Created, Self::Project, Self::Users];
    /// Which go first when there is no room for all of them.
    const KEPT: [Self; 4] = [Self::LastUsed, Self::Project, Self::Created, Self::Users];

    fn heading(self) -> &'static str {
        match self {
            Self::LastUsed => tr!(docker_disk_last_used),
            Self::Created => tr!(docker_created),
            Self::Project => tr!(docker_disk_project),
            Self::Users => tr!(docker_disk_used_by),
        }
    }

    fn value(self, disk: &Disk, volume: &Volume, clock: &Zoned) -> String {
        match self {
            Self::LastUsed => last_used(disk.last_used(volume), clock),
            Self::Created => day(volume.created, clock),
            Self::Project => volume.compose.as_ref().map(|compose| compose.project.clone()).unwrap_or_default(),
            Self::Users => {
                volume.users.iter().map(|user| user.container.as_str()).collect::<Vec<_>>().join(LIST_SEPARATOR)
            }
        }
    }
}

fn day(time: Option<Timestamp>, clock: &Zoned) -> String {
    format::day(time.map(SystemTime::from), clock)
}

/// The last use as the list shows it: in use, a day, a day marked as an estimate, or nothing.
fn last_used(used: LastUsed, clock: &Zoned) -> String {
    match used {
        LastUsed::Now => tr!(docker_disk_now).into(),
        LastUsed::Stopped(time) => day(Some(time), clock),
        LastUsed::Changed(time) => format!("{ABOUT}{}", day(Some(time), clock)),
        LastUsed::Unknown => String::new(),
    }
}

/// Where each shown fact starts in the Name column, and how wide it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Layout {
    /// The cells the name keeps.
    pub label: usize,
    pub facts: Vec<(Fact, usize, usize)>,
}

/// Lays out the facts in a Name column `width` cells wide, after names `label` cells wide:
/// the last use first, shortening long names for it if need be, then the others as they fit,
/// with the containers taking what is left.
pub(super) fn layout(width: usize, label: usize, widths: impl Fn(Fact) -> usize) -> Layout {
    let needed = widths(Fact::LastUsed) + GAP;
    let label = if width.saturating_sub(label) < needed {
        label.min(width.saturating_sub(needed).max(NAME_FLOOR))
    } else {
        label
    }
    .min(width);
    let mut room = width - label;
    let mut kept = Vec::new();
    for fact in Fact::KEPT {
        let wanted = widths(fact) + GAP;
        if fact == Fact::Users && room >= USERS_MIN + GAP {
            kept.push((fact, room - GAP));
            room = 0;
        } else if fact != Fact::Users && wanted <= room {
            kept.push((fact, wanted - GAP));
            room -= wanted;
        }
    }
    let mut x = label;
    let facts = Fact::SHOWN
        .into_iter()
        .filter_map(|fact| kept.iter().find(|(kept, _)| *kept == fact))
        .map(|&(fact, width)| {
            let start = x + GAP;
            x = start + width;
            (fact, start, width)
        })
        .collect();
    Layout { label, facts }
}

/// The layout for the volumes the list shows, so their columns line up across the kinds.
pub(super) fn list_layout(
    disk: &Disk,
    usage: &DiskUse,
    width: usize,
    clock: &Zoned,
    kind_width: impl Fn(DiskKind) -> usize,
    item_width: impl Fn(DiskItem) -> usize,
) -> Layout {
    let shown: Vec<&DiskGroup> = disk
        .groups
        .iter()
        .filter(|group| group.kind.holds_data() && disk.unfolded(group.kind) && !group.items.is_empty())
        .collect();
    let items: Vec<DiskItem> = shown.iter().flat_map(|group| group.items.iter().copied()).collect();
    let label = shown
        .iter()
        .map(|group| kind_width(group.kind))
        .chain(items.iter().map(|&item| item_width(item)))
        .max()
        .unwrap_or(0);
    let volumes: Vec<&Volume> = items
        .iter()
        .filter_map(|item| match item {
            DiskItem::Volume(index) => usage.volumes.get(*index),
            _ => None,
        })
        .collect();
    layout(width, label, |fact| {
        let widest = volumes.iter().map(|volume| fact.value(disk, volume, clock).width()).max().unwrap_or(0);
        let width = widest.max(fact.heading().width());
        if fact == Fact::Project { width.min(PROJECT_LIMIT) } else { width }
    })
}

/// The headings of the facts, for a volume kind's row.
pub(super) fn headings(layout: &Layout) -> Vec<(usize, usize, &'static str)> {
    layout.facts.iter().map(|&(fact, start, width)| (start, width, fact.heading())).collect()
}

/// The facts of a volume, for its row.
pub(super) fn cells(layout: &Layout, disk: &Disk, volume: &Volume, clock: &Zoned) -> Vec<(usize, usize, String)> {
    layout.facts.iter().map(|&(fact, start, width)| (start, width, fact.value(disk, volume, clock))).collect()
}

fn user_line(user: &VolumeUser) -> String {
    let (container, path) = (user.container.as_str(), user.path.as_str());
    if user.state.is_up() {
        tr!(docker_disk_mount_running, container = container, path = path)
    } else {
        tr!(docker_disk_mount_stopped, container = container, path = path)
    }
}

/// The last use as the details say it, and the note that goes with it.
fn last_use_fact(disk: &Disk, volume: &Volume, clock: &Zoned) -> (String, Option<&'static str>) {
    let stamp = |time: Timestamp| format::stamp(Some(SystemTime::from(time)), clock);
    match disk.last_used(volume) {
        LastUsed::Now => (tr!(docker_disk_used_now).into(), None),
        LastUsed::Stopped(time) => {
            let last =
                volume.users.iter().filter(|user| user.finished == Some(time)).map(|user| user.container.as_str());
            let container = last.collect::<Vec<_>>().join(LIST_SEPARATOR);
            (tr!(docker_disk_used_stopped, time = stamp(time), container = container), None)
        }
        LastUsed::Changed(time) => (tr!(docker_disk_used_about, time = stamp(time)), Some(tr!(docker_disk_estimate))),
        LastUsed::Unknown => (tr!(docker_disk_used_unknown).into(), Some(tr!(docker_disk_no_record))),
    }
}

/// Everything known about a volume, for the other pane.
pub(super) fn details(
    sheet: &mut Sheet<'_>,
    disk: &Disk,
    volume: &Volume,
    item: DiskItem,
    clock: &Zoned,
    buf: &mut Buffer,
) {
    let theme = sheet.look.theme;
    let usage = disk.usage.as_ref();
    let (used, note) = last_use_fact(disk, volume, clock);
    let users_label = if volume.anonymous { tr!(docker_disk_made_by) } else { tr!(docker_disk_used_by) };
    let mut facts: Vec<(&str, String)> = Vec::new();
    if volume.anonymous {
        facts.push((tr!(docker_name), volume.name.clone()));
    }
    facts.push((tr!(docker_disk_size), size(volume.size)));
    facts.push((tr!(docker_disk_reclaimable), size(usage.and_then(|usage| item.frees(usage)))));
    facts.push((tr!(docker_disk_last_used), used));
    facts.push((tr!(docker_created), format::stamp(volume.created.map(SystemTime::from), clock)));
    if let Some(compose) = &volume.compose {
        let names = std::iter::once(compose.project.as_str()).chain(compose.volume.as_deref());
        facts.push((tr!(docker_disk_compose), names.collect::<Vec<_>>().join(FACT_SEPARATOR)));
    }
    if volume.driver != LOCAL_DRIVER || !volume.options.is_empty() {
        let options = volume.options.iter().map(|(key, value)| format!("{key}{OPTION_VALUE}{value}"));
        let driver = std::iter::once(volume.driver.clone()).chain(options);
        facts.push((tr!(docker_disk_driver), driver.collect::<Vec<_>>().join(FACT_SEPARATOR)));
    }
    let mut users = volume.users.iter().map(user_line);
    if let Some(first) = users.next() {
        facts.push((users_label, first));
        facts.extend(users.map(|line| ("", line)));
    }
    if volume.anonymous {
        let mut images: Vec<&str> = volume.users.iter().map(|user| user.image.as_str()).collect();
        images.sort_unstable();
        images.dedup();
        facts.push((tr!(docker_image), images.join(LIST_SEPARATOR)));
    }
    if let Some(folder) = disk.folders.get(&volume.name) {
        facts.push((tr!(docker_disk_folder), folder.display().to_string()));
    }
    if let Some(contents) = disk.contents.get(&volume.name) {
        facts.push((tr!(docker_disk_holds), contents.top.join(LIST_SEPARATOR)));
    }
    sheet.label = label_width(&facts.iter().map(|(label, _)| *label).collect::<Vec<_>>());
    for (label, value) in facts {
        sheet.fact(buf, label, value);
    }
    let gone = (volume.anonymous && volume.users.is_empty()).then(|| tr!(docker_disk_maker_gone));
    let (reach, hint) = if disk.folders.contains_key(&volume.name) {
        (tr!(docker_disk_browse), None)
    } else {
        (tr!(docker_disk_browse_helper), disk.share_hint())
    };
    for paragraph in [note, gone, Some(reach), hint].into_iter().flatten() {
        sheet.gap();
        sheet.paragraph(buf, paragraph, dim(theme));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths(fact: Fact) -> usize {
        match fact {
            Fact::LastUsed => 11,
            Fact::Created => 10,
            Fact::Project => 12,
            Fact::Users => 9,
        }
    }

    fn shown(layout: &Layout) -> Vec<Fact> {
        layout.facts.iter().map(|&(fact, ..)| fact).collect()
    }

    #[test]
    fn a_wide_name_column_shows_every_fact_with_the_containers_taking_the_rest() {
        let layout = layout(120, 30, widths);
        assert_eq!(shown(&layout), Fact::SHOWN);
        assert_eq!(
            layout.facts,
            [(Fact::LastUsed, 32, 11), (Fact::Created, 45, 10), (Fact::Project, 57, 12), (Fact::Users, 71, 49)]
        );
    }

    #[test]
    fn narrower_columns_keep_the_last_use_then_the_project() {
        assert_eq!(shown(&layout(60, 30, widths)), [Fact::LastUsed, Fact::Project]);
        assert_eq!(shown(&layout(75, 30, widths)), [Fact::LastUsed, Fact::Created, Fact::Project]);
    }

    #[test]
    fn long_names_shorten_for_the_last_use_down_to_a_floor() {
        let layout = layout(38, 34, widths);
        assert_eq!((layout.label, shown(&layout)), (25, vec![Fact::LastUsed]));
        let narrow = super::layout(26, 34, widths);
        assert_eq!((narrow.label, shown(&narrow)), (20, Vec::new()), "too narrow for any fact");
    }
}
