use std::path::Path;

const HOMEBREW_MARKERS: [&str; 3] = ["/Cellar/", "homebrew/", "linuxbrew/"];
const CARGO_MARKER: &str = "/.cargo/bin/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Homebrew,
    Cargo,
    Standalone,
}

impl Channel {
    pub fn upgrade_hint(self) -> Option<&'static str> {
        match self {
            Self::Homebrew => Some("brew upgrade borg-commander"),
            Self::Cargo => Some("cargo install --locked --git https://github.com/0xcd21h/borg-commander borg-app"),
            Self::Standalone => None,
        }
    }
}

pub fn channel(exe: &Path) -> Channel {
    let text = exe.to_string_lossy();
    if HOMEBREW_MARKERS.iter().any(|marker| text.contains(marker)) {
        Channel::Homebrew
    } else if text.contains(CARGO_MARKER) {
        Channel::Cargo
    } else {
        Channel::Standalone
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_package_managers_by_path() {
        assert_eq!(channel(Path::new("/opt/homebrew/Cellar/borg-commander/0.2.0/bin/borg")), Channel::Homebrew);
        assert_eq!(channel(Path::new("/home/me/.linuxbrew/bin/borg")), Channel::Homebrew);
        assert_eq!(channel(Path::new("/home/me/.cargo/bin/borg")), Channel::Cargo);
        assert_eq!(channel(Path::new("/usr/local/bin/borg")), Channel::Standalone);
        assert!(Channel::Standalone.upgrade_hint().is_none());
    }
}
