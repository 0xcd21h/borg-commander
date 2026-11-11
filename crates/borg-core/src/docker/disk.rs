//! Disk use: what Docker keeps on disk kind by kind, the items of each kind, and the commands
//! that free the space. Borg never runs them: it types one at the host shell's prompt and shows
//! the console, the user reads it and presses Enter, and Docker asks again before a prune.

use super::DockerData;
use crate::command::CommandLine;
use borg_docker::{DiskUse, LastUse, Volume};
use borg_i18n::tr;
use jiff::Timestamp;
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    path::PathBuf,
};

mod keys;

pub(crate) use keys::{click, command_ended, measure, on_key, paste, toggle};

const DOCKER: &str = "docker";
const CONTEXT_FLAG: &str = "--context";
const ALL_FLAG: &str = "-a";
/// An anonymous volume's name is 64 hex digits; the list shows the first ones, as Docker shows IDs.
const SHORT_NAME: usize = 12;

/// What reading a volume's files found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VolumeContents {
    /// The newest change to anything in it, its top folder included.
    pub newest: Option<Timestamp>,
    /// The names at its top, sorted.
    pub top: Vec<String>,
}

/// When a volume was last used, as far as Borg can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastUsed {
    /// A container that mounts it is up.
    Now,
    /// When the last container that mounts it stopped.
    Stopped(Timestamp),
    /// An estimate: the newest change to its files, later than any container's stop. Reading
    /// leaves no trace, so it may have been used since.
    Changed(Timestamp),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiskKind {
    BuildCache,
    DanglingImages,
    UnusedImages,
    ImagesInUse,
    StoppedContainers,
    AnonymousVolumes,
    NamedVolumes,
    VolumesInUse,
    UnusedNetworks,
}

impl DiskKind {
    pub const ALL: [Self; 9] = [
        Self::BuildCache,
        Self::DanglingImages,
        Self::UnusedImages,
        Self::ImagesInUse,
        Self::StoppedContainers,
        Self::AnonymousVolumes,
        Self::NamedVolumes,
        Self::VolumesInUse,
        Self::UnusedNetworks,
    ];

    /// The words after `docker` that free this kind's space; nothing frees what is in use.
    fn prune(self) -> Option<&'static [&'static str]> {
        match self {
            Self::BuildCache => Some(&["builder", "prune"]),
            Self::DanglingImages => Some(&["image", "prune"]),
            Self::UnusedImages => Some(&["image", "prune", ALL_FLAG]),
            Self::StoppedContainers => Some(&["container", "prune"]),
            Self::AnonymousVolumes => Some(&["volume", "prune"]),
            Self::NamedVolumes => Some(&["volume", "prune", ALL_FLAG]),
            Self::UnusedNetworks => Some(&["network", "prune"]),
            Self::ImagesInUse | Self::VolumesInUse => None,
        }
    }

    /// What its items are removed with one by one. BuildKit takes a single record filter at a
    /// time, so build cache goes with a prune only.
    pub fn family(self) -> Option<DiskFamily> {
        match self {
            Self::DanglingImages | Self::UnusedImages => Some(DiskFamily::Images),
            Self::StoppedContainers => Some(DiskFamily::Containers),
            Self::AnonymousVolumes | Self::NamedVolumes => Some(DiskFamily::Volumes),
            Self::BuildCache | Self::ImagesInUse | Self::VolumesInUse | Self::UnusedNetworks => None,
        }
    }

    /// Volumes hold the data containers wrote, which no command brings back.
    pub fn holds_data(self) -> bool {
        matches!(self, Self::AnonymousVolumes | Self::NamedVolumes | Self::VolumesInUse)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiskFamily {
    Images,
    Containers,
    Volumes,
}

impl DiskFamily {
    fn removal(self) -> &'static [&'static str] {
        match self {
            Self::Images => &["rmi"],
            Self::Containers => &["rm"],
            Self::Volumes => &["volume", "rm"],
        }
    }
}

/// An item of a kind: its index in [`DiskUse`]'s list of its sort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskItem {
    Image(usize),
    Container(usize),
    Volume(usize),
    Cache(usize),
}

/// An item by what Docker calls it, which stays the same across measurements.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Id {
    Image(String),
    Container(String),
    Volume(String),
    Cache(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiskTotals {
    pub count: usize,
    pub size: u64,
    pub reclaimable: u64,
}

/// A kind with its totals and the items the filter leaves, biggest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskGroup {
    pub kind: DiskKind,
    pub totals: DiskTotals,
    pub items: Vec<DiskItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskRow {
    Kind(usize),
    Item(usize, DiskItem),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    Kind(DiskKind),
    Item(Id),
}

#[derive(Debug, Clone, Default)]
pub struct Disk {
    /// The Docker view shows disk use in place of the containers.
    pub open: bool,
    pub usage: Option<DiskUse>,
    /// The context commands name, so they reach the engine measured here whichever one the
    /// shell's `docker` would pick.
    pub context: Option<String>,
    pub error: Option<String>,
    pub loading: Option<u64>,
    pub groups: Vec<DiskGroup>,
    pub rows: Vec<DiskRow>,
    pub cursor: usize,
    pub top: usize,
    pub filter: String,
    pub filtering: bool,
    marks: HashSet<Id>,
    unfolded: HashSet<DiskKind>,
    selected: Option<Key>,
    /// A cleanup command waits at the prompt or runs, so the next command to end measures again.
    typed: bool,
    pub(super) stale: bool,
    /// Where each volume's files are on this machine, for the volumes Borg can read there.
    pub folders: HashMap<String, PathBuf>,
    /// What reading each volume's files found.
    pub contents: HashMap<String, VolumeContents>,
    /// The request reading volumes' files, while it runs.
    pub reading: Option<u64>,
}

/// The context commands name: the measured one whenever the CLI has another, or when it could
/// not say which it has, since the shell's `DOCKER_CONTEXT` or `docker context use` may pick
/// a different one.
pub(crate) fn named_context(current: Option<String>, all: Option<Vec<String>>) -> Option<String> {
    current.filter(|name| all.is_none_or(|all| all.iter().any(|other| other != name)))
}

fn docker(context: Option<&str>) -> CommandLine {
    let line = CommandLine::program(DOCKER);
    match context {
        Some(name) => line.flag(CONTEXT_FLAG).word(name),
        None => line,
    }
}

fn words(line: CommandLine, words: &[&str]) -> CommandLine {
    words.iter().fold(line, |line, word| line.flag(word))
}

impl DiskKind {
    pub fn members(self, usage: &DiskUse) -> Vec<DiskItem> {
        fn indexes<T>(items: &[T], keep: impl Fn(&T) -> bool, item: fn(usize) -> DiskItem) -> Vec<DiskItem> {
            items.iter().enumerate().filter(|(_, value)| keep(value)).map(|(index, _)| item(index)).collect()
        }
        match self {
            Self::BuildCache => indexes(&usage.cache, |_| true, DiskItem::Cache),
            Self::DanglingImages => indexes(&usage.images, |i| !i.in_use() && i.dangling(), DiskItem::Image),
            Self::UnusedImages => indexes(&usage.images, |i| !i.in_use() && !i.dangling(), DiskItem::Image),
            Self::ImagesInUse => indexes(&usage.images, |i| i.in_use(), DiskItem::Image),
            Self::StoppedContainers => indexes(&usage.containers, |c| c.stopped(), DiskItem::Container),
            Self::AnonymousVolumes => indexes(&usage.volumes, |v| !v.in_use() && v.anonymous, DiskItem::Volume),
            Self::NamedVolumes => indexes(&usage.volumes, |v| !v.in_use() && !v.anonymous, DiskItem::Volume),
            Self::VolumesInUse => indexes(&usage.volumes, |v| v.in_use(), DiskItem::Volume),
            Self::UnusedNetworks => Vec::new(),
        }
    }

    /// Its items biggest first; items Docker could not measure go last, then by name.
    fn sorted(self, usage: &DiskUse) -> Vec<DiskItem> {
        let mut items = self.members(usage);
        sort(usage, &mut items);
        items
    }

    pub fn totals(self, usage: &DiskUse) -> DiskTotals {
        if self == Self::UnusedNetworks {
            return DiskTotals { count: usage.networks.len(), ..DiskTotals::default() };
        }
        let items = self.members(usage);
        DiskTotals {
            count: items.len(),
            size: items.iter().filter_map(|item| item.size(usage)).sum(),
            reclaimable: items.iter().filter_map(|item| item.frees(usage)).sum(),
        }
    }
}

fn sort(usage: &DiskUse, items: &mut [DiskItem]) {
    items.sort_by_cached_key(|item| (Reverse(item.size(usage)), item.label(usage)));
}

impl DiskItem {
    pub fn size(self, usage: &DiskUse) -> Option<u64> {
        match self {
            Self::Image(i) => usage.images[i].size,
            Self::Container(i) => usage.containers[i].size,
            Self::Volume(i) => usage.volumes[i].size,
            Self::Cache(i) => usage.cache[i].size,
        }
    }

    /// What removing it frees: an image's own layers, the rest whole, nothing while in use.
    /// `None` when Docker could not measure it.
    pub fn frees(self, usage: &DiskUse) -> Option<u64> {
        let (bytes, free) = match self {
            Self::Image(i) => (usage.images[i].unique, !usage.images[i].in_use()),
            Self::Container(i) => (usage.containers[i].size, usage.containers[i].stopped()),
            Self::Volume(i) => (usage.volumes[i].size, !usage.volumes[i].in_use()),
            Self::Cache(i) => (usage.cache[i].size, usage.cache[i].reclaimable()),
        };
        bytes.map(|bytes| if free { bytes } else { 0 })
    }

    /// What the list calls it: an image's first tag, with a count of the others.
    pub fn label(self, usage: &DiskUse) -> String {
        match self {
            Self::Image(i) => {
                let image = &usage.images[i];
                match image.tags.len() {
                    0 | 1 => image.label().to_owned(),
                    tags => format!("{} +{}", image.label(), tags - 1),
                }
            }
            Self::Container(i) => usage.containers[i].name.clone(),
            Self::Volume(i) => {
                let volume = &usage.volumes[i];
                if volume.anonymous { volume.name.chars().take(SHORT_NAME).collect() } else { volume.name.clone() }
            }
            Self::Cache(i) => {
                let record = &usage.cache[i];
                if record.description.is_empty() { record.kind.clone() } else { record.description.clone() }
            }
        }
    }

    fn id(self, usage: &DiskUse) -> Id {
        match self {
            Self::Image(i) => Id::Image(usage.images[i].id.clone()),
            Self::Container(i) => Id::Container(usage.containers[i].id.clone()),
            Self::Volume(i) => Id::Volume(usage.volumes[i].name.clone()),
            Self::Cache(i) => Id::Cache(usage.cache[i].id.clone()),
        }
    }

    fn matches(self, usage: &DiskUse, filter: &str) -> bool {
        let fields: Vec<&str> = match self {
            Self::Image(i) => {
                let image = &usage.images[i];
                image
                    .tags
                    .iter()
                    .map(String::as_str)
                    .chain(image.repository.as_deref())
                    .chain([image.short_id()])
                    .collect()
            }
            Self::Container(i) => vec![usage.containers[i].name.as_str(), usage.containers[i].image.as_str()],
            Self::Volume(i) => {
                let volume = &usage.volumes[i];
                std::iter::once(volume.name.as_str())
                    .chain(volume.compose.as_ref().map(|compose| compose.project.as_str()))
                    .chain(volume.users.iter().flat_map(|user| [user.container.as_str(), user.image.as_str()]))
                    .collect()
            }
            Self::Cache(i) => vec![usage.cache[i].description.as_str(), usage.cache[i].kind.as_str()],
        };
        fields.into_iter().any(|field| field.to_lowercase().contains(filter))
    }

    /// The words `docker rmi`, `docker rm` or `docker volume rm` take for it.
    fn removal(self, usage: &DiskUse) -> Vec<&str> {
        match self {
            Self::Image(i) => usage.images[i].removal(),
            Self::Container(i) => vec![&usage.containers[i].name],
            Self::Volume(i) => vec![&usage.volumes[i].name],
            Self::Cache(_) => Vec::new(),
        }
    }
}

/// Why the cleanup key types nothing on a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoCleanup {
    /// Images and volumes that containers use stay until those containers go.
    InUse,
    /// Build cache records go together, with the prune on the kind's row.
    CacheRecord,
    /// A name holds a control character, which the shell's prompt would take as a key.
    ControlCharacter,
    Empty,
}

impl Disk {
    pub fn row(&self) -> Option<DiskRow> {
        self.rows.get(self.cursor).copied()
    }

    pub fn group(&self) -> Option<&DiskGroup> {
        match self.row()? {
            DiskRow::Kind(group) | DiskRow::Item(group, _) => self.groups.get(group),
        }
    }

    pub fn unfolded(&self, kind: DiskKind) -> bool {
        self.unfolded.contains(&kind) || !self.filter.is_empty()
    }

    pub fn marked(&self, item: DiskItem) -> bool {
        self.usage.as_ref().is_some_and(|usage| self.marks.contains(&item.id(usage)))
    }

    pub fn mark_count(&self) -> usize {
        self.marks.len()
    }

    /// When `volume` was last used: now or when its last container stopped, as Docker tells, or
    /// the newest change to its files when that is later or Docker tells nothing.
    pub fn last_used(&self, volume: &Volume) -> LastUsed {
        let changed = self.contents.get(&volume.name).and_then(|contents| contents.newest);
        match (volume.last_use(), changed) {
            (LastUse::Now, _) => LastUsed::Now,
            (LastUse::Stopped(stopped), Some(changed)) if changed > stopped => LastUsed::Changed(changed),
            (LastUse::Stopped(stopped), _) => LastUsed::Stopped(stopped),
            (LastUse::Unknown, Some(changed)) => LastUsed::Changed(changed),
            (LastUse::Unknown, None) => LastUsed::Unknown,
        }
    }

    /// The selected volume, if the cursor is on one.
    pub fn volume(&self) -> Option<&Volume> {
        match (self.usage.as_ref()?, self.row()?) {
            (usage, DiskRow::Item(_, DiskItem::Volume(index))) => usage.volumes.get(index),
            _ => None,
        }
    }

    /// Why Borg cannot show a volume's files: OrbStack shares them, but this machine did not let
    /// Borg read the share, or the engine keeps them in its virtual machine, elsewhere, or where
    /// only root reads.
    pub fn unreachable(&self) -> &'static str {
        if self.usage.as_ref().is_some_and(|usage| usage.engine.orbstack()) {
            tr!(docker_volume_share_denied)
        } else {
            tr!(docker_volume_out_of_reach)
        }
    }

    /// The volumes whose files Borg can read and has not read yet, biggest first.
    pub(super) fn unread(&self) -> Vec<(String, PathBuf)> {
        let Some(usage) = &self.usage else { return Vec::new() };
        let mut volumes: Vec<&Volume> = usage.volumes.iter().collect();
        volumes.sort_by_key(|volume| Reverse(volume.size));
        volumes
            .into_iter()
            .filter(|volume| !self.contents.contains_key(&volume.name))
            .filter_map(|volume| Some((volume.name.clone(), self.folders.get(&volume.name)?.clone())))
            .collect()
    }

    /// Everything the kinds would free together.
    pub fn reclaimable(&self) -> u64 {
        self.usage.as_ref().map_or(0, |usage| DiskKind::ALL.iter().map(|&kind| kind.totals(usage).reclaimable).sum())
    }

    pub fn list_start(&self, height: usize) -> usize {
        self.top.min(self.cursor).max(self.cursor.saturating_sub(height.max(1) - 1))
    }

    pub(super) fn keep_visible(&mut self, height: usize) {
        self.top = self.list_start(height);
    }

    fn key_at(&self, row: DiskRow) -> Option<Key> {
        match row {
            DiskRow::Kind(group) => Some(Key::Kind(self.groups[group].kind)),
            DiskRow::Item(_, item) => self.usage.as_ref().map(|usage| Key::Item(item.id(usage))),
        }
    }

    fn select(&mut self, cursor: usize) {
        self.cursor = cursor.min(self.rows.len().saturating_sub(1));
        self.selected = self.row().and_then(|row| self.key_at(row));
    }

    fn move_by(&mut self, delta: isize) {
        let last = self.rows.len().saturating_sub(1) as isize;
        self.select((self.cursor as isize + delta).clamp(0, last.max(0)) as usize);
    }

    /// Lays out the kinds with the items the filter and folding leave, keeps the cursor on what
    /// it was on, and drops marks on items that are gone or no longer removable.
    pub(crate) fn rebuild(&mut self) {
        let Some(usage) = &self.usage else {
            self.groups.clear();
            self.rows.clear();
            return;
        };
        let filter = self.filter.to_lowercase();
        self.groups = DiskKind::ALL
            .into_iter()
            .filter_map(|kind| {
                let items: Vec<DiskItem> =
                    kind.sorted(usage).into_iter().filter(|item| item.matches(usage, &filter)).collect();
                (filter.is_empty() || !items.is_empty()).then(|| DiskGroup { kind, totals: kind.totals(usage), items })
            })
            .collect();
        let removable: HashSet<Id> = DiskKind::ALL
            .into_iter()
            .filter(|kind| kind.family().is_some())
            .flat_map(|kind| kind.members(usage))
            .map(|item| item.id(usage))
            .collect();
        self.marks.retain(|mark| removable.contains(mark));
        self.rows = self
            .groups
            .iter()
            .enumerate()
            .flat_map(|(index, group)| {
                let open = self.unfolded(group.kind);
                std::iter::once(DiskRow::Kind(index))
                    .chain(group.items.iter().filter(move |_| open).map(move |&item| DiskRow::Item(index, item)))
            })
            .collect();
        let found = self
            .selected
            .clone()
            .and_then(|key| self.rows.iter().position(|&row| self.key_at(row) == Some(key.clone())));
        self.cursor = found.unwrap_or(self.cursor.min(self.rows.len().saturating_sub(1)));
        if found.is_none() {
            self.selected = self.row().and_then(|row| self.key_at(row));
        }
    }

    /// The command the cleanup key types on the selected row: the kind's prune on a kind, and on
    /// an item the removal of the marked items like it, or of this one when none are marked.
    pub fn cleanup(&self) -> Result<CommandLine, NoCleanup> {
        let line = self.command()?;
        if String::from_utf8_lossy(&line.line()).chars().any(char::is_control) {
            return Err(NoCleanup::ControlCharacter);
        }
        Ok(line)
    }

    fn command(&self) -> Result<CommandLine, NoCleanup> {
        let (Some(usage), Some(row), Some(group)) = (&self.usage, self.row(), self.group()) else {
            return Err(NoCleanup::Empty);
        };
        let line = docker(self.context.as_deref());
        match row {
            DiskRow::Kind(_) => group.kind.prune().map(|prune| words(line, prune)).ok_or(NoCleanup::InUse),
            DiskRow::Item(_, item) => {
                let family = match group.kind.family() {
                    Some(family) => family,
                    None if group.kind == DiskKind::BuildCache => return Err(NoCleanup::CacheRecord),
                    None => return Err(NoCleanup::InUse),
                };
                let marked = self.marked_like(usage, family);
                let items = if marked.is_empty() { vec![item] } else { marked };
                let line = words(line, family.removal());
                Ok(items.iter().flat_map(|item| item.removal(usage)).fold(line, CommandLine::word))
            }
        }
    }

    /// The marked items of a family, kind by kind and biggest first, whatever the filter shows.
    fn marked_like(&self, usage: &DiskUse, family: DiskFamily) -> Vec<DiskItem> {
        DiskKind::ALL
            .into_iter()
            .filter(|kind| kind.family() == Some(family))
            .flat_map(|kind| kind.sorted(usage))
            .filter(|&item| self.marks.contains(&item.id(usage)))
            .collect()
    }

    /// How many items the cleanup key removes on the selected row: the marked ones like it, or
    /// this one; none on a kind's row.
    pub fn removing(&self) -> usize {
        let Some(usage) = &self.usage else { return 0 };
        match (self.row(), self.group().and_then(|group| group.kind.family())) {
            (Some(DiskRow::Item(..)), Some(family)) => self.marked_like(usage, family).len().max(1),
            _ => 0,
        }
    }

    fn flip(&mut self, marks: Vec<Id>) {
        for mark in marks {
            if !self.marks.remove(&mark) {
                self.marks.insert(mark);
            }
        }
    }

    /// Marks or unmarks the selected item, if it can be removed on its own.
    fn toggle_mark(&mut self) {
        let removable = self.group().is_some_and(|group| group.kind.family().is_some());
        if let (Some(usage), Some(DiskRow::Item(_, item)), true) = (&self.usage, self.row(), removable) {
            let mark = item.id(usage);
            self.flip(vec![mark]);
        }
    }

    /// Marks the selected kind's unmarked items and unmarks the marked ones.
    fn invert_marks(&mut self) {
        let Some(kind) = self.group().map(|group| group.kind).filter(|kind| kind.family().is_some()) else { return };
        let Some(usage) = &self.usage else { return };
        let marks = kind.members(usage).into_iter().map(|item| item.id(usage)).collect();
        self.flip(marks);
    }
}

pub(super) fn receive(disk: &mut Disk, result: Result<DockerData, String>) {
    match result {
        Ok(DockerData::DiskUse { usage, context, contexts, folders }) => {
            let names: HashSet<&str> = usage.volumes.iter().map(|volume| volume.name.as_str()).collect();
            disk.contents.retain(|name, _| names.contains(name.as_str()) && folders.contains_key(name));
            disk.usage = Some(*usage);
            disk.context = named_context(context, contexts);
            disk.folders = folders;
            disk.error = None;
            disk.rebuild();
        }
        Ok(_) => {}
        Err(error) => disk.error = Some(error),
    }
}

/// One volume's files read, or the end of reading them.
pub(super) fn read(disk: &mut Disk, result: Result<DockerData, String>) {
    match result {
        Ok(DockerData::VolumeRead { name, contents }) => {
            disk.contents.insert(name, contents);
        }
        Ok(_) | Err(_) => disk.reading = None,
    }
}

#[cfg(test)]
mod tests;
