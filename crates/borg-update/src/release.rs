use serde::Deserialize;
use ureq::tls::{RootCerts, TlsConfig};

use crate::{UpdateError, Version};

pub const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/0xcd21h/borg-commander/releases/latest";
const USER_AGENT_HEADER: &str = "User-Agent";
const USER_AGENT: &str = concat!("borg-commander/", env!("CARGO_PKG_VERSION"));
const ACCEPT_HEADER: &str = "Accept";
const GITHUB_JSON: &str = "application/vnd.github+json";
const NOT_FOUND: u16 = 404;

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
    Ok(Some(Release { version: raw.tag_name.parse()?, assets: raw.assets }))
}

fn agent() -> ureq::Agent {
    let tls = TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder().tls_config(tls).build().new_agent()
}

pub(crate) fn fetch(url: &str) -> Result<Vec<u8>, UpdateError> {
    let mut response =
        agent().get(url).header(USER_AGENT_HEADER, USER_AGENT).header(ACCEPT_HEADER, GITHUB_JSON).call().map_err(
            |e| match e {
                ureq::Error::StatusCode(NOT_FOUND) => UpdateError::NoRelease,
                other => UpdateError::Network(other.to_string()),
            },
        )?;
    response.body_mut().read_to_vec().map_err(|e| UpdateError::Network(e.to_string()))
}

pub fn latest(url: &str, current: Version) -> Result<Option<Release>, UpdateError> {
    match fetch(url) {
        Ok(json) => Ok(parse_release(&json)?.filter(|release| release.version > current)),
        Err(UpdateError::NoRelease) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "tag_name": "v0.3.1",
        "draft": false,
        "prerelease": false,
        "assets": [
            { "name": "borg-0.3.1-x86_64-apple-darwin.tar.gz", "browser_download_url": "https://example.test/a.tar.gz" },
            { "name": "borg-0.3.1-x86_64-apple-darwin.tar.gz.sha256", "browser_download_url": "https://example.test/a.sha256" }
        ]
    }"#;

    #[test]
    fn reads_github_release_json() {
        let release =
            parse_release(SAMPLE.as_bytes()).unwrap_or_else(|e| panic!("{e}")).unwrap_or_else(|| panic!("release"));
        assert_eq!(release.version.to_string(), "0.3.1");
        assert_eq!(release.assets.len(), 2);
        assert_eq!(release.assets[0].url, "https://example.test/a.tar.gz");
    }

    #[test]
    fn ignores_prereleases() {
        let pre = SAMPLE.replace(r#""prerelease": false"#, r#""prerelease": true"#);
        assert_eq!(parse_release(pre.as_bytes()).ok().flatten(), None);
    }
}
