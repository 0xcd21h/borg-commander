//! RAR archives, read through an installed program: Borg carries no RAR decoder of its own.

use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::SystemTime,
};

use super::{Flow, Visit};
use crate::{Error, Format, Index, Item, ItemKind, Tool, ToolKind, index::ENTRY_LIMIT, name::clean};

const FIELD: &str = " = ";
const YES: &str = "+";
const SYMLINK_TYPE: char = 'l';
const PERMISSIONS_LENGTH: usize = 10;
const READ_BIT: char = 'r';
const WRITE_BIT: char = 'w';
const EXECUTE_BITS: [char; 3] = ['x', 's', 't'];
const PERMISSION_BITS: [u32; 9] = [0o400, 0o200, 0o100, 0o040, 0o020, 0o010, 0o004, 0o002, 0o001];
const SEVEN_ZIP_TIME: &str = "%Y-%m-%d %H:%M:%S";
/// Characters bsdtar reads as a pattern in the name of an entry to take out.
const PATTERN_CHARACTERS: [char; 4] = ['\\', '*', '?', '['];
const PATTERN_ESCAPE: char = '\\';

pub(crate) fn index(tool: &Tool, archive: &Path, bytes: u64, password: Option<&str>) -> Result<Index, Error> {
    if password.is_some() {
        return Err(Error::Tool(tool.name(), borg_i18n::tr!(archive_tool_password).into()));
    }
    let output = run(tool, &mut listing(tool, archive))?;
    let (items, left_out) = match tool.kind {
        ToolKind::SevenZip => seven_zip_listing(&output)?,
        ToolKind::Bsdtar => bsdtar_listing(&output)?,
    };
    Ok(Index::new(Format::Rar, bytes, items, left_out))
}

fn listing(tool: &Tool, archive: &Path) -> Command {
    let mut command = Command::new(&tool.program);
    match tool.kind {
        ToolKind::SevenZip => command.args(["l", "-slt", "-ba", "--"]),
        ToolKind::Bsdtar => command.args(["-tvf"]),
    };
    command.arg(archive);
    command
}

fn run(tool: &Tool, command: &mut Command) -> Result<String, Error> {
    let output = command.stdin(Stdio::null()).stderr(Stdio::piped()).output()?;
    if !output.status.success() {
        let reason = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(Error::Tool(tool.name(), reason));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The entries in 7-Zip's technical listing: blocks of `Key = value` lines.
pub(crate) fn seven_zip_listing(output: &str) -> Result<(Vec<Item>, usize), Error> {
    let mut items = Vec::new();
    let mut left_out = 0;
    for (slot, block) in output.split("\n\n").filter(|block| !block.trim().is_empty()).enumerate() {
        if items.len() >= ENTRY_LIMIT {
            return Err(Error::TooMany);
        }
        let field =
            |key: &str| block.lines().find_map(|line| line.strip_prefix(key).and_then(|rest| rest.strip_prefix(FIELD)));
        let Some(raw) = field("Path") else { continue };
        let Some(path) = clean(raw, true) else {
            left_out += 1;
            continue;
        };
        let permissions = field("Attributes").and_then(|attributes| attributes.split_whitespace().last());
        let kind = if field("Folder") == Some(YES) {
            ItemKind::Folder
        } else if permissions.is_some_and(|text| text.starts_with(SYMLINK_TYPE)) {
            ItemKind::Symlink
        } else {
            ItemKind::File
        };
        let mut item = Item::new(path, kind, raw.to_owned(), slot);
        item.size =
            if kind == ItemKind::File { field("Size").and_then(|size| size.parse().ok()).unwrap_or(0) } else { 0 };
        item.mode = permissions.and_then(mode).unwrap_or(item.mode);
        item.encrypted = field("Encrypted") == Some(YES);
        item.modified = field("Modified").and_then(local_time);
        items.push(item);
    }
    Ok((items, left_out))
}

/// The entries in `bsdtar -tv`'s listing, laid out like `ls -l`: mode, links, owner, group,
/// size, three fields of date, then the name, and `-> target` for a symlink.
pub(crate) fn bsdtar_listing(output: &str) -> Result<(Vec<Item>, usize), Error> {
    const BEFORE_NAME: usize = 8;
    const SIZE_FIELD: usize = 4;
    const LINK_ARROW: &str = " -> ";
    let mut items = Vec::new();
    let mut left_out = 0;
    for (slot, line) in output.lines().filter(|line| !line.trim().is_empty()).enumerate() {
        if items.len() >= ENTRY_LIMIT {
            return Err(Error::TooMany);
        }
        let mut rest = line;
        let mut fields = Vec::with_capacity(BEFORE_NAME);
        for _ in 0..BEFORE_NAME {
            let trimmed = rest.trim_start();
            let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
            fields.push(&trimmed[..end]);
            rest = &trimmed[end..];
        }
        let name = rest.strip_prefix(' ').unwrap_or(rest);
        let permissions = fields[0];
        let (raw, link) = match permissions.starts_with(SYMLINK_TYPE) {
            true => name.split_once(LINK_ARROW).map_or((name, None), |(name, target)| (name, Some(target))),
            false => (name, None),
        };
        let Some(path) = clean(raw, false) else {
            left_out += 1;
            continue;
        };
        let kind = match permissions.chars().next() {
            Some('d') => ItemKind::Folder,
            Some(SYMLINK_TYPE) => ItemKind::Symlink,
            _ => ItemKind::File,
        };
        let mut item = Item::new(path, kind, raw.trim_end_matches('/').to_owned(), slot);
        item.size = if kind == ItemKind::File { fields[SIZE_FIELD].parse().unwrap_or(0) } else { 0 };
        item.mode = mode(permissions).unwrap_or(item.mode);
        item.link = link.map(str::to_owned);
        items.push(item);
    }
    Ok((items, left_out))
}

/// The permission bits in an `ls -l` mode such as `-rwxr-xr-x`.
fn mode(text: &str) -> Option<u32> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() != PERMISSIONS_LENGTH {
        return None;
    }
    Some(chars[1..].iter().zip(PERMISSION_BITS).fold(0, |mode, (&flag, bit)| {
        let set = flag == READ_BIT || flag == WRITE_BIT || EXECUTE_BITS.contains(&flag);
        if set { mode | bit } else { mode }
    }))
}

fn local_time(text: &str) -> Option<SystemTime> {
    let civil = jiff::civil::DateTime::strptime(SEVEN_ZIP_TIME, text.trim()).ok()?;
    let zoned = civil.to_zoned(jiff::tz::TimeZone::system()).ok()?;
    Some(SystemTime::from(zoned.timestamp()))
}

/// An entry's name as bsdtar takes it to mean that entry alone, not a pattern other entries
/// could match.
pub(crate) fn literal_pattern(name: &str) -> String {
    name.chars().fold(String::with_capacity(name.len()), |mut pattern, character| {
        if PATTERN_CHARACTERS.contains(&character) {
            pattern.push(PATTERN_ESCAPE);
        }
        pattern.push(character);
        pattern
    })
}

/// Whether the program has written everything it had, which reading once more tells.
fn finished(output: &mut impl Read) -> bool {
    matches!(output.read(&mut [0]), Ok(0))
}

/// Streams each wanted entry out of the program, one run per entry. A run whose entry was not
/// read to its end is stopped, so leaving an entry out or cancelling never waits for the
/// program to unpack the rest of it. What the program says is wrong is read as it comes, so a
/// program with much to say never waits for Borg while Borg waits for it.
pub(crate) fn each(
    tool: &Tool,
    archive: &Path,
    password: Option<&str>,
    wanted: &[&Item],
    visit: Visit,
) -> Result<(), Error> {
    if password.is_some() {
        return Err(Error::Tool(tool.name(), borg_i18n::tr!(archive_tool_password).into()));
    }
    for item in wanted {
        let mut command = Command::new(&tool.program);
        match tool.kind {
            ToolKind::SevenZip => command.args(["x", "-so", "-spd", "-ba", "--"]).arg(archive).arg(&item.raw),
            ToolKind::Bsdtar => command.args(["-xOqf"]).arg(archive).arg("--").arg(literal_pattern(&item.raw)),
        };
        let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        let mut stdout = child.stdout.take().ok_or(Error::NoTool)?;
        let said = child.stderr.take().map(|mut stderr| {
            std::thread::spawn(move || {
                let mut errors = String::new();
                let _ = stderr.read_to_string(&mut errors);
                errors
            })
        });
        let flow = visit(item, &mut stdout);
        let read_whole = flow.is_ok() && finished(&mut stdout);
        if !read_whole {
            let _ = child.kill();
        }
        drop(stdout);
        let status = child.wait()?;
        let errors = said.and_then(|said| said.join().ok()).unwrap_or_default();
        if flow? == Flow::Stop {
            break;
        }
        if read_whole && !status.success() {
            return Err(Error::Tool(tool.name(), errors.trim().to_owned()));
        }
    }
    Ok(())
}
