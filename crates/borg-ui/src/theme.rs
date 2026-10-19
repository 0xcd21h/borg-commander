use std::{
    fs, io,
    path::{Path, PathBuf},
    str::FromStr,
};

use borg_core::{Appearance, ThemeChoice};
use ratatui::{
    style::{Color, Modifier, Style},
    widgets::BorderType,
};
use serde::Deserialize;

const THEME_EXTENSION: &str = "toml";

const BUILTIN: [(&str, &str); 7] = [
    ("midnight", include_str!("../themes/midnight.toml")),
    ("aurora", include_str!("../themes/aurora.toml")),
    ("neon", include_str!("../themes/neon.toml")),
    ("classic", include_str!("../themes/classic.toml")),
    ("paper", include_str!("../themes/paper.toml")),
    ("ember", include_str!("../themes/ember.toml")),
    ("cube", include_str!("../themes/cube.toml")),
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
    input,
    marked,
    marked_cursor,
    status,
    menu,
    menu_selected,
    menu_disabled,
    viewer,
    viewer_status,
    separator,
    kind_code,
    kind_image,
    kind_archive,
    kind_document,
    kind_media,
    kind_config,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Border {
    #[default]
    Plain,
    Rounded,
    Double,
    Thick,
}

impl From<Border> for BorderType {
    fn from(border: Border) -> Self {
        match border {
            Border::Plain => Self::Plain,
            Border::Rounded => Self::Rounded,
            Border::Double => Self::Double,
            Border::Thick => Self::Thick,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CursorShape {
    #[default]
    Bar,
    Pill,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Effects {
    pub cursor: CursorShape,
    pub inactive_dim: f32,
    pub accent: [Color; 2],
    pub border_gradient: Option<[Color; 2]>,
    pub title_gradient: Option<[Color; 2]>,
    pub cursor_gradient: Option<[Color; 2]>,
    pub glow: f32,
    pub breathe: bool,
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            cursor: CursorShape::Bar,
            inactive_dim: 0.0,
            accent: [Color::Cyan, Color::Blue],
            border_gradient: None,
            title_gradient: None,
            cursor_gradient: None,
            glow: 0.0,
            breathe: false,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct EffectsSpec {
    cursor: Option<CursorShape>,
    inactive_dim: Option<f32>,
    accent: Option<[ColorSpec; 2]>,
    border_gradient: Option<[ColorSpec; 2]>,
    title_gradient: Option<[ColorSpec; 2]>,
    cursor_gradient: Option<[ColorSpec; 2]>,
    glow: Option<f32>,
    breathe: Option<bool>,
}

impl EffectsSpec {
    fn over(self, base: Effects) -> Effects {
        let pair = |spec: Option<[ColorSpec; 2]>| spec.map(|[ColorSpec(a), ColorSpec(b)]| [a, b]);
        Effects {
            cursor: self.cursor.unwrap_or(base.cursor),
            inactive_dim: self.inactive_dim.unwrap_or(base.inactive_dim).clamp(0.0, 1.0),
            accent: pair(self.accent).unwrap_or(base.accent),
            border_gradient: pair(self.border_gradient).or(base.border_gradient),
            title_gradient: pair(self.title_gradient).or(base.title_gradient),
            cursor_gradient: pair(self.cursor_gradient).or(base.cursor_gradient),
            glow: self.glow.unwrap_or(base.glow).clamp(0.0, 1.0),
            breathe: self.breathe.unwrap_or(base.breathe),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    name: String,
    extends: Option<String>,
    border: Option<Border>,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    effects: EffectsSpec,
    #[serde(default)]
    styles: Styles<Option<StyleSpec>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub border: Border,
    pub hidden: bool,
    pub effects: Effects,
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

    pub fn appearance(&self, current: Option<&str>, unlocked: &[String]) -> Appearance {
        let choice = |theme: &Theme| ThemeChoice { id: theme.id.clone(), name: theme.name.clone() };
        let (visible, hidden): (Vec<&Theme>, Vec<&Theme>) =
            self.themes.iter().partition(|theme| !theme.hidden || unlocked.contains(&theme.id));
        let choices: Vec<ThemeChoice> = visible.into_iter().map(choice).collect();
        let index = current.and_then(|id| choices.iter().position(|c| c.id == id)).unwrap_or_default();
        let secret = hidden.into_iter().next().map(choice);
        Appearance::new(choices, index).with_secret(secret, unlocked.to_vec())
    }

    pub fn find(&self, id: Option<&str>) -> &Theme {
        let index = id.and_then(|id| self.position(id)).unwrap_or_default();
        &self.themes[index]
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
        let (border, effects, styles) = match &file.extends {
            Some(base) => {
                let index = self
                    .position(base)
                    .ok_or_else(|| ThemeError::UnknownBase { id: id.to_owned(), base: base.clone() })?;
                let base = &self.themes[index];
                (base.border, base.effects, base.styles.clone())
            }
            None => (Border::default(), Effects::default(), Styles::default()),
        };

        self.themes.push(Theme {
            id: id.to_owned(),
            name: file.name,
            border: file.border.unwrap_or(border),
            hidden: file.hidden,
            effects: file.effects.over(effects),
            styles: file.styles.over(&styles),
        });
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
        assert_eq!(ids, ["midnight", "aurora", "neon", "classic", "paper", "ember", "cube"]);
        assert_eq!(set.themes().iter().filter(|t| t.hidden).map(|t| t.id.as_str()).collect::<Vec<_>>(), ["cube"]);
    }

    #[test]
    fn hidden_themes_stay_secret_until_unlocked() {
        let set = ThemeSet::builtin().unwrap_or_else(|e| panic!("{e}"));
        let ids = |appearance: &Appearance| appearance.choices().iter().map(|c| c.id.clone()).collect::<Vec<_>>();
        let locked = set.appearance(Some("neon"), &[]);
        assert!(!ids(&locked).contains(&"cube".to_owned()));
        assert!(locked.has_secret());
        assert_eq!(locked.current_id(), Some("neon"));

        let unlocked = set.appearance(Some("cube"), &["cube".to_owned()]);
        assert_eq!(unlocked.current_id(), Some("cube"));
        assert!(!unlocked.has_secret());
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
    fn border_style_is_inherited_and_overridable() {
        let mut set = ThemeSet::builtin().unwrap_or_else(|e| panic!("{e}"));
        set.add("soft", "name = \"Soft\"\nextends = \"aurora\"\n").unwrap_or_else(|e| panic!("{e}"));
        set.add("boxy", "name = \"Boxy\"\nextends = \"aurora\"\nborder = \"double\"\n")
            .unwrap_or_else(|e| panic!("{e}"));

        let border = |id: &str| set.themes()[set.position(id).unwrap_or_default()].border;
        assert_eq!(border("classic"), Border::Plain);
        assert_eq!(border("soft"), Border::Rounded);
        assert_eq!(border("boxy"), Border::Double);
    }

    #[test]
    fn effects_inherit_and_override() {
        let mut set = ThemeSet::builtin().unwrap_or_else(|e| panic!("{e}"));
        set.add("glowy", "name = \"Glowy\"\nextends = \"neon\"\n[effects]\nglow = 0.9\n")
            .unwrap_or_else(|e| panic!("{e}"));
        let neon = &set.themes()[set.position("neon").unwrap_or_default()];
        let glowy = &set.themes()[set.position("glowy").unwrap_or_default()];
        assert!((glowy.effects.glow - 0.9).abs() < f32::EPSILON);
        assert_eq!(glowy.effects.border_gradient, neon.effects.border_gradient);
        assert_eq!(glowy.effects.cursor, CursorShape::Pill);
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
