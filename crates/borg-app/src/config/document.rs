//! The configuration file as the user keeps it. Borg reads it setting by setting, so one it
//! cannot use costs only that setting, and says what is wrong with each. When Borg saves, it
//! changes the settings that changed and leaves the rest of the file, with its comments and its
//! order, as the user wrote it.
//!
//! A setting Borg no longer has is listed in [`RETIRED`], once, with what became of it. Such a
//! setting loads without complaint and leaves the file the next time Borg saves.

use anyhow::{Context, Result};
use borg_i18n::tr;
use serde::{Serialize, de::DeserializeOwned};
use std::ops::Range;
use toml::{Table, Value};
use toml_edit::{Array, ArrayOfTables, Document, DocumentMut, InlineTable, Item, TableLike};

const PATH_SEPARATOR: char = '.';

/// What a retired setting turns into: the setting that took its place, and the value there.
type Successor = fn(&Item) -> Option<(&'static str, Item)>;

/// A setting Borg no longer has, by its place in the file, such as `watch.old-name`.
struct Retired {
    key: &'static str,
    /// What takes its place, for a setting that was renamed or reshaped; nothing for one that
    /// was dropped.
    successor: Option<Successor>,
}

const fn dropped(key: &'static str) -> Retired {
    Retired { key, successor: None }
}

/// Every setting Borg has retired. Add a line here when a setting goes; nothing else in Borg
/// needs to know it existed.
const RETIRED: [Retired; 6] = [
    dropped("animations"),
    dropped("startup-animation"),
    dropped("delight"),
    dropped("icons"),
    dropped("pixel-chrome"),
    Retired { key: "nerd-font", successor: Some(nerd_font) },
];

/// `nerd-font = true` asked for Nerd Font icons everywhere, which `icon-style` now says.
fn nerd_font(old: &Item) -> Option<(&'static str, Item)> {
    (old.as_bool() == Some(true)).then(|| ("icon-style", toml_edit::value("nerd")))
}

/// What Borg tells the user about their file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remark {
    /// A setting Borg does not have. It stays in the file and is not used.
    Unknown { key: String, line: usize },
    /// A setting whose value Borg cannot use, and why. Borg uses its default for it.
    Invalid { key: String, line: usize, reason: String },
    /// A setting Borg no longer has, which leaves the file at the next save.
    Retired { key: String },
    /// A setting that has another name or shape now, which the next save writes.
    Moved { key: String, now: String },
}

impl Remark {
    /// The remark as Borg tells it to the user.
    pub fn told(&self) -> String {
        match self {
            Self::Unknown { key, line } => tr!(config_unknown_setting, line = line, key = key),
            Self::Invalid { key, line, reason } => {
                tr!(config_invalid_setting, line = line, key = key, reason = reason)
            }
            Self::Retired { key } => tr!(config_retired_setting, key = key),
            Self::Moved { key, now } => tr!(config_moved_setting, key = key, now = now),
        }
    }
}

/// A configuration read from the user's file, with what Borg has to say about the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded<T> {
    pub settings: T,
    pub remarks: Vec<Remark>,
}

fn parts(key: &str) -> Vec<&str> {
    key.split(PATH_SEPARATOR).collect()
}

fn table_at<'a>(table: &'a mut dyn TableLike, path: &[&str]) -> Option<&'a mut dyn TableLike> {
    match path {
        [] => Some(table),
        [first, rest @ ..] => table_at(table.get_mut(first)?.as_table_like_mut()?, rest),
    }
}

fn remove(document: &mut DocumentMut, key: &str) -> Option<Item> {
    let path = parts(key);
    let (name, folders) = path.split_last()?;
    table_at(document.as_table_mut(), folders)?.remove(name)
}

fn insert_new(document: &mut DocumentMut, key: &str, item: Item) {
    let path = parts(key);
    let Some((name, folders)) = path.split_last() else { return };
    if let Some(table) = table_at(document.as_table_mut(), folders)
        && !table.contains_key(name)
    {
        table.insert(name, item);
    }
}

/// Takes the retired settings out of the document, puts what took their place in, and says
/// which it found.
fn retire(document: &mut DocumentMut) -> Vec<Remark> {
    RETIRED
        .iter()
        .filter_map(|retired| {
            let old = remove(document, retired.key)?;
            let key = retired.key.to_owned();
            Some(match retired.successor.and_then(|successor| successor(&old)) {
                Some((now, item)) => {
                    insert_new(document, now, item);
                    Remark::Moved { key, now: now.to_owned() }
                }
                None => Remark::Retired { key },
            })
        })
        .collect()
}

fn line_of(text: &str, at: usize) -> usize {
    text.bytes().take(at).filter(|byte| *byte == b'\n').count() + 1
}

/// Where in the file the setting lies that the span `at` belongs to, and whether the span is
/// the setting's name rather than its value. A section's own span covers only its heading, so
/// the settings inside every section are looked at before the section itself.
fn setting_at(table: &dyn TableLike, at: &Range<usize>) -> Option<(Vec<String>, bool)> {
    let within = |span: Option<Range<usize>>| span.is_some_and(|span| span.start <= at.start && at.start < span.end);
    table.iter().find_map(|(name, item)| {
        let named = within(table.key(name).and_then(toml_edit::Key::span));
        let inside = item.as_table_like().and_then(|section| setting_at(section, at));
        let (mut path, name_itself) = match inside {
            Some(deeper) => deeper,
            None if named || within(item.span()) => (Vec::new(), named),
            None => return None,
        };
        path.insert(0, name.to_owned());
        Some((path, name_itself))
    })
}

/// Reads the settings out of `text`. A setting Borg cannot use is left out and remarked on, and
/// the rest still load. Only a file that is not TOML at all fails.
pub fn load<T: DeserializeOwned>(text: &str) -> Result<Loaded<T>> {
    let mut document: DocumentMut = text.parse().context("the file is not valid TOML")?;
    let mut remarks = retire(&mut document);
    let mut current = document.to_string();
    loop {
        let error = match toml::from_str::<T>(&current) {
            Ok(settings) => return Ok(Loaded { settings, remarks }),
            Err(error) => error,
        };
        let spans = Document::parse(current.clone()).context("the file is not valid TOML")?;
        let found = error.span().and_then(|span| setting_at(spans.as_table(), &span).map(|found| (span, found)));
        let Some((span, (path, name_itself))) = found else { return Err(error).context("reading the settings") };
        let key = path.join(&PATH_SEPARATOR.to_string());
        let line = original_line(text, &key).unwrap_or_else(|| line_of(&current, span.start));
        remarks.push(match name_itself {
            true => Remark::Unknown { key: key.clone(), line },
            false => Remark::Invalid { key: key.clone(), line, reason: error.message().to_owned() },
        });
        let mut without: DocumentMut = current.parse().context("the file is not valid TOML")?;
        remove(&mut without, &key);
        current = without.to_string();
    }
}

/// The line of the user's own file a setting is on, which the lines of a file with other
/// settings already left out no longer tell.
fn original_line(text: &str, key: &str) -> Option<usize> {
    let spans = Document::parse(text.to_owned()).ok()?;
    let path = parts(key);
    let (name, folders) = path.split_last()?;
    let table = folders.iter().try_fold(spans.as_table() as &dyn TableLike, |table, folder| {
        table.get(folder).and_then(Item::as_table_like)
    })?;
    table.key(name).and_then(toml_edit::Key::span).map(|span| line_of(text, span.start))
}

fn value_of(value: &Value) -> toml_edit::Value {
    match value {
        Value::String(text) => text.as_str().into(),
        Value::Integer(number) => (*number).into(),
        Value::Float(number) => (*number).into(),
        Value::Boolean(flag) => (*flag).into(),
        Value::Datetime(moment) => (*moment).into(),
        Value::Array(values) => toml_edit::Value::Array(values.iter().map(value_of).collect::<Array>()),
        Value::Table(table) => {
            let mut inline = InlineTable::new();
            table.iter().for_each(|(name, value)| _ = inline.insert(name, value_of(value)));
            toml_edit::Value::InlineTable(inline)
        }
    }
}

fn table_of(table: &Table) -> toml_edit::Table {
    let mut written = toml_edit::Table::new();
    table.iter().for_each(|(name, value)| _ = written.insert(name, item_of(value)));
    written
}

/// A value as the file writes it: a section for a table, `[[name]]` entries for a list of
/// tables, and a plain value otherwise.
fn item_of(value: &Value) -> Item {
    match value {
        Value::Table(table) => Item::Table(table_of(table)),
        Value::Array(values) if !values.is_empty() && values.iter().all(Value::is_table) => {
            let tables = values.iter().filter_map(Value::as_table).map(table_of);
            Item::ArrayOfTables(tables.collect::<ArrayOfTables>())
        }
        other => Item::Value(value_of(other)),
    }
}

/// Writes one setting, keeping the comments the user wrote above and beside the one it replaces.
fn set(table: &mut dyn TableLike, name: &str, value: &Value) {
    let mut item = item_of(value);
    if let (Some(new), Some(old)) = (item.as_value_mut(), table.get(name).and_then(Item::as_value)) {
        *new.decor_mut() = old.decor().clone();
    }
    match table.get_mut(name) {
        Some(written) => *written = item,
        None => _ = table.insert(name, item),
    }
}

/// Brings the file's settings to `after`, where `before` is what Borg read from it: a setting
/// that changed is written, one that went is removed, and everything else stays as it is.
fn merge(file: &mut dyn TableLike, before: &Table, after: &Table) {
    for (name, value) in after {
        let sectioned = file.get(name).is_some_and(Item::is_table_like);
        match value {
            Value::Table(now) if sectioned => {
                let was = before.get(name).and_then(Value::as_table).cloned().unwrap_or_default();
                if let Some(section) = file.get_mut(name).and_then(Item::as_table_like_mut) {
                    merge(section, &was, now);
                }
            }
            _ if before.get(name) != Some(value) || !file.contains_key(name) => set(file, name, value),
            _ => {}
        }
    }
    for name in before.keys().filter(|name| !after.contains_key(*name)) {
        file.remove(name);
    }
}

/// The file's text with `settings` saved into it.
pub fn save<T: Serialize + DeserializeOwned>(text: &str, settings: &T) -> Result<String> {
    let before = Table::try_from(&load::<T>(text)?.settings)?;
    let after = Table::try_from(settings)?;
    let mut document: DocumentMut = text.parse().context("the file is not valid TOML")?;
    retire(&mut document);
    merge(document.as_table_mut(), &before, &after);
    Ok(document.to_string())
}
