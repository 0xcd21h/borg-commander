//! Reading and changing files for an agent, on this machine or inside a container, and the
//! diffs the user approves changes by.

use std::{
    fmt::Write as _,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use borg_core::{Change, DiffLine};

use super::{Reply, location::DOCKER_SCHEME};

/// The most of a file one read looks at: from its start, or from its end for its last lines.
const READ_CAP: u64 = 8 * 1024 * 1024;
/// The largest file an agent can write or edit; the whole file goes through a diff.
const EDIT_CAP: u64 = 2 * 1024 * 1024;
const DEFAULT_LINES: usize = 2000;
/// The most text one read returns.
const OUTPUT_CAP: usize = 128 * 1024;
const LINE_CAP: usize = 2000;
const MATCH_LIMIT: usize = 200;
const LIST_LIMIT: usize = 500;
/// Unchanged lines a diff shows around each change.
const CONTEXT: usize = 3;

/// A location with its container found.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Spot {
    Host(PathBuf),
    Container { id: String, name: String, path: PathBuf },
}

impl Spot {
    pub fn path(&self) -> &Path {
        match self {
            Self::Host(path) | Self::Container { path, .. } => path,
        }
    }

    /// The location as an agent writes it.
    pub fn uri(&self) -> String {
        match self {
            Self::Host(path) => path.display().to_string(),
            Self::Container { name, path, .. } => format!("{DOCKER_SCHEME}{name}{}", path.display()),
        }
    }
}

/// Which lines of a file to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lines {
    /// From the first number to the second, or to the end; both count from 1.
    Range(usize, Option<usize>),
    /// The last few.
    Last(usize),
}

/// Reads `120-180`, `120-` (to the end), `120` (one line) or `-50` (the last 50).
pub fn parse_lines(text: &str) -> Result<Lines, String> {
    let text = text.trim();
    let number = |s: &str| {
        s.trim().parse::<usize>().map_err(|_| format!("lines {text:?} is not like 120-180, 120-, 120 or -50"))
    };
    if let Some(last) = text.strip_prefix('-') {
        return Ok(Lines::Last(number(last)?));
    }
    match text.split_once('-') {
        Some((from, "")) => Ok(Lines::Range(number(from)?, None)),
        Some((from, to)) => Ok(Lines::Range(number(from)?, Some(number(to)?))),
        None => {
            let line = number(text)?;
            Ok(Lines::Range(line, Some(line)))
        }
    }
}

enum Raw {
    Folder,
    /// The bytes read, and whether the file goes on beyond them (before them when read
    /// from the end).
    File {
        bytes: Vec<u8>,
        cut: bool,
    },
}

async fn raw(spot: &Spot, cap: u64, tail: bool) -> Result<Raw> {
    match spot {
        Spot::Host(path) => {
            let path = path.clone();
            tokio::task::spawn_blocking(move || -> Result<Raw> {
                let meta = std::fs::metadata(&path)?;
                if meta.is_dir() {
                    return Ok(Raw::Folder);
                }
                let mut file = std::fs::File::open(&path)?;
                let size = meta.len();
                if tail && size > cap {
                    file.seek(SeekFrom::Start(size - cap))?;
                }
                let mut bytes = Vec::new();
                file.take(cap).read_to_end(&mut bytes)?;
                Ok(Raw::File { bytes, cut: size > cap })
            })
            .await?
        }
        Spot::Container { id, path, .. } => Ok(match borg_docker::read_file(id, path, cap + 1, tail).await? {
            None => Raw::Folder,
            Some(mut bytes) => {
                let cut = bytes.len() as u64 > cap;
                if cut {
                    if tail {
                        bytes.drain(..1)
                    } else {
                        bytes.drain(cap as usize..)
                    };
                }
                Raw::File { bytes, cut }
            }
        }),
    }
}

/// A folder's entries, or a file's lines with their numbers: the lines asked for, or those
/// containing `find`.
pub async fn read(spot: Spot, lines: Option<Lines>, find: Option<String>) -> Reply {
    let tail = matches!(lines, Some(Lines::Last(_)));
    match raw(&spot, READ_CAP, tail).await {
        Err(error) => Reply::error(format!("{}: {error:#}", spot.uri())),
        Ok(Raw::Folder) => listing(&spot).await,
        Ok(Raw::File { bytes, .. }) if bytes.contains(&0) => Reply::ok(format!("{} is binary", spot.uri())),
        Ok(Raw::File { bytes, cut }) => {
            let mut text = String::from_utf8_lossy(&bytes).into_owned();
            let mut note = String::new();
            if cut && tail {
                // The read starts inside a line, and the lines before it are not counted.
                text = text.split_once('\n').map_or_else(String::new, |(_, rest)| rest.to_owned());
                note = "\n(The file is larger than 8 MiB: only its end was read, and lines are numbered from there.)"
                    .into();
            } else if cut {
                note = "\n(The file is larger than 8 MiB: only its start was read.)".into();
            }
            Reply::ok(format!("{}{note}", numbered(&spot.uri(), &text, lines, find.as_deref())))
        }
    }
}

/// The lines asked for, numbered as `cat -n` does, so an edit can quote them without numbers.
fn numbered(uri: &str, text: &str, lines: Option<Lines>, find: Option<&str>) -> String {
    let all: Vec<&str> = text.lines().collect();
    let total = all.len();
    if total == 0 {
        return format!("{uri} is empty");
    }
    let (first, last) = match lines {
        None if find.is_some() => (1, total),
        None => (1, total.min(DEFAULT_LINES)),
        Some(Lines::Range(from, to)) => (from.max(1), to.unwrap_or(total).min(total)),
        Some(Lines::Last(count)) => (total.saturating_sub(count) + 1, total),
    };
    if first > last {
        return format!("{uri} has {total} lines");
    }
    let line = |number: usize| {
        let text = all[number - 1];
        let text = if text.chars().count() > LINE_CAP {
            format!("{}…", text.chars().take(LINE_CAP).collect::<String>())
        } else {
            text.to_owned()
        };
        format!("{number:>6}\t{text}\n")
    };
    let mut out = String::new();
    if let Some(find) = find {
        let found: Vec<usize> = (first..=last).filter(|&n| all[n - 1].contains(find)).collect();
        let _ = writeln!(out, "{uri}: {} lines containing {find:?}, of lines {first}–{last}", found.len());
        for &number in found.iter().take(MATCH_LIMIT) {
            out.push_str(&line(number));
        }
        if found.len() > MATCH_LIMIT {
            let _ = writeln!(out, "… {} more; narrow the lines or the text", found.len() - MATCH_LIMIT);
        }
        return out;
    }
    let mut body = String::new();
    let mut shown = first;
    for number in first..=last {
        let next = line(number);
        if body.len() + next.len() > OUTPUT_CAP {
            break;
        }
        body.push_str(&next);
        shown = number;
    }
    let _ = writeln!(out, "{uri}: lines {first}–{shown} of {total}");
    out.push_str(&body);
    if shown < total && lines.is_none_or(|_| shown < last) {
        let _ = writeln!(out, "… lines {}–{total} are not shown; read them with lines \"{}-\"", shown + 1, shown + 1);
    }
    out
}

async fn listing(spot: &Spot) -> Reply {
    let mut lines: Vec<String> = match spot {
        Spot::Host(dir) => match std::fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    match entry.metadata() {
                        Ok(meta) if meta.is_dir() => format!("{name}/"),
                        Ok(meta) => format!("{name}  {} bytes", meta.len()),
                        Err(_) => name,
                    }
                })
                .collect(),
            Err(error) => return Reply::error(format!("{}: {error}", spot.uri())),
        },
        Spot::Container { id, path, .. } => match borg_docker::files(id, path).await {
            Ok(files) => files
                .iter()
                .map(|file| {
                    if file.is_dir() {
                        format!("{}/", file.name)
                    } else {
                        format!("{}  {} bytes", file.name, file.size)
                    }
                })
                .collect(),
            Err(error) => return Reply::error(format!("{}: {error:#}", spot.uri())),
        },
    };
    lines.sort();
    let total = lines.len();
    lines.truncate(LIST_LIMIT);
    let more = if total > LIST_LIMIT { format!("\n… and {} more", total - LIST_LIMIT) } else { String::new() };
    Reply::ok(format!("{} ({total} entries)\n{}{more}", spot.uri(), lines.join("\n")))
}

/// Whether a location is a folder, for showing it: a file shows selected in its folder.
pub async fn is_folder(spot: &Spot) -> bool {
    match spot {
        Spot::Host(path) => path.is_dir(),
        Spot::Container { id, path, .. } => borg_docker::files(id, path).await.is_ok(),
    }
}

/// A whole text file, for changing it: `None` when it does not exist yet.
pub async fn load(spot: &Spot) -> Result<Option<String>> {
    let bytes = match spot {
        Spot::Host(path) => match std::fs::metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(meta) if meta.is_dir() => bail!("it is a folder"),
            Ok(meta) if meta.len() > EDIT_CAP => bail!("it is larger than 2 MiB"),
            Ok(_) => std::fs::read(path)?,
        },
        Spot::Container { id, path, .. } => match borg_docker::read_file(id, path, EDIT_CAP + 1, false).await {
            Err(error) if format!("{error:#}").contains("No such file") => return Ok(None),
            Err(error) => return Err(error),
            Ok(None) => bail!("it is a folder"),
            Ok(Some(bytes)) if bytes.len() as u64 > EDIT_CAP => bail!("it is larger than 2 MiB"),
            Ok(Some(bytes)) => bytes,
        },
    };
    if bytes.contains(&0) {
        bail!("it is binary");
    }
    String::from_utf8(bytes).map(Some).context("it is not UTF-8 text")
}

/// Writes a whole file in place, so an existing one keeps its owner and permissions.
pub async fn store(spot: &Spot, text: &str) -> Result<()> {
    match spot {
        Spot::Host(path) => {
            let (path, text) = (path.clone(), text.to_owned());
            tokio::task::spawn_blocking(move || std::fs::write(&path, text)).await??;
            Ok(())
        }
        Spot::Container { id, path, .. } => borg_docker::write_file(id, path, text.as_bytes()).await,
    }
}

/// Bytes one piece of a download or an upload carries.
pub const PIECE: u64 = 1024 * 1024;
/// The largest file an upload writes.
const UPLOAD_LIMIT: u64 = 1024 * 1024 * 1024;

/// A piece of a file for a download: its size and up to [`PIECE`] bytes from `offset`, as
/// JSON with the bytes in base64.
pub async fn piece(spot: &Spot, offset: u64) -> Result<String> {
    use base64::Engine as _;
    let (size, bytes) = match spot {
        Spot::Host(path) => {
            let path = path.clone();
            tokio::task::spawn_blocking(move || -> Result<(u64, Vec<u8>)> {
                let mut file = std::fs::File::open(&path)?;
                let meta = file.metadata()?;
                if meta.is_dir() {
                    bail!("it is a folder");
                }
                file.seek(SeekFrom::Start(offset))?;
                let mut bytes = Vec::new();
                file.take(PIECE).read_to_end(&mut bytes)?;
                Ok((meta.len(), bytes))
            })
            .await??
        }
        Spot::Container { id, path, .. } => borg_docker::read_range(id, path, offset, PIECE).await?,
    };
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(serde_json::json!({ "size": size, "data": data }).to_string())
}

/// A file arriving from another computer: written to a staged file, which takes the file's
/// name only once all of it arrived, so a broken upload never leaves half a file.
pub struct Upload {
    pub spot: Spot,
    staged: PathBuf,
    file: Option<std::fs::File>,
    size: u64,
    written: u64,
}

impl Upload {
    /// Stages an upload of `size` bytes: beside the file on this machine, so it moves into
    /// place in one step, or in `scratch` for a container.
    pub fn begin(spot: Spot, size: u64, scratch: &Path, token: &str) -> Result<Self> {
        if size > UPLOAD_LIMIT {
            bail!("the file is larger than 1 GiB");
        }
        let staged = match &spot {
            Spot::Host(path) => {
                let name = path.file_name().context("the path names no file")?.to_string_lossy();
                path.with_file_name(format!(".{name}.borg-upload-{token}"))
            }
            Spot::Container { .. } => {
                std::fs::create_dir_all(scratch)?;
                scratch.join(format!("upload-{token}"))
            }
        };
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
            .with_context(|| format!("could not write beside {}", spot.uri()))?;
        Ok(Self { spot, staged, file: Some(file), size, written: 0 })
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        use std::io::Write as _;
        if self.written + bytes.len() as u64 > self.size {
            bail!("more data than the upload announced");
        }
        self.file.as_mut().context("the upload is finished")?.write_all(bytes)?;
        self.written += bytes.len() as u64;
        Ok(())
    }

    /// Puts the complete file in place.
    pub async fn finish(mut self) -> Result<u64> {
        drop(self.file.take());
        let (written, size) = (self.written, self.size);
        if written != size {
            bail!("the upload ended after {written} of {size} bytes");
        }
        match &self.spot {
            Spot::Host(path) => std::fs::rename(&self.staged, path)?,
            Spot::Container { id, path, .. } => borg_docker::copy_in(id, &self.staged, path).await?,
        }
        Ok(size)
    }
}

impl Drop for Upload {
    /// Whatever happened, the staged file goes: it was moved into place, or the upload broke.
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.staged);
    }
}

/// A new file, every line of it added.
pub fn whole(text: &str) -> Vec<DiffLine> {
    text.lines()
        .enumerate()
        .map(|(index, line)| DiffLine { change: Change::Added, number: Some(index + 1), text: line.to_owned() })
        .collect()
}

/// The lines that change from `old` to `new`, with a few unchanged lines around each change
/// and the count of those left out between.
pub fn diff(old: &str, new: &str) -> Vec<DiffLine> {
    use similar::{ChangeTag, TextDiff};
    let changes = TextDiff::from_lines(old, new);
    let total = old.lines().count();
    let gap =
        |count: usize| DiffLine { change: Change::Gap, number: None, text: format!("⋯ {count} unchanged lines") };
    let mut lines = Vec::new();
    let mut seen = 0;
    for group in changes.grouped_ops(CONTEXT) {
        let Some(start) = group.first().map(|op| op.old_range().start) else { continue };
        if start > seen {
            lines.push(gap(start - seen));
        }
        for op in &group {
            for change in changes.iter_changes(op) {
                let text = change.value().trim_end_matches(['\n', '\r']).to_owned();
                let (change, number) = match change.tag() {
                    ChangeTag::Equal => (Change::Same, change.new_index()),
                    ChangeTag::Insert => (Change::Added, change.new_index()),
                    ChangeTag::Delete => (Change::Removed, change.old_index()),
                };
                lines.push(DiffLine { change, number: number.map(|n| n + 1), text });
            }
        }
        seen = group.last().map_or(seen, |op| op.old_range().end);
    }
    if total > seen && !lines.is_empty() {
        lines.push(gap(total - seen));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_read_by_range_from_the_end_or_by_the_text_they_contain() {
        assert_eq!(parse_lines("120-180"), Ok(Lines::Range(120, Some(180))));
        assert_eq!(parse_lines("120-"), Ok(Lines::Range(120, None)));
        assert_eq!(parse_lines("7"), Ok(Lines::Range(7, Some(7))));
        assert_eq!(parse_lines("-50"), Ok(Lines::Last(50)));
        assert!(parse_lines("x").is_err());

        let text = "alpha\nbeta\ngamma error\ndelta\nepsilon error\n";
        assert_eq!(
            numbered("/f", text, Some(Lines::Range(2, Some(3))), None),
            "/f: lines 2–3 of 5\n     2\tbeta\n     3\tgamma error\n"
        );
        assert_eq!(numbered("/f", text, Some(Lines::Last(1)), None), "/f: lines 5–5 of 5\n     5\tepsilon error\n");
        assert_eq!(
            numbered("/f", text, None, Some("error")),
            "/f: 2 lines containing \"error\", of lines 1–5\n     3\tgamma error\n     5\tepsilon error\n"
        );
    }

    #[test]
    fn a_diff_shows_changes_with_context_and_counts_what_it_leaves_out() {
        let old: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        let new = old.replace("line 10\n", "line ten\n");
        let lines = diff(&old, &new);
        assert_eq!(lines.first().map(|l| (l.change, l.text.as_str())), Some((Change::Gap, "⋯ 6 unchanged lines")));
        let changed: Vec<_> = lines
            .iter()
            .filter(|l| l.change != Change::Same && l.change != Change::Gap)
            .map(|l| (l.change, l.number, l.text.as_str()))
            .collect();
        assert_eq!(changed, [(Change::Removed, Some(10), "line 10"), (Change::Added, Some(10), "line ten")]);
        assert_eq!(lines.last().map(|l| l.text.as_str()), Some("⋯ 7 unchanged lines"));
        assert_eq!(whole("a\nb\n").len(), 2);
    }
}
