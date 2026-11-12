use std::time::Duration;

use serde::Deserialize;
use ureq::tls::{RootCerts, TlsConfig};

use crate::{UpdateError, Version};

pub const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/0xcd21h/borg-commander/releases/latest";
/// Newest first. A copy that cannot check the latest release looks here for the newest one it can.
pub const RELEASES_URL: &str = "https://api.github.com/repos/0xcd21h/borg-commander/releases?per_page=30";
const USER_AGENT_HEADER: &str = "User-Agent";
const USER_AGENT: &str = concat!("borg-commander/", env!("CARGO_PKG_VERSION"));
const ACCEPT_HEADER: &str = "Accept";
const GITHUB_JSON: &str = "application/vnd.github+json";
const NOT_FOUND: u16 = 404;
/// Where every file of a release is downloaded from. An address anywhere else is refused, so
/// Borg only ever installs what this project published.
const RELEASE_DOWNLOADS: &str = "https://github.com/0xcd21h/borg-commander/releases/download/";
const RELEASE_METADATA_LIMIT: u64 = 1024 * 1024;
const RELEASE_LIST_LIMIT: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Raw {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

pub fn parse_release(json: &[u8]) -> Result<Option<Release>, UpdateError> {
    let raw: Raw = serde_json::from_slice(json).map_err(|e| UpdateError::Release(e.to_string()))?;
    if raw.draft || raw.prerelease {
        return Ok(None);
    }
    if let Some(elsewhere) = raw.assets.iter().find(|asset| !asset.url.starts_with(RELEASE_DOWNLOADS)) {
        return Err(UpdateError::Release(format!("{} is not among this project's downloads", elsewhere.name)));
    }
    Ok(Some(Release { version: raw.tag_name.parse()?, assets: raw.assets }))
}

/// How long Borg waits to reach the server, for it to start answering, and for a whole download.
const CONNECT_WAIT: Duration = Duration::from_secs(15);
const ANSWER_WAIT: Duration = Duration::from_secs(30);
const DOWNLOAD_WAIT: Duration = Duration::from_secs(15 * 60);

fn agent() -> ureq::Agent {
    let tls = TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_connect(Some(CONNECT_WAIT))
        .timeout_recv_response(Some(ANSWER_WAIT))
        .timeout_global(Some(DOWNLOAD_WAIT))
        .build()
        .new_agent()
}

pub(crate) fn fetch(url: &str, limit: u64) -> Result<Vec<u8>, UpdateError> {
    let mut response =
        agent().get(url).header(USER_AGENT_HEADER, USER_AGENT).header(ACCEPT_HEADER, GITHUB_JSON).call().map_err(
            |e| match e {
                ureq::Error::StatusCode(NOT_FOUND) => UpdateError::NoRelease,
                other => UpdateError::Network(other.to_string()),
            },
        )?;
    response.body_mut().with_config().limit(limit).read_to_vec().map_err(|e| UpdateError::Network(e.to_string()))
}

pub fn latest(url: &str, current: Version) -> Result<Option<Release>, UpdateError> {
    match fetch(url, RELEASE_METADATA_LIMIT) {
        Ok(json) => Ok(parse_release(&json)?.filter(|release| release.version > current)),
        Err(UpdateError::NoRelease) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Reads each entry with `parse_release`, so a list is held to the same rules as one release.
/// Drafts, prereleases and entries that do not read as a release are left out.
pub(crate) fn parse_releases(json: &[u8]) -> Result<Vec<Release>, UpdateError> {
    let entries: Vec<serde_json::Value> =
        serde_json::from_slice(json).map_err(|e| UpdateError::Release(e.to_string()))?;
    let release = |entry: &serde_json::Value| parse_release(&serde_json::to_vec(entry).ok()?).ok().flatten();
    Ok(entries.iter().filter_map(release).collect())
}

pub(crate) fn releases(url: &str) -> Result<Vec<Release>, UpdateError> {
    parse_releases(&fetch(url, RELEASE_LIST_LIMIT)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "tag_name": "v0.3.1",
        "draft": false,
        "prerelease": false,
        "assets": [
            { "name": "borg-0.3.1-x86_64-apple-darwin.tar.gz", "browser_download_url": "https://github.com/0xcd21h/borg-commander/releases/download/v0.3.1/a.tar.gz" },
            { "name": "borg-0.3.1-x86_64-apple-darwin.tar.gz.sha256", "browser_download_url": "https://github.com/0xcd21h/borg-commander/releases/download/v0.3.1/a.sha256" }
        ]
    }"#;

    #[test]
    fn reads_github_release_json() {
        let release =
            parse_release(SAMPLE.as_bytes()).unwrap_or_else(|e| panic!("{e}")).unwrap_or_else(|| panic!("release"));
        assert_eq!(release.version.to_string(), "0.3.1");
        assert_eq!(release.assets.len(), 2);
        assert!(release.assets[0].url.starts_with(RELEASE_DOWNLOADS));
    }

    #[test]
    fn a_release_that_points_anywhere_but_this_projects_downloads_is_refused() {
        for elsewhere in [
            "https://example.test/a.tar.gz",
            "http://github.com/0xcd21h/borg-commander/releases/download/v0.3.1/a.tar.gz",
            "https://github.com/someone-else/borg-commander/releases/download/v0.3.1/a.tar.gz",
            "https://github.com.example.test/0xcd21h/borg-commander/releases/download/v0.3.1/a.tar.gz",
        ] {
            let moved = SAMPLE
                .replace("https://github.com/0xcd21h/borg-commander/releases/download/v0.3.1/a.tar.gz", elsewhere);
            assert!(matches!(parse_release(moved.as_bytes()), Err(UpdateError::Release(_))), "{elsewhere}");
        }
    }

    #[test]
    fn ignores_prereleases() {
        let pre = SAMPLE.replace(r#""prerelease": false"#, r#""prerelease": true"#);
        assert_eq!(parse_release(pre.as_bytes()).ok().flatten(), None);
    }

    #[test]
    fn reads_a_release_list_and_leaves_out_what_is_not_a_release() {
        let pre = SAMPLE.replace(r#""prerelease": false"#, r#""prerelease": true"#).replace("v0.3.1", "v0.4.0");
        let odd = SAMPLE.replace("v0.3.1", "nightly");
        let list = format!("[{}, {pre}, {odd}, {SAMPLE}]", SAMPLE.replace("v0.3.1", "v0.3.2"));
        let versions: Vec<String> = parse_releases(list.as_bytes())
            .unwrap_or_else(|e| panic!("{e}"))
            .iter()
            .map(|release| release.version.to_string())
            .collect();
        assert_eq!(versions, ["0.3.2", "0.3.1"]);
    }
}
