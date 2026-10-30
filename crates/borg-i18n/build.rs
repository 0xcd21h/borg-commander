use std::{collections::BTreeMap, env, fs, path::PathBuf};

const LOCALES: &[(&str, &str)] = &[("zh-CN", "Chinese"), ("pt", "Portuguese"), ("es", "Spanish"), ("ru", "Russian")];

fn fields(value: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut remainder = value;
    while let Some(start) = remainder.find('{') {
        remainder = &remainder[start + 1..];
        if remainder.starts_with('{') {
            remainder = &remainder[1..];
            continue;
        }
        let end = remainder.find('}').expect("unclosed message placeholder");
        fields.push(remainder[..end].to_owned());
        remainder = &remainder[end + 1..];
    }
    fields.sort();
    fields
}

fn pressed_keys(label: &str) -> Vec<&str> {
    label
        .split(" · ")
        .map(|part| {
            let part = part.trim_start();
            let start = if part.starts_with("[ ") { 2 } else { 0 };
            part[start..].find(' ').map_or(part, |end| &part[..start + end])
        })
        .collect()
}

/// Whether a word of a hint names a key: `Enter`, `F7`, `Ctrl-O`, `↑↓`, `/`, a letter or a digit, or
/// several of them joined by `/`.
fn is_key(word: &str) -> bool {
    const NAMED: &[&str] = &["Enter", "Tab", "Space", "Esc", "Ins", "Backspace", "PgUp", "PgDn"];
    word == "/"
        || !word.is_empty()
            && word.split('/').all(|key| {
                NAMED.contains(&key)
                    || ["Alt-", "Ctrl-", "Shift-"].iter().any(|held| key.len() > held.len() && key.starts_with(held))
                    || key.strip_prefix('F').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                    || key.len() == 1 && key.bytes().all(|b| b.is_ascii_alphanumeric())
                    || !key.is_empty() && key.chars().all(|c| matches!(c, '↑' | '↓' | '←' | '→'))
            })
}

fn variant(key: &str) -> String {
    key.split('_')
        .map(|part| {
            let mut letters = part.chars();
            letters.next().map_or_else(String::new, |first| first.to_uppercase().collect::<String>() + letters.as_str())
        })
        .collect()
}

fn main() {
    println!("cargo:rerun-if-changed=src/en.rs");
    println!("cargo:rerun-if-changed=locales");
    let source = fs::read_to_string("src/en.rs").expect("English catalog");
    let english: Vec<(String, String)> = source
        .lines()
        .filter_map(|line| {
            let (key, literal) = line.trim().split_once("=>")?;
            let literal = literal.trim().strip_suffix(',')?;
            let value = serde_json::from_str::<String>(literal).ok()?;
            Some((key.trim().to_owned(), value))
        })
        .collect();
    assert!(!english.is_empty(), "English catalog must not be empty");
    let mut catalogs = Vec::new();
    for &(code, variant) in LOCALES {
        let file = format!("locales/{code}.json");
        let values: BTreeMap<String, String> =
            serde_json::from_str(&fs::read_to_string(&file).expect(&file)).expect("valid catalog JSON");
        assert_eq!(values.len(), english.len(), "{code}: catalog key count");
        for (key, value) in &english {
            let translated = values.get(key).unwrap_or_else(|| panic!("{code}: missing {key}"));
            assert!(!translated.trim().is_empty(), "{code}: empty {key}");
            assert_eq!(fields(value), fields(translated), "{code}: placeholders in {key}");
            assert_eq!(value.starts_with(' '), translated.starts_with(' '), "{code}: leading padding in {key}");
            assert_eq!(value.ends_with(' '), translated.ends_with(' '), "{code}: trailing padding in {key}");
            if key.ends_with("_button") || key.ends_with("_shortcut") || key.contains("_button_") {
                assert_eq!(pressed_keys(value), pressed_keys(translated), "{code}: keys to press in {key}");
            } else if value.contains(" · ") && pressed_keys(value).into_iter().any(is_key) {
                let (keys, translated_keys) = (pressed_keys(value), pressed_keys(translated));
                assert_eq!(keys.len(), translated_keys.len(), "{code}: hint segments in {key}");
                for (pressed, translated) in keys.into_iter().zip(translated_keys) {
                    assert!(!is_key(pressed) || pressed == translated, "{code}: {pressed} to press in {key}");
                }
            }
        }
        catalogs.push((variant, values));
    }
    let mut output = String::from("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n#[repr(usize)]\npub enum Message {\n");
    for (key, _) in &english {
        output.push_str(&format!("{},\n", variant(key)));
    }
    output.push_str("}\nimpl Message { pub fn text(self) -> &'static str { message(self as usize) } }\n");
    output.push_str("pub fn message(index: usize) -> &'static str {\nmatch locale() {\n");
    output.push_str("Locale::English => ENGLISH[index],\n");
    for (variant, _) in &catalogs {
        output.push_str(&format!("Locale::{variant} => {}[index],\n", variant.to_uppercase()));
    }
    output.push_str("}\n}\n");
    for (name, values) in
        std::iter::once(("ENGLISH".to_owned(), english.iter().map(|(_, value)| value).collect::<Vec<_>>())).chain(
            catalogs.iter().map(|(variant, values)| {
                (variant.to_uppercase(), english.iter().map(|(key, _)| &values[key]).collect())
            }),
        )
    {
        output.push_str(&format!("const {name}: &[&str] = &[\n"));
        for value in values {
            output.push_str(&format!("{value:?},\n"));
        }
        output.push_str("];\n");
    }
    output.push_str("#[macro_export]\nmacro_rules! tr {\n");
    for (index, (key, value)) in english.iter().enumerate() {
        output.push_str(&format!("(@ {key}) => {{ $crate::Message::{} }};\n", variant(key)));
        output.push_str(&format!("({key}) => {{ $crate::message({index}) }};\n"));
        if value.contains('{') {
            output.push_str(&format!("({key}, $($args:tt)+) => {{ match $crate::locale() {{\n"));
            for (variant, values) in &catalogs {
                output.push_str(&format!("$crate::Locale::{variant} => std::format!({:?}, $($args)+),\n", values[key]));
            }
            output.push_str(&format!("$crate::Locale::English => std::format!({value:?}, $($args)+),\n}} }};\n"));
        }
    }
    output.push_str("}\n");
    let destination = PathBuf::from(env::var_os("OUT_DIR").expect("build output directory")).join("messages.rs");
    fs::write(destination, output).expect("write message catalog");
}
