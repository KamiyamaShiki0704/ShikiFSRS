use shikifsrs_runtime::{
    Error,
    memory::{AddressSpace, SliceMemory},
    pe::FileImage,
    signature::Pattern,
};

fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x600];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
    bytes[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
    bytes[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes());
    bytes[0x98..0x9a].copy_from_slice(&0x20bu16.to_le_bytes());
    bytes[0xb0..0xb8].copy_from_slice(&0x140000000u64.to_le_bytes());
    bytes[0xd0..0xd4].copy_from_slice(&0x3000u32.to_le_bytes());
    bytes[0xd4..0xd8].copy_from_slice(&0x200u32.to_le_bytes());
    bytes[0x188..0x18d].copy_from_slice(b".text");
    bytes[0x190..0x194].copy_from_slice(&0x800u32.to_le_bytes());
    bytes[0x194..0x198].copy_from_slice(&0x1000u32.to_le_bytes());
    bytes[0x198..0x19c].copy_from_slice(&0x400u32.to_le_bytes());
    bytes[0x19c..0x1a0].copy_from_slice(&0x200u32.to_le_bytes());
    bytes[0x1ac..0x1b0].copy_from_slice(&0x60000020u32.to_le_bytes());
    bytes[0x200..0x205].copy_from_slice(&[0x48, 0x8b, 0x41, 0x08, 0xc3]);
    bytes
}

#[test]
fn file_rvas_do_not_alias_raw_offsets_or_zero_fill_tails() {
    let file = fixture();
    let image = FileImage::parse(&file).unwrap();
    assert_eq!(image.preferred_base, 0x140000000);
    assert_eq!(
        image.read_rva(0x1000, 5).unwrap(),
        &[0x48, 0x8b, 0x41, 0x08, 0xc3]
    );
    assert!(image.read_rva(0x200, 1).is_err());
    assert!(image.read_rva(0x1400, 1).is_err());
    assert!(image.read_rva(0x13ff, 2).is_err());
    assert!(image.section_at(0x17ff).unwrap().is_executable());
    assert!(image.section_at(0x1800).is_none());
}

#[test]
fn every_truncated_prefix_is_rejected_without_panicking() {
    let file = fixture();
    for length in 0..file.len() {
        assert!(
            FileImage::parse(&file[..length]).is_err(),
            "length={length:#x}"
        );
    }
}

#[test]
fn malformed_architecture_sections_and_overflows_are_rejected() {
    for (offset, replacement) in [
        (0x84, 0x014cu32.to_le_bytes().to_vec()),
        (0x3c, u32::MAX.to_le_bytes().to_vec()),
        (0x194, 0xfffffff0u32.to_le_bytes().to_vec()),
        (0x1ac, 0x60000020u32.to_le_bytes().to_vec()),
    ] {
        let mut file = fixture();
        file[offset..offset + replacement.len()].copy_from_slice(&replacement);
        if offset == 0x1ac {
            file[0x198..0x19c].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        assert!(FileImage::parse(&file).is_err(), "offset={offset:#x}");
    }
}

#[test]
fn overlapping_sections_are_rejected_instead_of_selecting_the_first() {
    let mut file = fixture();
    file[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
    let first = file[0x188..0x1b0].to_vec();
    file[0x1b0..0x1d8].copy_from_slice(&first);
    assert!(FileImage::parse(&file).is_err());
}

#[test]
fn byte_reads_accept_unaligned_data_and_reject_partial_spans() {
    let bytes = [0, 0x78, 0x56, 0x34, 0x12];
    let memory = SliceMemory::new(0x1000, &bytes).unwrap();
    assert_eq!(memory.read_u32(0x1001).unwrap(), 0x12345678);
    assert!(memory.read_u32(0x1002).is_err());
    assert!(memory.read_u8(0xfff).is_err());
    assert!(SliceMemory::new(u64::MAX, &bytes).is_err());
    memory.read_exact(u64::MAX, &mut []).unwrap();
}

#[test]
fn rip_targets_resolve_negative_displacements_and_reject_overflow() {
    let bytes = (-12i32).to_le_bytes();
    let memory = SliceMemory::new(0x1008, &bytes).unwrap();
    assert_eq!(memory.rip_target(0x1008, 0).unwrap(), 0x1000);
    assert!(memory.rip_target(0x1008, u64::MAX).is_err());
    let low = SliceMemory::new(0, &bytes).unwrap();
    assert_eq!(low.rip_target(0, 0), Err(Error::AddressOverflow));
}

#[test]
fn ambiguous_signatures_and_nonidentifying_patterns_cannot_resolve() {
    let pattern: Pattern = "48 8B ?? 08".parse().unwrap();
    assert_eq!(pattern.find_unique(&[0, 0x48, 0x8b, 0x41, 8]).unwrap(), 1);
    assert_eq!(
        pattern.find_unique(&[0x48, 0x8b, 0, 8, 0x48, 0x8b, 1, 8]),
        Err(Error::AmbiguousPattern {
            first: 0,
            second: 4
        })
    );
    assert_eq!(
        pattern.find_unique(&[0x48, 0x8b]),
        Err(Error::PatternNotFound)
    );
    for text in ["", "?? ?", "GG", "0x48", "8", "????"] {
        assert!(text.parse::<Pattern>().is_err());
    }
}

#[cfg(windows)]
#[test]
fn process_adapter_copies_valid_bytes_and_reports_unmapped_memory() {
    use shikifsrs_runtime::memory::CurrentProcess;
    let value = 0x12345678u32;
    assert_eq!(
        CurrentProcess
            .read_u32((&value as *const u32) as u64)
            .unwrap(),
        value
    );
    assert!(matches!(
        CurrentProcess.read_u64(1),
        Err(Error::ProcessReadFailed { .. })
    ));
}

#[cfg(windows)]
#[test]
fn owned_readonly_process_handle_reads_and_rejects_invalid_pid() {
    use shikifsrs_runtime::memory::RemoteProcess;
    let process = RemoteProcess::open_readonly(std::process::id()).unwrap();
    let value = 0xfeedbeefu32;
    assert_eq!(
        process.read_u32((&value as *const u32) as u64).unwrap(),
        value
    );
    assert!(process.read_u64(1).is_err());
    assert!(matches!(
        RemoteProcess::open_readonly(0),
        Err(Error::WindowsCall { .. })
    ));
    process.read_exact(u64::MAX, &mut []).unwrap();
}
