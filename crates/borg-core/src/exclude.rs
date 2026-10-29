//! Generated folders a search skips: what tells them apart from folders someone wrote.

/// The file the Cache Directory Tagging Specification puts in cache folders, and the line it
/// starts with. Cargo's `target`, and many other tools' caches, carry it.
pub const CACHEDIR_TAG: &str = "CACHEDIR.TAG";
pub const CACHEDIR_SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// What marks a folder of this name as generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    /// The name alone: no one writes a `node_modules` by hand.
    Name,
    /// A file beside the folder, as `Cargo.toml` beside `target`.
    Beside(&'static [&'static str]),
    /// A file inside it, as `pyvenv.cfg` in a Python virtual environment.
    Inside(&'static str),
}

pub const ARTIFACTS: &[(&str, Marker)] = &[
    (".git", Marker::Name),
    (".hg", Marker::Name),
    (".svn", Marker::Name),
    ("node_modules", Marker::Name),
    ("bower_components", Marker::Name),
    ("__pycache__", Marker::Name),
    (".tox", Marker::Name),
    (".nox", Marker::Name),
    (".mypy_cache", Marker::Name),
    (".pytest_cache", Marker::Name),
    (".ruff_cache", Marker::Name),
    (".gradle", Marker::Name),
    (".next", Marker::Name),
    (".nuxt", Marker::Name),
    (".svelte-kit", Marker::Name),
    (".parcel-cache", Marker::Name),
    (".turbo", Marker::Name),
    (".terraform", Marker::Name),
    (".cache", Marker::Name),
    ("DerivedData", Marker::Name),
    ("Pods", Marker::Beside(&["Podfile"])),
    ("target", Marker::Beside(&["Cargo.toml", "pom.xml"])),
    ("build", Marker::Beside(&["build.gradle", "build.gradle.kts", "CMakeLists.txt", "setup.py"])),
    ("dist", Marker::Beside(&["package.json", "setup.py", "pyproject.toml"])),
    ("vendor", Marker::Beside(&["go.mod", "composer.json", "Gemfile"])),
    ("venv", Marker::Inside("pyvenv.cfg")),
    (".venv", Marker::Inside("pyvenv.cfg")),
];

/// Whether a folder called `name` is generated, given ways to look for a file beside it and
/// inside it. The cache tag is checked separately, since any folder may carry one.
pub fn is_artifact(name: &str, beside: impl Fn(&str) -> bool, inside: impl Fn(&str) -> bool) -> bool {
    ARTIFACTS.iter().filter(|(artifact, _)| *artifact == name).any(|(_, marker)| match marker {
        Marker::Name => true,
        Marker::Beside(files) => files.iter().any(|file| beside(file)),
        Marker::Inside(file) => inside(file),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_alone_or_with_their_marker_files_make_a_folder_an_artifact() {
        let none = |_: &str| false;
        assert!(is_artifact("node_modules", none, none));
        assert!(!is_artifact("target", none, none), "a target folder without a build file is someone's");
        assert!(is_artifact("target", |file| file == "Cargo.toml", none));
        assert!(is_artifact(".venv", none, |file| file == "pyvenv.cfg"));
        assert!(!is_artifact("src", |_| true, |_| true));
    }
}
