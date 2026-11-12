use std::process::Command as Program;

use super::*;

#[test]
fn every_filter_a_repository_names_is_turned_off_once() {
    let names = b"filter.lfs.clean\0filter.lfs.required\0filter.a.b.process\0core.bare\0";
    assert_eq!(
        without_filters(names).unwrap(),
        [
            "-c",
            "filter.a.b.clean=",
            "-c",
            "filter.a.b.smudge=",
            "-c",
            "filter.a.b.process=",
            "-c",
            "filter.a.b.required=false",
            "-c",
            "filter.lfs.clean=",
            "-c",
            "filter.lfs.smudge=",
            "-c",
            "filter.lfs.process=",
            "-c",
            "filter.lfs.required=false",
        ]
    );
    assert!(without_filters(b"").unwrap().is_empty());
    assert!(without_filters(b"filter.\xff.clean\0").is_err());
}

fn git(dir: &Path, args: &[&str]) {
    let done = Program::new("git").arg("-C").arg(dir).args(args).output().expect("git runs");
    assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
}

/// A repository somebody else made names a program as a filter for every file. Looking at its
/// folder, and at what changed in it, shows the change without running that program.
#[tokio::test]
async fn looking_at_a_repository_runs_nothing_its_own_configuration_names() {
    let folder = tempfile::tempdir().unwrap();
    let dir = folder.path().join("repository");
    let ran = folder.path().join("ran");
    std::fs::create_dir(&dir).unwrap();
    git(&dir, &["init", "--quiet"]);
    git(&dir, &["config", "user.email", "borg@example.com"]);
    git(&dir, &["config", "user.name", "Borg"]);
    std::fs::write(dir.join(".gitattributes"), "* filter=theirs\n").unwrap();
    std::fs::write(dir.join("notes.txt"), "first\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "--quiet", "--message", "first"]);
    git(&dir, &["config", "filter.theirs.clean", &format!("sh -c 'touch \"{}\"; cat'", ran.display())]);
    git(&dir, &["config", "filter.theirs.required", "true"]);
    std::fs::write(dir.join("notes.txt"), "second, and longer\n").unwrap();

    let repository = inspect(&dir).await.unwrap().expect("a repository");
    assert!(!repository.changes.is_empty(), "the change shows");
    let notes = [PathBuf::from("notes.txt")];
    for tool in [Tool::Status, Tool::Diff, Tool::StagedDiff, Tool::Log] {
        run(&dir, tool, &notes).await.unwrap();
    }
    assert!(!ran.exists(), "the repository's filter did not run");

    run(&dir, Tool::Stage, &notes).await.unwrap();
    assert!(ran.exists(), "staging, which the user asks for, still filters as Git does");
}
