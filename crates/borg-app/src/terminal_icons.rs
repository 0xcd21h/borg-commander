use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const ENV_TERM_PROGRAM: &str = "TERM_PROGRAM";
const ENV_TERM: &str = "TERM";
const ENV_SSH: [&str; 2] = ["SSH_CONNECTION", "SSH_TTY"];

/// Nerd Font symbols are built in (kitty since 0.36), so the font does not matter.
const BUNDLED_PROGRAMS: [&str; 2] = ["ghostty", "WezTerm"];
const BUNDLED_TERMS: [&str; 2] = ["xterm-ghostty", "xterm-kitty"];
const BUNDLED_MARKERS: [&str; 2] = ["KITTY_WINDOW_ID", "WEZTERM_PANE"];

/// A glyph missing from the font is drawn with another installed font.
const FALLBACK_TERMS: [&str; 3] = ["alacritty", "foot", "foot-extra"];
const FALLBACK_MARKERS: [&str; 3] = ["ALACRITTY_WINDOW_ID", "KONSOLE_VERSION", "VTE_VERSION"];

const FONT_LIST: &str = "fc-list";
const FONT_LIST_QUIET: &str = "--quiet";
const FONT_LIST_TIMEOUT: Duration = Duration::from_secs(1);
const FONT_LIST_POLL: Duration = Duration::from_millis(5);

pub fn drawable(var: impl Fn(&str) -> Option<String>, installed: impl FnOnce() -> bool) -> bool {
    let program = var(ENV_TERM_PROGRAM);
    let term = var(ENV_TERM);
    let is = |programs: &[&str], terms: &[&str], markers: &[&str]| {
        program.as_deref().is_some_and(|program| programs.iter().any(|known| program.eq_ignore_ascii_case(known)))
            || term.as_deref().is_some_and(|term| terms.contains(&term))
            || markers.iter().any(|marker| var(marker).is_some())
    };
    if is(&BUNDLED_PROGRAMS, &BUNDLED_TERMS, &BUNDLED_MARKERS) {
        return true;
    }
    // Over SSH the terminal draws with the client's fonts, not the ones installed here.
    let remote = ENV_SSH.iter().any(|name| var(name).is_some());
    !remote && is(&[], &FALLBACK_TERMS, &FALLBACK_MARKERS) && installed()
}

pub fn installed(glyphs: &[&str]) -> bool {
    let codepoints: Vec<String> =
        glyphs.iter().flat_map(|glyph| glyph.chars()).map(|c| format!("{:x}", u32::from(c))).collect();
    let Ok(mut list) = Command::new(FONT_LIST)
        .arg(FONT_LIST_QUIET)
        .arg(format!(":charset={}", codepoints.join(" ")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + FONT_LIST_TIMEOUT;
    loop {
        match list.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(FONT_LIST_POLL),
            _ => {
                let _ = list.kill();
                let _ = list.wait();
                return false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| pairs.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).to_owned())
    }

    fn unasked() -> bool {
        panic!("a terminal with its own icons, or one that never borrows glyphs, needs no font check")
    }

    #[test]
    fn terminals_with_built_in_icons_need_no_font() {
        assert!(drawable(env(&[("TERM_PROGRAM", "ghostty"), ("TERM", "xterm-256color")]), unasked));
        assert!(drawable(env(&[("TERM", "xterm-ghostty")]), unasked));
        assert!(drawable(env(&[("TERM", "xterm-kitty")]), unasked));
        assert!(drawable(env(&[("TERM", "xterm-256color"), ("KITTY_WINDOW_ID", "1")]), unasked));
        assert!(drawable(env(&[("TERM_PROGRAM", "WezTerm")]), unasked));
        assert!(drawable(env(&[("TERM", "tmux-256color"), ("WEZTERM_PANE", "0")]), unasked));
    }

    #[test]
    fn built_in_icons_reach_over_ssh() {
        assert!(drawable(env(&[("TERM", "xterm-kitty"), ("SSH_TTY", "/dev/pts/1")]), unasked));
    }

    #[test]
    fn borrowing_terminals_follow_the_installed_fonts() {
        for terminal in [
            env(&[("TERM", "xterm-256color"), ("KONSOLE_VERSION", "260801")]),
            env(&[("TERM", "alacritty")]),
            env(&[("TERM", "xterm-256color"), ("ALACRITTY_WINDOW_ID", "1")]),
            env(&[("TERM", "foot")]),
            env(&[("TERM", "xterm-256color"), ("VTE_VERSION", "7800")]),
        ] {
            assert!(drawable(&terminal, || true));
            assert!(!drawable(&terminal, || false));
        }
    }

    #[test]
    fn fonts_here_do_not_count_over_ssh() {
        assert!(!drawable(env(&[("KONSOLE_VERSION", "260801"), ("SSH_TTY", "/dev/pts/1")]), unasked));
        assert!(!drawable(env(&[("TERM", "alacritty"), ("SSH_CONNECTION", "10.0.0.2 50000 10.0.0.1 22")]), unasked));
    }

    #[test]
    fn other_terminals_keep_portable_icons() {
        assert!(!drawable(env(&[("TERM_PROGRAM", "Apple_Terminal"), ("TERM", "xterm-256color")]), unasked));
        assert!(!drawable(env(&[("TERM", "xterm-256color")]), unasked));
        assert!(!drawable(env(&[("TERM", "linux")]), unasked));
        assert!(!drawable(env(&[]), unasked));
    }

    #[test]
    fn a_missing_glyph_means_no_font() {
        assert!(!installed(&["\u{10fff0}"]));
    }
}
