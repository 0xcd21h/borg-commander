use std::sync::{Arc, atomic::AtomicBool};

use borg_core::{Aside, EditorSlot, Peek};
use borg_crypt::sealed::Format;
use borg_drives::Drives;
use borg_i18n::tr;

use super::*;

const SECRET: &str = "hunter two";
const NOTES: &[u8] = b"# Shopping\n\nOat milk and lemons.\n";
const FAST_LOG_N: u8 = 1;
const RANDOM: [u8; pte::RANDOM_LEN] = [0x42; pte::RANDOM_LEN];
const SIZE: (usize, usize) = (80, 20);
const CELL: (u16, u16) = (8, 16);

fn fast() -> age::WorkFactor {
    age::WorkFactor::new(FAST_LOG_N).expect("a valid work factor")
}

fn age_file(text: &[u8]) -> Vec<u8> {
    let fresh = age::Fresh {
        file_key: Zeroizing::new([1; age::FILE_KEY_LEN]),
        salt: [2; age::SALT_LEN],
        nonce: [3; age::NONCE_LEN],
    };
    age::encrypt_text(text, SECRET.as_bytes(), fast(), &fresh).expect("encrypt")
}

fn pte_file(text: &[u8]) -> Vec<u8> {
    pte::seal(text, &pte::MasterKey::from_password(SECRET), pte::Cipher::Aes256, &RANDOM).expect("seal").into_bytes()
}

fn written(folder: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = folder.path().join(name);
    fs::write(&path, bytes).expect("write");
    path
}

fn opened(path: &Path, then: Then, password: &str) -> Msg {
    let file = SealedFile { path: path.into(), format: detect(path).expect("an encrypted file"), then };
    open(file, &Password::new(password), SIZE)
}

#[test]
fn finds_both_formats_and_leaves_other_files_alone() {
    let folder = tempfile::tempdir().expect("folder");
    let armored_start = [b"\n\n".as_slice(), b"-----BEGIN AGE ENCRYPTED FILE-----\n"].concat();
    let cases = [
        ("notes.md.age", age_file(NOTES), Some(Format::Age)),
        ("notes.txt", pte_file(NOTES), Some(Format::Pte)),
        ("armored.age", armored_start, Some(Format::Age)),
        ("shout.txt", b"Hello there!\n".to_vec(), None),
        ("empty.txt", Vec::new(), None),
        ("huge.age", [b"age-encryption.org/v1\n".as_slice(), &vec![0; sealed::FILE_LIMIT]].concat(), None),
    ];
    for (name, bytes, format) in cases {
        assert_eq!(detect(&written(&folder, name, &bytes)), format, "{name}");
    }
    assert_eq!(detect(folder.path()), None, "a folder");
}

#[test]
fn the_viewer_shows_the_decrypted_text_under_the_real_path() {
    let folder = tempfile::tempdir().expect("folder");
    let path = written(&folder, "notes.md.age", &age_file(NOTES));
    let Msg::Opened(viewer) = opened(&path, Then::View, SECRET) else { panic!("the viewer opens") };
    assert_eq!(viewer.path(), Some(path.as_path()));
    let Content::Text(pager) = &viewer.content else { panic!("text") };
    assert!(pager.formatted(), "the name without .age picks Markdown");
    assert!(pager.rows().iter().any(|row| row.contains("Oat milk")), "{:?}", pager.rows());
}

#[test]
fn the_editor_gets_the_text_and_the_seal_and_saves_back_encrypted() {
    let folder = tempfile::tempdir().expect("folder");
    let path = written(&folder, "notes.txt", &pte_file(NOTES));
    let slot = EditorSlot::Screen;
    let Msg::SealedEditorOpened { file, seal: used, .. } = opened(&path, Then::Edit(slot), SECRET) else {
        panic!("the editor opens")
    };
    assert_eq!(file.text.as_bytes(), NOTES);
    let sealed = seal(&used, b"Bread too.\n", &path).expect("seal");
    let written = crate::edit_file::write(&path, &sealed, file.stamp, false).expect("write");
    assert_ne!(written, file.stamp);
    assert_eq!(detect(&path), Some(Format::Pte));
    let Msg::SealedEditorOpened { file, .. } = opened(&path, Then::Edit(slot), SECRET) else { panic!("reopens") };
    assert_eq!(file.text, "Bread too.\n");
}

#[test]
fn a_wrong_password_and_binary_text_are_reported() {
    let folder = tempfile::tempdir().expect("folder");
    let path = written(&folder, "notes.age", &age_file(NOTES));
    let Msg::UnsealFailed { error, .. } = opened(&path, Then::View, "wrong") else { panic!("refused") };
    assert_eq!(error, UnsealError::Sealed(sealed::Error::WrongPassword));
    let binary = written(&folder, "blob.age", &age_file(b"\0\x01\x02"));
    let Msg::UnsealFailed { error, .. } = opened(&binary, Then::Edit(EditorSlot::Screen), SECRET) else {
        panic!("refused")
    };
    assert_eq!(error, UnsealError::NotText);
}

#[test]
fn the_quick_view_names_the_format_and_never_asks() {
    let folder = tempfile::tempdir().expect("folder");
    let path = written(&folder, "notes.md.age", &age_file(NOTES));
    let peeked = crate::peek::peek(
        Aside::QuickView,
        &path,
        None,
        SIZE,
        CELL,
        Arc::new(AtomicBool::new(false)),
        &Drives::default(),
    );
    let Peek::Unavailable(text) = peeked else { panic!("a note in place of the text") };
    assert_eq!(text, tr!(quick_view_sealed, format = borg_core::format_name(Format::Age)));
}
