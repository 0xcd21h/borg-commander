//! A process's memory map: its regions from vmmap on macOS and from smaps on Linux.

use super::{KIB, REGION_LIMIT, verify};
use anyhow::{Result, bail};
use borg_core::{MemoryRegion, ProcessKey};
use borg_i18n::tr;
#[cfg(target_os = "linux")]
use {
    super::OUTPUT_LIMIT,
    std::{fs, io::Read},
};
#[cfg(target_os = "macos")]
use {
    super::{TIMEOUT, helper::bounded_output},
    tokio::process::Command,
};

pub(super) async fn regions(process: ProcessKey) -> Result<Vec<MemoryRegion>> {
    verify(process)?;
    #[cfg(target_os = "macos")]
    let result = {
        let mut command = Command::new("/usr/bin/vmmap");
        command.args(["-w", "-interleaved", "-noCoalesce", &process.pid.to_string()]);
        let bytes = bounded_output(command, TIMEOUT).await?;
        parse_vmmap(&String::from_utf8_lossy(&bytes))
    };
    #[cfg(target_os = "linux")]
    let result = tokio::task::spawn_blocking(move || {
        let mut bytes = Vec::new();
        fs::File::open(format!("/proc/{}/smaps", process.pid))?
            .take(OUTPUT_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > OUTPUT_LIMIT {
            bail!(tr!(activity_output_limit));
        }
        Ok::<_, anyhow::Error>(parse_smaps(&String::from_utf8_lossy(&bytes)))
    })
    .await??;
    verify(process)?;
    if result.is_empty() {
        bail!(tr!(activity_no_regions));
    }
    if result.len() > REGION_LIMIT {
        bail!(tr!(activity_output_limit));
    }
    Ok(result)
}

/// The process's mappings from the cheap maps file on Linux; macOS lists them only through
/// vmmap, which takes seconds on large processes, so there a read outside them simply fails.
pub(super) async fn quick_maps(process: ProcessKey) -> Result<Option<Vec<MemoryRegion>>> {
    #[cfg(target_os = "linux")]
    return tokio::task::spawn_blocking(move || {
        let mut bytes = Vec::new();
        fs::File::open(format!("/proc/{}/maps", process.pid))?.take(OUTPUT_LIMIT as u64).read_to_end(&mut bytes)?;
        Ok(Some(parse_smaps(&String::from_utf8_lossy(&bytes))))
    })
    .await?;
    #[cfg(target_os = "macos")]
    {
        let _ = process;
        Ok(None)
    }
}

pub(super) fn range(text: &str) -> Option<(u64, u64)> {
    let (a, b) = text.split_once('-')?;
    let start = u64::from_str_radix(a, 16).ok()?;
    let end = u64::from_str_radix(b, 16).ok()?;
    (end > start).then_some((start, end))
}

pub(super) fn amount(text: &str) -> Option<u64> {
    let split = text.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(text.len());
    let n: f64 = text[..split].parse().ok()?;
    let factor = match text[split..].trim() {
        "" | "B" => 1.0,
        "K" | "kB" => KIB,
        "M" => KIB.powi(2),
        "G" => KIB.powi(3),
        "T" => KIB.powi(4),
        _ => return None,
    };
    (n.is_finite() && n >= 0.0).then_some((n * factor) as u64)
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn parse_vmmap(text: &str) -> Vec<MemoryRegion> {
    let mut regions = Vec::new();
    for line in text.lines() {
        let words: Vec<_> = line.split_whitespace().collect();
        let Some((i, (start, end))) = words.iter().enumerate().find_map(|(i, w)| range(w).map(|r| (i, r))) else {
            continue;
        };
        let Some(perms) = words
            .iter()
            .skip(i + 1)
            .find(|w| w.contains('/') && w.len() >= 7 && w.chars().take(3).all(|c| "rwx-".contains(c)))
        else {
            continue;
        };
        // [ VSIZE RSDNT DIRTY SWAP ]
        let sizes: Vec<_> = line
            .split_once('[')
            .and_then(|(_, r)| r.split_once(']'))
            .map(|(r, _)| r.split_whitespace().map(amount).collect())
            .unwrap_or_default();
        let size = |n: usize| sizes.get(n).copied().flatten();
        let kind = words[..i].join(" ");
        let detail = words
            .iter()
            .skip_while(|w| !w.starts_with("SM="))
            .skip(1)
            .skip_while(|w| w.starts_with("PURGE="))
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        let path = detail.starts_with('/').then(|| detail.clone());
        regions.push(MemoryRegion {
            start,
            end,
            permissions: perms.split('/').next().unwrap_or_default().into(),
            name: if detail.is_empty() { kind } else { format!("{kind} · {detail}") },
            resident: size(1),
            private: None,
            shared: None,
            swap: size(3),
            dirty: size(2),
            path,
            offset: None,
        });
    }
    regions.sort_by_key(|r| r.start);
    regions.dedup_by_key(|r| r.start);
    regions
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn parse_smaps(text: &str) -> Vec<MemoryRegion> {
    let mut regions: Vec<MemoryRegion> = Vec::new();
    for line in text.lines() {
        let words: Vec<_> = line.split_whitespace().collect();
        if let Some((start, end)) = words.first().and_then(|w| range(w)) {
            if let Some(perms) = words.get(1) {
                let name = words.iter().skip(5).copied().collect::<Vec<_>>().join(" ");
                let path = name.starts_with('/').then(|| name.clone());
                regions.push(MemoryRegion {
                    start,
                    end,
                    permissions: (*perms).into(),
                    offset: path.as_ref().and(words.get(2)).and_then(|w| u64::from_str_radix(w, 16).ok()),
                    path,
                    name,
                    ..Default::default()
                });
            }
        } else if let (Some(region), Some(key), Some(value)) =
            (regions.last_mut(), words.first(), words.get(1).and_then(|w| amount(&format!("{w}kB"))))
        {
            match *key {
                "Rss:" => region.resident = Some(value),
                "Swap:" => region.swap = Some(value),
                "Private_Clean:" | "Private_Dirty:" => {
                    region.private = Some(region.private.unwrap_or(0).saturating_add(value))
                }
                "Shared_Clean:" | "Shared_Dirty:" => {
                    region.shared = Some(region.shared.unwrap_or(0).saturating_add(value))
                }
                _ => {}
            }
        }
    }
    regions
}
