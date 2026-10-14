//! Optional end-to-end offline test. The executable is only read as an immutable file.
use shikifsrs_nightreign::{NightreignFile, reader::CharacterKind};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, pe::FileImage};
use std::cell::RefCell;
use std::collections::BTreeMap;
#[path = "fixtures/effect_removal.rs"]
mod effect_removal_fixture;
use effect_removal_fixture::check_effect_removal_receipts;
#[path = "fixtures/effect_addition.rs"]
mod effect_addition_fixture;
use effect_addition_fixture::check_effect_addition_receipts;
#[path = "fixtures/effect_tiers.rs"]
mod effect_tier_fixture;
use effect_tier_fixture::check_effect_tier_plans;
#[path = "fixtures/goal_requests.rs"]
mod goal_request_fixture;
use goal_request_fixture::check_goal_request_receipts;

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
        if (0..output.len()).all(|offset| self.objects.contains_key(&(address + offset as u64))) {
            for (offset, byte) in output.iter_mut().enumerate() {
                *byte = self.objects[&(address + offset as u64)];
            }
            return Ok(());
        }
        if let Some(rva) = address
            .checked_sub(self.base)
            .and_then(|rva| u32::try_from(rva).ok())
            && rva < self.image.image_size
        {
            let bytes = self.image.read_rva(rva, output.len())?;
            output.copy_from_slice(bytes);
            if output.len() == 8
                && [
                    0x2cdf868,
                    0x2ce3fe8,
                    0x2ce4038,
                    0x2c1b930,
                    0x2c15d70,
                    0x2c307f8,
                    0x2c18120,
                    0x2c0ba90,
                    0x2cb1e08,
                    0x2cade48,
                    0x2bda590,
                    0x2c0ba90 + 0x510,
                    0x2c18120 + 0x510,
                    0x2c307f8 + 0x510,
                    0x2c03e48 + 0x10,
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

struct OperationFixture<'a> {
    inner: &'a MappedFixture<'a>,
    writes: RefCell<BTreeMap<u64, u8>>,
    calls: RefCell<Vec<u64>>,
}
impl AddressSpace for OperationFixture<'_> {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        let writes = self.writes.borrow();
        if (0..output.len()).all(|offset| writes.contains_key(&(address + offset as u64))) {
            for (offset, byte) in output.iter_mut().enumerate() {
                *byte = writes[&(address + offset as u64)];
            }
            return Ok(());
        }
        self.inner.read_exact(address, output)?;
        for (offset, byte) in output.iter_mut().enumerate() {
            if let Some(value) = writes.get(&(address + offset as u64)) {
                *byte = *value;
            }
        }
        Ok(())
    }
}
// SAFETY: This backend changes owned test bytes only, synchronously. Invocation routes,
// native limit signatures and data module addresses are verified by the SDK before entry.
unsafe impl shikifsrs_nightreign::character_operations::CharacterOperationBackend
    for OperationFixture<'_>
{
    unsafe fn invoke_limit(&self, entry: u64, module: u64, value: i32) -> Result<()> {
        assert_eq!(module, 0x170000);
        self.calls.borrow_mut().push(entry);
        let (current, limit, floor) = match entry - self.inner.base {
            0x427d40 => (0x140, 0x144, 0),
            0x427db0 => (0x150, 0x154, 0),
            0x427e00 => (0x15c, 0x160, -100),
            _ => panic!("unexpected fixture operation"),
        };
        let prior = self.read_u32(module + current)? as i32;
        let updated = prior.clamp(floor, value);
        let mut writes = self.writes.borrow_mut();
        for (offset, word) in [(limit, value), (current, updated)] {
            for (byte, value) in word.to_le_bytes().into_iter().enumerate() {
                writes.insert(module + offset + byte as u64, value);
            }
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
    // Exact direct roots are loader-relocated synthetic slots, not native singleton calls.
    for (rva, address) in [
        (0x4434088, 0x10000),
        (0x3c0f0a8, 0x30000),
        (0x3c28880, 0x70000),
        (0x4707290, 0x90000),
    ] {
        fixture.word(fixture.base + rva, address);
    }
    fixture.dword(0x20054, 0xab00009b);
    fixture.object(0x90000, 0x32e8478);
    fixture.word(0x90008, 0xa0000);
    fixture.put(0x90038, &[0]);
    fixture.word(0x90040, 0xb0000);
    fixture.word(0x90048, 0xc0000);
    fixture.word(0xc0018, 0xd0000);
    fixture.word(0xd0008, 0xe0000);
    fixture.word(0xd0010, 1);
    fixture.put(0xe0019, &[1]);
    fixture.word(0xe0008, 0xe1000);
    fixture.word(0xe1000, 0xe0000);
    fixture.word(0xe1008, 0xe0000);
    fixture.word(0xe1010, 0xe0000);
    fixture.put(0xe1019, &[0]);
    fixture.dword(0xe1020, 0xab00009b);
    fixture.word(0xe1028, 0xf0000);
    fixture.word(0xf0010, 0x110000);
    fixture.word(0xf0018, 0x110020);
    fixture.word(0x110008, 0x120000);
    fixture.word(0x80080, 0x130000);
    fixture.word(0x130080, 0x140000);
    fixture.put(0x14000a, &1u16.to_le_bytes());
    fixture.put(0x14002d, &[4, 2]);
    fixture.dword(0x13fff0, 0x101);
    fixture.dword(0x13fff4, 1);
    fixture.dword(0x140110, 99);
    fixture.dword(0x140114, 0);
    fixture.word(0x140048, 0x200);
    fixture.put(0x140200, &[1, 2, 3, 4]);
    fixture.word(0x501b8, 0x150000);
    fixture.word(0x150068, 0x160000);
    fixture.object(0x160000, 0x2c13720);
    fixture.word(0x160008, 0x50000);
    let pose = [0.0f32, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 1.0];
    fixture.put(
        0x160060,
        &pose
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    fixture.dword(0x140224, 1234);
    fixture.put(0x140349, &[0x10]);
    fixture.put(0x140008, &9u16.to_le_bytes());
    fixture.put(0x14002c, &[0]);
    fixture.put(0x14002f, &[0]);
    let mut type_name = [0; 32];
    type_name[..12].copy_from_slice(b"NPC_PARAM_ST");
    fixture.put(0x14000c, &type_name);
    fixture.word(0x150000, 0x170000);
    fixture.object(0x170000, 0x2c10b88);
    fixture.word(0x170008, 0x50000);
    let counters = [100i32, 120, 130, 140, 40, 50, 60, -50, 90, 100];
    fixture.put(
        0x170140,
        &counters
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    fixture.word(0x474e8, 0x50000);
    fixture.word(0x150080, 0x180000);
    fixture.object(0x180000, 0x2c0c808);
    fixture.word(0x180008, 0x50000);
    fixture.put(0x180010, &[0; 0x149]);
    fixture.word(0x180050, 0x180000);
    fixture.word(0x180020, 1u64 << 7);
    fixture.word(0x50190, 0x190000);
    fixture.object(0x190000, 0x2c1b5d8);
    fixture.word(0x190008, 0x1a0000);
    fixture.word(0x190010, 0x50000);
    fixture.word(0x190028, u64::MAX);
    fixture.put(0x1a0000, &[0; 0x6c]);
    fixture.word(0x1a0000, 0x1b0000);
    fixture.dword(0x1a0008, 123);
    fixture.put(0x1b0156, &449u16.to_le_bytes());
    fixture.put(0x1b013e, &0i16.to_le_bytes());
    fixture.put(0x1b031c, &1i16.to_le_bytes());
    fixture.dword(0x70000 + 15 * 72 + 0x80, 1);
    fixture.word(0x70000 + 15 * 72 + 0x88, 0x1e0000);
    fixture.object(0x1e0000, 0x2cade48);
    fixture.word(0x1e0080, 0x1f0000);
    fixture.word(0x1f0080, 0x200000);
    fixture.put(0x200000, &[0; 0x58]);
    fixture.put(0x200008, &4u16.to_le_bytes());
    fixture.put(0x20000a, &1u16.to_le_bytes());
    let mut effect_type = [0; 32];
    effect_type[..18].copy_from_slice(b"SP_EFFECT_PARAM_ST");
    fixture.put(0x20000c, &effect_type);
    fixture.put(0x20002d, &[4, 2]);
    fixture.dword(0x1ffff0, 0x101);
    fixture.dword(0x1ffff4, 1);
    fixture.dword(0x200110, 123);
    fixture.dword(0x200114, 0);
    fixture.word(0x200048, 0x1000);
    fixture.put(0x201000, &[0; 976]);
    fixture.put(0x201156, &449u16.to_le_bytes());
    fixture.word(0x300000, 0x90000);
    fixture.object(0x9c958, 0x2bf9180);
    fixture.word(0x9c960, 0x350000);
    fixture.word(0x350000, 0x50000);
    fixture.word(0x50018, 0x350000);
    fixture.object(0x300018, 0x2c025f0);
    fixture.word(0x300020, 0x300000);
    fixture.object(0x300300, 0x2bf9cf0);
    fixture.word(0x300308, 0);
    fixture.put(0x300310, &[0; 13]);
    fixture.dword(0x300314, u32::MAX);
    fixture.dword(0x300318, 42);
    fixture.put(0x30031c, &[1]);
    fixture.word(0x3000a8, 0x310000);
    fixture.object(0x310000, 0x2c03e48);
    fixture.dword(0x300060, 42.75f32.to_bits());
    fixture.word(0x90008, 0x320000);
    fixture.object(0x320000, 0x2c016b0);
    fixture.word(0x320008, 0x90000);
    fixture.word(0x9db70, 0x90000);
    fixture.word(0x9db78, 0xa0000);
    fixture.word(0x9db80, 0x330000);
    fixture.word(0x9db88, 1);
    fixture.word(0x330000, 0x340000);
    fixture.word(0x330008, 0x340000);
    fixture.put(0x340000, &[0; 40]);
    fixture.word(0x340000, 0x330000);
    fixture.word(0x340008, 0x330000);
    fixture.word(0x340010, 0x90000);
    fixture.dword(0x340018, 123);
    fixture.dword(0x34001c, 42);
    fixture.put(0x340020, &[1]);
    fixture.dword(0x340024, 2);
    let reader = file.bind_reader(&fixture, fixture.base).unwrap();
    use shikifsrs_nightreign::{character_modules::ModuleKind, parameter_fields};
    let state = reader.character_state(0x50000).unwrap().unwrap();
    let goal_state = reader.goal_action_state(0x300000).unwrap();
    assert_eq!(goal_state.raw_id_bits, 42);
    assert_eq!(goal_state.raw_flag, 1);
    assert_eq!(goal_state.raw_word_14, -1);
    check_goal_request_receipts(&file, &fixture);
    let queue = reader.think_action_queue(0x300000, 8).unwrap().unwrap();
    assert_eq!(queue.interface_address, 0x320000);
    assert_eq!(queue.events[0].raw_key_b, 42);
    assert_eq!(queue.events[0].raw_tag, 2);
    use shikifsrs_nightreign::think_actions::{ThinkPredicateEdge, ThinkPredicateTransition};
    assert_eq!(
        queue.events[0].predicate_transition(),
        ThinkPredicateTransition::BecameFalse
    );
    assert!(queue.has_transition_for_key_b(ThinkPredicateEdge::BecameFalse, 42));
    assert_eq!(
        queue.first_transition_key_b(ThinkPredicateEdge::BecameTrue),
        None
    );
    assert_eq!(queue.predicate_event_mask(), 1u64 << 44);
    let self_target = reader.think_self_target(0x300000).unwrap().unwrap();
    assert_eq!(self_target.character_address, Some(0x50000));
    assert!(
        reader
            .think_self_effect_predicate(0x300000, 123, 8)
            .unwrap()
            .unwrap()
            .present
    );
    assert!(
        !reader
            .think_self_effect_predicate(0x300000, -1, 8)
            .unwrap()
            .unwrap()
            .present
    );
    assert_eq!(state.is_local_player, Some(true));
    assert_eq!(state.attributes.health.current, 100);
    assert_eq!(state.attributes.stamina.current, -50);
    let actions = reader.action_requests(0x50000, 8).unwrap().unwrap();
    assert!(actions.primary_requested(7).unwrap());
    let effects = reader.special_effects(0x50000, 8).unwrap().unwrap();
    assert!(effects.contains_id(123));
    assert!(effects.contains_state(449));
    check_effect_removal_receipts(&file, &fixture);
    check_effect_addition_receipts(&file, &fixture);
    check_effect_tier_plans(&file, &fixture);
    let schema = reader.parameter_schema(0x70000, "NpcParam", 0, 8).unwrap();
    assert_eq!(schema.data_version, 9);
    assert!(!schema.has_row_stride_evidence());
    assert!(
        reader
            .parameter_field_with_policy(
                0x70000,
                0,
                99,
                parameter_fields::npc::HP,
                8,
                shikifsrs_nightreign::parameter_schema::SchemaPolicy::RequireCompleteStride
            )
            .is_err()
    );
    assert_eq!(
        reader
            .character_module(0x50000, ModuleKind::Physics)
            .unwrap()
            .unwrap()
            .module_address,
        0x160000
    );
    assert_eq!(
        reader
            .physics_transform(0x50000)
            .unwrap()
            .unwrap()
            .transform_point([1.0, 0.0, 0.0])
            .unwrap(),
        [3.0, 3.0, 4.0]
    );
    assert_eq!(
        reader
            .parameter_field(0x70000, 0, 99, parameter_fields::npc::HP, 8)
            .unwrap(),
        Some(1234)
    );
    assert_eq!(
        reader
            .parameter_field(0x70000, 0, 99, parameter_fields::npc::IS_GHOST, 8)
            .unwrap(),
        Some(true)
    );
    assert_eq!(
        reader
            .parameter_field(0x70000, 0, 100, parameter_fields::npc::HP, 8)
            .unwrap(),
        None
    );
    use shikifsrs_nightreign::roots::RootKind;
    assert_eq!(reader.root(RootKind::TaskGroups).unwrap(), Some(0x10000));
    assert_eq!(reader.root(RootKind::World).unwrap(), Some(0x30000));
    assert_eq!(reader.root(RootKind::Parameters).unwrap(), Some(0x70000));
    assert_eq!(reader.group_id(155).unwrap(), Some(0xab00009b));
    assert!(
        reader
            .registration_inserted(0xab00009b, 0x120000, 8)
            .unwrap()
    );
    assert!(
        !reader
            .registration_inserted(0xab00009b, 0x130000, 8)
            .unwrap()
    );
    assert_eq!(
        reader
            .current_parameter_rows("NpcParam", 0, 8)
            .unwrap()
            .unwrap()
            .rows[0]
            .id,
        99
    );
    assert_eq!(
        reader
            .copy_parameter_row(0x70000, "NpcParam", 0, 99, 4, 8)
            .unwrap()
            .unwrap()
            .bytes,
        [1, 2, 3, 4]
    );
    assert!(
        reader
            .copy_parameter_row(0x70000, "NpcParam", 0, 100, 4, 8)
            .unwrap()
            .is_none()
    );
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
    let operations_fixture = OperationFixture {
        inner: &fixture,
        writes: RefCell::new(BTreeMap::new()),
        calls: RefCell::new(Vec::new()),
    };
    let operations = file
        .bind_character_operations(&operations_fixture, fixture.base)
        .unwrap();
    use shikifsrs_nightreign::character_operations::ResourceLimit;
    // SAFETY: Owned-memory fixture, exclusive sequential mutation, no actual game code invoked.
    let changed = unsafe {
        operations
            .set_effective_limit(0x50000, ResourceLimit::Health, 80)
            .unwrap()
    };
    assert_eq!(changed.before.health.current, 100);
    assert_eq!(changed.after.health.current, 80);
    assert_eq!(changed.after.health.effective_limit, 80);
    // SAFETY: Same serialized fixture operation and original module ownership.
    let changed = unsafe {
        operations
            .set_effective_limit(0x50000, ResourceLimit::Focus, 30)
            .unwrap()
    };
    assert_eq!(changed.after.focus.current, 30);
    // SAFETY: Same fixture phase; negative current stamina remains valid above native floor.
    let changed = unsafe {
        operations
            .set_effective_limit(0x50000, ResourceLimit::Stamina, 70)
            .unwrap()
    };
    assert_eq!(changed.after.stamina.current, -50);
    assert_eq!(changed.after.stamina.effective_limit, 70);
    // SAFETY: Invalid limit is rejected before invoking the owned-memory backend.
    assert!(unsafe { operations.set_effective_limit(0x50000, ResourceLimit::Health, 0) }.is_err());
    assert_eq!(operations_fixture.calls.borrow().len(), 3);
    let mut wrong_schema = MappedFixture {
        image: fixture.image,
        base: fixture.base,
        objects: fixture.objects.clone(),
        changed_byte: None,
    };
    wrong_schema.put(0x140008, &8u16.to_le_bytes());
    let wrong_reader = file.bind_reader(&wrong_schema, fixture.base).unwrap();
    assert!(
        wrong_reader
            .parameter_field(0x70000, 0, 99, parameter_fields::npc::HP, 8)
            .is_err()
    );
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
