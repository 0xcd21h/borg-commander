use super::*;
use borg_core::{ProcessFile, ProcessFiles};
#[cfg(target_os = "macos")]
use {super::helper::bounded_output, tokio::process::Command};

pub(super) async fn list(process: ProcessKey) -> Result<ProcessFiles> {
    verify(process)?;
    #[cfg(target_os = "macos")]
    let result = {
        let mut command = Command::new("/usr/sbin/lsof");
        command.args(["-n", "-P", "-a", "-p", &process.pid.to_string(), "-F0fatnso"]);
        let bytes = bounded_output(command, TIMEOUT).await?;
        parse_lsof(&bytes)?
    };
    #[cfg(target_os = "linux")]
    let result = tokio::task::spawn_blocking(move || proc_files(process.pid)).await??;
    verify(process)?;
    Ok(result)
}

#[cfg(any(target_os = "macos", test))]
fn parse_lsof(bytes: &[u8]) -> Result<ProcessFiles> {
    let mut entries = Vec::new();
    let mut current: Option<ProcessFile> = None;
    for field in bytes.split(|&b| b == 0) {
        let field = field.strip_prefix(b"\n").unwrap_or(field);
        let Some((&key, value)) = field.split_first() else {
            continue;
        };
        let value = String::from_utf8_lossy(value).into_owned();
        if key == b'f' {
            if let Some(file) = current.take() {
                entries.push(file);
            }
            if entries.len() >= REGION_LIMIT {
                bail!(tr!(activity_output_limit));
            }
            current = Some(ProcessFile {
                descriptor: value,
                access: String::new(),
                kind: String::new(),
                name: String::new(),
                size: None,
                offset: None,
            });
        } else if let Some(file) = current.as_mut() {
            match key {
                b'a' => file.access = if value == "u" { "rw".into() } else { value },
                b't' => file.kind = value,
                b'n' => file.name = value,
                b's' => file.size = value.parse().ok(),
                b'o' => file.offset = value.strip_prefix("0t").unwrap_or(&value).parse().ok(),
                _ => {}
            }
        }
    }
    if let Some(file) = current {
        entries.push(file);
    }
    entries.sort_by(|a, b| {
        a.descriptor
            .parse::<u64>()
            .unwrap_or(u64::MAX)
            .cmp(&b.descriptor.parse::<u64>().unwrap_or(u64::MAX))
            .then(a.descriptor.cmp(&b.descriptor))
    });
    Ok(ProcessFiles { entries, skipped: 0 })
}

#[cfg(target_os = "linux")]
fn proc_files(pid: u32) -> Result<ProcessFiles> {
    use std::os::unix::fs::FileTypeExt;
    let mut entries = Vec::new();
    let mut skipped = 0;
    let mut bytes = 0usize;
    for entry in fs::read_dir(format!("/proc/{pid}/fd"))? {
        if entries.len() + skipped >= REGION_LIMIT {
            bail!(tr!(activity_output_limit));
        }
        let Ok(entry) = entry else {
            skipped += 1;
            continue;
        };
        let Ok(target) = fs::read_link(entry.path()) else {
            skipped += 1;
            continue;
        };
        let descriptor = entry.file_name().to_string_lossy().into_owned();
        let name = target.to_string_lossy().into_owned();
        bytes = bytes.saturating_add(name.len());
        if bytes > OUTPUT_LIMIT {
            bail!(tr!(activity_output_limit));
        }
        let metadata = fs::metadata(entry.path()).ok();
        let kind = metadata.as_ref().map_or("?", |m| {
            let t = m.file_type();
            if t.is_file() {
                "REG"
            } else if t.is_dir() {
                "DIR"
            } else if t.is_socket() {
                "SOCK"
            } else if t.is_fifo() {
                "PIPE"
            } else if t.is_char_device() {
                "CHR"
            } else {
                "OTHER"
            }
        });
        let info = fs::read_to_string(format!("/proc/{pid}/fdinfo/{descriptor}")).unwrap_or_default();
        let field = |key: &str| info.lines().find_map(|line| line.strip_prefix(key).map(str::trim));
        let access = field("flags:").and_then(|v| u64::from_str_radix(v, 8).ok()).map_or("?", |v| match v & 3 {
            0 => "r",
            1 => "w",
            _ => "rw",
        });
        let offset = field("pos:").and_then(|v| v.parse().ok());
        entries.push(ProcessFile {
            descriptor,
            access: access.into(),
            kind: kind.into(),
            name,
            size: metadata.filter(|m| m.is_file()).map(|m| m.len()),
            offset,
        });
    }
    entries.sort_by_key(|f| f.descriptor.parse::<u64>().unwrap_or(u64::MAX));
    Ok(ProcessFiles { entries, skipped })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lsof_fields_preserve_names_and_separate_descriptors_from_mapped_images() {
        let result = parse_lsof(b"p123\0cworker\0\nftext\0tREG\0s4096\0n/usr/lib/example.dylib\0\nf7\0au\0tREG\0s100\0o0t42\0n/tmp/a\nfile with spaces\0\nf2\0aw\0tPIPE\0npipe\0\n").unwrap();
        assert_eq!(result.entries.len(), 3);
        assert_eq!(result.entries[0].descriptor, "2");
        assert_eq!(result.entries[1].name, "/tmp/a\nfile with spaces");
        assert_eq!(result.entries[1].access, "rw");
        assert_eq!(result.entries[1].offset, Some(42));
        assert_eq!(result.entries[2].descriptor, "text");
    }
    #[tokio::test]
    async fn lists_an_open_file_for_the_current_process_and_rejects_reused_identity() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("process-open-file-test");
        let file = fs::File::create(&path).unwrap();
        let mut system = System::new();
        let pid = Pid::from_u32(std::process::id());
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing());
        let process = ProcessKey { pid: pid.as_u32(), start: system.process(pid).unwrap().start_time() };
        let report = list(process).await.unwrap();
        assert!(report.entries.iter().any(|f| f.name.ends_with("process-open-file-test") && f.kind == "REG"));
        assert!(list(ProcessKey { start: process.start + 1, ..process }).await.is_err());
        drop(file);
    }
}
