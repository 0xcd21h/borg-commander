use std::{
    fs, io,
    path::{Path, PathBuf},
    str::FromStr,
};

use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

const THEME_EXTENSION: &str = "toml";

const BUILTIN: [(&str, &str); 4] = [
    ("midnight", include_str!("../themes/midnight.toml")),
    ("classic", include_str!("../themes/classic.toml")),
    ("paper", include_str!("../themes/paper.toml")),
    ("ember", include_str!("../themes/ember.toml")),
];

#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    #[error("theme {id}: {source}")]
    Parse { id: String, source: toml::de::Error },
    #[error("theme {id}: unknown base theme {base}")]
    UnknownBase { id: String, base: String },
    #[error("theme {id}: duplicate theme id")]
    Duplicate { id: String },
    #[error("reading themes from {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
}

macro_rules! styles {
    ($($field:ident),* $(,)?) => {
        #[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
        #[serde(rename_all = "kebab-case", deny_unknown_fields)]
        pub struct Styles<T> {
            $(pub $field: T,)*
        }

        impl Styles<Option<StyleSpec>> {
            fn over(self, base: &Styles<Style>) -> Styles<Style> {
                Styles { $($field: self.$field.map_or(base.$field, StyleSpec::into_style),)* }
            }

            pub fn missing(&self) -> Vec<&'static str> {
                [$((stringify!($field), self.$field.is_none()),)*]
                    .into_iter()
                    .filter_map(|(name, missing)| missing.then_some(name))
                    .collect()
            }
        }
    };
}

styles! {
    panel,
    border,
    border_active,
    title,
    title_active,
    dir,
    file,
    executable,
    link,
    other,
    meta,
    cursor,
    cursor_inactive,
    footer,
    notice,
    key,
    key_label,
    overlay,
    overlay_border,
    overlay_selected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
struct ColorSpec(Color);

impl TryFrom<String> for ColorSpec {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Color::from_str(&value).map(Self).map_err(|_| format!("unknown color {value:?}"))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StyleSpec {
    fg: Option<ColorSpec>,
    bg: Option<ColorSpec>,
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    italic: bool,
    #[serde(default)]
    dim: bool,
    #[serde(default)]
    underline: bool,
    #[serde(default)]
    reversed: bool,
}

impl StyleSpec {
    fn into_style(self) -> Style {
        let modifiers = [
            (self.bold, Modifier::BOLD),
            (self.italic, Modifier::ITALIC),
            (self.dim, Modifier::DIM),
            (self.underline, Modifier::UNDERLINED),
            (self.reversed, Modifier::REVERSED),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .fold(Modifier::empty(), |acc, (_, modifier)| acc | modifier);

        let style = Style::new().add_modifier(modifiers);
        let style = self.fg.map_or(style, |ColorSpec(c)| style.fg(c));
        self.bg.map_or(style, |ColorSpec(c)| style.bg(c))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    name: String,
    extends: Option<String>,
    #[serde(default)]
    styles: Styles<Option<StyleSpec>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub styles: Styles<Style>,
}

#[derive(Debug, Clone)]
pub struct ThemeSet {
    themes: Vec<Theme>,
}

impl ThemeSet {
    pub fn builtin() -> Result<Self, ThemeError> {
        let mut set = Self { themes: Vec::new() };
        BUILTIN.iter().try_for_each(|(id, source)| set.add(id, source))?;
        Ok(set)
    }

    pub fn with_user_dir(mut self, dir: &Path) -> (Self, Vec<ThemeError>) {
        let files = match theme_files(dir) {
            Ok(files) => files,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return (self, Vec::new()),
            Err(source) => return (self, vec![ThemeError::Read { path: dir.to_path_buf(), source }]),
        };

        let errors = files
            .into_iter()
            .filter_map(|(id, path)| match fs::read_to_string(&path) {
                Ok(source) => self.add(&id, &source).err(),
                Err(source) => Some(ThemeError::Read { path, source }),
            })
            .collect();
        (self, errors)
    }

    pub fn themes(&self) -> &[Theme] {
        &self.themes
    }

    pub fn theme(&self, index: usize) -> &Theme {
        &self.themes[index.min(self.themes.len() - 1)]
    }

    pub fn position(&self, id: &str) -> Option<usize> {
        self.themes.iter().position(|theme| theme.id == id)
    }

    fn add(&mut self, id: &str, source: &str) -> Result<(), ThemeError> {
        if self.position(id).is_some() {
            return Err(ThemeError::Duplicate { id: id.to_owned() });
        }

        let file: ThemeFile =
            toml::from_str(source).map_err(|source| ThemeError::Parse { id: id.to_owned(), source })?;
        let base = match &file.extends {
            Some(base) => {
                let index = self
                    .position(base)
                    .ok_or_else(|| ThemeError::UnknownBase { id: id.to_owned(), base: base.clone() })?;
                self.themes[index].styles.clone()
            }
            None => Styles::default(),
        };

        self.themes.push(Theme { id: id.to_owned(), name: file.name, styles: file.styles.over(&base) });
        Ok(())
    }
}

fn theme_files(dir: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut files: Vec<_> = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == THEME_EXTENSION))
        .filter_map(|path| Some((path.file_stem()?.to_str()?.to_owned(), path)))
        .collect();
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_themes_parse() {
        let set = ThemeSet::builtin().unwrap_or_else(|e| panic!("{e}"));
        let ids: Vec<_> = set.themes().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["midnight", "classic", "paper", "ember"]);
    }

    #[test]
    fn root_theme_defines_every_style() {
        let (_, source) = BUILTIN[0];
        let file: ThemeFile = toml::from_str(source).unwrap_or_else(|e| panic!("{e}"));
        assert!(file.extends.is_none());
        assert_eq!(file.styles.missing(), Vec::<&str>::new());
    }

    #[test]
    fn derived_themes_inherit_missing_styles() {
        let mut set = ThemeSet::builtin().unwrap_or_else(|e| panic!("{e}"));
        set.add(
            "mine",
            "name = \"Mine\"\nextends = \"classic\"\n[styles]\ncursor = { fg = \"#ff0000\", bold = true }\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));

        let mine = &set.themes()[set.position("mine").unwrap_or_default()];
        let classic = &set.themes()[set.position("classic").unwrap_or_default()];
        assert_eq!(mine.styles.cursor, Style::new().fg(Color::Rgb(255, 0, 0)).add_modifier(Modifier::BOLD));
        assert_eq!(mine.styles.panel, classic.styles.panel);
    }

    #[test]
    fn rejects_bad_themes() {
        let mut set = ThemeSet::builtin().unwrap_or_else(|e| panic!("{e}"));
        let unknown_color = set.add("a", "name = \"A\"\n[styles]\npanel = { fg = \"chartreuse-ish\" }\n");
        let unknown_key = set.add("b", "name = \"B\"\n[styles]\npanell = { fg = \"red\" }\n");
        let unknown_base = set.add("c", "name = \"C\"\nextends = \"nope\"\n");
        let duplicate = set.add("paper", "name = \"P\"\n");

        assert!(matches!(unknown_color, Err(ThemeError::Parse { .. })));
        assert!(matches!(unknown_key, Err(ThemeError::Parse { .. })));
        assert!(matches!(unknown_base, Err(ThemeError::UnknownBase { .. })));
        assert!(matches!(duplicate, Err(ThemeError::Duplicate { .. })));
    }

    #[test]
    fn loads_user_directory_and_reports_errors() {
        let dir = std::env::temp_dir().join(format!("borg-theme-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        fs::write(dir.join("good.toml"), "name = \"Good\"\nextends = \"ember\"\n").unwrap_or_else(|e| panic!("{e}"));
        fs::write(dir.join("bad.toml"), "name = 3").unwrap_or_else(|e| panic!("{e}"));
        fs::write(dir.join("notes.txt"), "ignored").unwrap_or_else(|e| panic!("{e}"));

        let (set, errors) = ThemeSet::builtin().unwrap_or_else(|e| panic!("{e}")).with_user_dir(&dir);
        fs::remove_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));

        assert!(set.position("good").is_some());
        assert_eq!(errors.len(), 1);
    }
}
