use super::*;

const MOUNTINFO_TABLE: &str = "\
22 1 8:1 / / rw,relatime shared:1 - ext4 /dev/sda1 rw
41 22 0:40 / /home/ana/.local/state/vaults/Tax\\040papers rw,nosuid,nodev,relatime shared:20 - fuse.gocryptfs /home/ana/Documents/Tax\\040papers rw,user_id=1000,group_id=1000
42 22 0:41 / /mnt/backup rw,relatime shared:21 - fuse.sshfs ana@nas:/backup rw
";

const MAC_TABLE: &str = "\
/dev/disk3s1s1 on / (apfs, sealed, local, read-only, journaled)
/Volumes/Work/ana/Documents/Tax papers on /tmp/borg-commander-501/vaults/Tax papers (macfuse, nodev, nosuid, synchronous, mounted by ana)
ana@nas:/backup on /Volumes/backup (macfuse, nodev, nosuid, synchronous, mounted by ana)
";

#[test]
fn linux_mount_table_lists_only_gocryptfs_mounts_with_their_spaces() {
    let mounts = mountinfo(MOUNTINFO_TABLE);
    assert_eq!(
        mounts,
        vec![Mounted {
            source: "/home/ana/Documents/Tax papers".into(),
            point: PathBuf::from("/home/ana/.local/state/vaults/Tax papers"),
        }]
    );
}

#[test]
fn mac_mount_table_finds_a_vault_by_its_folder_and_by_its_mount() {
    let point = PathBuf::from("/tmp/borg-commander-501/vaults/Tax papers");
    assert_eq!(mount_point_of(MAC_TABLE, "/Volumes/Work/ana/Documents/Tax papers"), Some(point.clone()));
    assert_eq!(source_of(MAC_TABLE, &point).as_deref(), Some("/Volumes/Work/ana/Documents/Tax papers"));
    assert_eq!(mount_point_of(MAC_TABLE, "/Volumes/Work/ana/Documents"), None);
    assert_eq!(source_of(MAC_TABLE, Path::new("/")), None);
}

#[test]
fn escaped_bytes_in_mount_paths_are_restored_and_stray_backslashes_kept() {
    assert_eq!(unescape("/a\\040b\\011c\\134d"), "/a b\tc\\d");
    assert_eq!(unescape("/a\\x"), "/a\\x");
    assert_eq!(unescape("/end\\04"), "/end\\04");
}

#[test]
fn gocryptfs_shows_commas_in_the_folder_as_underscores() {
    assert_eq!(fsname(Path::new("/home/ana/a,b")), "/home/ana/a_b");
}

#[test]
fn the_recovery_key_is_hex_in_groups_of_eight_that_gocryptfs_reads_back() {
    let bytes: Vec<u8> = (0..32).collect();
    let key = recovery_key(&bytes);
    let groups: Vec<&str> = key.split('-').collect();
    assert_eq!(groups.len(), 8);
    assert!(groups.iter().all(|group| group.len() == 8));
    assert_eq!(groups[0], "00010203");
    assert_eq!(key.replace('-', ""), borg_link::hex(&bytes));
}

#[test]
fn mount_names_follow_the_vault_and_count_up_when_taken() {
    let names: Vec<String> = candidates("Tax papers").take(3).collect();
    assert_eq!(names, ["Tax papers", "Tax papers 2", "Tax papers 3"]);
}

#[test]
fn the_last_line_of_an_error_is_what_borg_shows() {
    assert_eq!(last_line(b"Reading Password from stdin\nfuse: device not found\n\n"), "fuse: device not found");
    assert_eq!(last_line(b""), "");
}

#[test]
fn a_folder_with_a_gocryptfs_config_is_a_locked_vault_until_it_is_mounted() {
    let home = tempfile::tempdir().unwrap();
    let cipher = home.path().join("Papers");
    fs::create_dir(&cipher).unwrap();
    fs::write(cipher.join(VAULT_CONFIG), "{}").unwrap();
    let vaults = Vaults::new(home.path(), None);
    assert_eq!(vaults.check(&cipher, None), Some(Found::Locked));
    assert_eq!(vaults.check(home.path(), None), None);
}

#[test]
fn a_mount_folder_left_empty_counts_as_a_closed_vault_and_is_removed() {
    let runtime = tempfile::tempdir().unwrap();
    let vaults = Vaults::new(runtime.path(), None);
    let root = vaults.folder.join("Papers");
    fs::create_dir_all(root.join("inner")).unwrap();
    fs::remove_dir(root.join("inner")).unwrap();
    assert_eq!(vaults.check(&root, None), Some(Found::Gone(root.clone())));
    assert!(!root.exists());
}

#[test]
fn a_mount_folder_that_still_holds_files_is_never_removed() {
    let runtime = tempfile::tempdir().unwrap();
    let vaults = Vaults::new(runtime.path(), None);
    let root = vaults.folder.join("Papers");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("note.txt"), "kept").unwrap();
    assert_eq!(vaults.check(&root.join("sub"), None), Some(Found::Gone(root.clone())));
    assert!(root.join("note.txt").exists());
}

#[test]
fn a_new_mount_point_reuses_an_empty_folder_and_skips_one_with_files() {
    let runtime = tempfile::tempdir().unwrap();
    let vaults = Vaults::new(runtime.path(), None);
    let busy = vaults.folder.join("Papers");
    fs::create_dir_all(&busy).unwrap();
    fs::write(busy.join("left.txt"), "kept").unwrap();
    let point = vaults.mount_point(Path::new("/home/ana/Papers")).unwrap();
    assert_eq!(point, vaults.folder.join("Papers 2"));
    assert_eq!(vaults.mount_point(Path::new("/home/ana/Papers")).unwrap(), point);
}

#[test]
fn messages_say_what_the_listing_found() {
    let dir = PathBuf::from("/home/ana/Papers");
    let locked = Found::Locked.message(Side::Left, 3, dir.clone());
    assert!(matches!(locked, Msg::VaultFound { side: Side::Left, generation: 3, cipher, open: None } if cipher == dir));
    assert!(Found::Locked.replaces_listing());
    let vault = VaultMount { cipher: dir, root: PathBuf::from("/run/vaults/Papers") };
    assert!(!Found::Inside(vault).replaces_listing());
}

#[test]
fn closing_a_vault_mounted_elsewhere_keeps_its_folder() {
    let runtime = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let vaults = Vaults::new(runtime.path(), None);
    let own = vaults.mount_point(Path::new("/home/ana/Papers")).unwrap();
    let theirs = elsewhere.path().join("Private");
    fs::create_dir(&theirs).unwrap();
    assert_eq!(vaults.close(&theirs), Ok(()));
    assert!(theirs.is_dir(), "a folder outside Borg's vaults folder stays");
    assert_eq!(vaults.close(&own), Ok(()));
    assert!(!own.exists(), "Borg's own empty mount folder goes");
}
