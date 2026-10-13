use shikifsrs_runtime::{Error, pe::FileImage};

const NAME: &str = ".?AVSynthetic@@";

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

// An original synthetic PE. No game or upstream library bytes are included.
fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x800];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
    bytes[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
    bytes[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes());
    bytes[0x98..0x9a].copy_from_slice(&0x20bu16.to_le_bytes());
    put64(&mut bytes, 0xb0, 0x140000000);
    put32(&mut bytes, 0xd0, 0x3000);
    put32(&mut bytes, 0xd4, 0x200);
    for (table, name, rva, raw, size, flags) in [
        (0x188, b".text\0\0\0", 0x1000, 0x200, 0x200, 0x60000020),
        (0x1b0, b".rdata\0\0", 0x2000, 0x400, 0x400, 0x40000040),
    ] {
        bytes[table..table + 8].copy_from_slice(name);
        put32(&mut bytes, table + 8, size);
        put32(&mut bytes, table + 12, rva);
        put32(&mut bytes, table + 16, size);
        put32(&mut bytes, table + 20, raw);
        put32(&mut bytes, table + 36, flags);
    }
    bytes[0x200] = 0xc3;
    bytes[0x430..0x430 + NAME.len()].copy_from_slice(NAME.as_bytes());
    for (index, word) in [1, 0, 0, 0x2020, 0x2180, 0x2100].iter().enumerate() {
        put32(&mut bytes, 0x500 + index * 4, *word);
    }
    put64(&mut bytes, 0x600, 0x140002100);
    put64(&mut bytes, 0x608, 0x140001000);
    bytes
}

#[test]
fn identifies_exact_type_locator_and_executable_vtable_entry() {
    let bytes = fixture();
    let image = FileImage::parse(&bytes).unwrap();
    assert_eq!(image.type_descriptor(NAME).unwrap(), 0x2020);
    let tables = image.vtables(NAME).unwrap();
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].locator_rva, 0x2100);
    assert_eq!(tables[0].table_rva, 0x2208);
    assert_eq!(tables[0].entries, vec![0x1000]);
}

#[test]
fn rejects_substrings_unterminated_names_and_invalid_queries() {
    let bytes = fixture();
    let image = FileImage::parse(&bytes).unwrap();
    assert!(matches!(
        image.type_descriptor(".?AVSynthetic"),
        Err(Error::TypeNotFound(_))
    ));
    for query in ["", "a\0b"] {
        assert!(matches!(
            image.type_descriptor(query),
            Err(Error::InvalidPe(_))
        ));
    }
    let mut bytes = fixture();
    bytes[0x430 + NAME.len()] = b'X';
    assert!(matches!(
        FileImage::parse(&bytes).unwrap().type_descriptor(NAME),
        Err(Error::TypeNotFound(_))
    ));
}

#[test]
fn rejects_duplicate_names_instead_of_selecting_first_descriptor() {
    let mut bytes = fixture();
    bytes[0x470..0x470 + NAME.len()].copy_from_slice(NAME.as_bytes());
    assert_eq!(
        FileImage::parse(&bytes).unwrap().vtables(NAME),
        Err(Error::AmbiguousType(NAME.into()))
    );
}

#[test]
fn malformed_locators_cannot_authorize_a_table() {
    for (at, value) in [
        (0x500, 0),
        (0x504, 0x3000),
        (0x510, 0xdead),
        (0x514, 0x2104),
    ] {
        let mut bytes = fixture();
        put32(&mut bytes, at, value);
        assert!(
            FileImage::parse(&bytes)
                .unwrap()
                .vtables(NAME)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn data_and_outside_image_targets_are_not_function_entries() {
    for address in [0x140002020, 0x13fffffff, 0x140003000, u64::MAX] {
        let mut bytes = fixture();
        put64(&mut bytes, 0x608, address);
        assert!(
            FileImage::parse(&bytes)
                .unwrap()
                .vtables(NAME)
                .unwrap()
                .is_empty()
        );
    }
}
