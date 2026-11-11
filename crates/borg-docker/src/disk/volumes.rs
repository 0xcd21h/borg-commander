//! What `docker volume inspect`, `docker inspect` and `docker info` add to a volume: when it was
//! made, the Compose project it belongs to, its driver, the containers that mount it with their
//! times, and where its files are on this machine, if anywhere.

use anyhow::{Context, Result};
use jiff::Timestamp;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

use crate::State;

/// One line per volume from `docker volume inspect`.
pub(crate) const VOLUME_FORMAT: &str = "{{json .}}";

/// One line per container from `docker inspect`: its state, times, image and mounts.
pub(crate) const CONTAINER_FORMAT: &str = concat!(
    r#"{"name":{{json .Name}},"image":{{json .Config.Image}},"state":{{json .State.Status}},"#,
    r#""started":{{json .State.StartedAt}},"finished":{{json .State.FinishedAt}},"mounts":{{json .Mounts}}}"#,
);

/// The engine's system and host name from `docker info`.
pub(crate) const ENGINE_FORMAT: &str = r#"{"system":{{json .OperatingSystem}},"host":{{json .Name}}}"#;

const COMPOSE_PROJECT: &str = "com.docker.compose.project";
const COMPOSE_VOLUME: &str = "com.docker.compose.volume";
const VOLUME_MOUNT: &str = "volume";
/// Docker names a container with a slash in front.
const NAME_PREFIX: char = '/';
/// What `docker info` calls OrbStack's engine.
const ORBSTACK: &str = "OrbStack";
/// Where OrbStack shares each volume's files in the home folder.
const ORBSTACK_VOLUMES: &str = "OrbStack/docker/volumes";

/// A container that mounts a volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeUser {
    pub container: String,
    pub image: String,
    pub state: State,
    /// Where the container sees the volume.
    pub path: String,
    pub read_only: bool,
    pub started: Option<Timestamp>,
    /// When it last stopped; `None` while it has never stopped.
    pub finished: Option<Timestamp>,
}

/// The Compose project a volume belongs to, and its name in the project's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compose {
    pub project: String,
    pub volume: Option<String>,
}

/// The engine disk use was measured on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Engine {
    /// Its system, such as "OrbStack", "Docker Desktop" or "Ubuntu 24.04.1 LTS".
    pub system: String,
    /// The name of the machine it runs on.
    pub host: String,
}

impl Engine {
    /// OrbStack shares every volume's files in the home folder.
    pub fn orbstack(&self) -> bool {
        self.system == ORBSTACK
    }
}

/// When a volume was last mounted, as its containers tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastUse {
    /// A container that mounts it is up.
    Now,
    /// When the last of the containers that mount it stopped. A container removed since may have
    /// used it later.
    Stopped(Timestamp),
    /// No container that mounts it is left, or none of them ever ran.
    Unknown,
}

/// What `docker volume inspect` says about one volume.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Facts {
    pub created: Option<Timestamp>,
    pub compose: Option<Compose>,
    pub driver: String,
    pub options: Vec<(String, String)>,
    pub mountpoint: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VolumeRow {
    name: String,
    created_at: Option<String>,
    driver: Option<String>,
    labels: Option<BTreeMap<String, String>>,
    options: Option<BTreeMap<String, String>>,
    mountpoint: Option<String>,
}

#[derive(Deserialize)]
struct ContainerRow {
    name: String,
    image: String,
    state: String,
    started: Option<String>,
    finished: Option<String>,
    mounts: Option<Vec<MountRow>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MountRow {
    #[serde(rename = "Type")]
    kind: String,
    name: Option<String>,
    destination: String,
    #[serde(rename = "RW")]
    writable: bool,
}

#[derive(Deserialize)]
struct EngineRow {
    system: String,
    host: String,
}

/// A time as `docker inspect` writes it; Go's zero time, for something that never happened,
/// reads as `None`.
fn instant(text: Option<&str>) -> Option<Timestamp> {
    text?.parse::<Timestamp>().ok().filter(|time| *time > Timestamp::UNIX_EPOCH)
}

fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().filter(|line| !line.trim().is_empty())
}

/// The facts of each volume, by name.
pub(crate) fn parse_volumes(text: &str) -> Result<HashMap<String, Facts>> {
    lines(text)
        .map(|line| {
            let row: VolumeRow = serde_json::from_str(line).context("reading docker volume inspect output")?;
            let labels = row.labels.unwrap_or_default();
            let compose = labels
                .get(COMPOSE_PROJECT)
                .map(|project| Compose { project: project.clone(), volume: labels.get(COMPOSE_VOLUME).cloned() });
            let facts = Facts {
                created: instant(row.created_at.as_deref()),
                compose,
                driver: row.driver.unwrap_or_default(),
                options: row.options.unwrap_or_default().into_iter().collect(),
                mountpoint: row.mountpoint.filter(|path| !path.is_empty()).map(PathBuf::from),
            };
            Ok((row.name, facts))
        })
        .collect()
}

/// The containers that mount each volume, by the volume's name, in the order Docker lists them.
pub(crate) fn parse_users(text: &str) -> Result<HashMap<String, Vec<VolumeUser>>> {
    let mut users: HashMap<String, Vec<VolumeUser>> = HashMap::new();
    for line in lines(text) {
        let row: ContainerRow = serde_json::from_str(line).context("reading docker inspect output")?;
        let state = State::parse(&row.state);
        let (started, finished) = (instant(row.started.as_deref()), instant(row.finished.as_deref()));
        let container = row.name.trim_start_matches(NAME_PREFIX).to_owned();
        for mount in row.mounts.unwrap_or_default().into_iter().filter(|mount| mount.kind == VOLUME_MOUNT) {
            let Some(volume) = mount.name else { continue };
            users.entry(volume).or_default().push(VolumeUser {
                container: container.clone(),
                image: row.image.clone(),
                state,
                path: mount.destination,
                read_only: !mount.writable,
                started,
                finished,
            });
        }
    }
    Ok(users)
}

pub(crate) fn parse_engine(text: &str) -> Result<Engine> {
    let row: EngineRow = serde_json::from_str(text.trim()).context("reading docker info output")?;
    Ok(Engine { system: row.system, host: row.host })
}

/// When the containers in `users` last had the volume mounted.
pub(crate) fn last_use(users: &[VolumeUser]) -> LastUse {
    if users.iter().any(|user| user.state.is_up()) {
        return LastUse::Now;
    }
    users.iter().filter_map(|user| user.finished).max().map_or(LastUse::Unknown, LastUse::Stopped)
}

/// Where a volume's files are on the machine Borg runs on: OrbStack's share in the home folder,
/// or the volume's own folder when the engine runs on this machine. Whether Borg may read them
/// there is for the caller to find out.
pub(crate) fn folder(
    name: &str,
    mountpoint: Option<&Path>,
    engine: &Engine,
    home: &Path,
    host: &str,
) -> Option<PathBuf> {
    if engine.orbstack() {
        return Some(home.join(ORBSTACK_VOLUMES).join(name));
    }
    mountpoint.filter(|_| !engine.host.is_empty() && engine.host == host).map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VOLUMES: &str = include_str!("../../tests/fixtures/volume-inspect.jsonl");
    const CONTAINERS: &str = include_str!("../../tests/fixtures/container-inspect.jsonl");

    fn time(text: &str) -> Option<Timestamp> {
        Some(text.parse().unwrap())
    }

    #[test]
    fn volumes_carry_their_creation_compose_project_and_driver() {
        let facts = parse_volumes(VOLUMES).unwrap();
        let data = &facts["shop_data"];
        assert_eq!(data.created, time("2026-09-14T08:02:11Z"));
        assert_eq!(data.compose, Some(Compose { project: "shop".into(), volume: Some("data".into()) }));
        assert_eq!(data.driver, "local");
        assert_eq!(data.mountpoint.as_deref(), Some(Path::new("/var/lib/docker/volumes/shop_data/_data")));
        let uploads = &facts["blog_uploads"];
        assert_eq!(uploads.created, time("2026-06-13T10:09:31Z"), "an offset reads as the same moment");
        let notes = &facts["scratch-notes"];
        assert_eq!((notes.compose.as_ref(), notes.created), (None, None));
        assert_eq!(notes.options, [("device".into(), ":/export/notes".into()), ("type".into(), "nfs".into())]);
    }

    #[test]
    fn containers_tell_which_volumes_they_mount_where_and_when() {
        let users = parse_users(CONTAINERS).unwrap();
        let db = &users["shop_data"][0];
        assert_eq!(
            (db.container.as_str(), db.path.as_str(), db.read_only),
            ("shop-db-1", "/var/lib/postgresql/data", false)
        );
        assert_eq!((db.state, db.finished), (State::Running, None), "a container that never stopped has no finish");
        let worker = &users["7c2f8e1d0a9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a1b0c9d8e7f6a5b4c3d"][0];
        assert_eq!((worker.image.as_str(), worker.state), ("shop-web:1.4", State::Exited));
        assert_eq!(worker.finished, time("2026-11-07T17:48:03.5Z"));
        let statics: Vec<(&str, bool)> =
            users["shop_static"].iter().map(|user| (user.container.as_str(), user.read_only)).collect();
        assert_eq!(statics, [("shop-web-1", true), ("shop-migrate-1", false)]);
        assert!(!users.contains_key("/home/me/projects/shop/init.sql"), "bind mounts are not volumes");
    }

    #[test]
    fn a_volume_was_last_used_when_its_last_container_stopped() {
        let users = parse_users(CONTAINERS).unwrap();
        assert_eq!(last_use(&users["shop_data"]), LastUse::Now);
        assert_eq!(last_use(&users["shop_static"]), LastUse::Now, "one running container is enough");
        let worker = &users["7c2f8e1d0a9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a1b0c9d8e7f6a5b4c3d"];
        assert_eq!(last_use(worker), LastUse::Stopped("2026-11-07T17:48:03.5Z".parse().unwrap()));
        assert_eq!(last_use(&users["blog_cache"]), LastUse::Unknown, "a container that never ran tells nothing");
        assert_eq!(last_use(&[]), LastUse::Unknown);
    }

    #[test]
    fn files_are_reachable_through_orbstack_or_on_the_engine_s_own_machine() {
        let home = Path::new("/home/me");
        let mountpoint = Some(Path::new("/var/lib/docker/volumes/shop_data/_data"));
        let orbstack = Engine { system: "OrbStack".into(), host: "orbstack".into() };
        assert_eq!(
            folder("shop_data", mountpoint, &orbstack, home, "mac"),
            Some(PathBuf::from("/home/me/OrbStack/docker/volumes/shop_data"))
        );
        let local = Engine { system: "Ubuntu 24.04.1 LTS".into(), host: "box".into() };
        assert_eq!(folder("shop_data", mountpoint, &local, home, "box").as_deref(), mountpoint);
        assert_eq!(
            folder("shop_data", mountpoint, &local, home, "laptop"),
            None,
            "a remote engine's folders are not here"
        );
        let desktop = Engine { system: "Docker Desktop".into(), host: "docker-desktop".into() };
        assert_eq!(folder("shop_data", mountpoint, &desktop, home, "mac"), None);
        assert_eq!(folder("shop_data", None, &local, home, "box"), None);
    }

    #[test]
    fn the_engine_names_its_system_and_host() {
        let engine = parse_engine(r#"{"system":"OrbStack","host":"orbstack"}"#).unwrap();
        assert_eq!(engine, Engine { system: "OrbStack".into(), host: "orbstack".into() });
    }
}
