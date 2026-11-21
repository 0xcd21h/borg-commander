//! macOS: the filesystem from `statfs`, then what `diskutil` says of its disk, and `hdiutil` for
//! disk images, which may be RAM disks.

use std::{ffi::c_char, path::Path, process::Command};

use crate::{Drive, DriveKind};

const DISKUTIL: &str = "/usr/sbin/diskutil";
const HDIUTIL: &str = "/usr/bin/hdiutil";
const DEVICE_PREFIX: &str = "/dev/";
const NETWORK_FILESYSTEMS: [&str; 8] = ["nfs", "smbfs", "afpfs", "webdav", "cifs", "ftp", "macfuse", "osxfuse"];
const SOLID_STATE: &str = "SolidState";
const BUS_PROTOCOL: &str = "BusProtocol";
const DISK_IMAGE: &str = "Disk Image";
const WHOLE_DISK: &str = "ParentWholeDisk";
const IMAGE_KEY: &str = "<key>image-path</key>";
const RAM_SCHEME: &str = "ram://";
const TRUE: &str = "<true/>";
const FALSE: &str = "<false/>";
const STRING_OPEN: &str = "<string>";
const STRING_CLOSE: &str = "</string>";

/// A value in a property list that `diskutil` and `hdiutil` print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Value<'a> {
    Flag(bool),
    Text(&'a str),
}

pub(super) fn detect(path: &Path, depth: usize) -> Drive {
    let Some(stats) = super::existing(path).and_then(|path| rustix::fs::statfs(path).ok()) else {
        return Drive::unknown(path.display().to_string());
    };
    let filesystem = text(&stats.f_fstypename);
    let source = text(&stats.f_mntfromname);
    let kind = if NETWORK_FILESYSTEMS.contains(&filesystem.as_str()) {
        DriveKind::Network
    } else if source.starts_with(DEVICE_PREFIX) {
        disk(&source, depth)
    } else {
        DriveKind::Unknown
    };
    Drive { kind, clones: super::clones(&filesystem), id: source, filesystem }
}

fn text(chars: &[c_char]) -> String {
    let bytes: Vec<u8> = chars.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn disk(device: &str, depth: usize) -> DriveKind {
    let Some(info) = output(DISKUTIL, &["info", "-plist", device]) else { return DriveKind::Unknown };
    if value(&info, BUS_PROTOCOL) == Some(Value::Text(DISK_IMAGE)) {
        let Some(Value::Text(whole)) = value(&info, WHOLE_DISK) else { return DriveKind::Unknown };
        let images = output(HDIUTIL, &["info", "-plist"]).unwrap_or_default();
        return match image_of(&images, whole) {
            Some(image) if image.starts_with(RAM_SCHEME) => DriveKind::Memory,
            Some(image) if depth > 0 => detect(Path::new(image), depth - 1).kind,
            _ => DriveKind::Unknown,
        };
    }
    match value(&info, SOLID_STATE) {
        Some(Value::Flag(true)) => DriveKind::SolidState,
        Some(Value::Flag(false)) => DriveKind::HardDrive,
        _ => DriveKind::Unknown,
    }
}

/// The value that follows `key` in a property list.
fn value<'a>(plist: &'a str, key: &str) -> Option<Value<'a>> {
    let marker = format!("<key>{key}</key>");
    let rest = plist[plist.find(&marker)? + marker.len()..].trim_start();
    if rest.starts_with(TRUE) {
        Some(Value::Flag(true))
    } else if rest.starts_with(FALSE) {
        Some(Value::Flag(false))
    } else {
        rest.starts_with(STRING_OPEN).then(|| first_string(rest)).flatten().map(Value::Text)
    }
}

fn first_string(text: &str) -> Option<&str> {
    let start = text.find(STRING_OPEN)? + STRING_OPEN.len();
    let length = text[start..].find(STRING_CLOSE)?;
    Some(&text[start..start + length])
}

/// The image attached as the whole disk `whole`, such as `disk4`, in the list `hdiutil` prints.
fn image_of<'a>(images: &'a str, whole: &str) -> Option<&'a str> {
    let entry = format!("{STRING_OPEN}{DEVICE_PREFIX}{whole}{STRING_CLOSE}");
    images.split(IMAGE_KEY).skip(1).find(|image| image.contains(&entry)).and_then(first_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISK: &str = "<dict>\n\t<key>BusProtocol</key>\n\t<string>Apple Fabric</string>\n\t<key>Internal</key>\n\t\
                        <true/>\n\t<key>SolidState</key>\n\t<false/>\n</dict>";
    const IMAGES: &str = "<key>images</key><array><dict><key>image-path</key><string>ram://2048</string>\
                          <key>system-entities</key><array><dict><key>dev-entry</key><string>/dev/disk4</string>\
                          </dict></array></dict><dict><key>image-path</key><string>/tmp/images/b.dmg</string>\
                          <key>system-entities</key><array><dict><key>dev-entry</key><string>/dev/disk5s1</string>\
                          </dict><dict><key>dev-entry</key><string>/dev/disk5</string></dict></array></dict></array>";

    #[test]
    fn property_lists_give_flags_and_text_by_key() {
        assert_eq!(value(DISK, BUS_PROTOCOL), Some(Value::Text("Apple Fabric")));
        assert_eq!(value(DISK, "Internal"), Some(Value::Flag(true)));
        assert_eq!(value(DISK, SOLID_STATE), Some(Value::Flag(false)));
        assert_eq!(value(DISK, "Missing"), None);
    }

    #[test]
    fn a_disk_image_is_found_by_its_whole_disk() {
        assert_eq!(image_of(IMAGES, "disk4"), Some("ram://2048"));
        assert_eq!(image_of(IMAGES, "disk5"), Some("/tmp/images/b.dmg"));
        assert_eq!(image_of(IMAGES, "disk6"), None);
    }
}
