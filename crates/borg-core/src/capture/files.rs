//! The two file formats captures come in: pcap, as `tcpdump -w` writes it, and pcapng, as
//! Wireshark saves it.

use super::CaptureError;
use std::time::Duration;

const PCAP_MICROS: u32 = 0xa1b2_c3d4;
const PCAP_NANOS: u32 = 0xa1b2_3c4d;
const PCAP_HEADER: usize = 24;
const PCAP_LINK_AT: usize = 20;
const RECORD_HEADER: usize = 16;
const RECORD_CAPTURED_AT: usize = 8;
const PCAPNG_SECTION: u32 = 0x0a0d_0d0a;
const PCAPNG_BYTE_ORDER: u32 = 0x1a2b_3c4d;
const PCAPNG_INTERFACE: u32 = 1;
const PCAPNG_SIMPLE: u32 = 3;
const PCAPNG_ENHANCED: u32 = 6;
const BLOCK_HEADER: usize = 8;
const BLOCK_TRAILER: usize = 4;
const OPTION_END: u16 = 0;
const OPTION_NAME: u16 = 2;
const OPTION_RESOLUTION: u16 = 9;
const POWER_OF_TWO: u8 = 0x80;
const MICROS_PER_SECOND: u64 = 1_000_000;
const NANOS_PER_SECOND: u64 = 1_000_000_000;
const DEFAULT_RESOLUTION: u8 = 6;
/// Records no packet comes near, which only a damaged file claims.
const LARGEST_RECORD: usize = 1 << 24;

/// One captured packet as the file holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Frame {
    pub stamp: Duration,
    pub length: u32,
    pub data: Vec<u8>,
    pub link: u32,
    pub interface: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Frames {
    pub frames: Vec<Frame>,
    /// Packets past the limit, left out.
    pub skipped: usize,
    /// The file ends inside a packet.
    pub cut: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Order {
    big: bool,
}

impl Order {
    fn u16(self, bytes: &[u8], at: usize) -> Option<u16> {
        let raw: [u8; 2] = bytes.get(at..at + 2)?.try_into().ok()?;
        Some(if self.big { u16::from_be_bytes(raw) } else { u16::from_le_bytes(raw) })
    }

    fn u32(self, bytes: &[u8], at: usize) -> Option<u32> {
        let raw: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
        Some(if self.big { u32::from_be_bytes(raw) } else { u32::from_le_bytes(raw) })
    }

    fn size(self, bytes: &[u8], at: usize) -> Option<usize> {
        self.u32(bytes, at).and_then(|value| usize::try_from(value).ok())
    }
}

/// The packets in a pcap or pcapng file, up to `limit` of them.
pub(super) fn frames(bytes: &[u8], limit: usize) -> Result<Frames, CaptureError> {
    let magic = Order { big: false }.u32(bytes, 0).ok_or(CaptureError::Short)?;
    if magic == PCAPNG_SECTION {
        return pcapng(bytes, limit);
    }
    pcap(bytes, limit)
}

/// The byte order and timestamp unit a pcap header announces.
fn pcap_header(bytes: &[u8]) -> Result<(Order, u64), CaptureError> {
    let magic = Order { big: false }.u32(bytes, 0).ok_or(CaptureError::Short)?;
    [false, true]
        .into_iter()
        .find_map(|big| match if big { magic.swap_bytes() } else { magic } {
            PCAP_MICROS => Some((Order { big }, MICROS_PER_SECOND)),
            PCAP_NANOS => Some((Order { big }, NANOS_PER_SECOND)),
            _ => None,
        })
        .ok_or(CaptureError::Unknown)
}

fn pcap(bytes: &[u8], limit: usize) -> Result<Frames, CaptureError> {
    let (order, units) = pcap_header(bytes)?;
    let link = order.u32(bytes, PCAP_LINK_AT).ok_or(CaptureError::Short)?;
    let mut frames = Frames::default();
    let mut at = PCAP_HEADER;
    while at < bytes.len() {
        let Some((frame, next)) = pcap_record(bytes, at, order, units, link) else {
            frames.cut = true;
            break;
        };
        keep(&mut frames, frame, limit);
        at = next;
    }
    Ok(frames)
}

fn pcap_record(bytes: &[u8], at: usize, order: Order, units: u64, link: u32) -> Option<(Frame, usize)> {
    let seconds = u64::from(order.u32(bytes, at)?);
    let fraction = u64::from(order.u32(bytes, at + 4)?);
    let captured = order.size(bytes, at + RECORD_CAPTURED_AT)?;
    let length = order.u32(bytes, at + 12)?;
    let start = at + RECORD_HEADER;
    let data = bytes.get(start..start.checked_add(captured.min(LARGEST_RECORD))?)?;
    let stamp = Duration::from_secs(seconds) + Duration::from_nanos(fraction * (NANOS_PER_SECOND / units));
    Some((Frame { stamp, length, data: data.to_vec(), link, interface: None }, start + data.len()))
}

fn keep(frames: &mut Frames, frame: Frame, limit: usize) {
    if frames.frames.len() < limit {
        frames.frames.push(frame);
    } else {
        frames.skipped += 1;
    }
}

/// An interface a pcapng section describes: its link type, name and timestamp unit.
#[derive(Debug, Clone)]
struct Interface {
    link: u32,
    name: Option<String>,
    units: u64,
}

fn pcapng(bytes: &[u8], limit: usize) -> Result<Frames, CaptureError> {
    let mut frames = Frames::default();
    let mut interfaces: Vec<Interface> = Vec::new();
    let mut order = Order { big: false };
    let mut at = 0;
    while at < bytes.len() {
        let Some(kind) = order.u32(bytes, at) else {
            frames.cut = true;
            break;
        };
        if kind == PCAPNG_SECTION {
            order = section_order(bytes, at).ok_or(CaptureError::Unknown)?;
            interfaces.clear();
        }
        let Some(total) = order.size(bytes, at + 4).filter(|total| *total >= BLOCK_HEADER + BLOCK_TRAILER) else {
            frames.cut = true;
            break;
        };
        let Some(block) = bytes.get(at..at + total) else {
            frames.cut = true;
            break;
        };
        match kind {
            PCAPNG_INTERFACE => interfaces.extend(interface(block, order)),
            PCAPNG_ENHANCED => {
                if let Some(frame) = enhanced(block, order, &interfaces) {
                    keep(&mut frames, frame, limit);
                }
            }
            PCAPNG_SIMPLE => {
                if let Some(frame) = simple(block, order, &interfaces) {
                    keep(&mut frames, frame, limit);
                }
            }
            _ => {}
        }
        at += total;
    }
    Ok(frames)
}

/// The byte order a section header's magic number gives.
fn section_order(bytes: &[u8], at: usize) -> Option<Order> {
    let magic = Order { big: false }.u32(bytes, at + BLOCK_HEADER)?;
    match magic {
        PCAPNG_BYTE_ORDER => Some(Order { big: false }),
        _ if magic.swap_bytes() == PCAPNG_BYTE_ORDER => Some(Order { big: true }),
        _ => None,
    }
}

/// Each option in a block's options, as its code and value.
fn options(block: &[u8], from: usize, order: Order) -> Vec<(u16, &[u8])> {
    let mut found = Vec::new();
    let mut at = from;
    let end = block.len().saturating_sub(BLOCK_TRAILER);
    while at + 4 <= end {
        let (Some(code), Some(length)) = (order.u16(block, at), order.u16(block, at + 2)) else { break };
        if code == OPTION_END {
            break;
        }
        let start = at + 4;
        let Some(value) = block.get(start..start + usize::from(length)) else { break };
        found.push((code, value));
        at = start + padded(usize::from(length));
    }
    found
}

fn padded(length: usize) -> usize {
    length.div_ceil(4) * 4
}

fn interface(block: &[u8], order: Order) -> Option<Interface> {
    const OPTIONS_AT: usize = 16;
    let link = u32::from(order.u16(block, BLOCK_HEADER)?);
    let mut found = Interface { link, name: None, units: MICROS_PER_SECOND };
    for (code, value) in options(block, OPTIONS_AT, order) {
        match code {
            OPTION_NAME => found.name = Some(String::from_utf8_lossy(value).trim_end_matches('\0').to_owned()),
            OPTION_RESOLUTION => found.units = resolution(value.first().copied().unwrap_or(DEFAULT_RESOLUTION)),
            _ => {}
        }
    }
    Some(found)
}

/// Timestamp units a second, from an interface's `if_tsresol` option.
fn resolution(code: u8) -> u64 {
    const LARGEST_EXPONENT: u32 = 19;
    let exponent = u32::from(code & !POWER_OF_TWO).min(LARGEST_EXPONENT);
    if code & POWER_OF_TWO == 0 { 10u64.pow(exponent) } else { 1u64 << exponent.min(63) }
}

fn enhanced(block: &[u8], order: Order, interfaces: &[Interface]) -> Option<Frame> {
    const DATA_AT: usize = 28;
    let interface = interfaces.get(order.size(block, BLOCK_HEADER)?)?;
    let high = u64::from(order.u32(block, BLOCK_HEADER + 4)?);
    let low = u64::from(order.u32(block, BLOCK_HEADER + 8)?);
    let captured = order.size(block, BLOCK_HEADER + 12)?;
    let length = order.u32(block, BLOCK_HEADER + 16)?;
    let data = block.get(DATA_AT..DATA_AT + captured)?;
    let ticks = (high << 32) | low;
    let stamp = Duration::from_secs(ticks / interface.units)
        + Duration::from_nanos((ticks % interface.units).saturating_mul(NANOS_PER_SECOND) / interface.units);
    Some(Frame { stamp, length, data: data.to_vec(), link: interface.link, interface: interface.name.clone() })
}

fn simple(block: &[u8], order: Order, interfaces: &[Interface]) -> Option<Frame> {
    const DATA_AT: usize = 12;
    let interface = interfaces.first()?;
    let length = order.u32(block, BLOCK_HEADER)?;
    let room = block.len().saturating_sub(DATA_AT + BLOCK_TRAILER);
    let data = block.get(DATA_AT..DATA_AT + room.min(usize::try_from(length).ok()?))?;
    Some(Frame {
        stamp: Duration::ZERO,
        length,
        data: data.to_vec(),
        link: interface.link,
        interface: interface.name.clone(),
    })
}

/// Counts the packets of a pcap stream as it grows, for a capture that is still running.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PcapCounter {
    order: Option<Order>,
    header: Vec<u8>,
    left: usize,
    pub packets: usize,
}

impl PcapCounter {
    /// Counts the whole packets in `chunk`, the next bytes the stream sent.
    pub fn feed(&mut self, mut chunk: &[u8]) {
        while !chunk.is_empty() {
            if self.left > 0 {
                let skipped = self.left.min(chunk.len());
                self.left -= skipped;
                chunk = &chunk[skipped..];
                if self.left == 0 {
                    self.packets += 1;
                }
                continue;
            }
            let wanted = if self.order.is_some() { RECORD_HEADER } else { PCAP_HEADER };
            let taken = wanted.saturating_sub(self.header.len()).min(chunk.len());
            self.header.extend_from_slice(&chunk[..taken]);
            chunk = &chunk[taken..];
            if self.header.len() < wanted {
                return;
            }
            match self.order {
                None => {
                    let Ok((order, _)) = pcap_header(&self.header) else { return };
                    self.order = Some(order);
                }
                Some(order) => match order.size(&self.header, RECORD_CAPTURED_AT) {
                    Some(0) => self.packets += 1,
                    Some(captured) => self.left = captured,
                    None => return,
                },
            }
            self.header.clear();
        }
    }
}
