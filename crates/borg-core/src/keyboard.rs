//! Keys read the same in any keyboard layout: a letter typed in a non-Latin layout stands for the
//! US key in its place, and a question's buttons answer to the letters they show.

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The keys of a US keyboard, row by row from the one with the digits.
const US_KEYS: &str = "`1234567890-=qwertyuiop[]\\asdfghjkl;'zxcvbnm,./";
/// What the same keys type with Shift held.
const US_SHIFTED: &str = "~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:\"ZXCVBNM<>?";

/// What a layout types on each key, in the order of `US_KEYS` and `US_SHIFTED`. Scripts with
/// capitals leave `shifted` empty: a capital stands for the shifted key of its small letter.
struct Layout {
    keys: &'static str,
    shifted: &'static str,
}

const fn with_capitals(keys: &'static str) -> Layout {
    Layout { keys, shifted: "" }
}

const RUSSIAN: Layout = with_capitals("ё1234567890-=йцукенгшщзхъ\\фывапролджэячсмитьбю.");
const UKRAINIAN: Layout = with_capitals("`1234567890-=йцукенгшщзхї\\фівапролджєячсмитьбю.");
const BELARUSIAN: Layout = with_capitals("ё1234567890-=йцукенгшўзх'\\фывапролджэячсмітьбю.");
const KAZAKH: Layout = with_capitals("`1әіңғ67үұқөһйцукенгшщзхъ\\фывапролджэячсмитьбю.");
const GREEK: Layout = with_capitals("`1234567890-=;ςερτυθιοπ[]\\ασδφγηξκλ;'ζχψωβνμ,./");
const HEBREW: Layout = with_capitals("`1234567890-=/'קראטוןםפ[]\\שדגכעיחלךף'זסבהנמצתץ.");
const ARABIC: Layout = with_capitals("ذ1234567890-=ضصثقفغعهخحجد\\شسيبلاتنمكطئءؤرbىةوزظ");
const GEORGIAN: Layout = Layout {
    keys: "`1234567890-=ქწერტყუიოპ[]\\ასდფგჰჯკლ;'ზხცვბნმ,./",
    shifted: "~!@#$%^&*()_+QჭEღთYUIOP{}|AშDFGHჟKL:\"ძXჩVBNM<>?",
};
const KOREAN: Layout = Layout {
    keys: "`1234567890-=ㅂㅈㄷㄱㅅㅛㅕㅑㅐㅔ[]\\ㅁㄴㅇㄹㅎㅗㅓㅏㅣ;'ㅋㅌㅊㅍㅠㅜㅡ,./",
    shifted: "~!@#$%^&*()_+ㅃㅉㄸㄲㅆYUIㅒㅖ{}|ASDFGHJKL:\"ZXCVBNM<>?",
};
const THAI: Layout = with_capitals("`ๅ23ภถุึคตจขชๆไำพะัีรนยบลฃฟหกดเ้่าสวงผปแอิืทมใฝ");

/// The layouts in the order they are tried: a letter that two layouts put on different keys
/// stands for its key in the first one.
const LAYOUTS: &[Layout] = &[RUSSIAN, UKRAINIAN, BELARUSIAN, KAZAKH, GREEK, HEBREW, ARABIC, GEORGIAN, KOREAN, THAI];
pub(crate) const YES_KEY: char = 'y';
pub(crate) const NO_KEY: char = 'n';

/// The key as the US layout names it, for a letter a non-Latin layout typed. Other keys stay
/// as they are.
pub(crate) fn latin(key: &KeyEvent) -> KeyEvent {
    let KeyCode::Char(typed) = key.code else { return *key };
    us_key(typed).map_or(*key, |us| KeyEvent { code: KeyCode::Char(us), ..*key })
}

fn us_key(typed: char) -> Option<char> {
    if typed.is_ascii() {
        return None;
    }
    let small = Some(lowercase(typed)).filter(|&small| small != typed);
    LAYOUTS.iter().find_map(|layout| {
        same_place(layout.keys, typed, US_KEYS)
            .or_else(|| same_place(layout.shifted, typed, US_SHIFTED))
            .or_else(|| small.and_then(|small| same_place(layout.keys, small, US_SHIFTED)))
    })
}

fn same_place(layout: &str, typed: char, us: &str) -> Option<char> {
    layout.chars().position(|letter| letter == typed).and_then(|index| us.chars().nth(index))
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

/// What a key does to a question with Yes and No buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum YesNo {
    /// The other button becomes the chosen one.
    Switch,
    /// The question closes with this answer, or with none for a letter that names neither button.
    Close(Option<bool>),
    Ignore,
}

/// The way a question with Yes and No buttons takes `key` while `accept` is the chosen one:
/// Enter takes the chosen button, Esc says No, a letter answers, and the arrows and Tab switch.
pub(crate) fn yes_no(accept: bool, key: &KeyEvent) -> YesNo {
    match key.code {
        KeyCode::Char(_) => YesNo::Close(answer(key)),
        KeyCode::Esc => YesNo::Close(Some(false)),
        KeyCode::Enter => YesNo::Close(Some(accept)),
        KeyCode::Left | KeyCode::Right | KeyCode::Tab => YesNo::Switch,
        _ => YesNo::Ignore,
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

    fn held(letter: char) -> char {
        match latin(&typed(letter, KeyModifiers::ALT)).code {
            KeyCode::Char(us) => us,
            code => panic!("{code:?}"),
        }
    }

    #[test]
    fn every_layout_types_something_on_each_us_key() {
        let keys = US_KEYS.chars().count();
        assert_eq!(US_SHIFTED.chars().count(), keys);
        for (index, layout) in LAYOUTS.iter().enumerate() {
            assert_eq!(layout.keys.chars().count(), keys, "layout {index}");
            assert!([0, keys].contains(&layout.shifted.chars().count()), "layout {index}");
        }
    }

    #[test]
    fn no_layout_puts_a_letter_on_two_keys() {
        for (index, layout) in LAYOUTS.iter().enumerate() {
            let mut letters: Vec<char> =
                layout.keys.chars().chain(layout.shifted.chars()).filter(|c| !c.is_ascii()).collect();
            let count = letters.len();
            letters.sort_unstable();
            letters.dedup();
            assert_eq!(letters.len(), count, "layout {index}");
        }
    }

    /// The Ukrainian і, which Belarusian and Kazakh put on other keys.
    const UKRAINIAN_I: char = 'і';

    #[test]
    fn each_letter_reads_as_its_own_key_in_every_layout_but_the_ukrainian_i() {
        for (index, layout) in LAYOUTS.iter().enumerate() {
            let rows = [(layout.keys, US_KEYS), (layout.shifted, US_SHIFTED)];
            for (letter, key) in rows.into_iter().flat_map(|(row, us)| row.chars().zip(us.chars())) {
                if !letter.is_ascii() && letter != UKRAINIAN_I {
                    assert_eq!(us_key(letter), Some(key), "layout {index}, {letter}");
                }
            }
        }
    }

    #[test]
    fn letters_of_other_scripts_read_as_the_us_keys_in_their_place() {
        assert_eq!([held('κ'), held('ל'), held('ن'), held('კ'), held('ㅏ'), held('า')], ['k'; 6]);
        assert_eq!([held('ў'), held('ә'), held('ף'), held('ط')], ['o', '2', ';', '\'']);
        assert_eq!([held('Σ'), held('Х'), held('ღ'), held('ㅃ')], ['S', '{', 'R', 'Q']);
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
