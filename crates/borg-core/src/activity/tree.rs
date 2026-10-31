//! The memory map's tree: regions grouped by what they hold, and the files they map.

use super::Activity;
use super::{GroupEntry, Member, MemoryGroup, MemoryRegion, MemoryTotals, RegionItem, RegionRow};
use std::collections::HashMap;

impl Activity {
    pub fn regions(&self) -> &[MemoryRegion] {
        &self.regions
    }

    pub fn grouped(&self) -> bool {
        self.grouped
    }

    /// Totals of the whole memory map.
    pub fn region_totals(&self) -> MemoryTotals {
        self.totals
    }

    pub fn group_totals(&self, group: MemoryGroup) -> MemoryTotals {
        self.groups.iter().find(|g| g.group == group).map_or_else(MemoryTotals::default, |g| g.totals)
    }

    /// Replaces the memory map and ranks it once; moving through it reuses the result.
    pub fn set_regions(&mut self, regions: Vec<MemoryRegion>) {
        self.regions = regions;
        self.program = self.process().and_then(|p| p.executable.as_ref()).map(|p| p.to_string_lossy().into_owned());
        self.totals = MemoryTotals::of(&self.regions);
        self.groups = self.rank_groups();
        self.refresh_tree();
    }

    pub(crate) fn set_grouped(&mut self, grouped: bool) {
        self.grouped = grouped;
        self.refresh_tree();
    }

    pub fn region_group(&self, region: &MemoryRegion) -> MemoryGroup {
        region.group(self.program.as_deref())
    }

    /// Groups ordered by footprint (reservations last), each with its members: mapped files
    /// for groups listed per file, regions otherwise, ordered by footprint.
    pub(super) fn rank_groups(&self) -> Vec<GroupEntry> {
        let mut buckets: HashMap<MemoryGroup, Vec<usize>> = HashMap::new();
        for (i, region) in self.regions.iter().enumerate() {
            buckets.entry(self.region_group(region)).or_default().push(i);
        }
        let totals = |indices: &[usize]| MemoryTotals::of(indices.iter().map(|&i| &self.regions[i]));
        let mut groups: Vec<_> = MemoryGroup::ALL
            .into_iter()
            .filter_map(|group| {
                let indices = buckets.remove(&group)?;
                let mut members: Vec<Member> = if group.by_file() {
                    let mut files: Vec<(usize, Vec<usize>)> = Vec::new();
                    let mut positions: HashMap<&str, usize> = HashMap::new();
                    for &i in &indices {
                        let at = *positions.entry(self.file_key(i)).or_insert_with(|| {
                            files.push((i, Vec::new()));
                            files.len() - 1
                        });
                        files[at].1.push(i);
                    }
                    files
                        .into_iter()
                        .map(|(first, members)| (RegionRow::File(first), totals(&members), members))
                        .collect()
                } else {
                    indices.iter().map(|&i| (RegionRow::Region(i), totals(&[i]), vec![i])).collect()
                };
                members.sort_by(|a, b| a.1.rank().cmp(&b.1.rank()).then_with(|| self.member_order(a.0, b.0)));
                Some(GroupEntry { group, totals: totals(&indices), members })
            })
            .collect();
        groups.sort_by_key(|g| (g.group == MemoryGroup::Reserved, g.totals.rank()));
        groups
    }

    pub(super) fn member_order(&self, a: RegionRow, b: RegionRow) -> std::cmp::Ordering {
        match (a, b) {
            (RegionRow::File(a), RegionRow::File(b)) => self.file_name(a).cmp(self.file_name(b)),
            (RegionRow::Region(a), RegionRow::Region(b)) => self.regions[a].start.cmp(&self.regions[b].start),
            _ => std::cmp::Ordering::Equal,
        }
    }

    /// The members of a group, largest footprint first.
    pub fn group_members(&self, group: MemoryGroup) -> impl Iterator<Item = (RegionRow, MemoryTotals)> + '_ {
        self.groups.iter().filter(move |g| g.group == group).flat_map(|g| g.members.iter().map(|m| (m.0, m.1)))
    }

    pub(super) fn refresh_tree(&mut self) {
        let item = |row, depth, totals, files, expanded| RegionItem { row, depth, totals, files, expanded };
        let single = |i: usize| MemoryTotals::of([&self.regions[i]]);
        if !self.grouped {
            self.visible =
                (0..self.regions.len()).map(|i| item(RegionRow::Region(i), 0, single(i), 0, false)).collect();
            return;
        }
        let mut items = Vec::new();
        for entry in &self.groups {
            let open = self.expanded_groups.contains(&entry.group);
            let files = if entry.group.by_file() { entry.members.len() } else { 0 };
            items.push(item(RegionRow::Group(entry.group), 0, entry.totals, files, open));
            if !open {
                continue;
            }
            for (row, totals, regions) in &entry.members {
                let file_open =
                    matches!(row, RegionRow::File(first) if self.expanded_files.contains(self.file_key(*first)));
                items.push(item(*row, 1, *totals, 0, file_open));
                if file_open {
                    items.extend(regions.iter().map(|&i| item(RegionRow::Region(i), 2, single(i), 0, false)));
                }
            }
        }
        self.visible = items;
    }

    /// The region list as shown: groups, open groups' members and open files' regions, or
    /// every region in address order when ungrouped.
    pub fn region_tree(&self) -> &[RegionItem] {
        &self.visible
    }

    pub(super) fn file_key(&self, first: usize) -> &str {
        self.regions.get(first).map_or("", |r| r.path.as_deref().unwrap_or(&r.name))
    }

    /// The short name of a mapped file: its last path component.
    pub fn file_name(&self, first: usize) -> &str {
        let key = self.file_key(first);
        key.rsplit('/').find(|part| !part.is_empty()).unwrap_or(key)
    }

    pub(super) fn file_entry(&self, first: usize) -> Option<(MemoryGroup, &Member)> {
        self.groups.iter().find_map(|g| g.members.iter().find(|m| m.0 == RegionRow::File(first)).map(|m| (g.group, m)))
    }

    /// The group listing a mapped file.
    pub fn file_group(&self, first: usize) -> Option<MemoryGroup> {
        self.file_entry(first).map(|(group, _)| group)
    }

    /// The regions mapped from the same file as the region at `first`, in address order.
    pub fn file_regions(&self, first: usize) -> Vec<&MemoryRegion> {
        self.file_entry(first).map(|(_, m)| m.2.iter().map(|&i| &self.regions[i]).collect()).unwrap_or_default()
    }

    pub fn region_rows(&self) -> Vec<RegionRow> {
        self.visible.iter().map(|item| item.row).collect()
    }
}
