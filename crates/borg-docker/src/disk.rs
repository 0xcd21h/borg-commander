//! What the engine keeps on disk, as `docker system df --verbose` reports it, with every tag of
//! each image and the networks no container uses.

use anyhow::{Context, Result};
use jiff::Timestamp;
use serde::Deserialize;
use std::collections::HashMap;

use crate::{State, parse_size};

/// One line per tag, with the image's full ID, from `docker image ls`.
pub(crate) const TAGS_FORMAT: &str = r#"{"id":{{json .ID}},"repository":{{json .Repository}},"tag":{{json .Tag}}}"#;

/// One line per network from `docker network inspect`; Swarm's ingress network is never pruned.
pub(crate) const NETWORK_FORMAT: &str = r#"{"name":{{json .Name}},"ingress":{{json .Ingress}}}"#;

/// How Docker writes a repository or tag that is not there.
const MISSING: &str = "<none>";
const ID_PREFIX: &str = "sha256:";
const SHORT_ID: usize = 12;
/// The label Docker puts on the volumes it creates for a container's unnamed mounts.
const ANONYMOUS_LABEL: &str = "com.docker.volume.anonymous";
const LABEL_SEPARATOR: char = ',';
const LABEL_VALUE: char = '=';
const MOUNT_SEPARATOR: char = ',';
const TRUE: &str = "true";
/// Docker's CLI prints times as Go does, "2026-10-20 09:10:02.5 +0000 UTC", in the CLI's zone
/// or in UTC; the abbreviation at the end adds nothing to the offset.
const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.f %z";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub id: String,
    /// Every `repository:tag` that names it.
    pub tags: Vec<String>,
    /// A repository that names it without a tag, such as a pull by digest.
    pub repository: Option<String>,
    pub size: Option<u64>,
    /// The part of `size` other images use too.
    pub shared: Option<u64>,
    /// The part of `size` only this image uses, which removing it frees.
    pub unique: Option<u64>,
    pub created: Option<Timestamp>,
    /// The containers made from it, running or not; `None` when Docker did not count them.
    pub containers: Option<u32>,
}

impl Image {
    /// The ID as `docker image ls` shows it.
    pub fn short_id(&self) -> &str {
        let id = self.id.strip_prefix(ID_PREFIX).unwrap_or(&self.id);
        id.get(..SHORT_ID).unwrap_or(id)
    }

    /// Nothing names it any more, as with an older build whose tag moved to a newer one.
    pub fn dangling(&self) -> bool {
        self.tags.is_empty() && self.repository.is_none()
    }

    pub fn in_use(&self) -> bool {
        self.containers != Some(0)
    }

    /// Its first tag, its repository, or its ID.
    pub fn label(&self) -> &str {
        self.tags.first().or(self.repository.as_ref()).map_or_else(|| self.short_id(), String::as_str)
    }

    /// What `docker rmi` takes to remove it: every tag, since removing the last one removes the
    /// image, or else its ID.
    pub fn removal(&self) -> Vec<&str> {
        if self.tags.is_empty() { vec![self.short_id()] } else { self.tags.iter().map(String::as_str).collect() }
    }
}

/// A container's own layer: what it wrote on top of its image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: State,
    /// Docker's own summary, such as "Exited (0) 3 days ago".
    pub status: String,
    pub size: Option<u64>,
}

impl Layer {
    /// What `docker container prune` removes: containers that are not running, paused or
    /// restarting.
    pub fn stopped(&self) -> bool {
        matches!(self.state, State::Created | State::Exited | State::Dead)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    pub name: String,
    /// `None` when Docker could not measure it.
    pub size: Option<u64>,
    /// The containers that mount it, running or not; `None` when Docker did not count them.
    pub links: Option<u32>,
    /// Docker made it for a container's unnamed mount, so `docker volume prune` removes it
    /// without `--all`.
    pub anonymous: bool,
    /// The names of the containers that mount it.
    pub users: Vec<String>,
}

impl Volume {
    pub fn in_use(&self) -> bool {
        self.links != Some(0)
    }
}

/// A record in BuildKit's cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheRecord {
    pub id: String,
    /// BuildKit's type, such as `regular`, `exec.cachemount` or `source.local`.
    pub kind: String,
    pub size: Option<u64>,
    /// Images or other records hold the same data, so removing it frees nothing.
    pub shared: bool,
    pub in_use: bool,
    pub last_used: Option<Timestamp>,
    pub description: String,
}

impl CacheRecord {
    /// Pruning frees its bytes: nothing uses it and nothing shares them.
    pub fn reclaimable(&self) -> bool {
        !self.in_use && !self.shared
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiskUse {
    pub images: Vec<Image>,
    pub containers: Vec<Layer>,
    pub volumes: Vec<Volume>,
    pub cache: Vec<CacheRecord>,
    /// The networks no container is connected to, which `docker network prune` removes.
    pub networks: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Report {
    images: Option<Vec<ImageRow>>,
    containers: Option<Vec<ContainerRow>>,
    volumes: Option<Vec<VolumeRow>>,
    build_cache: Option<Vec<CacheRow>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ImageRow {
    #[serde(rename = "ID")]
    id: String,
    repository: String,
    tag: String,
    size: String,
    shared_size: String,
    unique_size: String,
    created_at: String,
    containers: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ContainerRow {
    #[serde(rename = "ID")]
    id: String,
    names: String,
    image: String,
    state: String,
    status: String,
    size: String,
    mounts: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VolumeRow {
    name: String,
    size: String,
    links: String,
    labels: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CacheRow {
    #[serde(rename = "ID")]
    id: String,
    cache_type: String,
    size: String,
    shared: String,
    in_use: String,
    last_used_at: String,
    description: String,
}

#[derive(Deserialize)]
struct TagRow {
    id: String,
    repository: String,
    tag: String,
}

#[derive(Deserialize)]
struct NetworkRow {
    name: String,
    ingress: bool,
}

fn named(value: &str) -> Option<&str> {
    (value != MISSING).then_some(value)
}

fn moment(text: &str) -> Option<Timestamp> {
    let (time, _zone) = text.rsplit_once(' ')?;
    jiff::fmt::strtime::parse(TIME_FORMAT, time).ok()?.to_timestamp().ok()
}

fn anonymous(labels: &str) -> bool {
    labels.split(LABEL_SEPARATOR).any(|label| label.split(LABEL_VALUE).next() == Some(ANONYMOUS_LABEL))
}

/// An image's tags, and a repository that names it without a tag.
type Names = (Vec<String>, Option<String>);

fn add_name(names: &mut Names, repository: &str, tag: &str) {
    match (named(repository), named(tag)) {
        (Some(name), Some(tag)) => names.0.push(format!("{name}:{tag}")),
        (Some(name), None) => {
            names.1.get_or_insert_with(|| name.to_owned());
        }
        (None, _) => {}
    }
}

/// The names of each image ID, in the order Docker lists them.
fn names(text: &str) -> Result<HashMap<String, Names>> {
    let mut names: HashMap<String, Names> = HashMap::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let row: TagRow = serde_json::from_str(line).context("reading docker image ls output")?;
        add_name(names.entry(row.id).or_default(), &row.repository, &row.tag);
    }
    Ok(names)
}

pub(crate) fn parse(report: &str, tags: &str, networks: Vec<String>) -> Result<DiskUse> {
    let report: Report = serde_json::from_str(report.trim()).context("reading docker system df output")?;
    let mut names = names(tags)?;
    let images = report
        .images
        .unwrap_or_default()
        .into_iter()
        .map(|row| {
            let (tags, repository) = names.remove(&row.id).unwrap_or_else(|| {
                let mut own = Names::default();
                add_name(&mut own, &row.repository, &row.tag);
                own
            });
            Image {
                tags,
                repository,
                size: parse_size(&row.size),
                shared: parse_size(&row.shared_size),
                unique: parse_size(&row.unique_size),
                created: moment(&row.created_at),
                containers: row.containers.parse().ok(),
                id: row.id,
            }
        })
        .collect();
    let rows = report.containers.unwrap_or_default();
    let mut users: HashMap<&str, Vec<String>> = HashMap::new();
    for row in &rows {
        for mount in row.mounts.split(MOUNT_SEPARATOR).filter(|mount| !mount.is_empty()) {
            users.entry(mount).or_default().push(row.names.clone());
        }
    }
    let volumes = report
        .volumes
        .unwrap_or_default()
        .into_iter()
        .map(|row| Volume {
            users: users.get(row.name.as_str()).cloned().unwrap_or_default(),
            size: parse_size(&row.size),
            links: row.links.parse().ok(),
            anonymous: anonymous(&row.labels),
            name: row.name,
        })
        .collect();
    let containers = rows
        .iter()
        .map(|row| Layer {
            id: row.id.clone(),
            name: row.names.clone(),
            image: row.image.clone(),
            state: State::parse(&row.state),
            status: row.status.clone(),
            size: parse_size(&row.size),
        })
        .collect();
    let cache = report
        .build_cache
        .unwrap_or_default()
        .into_iter()
        .map(|row| CacheRecord {
            size: parse_size(&row.size),
            shared: row.shared == TRUE,
            in_use: row.in_use == TRUE,
            last_used: moment(&row.last_used_at),
            id: row.id,
            kind: row.cache_type,
            description: row.description,
        })
        .collect();
    Ok(DiskUse { images, containers, volumes, cache, networks })
}

/// The names of the networks `docker network prune` would remove among those inspected.
pub(crate) fn parse_networks(text: &str) -> Result<Vec<String>> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<NetworkRow>(line).context("reading docker network inspect output"))
        .filter_map(|row| row.map(|row| (!row.ingress).then_some(row.name)).transpose())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = include_str!("../tests/fixtures/system-df.json");
    const TAGS: &str = include_str!("../tests/fixtures/image-tags.jsonl");
    const NETWORKS: &str = include_str!("../tests/fixtures/network-inspect.jsonl");

    fn usage() -> DiskUse {
        parse(REPORT, TAGS, parse_networks(NETWORKS).unwrap()).unwrap()
    }

    fn image<'a>(usage: &'a DiskUse, label: &str) -> &'a Image {
        usage.images.iter().find(|image| image.label() == label).unwrap()
    }

    #[test]
    fn images_carry_every_tag_and_what_only_they_hold() {
        let usage = usage();
        assert_eq!(usage.images.len(), 7);
        let web = image(&usage, "shop-web:1.4");
        assert_eq!(web.tags, ["shop-web:1.4", "shop-web:latest"]);
        assert_eq!(web.removal(), ["shop-web:1.4", "shop-web:latest"]);
        assert_eq!((web.size, web.shared, web.unique), (Some(212_000_000), Some(74_800_000), Some(137_200_000)));
        assert_eq!((web.containers, web.in_use(), web.dangling()), (Some(3), true, false));
        assert_eq!(web.created, Some("2026-10-20T09:10:02Z".parse().unwrap()));
        let old = image(&usage, "7f3e2d1c0b9a");
        assert!(old.dangling() && !old.in_use());
        assert_eq!(old.removal(), ["7f3e2d1c0b9a"]);
    }

    #[test]
    fn an_image_named_only_by_its_repository_is_removed_by_id() {
        let usage = usage();
        let pulled = image(&usage, "mirror.example/tools/jq");
        assert!(!pulled.dangling() && !pulled.in_use());
        assert_eq!(pulled.removal(), ["d5c1b0a9f8e7"]);
        assert_eq!((pulled.size, pulled.unique), (None, None), "sizes Docker did not measure stay unknown");
        let kept = image(&usage, "e2a9f8b7c6d5");
        assert!(kept.dangling() && kept.in_use(), "a stopped container still holds its untagged image");
    }

    #[test]
    fn stopped_containers_are_the_ones_prune_removes() {
        let usage = usage();
        let stopped: Vec<(&str, Option<u64>)> =
            usage.containers.iter().filter(|c| c.stopped()).map(|c| (c.name.as_str(), c.size)).collect();
        assert_eq!(
            stopped,
            [("shop-worker-1", Some(48_200_000)), ("scratch", Some(1500)), ("shop-migrate-1", Some(0))]
        );
    }

    #[test]
    fn volumes_know_who_mounts_them_and_which_are_anonymous() {
        let usage = usage();
        let volume = |name: &str| usage.volumes.iter().find(|v| v.name == name).unwrap();
        assert_eq!(volume("shop_data").users, ["shop-db-1"]);
        assert!(volume("shop_data").in_use() && !volume("shop_data").anonymous);
        assert!(!volume("blog_uploads").in_use() && volume("blog_uploads").users.is_empty());
        assert_eq!(volume("scratch-notes").size, None);
        let anonymous: Vec<(bool, Vec<String>)> =
            usage.volumes.iter().filter(|v| v.anonymous).map(|v| (v.in_use(), v.users.clone())).collect();
        assert_eq!(anonymous, [(false, Vec::new()), (true, vec!["shop-worker-1".to_owned()])]);
    }

    #[test]
    fn build_cache_frees_only_what_nothing_uses_or_shares() {
        let usage = usage();
        let free: Vec<&str> = usage.cache.iter().filter(|r| r.reclaimable()).map(|r| r.kind.as_str()).collect();
        assert_eq!(free, ["regular", "exec.cachemount", "frontend"]);
        let local = usage.cache.iter().find(|r| r.in_use).unwrap();
        assert_eq!((local.last_used, local.size), (None, Some(1_200_000)));
        let first = &usage.cache[0];
        assert_eq!(first.last_used, Some("2026-11-07T16:05:42.118302551Z".parse().unwrap()));
    }

    #[test]
    fn swarm_ingress_is_not_an_unused_network() {
        assert_eq!(usage().networks, ["blog_default", "shop_backend"]);
        assert!(parse_networks("").unwrap().is_empty());
    }

    #[test]
    fn an_empty_engine_reports_nothing() {
        let empty = r#"{"Images":[],"Containers":[],"Volumes":[],"BuildCache":null}"#;
        assert_eq!(parse(empty, "", Vec::new()).unwrap(), DiskUse::default());
    }
}
