//! Reading the path at the cursor and typing the rest of a subfolder's name.

use super::*;

const HOME_DIR: &str = "/home/me";
const BASE: &str = "/home/me/repo";

fn at_end(text: &str) -> TypedLine {
    TypedLine { text: text.into(), cursor: text.len() }
}

fn word(text: &str) -> Word {
    word_at_cursor(&at_end(text)).unwrap()
}

fn target_of(text: &str, held: &[(&str, &str)]) -> Option<Target> {
    let holds = |dir: &Path, name: &str| held.iter().any(|(d, n)| Path::new(d) == dir && *n == name);
    target(text, Path::new(BASE), Some(Path::new(HOME_DIR)), holds)
}

fn erased(count: usize, typed: &str) -> Vec<u8> {
    let mut keys = vec![ERASE; count];
    keys.extend(typed.bytes());
    keys
}

#[test]
fn the_word_at_the_cursor_is_read_as_the_shell_reads_it() {
    assert_eq!(word("ls /Volumes/Mac").text, "/Volumes/Mac");
    assert_eq!(word("ls /Volumes/Macintosh\\ H").text, "/Volumes/Macintosh H");
    assert_eq!(word("cp a \"/Volumes/Macintosh H").text, "/Volumes/Macintosh H");
    assert_eq!(word("cp a '/tmp/a\\b").text, "/tmp/a\\b", "a backslash in single quotes stays");
    assert_eq!(word("make;ls /tm").text, "/tm");
    assert_eq!(word("ls ").text, "");
    let inside = TypedLine { text: "ls /tmp".into(), cursor: "ls /t".len() };
    assert!(word_at_cursor(&inside).is_none(), "the cursor is inside the word");
    let before_blank = TypedLine { text: "ls /tm -l".into(), cursor: "ls /tm".len() };
    assert_eq!(word_at_cursor(&before_blank).map(|word| word.text), Some("/tm".into()));
}

#[test]
fn a_word_is_a_path_when_it_holds_a_slash_or_is_home() {
    assert!(word("git add src/").is_path());
    assert!(word("ls ~").is_path());
    assert!(!word("git commit").is_path());
}

#[test]
fn the_target_is_the_folder_before_the_last_slash() {
    let at = |dir: &str, name: &str| Some(Target { dir: dir.into(), name: name.into(), inside: false });
    let inside = |dir: &str| Some(Target { dir: dir.into(), name: String::new(), inside: true });
    assert_eq!(target_of("/Volumes/Ma", &[]), at("/Volumes", "Ma"));
    assert_eq!(target_of("/Volumes/", &[]), at("/Volumes", ""));
    assert_eq!(target_of("/Volumes", &[]), at("/", "Volumes"));
    assert_eq!(target_of("/Volumes", &[("/", "Volumes")]), inside("/Volumes"), "a folder typed whole");
    assert_eq!(target_of("~/Pro", &[]), at(HOME_DIR, "Pro"));
    assert_eq!(target_of("~", &[]), inside(HOME_DIR));
    assert_eq!(target_of("src/../docs/", &[]), at("/home/me/repo/docs", ""));
    assert_eq!(target_of("cra", &[]), at(BASE, "cra"));
    assert_eq!(target_of("..", &[]), inside(HOME_DIR));
    assert_eq!(target_of("~bob/x", &[]), None, "another user's home");
}

#[test]
fn names_that_start_with_what_is_typed_come_first() {
    let names: Vec<String> = ["Macintosh HD", "Data", "backup", "Macro", ".Trashes", "my-mac"].map(String::from).into();
    assert_eq!(matching(&names, "mac", false), ["Macintosh HD", "Macro", "my-mac"]);
    assert_eq!(matching(&names, "", false), ["backup", "Data", "Macintosh HD", "Macro", "my-mac"]);
    assert_eq!(matching(&names, ".", false), [".Trashes"], "hidden folders show once a dot is typed");
    assert_eq!(matching(&names, "", true)[0], ".Trashes");
}

#[test]
fn picking_types_the_rest_of_the_name_quoted_like_the_word() {
    let keys_for = |line: &str, held: &[(&str, &str)], name: &str| {
        let word = word(line);
        keys(&word, &target_of(&word.text, held).unwrap(), name)
    };
    assert_eq!(keys_for("ls /Volumes/Mac", &[], "Macintosh HD"), erased(3, "Macintosh\\ HD/"));
    assert_eq!(keys_for("ls \"/Volumes/Mac", &[], "Macintosh HD"), erased(3, "Macintosh HD/"));
    assert_eq!(keys_for("ls /Volumes/\"Mac", &[], "Macintosh HD"), erased(4, "Macintosh\\ HD/"));
    assert_eq!(keys_for("ls /Volumes/Macintosh\\ H", &[], "Macintosh HD"), erased(12, "Macintosh\\ HD/"));
    assert_eq!(keys_for("ls /Volumes", &[("/", "Volumes")], "Data"), erased(0, "/Data/"));
    assert_eq!(keys_for("ls '/tmp/", &[], "it's"), erased(0, "it'\\''s/"));
    assert_eq!(keys_for("ls /tmp/", &[], "a$b (1)"), erased(0, "a\\$b\\ \\(1\\)/"));
}
