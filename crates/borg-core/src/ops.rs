use std::path::Path;

use crate::command::CommandLine;

const EDIT_SCRIPT: &str = r#""${VISUAL:-${EDITOR:-vi}}" "$1""#;
const SCRIPT_NAME: &str = "sh";
pub const DEFAULT_ARCHIVE_SUFFIX: &str = ".tar.gz";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    TarZst,
    SevenZip,
}

const SUFFIXES: [(&str, ArchiveFormat); 11] = [
    (".tar.gz", ArchiveFormat::TarGz),
    (".tgz", ArchiveFormat::TarGz),
    (".tar.bz2", ArchiveFormat::TarBz2),
    (".tbz2", ArchiveFormat::TarBz2),
    (".tar.xz", ArchiveFormat::TarXz),
    (".txz", ArchiveFormat::TarXz),
    (".tar.zst", ArchiveFormat::TarZst),
    (".tzst", ArchiveFormat::TarZst),
    (".tar", ArchiveFormat::Tar),
    (".zip", ArchiveFormat::Zip),
    (".7z", ArchiveFormat::SevenZip),
];

impl ArchiveFormat {
    pub fn detect(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        SUFFIXES.iter().find(|(suffix, _)| lower.ends_with(suffix)).map(|(_, format)| *format)
    }

    fn tar_create_flag(self) -> Option<&'static str> {
        match self {
            Self::Tar => Some("-cf"),
            Self::TarGz => Some("-czf"),
            Self::TarBz2 => Some("-cjf"),
            Self::TarXz => Some("-cJf"),
            Self::TarZst => Some("--zstd -cf"),
            Self::Zip | Self::SevenZip => None,
        }
    }
}

pub fn copy<'a>(sources: impl IntoIterator<Item = &'a str>, dest: &Path) -> CommandLine {
    CommandLine::program("cp").flag("-R").flag("-i").locals(sources).path(dest)
}

pub fn move_to<'a>(sources: impl IntoIterator<Item = &'a str>, dest: &Path) -> CommandLine {
    CommandLine::program("mv").flag("-i").locals(sources).path(dest)
}

pub fn remove<'a>(sources: impl IntoIterator<Item = &'a str>) -> CommandLine {
    CommandLine::program("rm").flag("-r").flag("-f").locals(sources)
}

pub fn mkdir(name: &str) -> CommandLine {
    CommandLine::program("mkdir").flag("-p").local(name)
}

pub fn edit(name: &str) -> CommandLine {
    CommandLine::program("sh").flag("-c").arg(EDIT_SCRIPT.as_bytes()).flag(SCRIPT_NAME).local(name)
}

pub fn run(name: &str) -> CommandLine {
    CommandLine::default().local(name)
}

pub fn compress<'a>(archive: &str, format: ArchiveFormat, sources: impl IntoIterator<Item = &'a str>) -> CommandLine {
    let base = match (format, format.tar_create_flag()) {
        (_, Some(flag)) => CommandLine::program("tar").flag(flag),
        (ArchiveFormat::Zip, None) => CommandLine::program("zip").flag("-r").flag("-q"),
        (_, None) => CommandLine::program("7z").flag("a").flag("-bd"),
    };
    base.local(archive).locals(sources)
}

pub fn extract(archive: &str, format: ArchiveFormat, dest: &Path) -> CommandLine {
    match format {
        ArchiveFormat::Zip => CommandLine::program("unzip").flag("-q").local(archive).flag("-d").path(dest),
        ArchiveFormat::SevenZip => CommandLine::program("7z").flag("x").flag("-bd").local(archive).glued("-o", dest),
        _ => CommandLine::program("tar").flag("-xf").local(archive).flag("-C").path(dest),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &CommandLine) -> String {
        String::from_utf8_lossy(&line.line()).into_owned()
    }

    #[test]
    fn detects_formats_by_suffix() {
        assert_eq!(ArchiveFormat::detect("a.TAR.GZ"), Some(ArchiveFormat::TarGz));
        assert_eq!(ArchiveFormat::detect("a.tgz"), Some(ArchiveFormat::TarGz));
        assert_eq!(ArchiveFormat::detect("a.zip"), Some(ArchiveFormat::Zip));
        assert_eq!(ArchiveFormat::detect("a.tar"), Some(ArchiveFormat::Tar));
        assert_eq!(ArchiveFormat::detect("notes.txt"), None);
    }

    #[test]
    fn builds_file_operations() {
        let dest = Path::new("/other side");
        assert_eq!(text(&copy(["a", "b c"], dest)), "cp -R -i './a' './b c' '/other side'");
        assert_eq!(text(&move_to(["a"], dest)), "mv -i './a' '/other side'");
        assert_eq!(text(&remove(["-rf"])), "rm -r -f './-rf'");
        assert_eq!(text(&mkdir("new/dir")), "mkdir -p './new/dir'");
    }

    #[test]
    fn builds_archive_commands() {
        assert_eq!(text(&compress("out.tar.gz", ArchiveFormat::TarGz, ["a"])), "tar -czf './out.tar.gz' './a'");
        assert_eq!(text(&compress("out.zip", ArchiveFormat::Zip, ["a"])), "zip -r -q './out.zip' './a'");
        assert_eq!(text(&compress("o.7z", ArchiveFormat::SevenZip, ["a"])), "7z a -bd './o.7z' './a'");
        assert_eq!(text(&extract("x.zip", ArchiveFormat::Zip, Path::new("/d"))), "unzip -q './x.zip' -d '/d'");
        assert_eq!(text(&extract("x.tar.xz", ArchiveFormat::TarXz, Path::new("/d"))), "tar -xf './x.tar.xz' -C '/d'");
        assert_eq!(text(&extract("x.7z", ArchiveFormat::SevenZip, Path::new("/d"))), "7z x -bd './x.7z' -o'/d'");
    }

    #[test]
    fn runs_local_executables() {
        assert_eq!(text(&run("my tool")), "'./my tool'");
    }

    #[test]
    fn edit_uses_visual_then_editor_through_posix_sh() {
        assert_eq!(text(&edit("a.txt")), r#"sh -c '"${VISUAL:-${EDITOR:-vi}}" "$1"' sh './a.txt'"#);
    }
}
