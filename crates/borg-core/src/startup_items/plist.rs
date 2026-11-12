//! Property lists as launchd reads them, XML or binary, as far as a launch agent's keys need:
//! dictionaries, arrays, strings, integers and booleans. Anything else reads as `Other`.

const XML_DEPTH: usize = 64;
const BINARY_MAGIC: &[u8] = b"bplist00";
const TRAILER_BYTES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    String(String),
    Integer(i64),
    Bool(bool),
    Array(Vec<Value>),
    Dict(Vec<(String, Value)>),
    Other,
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Dict(entries) => entries.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn flag(&self) -> Option<bool> {
        match self {
            Self::Bool(flag) => Some(*flag),
            _ => None,
        }
    }
}

/// The property list in `bytes`, or `None` when it is neither a well-formed XML plist nor a
/// binary one.
pub fn parse(bytes: &[u8]) -> Option<Value> {
    if bytes.starts_with(BINARY_MAGIC) {
        return Binary::new(bytes)?.top();
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut reader = Xml { rest: text };
    loop {
        match reader.next()? {
            Token::Open("plist") => return reader.value(0),
            Token::Open(_) | Token::Close(_) | Token::Empty(_) => return None,
            Token::Text(_) => {}
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Token<'a> {
    Open(&'a str),
    Close(&'a str),
    Empty(&'a str),
    Text(String),
}

struct Xml<'a> {
    rest: &'a str,
}

impl<'a> Xml<'a> {
    fn next(&mut self) -> Option<Token<'a>> {
        loop {
            if self.rest.is_empty() {
                return None;
            }
            if let Some(after) = self.rest.strip_prefix("<!--") {
                self.rest = &after[after.find("-->")? + 3..];
            } else if self.rest.starts_with("<?") || self.rest.starts_with("<!") {
                self.rest = &self.rest[self.rest.find('>')? + 1..];
            } else if let Some(after) = self.rest.strip_prefix('<') {
                let end = after.find('>')?;
                let tag = &after[..end];
                self.rest = &after[end + 1..];
                let name = |tag: &'a str| tag.split_whitespace().next().unwrap_or_default();
                return Some(if let Some(closed) = tag.strip_prefix('/') {
                    Token::Close(name(closed))
                } else if let Some(empty) = tag.strip_suffix('/') {
                    Token::Empty(name(empty))
                } else {
                    Token::Open(name(tag))
                });
            } else {
                let end = self.rest.find('<').unwrap_or(self.rest.len());
                let raw = &self.rest[..end];
                self.rest = &self.rest[end..];
                return Some(Token::Text(unescape(raw)));
            }
        }
    }

    fn significant(&mut self) -> Option<Token<'a>> {
        loop {
            match self.next()? {
                Token::Text(text) if text.trim().is_empty() => {}
                token => return Some(token),
            }
        }
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        if depth > XML_DEPTH {
            return None;
        }
        match self.significant()? {
            Token::Empty("true") => Some(Value::Bool(true)),
            Token::Empty("false") => Some(Value::Bool(false)),
            Token::Empty("dict") => Some(Value::Dict(Vec::new())),
            Token::Empty("array") => Some(Value::Array(Vec::new())),
            Token::Empty("string") => Some(Value::String(String::new())),
            Token::Empty(_) => Some(Value::Other),
            Token::Open("dict") => self.dict(depth),
            Token::Open("array") => {
                let mut items = Vec::new();
                loop {
                    if self.closes("array") {
                        return Some(Value::Array(items));
                    }
                    items.push(self.value(depth + 1)?);
                }
            }
            Token::Open(name @ ("string" | "integer")) => {
                let text = self.text(name)?;
                Some(if name == "string" {
                    Value::String(text)
                } else {
                    integer(&text).map_or(Value::Other, Value::Integer)
                })
            }
            Token::Open(name) => self.text(name).map(|_| Value::Other),
            Token::Close(_) | Token::Text(_) => None,
        }
    }

    fn dict(&mut self, depth: usize) -> Option<Value> {
        let mut entries = Vec::new();
        loop {
            match self.significant()? {
                Token::Close("dict") => return Some(Value::Dict(entries)),
                Token::Open("key") => {
                    let key = self.text("key")?;
                    entries.push((key, self.value(depth + 1)?));
                }
                Token::Empty("key") => entries.push((String::new(), self.value(depth + 1)?)),
                _ => return None,
            }
        }
    }

    fn closes(&mut self, name: &str) -> bool {
        let before = self.rest;
        match self.significant() {
            Some(Token::Close(closed)) if closed == name => true,
            _ => {
                self.rest = before;
                false
            }
        }
    }

    fn text(&mut self, name: &str) -> Option<String> {
        let mut text = String::new();
        loop {
            match self.next()? {
                Token::Text(part) => text.push_str(&part),
                Token::Close(closed) if closed == name => return Some(text),
                _ => return None,
            }
        }
    }
}

fn integer(text: &str) -> Option<i64> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => i64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find(';') else { break };
        let entity = &rest[1..end];
        let decoded = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .map(|hex| u32::from_str_radix(hex, 16))
                .or_else(|| entity.strip_prefix('#').map(str::parse))
                .and_then(Result::ok)
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A binary property list: its objects, where the offset table says each starts, and the
/// sizes of offsets and references.
struct Binary<'a> {
    bytes: &'a [u8],
    offsets: &'a [u8],
    offset_size: usize,
    reference_size: usize,
    count: usize,
    top: usize,
    /// Objects Borg may still read. A list can name one object many times, and lists of such
    /// lists a few levels deep stand for more objects than any machine holds.
    budget: std::cell::Cell<usize>,
}

const MARKER_SIMPLE: u8 = 0x0;
const MARKER_INTEGER: u8 = 0x1;
const MARKER_ASCII: u8 = 0x5;
const MARKER_UTF16: u8 = 0x6;
const MARKER_ARRAY: u8 = 0xA;
const MARKER_DICT: u8 = 0xD;
const SIMPLE_FALSE: u8 = 0x08;
const SIMPLE_TRUE: u8 = 0x09;
const LONG_COUNT: u8 = 0xF;
const BINARY_DEPTH: usize = 64;

impl<'a> Binary<'a> {
    fn new(bytes: &'a [u8]) -> Option<Self> {
        let trailer = bytes.get(bytes.len().checked_sub(TRAILER_BYTES)?..)?;
        let offset_size = usize::from(trailer[6]);
        let reference_size = usize::from(trailer[7]);
        let count = usize::try_from(be(&trailer[8..16])?).ok()?;
        let top = usize::try_from(be(&trailer[16..24])?).ok()?;
        let table = usize::try_from(be(&trailer[24..32])?).ok()?;
        if !(1..=8).contains(&offset_size) || !(1..=8).contains(&reference_size) || top >= count {
            return None;
        }
        let offsets = bytes.get(table..table.checked_add(count.checked_mul(offset_size)?)?)?;
        let budget = std::cell::Cell::new(bytes.len());
        Some(Self { bytes, offsets, offset_size, reference_size, count, top, budget })
    }

    fn top(&self) -> Option<Value> {
        self.object(self.top, 0)
    }

    fn object(&self, index: usize, depth: usize) -> Option<Value> {
        if index >= self.count || depth > BINARY_DEPTH {
            return None;
        }
        self.budget.set(self.budget.get().checked_sub(1)?);
        let at =
            usize::try_from(be(self.offsets.get(index * self.offset_size..(index + 1) * self.offset_size)?)?).ok()?;
        let marker = *self.bytes.get(at)?;
        let (kind, low) = (marker >> 4, marker & 0x0F);
        match kind {
            MARKER_SIMPLE => Some(match marker {
                SIMPLE_FALSE => Value::Bool(false),
                SIMPLE_TRUE => Value::Bool(true),
                _ => Value::Other,
            }),
            MARKER_INTEGER => {
                let size = 1usize.checked_shl(u32::from(low))?;
                let bytes = self.bytes.get(at + 1..at + 1 + size)?;
                Some(match size {
                    1..=4 => Value::Integer(be(bytes)? as i64),
                    8 => Value::Integer(i64::from_be_bytes(bytes.try_into().ok()?)),
                    _ => Value::Other,
                })
            }
            MARKER_ASCII | MARKER_UTF16 => {
                let (length, start) = self.length(at, low)?;
                if kind == MARKER_ASCII {
                    let bytes = self.bytes.get(start..start.checked_add(length)?)?;
                    return Some(Value::String(String::from_utf8_lossy(bytes).into_owned()));
                }
                let bytes = self.bytes.get(start..start.checked_add(length.checked_mul(2)?)?)?;
                let units: Vec<u16> = bytes.as_chunks().0.iter().map(|pair| u16::from_be_bytes(*pair)).collect();
                Some(Value::String(String::from_utf16_lossy(&units)))
            }
            MARKER_ARRAY => {
                let (length, start) = self.length(at, low)?;
                let items = (0..length)
                    .map(|slot| self.reference(start, slot).and_then(|item| self.object(item, depth + 1)))
                    .collect::<Option<Vec<_>>>()?;
                Some(Value::Array(items))
            }
            MARKER_DICT => {
                let (length, start) = self.length(at, low)?;
                let entries = (0..length)
                    .map(|slot| {
                        let key = self.object(self.reference(start, slot)?, depth + 1)?;
                        let value = self.object(self.reference(start, length.checked_add(slot)?)?, depth + 1)?;
                        Some((key.text().unwrap_or_default().to_owned(), value))
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(Value::Dict(entries))
            }
            _ => Some(Value::Other),
        }
    }

    /// A collection's or string's length and where its contents start: in the marker's low
    /// bits, or in the integer object after a marker whose low bits are all set.
    fn length(&self, at: usize, low: u8) -> Option<(usize, usize)> {
        if low != LONG_COUNT {
            return Some((usize::from(low), at + 1));
        }
        let marker = *self.bytes.get(at + 1)?;
        if marker >> 4 != MARKER_INTEGER {
            return None;
        }
        let size = 1usize.checked_shl(u32::from(marker & 0x0F))?;
        let length = usize::try_from(be(self.bytes.get(at + 2..at + 2 + size)?)?).ok()?;
        (length <= self.bytes.len()).then_some((length, at + 2 + size))
    }

    fn reference(&self, start: usize, slot: usize) -> Option<usize> {
        let from = start.checked_add(slot.checked_mul(self.reference_size)?)?;
        usize::try_from(be(self.bytes.get(from..from.checked_add(self.reference_size)?)?)?).ok()
    }
}

/// A big-endian unsigned number of up to eight bytes.
fn be(bytes: &[u8]) -> Option<u64> {
    (bytes.len() <= 8).then(|| bytes.iter().fold(0u64, |value, byte| value << 8 | u64::from(*byte)))
}
