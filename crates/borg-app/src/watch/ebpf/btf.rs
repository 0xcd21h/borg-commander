//! Reads where the running kernel keeps the fields the programs follow to a command line, from
//! the type information the kernel publishes at `/sys/kernel/btf/vmlinux`.
//!
//! The layout of `task_struct` and `mm_struct` changes with every kernel build and configuration,
//! so no offset can be built into Borg. BTF describes each type as a list of records, and this
//! reader walks them for the few members it needs, following anonymous structs and unions the
//! way C does.

use std::collections::HashMap;

const MAGIC: u16 = 0xEB9F;
const HEADER_BYTES: usize = 24;
const TYPE_BYTES: usize = 12;

const KIND_INT: u32 = 1;
const KIND_ARRAY: u32 = 3;
const KIND_STRUCT: u32 = 4;
const KIND_UNION: u32 = 5;
const KIND_ENUM: u32 = 6;
const KIND_TYPEDEF: u32 = 8;
const KIND_VOLATILE: u32 = 9;
const KIND_CONST: u32 = 10;
const KIND_RESTRICT: u32 = 11;
const KIND_FUNC_PROTO: u32 = 13;
const KIND_VAR: u32 = 14;
const KIND_DATASEC: u32 = 15;
const KIND_DECL_TAG: u32 = 17;
const KIND_TYPE_TAG: u32 = 18;
const KIND_ENUM64: u32 = 19;
const KINDS_KNOWN: u32 = 19;

/// Where the programs find a process's command line: the task's memory, and the start and end
/// of the arguments in it. Byte offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub task_mm: u32,
    pub mm_arg_start: u32,
    pub mm_arg_end: u32,
}

impl Layout {
    /// Reads the layout from a kernel's BTF. Nothing when the BTF is not one Borg can read or
    /// lacks a member.
    pub fn read(btf: &[u8]) -> Option<Self> {
        let types = Types::parse(btf)?;
        let task = types.structure("task_struct")?;
        let mm = types.structure("mm_struct")?;
        Some(Self {
            task_mm: types.member(task, "mm", 0)?,
            mm_arg_start: types.member(mm, "arg_start", 0)?,
            mm_arg_end: types.member(mm, "arg_end", 0)?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct Member {
    name: u32,
    kind_of: u32,
    /// In bits from the start of the struct.
    offset_bits: u32,
}

#[derive(Debug, Clone)]
struct Type {
    kind: u32,
    /// What a typedef, qualifier or tag refers to.
    refers_to: u32,
    members: Vec<Member>,
}

struct Types<'a> {
    strings: &'a [u8],
    /// Indexed by type id; id 0 is void.
    types: Vec<Type>,
    structs: HashMap<&'a [u8], u32>,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

impl<'a> Types<'a> {
    /// The type records of a BTF blob. BTF comes in the byte order of the machine that wrote it,
    /// and the kernel's is always the byte order of the machine it runs on, which for every
    /// machine Borg ships for is little-endian.
    fn parse(btf: &'a [u8]) -> Option<Self> {
        if u16_at(btf, 0)? != MAGIC {
            return None;
        }
        let header = usize::try_from(u32_at(btf, 4)?).ok()?;
        let field = |at| u32_at(btf, at).and_then(|value| usize::try_from(value).ok());
        let (type_off, type_len) = (field(8)?, field(12)?);
        let (str_off, str_len) = (field(16)?, field(20)?);
        if header < HEADER_BYTES {
            return None;
        }
        let body = btf.get(header..)?;
        let records = body.get(type_off..type_off.checked_add(type_len)?)?;
        let strings = body.get(str_off..str_off.checked_add(str_len)?)?;

        let mut types = vec![Type { kind: 0, refers_to: 0, members: Vec::new() }];
        let mut structs = HashMap::new();
        let mut at = 0;
        while at < records.len() {
            let name = u32_at(records, at)?;
            let info = u32_at(records, at + 4)?;
            let size_or_type = u32_at(records, at + 8)?;
            let kind = (info >> 24) & 0x1f;
            let count = (info & 0xffff) as usize;
            let bitfields = info >> 31 == 1;
            at += TYPE_BYTES;
            let extra = match kind {
                KIND_INT | KIND_VAR | KIND_DECL_TAG => 4,
                KIND_ARRAY => 12,
                KIND_STRUCT | KIND_UNION | KIND_DATASEC | KIND_ENUM64 => 12 * count,
                KIND_ENUM | KIND_FUNC_PROTO => 8 * count,
                kind if kind <= KINDS_KNOWN => 0,
                _ => return None,
            };
            let mut members = Vec::new();
            if kind == KIND_STRUCT || kind == KIND_UNION {
                for index in 0..count {
                    let member = at + 12 * index;
                    let offset = u32_at(records, member + 8)?;
                    members.push(Member {
                        name: u32_at(records, member)?,
                        kind_of: u32_at(records, member + 4)?,
                        offset_bits: if bitfields { offset & 0x00ff_ffff } else { offset },
                    });
                }
            }
            at = at.checked_add(extra)?;
            if at > records.len() {
                return None;
            }
            let id = u32::try_from(types.len()).ok()?;
            if kind == KIND_STRUCT && name != 0 {
                structs.entry(text(strings, name)?).or_insert(id);
            }
            types.push(Type { kind, refers_to: size_or_type, members });
        }
        Some(Self { strings, types, structs })
    }

    fn structure(&self, name: &str) -> Option<u32> {
        self.structs.get(name.as_bytes()).copied()
    }

    /// The byte offset of a member of a struct or union, looking inside its anonymous members.
    fn member(&self, id: u32, name: &str, depth: u32) -> Option<u32> {
        if depth > 8 {
            return None;
        }
        let kind = self.types.get(id as usize)?;
        for member in &kind.members {
            let offset = if member.name == 0 {
                let inner = self.resolve(member.kind_of)?;
                match self.types.get(inner as usize)?.kind {
                    KIND_STRUCT | KIND_UNION => self.member(inner, name, depth + 1).map(|inner| (member, inner)),
                    _ => None,
                }
            } else if text(self.strings, member.name)? == name.as_bytes() {
                Some((member, 0))
            } else {
                None
            };
            if let Some((member, inner)) = offset {
                if member.offset_bits % 8 != 0 {
                    return None;
                }
                return (member.offset_bits / 8).checked_add(inner);
            }
        }
        None
    }

    /// The type a member's type names once typedefs, qualifiers and tags are set aside.
    fn resolve(&self, mut id: u32) -> Option<u32> {
        for _ in 0..16 {
            let kind = self.types.get(id as usize)?;
            match kind.kind {
                KIND_TYPEDEF | KIND_VOLATILE | KIND_CONST | KIND_RESTRICT | KIND_TYPE_TAG => id = kind.refers_to,
                _ => return Some(id),
            }
        }
        None
    }
}

/// A name from the string section, up to its closing zero.
fn text(strings: &[u8], at: u32) -> Option<&[u8]> {
    let rest = strings.get(at as usize..)?;
    Some(&rest[..memchr::memchr(0, rest)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes BTF the way the kernel lays it out, for the reader to read back.
    struct Blob {
        strings: Vec<u8>,
        records: Vec<u8>,
        next_id: u32,
    }

    impl Blob {
        fn new() -> Self {
            Self { strings: vec![0], records: Vec::new(), next_id: 1 }
        }

        fn name(&mut self, name: &str) -> u32 {
            if name.is_empty() {
                return 0;
            }
            let at = self.strings.len() as u32;
            self.strings.extend_from_slice(name.as_bytes());
            self.strings.push(0);
            at
        }

        fn record(&mut self, name: &str, kind: u32, count: u32, bitfields: bool, size_or_type: u32) -> u32 {
            let name = self.name(name);
            let info = (u32::from(bitfields) << 31) | (kind << 24) | count;
            for value in [name, info, size_or_type] {
                self.records.extend_from_slice(&value.to_le_bytes());
            }
            self.next_id += 1;
            self.next_id - 1
        }

        fn int(&mut self, name: &str) -> u32 {
            let id = self.record(name, KIND_INT, 0, false, 8);
            self.records.extend_from_slice(&64_u32.to_le_bytes());
            id
        }

        fn pointer(&mut self, to: u32) -> u32 {
            self.record("", 2, 0, false, to)
        }

        fn forward(&mut self, name: &str) -> u32 {
            self.record(name, 7, 0, false, 0)
        }

        fn typedef(&mut self, name: &str, to: u32) -> u32 {
            self.record(name, KIND_TYPEDEF, 0, false, to)
        }

        /// A struct or union of members, each a name, a type and an offset (in bits).
        fn aggregate(&mut self, kind: u32, name: &str, bitfields: bool, members: &[(&str, u32, u32)]) -> u32 {
            let named: Vec<_> = members.iter().map(|&(name, kind, offset)| (self.name(name), kind, offset)).collect();
            let id = self.record(name, kind, members.len() as u32, bitfields, 64);
            for (name, kind, offset) in named {
                for value in [name, kind, offset] {
                    self.records.extend_from_slice(&value.to_le_bytes());
                }
            }
            id
        }

        fn bytes(&self) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&MAGIC.to_le_bytes());
            out.push(1);
            out.push(0);
            let header = HEADER_BYTES as u32;
            let records = self.records.len() as u32;
            for value in [header, 0, records, records, self.strings.len() as u32] {
                out.extend_from_slice(&value.to_le_bytes());
            }
            out.extend_from_slice(&self.records);
            out.extend_from_slice(&self.strings);
            out
        }
    }

    /// A kernel's types as they are laid out: `task_struct` named before it is defined,
    /// `mm_struct` with its members inside an anonymous struct behind a typedef and a volatile
    /// qualifier, and bit-field offsets on the outer struct.
    fn kernel() -> Vec<u8> {
        let mut blob = Blob::new();
        let long = blob.int("unsigned long");
        blob.forward("task_struct");
        let mm = blob.forward("mm_struct");
        let mm_pointer = blob.pointer(mm);
        blob.aggregate(KIND_STRUCT, "task_struct", false, &[("pid", long, 0), ("mm", mm_pointer, 2_048 * 8)]);
        let arguments = blob.aggregate(
            KIND_STRUCT,
            "",
            false,
            &[("arg_start", long, 296 * 8), ("arg_end", long, 304 * 8), ("env_start", long, 312 * 8)],
        );
        let union = blob.aggregate(KIND_UNION, "", false, &[("padding", long, 0), ("", arguments, 0)]);
        let wrapped = blob.typedef("wrapped", union);
        let volatile = blob.record("", KIND_VOLATILE, 0, false, wrapped);
        blob.aggregate(KIND_STRUCT, "mm_struct", true, &[("count", long, 3 << 24), ("", volatile, 64)]);
        blob.bytes()
    }

    #[test]
    fn finds_the_command_line_inside_anonymous_members() {
        assert_eq!(Layout::read(&kernel()), Some(Layout { task_mm: 2_048, mm_arg_start: 304, mm_arg_end: 312 }));
    }

    #[test]
    fn reads_nothing_from_what_is_not_btf() {
        let mut wrong = kernel();
        wrong[0] = 0;
        assert_eq!(Layout::read(&wrong), None);
        let blob = kernel();
        assert_eq!(Layout::read(&blob[..blob.len() / 2]), None);
        assert_eq!(Layout::read(&[]), None);
    }

    #[test]
    fn reads_nothing_when_a_member_is_missing() {
        let mut blob = Blob::new();
        let long = blob.int("unsigned long");
        blob.aggregate(KIND_STRUCT, "task_struct", false, &[("mm", long, 64)]);
        blob.aggregate(KIND_STRUCT, "mm_struct", false, &[("arg_start", long, 0)]);
        assert_eq!(Layout::read(&blob.bytes()), None);
    }

    #[test]
    fn reads_nothing_from_a_kind_it_does_not_know() {
        let mut blob = Blob::new();
        blob.record("future", 30, 0, false, 0);
        assert_eq!(Layout::read(&blob.bytes()), None);
    }

    #[test]
    fn reads_the_running_kernel_where_there_is_one() {
        let Ok(btf) = std::fs::read("/sys/kernel/btf/vmlinux") else { return };
        let layout = Layout::read(&btf).expect("the kernel's own BTF has task_struct and mm_struct");
        assert!(layout.mm_arg_start < layout.mm_arg_end);
        assert_eq!(layout.mm_arg_end - layout.mm_arg_start, 8);
    }
}
