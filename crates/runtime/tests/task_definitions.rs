use shikifsrs_runtime::{pe::FileImage, task_definitions::TaskDefinitions};

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x1200];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
    bytes[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
    bytes[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes());
    bytes[0x98..0x9a].copy_from_slice(&0x20bu16.to_le_bytes());
    put64(&mut bytes, 0xb0, 0x140000000);
    put32(&mut bytes, 0xd0, 0x3000);
    put32(&mut bytes, 0xd4, 0x200);
    bytes[0x188..0x18e].copy_from_slice(b".rdata");
    put32(&mut bytes, 0x190, 0x1000);
    put32(&mut bytes, 0x194, 0x1000);
    put32(&mut bytes, 0x198, 0x1000);
    put32(&mut bytes, 0x19c, 0x200);
    put32(&mut bytes, 0x1ac, 0x40000040);
    for (ordinal, (index, name)) in [(2, "Dummy"), (0, "FrameBegin"), (1, "更新😀")]
        .into_iter()
        .enumerate()
    {
        let at = 0x200 + ordinal * 32;
        let label = 0x400 + ordinal * 64;
        put32(&mut bytes, at, index);
        put64(&mut bytes, at + 8, 0x140001000 + (label - 0x200) as u64);
        for (word, value) in [u32::MAX, 0, 0x80000000, index].into_iter().enumerate() {
            put32(&mut bytes, at + 16 + word * 4, value);
        }
        for (unit, value) in name.encode_utf16().chain([0]).enumerate() {
            bytes[label + unit * 2..label + unit * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn catalog_uses_registration_order_preserves_raw_values_and_copies_names() {
    let mut bytes = fixture();
    let catalog = TaskDefinitions::read(&FileImage::parse(&bytes).unwrap(), 0x1000, 3).unwrap();
    assert_eq!(
        catalog
            .records()
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>(),
        ["FrameBegin", "更新😀", "Dummy"]
    );
    assert_eq!(catalog.find("更新😀").unwrap().registration_index, 1);
    assert_eq!(
        catalog.find("Dummy").unwrap().native_seed_words,
        [u32::MAX, 0, 0x80000000, 2]
    );
    assert!(catalog.find("FrameEnd").is_none());
    bytes.fill(0);
    assert_eq!(catalog.records()[0].name, "FrameBegin");
}

#[test]
fn bad_indices_and_duplicate_names_fail_the_whole_catalog() {
    for (at, value) in [(0x200, 0), (0x200, 3), (0x200, u32::MAX)] {
        let mut bytes = fixture();
        put32(&mut bytes, at, value);
        assert!(TaskDefinitions::read(&FileImage::parse(&bytes).unwrap(), 0x1000, 3).is_err());
    }
    let mut bytes = fixture();
    let first_pointer = u64::from_le_bytes(bytes[0x208..0x210].try_into().unwrap());
    put64(&mut bytes, 0x228, first_pointer);
    assert!(TaskDefinitions::read(&FileImage::parse(&bytes).unwrap(), 0x1000, 3).is_err());
}

#[test]
fn non_data_sections_and_unbacked_or_unbounded_tables_are_rejected() {
    for flags in [0xc0000040, 0x60000020] {
        let mut bytes = fixture();
        put32(&mut bytes, 0x1ac, flags);
        assert!(TaskDefinitions::read(&FileImage::parse(&bytes).unwrap(), 0x1000, 3).is_err());
    }
    let bytes = fixture();
    let image = FileImage::parse(&bytes).unwrap();
    for count in [0, 4097, usize::MAX] {
        assert!(TaskDefinitions::read(&image, 0x1000, count).is_err());
    }
    for table in [0, 0x1fc0, 0x2000, u32::MAX] {
        assert!(TaskDefinitions::read(&image, table, 3).is_err());
    }
}

#[test]
fn invalid_name_pointers_and_utf16_are_rejected_without_defaults() {
    for pointer in [
        0,
        0x13fffffff,
        0x140000080,
        0x140001201,
        0x140002000,
        u64::MAX,
    ] {
        let mut bytes = fixture();
        put64(&mut bytes, 0x208, pointer);
        assert!(TaskDefinitions::read(&FileImage::parse(&bytes).unwrap(), 0x1000, 3).is_err());
    }
    for first in [0u16, 0xd800] {
        let mut bytes = fixture();
        bytes[0x400..0x402].copy_from_slice(&first.to_le_bytes());
        assert!(TaskDefinitions::read(&FileImage::parse(&bytes).unwrap(), 0x1000, 3).is_err());
    }
    let mut bytes = fixture();
    for word in bytes[0x400..0x600].chunks_exact_mut(2) {
        word.copy_from_slice(&1u16.to_le_bytes());
    }
    assert!(TaskDefinitions::read(&FileImage::parse(&bytes).unwrap(), 0x1000, 3).is_err());
    let mut bytes = fixture();
    put64(&mut bytes, 0x208, 0x140001ffe);
    bytes[0x11fe..].copy_from_slice(&1u16.to_le_bytes());
    assert!(TaskDefinitions::read(&FileImage::parse(&bytes).unwrap(), 0x1000, 3).is_err());
}
