//! Linux: the mount from `/proc/self/mountinfo`, then the block device's flags in `/sys`.

use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use crate::{Drive, DriveKind};

const MOUNTINFO: &str = "/proc/self/mountinfo";
const SYS_BLOCK: &str = "/sys/dev/block";
const ROTATIONAL: &str = "queue/rotational";
const PARTITION_ROTATIONAL: &str = "../queue/rotational";
const LOOP_BACKING: &str = "loop/backing_file";
const SPINNING: &str = "1";
const FIELDS_END: &str = " - ";
const DEVICE_SEPARATOR: char = ':';
const ESCAPE: char = '\\';
const ESCAPE_DIGITS: usize = 3;
const OCTAL_RADIX: u32 = 8;
const ROOT: char = '/';
const OVERLAY: &str = "overlay";
const UPPER_DIR: &str = "upperdir=";
const OPTION_SEPARATOR: char = ',';
const LOOP_DEVICE: &str = "loop";
const MEMORY_DEVICES: [&str; 2] = ["zram", "ram"];
const MEMORY_FILESYSTEMS: [&str; 4] = ["tmpfs", "ramfs", "devtmpfs", "hugetlbfs"];
const NETWORK_FILESYSTEMS: [&str; 17] = [
    "nfs",
    "nfs4",
    "cifs",
    "smb3",
    "smbfs",
    "9p",
    "virtiofs",
    "ceph",
    "glusterfs",
    "afs",
    "lustre",
    "davfs",
    "fuse.sshfs",
    "fuse.rclone",
    "fuse.s3fs",
    "fuse.davfs2",
    "fuse.gcsfuse",
];

/// One line of mountinfo: a filesystem, its device and where it is mounted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Mount {
    pub device: (u32, u32),
    pub point: String,
    pub filesystem: String,
    pub source: String,
    /// The filesystem's own options, such as an overlay's `upperdir`.
    pub options: String,
}

pub(super) fn detect(path: &Path, depth: usize) -> Drive {
    let shown = path.display().to_string();
    let Some(device) = super::existing(path).and_then(|path| fs::metadata(path).ok()).map(|meta| meta.dev()) else {
        return Drive::unknown(shown);
    };
    let info = fs::read_to_string(MOUNTINFO).unwrap_or_default();
    match mount(&info, (rustix::fs::major(device), rustix::fs::minor(device))) {
        Some(mount) => of_mount(&mount, depth),
        None => Drive::unknown(shown),
    }
}

/// The mount whose filesystem has the device numbers `device`, from the text of mountinfo.
pub(super) fn mount(info: &str, device: (u32, u32)) -> Option<Mount> {
    info.lines().filter_map(parse).find(|mount| mount.device == device)
}

fn parse(line: &str) -> Option<Mount> {
    let (front, back) = line.split_once(FIELDS_END)?;
    let fields: Vec<&str> = front.split(' ').collect();
    let (major, minor) = fields.get(2)?.split_once(DEVICE_SEPARATOR)?;
    let mut back = back.split(' ');
    Some(Mount {
        device: (major.parse().ok()?, minor.parse().ok()?),
        point: unescape(fields.get(4)?),
        filesystem: back.next()?.to_owned(),
        source: unescape(back.next()?),
        options: back.next().unwrap_or_default().to_owned(),
    })
}

/// mountinfo writes a space and a few other characters as octal escapes, such as `\040`.
fn unescape(field: &str) -> String {
    let mut text = String::with_capacity(field.len());
    let mut rest = field;
    while let Some(at) = rest.find(ESCAPE) {
        text.push_str(&rest[..at]);
        let digits = rest.get(at + 1..at + 1 + ESCAPE_DIGITS);
        match digits.and_then(|digits| u8::from_str_radix(digits, OCTAL_RADIX).ok()) {
            Some(byte) => {
                text.push(char::from(byte));
                rest = &rest[at + 1 + ESCAPE_DIGITS..];
            }
            None => {
                text.push(ESCAPE);
                rest = &rest[at + 1..];
            }
        }
    }
    text.push_str(rest);
    text
}

/// What kind of drive a filesystem is by its name alone, for those that have no device.
pub(super) fn by_filesystem(filesystem: &str) -> Option<DriveKind> {
    if MEMORY_FILESYSTEMS.contains(&filesystem) {
        Some(DriveKind::Memory)
    } else if NETWORK_FILESYSTEMS.contains(&filesystem) {
        Some(DriveKind::Network)
    } else {
        None
    }
}

/// What kind of drive a block device is, by its name and whether it spins.
pub(super) fn by_device(name: &str, spinning: Option<bool>) -> DriveKind {
    let numbered = |prefix: &&str| {
        name.strip_prefix(*prefix).is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
    };
    if MEMORY_DEVICES.iter().any(numbered) {
        return DriveKind::Memory;
    }
    match spinning {
        Some(true) => DriveKind::HardDrive,
        Some(false) => DriveKind::SolidState,
        None => DriveKind::Unknown,
    }
}

fn of_mount(mount: &Mount, depth: usize) -> Drive {
    let kind = match by_filesystem(&mount.filesystem) {
        Some(kind) => kind,
        None if mount.filesystem == OVERLAY => upper(mount)
            .filter(|_| depth > 0)
            .map_or(DriveKind::Unknown, |upper| detect(Path::new(&upper), depth - 1).kind),
        None if mount.source.starts_with(ROOT) => block(&mount.source, depth),
        None => DriveKind::Unknown,
    };
    Drive { id: identity(mount), kind, filesystem: mount.filesystem.clone(), clones: super::clones(&mount.filesystem) }
}

/// A device path or a share names the drive; a filesystem without one, such as `tmpfs`, goes by
/// where it is mounted.
fn identity(mount: &Mount) -> String {
    if mount.source.starts_with(ROOT) || mount.source.contains(DEVICE_SEPARATOR) {
        mount.source.clone()
    } else {
        format!("{}{DEVICE_SEPARATOR}{}", mount.filesystem, mount.point)
    }
}

fn upper(mount: &Mount) -> Option<String> {
    mount.options.split(OPTION_SEPARATOR).find_map(|option| option.strip_prefix(UPPER_DIR)).map(unescape)
}

fn block(source: &str, depth: usize) -> DriveKind {
    let Ok(meta) = fs::metadata(source) else { return DriveKind::Unknown };
    let device = meta.rdev();
    let sys = PathBuf::from(SYS_BLOCK).join(format!(
        "{}{DEVICE_SEPARATOR}{}",
        rustix::fs::major(device),
        rustix::fs::minor(device)
    ));
    let name = fs::read_link(&sys)
        .ok()
        .and_then(|link| link.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_default();
    if name.starts_with(LOOP_DEVICE)
        && depth > 0
        && let Ok(backing) = fs::read_to_string(sys.join(LOOP_BACKING))
    {
        return detect(Path::new(backing.trim()), depth - 1).kind;
    }
    let spinning = [ROTATIONAL, PARTITION_ROTATIONAL]
        .iter()
        .find_map(|file| fs::read_to_string(sys.join(file)).ok())
        .map(|flag| flag.trim() == SPINNING);
    by_device(&name, spinning)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = "\
22 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw
36 22 0:32 / /tmp rw,nosuid shared:15 - tmpfs tmpfs rw,size=8G
41 22 0:45 / /mnt/my\\040share rw,relatime - nfs4 nas:/export/media rw,vers=4.2
52 22 0:52 / /var/lib/docker/overlay2/x/merged rw - overlay overlay rw,lowerdir=/a,upperdir=/var/lib/docker/u,workdir=/w";

    #[test]
    fn mountinfo_names_each_filesystem_with_its_device_and_place() {
        let root = mount(INFO, (259, 2)).expect("the root filesystem");
        assert_eq!(
            (root.filesystem.as_str(), root.source.as_str(), root.point.as_str()),
            ("ext4", "/dev/nvme0n1p2", "/")
        );
        let share = mount(INFO, (0, 45)).expect("the share");
        assert_eq!(share.point, "/mnt/my share", "escapes become the characters they stand for");
        assert_eq!(identity(&share), "nas:/export/media");
        let tmp = mount(INFO, (0, 32)).expect("tmpfs");
        assert_eq!(identity(&tmp), "tmpfs:/tmp");
        assert_eq!(upper(&mount(INFO, (0, 52)).expect("overlay")).as_deref(), Some("/var/lib/docker/u"));
        assert_eq!(mount(INFO, (8, 1)), None);
    }

    #[test]
    fn filesystems_and_devices_tell_the_kind_of_drive() {
        assert_eq!(by_filesystem("tmpfs"), Some(DriveKind::Memory));
        assert_eq!(by_filesystem("fuse.sshfs"), Some(DriveKind::Network));
        assert_eq!(by_filesystem("ext4"), None);
        assert_eq!(by_device("zram0", Some(false)), DriveKind::Memory);
        assert_eq!(by_device("ram12", None), DriveKind::Memory);
        assert_eq!(by_device("ramdisk", Some(true)), DriveKind::HardDrive);
        assert_eq!(by_device("sda1", Some(true)), DriveKind::HardDrive);
        assert_eq!(by_device("nvme0n1p2", Some(false)), DriveKind::SolidState);
        assert_eq!(by_device("vda", None), DriveKind::Unknown);
    }
}
