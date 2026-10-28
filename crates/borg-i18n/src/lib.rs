mod en;
pub use en::MESSAGES;

pub const SOURCE_LOCALE: &str = "en";

use std::{
    cell::Cell,
    sync::atomic::{AtomicU8, Ordering},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Locale {
    #[default]
    English,
    Chinese,
    Portuguese,
    Spanish,
    Russian,
}

impl Locale {
    pub const ALL: [Self; 5] = [Self::English, Self::Chinese, Self::Portuguese, Self::Spanish, Self::Russian];

    pub fn parse(value: &str) -> Option<Self> {
        match value.split(['-', '_', '.', '@']).next()?.to_ascii_lowercase().as_str() {
            "en" | "c" | "posix" => Some(Self::English),
            "zh" => Some(Self::Chinese),
            "pt" => Some(Self::Portuguese),
            "es" => Some(Self::Spanish),
            "ru" => Some(Self::Russian),
            _ => None,
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Chinese => "zh-CN",
            Self::Portuguese => "pt",
            Self::Spanish => "es",
            Self::Russian => "ru",
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Chinese => "简体中文",
            Self::Portuguese => "Português (Portugal)",
            Self::Spanish => "Español",
            Self::Russian => "Русский",
        }
    }
}

static LOCALE: AtomicU8 = AtomicU8::new(Locale::English as u8);

thread_local! {
    static OVERRIDE: Cell<Option<Locale>> = const { Cell::new(None) };
}

pub fn locale() -> Locale {
    OVERRIDE.get().unwrap_or_else(|| Locale::ALL[usize::from(LOCALE.load(Ordering::Relaxed))])
}

pub fn set_locale(locale: Locale) {
    LOCALE.store(locale as u8, Ordering::Relaxed);
}

pub fn with_locale<T>(locale: Locale, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<Locale>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDE.set(self.0);
        }
    }
    let _restore = Restore(OVERRIDE.replace(Some(locale)));
    run()
}

pub fn detect(requested: Option<&str>, system: Option<&str>) -> Locale {
    requested.or(system).and_then(Locale::parse).unwrap_or_default()
}

include!(concat!(env!("OUT_DIR"), "/messages.rs"));

#[cfg(test)]
mod tests {
    #[test]
    fn counts_keep_zero_singular_and_plural_forms_in_the_catalog() {
        use super::{Count, count};
        assert_eq!(count(0, Count::File), "0 files");
        assert_eq!(count(1, Count::File), "1 file");
        assert_eq!(count(2, Count::File), "2 files");
        assert_eq!(count(1, Count::Error), "1 error");
        assert_eq!(count(2, Count::Folder), "2 folders");
    }

    #[test]
    fn message_arguments_preserve_braces_and_unicode_in_user_text() {
        let name = "日本語 {draft}";
        assert_eq!(tr!(collection_title, name = name), "Collection: 日本語 {draft}");
    }

    #[test]
    fn catalog_keys_are_unique_and_messages_are_not_empty() {
        let mut seen = std::collections::HashSet::new();
        for (key, message) in super::en::MESSAGES {
            assert!(seen.insert(key), "duplicate message key: {key}");
            assert!(!message.trim().is_empty(), "empty message: {key}");
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Count {
    File,
    Folder,
    Item,
    Error,
    Region,
}

pub fn count(count: usize, noun: Count) -> String {
    let singular = if locale() == Locale::Russian { count % 10 == 1 && count % 100 != 11 } else { count == 1 };
    if locale() == Locale::Russian && (2..=4).contains(&(count % 10)) && !(12..=14).contains(&(count % 100)) {
        return match noun {
            Count::File => tr!(count_file_few, count = count),
            Count::Folder => tr!(count_folder_few, count = count),
            Count::Item => tr!(count_item_few, count = count),
            Count::Error => tr!(count_error_few, count = count),
            Count::Region => tr!(count_region_few, count = count),
        };
    }
    match (noun, singular) {
        (Count::File, true) => tr!(count_file_one, count = count),
        (Count::File, false) => tr!(count_file_many, count = count),
        (Count::Folder, true) => tr!(count_folder_one, count = count),
        (Count::Folder, false) => tr!(count_folder_many, count = count),
        (Count::Item, true) => tr!(count_item_one, count = count),
        (Count::Item, false) => tr!(count_item_many, count = count),
        (Count::Error, true) => tr!(count_error_one, count = count),
        (Count::Error, false) => tr!(count_error_many, count = count),
        (Count::Region, true) => tr!(count_region_one, count = count),
        (Count::Region, false) => tr!(count_region_many, count = count),
    }
}

#[cfg(test)]
mod locale_tests {
    use super::*;

    #[test]
    fn language_tags_and_precedence_are_explicit() {
        for (tag, expected) in [
            ("zh_Hans_CN.UTF-8", Locale::Chinese),
            ("pt-PT", Locale::Portuguese),
            ("es_MX.UTF-8", Locale::Spanish),
            ("ru_RU.UTF-8", Locale::Russian),
            ("C.UTF-8", Locale::English),
        ] {
            assert_eq!(Locale::parse(tag), Some(expected));
        }
        assert_eq!(detect(Some("es"), Some("ru_RU")), Locale::Spanish);
        assert_eq!(detect(None, Some("ru_RU")), Locale::Russian);
        assert_eq!(detect(Some("unsupported"), Some("ru_RU")), Locale::English);
        assert_eq!(detect(None, None), Locale::English);
    }

    #[test]
    fn translated_messages_keep_user_text_and_numeric_formatting() {
        for language in Locale::ALL {
            with_locale(language, || {
                let name = "中文 {draft} café";
                assert!(tr!(collection_title, name = name).contains(name));
                let details = tr!(activity_capture_label, pid = 42, address = 0x2a, bytes = 256, mode = "hex");
                assert!(details.contains("0x2a"));
                assert!(details.contains("256"));
                assert!(!tr!(quit).is_empty());
            });
        }
    }

    #[test]
    fn russian_counts_handle_teens_and_compound_numbers() {
        with_locale(Locale::Russian, || {
            for (number, expected) in [
                (0, "0 файлов"),
                (1, "1 файл"),
                (2, "2 файла"),
                (5, "5 файлов"),
                (11, "11 файлов"),
                (12, "12 файлов"),
                (21, "21 файл"),
                (22, "22 файла"),
                (25, "25 файлов"),
                (101, "101 файл"),
                (111, "111 файлов"),
            ] {
                assert_eq!(count(number, Count::File), expected);
            }
        });
    }

    #[test]
    fn language_scopes_restore_after_nesting_and_panics() {
        let before = locale();
        with_locale(Locale::Spanish, || {
            assert_eq!(tr!(quit), "Salir");
            with_locale(Locale::Chinese, || assert_eq!(tr!(quit), "退出"));
            assert_eq!(locale(), Locale::Spanish);
        });
        let _ = std::panic::catch_unwind(|| with_locale(Locale::Russian, || panic!("scope test")));
        assert_eq!(locale(), before);
    }
}
