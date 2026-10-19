use std::{fs, path::Path, process::Command};

use anyhow::{Result, ensure};
use borg_git::{Output, Tool, inspect, run};

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn repository() -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    git(folder.path(), &["init", "-b", "main"]);
    git(folder.path(), &["config", "user.name", "Borg Test"]);
    git(folder.path(), &["config", "user.email", "borg@example.test"]);
    folder
}

fn text(output: Output) -> String {
    let Output::Document { text, .. } = output else { panic!("expected a document") };
    String::from_utf8(text).unwrap()
}

#[tokio::test]
async fn status_handles_staging_renames_directories_and_unusual_names() -> Result<()> {
    let folder = repository();
    let root = folder.path();
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/main.rs"), "fn main() {}\n")?;
    fs::write(root.join("old name.txt"), "original\n")?;
    fs::write(root.join(".gitignore"), "*.ignored\n")?;
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "initial"]);
    fs::write(root.join("src/main.rs"), "fn main() { println!(\"hello\"); }\n")?;
    git(root, &["mv", "old name.txt", "new\nname.txt"]);
    fs::write(root.join("new.ignored"), "ignored")?;
    fs::write(root.join(":(glob)*.txt"), "literal")?;
    let status = inspect(root).await?.unwrap();
    assert_eq!(status.branch, "main");
    assert_eq!(status.status(&root.join("src/main.rs")).unwrap().label(), " M");
    assert_eq!(status.status(&root.join("src")).unwrap().label(), " M");
    assert_eq!(status.status(&root.join("new\nname.txt")).unwrap().label(), "R ");
    assert_eq!(status.status(&root.join(":(glob)*.txt")).unwrap().label(), "??");
    assert!(status.status(&root.join("new.ignored")).is_none());
    run(root, Tool::Stage, &[root.join(":(glob)*.txt")]).await?;
    let status = inspect(root).await?.unwrap();
    assert_eq!(status.status(&root.join(":(glob)*.txt")).unwrap().label(), "A ");
    let nested = inspect(&root.join("src")).await?.unwrap();
    assert_eq!(nested.root, status.root);
    assert_eq!(nested.status(&root.join("src/main.rs")).unwrap().label(), " M");
    Ok(())
}

#[tokio::test]
async fn stage_and_unstage_preserve_working_files_with_and_without_commits() -> Result<()> {
    let folder = repository();
    let root = folder.path();
    let path = root.join("-notes.txt");
    fs::write(&path, "original\n")?;
    assert!(text(run(root, Tool::Log, &[]).await?).contains("No commits yet"));
    run(root, Tool::Stage, std::slice::from_ref(&path)).await?;
    assert_eq!(inspect(root).await?.unwrap().status(&path).unwrap().label(), "A ");
    run(root, Tool::Unstage, std::slice::from_ref(&path)).await?;
    assert_eq!(fs::read_to_string(&path)?, "original\n");
    assert_eq!(inspect(root).await?.unwrap().status(&path).unwrap().label(), "??");
    run(root, Tool::Stage, std::slice::from_ref(&path)).await?;
    git(root, &["commit", "-qm", "initial"]);
    fs::write(&path, "changed\n")?;
    ensure!(text(run(root, Tool::Diff, std::slice::from_ref(&path)).await?).contains("+changed"));
    run(root, Tool::Stage, std::slice::from_ref(&path)).await?;
    assert!(text(run(root, Tool::StagedDiff, std::slice::from_ref(&path)).await?).contains("+changed"));
    fs::write(&path, "changed again\n")?;
    assert_eq!(inspect(root).await?.unwrap().status(&path).unwrap().label(), "MM");
    run(root, Tool::Unstage, std::slice::from_ref(&path)).await?;
    assert_eq!(fs::read_to_string(&path)?, "changed again\n");
    assert_eq!(inspect(root).await?.unwrap().status(&path).unwrap().label(), " M");
    assert!(text(run(root, Tool::Log, &[]).await?).contains("initial"));
    Ok(())
}

#[tokio::test]
async fn plain_folders_and_git_worktrees_are_supported() -> Result<()> {
    let folder = tempfile::tempdir()?;
    assert!(inspect(folder.path()).await?.is_none());
    let repository = repository();
    fs::write(repository.path().join("hello"), "hello")?;
    git(repository.path(), &["add", "."]);
    git(repository.path(), &["commit", "-qm", "initial"]);
    let path = folder.path().join("linked");
    git(repository.path(), &["worktree", "add", "-b", "linked", path.to_str().unwrap()]);
    fs::write(path.join("hello"), "modified")?;
    let status = inspect(&path).await?.unwrap();
    assert_eq!(status.branch, "linked");
    assert_eq!(status.status(&path.join("hello")).unwrap().label(), " M");
    Ok(())
}

#[test]
fn nul_records_preserve_non_utf8_paths_and_conflicts() -> Result<()> {
    let repository = borg_git::Repository::parse(
        "/work".into(),
        b"## main\0 M src/a.rs\0?? src/b.rs\0UU broken\0?? non-utf8-\xff\0",
    )?;
    assert_eq!(repository.status(Path::new("/work/src")).unwrap().label(), " M");
    assert!(repository.status(Path::new("/work/broken")).unwrap().conflict());
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    assert_eq!(repository.status(&Path::new("/work").join(OsStr::from_bytes(b"non-utf8-\xff"))).unwrap().label(), "??");
    Ok(())
}

#[test]
fn directory_status_does_not_turn_independent_deletions_into_a_conflict() {
    let repository = borg_git::Repository::parse("/work".into(), b"## main\0D  src/one.rs\0 D src/two.rs\0").unwrap();
    let status = repository.status(Path::new("/work/src")).unwrap();
    assert_eq!(status.label(), "MM");
    assert!(!status.conflict());
}

#[tokio::test]
async fn untracked_subfolders_and_symlinked_paths_preserve_ignored_files() -> Result<()> {
    let folder = repository();
    let root = folder.path();
    fs::write(root.join(".gitignore"), "*.ignored\n")?;
    fs::create_dir(root.join("drafts"))?;
    fs::write(root.join("drafts/new.txt"), "new")?;
    fs::write(root.join("drafts/cache.ignored"), "ignored")?;
    let status = inspect(&root.join("drafts")).await?.unwrap();
    assert_eq!(status.status(&root.join("drafts/new.txt")).unwrap().label(), "??");
    assert!(status.status(&root.join("drafts/cache.ignored")).is_none());
    let alias = tempfile::tempdir()?;
    let link = alias.path().join("linked");
    std::os::unix::fs::symlink(root.join("drafts"), &link)?;
    let status = inspect(&link).await?.unwrap();
    assert_eq!(status.status(&link.join("new.txt")).unwrap().label(), "??");
    run(&link, Tool::Stage, &[link.join("new.txt")]).await?;
    assert_eq!(inspect(&link).await?.unwrap().status(&link.join("new.txt")).unwrap().label(), "A ");
    Ok(())
}
