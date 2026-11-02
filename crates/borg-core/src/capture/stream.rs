//! A conversation's bytes as its two sides sent them: TCP segments put back in order by their
//! sequence numbers without the repeats, UDP and ICMP payloads as they came, in turns of one
//! side speaking.

use super::{
    HexLayout, KeyLog, Packet,
    decode::{Datagram, SYN, Tcp},
    flows::{Flow, FlowKind},
};
use std::{ops::Range, time::Duration};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stream {
    /// Every turn's bytes, one turn after another.
    pub bytes: Vec<u8>,
    pub turns: Vec<Turn>,
    /// A key log opened its TLS records, and the bytes are what they carried.
    pub decrypted: bool,
}

/// One side's bytes up to the other side's answer, or up to bytes the capture lacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// The conversation's client sent it.
    pub from_client: bool,
    /// When the packet that completed its first bytes was captured.
    pub stamp: Duration,
    /// Where its bytes lie in `Stream::bytes`.
    pub range: Range<usize>,
    /// Bytes its side sent right before it that the capture lacks.
    pub missing: u64,
    /// Its bytes are TLS records that no key opened.
    pub sealed: bool,
}

impl Turn {
    pub fn len(&self) -> usize {
        self.range.len()
    }

    pub fn is_empty(&self) -> bool {
        self.range.is_empty()
    }
}

/// A row of a stream laid out as a hex dump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamRow<'a> {
    /// A turn begins.
    Turn(&'a Turn),
    /// A line of a turn's bytes, `offset` bytes into the turn.
    Bytes { offset: usize, bytes: &'a [u8] },
}

/// Bytes one side sent, whole once the packet at `delivered` came, after `missing` bytes the
/// capture lacks.
struct Piece<'a> {
    delivered: usize,
    from_client: bool,
    data: &'a [u8],
    missing: u64,
}

impl Stream {
    /// The bytes of `flow`, whose packets are among `packets`.
    pub fn of(flow: &Flow, packets: &[Packet]) -> Self {
        let mut pieces: Vec<Piece> = match flow.kind {
            FlowKind::Tcp => [true, false].into_iter().flat_map(|side| side_pieces(flow, packets, side)).collect(),
            FlowKind::Udp | FlowKind::Icmp => datagram_pieces(flow, packets),
        };
        pieces.sort_by_key(|piece| piece.delivered);
        let mut stream = Self::default();
        for piece in pieces {
            let start = stream.bytes.len();
            stream.bytes.extend_from_slice(piece.data);
            match stream.turns.last_mut() {
                Some(turn) if turn.from_client == piece.from_client && piece.missing == 0 => {
                    turn.range.end = stream.bytes.len();
                }
                _ => stream.turns.push(Turn {
                    from_client: piece.from_client,
                    stamp: packets[piece.delivered].stamp,
                    range: start..stream.bytes.len(),
                    missing: piece.missing,
                    sealed: false,
                }),
            }
        }
        stream
    }

    /// The stream with its TLS records opened by `keys`, when it is TLS and they hold its
    /// connection's secrets.
    pub fn decrypted(&self, keys: &KeyLog) -> Option<Self> {
        super::tls::decrypt(self, keys)
    }

    /// The layout of the widest dump that fits `width` cells.
    pub fn layout(&self, width: usize) -> HexLayout {
        HexLayout::fitting(width, self.turns.iter().map(Turn::len).max().unwrap_or_default())
    }

    pub fn row_count(&self, layout: HexLayout) -> usize {
        self.turns.iter().map(|turn| turn_rows(turn, layout)).sum()
    }

    /// The rows from row `top` on.
    pub fn rows_from(&self, layout: HexLayout, top: usize) -> impl Iterator<Item = StreamRow<'_>> {
        let mut skip = top;
        let first = self
            .turns
            .iter()
            .position(|turn| {
                let rows = turn_rows(turn, layout);
                if skip < rows {
                    return true;
                }
                skip -= rows;
                false
            })
            .unwrap_or(self.turns.len());
        self.turns[first..]
            .iter()
            .flat_map(move |turn| {
                let bytes = &self.bytes[turn.range.clone()];
                let lines = bytes
                    .chunks(layout.columns)
                    .enumerate()
                    .map(move |(row, bytes)| StreamRow::Bytes { offset: row * layout.columns, bytes });
                std::iter::once(StreamRow::Turn(turn)).chain(lines)
            })
            .skip(skip)
    }

    /// The row the next turn after row `top` begins on, or the previous one before it.
    pub fn turn_row(&self, layout: HexLayout, top: usize, forward: bool) -> Option<usize> {
        let mut starts = self.turns.iter().scan(0, |row, turn| {
            let start = *row;
            *row += turn_rows(turn, layout);
            Some(start)
        });
        if forward { starts.find(|&start| start > top) } else { starts.take_while(|&start| start < top).last() }
    }
}

fn turn_rows(turn: &Turn, layout: HexLayout) -> usize {
    1 + layout.rows(turn.len())
}

fn datagram_pieces<'a>(flow: &Flow, packets: &'a [Packet]) -> Vec<Piece<'a>> {
    flow.packets
        .iter()
        .map(|&number| (number, &packets[number]))
        .filter(|(_, packet)| !packet.payload().is_empty())
        .map(|(delivered, packet)| Piece {
            delivered,
            from_client: flow.from_client(packet),
            data: packet.payload(),
            missing: 0,
        })
        .collect()
}

/// What one side of a TCP conversation sent, in the order of its sequence numbers. Each piece
/// counts as whole once every byte before it came, as the receiving program would read it.
fn side_pieces<'a>(flow: &Flow, packets: &'a [Packet], from_client: bool) -> Vec<Piece<'a>> {
    let segments: Vec<(usize, Tcp, &[u8])> = flow
        .packets
        .iter()
        .map(|&number| (number, &packets[number]))
        .filter(|(_, packet)| flow.from_client(packet) == from_client)
        .filter_map(|(number, packet)| match packet.layers.datagram {
            Some(Datagram::Tcp(tcp)) => Some((number, tcp, packet.payload())),
            _ => None,
        })
        .collect();
    let Some(base) = first_byte(&segments) else { return Vec::new() };
    let mut sent: Vec<(u64, usize, &[u8])> = segments
        .iter()
        .filter(|(_, _, payload)| !payload.is_empty())
        .map(|&(number, tcp, payload)| (u64::from(data_start(tcp).wrapping_sub(base)), number, payload))
        .collect();
    sent.sort_by_key(|&(at, number, _)| (at, number));
    let (mut next, mut delivered) = (0, 0);
    let mut pieces = Vec::new();
    for (at, number, payload) in sent {
        let end = at + payload.len() as u64;
        if end <= next {
            continue;
        }
        delivered = delivered.max(number);
        let skip = next.saturating_sub(at) as usize;
        pieces.push(Piece { delivered, from_client, data: &payload[skip..], missing: at.saturating_sub(next) });
        next = end;
    }
    pieces
}

/// The sequence number of a segment's first byte of data, which comes after its SYN.
fn data_start(tcp: Tcp) -> u32 {
    tcp.seq.wrapping_add(u32::from(tcp.has(SYN)))
}

/// The sequence number of the side's first byte: the one after its SYN, or else the earliest
/// its segments carry.
fn first_byte(segments: &[(usize, Tcp, &[u8])]) -> Option<u32> {
    if let Some((_, syn, _)) = segments.iter().find(|(_, tcp, _)| tcp.has(SYN)) {
        return Some(data_start(*syn));
    }
    let first = segments.first()?.1.seq;
    let earliest = segments.iter().map(|(_, tcp, _)| tcp.seq.wrapping_sub(first) as i32).min()?.min(0);
    Some(first.wrapping_add_signed(earliest))
}

#[cfg(test)]
mod tests;
