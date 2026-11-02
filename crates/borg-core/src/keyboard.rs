//! Keys read the same in any keyboard layout: a letter typed in a Cyrillic layout stands for the
//! US key in its place, and a question's buttons answer to the letters they show.

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The letters of the Russian, Ukrainian and Belarusian layouts, in the order of `US_KEYS`.
const CYRILLIC_KEYS: &str = "йцукенгшщзхъфывапролджэячсмитьбюёіїєўЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭЯЧСМИТЬБЮЁІЇЄЎ";
/// The US key in the place of each of `CYRILLIC_KEYS`.
const US_KEYS: &str = "qwertyuiop[]asdfghjkl;'zxcvbnm,.`s]'oQWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>~S}\"O";
const YES_KEY: char = 'y';
const NO_KEY: char = 'n';

/// The key as the US layout names it, for a letter a Cyrillic layout typed. Other keys stay
/// as they are.
pub(crate) fn latin(key: &KeyEvent) -> KeyEvent {
    let KeyCode::Char(typed) = key.code else { return *key };
    CYRILLIC_KEYS
        .chars()
        .position(|letter| letter == typed)
        .and_then(|index| US_KEYS.chars().nth(index))
        .map_or(*key, |us| KeyEvent { code: KeyCode::Char(us), ..*key })
}

/// A key held with Ctrl or Alt is a command in any layout, so it is read as the US key.
pub(crate) fn latin_held(key: &KeyEvent) -> KeyEvent {
    if key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) { latin(key) } else { *key }
}

/// The answer a key gives a question with Yes and No buttons: the first letter of either
/// button in the interface's language, or Y and N in any language.
pub(crate) fn answer(key: &KeyEvent) -> Option<bool> {
    let KeyCode::Char(typed) = key.code else { return None };
    let typed = lowercase(typed);
    let (yes, no) = (initial(tr!(yes)), initial(tr!(no)));
    if yes != no {
        if yes == Some(typed) {
            return Some(true);
        }
        if no == Some(typed) {
            return Some(false);
        }
    }
    let KeyCode::Char(us) = latin(key).code else { return None };
    match lowercase(us) {
        YES_KEY => Some(true),
        NO_KEY => Some(false),
        _ => None,
    }
}

/// The first letter of a button's label, without its brackets.
fn initial(label: &str) -> Option<char> {
    label.chars().find(|c| c.is_alphabetic()).map(lowercase)
}

fn lowercase(letter: char) -> char {
    letter.to_lowercase().next().unwrap_or(letter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use borg_i18n::{Locale, with_locale};

    fn typed(letter: char, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(letter), modifiers)
    }

    #[test]
    fn every_cyrillic_letter_has_a_us_key() {
        assert_eq!(CYRILLIC_KEYS.chars().count(), US_KEYS.chars().count());
        let mut letters: Vec<char> = CYRILLIC_KEYS.chars().collect();
        letters.sort_unstable();
        letters.dedup();
        assert_eq!(letters.len(), CYRILLIC_KEYS.chars().count());
    }

    #[test]
    fn cyrillic_letters_read_as_the_us_keys_in_their_place() {
        assert_eq!(latin(&typed('т', KeyModifiers::ALT)), typed('n', KeyModifiers::ALT));
        assert_eq!(latin(&typed('щ', KeyModifiers::CONTROL)), typed('o', KeyModifiers::CONTROL));
        assert_eq!(latin(&typed('Ф', KeyModifiers::ALT)), typed('A', KeyModifiers::ALT));
        assert_eq!(latin(&typed('ю', KeyModifiers::ALT)), typed('.', KeyModifiers::ALT));
        assert_eq!(latin(&typed('і', KeyModifiers::NONE)), typed('s', KeyModifiers::NONE));
    }

    #[test]
    fn other_keys_stay_as_typed() {
        for key in [typed('n', KeyModifiers::ALT), typed('ñ', KeyModifiers::NONE), KeyEvent::from(KeyCode::F(5))] {
            assert_eq!(latin(&key), key);
        }
    }

    #[test]
    fn only_keys_held_with_ctrl_or_alt_change_on_their_own() {
        assert_eq!(latin_held(&typed('т', KeyModifiers::NONE)), typed('т', KeyModifiers::NONE));
        assert_eq!(latin_held(&typed('т', KeyModifiers::SHIFT)), typed('т', KeyModifiers::SHIFT));
        assert_eq!(latin_held(&typed('с', KeyModifiers::CONTROL)), typed('c', KeyModifiers::CONTROL));
    }

    #[test]
    fn yes_and_no_answer_to_their_buttons_letters() {
        let answers =
            |letters: &str| letters.chars().map(|c| answer(&typed(c, KeyModifiers::NONE))).collect::<Vec<_>>();
        assert_eq!(
            with_locale(Locale::English, || answers("ynYNx")),
            [Some(true), Some(false), Some(true), Some(false), None]
        );
        assert_eq!(
            with_locale(Locale::Russian, || answers("днтyn")),
            [Some(true), Some(false), Some(false), Some(true), Some(false)]
        );
        assert_eq!(with_locale(Locale::Spanish, || answers("sny")), [Some(true), Some(false), Some(true)]);
        assert_eq!(with_locale(Locale::Portuguese, || answers("sn")), [Some(true), Some(false)]);
        assert_eq!(
            with_locale(Locale::Chinese, || answers("是否yn")),
            [Some(true), Some(false), Some(true), Some(false)]
        );
    }

    #[test]
    fn every_language_keeps_its_button_letters_apart_from_the_other_answer() {
        for locale in Locale::ALL {
            with_locale(locale, || {
                let (yes, no) = (initial(tr!(yes)), initial(tr!(no)));
                assert_ne!(yes, no, "{}", locale.code());
                assert_ne!(yes, Some(NO_KEY), "{}", locale.code());
                assert_ne!(no, Some(YES_KEY), "{}", locale.code());
            });
        }
    }
}
