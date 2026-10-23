//! Where a path an agent names lives: on the machine Borg runs on, or inside a container.
//! Every tool reads and writes locations the same way, and `screen` reports the panels as
//! locations, so an agent can use what it sees as it is.

use std::path::{Component, Path, PathBuf};

pub const DOCKER_SCHEME: &str = "docker://";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Host(PathBuf),
    /// Inside a container, named as the agent or a panel names it (a name, Compose service
    /// or id), at an absolute path.
    Container {
        container: String,
        path: PathBuf,
    },
}

impl Location {
    /// Reads a location: `docker://web/etc/nginx` is in a container, an absolute path or
    /// `~/…` is on this machine, and a relative path continues from `base`, the active
    /// panel's location.
    pub fn parse(text: &str, base: &Location, home: Option<&Path>) -> Self {
        let text = text.trim();
        if let Some(rest) = text.strip_prefix(DOCKER_SCHEME) {
            let (container, path) = rest.split_once('/').map_or((rest, "/"), |(name, path)| (name, path));
            let path = if path.starts_with('/') { PathBuf::from(path) } else { Path::new("/").join(path) };
            return Self::Container { container: container.to_owned(), path: clean(&path) };
        }
        if let (Some(rest), Some(home)) = (text.strip_prefix('~'), home)
            && (rest.is_empty() || rest.starts_with('/'))
        {
            return Self::Host(clean(&home.join(rest.trim_start_matches('/'))));
        }
        if text.starts_with('/') {
            return Self::Host(clean(Path::new(text)));
        }
        match base {
            Self::Host(dir) => Self::Host(clean(&dir.join(text))),
            Self::Container { container, path } => {
                Self::Container { container: container.clone(), path: clean(&path.join(text)) }
            }
        }
    }
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Host(path) => write!(f, "{}", path.display()),
            Self::Container { container, path } => write!(f, "{DOCKER_SCHEME}{container}{}", path.display()),
        }
    }
}

/// A path without `.` and with `..` applied, so the agent reads back the path it means.
pub fn clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_are_host_paths_container_uris_or_relative_to_the_active_panel() {
        let home = Path::new("/home/deploy");
        let host = Location::Host(PathBuf::from("/srv/app"));
        let web = Location::Container { container: "web".into(), path: PathBuf::from("/etc") };
        let parse = |text, base| Location::parse(text, base, Some(home));

        assert_eq!(parse("logs/../conf/./app.toml", &host), Location::Host("/srv/app/conf/app.toml".into()));
        assert_eq!(parse("/var/log", &web), Location::Host("/var/log".into()), "an absolute path is on the host");
        assert_eq!(parse("~/notes.txt", &host), Location::Host("/home/deploy/notes.txt".into()));
        assert_eq!(
            parse("nginx/nginx.conf", &web),
            Location::Container { container: "web".into(), path: "/etc/nginx/nginx.conf".into() }
        );
        let uri = parse("docker://shop-web-1/app/../srv", &host);
        assert_eq!(uri, Location::Container { container: "shop-web-1".into(), path: "/srv".into() });
        assert_eq!(uri.to_string(), "docker://shop-web-1/srv");
        assert_eq!(parse("docker://web", &host).to_string(), "docker://web/");
    }
}
