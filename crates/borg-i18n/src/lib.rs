#[macro_use]
mod en;

pub const SOURCE_LOCALE: &str = "en";

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
        assert_eq!(format!(tr!(collection_title), name = name), "Collection: 日本語 {draft}");
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
}

pub fn count(count: usize, noun: Count) -> String {
    match (noun, count == 1) {
        (Count::File, true) => format!(tr!(count_file_one), count = count),
        (Count::File, false) => format!(tr!(count_file_many), count = count),
        (Count::Folder, true) => format!(tr!(count_folder_one), count = count),
        (Count::Folder, false) => format!(tr!(count_folder_many), count = count),
        (Count::Item, true) => format!(tr!(count_item_one), count = count),
        (Count::Item, false) => format!(tr!(count_item_many), count = count),
        (Count::Error, true) => format!(tr!(count_error_one), count = count),
        (Count::Error, false) => format!(tr!(count_error_many), count = count),
    }
}
