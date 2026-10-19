use ratatui::style::Style;

use crate::theme::Styles;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Code,
    Image,
    Archive,
    Document,
    Media,
    Config,
}

const CODE: &[&str] = &[
    "rs", "c", "h", "cc", "cpp", "hpp", "py", "js", "mjs", "ts", "tsx", "jsx", "go", "java", "kt", "swift", "rb",
    "php", "sh", "bash", "zsh", "fish", "lua", "zig", "hs", "ml", "ex", "exs", "cs", "scala", "sql", "html", "css",
    "scss", "vue", "svelte", "dart", "pl", "nim", "el", "clj", "erl", "vim",
];
const IMAGE: &[&str] =
    &["png", "jpg", "jpeg", "gif", "bmp", "svg", "webp", "ico", "tif", "tiff", "heic", "avif", "psd"];
const ARCHIVE: &[&str] = &[
    "zip", "tar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "zst", "7z", "rar", "lz4", "lzma", "deb", "rpm", "dmg",
    "iso", "jar", "whl", "apk",
];
const DOCUMENT: &[&str] = &[
    "md", "markdown", "txt", "pdf", "rst", "org", "tex", "doc", "docx", "odt", "rtf", "epub", "csv", "tsv", "xls",
    "xlsx", "ppt", "pptx", "log",
];
const MEDIA: &[&str] = &["mp3", "wav", "flac", "ogg", "opus", "m4a", "aac", "mp4", "mkv", "mov", "avi", "webm", "wmv"];
const CONFIG: &[&str] =
    &["toml", "yaml", "yml", "json", "jsonc", "ini", "cfg", "conf", "env", "lock", "xml", "plist", "properties"];
const CONFIG_NAMES: &[&str] =
    &["makefile", "dockerfile", "justfile", ".gitignore", ".gitattributes", ".editorconfig", ".env", "cargo.lock"];
const TABLES: [(Kind, &[&str]); 6] = [
    (Kind::Code, CODE),
    (Kind::Image, IMAGE),
    (Kind::Archive, ARCHIVE),
    (Kind::Document, DOCUMENT),
    (Kind::Media, MEDIA),
    (Kind::Config, CONFIG),
];
const EXTENSION_SEPARATOR: char = '.';

pub fn classify(name: &str) -> Option<Kind> {
    let lower = name.to_ascii_lowercase();
    if CONFIG_NAMES.contains(&lower.as_str()) {
        return Some(Kind::Config);
    }
    let (stem, extension) = lower.rsplit_once(EXTENSION_SEPARATOR)?;
    if stem.is_empty() {
        return None;
    }
    TABLES.iter().find(|(_, extensions)| extensions.contains(&extension)).map(|(kind, _)| *kind)
}

pub fn style(kind: Kind, styles: &Styles<Style>) -> Style {
    match kind {
        Kind::Code => styles.kind_code,
        Kind::Image => styles.kind_image,
        Kind::Archive => styles.kind_archive,
        Kind::Document => styles.kind_document,
        Kind::Media => styles.kind_media,
        Kind::Config => styles.kind_config,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_extension_and_well_known_names() {
        assert_eq!(classify("main.rs"), Some(Kind::Code));
        assert_eq!(classify("Photo.JPG"), Some(Kind::Image));
        assert_eq!(classify("backup.tar.gz"), Some(Kind::Archive));
        assert_eq!(classify("Makefile"), Some(Kind::Config));
        assert_eq!(classify(".bashrc"), None);
        assert_eq!(classify("README"), None);
    }
}
