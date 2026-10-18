use std::{fmt, str::FromStr};

use crate::UpdateError;

const TAG_PREFIX: char = 'v';
const PRERELEASE: char = '-';
const PARTS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl FromStr for Version {
    type Err = UpdateError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bare = text.trim().trim_start_matches(TAG_PREFIX);
        let core = bare.split(PRERELEASE).next().unwrap_or(bare);
        let parts: Vec<u64> = core
            .split('.')
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|_| UpdateError::Version(text.to_owned()))?;
        match parts.as_slice() {
            [major, minor, patch] if parts.len() == PARTS => Ok(Self { major: *major, minor: *minor, patch: *patch }),
            _ => Err(UpdateError::Version(text.to_owned())),
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tags_and_orders_versions() {
        let parse = |text: &str| text.parse::<Version>().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(parse("v1.2.3"), Version { major: 1, minor: 2, patch: 3 });
        assert_eq!(parse("0.10.0-beta.1"), Version { major: 0, minor: 10, patch: 0 });
        assert!(parse("0.10.0") > parse("0.9.12"));
        assert_eq!(parse("1.0.0").to_string(), "1.0.0");
        assert!("1.2".parse::<Version>().is_err());
        assert!("latest".parse::<Version>().is_err());
    }
}
