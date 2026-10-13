//! Optional end-to-end offline test. The executable is only read as an immutable file.
use shikifsrs_nightreign::{NightreignFile, reader::CharacterKind};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, pe::FileImage};
use std::collections::BTreeMap;

struct MappedFixture<'a> {
    image: &'a FileImage<'a>,
    base: u64,
    objects: BTreeMap<u64, u8>,
    changed_byte: Option<(u32, u8)>,
}

impl MappedFixture<'_> {
    fn put(&mut self, address: u64, bytes: &[u8]) {
        for (offset, byte) in bytes.iter().enumerate() {
            self.objects.insert(address + offset as u64, *byte);
        }
    }
    fn word(&mut self, address: u64, value: u64) {
        self.put(address, &value.to_le_bytes());
    }
    fn dword(&mut self, address: u64, value: u32) {
        self.put(address, &value.to_le_bytes());
    }
    fn object(&mut self, address: u64, table: u32) {
        self.word(address, self.base + table as u64);
    }
}

impl AddressSpace for MappedFixture<'_> {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        if let Some(rva) = address
            .checked_sub(self.base)
            .and_then(|rva| u32::try_from(rva).ok())
            && rva < self.image.image_size
        {
            let bytes = self.image.read_rva(rva, output.len())?;
            output.copy_from_slice(bytes);
            if output.len() == 8
                && [
                    0x2cdf868, 0x2ce3fe8, 0x2ce4038, 0x2c1b930, 0x2c15d70, 0x2c307f8, 0x2c18120,
                    0x2c0ba90, 0x2cb1e08, 0x2cade48,
                ]
                .contains(&rva)
            {
                let preferred = u64::from_le_bytes(bytes.try_into().unwrap());
                output.copy_from_slice(
                    &(preferred - self.image.preferred_base + self.base).to_le_bytes(),
                );
            }
            if let Some((changed_rva, value)) = self.changed_byte
                && let Some(offset) = changed_rva.checked_sub(rva)
                && let Some(byte) = output.get_mut(offset as usize)
            {
                *byte = value;
            }
            return Ok(());
        }
        for (offset, byte) in output.iter_mut().enumerate() {
            *byte = *self
                .objects
                .get(&(address + offset as u64))
                .ok_or(Error::OutOfBounds {
                    address,
                    length: offset + 1,
                })?;
        }
        Ok(())
    }
}

#[test]
#[ignore = "set SHIKIFSRS_NR_EXE to the exact local NR executable; no game is executed"]
fn exact_file_catalog_and_relocated_fixture_cover_all_four_read_interfaces() {
    let path = std::env::var_os("SHIKIFSRS_NR_EXE").expect("SHIKIFSRS_NR_EXE must be set");
    let bytes = std::fs::read(path).unwrap();
    let file = NightreignFile::verify(&bytes).unwrap();
    assert_eq!(file.parameter_definitions().len(), 253);
    assert_eq!(file.parameter_definitions()[6].name, "NpcParam");
    assert_eq!(file.parameter_definitions()[69].name, "PartsDrawParam");
    let mut fixture = MappedFixture {
        image: file.file().image(),
        base: 0x240000000,
        objects: BTreeMap::new(),
        changed_byte: None,
    };
    // All engine-like addresses below are owned synthetic data, never process reads.
    fixture.object(0x10000, 0x2cdf868);
    for index in 0..156 {
        fixture.word(0x10008 + index * 8, 0);
    }
    fixture.word(0x10008 + 155 * 8, 0x20000);
    fixture.object(0x20000, 0x2ce3fe8);
    let name: Vec<u8> = "Dummy"
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    fixture.put(0x20018, &name);
    fixture.word(0x20028, 5);
    fixture.word(0x20030, 7);
    for (offset, value) in [(0x48, 63), (0x4c, 0), (0x50, 0)] {
        fixture.dword(0x20000 + offset, value);
    }
    fixture.object(0x30000, 0x2c1b930);
    fixture.object(0x39a40, 0x2c15d70);
    fixture.dword(0x39a50, 1);
    fixture.word(0x39a58, 0x40000);
    fixture.word(0x40000, 0x50000);
    fixture.object(0x50000, 0x2c307f8);
    fixture.word(0x505a0, 0x60000);
    fixture.dword(0x60008, 29);
    fixture.object(0x70000, 0x2cb1e08);
    fixture.dword(0x70080 + 6 * 72, 1);
    fixture.word(0x70088 + 6 * 72, 0x80000);
    fixture.object(0x80000, 0x2cade48);
    let reader = file.bind_reader(&fixture, fixture.base).unwrap();
    let groups = reader.task_groups(0x10000).unwrap();
    assert_eq!(groups.len(), 156);
    assert_eq!(groups[155].as_ref().unwrap().name, "Dummy");
    let world = reader.world_players(0x30000, 16).unwrap();
    assert_eq!(world.find_by_key(29).unwrap().address, 0x50000);
    assert_eq!(
        reader.character(0x50000).unwrap().kind,
        CharacterKind::Player
    );
    assert_eq!(
        reader
            .parameter_caps(0x70000, "NpcParam")
            .unwrap()
            .addresses,
        [0x80000]
    );
    assert!(reader.parameter_caps(0x70000, "does-not-exist").is_err());
    let original = file.file().image().read_rva(0xfec930, 1).unwrap()[0];
    fixture.changed_byte = Some((0xfec930, original ^ 1));
    assert!(matches!(
        file.bind_reader(&fixture, fixture.base),
        Err(Error::ImageMismatch { rva: 0xfec930 })
    ));
    fixture.changed_byte = Some((0x2c307f8, 0));
    assert!(matches!(
        file.bind_reader(&fixture, fixture.base),
        Err(Error::ImageMismatch { rva: 0x2c307f8 })
    ));
}
