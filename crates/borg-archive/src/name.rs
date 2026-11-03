//! Entry names as Borg shows and unpacks them: relative, with `/` between folders, and never
//! leading out of the folder an archive unpacks into.

const SEPARATOR: char = '/';
const WINDOWS_SEPARATOR: char = '\\';
const CURRENT: &str = ".";
const PARENT: &str = "..";
const DRIVE: char = ':';

/// The entry's path inside the archive, or `None` when it is absolute, climbs out with `..`,
/// names a drive or holds a NUL. Zip archives made on Windows may separate folders with `\`.
pub(crate) fn clean(raw: &str, windows: bool) -> Option<String> {
    if raw.contains('\0') || raw.starts_with(SEPARATOR) || (windows && raw.starts_with(WINDOWS_SEPARATOR)) {
        return None;
    }
    let raw = if windows { raw.replace(WINDOWS_SEPARATOR, "/") } else { raw.to_owned() };
    let parts: Vec<&str> = raw.split(SEPARATOR).filter(|part| !part.is_empty() && *part != CURRENT).collect();
    let unsafe_part = |part: &&str| *part == PARENT || (windows && part.contains(DRIVE));
    if parts.is_empty() || parts.iter().any(unsafe_part) {
        return None;
    }
    Some(parts.join("/"))
}

/// The folder that holds `path`, `""` for the archive's top.
pub(crate) fn parent(path: &str) -> &str {
    path.rsplit_once(SEPARATOR).map_or("", |(parent, _)| parent)
}

/// The last part of `path`.
pub fn base(path: &str) -> &str {
    path.rsplit_once(SEPARATOR).map_or(path, |(_, name)| name)
}

/// Whether `path` is `folder` or lies beneath it; everything lies beneath the top, `""`.
pub(crate) fn within(path: &str, folder: &str) -> bool {
    folder.is_empty() || path == folder || path.strip_prefix(folder).is_some_and(|rest| rest.starts_with(SEPARATOR))
}

/// `path` relative to `folder`, which holds it.
pub(crate) fn relative<'a>(path: &'a str, folder: &str) -> &'a str {
    if folder.is_empty() { path } else { path[folder.len()..].trim_start_matches(SEPARATOR) }
}
