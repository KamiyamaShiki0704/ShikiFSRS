use super::*;
#[path = "action_effect_tests.rs"]
mod action_effect_tests;
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

#[test]
fn direct_roots_reject_wrong_types_and_detect_replacement() {
    use crate::roots::RootKind;
    let mut fixture = Fixture::default();
    let slot = 0x140000000 + RootKind::World.profile().0 as u64;
    fixture.word(slot, 0);
    assert_eq!(fixture.core().root(RootKind::World).unwrap(), None);
    fixture.word(slot, 0x10000);
    fixture.object(0x10000, PLAYER_TABLE);
    assert!(fixture.core().root(RootKind::World).is_err());
    fixture.object(0x10000, WORLD_TABLE);
    fixture.reads.borrow_mut().clear();
    assert_eq!(fixture.core().root(RootKind::World).unwrap(), Some(0x10000));
    fixture.reads.borrow_mut().clear();
    fixture.replacement = Some((slot, 0x20000u64.to_le_bytes().to_vec()));
    assert!(fixture.core().root(RootKind::World).is_err());
}

fn param_rows_fixture(format: u8, flags: u8) -> Fixture {
    let mut fixture = Fixture::default();
    fixture.object(0x10000, PARAMETER_CAP_TABLE);
    fixture.word(0x10080, 0x20000);
    fixture.word(0x20080, 0x30000);
    fixture.put(0x3000a, &2u16.to_le_bytes());
    fixture.put(0x3002d, &[format, flags]);
    fixture.dword(0x2fff0, 0x201);
    fixture.dword(0x2fff4, 2);
    // Native loader index aligns offset 0x201 to 0x210; ids map to opposite row ordinals.
    fixture.dword(0x30210, 3);
    fixture.dword(0x30214, 1);
    fixture.dword(0x30218, 90);
    fixture.dword(0x3021c, 0);
    let mode = format & 0x7f;
    let wide = flags & 2 != 0 && (mode == 4 || (mode == 5 && format & 0x80 != 0));
    let (start, stride, data) = if mode == 2 {
        (0x30, 12, 4)
    } else if wide {
        (0x40, 24, 8)
    } else {
        (0x40, 12, 4)
    };
    for ordinal in 0..2 {
        let address = 0x30000 + start + stride * ordinal + data;
        if wide {
            fixture.word(address, 0x400 + ordinal * 0x20);
        } else {
            fixture.dword(address, (0x400 + ordinal * 0x20) as u32);
        }
    }
    fixture
}

#[test]
fn param_runtime_index_resolves_ordinals_across_all_evidenced_formats() {
    for (format, flags) in [(2, 0), (3, 0), (4, 0), (4, 2), (5, 2), (0x85, 2)] {
        let fixture = param_rows_fixture(format, flags);
        let rows = fixture.core().parameter_rows(0x10000, 2).unwrap();
        assert_eq!(rows.row(3).unwrap().ordinal, 1);
        assert_eq!(rows.row(3).unwrap().address, 0x30420);
        assert_eq!(rows.row(90).unwrap().address, 0x30400);
        assert!(rows.row(4).is_none());
    }
}

#[test]
fn param_malformed_indices_formats_and_offsets_fail_closed() {
    let mut fixture = param_rows_fixture(4, 2);
    assert!(fixture.core().parameter_rows(0x10000, 1).is_err());
    fixture.dword(0x30214, u32::MAX);
    assert!(fixture.core().parameter_rows(0x10000, 2).is_err());
    fixture.dword(0x30214, 1);
    fixture.dword(0x30218, 3);
    assert!(fixture.core().parameter_rows(0x10000, 2).is_err());
    fixture.dword(0x30218, 90);
    fixture.word(0x30060, u64::MAX);
    assert!(fixture.core().parameter_rows(0x10000, 2).is_err());
    fixture.word(0x30060, 0x420);
    fixture.put(0x3002d, &[1]);
    assert!(fixture.core().parameter_rows(0x10000, 2).is_err());
}

#[test]
fn param_truncated_fields_and_changed_prefix_are_rejected() {
    let mut fixture = param_rows_fixture(2, 0);
    fixture.bytes.remove(&0x3021f);
    assert!(fixture.core().parameter_rows(0x10000, 2).is_err());
    fixture.dword(0x3021c, 0);
    fixture.reads.borrow_mut().clear();
    fixture.replacement = Some((0x2fff4, 1u32.to_le_bytes().to_vec()));
    assert!(fixture.core().parameter_rows(0x10000, 2).is_err());
}

fn physics_fixture() -> Fixture {
    use crate::character_modules::ModuleKind;
    let mut fixture = Fixture::default();
    fixture.object(0x10000, PLAYER_TABLE);
    fixture.word(0x101b8, 0x20000);
    fixture.word(0x20068, 0x30000);
    fixture.object(0x30000, ModuleKind::Physics.profile().1);
    fixture.word(0x30008, 0x10000);
    let values = [0.0f32, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 1.0];
    fixture.put(
        0x30060,
        &values
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    fixture
}

#[test]
fn module_owner_identity_and_optional_nulls_are_checked() {
    use crate::character_modules::ModuleKind;
    let mut fixture = physics_fixture();
    let module = fixture
        .core()
        .character_module(0x10000, ModuleKind::Physics)
        .unwrap()
        .unwrap();
    assert_eq!(module.module_address, 0x30000);
    assert_eq!(ModuleKind::BehaviorData.slot(), 24);
    fixture.word(0x30008, 0x40000);
    assert!(
        fixture
            .core()
            .character_module(0x10000, ModuleKind::Physics)
            .is_err()
    );
    fixture.word(0x30008, 0x10000);
    fixture.object(0x30000, ModuleKind::Data.profile().1);
    assert!(
        fixture
            .core()
            .character_module(0x10000, ModuleKind::Physics)
            .is_err()
    );
    fixture.word(0x20068, 0);
    assert_eq!(
        fixture
            .core()
            .character_module(0x10000, ModuleKind::Physics)
            .unwrap(),
        None
    );
    fixture.word(0x101b8, 0);
    assert_eq!(
        fixture
            .core()
            .character_module(0x10000, ModuleKind::Physics)
            .unwrap(),
        None
    );
}

#[test]
fn physics_pose_copies_known_span_and_converts_engine_space_points() {
    let fixture = physics_fixture();
    let pose = fixture.core().physics_transform(0x10000).unwrap().unwrap();
    assert_eq!(pose.rotation_xyzw, [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(pose.translation, [2.0, 3.0, 4.0, 1.0]);
    assert_eq!(
        pose.transform_point([1.0, -2.0, 3.0]).unwrap(),
        [3.0, 1.0, 7.0]
    );
    assert_eq!(
        fixture
            .reads
            .borrow()
            .iter()
            .filter(|entry| **entry == (0x30060, 32))
            .count(),
        2
    );
}

#[test]
fn physics_pose_truncation_nonfinite_values_and_replacement_fail_closed() {
    let mut fixture = physics_fixture();
    fixture.bytes.remove(&0x3007f);
    assert!(fixture.core().physics_transform(0x10000).is_err());
    fixture.put(0x3007c, &1.0f32.to_le_bytes());
    fixture.put(0x30060, &f32::NAN.to_le_bytes());
    assert!(fixture.core().physics_transform(0x10000).is_err());
    fixture.put(0x30060, &0.0f32.to_le_bytes());
    fixture.reads.borrow_mut().clear();
    fixture.replacement = Some((0x30060, vec![0; 32]));
    assert!(fixture.core().physics_transform(0x10000).is_err());
}

#[test]
fn quaternion_matrix_matches_statically_emulated_native_rotation() {
    let fixture = physics_fixture();
    let mut pose = fixture.core().physics_transform(0x10000).unwrap().unwrap();
    let half = 0.5f32.sqrt();
    pose.rotation_xyzw = [0.0, 0.0, half, half];
    pose.translation = [1.0, -2.0, 3.0, 9.0];
    let matrix = pose.rigid_matrix().unwrap();
    let expected = [
        [0.0, 1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [1.0, -2.0, 3.0, 1.0],
    ];
    for (row, wanted) in matrix.iter().zip(expected) {
        for (actual, target) in row.iter().zip(wanted) {
            assert!((actual - target).abs() < 1e-6);
        }
    }
    let point = pose.transform_point([1.0, 0.0, 0.0]).unwrap();
    assert!((point[0] - 1.0).abs() < 1e-6);
    assert!((point[1] + 1.0).abs() < 1e-6);
    pose.rotation_xyzw = [f32::INFINITY, 0.0, 0.0, 1.0];
    assert!(pose.rigid_matrix().is_err());
}

fn attribute_fixture() -> Fixture {
    use crate::character_modules::ModuleKind;
    let mut fixture = Fixture::default();
    fixture.object(0x10000, PLAYER_TABLE);
    fixture.word(0x101b8, 0x20000);
    fixture.word(0x20000, 0x30000);
    fixture.object(0x30000, ModuleKind::Data.profile().1);
    fixture.word(0x30008, 0x10000);
    let words = [100i32, 120, 130, 140, 20, 25, 30, -50, 90, 100];
    fixture.put(
        0x30140,
        &words
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    fixture.word(
        0x140000000 + crate::roots::RootKind::World.profile().0 as u64,
        0x40000,
    );
    fixture.object(0x40000, WORLD_TABLE);
    fixture.word(0x574e8, 0x10000);
    fixture
}
#[test]
fn resource_attributes_preserve_negative_stamina_and_local_identity() {
    let fixture = attribute_fixture();
    let state = fixture.core().character_state(0x10000).unwrap().unwrap();
    assert_eq!(state.attributes.health.current, 100);
    assert_eq!(state.attributes.health.effective_limit, 120);
    assert_eq!(state.attributes.focus.current, 20);
    assert_eq!(state.attributes.stamina.current, -50);
    assert_eq!(state.is_local_player, Some(true));
    assert!(!state.attributes.health_depleted());
}
#[test]
fn resource_attribute_replacement_and_unknown_world_fail_or_remain_unknown() {
    let mut fixture = attribute_fixture();
    let world = 0x140000000 + crate::roots::RootKind::World.profile().0 as u64;
    fixture.word(world, 0);
    assert_eq!(
        fixture
            .core()
            .character_state(0x10000)
            .unwrap()
            .unwrap()
            .is_local_player,
        None
    );
    fixture.reads.borrow_mut().clear();
    fixture.replacement = Some((0x30140, vec![0; 40]));
    assert!(fixture.core().character_attributes(0x10000).is_err());
}
fn schema_fixture(format: u8) -> Fixture {
    let mut fixture = param_rows_fixture(format, 2);
    fixture.put(0x30008, &9u16.to_le_bytes());
    fixture.put(0x3002c, &[0]);
    fixture.put(0x3002f, &[0]);
    let mode = format & 0x7f;
    let wide = mode == 4 || (mode == 5 && format & 0x80 != 0);
    let start = if mode == 2 { 0x30 } else { 0x40 };
    let stride = if wide { 24 } else { 12 };
    let data = if wide { 8 } else { 4 };
    if wide {
        fixture.word(0x30000 + start + stride + data, 0x730);
    } else {
        fixture.dword(0x30000 + start + stride + data, 0x730);
    }
    if format & 0x80 == 0 {
        let mut name = [0; 32];
        name[..12].copy_from_slice(b"NPC_PARAM_ST");
        fixture.put(0x3000c, &name);
    } else {
        fixture.word(0x30010, 0x900);
        fixture.put(0x30900, b"NPC_PARAM_ST\0");
    }
    fixture
}
#[test]
fn param_schema_checks_fixed_and_out_of_line_type_names_and_stride() {
    use crate::parameter_fields::npc;
    for format in [4, 0x84] {
        let fixture = schema_fixture(format);
        let rows = fixture.core().parameter_rows(0x10000, 8).unwrap();
        let schema = fixture.core().parameter_schema(&rows).unwrap();
        schema.check_definition(npc::HP.definition()).unwrap();
        assert!(schema.has_row_stride_evidence());
        assert_eq!(schema.param_type, "NPC_PARAM_ST");
        assert_eq!(schema.data_version, 9);
    }
}
#[test]
fn param_schema_rejects_version_type_endianness_and_size_mismatches() {
    use crate::parameter_fields::npc;
    let mut fixture = schema_fixture(4);
    let rows = fixture.core().parameter_rows(0x10000, 8).unwrap();
    fixture.put(0x30008, &8u16.to_le_bytes());
    assert!(
        fixture
            .core()
            .parameter_schema(&rows)
            .unwrap()
            .check_definition(npc::HP.definition())
            .is_err()
    );
    fixture.put(0x30008, &9u16.to_le_bytes());
    fixture.put(0x3000c, b"BAD_PARAM_ST");
    assert!(
        fixture
            .core()
            .parameter_schema(&rows)
            .unwrap()
            .check_definition(npc::HP.definition())
            .is_err()
    );
    fixture.put(0x3000c, b"NPC_PARAM_ST");
    fixture.put(0x3002c, &[255]);
    assert!(fixture.core().parameter_schema(&rows).is_err());
    fixture.put(0x3002c, &[0]);
    let mut wrong = rows.clone();
    wrong.rows[0].relative_offset += 1;
    assert!(
        fixture
            .core()
            .parameter_schema(&wrong)
            .unwrap()
            .check_definition(npc::HP.definition())
            .is_err()
    );
}
#[test]
fn param_single_row_metadata_is_not_promoted_to_stride_evidence() {
    let mut fixture = schema_fixture(4);
    let mut rows = fixture.core().parameter_rows(0x10000, 8).unwrap();
    rows.native_row_count = 1;
    rows.rows.truncate(1);
    fixture.put(0x3000a, &1u16.to_le_bytes());
    let schema = fixture.core().parameter_schema(&rows).unwrap();
    assert!(!schema.has_row_stride_evidence());
    schema
        .check_definition(crate::parameter_fields::npc::HP.definition())
        .unwrap();
    assert!(
        schema
            .check_definition_with_policy(
                crate::parameter_fields::npc::HP.definition(),
                crate::parameter_schema::SchemaPolicy::RequireCompleteStride
            )
            .is_err()
    );
}
