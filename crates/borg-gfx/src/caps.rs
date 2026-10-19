use std::{
    io::{self, Read, Write},
    os::fd::AsFd,
    str::FromStr,
    time::{Duration, Instant},
};

use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    termios::tcgetwinsize,
};

const KITTY_QUERY: &str = "\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\";
const KITTY_OK: &str = "\x1b_Gi=31;OK";
const CELL_QUERY: &str = "\x1b[16t";
const CELL_REPLY: &str = "\x1b[6;";
const DEVICE_QUERY: &str = "\x1b[c";
const OSC: &str = "\x1b]";
const FOREGROUND_SLOT: &str = "10";
const BACKGROUND_SLOT: &str = "11";
const PALETTE_SLOT: &str = "4";
const ANSI_COLORS: usize = 16;
const RGB_PREFIX: &str = "rgb:";
const STRING_TERMINATOR: &str = "\x1b\\";
const BELL: char = '\u{7}';
const ESCAPE: char = '\u{1b}';
const DEVICE_REPLY: &str = "\x1b[?";
const SIXEL_FEATURE: &str = "4";
const READ_CHUNK: usize = 1024;
pub const DEFAULT_CELL: CellSize = CellSize { width: 10, height: 20 };

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Kitty,
    Sixel,
    Iterm,
    Cells,
}

impl Protocol {
    pub fn pixels(self) -> bool {
        self != Self::Cells
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Preference {
    #[default]
    Auto,
    Kitty,
    Sixel,
    Iterm,
    Off,
}

impl FromStr for Preference {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "kitty" => Ok(Self::Kitty),
            "sixel" => Ok(Self::Sixel),
            "iterm" => Ok(Self::Iterm),
            "off" => Ok(Self::Off),
            other => Err(format!("unknown graphics mode {other:?} (auto, kitty, sixel, iterm or off)")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellSize {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub protocol: Protocol,
    pub cell: CellSize,
}

impl Capabilities {
    pub const CELLS: Self = Self { protocol: Protocol::Cells, cell: DEFAULT_CELL };

    pub fn choose(preference: Preference, replies: &Replies, hints: Hints) -> Self {
        let protocol = match preference {
            Preference::Off => Protocol::Cells,
            Preference::Kitty => Protocol::Kitty,
            Preference::Sixel => Protocol::Sixel,
            Preference::Iterm => Protocol::Iterm,
            Preference::Auto if hints.multiplexed => Protocol::Cells,
            Preference::Auto if replies.kitty => Protocol::Kitty,
            Preference::Auto if hints.iterm => Protocol::Iterm,
            Preference::Auto if replies.sixel => Protocol::Sixel,
            Preference::Auto => Protocol::Cells,
        };
        Self { protocol, cell: replies.cell.unwrap_or(DEFAULT_CELL) }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hints {
    pub multiplexed: bool,
    pub iterm: bool,
}

impl Hints {
    pub fn from_env(var: impl Fn(&str) -> Option<String>) -> Self {
        let multiplexed = var("TMUX").is_some() || var("STY").is_some();
        let iterm =
            var("TERM_PROGRAM").as_deref() == Some("iTerm.app") || var("LC_TERMINAL").as_deref() == Some("iTerm2");
        Self { multiplexed, iterm }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalColors {
    pub foreground: Option<[u8; 3]>,
    pub background: Option<[u8; 3]>,
    pub ansi: [Option<[u8; 3]>; ANSI_COLORS],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Replies {
    pub kitty: bool,
    pub sixel: bool,
    pub cell: Option<CellSize>,
    pub colors: TerminalColors,
    pub complete: bool,
}

fn channel(hex: &str) -> Option<u8> {
    let digits = u32::try_from(hex.len()).ok().filter(|n| (1..=4).contains(n))?;
    let value = u32::from_str_radix(hex, 16).ok()?;
    let max = 16u32.pow(digits) - 1;
    u8::try_from(value * u32::from(u8::MAX) / max).ok()
}

fn rgb(spec: &str) -> Option<[u8; 3]> {
    let mut parts = spec.strip_prefix(RGB_PREFIX)?.split('/').map(channel);
    Some([parts.next()??, parts.next()??, parts.next()??])
}

fn color_replies(text: &str) -> TerminalColors {
    let mut colors = TerminalColors::default();
    for reply in text.split(OSC).skip(1) {
        let body = reply.split([BELL, ESCAPE]).next().unwrap_or_default();
        let fields: Vec<&str> = body.split(';').collect();
        match fields.as_slice() {
            [FOREGROUND_SLOT, spec] => colors.foreground = rgb(spec),
            [BACKGROUND_SLOT, spec] => colors.background = rgb(spec),
            [PALETTE_SLOT, index, spec] => {
                if let Some(slot) = index.parse::<usize>().ok().and_then(|i| colors.ansi.get_mut(i)) {
                    *slot = rgb(spec);
                }
            }
            _ => {}
        }
    }
    colors
}

fn color_queries() -> String {
    let palette: String = (0..ANSI_COLORS).map(|i| format!("{OSC}{PALETTE_SLOT};{i};?{STRING_TERMINATOR}")).collect();
    format!("{OSC}{FOREGROUND_SLOT};?{STRING_TERMINATOR}{OSC}{BACKGROUND_SLOT};?{STRING_TERMINATOR}{palette}")
}

impl Replies {
    pub fn parse(bytes: &[u8]) -> Self {
        let text = String::from_utf8_lossy(bytes);
        let device = text.split(DEVICE_REPLY).nth(1).and_then(|rest| rest.split_once('c')).map(|(params, _)| params);
        Self {
            kitty: text.contains(KITTY_OK),
            sixel: device.is_some_and(|params| params.split(';').any(|p| p == SIXEL_FEATURE)),
            cell: text.split(CELL_REPLY).nth(1).and_then(|rest| rest.split_once('t')).and_then(|(sizes, _)| {
                let (height, width) = sizes.split_once(';')?;
                cell(width.parse().ok()?, height.parse().ok()?)
            }),
            colors: color_replies(&text),
            complete: device.is_some(),
        }
    }
}

fn cell(width: u16, height: u16) -> Option<CellSize> {
    (width > 0 && height > 0).then_some(CellSize { width, height })
}

fn winsize_cell() -> Option<CellSize> {
    let size = tcgetwinsize(io::stdout()).ok()?;
    cell(size.ws_xpixel.checked_div(size.ws_col)?, size.ws_ypixel.checked_div(size.ws_row)?)
}

pub fn probe(timeout: Duration) -> io::Result<Replies> {
    let mut out = io::stdout();
    out.write_all(format!("{KITTY_QUERY}{CELL_QUERY}{}{DEVICE_QUERY}", color_queries()).as_bytes())?;
    out.flush()?;

    let deadline = Instant::now() + timeout;
    let mut stdin = io::stdin();
    let mut received = Vec::new();
    let mut replies = Replies::default();
    while !replies.complete {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let wait = Timespec::try_from(left).map_err(io::Error::other)?;
        let fd = stdin.as_fd();
        let mut fds = [PollFd::new(&fd, PollFlags::IN)];
        if poll(&mut fds, Some(&wait))? == 0 {
            break;
        }
        let mut chunk = [0; READ_CHUNK];
        let read = stdin.read(&mut chunk)?;
        received.extend_from_slice(&chunk[..read]);
        replies = Replies::parse(&received);
    }
    replies.cell = winsize_cell().or(replies.cell);
    Ok(replies)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kitty_cell_size_and_sixel_replies() {
        let replies = Replies::parse(b"\x1b_Gi=31;OK\x1b\\\x1b[6;20;10t\x1b[?62;4;22c");
        assert_eq!(
            replies,
            Replies {
                kitty: true,
                sixel: true,
                cell: Some(CellSize { width: 10, height: 20 }),
                complete: true,
                ..Replies::default()
            }
        );

        let plain = Replies::parse(b"\x1b[?6c");
        assert_eq!(plain, Replies { complete: true, ..Replies::default() });
        assert!(!Replies::parse(b"\x1b[6;20;10t").complete);
    }

    #[test]
    fn parses_terminal_colour_replies() {
        let replies = Replies::parse(
            b"\x1b]10;rgb:0000/0000/0000\x1b\\\x1b]11;rgb:ffff/fefe/f0f0\x07\x1b]4;4;rgb:80/a0/f0\x1b\\\x1b[?62c",
        );
        assert_eq!(replies.colors.foreground, Some([0, 0, 0]));
        assert_eq!(replies.colors.background, Some([255, 254, 240]));
        assert_eq!(replies.colors.ansi[4], Some([128, 160, 240]));
        assert_eq!(replies.colors.ansi[0], None);
        assert!(replies.complete);
    }

    #[test]
    fn chooses_the_best_protocol() {
        let kitty = Replies { kitty: true, sixel: true, ..Replies::default() };
        let sixel = Replies { sixel: true, ..Replies::default() };
        let none = Hints::default();
        let auto = |replies: &Replies, hints| Capabilities::choose(Preference::Auto, replies, hints).protocol;
        assert_eq!(auto(&kitty, none), Protocol::Kitty);
        assert_eq!(auto(&sixel, none), Protocol::Sixel);
        assert_eq!(auto(&sixel, Hints { iterm: true, ..none }), Protocol::Iterm);
        assert_eq!(auto(&kitty, Hints { multiplexed: true, ..none }), Protocol::Cells);
        assert_eq!(Capabilities::choose(Preference::Off, &kitty, none).protocol, Protocol::Cells);
        assert_eq!(Capabilities::choose(Preference::Sixel, &Replies::default(), none).protocol, Protocol::Sixel);
    }

    #[test]
    fn reads_environment_hints() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| pairs.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).to_owned())
        };
        assert!(Hints::from_env(env(&[("TMUX", "/tmp/x")])).multiplexed);
        assert!(Hints::from_env(env(&[("LC_TERMINAL", "iTerm2")])).iterm);
        assert_eq!(Hints::from_env(env(&[])), Hints::default());
    }

    #[test]
    fn preferences_parse() {
        assert_eq!("kitty".parse(), Ok(Preference::Kitty));
        assert!("png".parse::<Preference>().is_err());
    }
}
