use super::*;
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Default)]
struct Fixture {
    bytes: BTreeMap<u64, u8>,
    reads: RefCell<Vec<(u64, usize)>>,
    replacement: Option<(u64, Vec<u8>)>,
}

impl Fixture {
    fn put(&mut self, address: u64, bytes: &[u8]) {
        for (offset, byte) in bytes.iter().enumerate() {
            self.bytes.insert(address + offset as u64, *byte);
        }
    }
    fn word(&mut self, address: u64, value: u64) {
        self.put(address, &value.to_le_bytes());
    }
    fn dword(&mut self, address: u64, value: u32) {
        self.put(address, &value.to_le_bytes());
    }
    fn object(&mut self, address: u64, table: u32) {
        self.word(address, 0x140000000 + table as u64);
    }
    fn core(&self) -> ReaderCore<'_, Self> {
        ReaderCore {
            memory: self,
            base: 0x140000000,
        }
    }
    fn player(&mut self, address: u64, data: u64, key: u32) {
        self.object(address, PLAYER_TABLE);
        self.word(address + 0x5a0, data);
        if data != 0 {
            self.dword(data + 8, key);
        }
    }
    fn group(&mut self, address: u64, name: &str, timeline: bool) {
        self.object(
            address,
            if timeline {
                TIMELINE_TABLE
            } else {
                GROUP_TABLE
            },
        );
        let words: Vec<u16> = name.encode_utf16().collect();
        self.word(address + 0x28, words.len() as u64);
        let storage = if words.len() < 8 {
            self.word(address + 0x30, 7);
            address + 0x18
        } else {
            self.word(address + 0x30, 31);
            self.word(address + 0x18, 0x70000);
            0x70000
        };
        let bytes: Vec<u8> = words
            .into_iter()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        self.put(storage, &bytes);
        for (offset, value) in [(0x48, 63), (0x4c, 1), (0x50, 0)] {
            self.dword(address + offset, value);
        }
        if timeline {
            self.dword(address + 0x78, 256);
        }
    }
    fn pool(&mut self, root: u64, count: u32, storage: u64) {
        self.object(root, WORLD_TABLE);
        self.object(root + 0x9a40, PLAYER_POOL_TABLE);
        self.dword(root + 0x9a50, count);
        self.word(root + 0x9a58, storage);
    }
}

impl AddressSpace for Fixture {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        let previous = self.reads.borrow().contains(&(address, output.len()));
        self.reads.borrow_mut().push((address, output.len()));
        if previous
            && let Some((changed, bytes)) = &self.replacement
            && *changed == address
            && bytes.len() == output.len()
        {
            output.copy_from_slice(bytes);
            return Ok(());
        }
        for (offset, byte) in output.iter_mut().enumerate() {
            *byte = *self
                .bytes
                .get(&at(address, offset as u64)?)
                .ok_or(Error::OutOfBounds {
                    address,
                    length: offset + 1,
                })?;
        }
        Ok(())
    }
}

#[test]
fn indexed_groups_copy_inline_and_heap_names_without_reading_base_tail() {
    for (name, timeline) in [("Dummy", false), ("ChrIns_PostPhysics", true)] {
        let mut fixture = Fixture::default();
        fixture.object(0x10000, TASK_TABLE);
        fixture.word(0x10008 + 155 * 8, 0x20000);
        fixture.group(0x20000, name, timeline);
        let group = fixture
            .core()
            .task_group(0x10000, 155, name)
            .unwrap()
            .unwrap();
        assert_eq!(group.name, name);
        assert_eq!(group.seed_words, [63, 1, 0]);
        assert_eq!(group.timeline_seed, timeline.then_some(256));
        assert_eq!(fixture.reads.borrow().contains(&(0x20078, 4)), timeline);
    }
}

#[test]
fn group_bounds_nulls_and_catalog_mismatch_fail_closed() {
    let mut fixture = Fixture::default();
    assert!(fixture.core().task_group(0x10000, 156, "Dummy").is_err());
    assert!(fixture.reads.borrow().is_empty());
    fixture.object(0x10000, TASK_TABLE);
    fixture.word(0x10008, 0);
    assert_eq!(
        fixture.core().task_group(0x10000, 0, "FrameBegin").unwrap(),
        None
    );
    fixture.word(0x10008, 0x20000);
    fixture.group(0x20000, "Dummy", false);
    assert!(fixture.core().task_group(0x10000, 0, "FrameBegin").is_err());
    fixture.word(0x20028, u64::MAX);
    assert!(fixture.core().task_group(0x10000, 0, "Dummy").is_err());
}

#[test]
fn group_truncation_invalid_utf16_and_slot_replacement_are_rejected() {
    let mut fixture = Fixture::default();
    fixture.object(0x10000, TASK_TABLE);
    fixture.word(0x10008, 0x20000);
    fixture.group(0x20000, "Dummy", false);
    fixture.bytes.remove(&0x20023); // terminator byte
    assert!(fixture.core().task_group(0x10000, 0, "Dummy").is_err());
    fixture.group(0x20000, "Dummy", false);
    fixture.put(0x20018, &0xd800u16.to_le_bytes());
    assert!(fixture.core().task_group(0x10000, 0, "Dummy").is_err());
    fixture.group(0x20000, "Dummy", false);
    fixture.replacement = Some((0x10008, 0x30000u64.to_le_bytes().to_vec()));
    assert!(fixture.core().task_group(0x10000, 0, "Dummy").is_err());
}

#[test]
fn world_slots_preserve_holes_and_ignore_second_stride_word() {
    let mut fixture = Fixture::default();
    fixture.pool(0x10000, 3, 0x30000);
    fixture.word(0x30000, 0x40000);
    fixture.word(0x30010, 0);
    fixture.word(0x30020, 0x50000);
    fixture.player(0x40000, 0x60000, 17);
    fixture.player(0x50000, 0, 0);
    let copied = fixture.core().world_players(0x10000, 3).unwrap();
    assert_eq!(copied.slots.len(), 3);
    assert_eq!(copied.slots[1], None);
    assert_eq!(copied.find_by_key(17).unwrap().address, 0x40000);
    assert!(copied.find_by_key(18).is_none());
    assert!(
        !fixture
            .reads
            .borrow()
            .iter()
            .any(|&(address, _)| address == 0x30008)
    );
}

#[test]
fn world_bounds_wrong_types_and_changed_count_are_rejected() {
    let mut fixture = Fixture::default();
    fixture.pool(0x10000, 2, 0x30000);
    assert!(fixture.core().world_players(0x10000, 1).is_err());
    assert!(
        !fixture
            .reads
            .borrow()
            .iter()
            .any(|&(address, _)| address == 0x30000)
    );
    fixture.word(0x30000, 0x40000);
    fixture.word(0x30010, 0);
    fixture.object(0x40000, ENEMY_TABLE);
    assert!(fixture.core().world_players(0x10000, 2).is_err());
    fixture.player(0x40000, 0, 0);
    fixture.reads.borrow_mut().clear();
    fixture.replacement = Some((0x19a50, 1u32.to_le_bytes().to_vec()));
    assert!(fixture.core().world_players(0x10000, 2).is_err());
}

#[test]
fn empty_world_does_not_dereference_null_storage() {
    let mut fixture = Fixture::default();
    fixture.pool(0x10000, 0, 0);
    assert!(
        fixture
            .core()
            .world_players(0x10000, 1)
            .unwrap()
            .slots
            .is_empty()
    );
    assert!(fixture.core().world_players(0x10000, 0).is_err());
    assert!(fixture.core().world_players(0x10000, 4097).is_err());
}

#[test]
fn character_types_data_truncation_and_replacement_are_checked() {
    let mut fixture = Fixture::default();
    fixture.object(0x10000, CHARACTER_TABLE);
    fixture.object(0x20000, ENEMY_TABLE);
    assert_eq!(
        fixture.core().character(0x10000).unwrap().kind,
        CharacterKind::Base
    );
    assert_eq!(
        fixture.core().character(0x20000).unwrap().kind,
        CharacterKind::Enemy
    );
    assert!(fixture.core().character(0).is_err());
    assert!(fixture.core().character(u64::MAX - 6).is_err());
    fixture.player(0x30000, 0x40000, 7);
    fixture.bytes.remove(&0x4000b);
    assert!(fixture.core().character(0x30000).is_err());
    fixture.dword(0x40008, 7);
    fixture.replacement = Some((0x305a0, 0x50000u64.to_le_bytes().to_vec()));
    assert!(fixture.core().character(0x30000).is_err());
}

#[test]
fn parameter_slots_use_72_byte_stride_and_eight_cap_bound() {
    let mut fixture = Fixture::default();
    fixture.object(0x10000, PARAMETER_TABLE);
    let offset = 252 * 72;
    fixture.dword(0x10080 + offset, 8);
    for index in 0..8 {
        let cap = 0x30000 + index * 0x100;
        fixture.word(0x10088 + offset + index * 8, cap);
        fixture.object(cap, PARAMETER_CAP_TABLE);
    }
    let definition = ParameterDefinition {
        slot: 252,
        name: "AttachEffectFilterSubCategoryParam",
    };
    let snapshot = fixture.core().parameter_caps(0x10000, definition).unwrap();
    assert_eq!(snapshot.addresses.len(), 8);
    fixture.dword(0x10080 + offset, 9);
    assert!(fixture.core().parameter_caps(0x10000, definition).is_err());
    assert!(
        fixture
            .core()
            .parameter_caps(
                0x10000,
                ParameterDefinition {
                    slot: 253,
                    name: "unknown"
                }
            )
            .is_err()
    );
}

#[test]
fn parameter_null_wrong_type_and_changed_count_are_rejected() {
    let mut fixture = Fixture::default();
    fixture.object(0x10000, PARAMETER_TABLE);
    fixture.dword(0x10080, 1);
    fixture.word(0x10088, 0);
    let definition = ParameterDefinition {
        slot: 0,
        name: "EquipParamWeapon",
    };
    assert!(fixture.core().parameter_caps(0x10000, definition).is_err());
    fixture.word(0x10088, 0x30000);
    fixture.object(0x30000, PLAYER_TABLE);
    assert!(fixture.core().parameter_caps(0x10000, definition).is_err());
    fixture.object(0x30000, PARAMETER_CAP_TABLE);
    fixture.reads.borrow_mut().clear();
    fixture.replacement = Some((0x10080, 0u32.to_le_bytes().to_vec()));
    assert!(fixture.core().parameter_caps(0x10000, definition).is_err());
}

#[test]
fn address_overflow_is_reported_before_memory_access() {
    let fixture = Fixture::default();
    assert_eq!(
        fixture.core().u64(u64::MAX - 7, 8),
        Err(Error::AddressOverflow)
    );
    assert!(fixture.reads.borrow().is_empty());
}
