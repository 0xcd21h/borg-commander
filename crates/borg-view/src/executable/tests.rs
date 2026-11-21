use super::*;

/// The code of a program with one code range.
fn only(range: Range<u64>) -> Option<Vec<Range<u64>>> {
    Some(std::iter::once(range).collect())
}

fn put(file: &mut Vec<u8>, at: usize, bytes: &[u8]) {
    if file.len() < at + bytes.len() {
        file.resize(at + bytes.len(), 0);
    }
    file[at..at + bytes.len()].copy_from_slice(bytes);
}

/// A 64-bit little-endian ELF program with a code segment, a data segment and a note.
fn elf64() -> Vec<u8> {
    let mut file = vec![0; 0x3000];
    put(&mut file, 0, b"\x7fELF\x02\x01\x01");
    put(&mut file, 0x10, &ET_DYN.to_le_bytes());
    put(&mut file, 0x20, &64u64.to_le_bytes());
    put(&mut file, 0x36, &(PROGRAM_HEADER_WIDE as u16).to_le_bytes());
    put(&mut file, 0x38, &3u16.to_le_bytes());
    let program = |kind: u32, flags: u32, offset: u64, size: u64| {
        let mut entry = vec![0; PROGRAM_HEADER_WIDE];
        put(&mut entry, 0, &kind.to_le_bytes());
        put(&mut entry, 4, &flags.to_le_bytes());
        put(&mut entry, 8, &offset.to_le_bytes());
        put(&mut entry, 32, &size.to_le_bytes());
        entry
    };
    put(&mut file, 64, &program(PT_LOAD, PF_X | 4, 0x1000, 0x800));
    put(&mut file, 64 + 56, &program(PT_LOAD, 4 | 2, 0x2000, 0x1000));
    put(&mut file, 64 + 112, &program(4, PF_X, 0x0, 0x100));
    file
}

/// A 32-bit big-endian ELF program whose code segment runs past the end of the file.
fn elf32_big() -> Vec<u8> {
    let mut file = vec![0; 0x1800];
    put(&mut file, 0, b"\x7fELF\x01\x02\x01");
    put(&mut file, 0x10, &ET_EXEC.to_be_bytes());
    put(&mut file, 0x1c, &52u32.to_be_bytes());
    put(&mut file, 0x2a, &(PROGRAM_HEADER_NARROW as u16).to_be_bytes());
    put(&mut file, 0x2c, &1u16.to_be_bytes());
    put(&mut file, 52, &PT_LOAD.to_be_bytes());
    put(&mut file, 52 + 4, &0x1000u32.to_be_bytes());
    put(&mut file, 52 + 16, &0x9000u32.to_be_bytes());
    put(&mut file, 52 + 24, &(PF_X | 4).to_be_bytes());
    file
}

/// A 64-bit Mach-O program with `__TEXT` mapped executable from `text` and `__DATA` after it.
fn mach_o64(kind: u32, text: Range<u64>) -> Vec<u8> {
    let mut file = vec![0; 0x4000];
    put(&mut file, 0, &MH_MAGIC_64.to_le_bytes());
    put(&mut file, 12, &kind.to_le_bytes());
    put(&mut file, 16, &2u32.to_le_bytes());
    put(&mut file, 20, &(72u32 * 2).to_le_bytes());
    let segment = |offset: u64, size: u64, protection: u32| {
        let mut command = vec![0; 72];
        put(&mut command, 0, &LC_SEGMENT_64.to_le_bytes());
        put(&mut command, 4, &72u32.to_le_bytes());
        put(&mut command, 40, &offset.to_le_bytes());
        put(&mut command, 48, &size.to_le_bytes());
        put(&mut command, 56, &7u32.to_le_bytes());
        put(&mut command, 60, &protection.to_le_bytes());
        command
    };
    put(&mut file, MACH_HEADER_64, &segment(text.start, text.end - text.start, 1 | VM_PROT_EXECUTE));
    put(&mut file, MACH_HEADER_64 + 72, &segment(text.end, 0x1000, 1 | 2));
    file
}

#[test]
fn elf_code_is_its_executable_loadable_segments() {
    assert_eq!(code_ranges(&elf64()).unwrap(), only(0x1000..0x1800));
    assert_eq!(code_ranges(&elf32_big()).unwrap(), only(0x1000..0x1800), "cut at the end of the file");
    let mut object = elf64();
    put(&mut object, 0x10, &1u16.to_le_bytes());
    assert_eq!(code_ranges(&object).unwrap(), None, "relocatable objects are no programs");
}

#[test]
fn mach_o_code_is_its_executable_segments_in_every_slice() {
    assert_eq!(code_ranges(&mach_o64(2, 0..0x2000)).unwrap(), only(0..0x2000));
    assert_eq!(code_ranges(&mach_o64(MH_OBJECT, 0..0x2000)).unwrap(), None);

    let (first, second) = (mach_o64(2, 0..0x1000), mach_o64(6, 0..0x3000));
    let mut universal = vec![0; 0x1000];
    put(&mut universal, 0, &FAT_MAGIC.to_be_bytes());
    put(&mut universal, 4, &2u32.to_be_bytes());
    for (index, offset) in [0x1000u32, 0x5000].into_iter().enumerate() {
        put(&mut universal, FAT_HEADER + index * FAT_ARCH + 8, &offset.to_be_bytes());
    }
    put(&mut universal, 0x1000, &first);
    put(&mut universal, 0x5000, &second);
    assert_eq!(code_ranges(&universal).unwrap(), Some(vec![0x1000..0x2000, 0x5000..0x8000]));

    let mut java = vec![0; 64];
    put(&mut java, 0, &FAT_MAGIC.to_be_bytes());
    put(&mut java, 4, &[0, 0, 0, 65]);
    assert_eq!(code_ranges(&java).unwrap(), None, "a Java class is no universal binary");
}

#[test]
fn pe_code_is_its_code_and_executable_sections() {
    let mut file = vec![0; 0x3000];
    put(&mut file, 0, MZ);
    put(&mut file, PE_POINTER, &0x80u32.to_le_bytes());
    put(&mut file, 0x80, PE_SIGNATURE);
    put(&mut file, 0x80 + 6, &3u16.to_le_bytes());
    put(&mut file, 0x80 + 20, &0xf0u16.to_le_bytes());
    let table = 0x80 + COFF_HEADER + 0xf0;
    let section = |offset: u32, size: u32, flags: u32| {
        let mut entry = vec![0; PE_SECTION];
        put(&mut entry, 16, &size.to_le_bytes());
        put(&mut entry, 20, &offset.to_le_bytes());
        put(&mut entry, 36, &flags.to_le_bytes());
        entry
    };
    put(&mut file, table, &section(0x400, 0x600, IMAGE_SCN_CNT_CODE | IMAGE_SCN_MEM_EXECUTE));
    put(&mut file, table + PE_SECTION, &section(0xa00, 0x200, 0x4000_0000));
    put(&mut file, table + 2 * PE_SECTION, &section(0xc00, 0x400, IMAGE_SCN_MEM_EXECUTE));
    assert_eq!(code_ranges(&file).unwrap(), Some(vec![0x400..0xa00, 0xc00..0x1000]));
}

#[test]
fn other_files_and_broken_headers_have_no_code() {
    assert_eq!(code_ranges(&b"#!/bin/sh\necho hi\n".to_vec()).unwrap(), None);
    assert_eq!(code_ranges(&Vec::new()).unwrap(), None);
    let mut short = elf64();
    short.truncate(20);
    assert_eq!(code_ranges(&short).unwrap(), None);
    let mut broken = elf64();
    put(&mut broken, 0x36, &8u16.to_le_bytes());
    assert_eq!(code_ranges(&broken).unwrap(), None, "program headers too small to read");
}

#[test]
fn ranges_are_sorted_and_merged() {
    assert_eq!(tidy(vec![50..60, 0..10, 5..20, 20..30, 70..70, 90..200], 100), vec![0..30, 50..60, 90..100]);
}
