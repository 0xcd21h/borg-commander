//! The two file formats captures come in: pcap, as `tcpdump -w` writes it, and pcapng, as
//! Wireshark saves it and macOS's tcpdump does on its `pktap` interface, with the process
//! behind each packet, and with the TLS secrets that decrypt its connections.

use super::{CaptureError, decode};
use std::{collections::HashMap, net::SocketAddr, time::Duration};

const PCAP_MICROS: u32 = 0xa1b2_c3d4;
const PCAP_NANOS: u32 = 0xa1b2_3c4d;
const PCAP_HEADER: usize = 24;
/// The magic number that opens either format.
const PCAP_MAGIC: usize = 4;
const PCAP_SNAP_AT: usize = 16;
const PCAP_LINK_AT: usize = 20;
/// The bits of a pcap header's link field that give the link type.
const PCAP_LINK_TYPE: u32 = 0xffff;
const RECORD_HEADER: usize = 16;
const RECORD_CAPTURED_AT: usize = 8;
const RECORD_LENGTH_AT: usize = 12;
const PCAPNG_SECTION: u32 = 0x0a0d_0d0a;
const PCAPNG_BYTE_ORDER: u32 = 0x1a2b_3c4d;
const PCAPNG_INTERFACE: u32 = 1;
const PCAPNG_SIMPLE: u32 = 3;
const PCAPNG_ENHANCED: u32 = 6;
const PCAPNG_VERSION: [u16; 2] = [1, 0];
/// Where an enhanced packet block's captured length and packet data start.
const ENHANCED_CAPTURED_AT: usize = BLOCK_HEADER + 12;
const ENHANCED_LENGTH_AT: usize = ENHANCED_CAPTURED_AT + 4;
const ENHANCED_DATA_AT: usize = 28;
/// What a section header says when it does not tell its length.
const UNKNOWN_SECTION_LENGTH: i64 = -1;
/// A decryption secrets block, and the type of secrets in a TLS key log.
const PCAPNG_SECRETS: u32 = 0x0000_000a;
const TLS_KEY_LOG: u32 = 0x544c_534b;
/// Where a secrets block's type, length and secrets start.
const SECRETS_AT: usize = BLOCK_HEADER + 8;
/// The process information block macOS adds: a process's PID and name, which the packets that
/// follow point at by its place among the section's process blocks.
const PCAPNG_PROCESS: u32 = 0x8000_0001;
/// What the first bytes of each pcapng block tell: its type, its length, and for a section
/// header the byte order.
const BLOCK_START: usize = 12;
const BLOCK_HEADER: usize = 8;
const BLOCK_TRAILER: usize = 4;
const OPTION_END: u16 = 0;
const OPTION_NAME: u16 = 2;
const OPTION_RESOLUTION: u16 = 9;
const OPTION_PROCESS_NAME: u16 = 2;
/// An enhanced packet's option giving its process block's place in the section.
const OPTION_PROCESS: u16 = 0x8001;
/// The PID a process block holds when the system could not tell one.
const NO_PROCESS: u32 = u32::MAX;
const POWER_OF_TWO: u8 = 0x80;
const MICROS_PER_SECOND: u64 = 1_000_000;
const NANOS_PER_SECOND: u64 = 1_000_000_000;
const DEFAULT_RESOLUTION: u8 = 6;
/// The `if_tsresol` of an interface whose timestamps count nanoseconds.
const NANO_RESOLUTION: u8 = 9;
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
    /// The PID of the process that sent or received it, when the file records one.
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Frames {
    pub frames: Vec<Frame>,
    /// The names of the processes the file records, by PID.
    pub processes: HashMap<u32, String>,
    /// The TLS key logs the file holds.
    pub secrets: Vec<String>,
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

    fn put_u16(self, out: &mut Vec<u8>, value: u16) {
        out.extend(if self.big { value.to_be_bytes() } else { value.to_le_bytes() });
    }

    fn put_u32(self, out: &mut Vec<u8>, value: u32) {
        out.extend(if self.big { value.to_be_bytes() } else { value.to_le_bytes() });
    }

    fn put_size(self, out: &mut Vec<u8>, value: usize) -> Option<()> {
        self.put_u32(out, u32::try_from(value).ok()?);
        Some(())
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
    let length = order.u32(bytes, at + RECORD_LENGTH_AT)?;
    let start = at + RECORD_HEADER;
    let data = bytes.get(start..start.checked_add(captured.min(LARGEST_RECORD))?)?;
    let stamp = Duration::from_secs(seconds) + Duration::from_nanos(fraction * (NANOS_PER_SECOND / units));
    Some((Frame { stamp, length, data: data.to_vec(), link, interface: None, pid: None }, start + data.len()))
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
    let mut processes: Vec<Option<u32>> = Vec::new();
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
            processes.clear();
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
            PCAPNG_PROCESS => {
                let (pid, name) = process(block, order);
                if let (Some(pid), Some(name)) = (pid, name) {
                    frames.processes.insert(pid, name);
                }
                processes.push(pid);
            }
            PCAPNG_ENHANCED => {
                if let Some(frame) = enhanced(block, order, &interfaces, &processes) {
                    keep(&mut frames, frame, limit);
                }
            }
            PCAPNG_SIMPLE => {
                if let Some(frame) = simple(block, order, &interfaces) {
                    keep(&mut frames, frame, limit);
                }
            }
            PCAPNG_SECRETS => frames.secrets.extend(key_log(block, order)),
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

/// A process block's PID, unless the system could not tell it, and the name it gives.
fn process(block: &[u8], order: Order) -> (Option<u32>, Option<String>) {
    const OPTIONS_AT: usize = BLOCK_HEADER + 4;
    let pid = order.u32(block, BLOCK_HEADER).filter(|pid| *pid != NO_PROCESS);
    let name = options(block, OPTIONS_AT, order)
        .into_iter()
        .find(|(code, _)| *code == OPTION_PROCESS_NAME)
        .map(|(_, value)| String::from_utf8_lossy(value).trim_end_matches('\0').to_owned())
        .filter(|name| !name.is_empty());
    (pid, name)
}

fn enhanced(block: &[u8], order: Order, interfaces: &[Interface], processes: &[Option<u32>]) -> Option<Frame> {
    const DATA_AT: usize = ENHANCED_DATA_AT;
    let interface = interfaces.get(order.size(block, BLOCK_HEADER)?)?;
    let high = u64::from(order.u32(block, BLOCK_HEADER + 4)?);
    let low = u64::from(order.u32(block, BLOCK_HEADER + 8)?);
    let captured = order.size(block, ENHANCED_CAPTURED_AT)?;
    let length = order.u32(block, ENHANCED_LENGTH_AT)?;
    let data = block.get(DATA_AT..DATA_AT + captured)?;
    let ticks = (high << 32) | low;
    let stamp = Duration::from_secs(ticks / interface.units)
        + Duration::from_nanos((ticks % interface.units).saturating_mul(NANOS_PER_SECOND) / interface.units);
    let pid = options(block, DATA_AT + padded(captured), order)
        .into_iter()
        .find(|(code, value)| *code == OPTION_PROCESS && value.len() == 4)
        .and_then(|(_, value)| *processes.get(order.size(value, 0)?)?);
    Some(Frame { stamp, length, data: data.to_vec(), link: interface.link, interface: interface.name.clone(), pid })
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
        pid: None,
    })
}

/// The TLS key log a decryption secrets block holds.
fn key_log(block: &[u8], order: Order) -> Option<String> {
    let length = order.size(block, BLOCK_HEADER + 4)?;
    let text = block.get(SECRETS_AT..SECRETS_AT.checked_add(length)?)?;
    (order.u32(block, BLOCK_HEADER)? == TLS_KEY_LOG).then(|| String::from_utf8_lossy(text).into_owned())
}

/// The capture in `bytes` as pcapng, with the TLS key log `key_log` in a decryption secrets
/// block after its section header, where Wireshark finds it before the packets. A pcap file is
/// written out as pcapng first. `None` when `bytes` are neither.
pub fn with_secrets(bytes: &[u8], key_log: &str) -> Option<Vec<u8>> {
    let little = Order { big: false };
    if little.u32(bytes, 0)? == PCAPNG_SECTION {
        let order = section_order(bytes, 0)?;
        let header = order.size(bytes, 4).filter(|length| *length <= bytes.len())?;
        return Some([&bytes[..header], &secrets_block(order, key_log)?, &bytes[header..]].concat());
    }
    let (order, _) = pcap_header(bytes).ok()?;
    let found = pcap(bytes, usize::MAX).ok()?;
    let mut out = section_block(little)?;
    let link = u16::try_from(order.u32(bytes, PCAP_LINK_AT)? & PCAP_LINK_TYPE).ok()?;
    out.extend(nano_interface_block(little, link, order.u32(bytes, PCAP_SNAP_AT)?)?);
    out.extend(secrets_block(little, key_log)?);
    for frame in &found.frames {
        out.extend(packet_block(little, frame)?);
    }
    Some(out)
}

/// A pcapng block of type `kind` around `body`, padded to whole words.
fn block(order: Order, kind: u32, body: &[u8]) -> Option<Vec<u8>> {
    let total = BLOCK_HEADER + padded(body.len()) + BLOCK_TRAILER;
    let mut out = Vec::with_capacity(total);
    order.put_u32(&mut out, kind);
    order.put_size(&mut out, total)?;
    out.extend(body);
    out.resize(total - BLOCK_TRAILER, 0);
    order.put_size(&mut out, total)?;
    Some(out)
}

fn section_block(order: Order) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    order.put_u32(&mut body, PCAPNG_BYTE_ORDER);
    PCAPNG_VERSION.into_iter().for_each(|part| order.put_u16(&mut body, part));
    body.extend(if order.big { UNKNOWN_SECTION_LENGTH.to_be_bytes() } else { UNKNOWN_SECTION_LENGTH.to_le_bytes() });
    block(order, PCAPNG_SECTION, &body)
}

/// An interface whose packets have link type `link`, at most `snap` bytes of each kept, and
/// timestamps in nanoseconds.
fn nano_interface_block(order: Order, link: u16, snap: u32) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    order.put_u16(&mut body, link);
    order.put_u16(&mut body, 0);
    order.put_u32(&mut body, snap);
    order.put_u16(&mut body, OPTION_RESOLUTION);
    order.put_u16(&mut body, 1);
    body.extend([NANO_RESOLUTION, 0, 0, 0]);
    order.put_u16(&mut body, OPTION_END);
    order.put_u16(&mut body, 0);
    block(order, PCAPNG_INTERFACE, &body)
}

fn secrets_block(order: Order, key_log: &str) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    order.put_u32(&mut body, TLS_KEY_LOG);
    order.put_size(&mut body, key_log.len())?;
    body.extend(key_log.as_bytes());
    block(order, PCAPNG_SECRETS, &body)
}

/// An enhanced packet block on the first interface, its timestamp in nanoseconds.
fn packet_block(order: Order, frame: &Frame) -> Option<Vec<u8>> {
    let nanos = u64::try_from(frame.stamp.as_nanos()).ok()?;
    let mut body = Vec::new();
    order.put_u32(&mut body, 0);
    order.put_u32(&mut body, (nanos >> u32::BITS) as u32);
    order.put_u32(&mut body, nanos as u32);
    order.put_size(&mut body, frame.data.len())?;
    order.put_u32(&mut body, frame.length);
    body.extend(&frame.data);
    block(order, PCAPNG_ENHANCED, &body)
}

/// The format a stream turned out to be, once its first bytes came.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stream {
    Pcap(Order),
    Pcapng(Order),
    /// Neither pcap nor pcapng, or damaged: nothing more is counted.
    Unknown,
}

/// Counts the packets of a pcap or pcapng stream as it grows, for a capture that is still
/// running.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PcapCounter {
    stream: Option<Stream>,
    header: Vec<u8>,
    /// Bytes still to pass before the next header.
    left: usize,
    /// Whether what is being passed is a packet.
    packet: bool,
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
                if self.left == 0 && self.packet {
                    self.packets += 1;
                }
                continue;
            }
            let wanted = match self.stream {
                None => PCAP_MAGIC,
                Some(Stream::Pcap(_)) => RECORD_HEADER,
                Some(Stream::Pcapng(_)) => BLOCK_START,
                Some(Stream::Unknown) => return,
            };
            let taken = wanted.saturating_sub(self.header.len()).min(chunk.len());
            self.header.extend_from_slice(&chunk[..taken]);
            chunk = &chunk[taken..];
            if self.header.len() < wanted {
                return;
            }
            self.read_header();
        }
    }

    /// Takes in a whole header: the stream's magic number, a pcap record's or a pcapng block's.
    fn read_header(&mut self) {
        let magic = Order { big: false }.u32(&self.header, 0);
        let next = match self.stream {
            None => match magic {
                Some(PCAPNG_SECTION) => {
                    self.stream = Some(Stream::Pcapng(Order { big: false }));
                    return;
                }
                _ => pcap_header(&self.header).ok().map(|(order, _)| {
                    self.stream = Some(Stream::Pcap(order));
                    (PCAP_HEADER - PCAP_MAGIC, false)
                }),
            },
            Some(Stream::Pcap(order)) => order.size(&self.header, RECORD_CAPTURED_AT).map(|captured| (captured, true)),
            Some(Stream::Pcapng(order)) => self.block(order),
            Some(Stream::Unknown) => None,
        };
        self.header.clear();
        match next {
            Some((0, true)) => self.packets += 1,
            Some((left, packet)) => (self.left, self.packet) = (left, packet),
            None => self.stream = Some(Stream::Unknown),
        }
    }

    /// The rest of a pcapng block to pass, and whether it holds a packet.
    fn block(&mut self, order: Order) -> Option<(usize, bool)> {
        let kind = order.u32(&self.header, 0)?;
        let order = if kind == PCAPNG_SECTION { section_order(&self.header, 0)? } else { order };
        self.stream = Some(Stream::Pcapng(order));
        let total = order.size(&self.header, 4).filter(|total| *total >= BLOCK_START)?;
        Some((total - BLOCK_START, matches!(kind, PCAPNG_ENHANCED | PCAPNG_SIMPLE)))
    }
}

/// A conversation as the budget counts it: its protocol and both ends, the lower one first.
type Conversation = (u8, SocketAddr, SocketAddr);

/// What a whole unit of a stream is to the budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// The stream's header, or bytes of a stream Borg cannot read, which pass as they are.
    Pass,
    Record(Order),
    Block(Order),
}

/// Keeps the start of every conversation in a pcap or pcapng stream as it grows: a
/// conversation's packets stay whole until they carried `budget` bytes of payload, and the
/// later ones keep their headers, up to where their payload starts. Each trimmed packet still
/// says how long it was, so readers know what the capture left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectionBudget {
    budget: u64,
    stream: Option<Stream>,
    /// What came after the last whole record or block.
    pending: Vec<u8>,
    /// The link type of a pcap stream, or of each interface of the pcapng section.
    links: Vec<u32>,
    carried: HashMap<Conversation, u64>,
}

impl ConnectionBudget {
    pub fn new(budget: u64) -> Self {
        Self { budget, ..Self::default() }
    }

    /// What to write for `chunk`, the next bytes the stream sent: every whole record or block
    /// so far, trimmed where its conversation used up its budget.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(chunk);
        let pending = std::mem::take(&mut self.pending);
        let mut out = Vec::with_capacity(pending.len());
        let mut at = 0;
        while let Some((length, part)) = self.unit(&pending[at..]) {
            let unit = &pending[at..at + length];
            let trimmed = match part {
                Part::Record(order) => self.trim_record(unit, order),
                Part::Block(order) => self.trim_block(unit, order),
                Part::Pass => None,
            };
            out.extend_from_slice(trimmed.as_deref().unwrap_or(unit));
            at += length;
        }
        self.pending = pending[at..].to_vec();
        out
    }

    /// The bytes held back when the stream ends: a record it did not finish.
    pub fn rest(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }

    /// How long the next unit is once all of it came, and what it is: the stream's header, a
    /// pcap record or a pcapng block. Bytes of a stream that is neither format pass as they are.
    fn unit(&mut self, bytes: &[u8]) -> Option<(usize, Part)> {
        let little = Order { big: false };
        let whole = |length: usize, part| (length > 0 && bytes.len() >= length).then_some((length, part));
        match self.stream {
            None => {
                if little.u32(bytes, 0)? == PCAPNG_SECTION {
                    self.stream = Some(Stream::Pcapng(little));
                    return self.unit(bytes);
                }
                let header = bytes.get(..PCAP_HEADER)?;
                self.stream = Some(match pcap_header(header) {
                    Ok((order, _)) => {
                        self.links = vec![order.u32(header, PCAP_LINK_AT)? & PCAP_LINK_TYPE];
                        Stream::Pcap(order)
                    }
                    Err(_) => Stream::Unknown,
                });
                Some((PCAP_HEADER, Part::Pass))
            }
            Some(Stream::Pcap(order)) => match order.size(bytes, RECORD_CAPTURED_AT)? {
                captured if captured <= LARGEST_RECORD => whole(RECORD_HEADER + captured, Part::Record(order)),
                _ => self.damaged(bytes),
            },
            Some(Stream::Pcapng(order)) => {
                let kind = order.u32(bytes, 0)?;
                let order = if kind == PCAPNG_SECTION { section_order(bytes, 0)? } else { order };
                match order.size(bytes, 4)? {
                    total if (BLOCK_START..=LARGEST_RECORD).contains(&total) => {
                        let found = whole(total, Part::Block(order))?;
                        self.stream = Some(Stream::Pcapng(order));
                        Some(found)
                    }
                    _ => self.damaged(bytes),
                }
            }
            Some(Stream::Unknown) => whole(bytes.len(), Part::Pass),
        }
    }

    /// Passes the rest of a stream whose record lengths make no sense as it is.
    fn damaged(&mut self, bytes: &[u8]) -> Option<(usize, Part)> {
        self.stream = Some(Stream::Unknown);
        self.unit(bytes)
    }

    /// A pcap record trimmed to its headers, or `None` to keep it whole.
    fn trim_record(&mut self, record: &[u8], order: Order) -> Option<Vec<u8>> {
        let data = record.get(RECORD_HEADER..)?;
        let keep = self.kept(*self.links.first()?, data)?;
        let mut out = record[..RECORD_CAPTURED_AT].to_vec();
        order.put_size(&mut out, keep)?;
        out.extend_from_slice(&record[RECORD_LENGTH_AT..RECORD_HEADER]);
        out.extend_from_slice(&data[..keep]);
        Some(out)
    }

    /// A pcapng block with its packet trimmed to its headers, or `None` to keep it whole. A
    /// section header starts over with no interfaces, and an interface block adds one.
    fn trim_block(&mut self, block: &[u8], order: Order) -> Option<Vec<u8>> {
        match order.u32(block, 0)? {
            PCAPNG_SECTION => {
                self.links.clear();
                None
            }
            PCAPNG_INTERFACE => {
                self.links.extend(order.u16(block, BLOCK_HEADER).map(u32::from));
                None
            }
            PCAPNG_ENHANCED => {
                let link = *self.links.get(order.size(block, BLOCK_HEADER)?)?;
                let captured = order.size(block, ENHANCED_CAPTURED_AT)?;
                let length = order.u32(block, ENHANCED_LENGTH_AT)?;
                let data = block.get(ENHANCED_DATA_AT..ENHANCED_DATA_AT + captured)?;
                let keep = self.kept(link, data)?;
                let options = block.get(ENHANCED_DATA_AT + padded(captured)..block.len() - BLOCK_TRAILER)?;
                let mut body = block[BLOCK_HEADER..ENHANCED_CAPTURED_AT].to_vec();
                order.put_size(&mut body, keep)?;
                order.put_u32(&mut body, length);
                body.extend_from_slice(&data[..keep]);
                body.resize(body.len() + padded(keep) - keep, 0);
                body.extend_from_slice(options);
                self::block(order, PCAPNG_ENHANCED, &body)
            }
            _ => None,
        }
    }

    /// How much of a packet to keep, when less than all of it: none of its payload once its
    /// conversation's packets carried the budget in payload.
    fn kept(&mut self, link: u32, data: &[u8]) -> Option<usize> {
        let layers = decode::layers(link, data);
        let ip = layers.ip?;
        let (source, destination) = layers.datagram?.ports().unwrap_or_default();
        let ends = (SocketAddr::new(ip.source, source), SocketAddr::new(ip.destination, destination));
        let carried = self.carried.entry((ip.protocol, ends.0.min(ends.1), ends.0.max(ends.1))).or_default();
        if *carried < self.budget {
            *carried += layers.carried as u64;
            return None;
        }
        let keep = layers.payload.start;
        (keep < data.len()).then_some(keep)
    }
}
