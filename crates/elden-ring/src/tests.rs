use super::*;
use crate::character_modules::CharacterModule;
use crate::objects::{GameObjectCatalog, ObjectKind};
use sha2::{Digest, Sha256};
use std::{cell::Cell, collections::BTreeMap};

#[test]
fn initializer_override_preserves_signed_creation_ids_and_reads_no_neighbor_words() {
    let (bytes, mut memory) = fixture();
    let target = vitals_fixture(&mut memory);
    let file = verified(&bytes);
    for id in [-1i32, 0, 1000, i32::MIN, i32::MAX] {
        memory.put(0xc010c4, &id.to_le_bytes());
        let copy = object_catalog(&file, &memory)
            .character_initializer_override(target)
            .unwrap()
            .unwrap();
        assert_eq!(copy.stored_id, id);
        assert_eq!(copy.character, target);
        assert_eq!(copy.data.address, 0xc01000);
    }
    for missing in 0..4 {
        let (_, mut memory) = fixture();
        let target = vitals_fixture(&mut memory);
        memory.put(0xc010c4, &1000i32.to_le_bytes());
        memory.bytes.remove(&(0xc010c4 + missing));
        assert!(
            object_catalog(&file, &memory)
                .character_initializer_override(target)
                .is_err()
        );
    }
    memory.put64(0xc00000, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .character_initializer_override(target)
            .unwrap(),
        None
    );
    assert!(
        object_catalog(&file, &memory)
            .character_initializer_override(character_sets::CharacterIdentity {
                epoch: 0,
                ..target
            })
            .is_err()
    );
}

#[test]
fn initializer_override_rejects_visible_owner_module_identity_and_word_changes() {
    let (bytes, _) = fixture();
    let file = verified(&bytes);
    for watched in [
        0xc010c4,
        0xc01000,
        0xc01008,
        0xc00000,
        0x610190,
        0x610008,
        0x610000,
        BASE + 0x3d69ff8,
    ] {
        let (_, mut memory) = fixture();
        let target = vitals_fixture(&mut memory);
        memory.put(0xc010c4, &1000i32.to_le_bytes());
        memory.watch_address = watched;
        memory.change_slot = true;
        assert!(
            object_catalog(&file, &memory)
                .character_initializer_override(target)
                .is_err(),
            "{watched:x}"
        );
    }
    let (_, mut memory) = fixture();
    let target = vitals_fixture(&mut memory);
    memory.put(0xc010c4, &1000i32.to_le_bytes());
    assert!(
        object_catalog(&file, &memory)
            .character_initializer_override(character_sets::CharacterIdentity {
                native_identifier: target.native_identifier ^ 1,
                ..target
            })
            .is_err()
    );
    memory.put64(0xc01008, target.address + 0x1000);
    assert!(
        object_catalog(&file, &memory)
            .character_initializer_override(target)
            .is_err()
    );
}

#[test]
fn reciprocal_entry_state_preserves_raw_bytes_without_requiring_a_collection_scan() {
    let (bytes, mut memory) = fixture();
    let (set, target) = registration_fixture(&mut memory);
    let file = verified(&bytes);
    // Remove set headers and every unselected entry byte: this API proves only
    // a reciprocal link, and cannot claim membership or an ordinal.
    for address in set..set + 0x20 {
        memory.bytes.remove(&address);
    }
    for address in 0xd0002b..0xd00030 {
        memory.bytes.remove(&address);
    }
    for raw in [[0, 0, 0], [4, 0x81, 0xff], [255, 0x7f, 0x80]] {
        memory.put(0xd00028, &raw);
        let copy = object_catalog(&file, &memory)
            .character_entry_state(target)
            .unwrap()
            .unwrap();
        assert_eq!(copy.character, target);
        assert_eq!(copy.entry_address, 0xd00020);
        assert_eq!(copy.native_state_bytes, raw);
    }
    memory.put64(target.address + 0x10, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .character_entry_state(target)
            .unwrap(),
        None
    );
    assert!(
        object_catalog(&file, &memory)
            .character_entry_state(character_sets::CharacterIdentity { epoch: 0, ..target })
            .is_err()
    );
}

#[test]
fn reciprocal_entry_state_rejects_truncation_and_changes_across_its_selected_graph() {
    let (bytes, _) = fixture();
    let file = verified(&bytes);
    struct ChangedByte<'a> {
        memory: &'a SparseMemory,
        watched: u64,
        reads: Cell<usize>,
    }
    impl AddressSpace for ChangedByte<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if let Some(index) = self.watched.checked_sub(address)
                && index < output.len() as u64
            {
                let previous = self.reads.replace(self.reads.get() + 1);
                if previous > 0 {
                    output[index as usize] ^= 1;
                }
            }
            Ok(())
        }
    }
    for missing in 0..3 {
        let (_, mut memory) = fixture();
        let (_, target) = registration_fixture(&mut memory);
        memory.put(0xd00028, &[4, 9, 0xe7]);
        memory.bytes.remove(&(0xd00028 + missing));
        assert!(
            object_catalog(&file, &memory)
                .character_entry_state(target)
                .is_err()
        );
    }
    for watched in [
        0xd00020,
        0xd00028,
        0xd00029,
        0xd0002a,
        0x620010,
        0x620008,
        0x620000,
        BASE + 0x3d69ff8,
    ] {
        let (_, mut memory) = fixture();
        let (_, target) = registration_fixture(&mut memory);
        memory.put(0xd00028, &[4, 9, 0xe7]);
        let changed = ChangedByte {
            memory: &memory,
            watched,
            reads: Cell::new(0),
        };
        let objects = GameObjectCatalog {
            module: file
                .bind(
                    &changed,
                    BASE,
                    &[CodeRange {
                        rva: 0x1000,
                        length: 1,
                    }],
                )
                .unwrap(),
        };
        assert!(
            objects.character_entry_state(target).is_err(),
            "{watched:x}"
        );
    }
    for invalid in [1, 0x610000, 0x630000] {
        let (_, mut memory) = fixture();
        let (_, target) = registration_fixture(&mut memory);
        memory.put64(target.address + 0x10, invalid);
        assert!(
            object_catalog(&file, &memory)
                .character_entry_state(target)
                .is_err()
        );
    }
    let (_, mut memory) = fixture();
    let (_, target) = registration_fixture(&mut memory);
    memory.put(0xd00028, &[4, 9, 0xe7]);
    for stale in [
        character_sets::CharacterIdentity {
            world_address: target.world_address + 8,
            ..target
        },
        character_sets::CharacterIdentity {
            native_identifier: target.native_identifier ^ (1 << 63),
            ..target
        },
    ] {
        assert!(
            object_catalog(&file, &memory)
                .character_entry_state(stale)
                .is_err()
        );
    }
}

#[test]
fn loading_copy_preserves_request_latched_prior_and_full_identity_rejects_selected_truncation() {
    let (bytes, mut memory) = fixture();
    let (_, target) = registration_fixture(&mut memory);
    let file = verified(&bytes);
    for (offset, bits) in [
        (0x1b0, 0xdeadbeefu32),
        (0x1b4, 0xfedcba98),
        (0x1b8, 0x12345678),
    ] {
        memory.put(target.address + offset, &bits.to_le_bytes());
    }
    let actual = object_catalog(&file, &memory)
        .character_load_requests(target)
        .unwrap()
        .unwrap();
    assert_eq!(actual.character, target);
    assert_eq!(
        (actual.request_bits, actual.latched_bits, actual.prior_bits),
        (0xdeadbeef, 0xfedcba98, 0x12345678)
    );
    for offset in [0x1b0, 0x1b4, 0x1b8] {
        for byte in 0..4 {
            let address = target.address + offset + byte;
            let saved = memory.bytes.remove(&address).unwrap();
            assert!(
                object_catalog(&file, &memory)
                    .character_load_requests(target)
                    .is_err()
            );
            memory.put(address, &[saved]);
        }
    }
    assert!(
        object_catalog(&file, &memory)
            .character_load_requests(character_sets::CharacterIdentity {
                native_identifier: target.native_identifier ^ (1 << 48),
                ..target
            })
            .unwrap()
            .is_none()
    );
    assert!(
        object_catalog(&file, &memory)
            .character_load_requests(character_sets::CharacterIdentity { epoch: 0, ..target })
            .is_err()
    );
}
#[test]
fn loading_copy_rejects_changes_in_every_selected_word_and_identity_path() {
    let (bytes, mut memory) = fixture();
    let (_, target) = registration_fixture(&mut memory);
    let file = verified(&bytes);
    for offset in [0x1b0, 0x1b4, 0x1b8] {
        memory.put(target.address + offset, &0x12345678u32.to_le_bytes());
    }
    for watch in [
        target.address + 0x1b0,
        target.address + 0x1b4,
        target.address + 0x1b8,
        target.address,
        target.address + 8,
        BASE + 0x3d69ff8,
    ] {
        memory.watch_address = watch;
        memory.change_slot = true;
        memory.slot_reads.set(0);
        assert!(
            object_catalog(&file, &memory)
                .character_load_requests(target)
                .is_err()
        );
    }
}

fn buddy_fixture(memory: &mut SparseMemory) -> [u64; 7] {
    singleton_fixture(memory);
    let [manager, head, left, right, list_a, list_b, member] = [
        0x3900000, 0x3901000, 0x3902000, 0x3903000, 0x3904000, 0x3905000, 0x3906000,
    ];
    let set = 0x600000 + 0x10f90;
    memory.put64(0x600000 + 0x1e538, manager);
    memory.put64(manager + 0x30, set);
    memory.put64(set, BASE + 0x2a41348);
    memory.put(manager + 0x20, &(-1i32).to_le_bytes());
    memory.put(manager + 0x90, &0x7fc01234u32.to_le_bytes());
    memory.put64(manager + 0x78, head);
    memory.put64(manager + 0x80, 2);
    memory.put64(head, left);
    memory.put64(head + 8, right);
    memory.put64(head + 0x10, right);
    memory.put(head + 0x19, &[1]);
    for (node, children, parent, key, list, count) in [
        (left, [head, head], right, i32::MIN, list_a, 1),
        (right, [left, head], head, i32::MAX, list_b, 0),
    ] {
        memory.put64(node, children[0]);
        memory.put64(node + 8, parent);
        memory.put64(node + 0x10, children[1]);
        memory.put(node + 0x19, &[0]);
        memory.put(node + 0x20, &key.to_le_bytes());
        memory.put64(node + 0x30, list);
        memory.put64(node + 0x38, count);
    }
    memory.put64(list_a, member);
    memory.put64(list_a + 8, member);
    memory.put64(list_b, list_b);
    memory.put64(list_b + 8, list_b);
    memory.put64(member, list_a);
    memory.put64(member + 8, list_a);
    memory.put64(member + 0x10, 0x610000);
    memory.put(member + 0x1c, &20700000i32.to_le_bytes());
    memory.put(member + 0x31, &[0xfe]);
    memory.put64(0x610008, 0xfedcba9876543210);
    // Allocator pointers, tree padding/value+28, member+18, other flags and W are unmapped.
    [manager, head, left, right, list_a, list_b, member]
}

#[test]
fn buddy_groups_preserve_signed_keys_list_order_null_members_full_identity_and_raw_words() {
    let (bytes, mut memory) = fixture();
    let [manager, _, _, _, _, _, member] = buddy_fixture(&mut memory);
    let file = verified(&bytes);
    let value = object_catalog(&file, &memory)
        .buddy_groups(7, 2, 1)
        .unwrap()
        .unwrap();
    assert_eq!(
        value
            .groups
            .iter()
            .map(|g| g.native_key)
            .collect::<Vec<_>>(),
        [i32::MIN, i32::MAX]
    );
    let target = value.groups[0].members[0].character.unwrap();
    assert_eq!(target.native_identifier, 0xfedcba9876543210);
    assert_eq!(target.epoch, 7);
    assert_eq!(value.groups[0].members[0].inactive_request_byte, 0xfe);
    assert_eq!(value.groups[0].members[0].buddy_param_id, 20700000);
    assert_eq!(value.control.item_cooldown_bits, 0x7fc01234);
    memory.put64(member + 0x10, 0);
    let empty_character = object_catalog(&file, &memory)
        .buddy_groups(7, 2, 1)
        .unwrap()
        .unwrap();
    assert_eq!(empty_character.groups[0].members[0].character, None);
    memory.put64(manager + 0x78, 0x3901000);
    memory.put64(manager + 0x80, 0);
    for offset in [0, 8, 0x10] {
        memory.put64(0x3901000 + offset, 0x3901000);
    }
    assert!(
        object_catalog(&file, &memory)
            .buddy_groups(7, 1, 1)
            .unwrap()
            .unwrap()
            .groups
            .is_empty()
    );
    memory.put64(0x600000 + 0x1e538, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .buddy_groups(7, 1, 1)
            .unwrap(),
        None
    );
    assert!(
        object_catalog(&file, &memory)
            .buddy_groups(0, 1, 1)
            .is_err()
    );
}

#[test]
fn buddy_groups_reject_bounds_signed_order_aliases_cycles_counts_and_selected_truncation() {
    let (bytes, mut memory) = fixture();
    let [manager, head, left, right, list_a, _, member] = buddy_fixture(&mut memory);
    let file = verified(&bytes);
    for (groups, members) in [(0, 1), (4097, 1), (1, 1), (2, 0), (2, 4097)] {
        assert!(
            object_catalog(&file, &memory)
                .buddy_groups(1, groups, members)
                .is_err()
        );
    }
    let original = memory.bytes.clone();
    for (address, value) in [
        (manager + 0x30, 0x600000 + 0x10f38),
        (manager + 0x78, head + 1),
        (manager + 0x80, 3),
        (head + 8, head),
        (head, right),
        (left, left),
        (left + 8, head),
        (right + 0x10, left),
        (left + 0x30, 0),
        (left + 0x38, 2),
        (member, member),
        (member + 8, member),
        (list_a + 8, list_a),
        (member + 0x10, 0x610001),
    ] {
        memory.put64(address, value);
        assert!(
            object_catalog(&file, &memory)
                .buddy_groups(1, 2, 1)
                .is_err(),
            "accepted malformed link {address:x}"
        );
        memory.bytes = original.clone();
    }
    memory.put(left + 0x20, &i32::MAX.to_le_bytes());
    assert!(
        object_catalog(&file, &memory)
            .buddy_groups(1, 2, 1)
            .is_err()
    );
    memory.bytes = original.clone();
    for (address, width) in [
        (manager + 0x20, 4),
        (manager + 0x90, 4),
        (manager + 0x30, 8),
        (manager + 0x78, 8),
        (manager + 0x80, 8),
        (head, 8),
        (head + 8, 8),
        (head + 0x10, 8),
        (head + 0x19, 1),
        (left + 0x19, 1),
        (left + 0x20, 4),
        (left + 0x30, 8),
        (left + 0x38, 8),
        (list_a, 8),
        (list_a + 8, 8),
        (member, 8),
        (member + 8, 8),
        (member + 0x10, 8),
        (member + 0x1c, 4),
        (member + 0x31, 1),
    ] {
        for byte in address..address + width {
            memory.bytes.remove(&byte);
            assert!(
                object_catalog(&file, &memory)
                    .buddy_groups(1, 2, 1)
                    .is_err()
            );
            memory.bytes = original.clone();
        }
    }
}

#[test]
fn buddy_groups_reject_changes_in_graph_payload_full_identity_manager_set_and_world() {
    let (bytes, mut memory) = fixture();
    let [manager, head, left, _, list_a, _, member] = buddy_fixture(&mut memory);
    let file = verified(&bytes);
    struct Changed<'a> {
        memory: &'a SparseMemory,
        address: u64,
        reads: Cell<usize>,
    }
    impl AddressSpace for Changed<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if address == self.address {
                let previous = self.reads.replace(self.reads.get() + 1);
                if previous > 0 {
                    output[0] ^= 1;
                }
            }
            Ok(())
        }
    }
    for address in [
        BASE + 0x3d69ff8,
        0x600000,
        0x600000 + 0x1e538,
        0x600000 + 0x10f90,
        manager + 0x30,
        manager + 0x20,
        manager + 0x90,
        manager + 0x78,
        manager + 0x80,
        head + 8,
        head + 0x19,
        left + 0x20,
        left + 8,
        left + 0x30,
        left + 0x38,
        list_a,
        member,
        member + 8,
        member + 0x10,
        member + 0x1c,
        member + 0x31,
        0x610000,
        0x610008,
    ] {
        let changed = Changed {
            memory: &memory,
            address,
            reads: Cell::new(0),
        };
        let objects = GameObjectCatalog {
            module: file
                .bind(
                    &changed,
                    BASE,
                    &[CodeRange {
                        rva: 0x1000,
                        length: 1,
                    }],
                )
                .unwrap(),
        };
        assert!(
            objects.buddy_groups(1, 2, 1).is_err(),
            "accepted changed buddy storage {address:x}"
        );
    }
}

#[test]
fn buddy_control_stores_only_the_selected_word_and_retains_failed_postflight_receipts() {
    use crate::buddy_groups::{BuddyControlWord as Field, commit_control, control_plan};
    let (bytes, mut memory) = fixture();
    buddy_fixture(&mut memory);
    let file = verified(&bytes);
    let source = object_catalog(&file, &memory)
        .buddy_control(1)
        .unwrap()
        .unwrap();
    for field in [Field::RequestIdentifier, Field::ItemCooldown] {
        let plan = control_plan(Some(source), source, field, 0).unwrap();
        let mut expected = source;
        match field {
            Field::RequestIdentifier => expected.request_identifier = 0,
            Field::ItemCooldown => expected.item_cooldown_bits = 0,
        }
        let writes = Cell::new(0);
        let result = commit_control(
            plan,
            |address, bits| {
                assert_eq!(
                    address,
                    source.manager_address
                        + if field == Field::RequestIdentifier {
                            0x20
                        } else {
                            0x90
                        }
                );
                assert_eq!(bits, 0);
                writes.set(writes.get() + 1);
            },
            || Ok(Some(expected)),
        );
        assert_eq!(writes.get(), 1);
        assert!(result.stored);
        assert_eq!(result.postflight, Ok(expected));
        let failed = commit_control(
            control_plan(Some(source), source, field, u32::MAX).unwrap(),
            |_, _| {},
            || {
                Err(Error::NativeOperationRejected(
                    "synthetic postflight failure",
                ))
            },
        );
        assert!(failed.stored);
        assert_eq!(failed.source, source);
        assert!(failed.postflight.is_err());
        assert!(control_plan(None, source, field, 0).is_err());
        let mut stale = source;
        stale.epoch += 1;
        assert!(control_plan(Some(stale), source, field, 0).is_err());
    }
}

fn frontend_marker_fixture(memory: &mut SparseMemory) -> u64 {
    let frontend = 0x2700000;
    memory.put64(BASE + 0x3d6f8f0, frontend);
    memory.put64(frontend, BASE + 0x2aa0a08);
    for index in 0..7 {
        let display = frontend + 0x5c60 + index * 0xb0;
        memory.put64(display + 0x98, 0x8765432100000042 + (index << 32));
        memory.put(display + 0x10, &[255]);
        memory.put(display, &(-0.0f32).to_bits().to_le_bytes());
        memory.put(display + 4, &f32::NAN.to_bits().to_le_bytes());
        memory.put(display + 0x8c, &0.25f32.to_bits().to_le_bytes());
        let tag = frontend + 0x80 + 0x1e88 + index * 0x128;
        memory.put64(tag + 8, 0x8765432100000042 + (index << 32));
        memory.put(tag, &[1, 255, 2]);
        memory.put(tag + 0x10, &i32::MIN.to_le_bytes());
        memory.put(tag + 0x14, &i32::MAX.to_le_bytes());
    }
    frontend
}

#[test]
fn frontend_markers_preserve_seven_full_identifiers_float_bits_and_signed_pixels_without_ui_strings()
 {
    let (bytes, mut memory) = fixture();
    let frontend = frontend_marker_fixture(&mut memory);
    let file = verified(&bytes);
    let value = object_catalog(&file, &memory)
        .frontend_markers(7)
        .unwrap()
        .unwrap();
    assert_eq!(value.values_address, frontend + 0x80);
    for index in 0..7 {
        let identifier = 0x8765432100000042 + ((index as u64) << 32);
        assert_eq!(value.displays[index].identifier, identifier);
        assert_eq!(value.tags[index].identifier, identifier);
        assert_eq!(
            value.displays[index].screen_bits,
            [(-0.0f32).to_bits(), f32::NAN.to_bits()]
        );
        assert_eq!(value.tags[index].screen_pixels, [i32::MIN, i32::MAX]);
        assert_eq!(value.tags[index].flags, [1, 255, 2]);
        assert_eq!(value.displays[index].elapsed_bits, 0.25f32.to_bits());
    }
    // No gauge, string, resource or count byte was supplied by this fixture.
    assert!(object_catalog(&file, &memory).frontend_hud(7).is_err());
    assert!(object_catalog(&file, &memory).frontend_markers(0).is_err());
    memory.put64(BASE + 0x3d6f8f0, 0);
    assert_eq!(
        object_catalog(&file, &memory).frontend_markers(7).unwrap(),
        None
    );
    memory.put64(BASE + 0x3d6f8f0, frontend);
    memory.put64(frontend, BASE + 0x2aed8e0);
    assert!(object_catalog(&file, &memory).frontend_markers(7).is_err());
}

#[test]
fn frontend_markers_reject_every_selected_truncated_byte_and_visible_changes() {
    let (bytes, mut memory) = fixture();
    let frontend = frontend_marker_fixture(&mut memory);
    let file = verified(&bytes);
    let mut ranges = vec![(BASE + 0x3d6f8f0, 8), (frontend, 8)];
    for index in 0..7 {
        let display = frontend + 0x5c60 + index * 0xb0;
        let tag = frontend + 0x80 + 0x1e88 + index * 0x128;
        ranges.extend([
            (display, 8),
            (display + 0x10, 1),
            (display + 0x8c, 4),
            (display + 0x98, 8),
            (tag, 3),
            (tag + 8, 8),
            (tag + 0x10, 8),
        ]);
    }
    for (address, length) in &ranges {
        for offset in 0..*length {
            let missing = address + offset;
            let old = memory.bytes.remove(&missing).unwrap();
            assert!(
                object_catalog(&file, &memory).frontend_markers(7).is_err(),
                "missing {missing:x}"
            );
            memory.put(missing, &[old]);
        }
    }
    for (address, _) in ranges {
        memory.watch_address = address;
        memory.slot_reads.set(0);
        memory.change_slot = true;
        assert!(
            object_catalog(&file, &memory).frontend_markers(7).is_err(),
            "changed {address:x}"
        );
    }
}

fn frontend_hud_fixture(memory: &mut SparseMemory) -> u64 {
    let frontend = 0x2700000;
    memory.put64(BASE + 0x3d6f8f0, frontend);
    memory.put64(frontend, BASE + 0x2aa0a08);
    for index in 0..7 {
        let display = frontend + 0x5c60 + index * 0xb0;
        memory.put(display + 0x10, &[255]);
        memory.put(display + 0x12, &[2]);
        memory.put(display + 0x13, &[u8::from(index == 1)]);
        memory.put(display + 0xa4, &[u8::from(index == 2)]);
        memory.put64(display + 0x98, 0x1234567812340000 + index);
        for i in 0..4 {
            memory.put(display + 0x78 + i * 4, &(100u32 + i as u32).to_le_bytes());
        }
        memory.put(display + 0x8c, &0.5f32.to_bits().to_le_bytes());
        let tag = frontend + 0x80 + 0x1e88 + index * 0x128;
        memory.put(tag, &[1, 255, 2]);
        memory.put64(tag + 8, 0x1234567812340000 + index);
        for i in 0..4 {
            memory.put(tag + 0x18 + i * 4, &(200u32 + i as u32).to_le_bytes());
        }
        memory.put(tag + 0xdc, &123u32.to_le_bytes());
        memory.put(tag + 0xe0, &234u32.to_le_bytes());
        memory.put(tag + 0xe4, &[u8::from(index == 1)]);
        memory.put(tag + 0xe5, &[u8::from(index == 2)]);
    }
    let group = frontend + 0x80 + 0x4cd8;
    memory.put(group + 0x80, &u32::MAX.to_le_bytes()); // Copy raw counts; never index by them.
    for index in 0..5 {
        let display = group + 0x88 + index * 0x18;
        memory.put64(display, 0x76543210fedc0000 + index);
        for i in 0..4 {
            memory.put(display + 8 + i * 4, &(300u32 + i as u32).to_le_bytes());
        }
    }
    frontend
}

#[test]
fn frontend_hud_copies_fixed_records_and_exact_view_alias_without_strings_or_count_indexing() {
    let (bytes, mut memory) = fixture();
    let frontend = frontend_hud_fixture(&mut memory);
    let file = verified(&bytes);
    let value = object_catalog(&file, &memory)
        .frontend_hud(7)
        .unwrap()
        .unwrap();
    assert_eq!(value.values_address, frontend + 0x80);
    assert_eq!(value.spirit_count, u32::MAX);
    assert_eq!(value.spirits[4].hp_words, [300, 301, 302, 303]);
    assert_eq!(value.displays[1].name_color, 1);
    assert_eq!(value.tags[2].downscaled, 1);
    assert!(object_catalog(&file, &memory).frontend_hud(0).is_err());
    let view = 0x2800000;
    memory.put64(view, BASE + 0x2aed8e0);
    memory.put64(view + 0x25c00, frontend + 0x80);
    memory.put64(view + 0xa98, frontend + 0x80);
    let link = object_catalog(&file, &memory)
        .frontend_view_link(view, 7)
        .unwrap()
        .unwrap();
    assert_eq!(link.values_address, value.values_address);
    memory.put64(view + 0xa98, frontend + 0x88);
    assert!(
        object_catalog(&file, &memory)
            .frontend_view_link(view, 7)
            .is_err()
    );
    memory.put64(BASE + 0x3d6f8f0, 0);
    assert_eq!(
        object_catalog(&file, &memory).frontend_hud(7).unwrap(),
        None
    );
}

#[test]
fn frontend_hud_rejects_visible_field_global_type_truncation_and_alias_changes() {
    let (bytes, mut memory) = fixture();
    let frontend = frontend_hud_fixture(&mut memory);
    let file = verified(&bytes);
    for watched in [
        BASE + 0x3d6f8f0,
        frontend,
        frontend + 0x5c60 + 0x10,
        frontend + 0x5c60 + 0x8c,
        frontend + 0x5c60 + 0xb0 + 0x13,
        frontend + 0x80 + 0x1e88,
        frontend + 0x80 + 0x1e88 + 0xdc,
        frontend + 0x80 + 0x4cd8 + 0x80,
        frontend + 0x80 + 0x4cd8 + 0x88 + 8,
    ] {
        memory.watch_address = watched;
        memory.slot_reads.set(0);
        memory.change_slot = true;
        assert!(
            object_catalog(&file, &memory).frontend_hud(7).is_err(),
            "{watched:x}"
        );
    }
    memory.change_slot = false;
    let view = 0x2800000;
    memory.put64(view, BASE + 0x2aed8e0);
    memory.put64(view + 0x25c00, frontend + 0x80);
    memory.put64(view + 0xa98, frontend + 0x80);
    memory.watch_address = view + 0x25c00;
    memory.slot_reads.set(0);
    memory.change_slot = true;
    assert!(
        object_catalog(&file, &memory)
            .frontend_view_link(view, 7)
            .is_err()
    );
    memory.change_slot = false;
    memory.put64(view, BASE + 0x2aa0a08);
    assert!(
        object_catalog(&file, &memory)
            .frontend_view_link(view, 7)
            .is_err()
    );
    memory.bytes.remove(&(frontend + 0x80 + 0x1e88 + 0xe0));
    assert!(object_catalog(&file, &memory).frontend_hud(7).is_err());
}

#[test]
fn frontend_hud_selection_is_full_handle_or_raw_color_or_downscaled_and_preserves_neighbors() {
    use friendly_hud::{HudSelection, commit_hud, hud_plan};
    use std::cell::RefCell;
    let (bytes, mut memory) = fixture();
    frontend_hud_fixture(&mut memory);
    let file = verified(&bytes);
    let source = object_catalog(&file, &memory)
        .frontend_hud(7)
        .unwrap()
        .unwrap();
    let known = [source.displays[0].identifier];
    let select = || HudSelection {
        identifiers: &known,
        name_color: Some(1),
        include_downscaled: true,
    };
    assert!(!select().matches(known[0] ^ (1 << 32), 0, 0));
    let display = source.displays[0].address;
    let tag = source.tags[0].address;
    for (address, raw) in [
        (display + 0x11, vec![0xa5]),
        (display + 0xa0, vec![0xa5; 4]),
        (display + 0xa5, vec![1]),
        (display + 0x18, vec![0xa5; 0x60]),
        (tag + 3, vec![0xa5; 5]),
        (tag + 0x10, vec![0xa5; 8]),
        (tag + 0x28, vec![0xa5; 0xb0]),
    ] {
        memory.put(address, &raw);
    }
    let before = memory.bytes.clone();
    let plan = hud_plan(Some(source), source, select(), true).unwrap();
    let memory = RefCell::new(memory);
    let receipt = commit_hud(
        plan,
        |address, values| memory.borrow_mut().put(address as u64, values),
        || object_catalog(&file, &memory.borrow()).frontend_hud(7),
    );
    assert_eq!(receipt.selected_displays, [0, 1, 2]);
    assert_eq!(receipt.selected_tags, [0, 1, 2]);
    let after = receipt.postflight.unwrap();
    assert_eq!(after.spirit_count, 0);
    assert!(
        after
            .spirits
            .iter()
            .all(|s| s.identifier == u64::MAX && s.hp_words == [0; 4])
    );
    assert_eq!(after.tags[0].hp_words, [0, 201, 0, 0]); // Recoverable HP intentionally retained.
    assert_eq!(after.displays[0].damage_delta_bits, 999.0f32.to_bits());
    assert_eq!(after.displays[3..], source.displays[3..]);
    assert_eq!(after.tags[3..], source.tags[3..]);
    for (address, value) in before {
        if !receipt.stores.iter().any(|store| {
            store.address as u64 <= address
                && address < store.address as u64 + store.written.len() as u64
        }) {
            assert_eq!(
                memory.borrow().bytes[&address],
                value,
                "neighbor {address:x}"
            );
        }
    }
    let plan = hud_plan(Some(after), after, select(), true).unwrap();
    let receipt = commit_hud(plan, |_, _| panic!("already cleared"), || Ok(Some(after)));
    assert!(receipt.stores.is_empty());
    assert_eq!(receipt.postflight.unwrap(), after);
}

#[test]
fn frontend_hud_failed_postflight_retains_attempts_and_foreign_or_unbounded_plan_never_stores() {
    use friendly_hud::{HudSelection, commit_hud, hud_plan};
    let (bytes, mut memory) = fixture();
    frontend_hud_fixture(&mut memory);
    let file = verified(&bytes);
    let source = object_catalog(&file, &memory)
        .frontend_hud(7)
        .unwrap()
        .unwrap();
    let select = || HudSelection {
        identifiers: &[],
        name_color: None,
        include_downscaled: false,
    };
    let plan = hud_plan(Some(source), source, select(), true).unwrap();
    let calls = Cell::new(0);
    let receipt = commit_hud(plan, |_, _| calls.set(calls.get() + 1), || Ok(Some(source)));
    assert_eq!(calls.get(), 26);
    assert_eq!(receipt.stores.len(), 26);
    assert!(receipt.postflight.is_err());
    assert!(receipt.selected_displays.is_empty());
    assert!(receipt.selected_tags.is_empty());
    let mut later = source;
    later.epoch += 1;
    assert!(hud_plan(Some(later), source, select(), true).is_err());
    assert!(hud_plan(None, source, select(), true).is_err());
    let oversized = [0; 257];
    assert!(
        hud_plan(
            Some(source),
            source,
            HudSelection {
                identifiers: &oversized,
                name_color: None,
                include_downscaled: false
            },
            true
        )
        .is_err()
    );
    let plan = hud_plan(Some(source), source, select(), false).unwrap();
    let receipt = commit_hud(plan, |_, _| panic!("empty policy"), || Ok(Some(source)));
    assert!(receipt.stores.is_empty());
    assert_eq!(receipt.postflight.unwrap(), source);
}

fn summon_hud_fixture(memory: &mut SparseMemory) -> [u64; 6] {
    let frontend = 0x2600000;
    let queue = frontend + 0x62b0;
    let list = queue + 0x58;
    memory.put64(BASE + 0x3d6f8f0, frontend);
    memory.put64(frontend, BASE + 0x2aa0a08);
    memory.put64(queue, BASE + 0x2ad0618);
    memory.put64(list, BASE + 0x2ad0608);
    let messages = [
        queue + 8,
        list + 8,
        list + 0x70,
        list + 0xd8,
        list + 0x140,
        list + 0x1b8,
    ];
    for (index, address) in messages.iter().enumerate() {
        memory.put64(*address, BASE + 0x2ad0488);
        memory.put(*address + 8, &(index as i16 + 1).to_le_bytes());
        memory.put(*address + 0xa, &[255]);
        memory.put(*address + 0x48, &[2]);
    }
    // Strings, payloads, padding, count and links deliberately remain unreadable.
    messages
}

#[test]
fn summon_hud_copies_all_six_typed_markers_without_native_payload_or_links() {
    let (bytes, mut memory) = fixture();
    let addresses = summon_hud_fixture(&mut memory);
    let file = verified(&bytes);
    let value = object_catalog(&file, &memory)
        .summon_hud(7)
        .unwrap()
        .unwrap();
    assert_eq!(value.messages.map(|m| m.message.address), addresses);
    assert_eq!(value.messages.map(|m| m.native_kind), [1, 2, 3, 4, 5, 6]);
    assert_eq!(value.messages.map(|m| m.raw_flag), [255; 6]);
    assert!(object_catalog(&file, &memory).summon_hud(0).is_err());
    memory.put(addresses[5] + 8, &i16::MIN.to_le_bytes());
    assert_eq!(
        object_catalog(&file, &memory)
            .summon_hud(7)
            .unwrap()
            .unwrap()
            .messages[5]
            .native_kind,
        i16::MIN
    );
    memory.put64(BASE + 0x3d6f8f0, 0);
    assert_eq!(object_catalog(&file, &memory).summon_hud(7).unwrap(), None);
}

#[test]
fn summon_hud_rejects_changed_marker_global_and_every_embedded_type() {
    let (bytes, mut memory) = fixture();
    let addresses = summon_hud_fixture(&mut memory);
    let file = verified(&bytes);
    for watched in [
        BASE + 0x3d6f8f0,
        0x2600000,
        0x26062b0,
        0x2606308,
        addresses[0],
        addresses[1] + 8,
        addresses[2] + 0xa,
        addresses[5] + 0x48,
    ] {
        memory.watch_address = watched;
        memory.slot_reads.set(0);
        memory.change_slot = true;
        assert!(
            object_catalog(&file, &memory).summon_hud(7).is_err(),
            "{watched:x}"
        );
    }
    memory.change_slot = false;
    for address in addresses {
        memory.put64(address, BASE + 0x2ad0618);
        assert!(object_catalog(&file, &memory).summon_hud(7).is_err());
        memory.put64(address, BASE + 0x2ad0488);
    }
    memory.bytes.remove(&(addresses[4] + 0x48));
    assert!(object_catalog(&file, &memory).summon_hud(7).is_err());
}

#[test]
fn summon_hud_reset_preserves_all_neighbor_bytes_and_retains_actual_failed_stores() {
    use summon_hud::{commit_suppression, suppression_plan};
    let (bytes, mut memory) = fixture();
    summon_hud_fixture(&mut memory);
    let file = verified(&bytes);
    let mut source = object_catalog(&file, &memory)
        .summon_hud(7)
        .unwrap()
        .unwrap();
    let mut storage = [[0xa5a5a5a5a5a5a5a5u64; 13]; 6];
    for (message, buffer) in source.messages.iter_mut().zip(&mut storage) {
        message.message.address = buffer.as_mut_ptr() as u64;
        let raw = message.message.address as *mut u8;
        // SAFETY: Original synthetic initialized aligned arrays owned by this test.
        unsafe {
            std::ptr::copy_nonoverlapping(
                message.native_kind.to_le_bytes().as_ptr(),
                raw.add(8),
                2,
            );
            raw.add(0xa).write(message.raw_flag);
            raw.add(0x48).write(message.raw_tail_flag);
        }
    }
    let before = storage;
    let plan = suppression_plan(Some(source), source).unwrap();
    let count = Cell::new(0);
    let receipt = commit_suppression(
        plan,
        |address, values| {
            count.set(count.get() + 1);
            // SAFETY: Plan destinations refer only to the initialized test arrays above.
            unsafe {
                std::ptr::copy_nonoverlapping(values.as_ptr(), address as *mut u8, values.len());
            }
        },
        || Err(Error::InvalidProfile("synthetic postflight failure")),
    );
    assert_eq!(count.get(), 18);
    assert_eq!(receipt.stores.len(), 18);
    assert!(receipt.postflight.is_err());
    for (buffer, original) in storage.iter().zip(before) {
        let actual: Vec<_> = buffer.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut expected: Vec<_> = original.iter().flat_map(|v| v.to_le_bytes()).collect();
        expected[8..10].copy_from_slice(&(-1i16).to_le_bytes());
        expected[0xa] = 0;
        expected[0x48] = 0;
        assert_eq!(actual, expected);
    }
    assert!(suppression_plan(None, source).is_err());
    let mut later = source;
    later.epoch += 1;
    assert!(suppression_plan(Some(later), source).is_err());
    let mut cleared = source;
    for m in &mut cleared.messages {
        m.native_kind = -1;
        m.raw_flag = 0;
        m.raw_tail_flag = 0;
    }
    let plan = suppression_plan(Some(cleared), cleared).unwrap();
    let receipt = commit_suppression(plan, |_, _| panic!("already reset"), || Ok(Some(cleared)));
    assert!(receipt.stores.is_empty());
    assert_eq!(receipt.postflight.unwrap(), cleared);
    let plan = suppression_plan(Some(source), source).unwrap();
    assert!(
        commit_suppression(plan, |_, _| {}, || Ok(Some(source)))
            .postflight
            .is_err()
    );
}

const BASE: u64 = 0x7ff000000000;
const MANAGER: u64 = 0x100000;
const GROUP: u64 = 0x200000;

fn unit_fixture(
    memory: &mut SparseMemory,
) -> (
    character_sets::CharacterIdentity,
    character_units::CharacterUnitReadPlan,
) {
    use character_sets::WorldCollectionSelection as S;
    use character_visuals::CharacterVisualLink as V;
    character_links_fixture(memory);
    let target = physics_fixture(memory);
    assert_eq!(controller_fixture(memory, ObjectKind::Player), target);
    memory.put64(0xe00028, 0xf00000);
    memory.put64(0xf00000, BASE + 0x2b97168);
    memory.put64(0xf00010, target.address);
    memory.put64(0xe00020, 0xf10000);
    memory.put(0xf10000, &[0xff; 32]);
    assert_eq!(visual_fixture(memory), target);
    memory.put(target.address + 0x60, &80001234i32.to_le_bytes());
    memory.put(target.address + 0x68, &(-9i32).to_le_bytes());
    memory.put(target.address + 0x188, &u32::MAX.to_le_bytes());
    memory.put(target.address + 0x1e8, &0xfefefefeu32.to_le_bytes());
    memory.put64(target.address + 0x5f0, 0x2300000);
    memory.put64(0x2300000, 0);
    memory.put(0x2300008, &(-1i32).to_le_bytes());
    memory.put(0x230000c, &0u32.to_le_bytes());
    let set = target.world_address + 0x10ee0;
    memory.put64(set, BASE + 0x2a41348);
    memory.put(set + 0x10, &1u32.to_le_bytes());
    memory.put64(set + 0x18, 0x2400000);
    memory.put64(0x2400000, target.address);
    memory.put(0x2400008, &[4, 0xfd, 0xe7]);
    memory.put64(target.address + 0x10, 0x2400000);
    (
        target,
        character_units::CharacterUnitReadPlan {
            member: Some((S::Players, 1)),
            controller: true,
            ragdoll: true,
            controller_storage: true,
            physics: true,
            effects_limit: Some(2),
            visuals: vec![
                V::Model,
                V::Pose,
                V::AlternatePose,
                V::ClothPose,
                V::SkeletonMapper,
            ],
            npc_row_limit: Some(1),
            stored_npc_request_id: true,
            stored_event_id: true,
        },
    )
}

#[test]
fn storage_descriptors_feed_existing_typed_unit_reader_without_cpp_structure_references() {
    use character_units::UnitComponent as C;
    use storage_facts::NativeStorageField as F;
    let (bytes, mut memory) = fixture();
    let (target, plan) = unit_fixture(&mut memory);
    let control = memory
        .read_u64(target.address + u64::from(F::CharacterController.location().offset))
        .unwrap();
    let modules = memory
        .read_u64(target.address + u64::from(F::CharacterModules.location().offset))
        .unwrap();
    let physics = memory
        .read_u64(modules + u64::from(F::ModulesPhysics.location().offset))
        .unwrap();
    for (field, value) in [
        (F::ControllerScaleX, 0.25f32),
        (F::ControllerScaleY, 1.5),
        (F::ControllerScaleZ, 9.0),
    ] {
        memory.put(
            control + u64::from(field.location().offset),
            &value.to_bits().to_le_bytes(),
        );
    }
    memory.put(
        physics + u64::from(F::PhysicsMapRadius.location().offset),
        &7.25f32.to_bits().to_le_bytes(),
    );
    memory.put(
        target.address + u64::from(F::CharacterStoredEvent.location().offset),
        &12345u32.to_le_bytes(),
    );
    let file = verified(&bytes);
    let snapshot = object_catalog(&file, &memory)
        .character_unit_snapshot(target, &plan)
        .unwrap()
        .unwrap();
    let C::Present(controller) = snapshot.controller else {
        panic!("controller");
    };
    assert_eq!(controller.axis_scale(), [0.25, 1.5, 9.0]);
    let C::Present(physics) = snapshot.physics else {
        panic!("physics");
    };
    assert_eq!(f32::from_bits(physics.map_collision_bits[1]), 7.25);
    let C::Present(event) = snapshot.stored_event_id else {
        panic!("event");
    };
    assert_eq!(event.stored_id, 12345);
}

#[test]
fn unit_snapshot_combines_selected_components_without_guessing_readiness_or_npc_request_id() {
    use character_units::UnitComponent as C;
    use character_visuals::CharacterVisualLink as V;
    let (bytes, mut memory) = fixture();
    let (target, plan) = unit_fixture(&mut memory);
    let file = verified(&bytes);
    let snapshot = object_catalog(&file, &memory)
        .character_unit_snapshot(target, &plan)
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.selection.raw_kind, -9);
    assert_eq!(snapshot.selection.raw_character_number, u32::MAX);
    assert_eq!(snapshot.selection.character, target);
    assert_eq!(snapshot.plan, plan);
    let C::Present(member) = snapshot.member else {
        panic!("member copy")
    };
    assert_eq!(member.native_state_bytes, [4, 0xfd, 0xe7]);
    let C::Present(controller) = snapshot.controller else {
        panic!("controller copy")
    };
    assert_eq!(controller.axis_scale(), [2.0, 3.0, 4.0]);
    let C::Present(ragdoll) = snapshot.ragdoll else {
        panic!("ragdoll copy")
    };
    assert_eq!(ragdoll.controller, controller);
    assert_eq!(ragdoll.ragdoll.address, 0xf00000);
    let C::Present(storage) = snapshot.controller_storage else {
        panic!("storage copy")
    };
    assert_eq!(storage.controller, controller);
    assert_eq!(storage.words, [u64::MAX; 4]);
    let C::Present(physics) = snapshot.physics else {
        panic!("physics copy")
    };
    assert_eq!(f32::from_bits(physics.weight_bits), -1.0);
    let C::Present(effects) = snapshot.effects else {
        panic!("effect copy")
    };
    assert_eq!(
        effects
            .entries
            .iter()
            .map(|x| x.native_id)
            .collect::<Vec<_>>(),
        vec![899000, 899001]
    );
    let C::Present(npc) = snapshot.npc_parameter else {
        panic!("native query copy")
    };
    assert_eq!(npc.id, -1);
    let C::Present(request) = snapshot.stored_npc_request_id else {
        panic!("stored NPC request")
    };
    assert_eq!(request.stored_id, 80001234);
    assert!(npc.resolved.is_none());
    let C::Present(event) = snapshot.stored_event_id else {
        panic!("stored event")
    };
    assert_eq!(event.stored_id, 0xfefefefe);
    assert_eq!(
        snapshot.visuals.iter().map(|x| x.0).collect::<Vec<_>>(),
        plan.visuals
    );
    for (link, field) in [
        (V::Model, 0x50),
        (V::Pose, 0x398),
        (V::AlternatePose, 0x3a0),
        (V::ClothPose, 0x3a8),
        (V::SkeletonMapper, 0x3b0),
    ] {
        let saved: Vec<_> = (0..8)
            .map(|i| memory.bytes[&(target.address + field + i)])
            .collect();
        memory.put64(target.address + field, 0);
        let copy = object_catalog(&file, &memory)
            .character_unit_snapshot(target, &plan)
            .unwrap()
            .unwrap();
        assert_eq!(copy.visuals.iter().find(|x| x.0 == link).unwrap().1, None);
        memory.put(target.address + field, &saved);
    }
    memory.put64(target.address + 0x58, 0);
    memory.put64(target.address + 0x178, 0);
    memory.put64(target.address + 0x190, 0);
    memory.put64(target.address + 0x5f0, 0);
    let copy = object_catalog(&file, &memory)
        .character_unit_snapshot(target, &plan)
        .unwrap()
        .unwrap();
    assert_eq!(copy.controller, C::Missing);
    assert_eq!(copy.ragdoll, C::Missing);
    assert_eq!(copy.controller_storage, C::Missing);
    assert_eq!(copy.physics, C::Missing);
    assert_eq!(copy.effects, C::Missing);
    assert_eq!(copy.npc_parameter, C::Missing);
}
#[test]
fn unit_snapshot_does_not_read_unrequested_storage_and_validates_plan_before_missing_world() {
    use character_sets::WorldCollectionSelection as S;
    use character_units::{CharacterUnitReadPlan as Plan, UnitComponent as C};
    use character_visuals::CharacterVisualLink as V;
    let (bytes, mut memory) = fixture();
    let target = module_fixture(&mut memory, CharacterModule::Physics);
    let file = verified(&bytes);
    memory.put(target.address + 0x68, &7i32.to_le_bytes());
    memory.put(target.address + 0x188, &8000u32.to_le_bytes());
    // No controller/effect/event/visual/registration/NPC fields have been mapped.
    let copy = object_catalog(&file, &memory)
        .character_unit_snapshot(target, &Plan::default())
        .unwrap()
        .unwrap();
    assert_eq!(copy.controller, C::Unrequested);
    assert_eq!(copy.ragdoll, C::Unrequested);
    assert_eq!(copy.controller_storage, C::Unrequested);
    assert_eq!(copy.physics, C::Unrequested);
    assert_eq!(copy.effects, C::Unrequested);
    assert_eq!(copy.member, C::Unrequested);
    assert_eq!(copy.npc_parameter, C::Unrequested);
    assert_eq!(copy.stored_npc_request_id, C::Unrequested);
    assert_eq!(copy.stored_event_id, C::Unrequested);
    assert!(copy.visuals.is_empty());
    memory.put64(BASE + 0x3d69ff8, 0);
    for plan in [
        Plan {
            member: Some((S::Indexed(197), 1)),
            ..Plan::default()
        },
        Plan {
            member: Some((S::Players, 0)),
            ..Plan::default()
        },
        Plan {
            member: Some((S::Players, 65537)),
            ..Plan::default()
        },
        Plan {
            effects_limit: Some(0),
            ..Plan::default()
        },
        Plan {
            effects_limit: Some(65537),
            ..Plan::default()
        },
        Plan {
            npc_row_limit: Some(0),
            ..Plan::default()
        },
        Plan {
            npc_row_limit: Some(65536),
            ..Plan::default()
        },
        Plan {
            visuals: vec![V::Model, V::Model],
            ..Plan::default()
        },
        Plan {
            visuals: vec![V::Model; 6],
            ..Plan::default()
        },
    ] {
        assert!(
            object_catalog(&file, &memory)
                .character_unit_snapshot(target, &plan)
                .is_err()
        );
    }
    assert_eq!(
        object_catalog(&file, &memory)
            .character_unit_snapshot(target, &Plan::default())
            .unwrap(),
        None
    );
    assert!(
        object_catalog(&file, &memory)
            .character_unit_snapshot(
                character_sets::CharacterIdentity { epoch: 0, ..target },
                &Plan::default()
            )
            .is_err()
    );
}
#[test]
fn unit_snapshot_detects_changes_after_individual_reader_postflight() {
    let (bytes, mut memory) = fixture();
    let (target, plan) = unit_fixture(&mut memory);
    let file = verified(&bytes);
    struct Later<'a> {
        memory: &'a SparseMemory,
        watch: u64,
        count: Cell<usize>,
    }
    impl AddressSpace for Later<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if address == self.watch {
                let count = self.count.get() + 1;
                self.count.set(count);
                if count > 2 {
                    output.fill(0);
                }
            }
            Ok(())
        }
    }
    for watch in [
        0xf10000,
        0xe00028,
        target.address + 0x60,
        target.address + 0x68,
        target.address + 0x188,
        0xe002d4,
        0xd00330,
        0xc11008,
        target.address + 0x50,
        target.address + 0x1e8,
    ] {
        let memory = Later {
            memory: &memory,
            watch,
            count: Cell::new(0),
        };
        let objects = GameObjectCatalog {
            module: file
                .bind(
                    &memory,
                    BASE,
                    &[CodeRange {
                        rva: 0x1000,
                        length: 1,
                    }],
                )
                .unwrap(),
        };
        assert!(
            objects.character_unit_snapshot(target, &plan).is_err(),
            "late field {watch:x}"
        );
        assert!(memory.count.get() > 2);
    }
    for watch in [
        target.address,
        target.address + 8,
        0xe00010,
        0xd00008,
        0xc10010,
        target.address + 0x10,
        0x2400008,
    ] {
        memory.watch_address = watch;
        memory.change_slot = true;
        memory.slot_reads.set(0);
        assert!(
            object_catalog(&file, &memory)
                .character_unit_snapshot(target, &plan)
                .is_err()
        );
    }
}

fn team_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    let (_, target) = registration_fixture(memory);
    memory.put(target.address + 0x6c, &[7]);
    memory.put64(target.address + 0x178, 0);
    memory.put64(target.address + 0x318, 0);
    target
}
fn team_effect_fixture(memory: &mut SparseMemory, target: character_sets::CharacterIdentity) {
    memory.put64(target.address + 0x178, 0x2000000);
    memory.put64(0x2000000, BASE + 0x2a4d490);
    memory.put64(0x2000010, target.address);
    memory.put64(0x2000028, 0xfu64 << 52);
    memory.put64(0x2000008, 0x2010000);
    memory.put(0x2010060, &0u32.to_le_bytes());
    // PARAM rows are copied bytes, not C++ objects requiring an invented alignment.
    memory.put64(0x2010000, 0x2020001);
    memory.put(0x2020157, &132u16.to_le_bytes());
    // Matching prefix does not need to read the rest of the effect chain.
}
fn temporary_team_fixture(memory: &mut SparseMemory, target: character_sets::CharacterIdentity) {
    memory.put64(target.address + 0x318, 0x2100000);
    memory.put64(0x2100000, BASE + 0x2a44410);
    memory.put(0x2100008, &0u32.to_le_bytes());
    memory.put(0x210000c, &24u32.to_le_bytes());
    memory.put64(0x2100010, 0);
    memory.put(0x2100018, &[255]);
    memory.put64(0x2100030, 0x2110001);
    memory.put64(0x2100038, 1);
    memory.put(0x2100044, &[7]);
    memory.put64(0x2110001, 0x2120001);
    memory.put(0x2120015, &[34]);
}

#[test]
fn effective_team_preserves_unknown_values_and_native_override_precedence() {
    let (bytes, mut memory) = fixture();
    let target = team_fixture(&mut memory);
    let file = verified(&bytes);
    for value in [0, 15, 34, 40, 78, 79, 255] {
        memory.put(target.address + 0x6c, &[value]);
        let observation = object_catalog(&file, &memory)
            .character_team(target, 1)
            .unwrap()
            .unwrap();
        assert_eq!(observation.stored_team, value);
        assert_eq!(observation.effective_team, value);
        assert_eq!(observation.team_index().is_ok(), value < 79);
        assert!(!observation.effect_override);
    }
    team_effect_fixture(&mut memory, target);
    let observation = object_catalog(&file, &memory)
        .character_team(target, 1)
        .unwrap()
        .unwrap();
    assert_eq!(observation.effective_team, 15);
    assert!(observation.effect_override);
    temporary_team_fixture(&mut memory, target);
    let observation = object_catalog(&file, &memory)
        .character_team(target, 1)
        .unwrap()
        .unwrap();
    assert_eq!(observation.effective_team, 36);
    assert!(observation.effect_override);
    assert_eq!(
        observation.temporary_slot.unwrap().kind,
        ObjectKind::TemporaryTeamSlot
    );
    // Native empty temporary slot overrides both stored team and effect result with zero.
    memory.put64(0x2100038, 0);
    memory
        .bytes
        .retain(|address, _| !(0x2100030..0x2100038).contains(address));
    assert_eq!(
        object_catalog(&file, &memory)
            .character_team(target, 1)
            .unwrap()
            .unwrap()
            .effective_team,
        0
    );
}

#[test]
fn team_effect_cache_and_skip_flags_use_only_native_consumed_prefix() {
    let (bytes, mut memory) = fixture();
    let target = team_fixture(&mut memory);
    team_effect_fixture(&mut memory, target);
    let file = verified(&bytes);
    memory.put64(0x2000028, !(0xfu64 << 52));
    memory
        .bytes
        .retain(|address, _| !(0x2000008..0x2000010).contains(address));
    assert_eq!(
        object_catalog(&file, &memory)
            .character_team(target, 1)
            .unwrap()
            .unwrap()
            .effective_team,
        7
    );
    memory.put64(0x2000028, 1u64 << 52);
    memory.put64(0x2000008, 0x2010000);
    memory.put64(0x2010030, 0);
    for flags in [1u32, 2, 0x40000, 0x80000, 0x80000000, 0x800c0003] {
        memory.put(0x2010060, &flags.to_le_bytes());
        // Skipped entries never read uninitialized parameter storage.
        memory
            .bytes
            .retain(|address, _| !(0x2010000..0x2010008).contains(address));
        assert_eq!(
            object_catalog(&file, &memory)
                .character_team(target, 1)
                .unwrap()
                .unwrap()
                .effective_team,
            7
        );
    }
    memory.put(0x2010060, &4u32.to_le_bytes());
    memory.put64(0x2010000, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .character_team(target, 1)
            .unwrap()
            .unwrap()
            .effective_team,
        7
    );
    memory.put64(0x2010000, 0x2020001);
    assert_eq!(
        object_catalog(&file, &memory)
            .character_team(target, 1)
            .unwrap()
            .unwrap()
            .effective_team,
        15
    );
    memory.put(0x2020157, &131u16.to_le_bytes());
    memory.put64(0x2010030, 0x2010000);
    assert!(
        object_catalog(&file, &memory)
            .character_team(target, 2)
            .is_err()
    );
    memory.put64(0x2010030, 0x2030000);
    memory.put(0x2030060, &0u32.to_le_bytes());
    memory.put64(0x2030000, 0x2020001);
    memory.put64(0x2030030, 0);
    assert!(
        object_catalog(&file, &memory)
            .character_team(target, 1)
            .is_err()
    );
    assert_eq!(
        object_catalog(&file, &memory)
            .character_team(target, 2)
            .unwrap()
            .unwrap()
            .effective_team,
        7
    );
}

#[test]
fn temporary_team_remapping_and_first_active_slot_match_owned_switch_cases() {
    let (bytes, mut memory) = fixture();
    let target = team_fixture(&mut memory);
    temporary_team_fixture(&mut memory, target);
    let file = verified(&bytes);
    for (team, cases) in [
        (
            34u8,
            &[(6u8, 34u8), (7, 36), (8, 37), (9, 38), (11, 39), (24, 35)][..],
        ),
        (
            40,
            &[
                (6, 40),
                (7, 42),
                (8, 43),
                (9, 44),
                (11, 45),
                (24, 41),
                (28, 46),
            ][..],
        ),
        (255, &[][..]),
    ] {
        memory.put(0x2120015, &[team]);
        for role in 0u8..=255 {
            memory.put(0x2100044, &[role]);
            let expected = cases
                .iter()
                .find(|(r, _)| *r == role)
                .map_or(team, |(_, t)| *t);
            assert_eq!(
                object_catalog(&file, &memory)
                    .character_team(target, 1)
                    .unwrap()
                    .unwrap()
                    .effective_team,
                expected
            );
        }
    }
    // Inactive/other-kind/index nodes precede the first match, and need no team-specific payload.
    memory.put64(target.address + 0x318, 0x2130000);
    memory.put(0x2130008, &0u32.to_le_bytes());
    memory.put(0x213000c, &24u32.to_le_bytes());
    memory.put(0x2130018, &[0]);
    memory.put64(0x2130010, 0x2100000);
    assert!(
        object_catalog(&file, &memory)
            .character_team(target, 1)
            .is_err()
    );
    assert_eq!(
        object_catalog(&file, &memory)
            .character_team(target, 2)
            .unwrap()
            .unwrap()
            .effective_team,
        255
    );
    memory.put(0x2130018, &[1]);
    memory.put(0x2130008, &1u32.to_le_bytes());
    assert_eq!(
        object_catalog(&file, &memory)
            .character_team(target, 2)
            .unwrap()
            .unwrap()
            .effective_team,
        255
    );
    memory.put(0x2130008, &0u32.to_le_bytes());
    memory.put(0x213000c, &23u32.to_le_bytes());
    assert_eq!(
        object_catalog(&file, &memory)
            .character_team(target, 2)
            .unwrap()
            .unwrap()
            .effective_team,
        255
    );
    memory.put64(0x2130010, 0x2130000);
    assert!(
        object_catalog(&file, &memory)
            .character_team(target, 2)
            .is_err()
    );
}

#[test]
fn effective_team_rejects_visible_input_changes_foreign_owners_and_truncation() {
    for watched in [
        0x62006c,
        0x620178,
        0x2000000,
        0x2000010,
        0x2000028,
        0x2000008,
        0x2010000,
        0x2020157,
        0x620318,
        0x2100000,
        0x210000c,
        0x2100018,
        0x2100030,
        0x2100038,
        0x2100044,
        0x2110001,
        0x2120015,
        0x620008,
        BASE + 0x3d69ff8,
    ] {
        let (bytes, mut memory) = fixture();
        let target = team_fixture(&mut memory);
        team_effect_fixture(&mut memory, target);
        temporary_team_fixture(&mut memory, target);
        memory.watch_address = watched;
        memory.change_slot = true;
        let file = verified(&bytes);
        assert!(
            object_catalog(&file, &memory)
                .character_team(target, 2)
                .is_err(),
            "watch {watched:x}"
        );
    }
    let (bytes, mut memory) = fixture();
    let target = team_fixture(&mut memory);
    team_effect_fixture(&mut memory, target);
    temporary_team_fixture(&mut memory, target);
    let file = verified(&bytes);
    memory.put64(0x2000010, target.address + 8);
    assert!(
        object_catalog(&file, &memory)
            .character_team(target, 2)
            .is_err()
    );
    memory.put64(0x2000010, target.address);
    memory.put64(0x2100000, BASE + 0x2a7fbb0);
    assert!(
        object_catalog(&file, &memory)
            .character_team(target, 2)
            .is_err()
    );
    memory.put64(0x2100000, BASE + 0x2a44410);
    memory.bytes.remove(&0x2120015);
    assert!(
        object_catalog(&file, &memory)
            .character_team(target, 2)
            .is_err()
    );
    for limit in [0, 65537] {
        assert!(
            object_catalog(&file, &memory)
                .character_team(target, limit)
                .is_err()
        );
    }
    assert!(
        object_catalog(&file, &memory)
            .character_team(character_sets::CharacterIdentity { epoch: 0, ..target }, 2)
            .is_err()
    );
    assert!(
        object_catalog(&file, &memory)
            .character_team(
                character_sets::CharacterIdentity {
                    native_identifier: 1,
                    ..target
                },
                2
            )
            .is_err()
    );
    memory.put64(BASE + 0x3d69ff8, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .character_team(target, 2)
            .unwrap(),
        None
    );
}

#[test]
fn directed_team_relations_use_live_cells_exact_types_and_dispatch() {
    let (bytes, mut memory) = fixture();
    let file = verified(&bytes);
    let cell = |subject: u8, other: u8| {
        BASE + 0x3b283f0 + (u64::from(subject) * 79 + u64::from(other)) * 8
    };
    for (table, method, kind, expected) in [
        (0x2a4f868, 0x51b580, ObjectKind::TeamNeutral, false),
        (0x2a4f878, 0x51b590, ObjectKind::TeamFriend, false),
        (0x2a4f888, 0x51b5a0, ObjectKind::TeamEnemy, true),
        (0x2a4f898, 0x51b5b0, ObjectKind::TeamRival, true),
    ] {
        memory.put64(0x2200000, BASE + table);
        memory.put64(BASE + table, BASE + method);
        for (subject, other) in [(0, 0), (78, 78), (0, 78), (78, 0), (1, 2)] {
            memory.put64(cell(subject, other), 0x2200000);
            let result = object_catalog(&file, &memory)
                .team_relation(subject, other)
                .unwrap()
                .unwrap();
            assert_eq!(
                (
                    result.subject_team,
                    result.other_team,
                    result.object.kind,
                    result.hostile
                ),
                (subject, other, kind, expected)
            );
        }
    }
    memory.put64(cell(2, 1), 0);
    assert_eq!(
        object_catalog(&file, &memory).team_relation(2, 1).unwrap(),
        None
    );
    assert!(
        object_catalog(&file, &memory)
            .team_relation(1, 2)
            .unwrap()
            .unwrap()
            .hostile
    );
    for pair in [(79, 0), (0, 79), (255, 255)] {
        assert!(
            object_catalog(&file, &memory)
                .team_relation(pair.0, pair.1)
                .is_err()
        );
    }
    for watched in [cell(1, 2), 0x2200000, BASE + 0x2a4f898] {
        memory.slot_reads.set(0);
        memory.watch_address = watched;
        memory.change_slot = true;
        assert!(object_catalog(&file, &memory).team_relation(1, 2).is_err());
    }
    memory.change_slot = false;
    memory.put64(BASE + 0x2a4f898, BASE + 0x51b590);
    assert!(object_catalog(&file, &memory).team_relation(1, 2).is_err());
    memory.put64(0x2200000, BASE + 0x2a7fbb0);
    assert!(object_catalog(&file, &memory).team_relation(1, 2).is_err());
    memory.put64(cell(1, 2), 0x2200001);
    assert!(object_catalog(&file, &memory).team_relation(1, 2).is_err());
}

#[test]
fn character_relation_coordinates_fresh_same_scope_inputs_and_direction() {
    let (bytes, mut memory) = fixture();
    let subject = team_fixture(&mut memory);
    let other = character_sets::CharacterIdentity {
        address: 0x610000,
        native_identifier: 0xffffffff10000001,
        ..subject
    };
    memory.put(other.address + 0x6c, &[2]);
    memory.put64(other.address + 0x178, 0);
    memory.put64(other.address + 0x318, 0);
    let subject_cell = BASE + 0x3b283f0 + (7 * 79 + 2) * 8;
    let other_cell = BASE + 0x3b283f0 + (2 * 79 + 7) * 8;
    memory.put64(subject_cell, 0x2200000);
    memory.put64(other_cell, 0x2210000);
    memory.put64(0x2200000, BASE + 0x2a4f888);
    memory.put64(0x2210000, BASE + 0x2a4f878);
    memory.put64(BASE + 0x2a4f888, BASE + 0x51b5a0);
    memory.put64(BASE + 0x2a4f878, BASE + 0x51b590);
    let file = verified(&bytes);
    let result = object_catalog(&file, &memory)
        .character_relation(subject, other, 1)
        .unwrap()
        .unwrap();
    assert_eq!(result.subject.character, subject);
    assert_eq!(result.other.character, other);
    assert!(result.relation.hostile);
    assert!(
        !object_catalog(&file, &memory)
            .character_relation(other, subject, 1)
            .unwrap()
            .unwrap()
            .relation
            .hostile
    );
    // Introduce a change after the first complete input/cell copy, during coordination.
    struct Later<'a> {
        memory: &'a SparseMemory,
        watch: u64,
        count: Cell<usize>,
    }
    impl AddressSpace for Later<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if address == self.watch {
                self.count.set(self.count.get() + 1);
                if self.count.get() > 2 {
                    output.fill(0);
                }
            }
            Ok(())
        }
    }
    for watch in [subject.address + 0x6c, other.address + 0x6c, subject_cell] {
        let memory = Later {
            memory: &memory,
            watch,
            count: Cell::new(0),
        };
        let objects = GameObjectCatalog {
            module: file
                .bind(
                    &memory,
                    BASE,
                    &[CodeRange {
                        rva: 0x1000,
                        length: 1,
                    }],
                )
                .unwrap(),
        };
        assert!(objects.character_relation(subject, other, 1).is_err());
    }
    for wrong in [
        character_sets::CharacterIdentity { epoch: 2, ..other },
        character_sets::CharacterIdentity {
            world_address: other.world_address + 8,
            ..other
        },
        character_sets::CharacterIdentity {
            native_identifier: other.native_identifier ^ 0x100000000,
            ..other
        },
    ] {
        assert!(
            object_catalog(&file, &memory)
                .character_relation(subject, wrong, 1)
                .is_err()
        );
    }
    for limit in [0, 65537] {
        assert!(
            object_catalog(&file, &memory)
                .character_relation(subject, other, limit)
                .is_err()
        );
    }
    memory.put64(subject_cell, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .character_relation(subject, other, 1)
            .unwrap(),
        None
    );
    memory.put(other.address + 0x6c, &[79]);
    assert!(
        object_catalog(&file, &memory)
            .character_relation(subject, other, 1)
            .is_err()
    );
    memory.put64(BASE + 0x3d69ff8, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .character_relation(subject, other, 1)
            .unwrap(),
        None
    );
}

#[test]
fn stored_npc_request_preserves_values_without_handle_lookup_and_rejects_visible_changes() {
    let (bytes, mut memory) = fixture();
    let (_, target) = registration_fixture(&mut memory);
    let file = verified(&bytes);
    // No association link or PARAM repository has been mapped for this reader.
    for table in [0x2a7fbb0, 0x2a47090, 0x2a310c8] {
        memory.put64(target.address, BASE + table);
        for stored in [i32::MIN, -5, -1, 0, 80001234, i32::MAX] {
            memory.put(target.address + 0x60, &stored.to_le_bytes());
            let copy = object_catalog(&file, &memory)
                .character_npc_request_id(target)
                .unwrap()
                .unwrap();
            assert_eq!((copy.character, copy.stored_id), (target, stored));
        }
    }
    memory.put64(target.address, BASE + 0x2a7fbb0);
    for watch in [
        target.address + 0x60,
        target.address,
        target.address + 8,
        BASE + 0x3d69ff8,
    ] {
        memory.watch_address = watch;
        memory.change_slot = true;
        memory.slot_reads.set(0);
        assert!(
            object_catalog(&file, &memory)
                .character_npc_request_id(target)
                .is_err()
        );
    }
    memory.change_slot = false;
    for byte in 0..4 {
        let address = target.address + 0x60 + byte;
        let value = memory.bytes.remove(&address).unwrap();
        assert!(
            object_catalog(&file, &memory)
                .character_npc_request_id(target)
                .is_err()
        );
        memory.put(address, &[value]);
    }
    assert!(
        object_catalog(&file, &memory)
            .character_npc_request_id(character_sets::CharacterIdentity { epoch: 0, ..target })
            .is_err()
    );
    assert!(
        object_catalog(&file, &memory)
            .character_npc_request_id(character_sets::CharacterIdentity {
                native_identifier: target.native_identifier ^ 0x100000000,
                ..target
            })
            .is_err()
    );
    memory.put64(target.address, BASE + 0x2a7fba8);
    assert!(
        object_catalog(&file, &memory)
            .character_npc_request_id(target)
            .is_err()
    );
    memory.put64(BASE + 0x3d69ff8, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .character_npc_request_id(target)
            .unwrap(),
        None
    );
}

#[test]
fn initializer_words_copy_only_selected_storage_and_preserve_signed_values() {
    use character_parameters::CharacterInitializerWord as F;
    let (bytes, mut memory) = fixture();
    let (_, target) = registration_fixture(&mut memory);
    let file = verified(&bytes);
    for (table, kind) in [
        (0x2a7fbb0, ObjectKind::Player),
        (0x2a47090, ObjectKind::Enemy),
        (0x2a310c8, ObjectKind::Character),
    ] {
        memory.put64(target.address, BASE + table);
        for field in [F::ResourceNumber, F::EnemyThinkRequest] {
            let offset = if field == F::ResourceNumber {
                0x64
            } else {
                0x5a0
            };
            // No NPC handle, PARAM row, adjacent character-number or other word
            // is mapped. A non-enemy must not read an enemy tail at all.
            for value in [i32::MIN, -1, 0, 80001234, i32::MAX] {
                if field == F::ResourceNumber || kind == ObjectKind::Enemy {
                    memory.put(target.address + offset, &value.to_le_bytes());
                } else {
                    for byte in 0..4 {
                        memory.bytes.remove(&(target.address + offset + byte));
                    }
                }
                let result = object_catalog(&file, &memory)
                    .character_initializer_word(target, field)
                    .unwrap();
                if field == F::EnemyThinkRequest && kind != ObjectKind::Enemy {
                    assert_eq!(result, None);
                } else {
                    let copied = result.unwrap();
                    assert_eq!(
                        (copied.character, copied.field, copied.value),
                        (target, field, value)
                    );
                }
            }
        }
    }
}

#[test]
fn initializer_words_reject_truncation_visible_changes_and_stale_identity() {
    use character_parameters::CharacterInitializerWord as F;
    let (bytes, mut memory) = fixture();
    let (_, target) = registration_fixture(&mut memory);
    let file = verified(&bytes);
    memory.put64(target.address, BASE + 0x2a47090);
    for (field, offset) in [(F::ResourceNumber, 0x64), (F::EnemyThinkRequest, 0x5a0)] {
        memory.put(target.address + offset, &(-123i32).to_le_bytes());
        for watch in [
            target.address + offset,
            target.address,
            target.address + 8,
            BASE + 0x3d69ff8,
        ] {
            memory.watch_address = watch;
            memory.change_slot = true;
            memory.slot_reads.set(0);
            assert!(
                object_catalog(&file, &memory)
                    .character_initializer_word(target, field)
                    .is_err()
            );
        }
        memory.change_slot = false;
        for byte in 0..4 {
            let at = target.address + offset + byte;
            let value = memory.bytes.remove(&at).unwrap();
            assert!(
                object_catalog(&file, &memory)
                    .character_initializer_word(target, field)
                    .is_err()
            );
            memory.put(at, &[value]);
        }
        for bad in [
            character_sets::CharacterIdentity { epoch: 0, ..target },
            character_sets::CharacterIdentity {
                native_identifier: target.native_identifier ^ 0x100000000,
                ..target
            },
        ] {
            assert!(
                object_catalog(&file, &memory)
                    .character_initializer_word(bad, field)
                    .is_err()
            );
        }
        memory.put64(target.address, BASE + 0x2a47088);
        assert!(
            object_catalog(&file, &memory)
                .character_initializer_word(target, field)
                .is_err()
        );
        memory.put64(target.address, BASE + 0x2a47090);
    }
    memory.put64(BASE + 0x3d69ff8, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .character_initializer_word(target, F::ResourceNumber)
            .unwrap(),
        None
    );
}

#[test]
fn raw_character_selection_fields_preserve_unknown_values_and_exact_class() {
    let (bytes, mut memory) = fixture();
    let (_, target) = registration_fixture(&mut memory);
    let file = verified(&bytes);
    for (table, kind) in [
        (0x2a7fbb0, ObjectKind::Player),
        (0x2a47090, ObjectKind::Enemy),
    ] {
        memory.put64(target.address, BASE + table);
        for raw_kind in [i32::MIN, -1, 0, 5, 6, 7, 19, 20, 21, i32::MAX] {
            for number in [0u32, 8000, 0xffffffff] {
                memory.put(target.address + 0x68, &raw_kind.to_le_bytes());
                memory.put(target.address + 0x188, &number.to_le_bytes());
                let result = object_catalog(&file, &memory)
                    .character_selection(target)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    (
                        result.object_kind,
                        result.raw_kind,
                        result.raw_character_number
                    ),
                    (kind, raw_kind, number)
                );
            }
        }
    }
    for watch in [
        target.address + 0x68,
        target.address + 0x188,
        target.address,
        target.address + 8,
        BASE + 0x3d69ff8,
    ] {
        memory.watch_address = watch;
        memory.change_slot = true;
        memory.slot_reads.set(0);
        assert!(
            object_catalog(&file, &memory)
                .character_selection(target)
                .is_err()
        );
    }
    memory.change_slot = false;
    for offset in [0x68u64, 0x188] {
        for byte in 0..4 {
            let address = target.address + offset + byte;
            let value = memory.bytes.remove(&address).unwrap();
            assert!(
                object_catalog(&file, &memory)
                    .character_selection(target)
                    .is_err()
            );
            memory.put(address, &[value]);
        }
    }
    assert!(
        object_catalog(&file, &memory)
            .character_selection(character_sets::CharacterIdentity { epoch: 0, ..target })
            .is_err()
    );
    assert!(
        object_catalog(&file, &memory)
            .character_selection(character_sets::CharacterIdentity {
                native_identifier: 0,
                ..target
            })
            .is_err()
    );
    memory.put64(BASE + 0x3d69ff8, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .character_selection(target)
            .unwrap(),
        None
    );
}

#[test]
fn frontend_state_keeps_unknown_word_and_absence_separate_without_world_access() {
    let (bytes, mut memory) = fixture();
    let file = verified(&bytes);
    memory.put64(BASE + 0x3d6f8f0, 0);
    assert_eq!(
        object_catalog(&file, &memory).frontend_state(1).unwrap(),
        None
    );
    memory.put64(BASE + 0x3d6f8f0, 0x2300000);
    memory.put64(0x2300000, BASE + 0x2aa0a08);
    for value in [i32::MIN, -1, 0, 1, 2, 3, 4, 255, i32::MAX] {
        memory.put(0x230653c, &value.to_le_bytes());
        let result = object_catalog(&file, &memory)
            .frontend_state(9)
            .unwrap()
            .unwrap();
        assert_eq!(result.frontend.kind, ObjectKind::Frontend);
        assert_eq!(result.frontend.address, 0x2300000);
        assert_eq!(result.module_base, BASE);
        assert_eq!(result.epoch, 9);
        assert_eq!(result.native_state, value);
    }
    // No frontend neighbor, world singleton or player fields are mapped.
}

#[test]
fn frontend_state_rejects_changed_foreign_unaligned_truncated_or_invalid_sources() {
    let (bytes, mut memory) = fixture();
    let file = verified(&bytes);
    memory.put64(BASE + 0x3d6f8f0, 0x2300000);
    memory.put64(0x2300000, BASE + 0x2aa0a08);
    memory.put(0x230653c, &5i32.to_le_bytes());
    for watch in [BASE + 0x3d6f8f0, 0x2300000, 0x230653c] {
        memory.watch_address = watch;
        memory.slot_reads.set(0);
        memory.change_slot = true;
        assert!(object_catalog(&file, &memory).frontend_state(1).is_err());
    }
    memory.change_slot = false;
    for index in 0..4 {
        let address = 0x230653c + index;
        let value = memory.bytes.remove(&address).unwrap();
        assert!(object_catalog(&file, &memory).frontend_state(1).is_err());
        memory.put(address, &[value]);
    }
    memory.put64(0x2300000, BASE + 0x2a9eed8);
    assert!(object_catalog(&file, &memory).frontend_state(1).is_err());
    memory.put64(BASE + 0x3d6f8f0, 0x2300001);
    assert!(object_catalog(&file, &memory).frontend_state(1).is_err());
    memory.put64(BASE + 0x3d6f8f0, 0);
    assert!(object_catalog(&file, &memory).frontend_state(0).is_err());
}

#[test]
fn immutable_metadata_cache_rejects_other_files_and_keeps_live_code_validation_fresh() {
    let (bytes, mut memory) = fixture();
    let file = verified(&bytes);
    let gate = file_metadata::FileMetadataGate::new(file.identity());
    let scans = Cell::new(0);
    for _ in 0..2 {
        gate.verify(&file, || {
            scans.set(scans.get() + 1);
            Ok(())
        })
        .unwrap();
    }
    assert_eq!(scans.get(), 1);
    let mut changed = bytes.clone();
    changed[0x500] ^= 1;
    let other = verified(&changed);
    assert!(matches!(
        gate.verify(&other, || unreachable!()),
        Err(Error::UnsupportedImage { .. })
    ));
    file.bind(
        &memory,
        BASE,
        &[CodeRange {
            rva: 0x1000,
            length: 1,
        }],
    )
    .unwrap();
    memory.put(BASE + 0x1000, &[0x90]);
    gate.verify(&file, || unreachable!()).unwrap();
    assert!(matches!(
        file.bind(
            &memory,
            BASE,
            &[CodeRange {
                rva: 0x1000,
                length: 1
            }]
        ),
        Err(Error::ImageMismatch { .. })
    ));
    let failed = file_metadata::FileMetadataGate::new(file.identity());
    let expected = Error::InvalidProfile("original synthetic metadata mismatch");
    assert_eq!(
        failed.verify(&file, || Err(expected.clone())).unwrap_err(),
        expected
    );
    assert_eq!(
        failed.verify(&file, || unreachable!()).unwrap_err(),
        expected
    );
}

#[test]
fn immutable_metadata_verification_runs_once_across_concurrent_adapters() {
    use std::sync::{
        Barrier,
        atomic::{AtomicUsize, Ordering},
    };
    let (bytes, _) = fixture();
    let file = verified(&bytes);
    let gate = file_metadata::FileMetadataGate::new(file.identity());
    let start = Barrier::new(9);
    let scans = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                start.wait();
                gate.verify(&file, || {
                    scans.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
                .unwrap();
            });
        }
        start.wait();
    });
    assert_eq!(scans.load(Ordering::SeqCst), 1);
}

#[test]
fn gparam_singletons_are_independently_typed_and_missing_is_not_a_request() {
    for (global, table, kind, role) in [
        (0x3d5f158, 0x29d4800, ObjectKind::FileResourceManager, 0),
        (
            0x3d735c8,
            0x2b583c0,
            ObjectKind::WorldSceneDrawParameters,
            1,
        ),
        (0x3d76ba8, 0x2b73788, ObjectKind::GparamRepository, 2),
    ] {
        let (file, mut memory) = fixture();
        memory.put64(BASE + global, 0x1300000);
        memory.put64(0x1300000, BASE + table);
        let checked = verified(&file);
        let objects = object_catalog(&checked, &memory);
        let read = |objects: &GameObjectCatalog<'_, '_, SparseMemory>| match role {
            0 => objects.gparam_file_manager(),
            1 => objects.gparam_draw_manager(),
            _ => objects.gparam_repository(),
        };
        assert_eq!(
            read(&objects).unwrap(),
            Some(objects::ObjectObservation {
                address: 0x1300000,
                kind
            })
        );
        memory.put64(BASE + global, 0);
        assert_eq!(read(&object_catalog(&checked, &memory)).unwrap(), None);
        // Other manager globals and unrelated object fields deliberately stay unmapped.
    }
}

#[test]
fn gparam_singletons_refuse_foreign_types_unaligned_or_truncated_addresses() {
    for pointer in [0x1300000, 0x1300001, u64::MAX - 7] {
        let (file, mut memory) = fixture();
        memory.put64(BASE + 0x3d5f158, pointer);
        memory.put64(0x1300000, BASE + 0x2b583c0);
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .gparam_file_manager()
                .is_err()
        );
    }
}

#[test]
fn gparam_singleton_reads_detect_visible_global_and_vtable_changes() {
    for watched in [BASE + 0x3d735c8, 0x1300000] {
        let (file, mut memory) = fixture();
        memory.put64(BASE + 0x3d735c8, 0x1300000);
        memory.put64(0x1300000, BASE + 0x2b583c0);
        memory.watch_address = watched;
        memory.change_slot = true;
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .gparam_draw_manager()
                .is_err()
        );
    }
}

fn torrent_state_fixture(memory: &mut SparseMemory, selector: u8) {
    memory.put64(BASE + 0x3d61f98, 0x1100000);
    // Naked manager first word and unrelated fields intentionally remain unmapped.
    memory.put64(0x1100008, 0x1200000);
    memory.put64(0x1200000, BASE + 0x29e8fa8);
    memory.put(0x1200960, &[selector]);
}

fn npc_data_pool_fixture(memory: &mut SparseMemory) {
    memory.put64(BASE + 0x3d61f98, 0x1100000);
    memory.put64(0x1100028, 0x1300000);
    memory.put(0x1300000, &[0; 320]);
    for (index, address, identifier) in [(0, 0x1200000, 0i32), (39, 0x1400000, -1i32)] {
        memory.put64(0x1300000 + index * 8, address);
        memory.put64(address, BASE + 0x29e8fa8);
        memory.put(address + 8, &identifier.to_le_bytes());
    }
    // No primary-player link, manager vtable, object tail or adjacent array bytes mapped.
}

#[test]
fn npc_player_data_pool_copies_full_bounds_and_rejects_invalid_or_duplicate_slots() {
    let (file, mut memory) = fixture();
    npc_data_pool_fixture(&mut memory);
    let checked = verified(&file);
    let copied = object_catalog(&checked, &memory)
        .npc_player_data_pool(7)
        .unwrap()
        .unwrap();
    assert_eq!(copied.module_base, BASE);
    assert_eq!(
        (copied.manager_address, copied.slots_address, copied.epoch),
        (0x1100000, 0x1300000, 7)
    );
    let occupied: Vec<_> = copied.occupied().collect();
    assert_eq!(occupied.len(), 2);
    assert_eq!((occupied[0].index, occupied[0].native_identifier), (0, 0));
    assert_eq!((occupied[1].index, occupied[1].native_identifier), (39, -1));
    assert!(copied.slots[1..39].iter().all(Option::is_none));
    assert!(
        object_catalog(&checked, &memory)
            .npc_player_data_pool(0)
            .is_err()
    );
    for mode in 0..8 {
        npc_data_pool_fixture(&mut memory);
        match mode {
            0 => memory.put64(0x1300008, 0x1200000),
            1 => memory.put64(0x1400000, BASE + 0x2a7fbb0),
            2 => memory.put64(0x1100028, 0x1300001),
            3 => memory.put64(BASE + 0x3d61f98, 0x1100001),
            4 => memory.put64(0x1300000, 0x1200001),
            5 => {
                memory.bytes.remove(&0x130013f);
            }
            6 => {
                memory.bytes.remove(&0x140000b);
            }
            _ => memory.put64(0x1100028, u64::MAX - 7),
        }
        assert!(
            object_catalog(&checked, &memory)
                .npc_player_data_pool(7)
                .is_err(),
            "mode {mode}"
        );
    }
    npc_data_pool_fixture(&mut memory);
    memory.put(0x1300000, &[0; 320]);
    assert_eq!(
        object_catalog(&checked, &memory)
            .npc_player_data_pool(7)
            .unwrap()
            .unwrap()
            .occupied()
            .count(),
        0
    );
    memory.put64(0x1100028, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .npc_player_data_pool(7)
            .unwrap(),
        None
    );
    memory.put64(BASE + 0x3d61f98, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .npc_player_data_pool(7)
            .unwrap(),
        None
    );
}

#[test]
fn npc_player_data_pool_detects_visible_changes_in_all_copied_links_types_and_ids() {
    for watched in [
        BASE + 0x3d61f98,
        0x1100028,
        0x1300000,
        0x1200000,
        0x1400000,
        0x1400008,
    ] {
        let (file, mut memory) = fixture();
        npc_data_pool_fixture(&mut memory);
        memory.watch_address = watched;
        memory.change_slot = true;
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .npc_player_data_pool(7)
                .is_err(),
            "watched {watched:#x}"
        );
    }
}

#[test]
fn npc_player_data_association_excludes_primary_data_and_rechecks_entire_player_and_pool() {
    for watched in [
        0,
        BASE + 0x3d69ff8,
        0x610000,
        0x610008,
        0x610580,
        0x1300000,
        0x1400008,
    ] {
        let (file, mut memory) = fixture();
        singleton_fixture(&mut memory);
        npc_data_pool_fixture(&mut memory);
        memory.put64(0x610008, 123);
        memory.put64(0x610580, 0x1400000);
        let checked = verified(&file);
        let target = object_catalog(&checked, &memory)
            .character_identity(0x610000, 7)
            .unwrap()
            .unwrap();
        memory.watch_address = watched;
        memory.slot_reads.set(0);
        memory.change_slot = watched != 0;
        let result = object_catalog(&checked, &memory).npc_player_data_for_character(target);
        if watched != 0 {
            assert!(result.is_err(), "watched {watched:#x}");
            continue;
        }
        let result = result.unwrap().unwrap();
        assert_eq!(result.character, target);
        assert_eq!(result.slot.index, 39);
        memory.put64(0x610580, 0x1500000);
        memory.put64(0x1500000, BASE + 0x29e8fa8);
        assert_eq!(
            object_catalog(&checked, &memory)
                .npc_player_data_for_character(target)
                .unwrap(),
            None
        );
        memory.put64(0x610580, 0);
        assert_eq!(
            object_catalog(&checked, &memory)
                .npc_player_data_for_character(target)
                .unwrap(),
            None
        );
        memory.put64(0x610000, BASE + 0x2a47090);
        assert!(
            object_catalog(&checked, &memory)
                .npc_player_data_for_character(target)
                .is_err()
        );
    }
}

#[test]
fn npc_data_release_confirmation_never_reads_released_storage_or_new_aliases() {
    let (file, mut memory) = fixture();
    npc_data_pool_fixture(&mut memory);
    let checked = verified(&file);
    let before = object_catalog(&checked, &memory)
        .npc_player_data_pool(7)
        .unwrap()
        .unwrap();
    let removed = before.slots[39].unwrap();
    memory.put64(0x1300138, 0);
    // Former object is completely unreadable, even its vtable/identifier.
    for address in 0x1400000..0x140000c {
        memory.bytes.remove(&address);
    }
    let after = object_catalog(&checked, &memory)
        .npc_player_data_pool_after_release(&before, removed)
        .unwrap();
    assert_eq!(after.slots[39], None);
    assert_eq!(after.slots[0], before.slots[0]);
    for index in [0, 1, 39] {
        memory.put64(0x1300000 + index * 8, removed.data_address);
        assert_eq!(
            object_catalog(&checked, &memory)
                .npc_player_data_pool_after_release(&before, removed)
                .unwrap_err(),
            Error::InvalidProfile("released NPC data address remains in pool")
        );
        memory.put64(
            0x1300000 + index * 8,
            before.slots[index as usize].map_or(0, |v| v.data_address),
        );
        memory.put64(0x1300138, 0);
    }
    // A different raw ID on a surviving allocation is not silently accepted.
    memory.put(0x1200008, &1i32.to_le_bytes());
    assert!(
        object_catalog(&checked, &memory)
            .npc_player_data_pool_after_release(&before, removed)
            .is_err()
    );
    memory.put(0x1200008, &0i32.to_le_bytes());
    let mut invalid = removed;
    invalid.index = 40;
    assert!(
        object_catalog(&checked, &memory)
            .npc_player_data_pool_after_release(&before, invalid)
            .is_err()
    );
    let mut foreign = before.clone();
    foreign.module_base += 8;
    assert!(
        object_catalog(&checked, &memory)
            .npc_player_data_pool_after_release(&foreign, removed)
            .is_err()
    );
    struct LateAlias<'a> {
        memory: &'a SparseMemory,
        reads: Cell<usize>,
        released: u64,
    }
    impl AddressSpace for LateAlias<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if address == 0x1300000 {
                let reads = self.reads.get();
                self.reads.set(reads + 1);
                if reads != 0 {
                    output[312..320].copy_from_slice(&self.released.to_le_bytes());
                }
            }
            Ok(())
        }
    }
    let changed = LateAlias {
        memory: &memory,
        reads: Cell::new(0),
        released: removed.data_address,
    };
    let objects = GameObjectCatalog {
        module: checked
            .bind(
                &changed,
                BASE,
                &[CodeRange {
                    rva: 0x1000,
                    length: 1,
                }],
            )
            .unwrap(),
    };
    assert_eq!(
        objects
            .npc_player_data_pool_after_release(&before, removed)
            .unwrap_err(),
        Error::InvalidProfile("released NPC data address remains in pool")
    );
}

#[test]
fn npc_data_release_confirmation_rechecks_survivors_and_pool_identity() {
    for watched in [BASE + 0x3d61f98, 0x1100028, 0x1300000, 0x1200000] {
        let (file, mut memory) = fixture();
        npc_data_pool_fixture(&mut memory);
        let checked = verified(&file);
        let before = object_catalog(&checked, &memory)
            .npc_player_data_pool(7)
            .unwrap()
            .unwrap();
        let removed = before.slots[39].unwrap();
        memory.put64(0x1300138, 0);
        memory.watch_address = watched;
        memory.slot_reads.set(0);
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .npc_player_data_pool_after_release(&before, removed)
                .is_err(),
            "watched {watched:#x}"
        );
    }
}

#[test]
fn torrent_selector_copies_native_four_slots_and_distinguishes_missing_state() {
    for selector in 0..=3 {
        let (file, mut memory) = fixture();
        torrent_state_fixture(&mut memory, selector);
        let checked = verified(&file);
        let objects = object_catalog(&checked, &memory);
        let result = objects.torrent_selector(7).unwrap().unwrap();
        assert_eq!(
            result,
            torrent::TorrentSelectorObservation {
                module_base: BASE,
                manager_address: 0x1100000,
                player_data_address: 0x1200000,
                epoch: 7,
                selector
            }
        );
        assert!(objects.torrent_selector(0).is_err());
        memory.put64(0x1100008, 0);
        assert_eq!(
            object_catalog(&checked, &memory)
                .torrent_selector(7)
                .unwrap(),
            None
        );
        memory.put64(BASE + 0x3d61f98, 0);
        assert_eq!(
            object_catalog(&checked, &memory)
                .torrent_selector(7)
                .unwrap(),
            None
        );
    }
}

#[test]
fn torrent_selector_rejects_untyped_truncated_unaligned_and_overflowed_links() {
    for mode in 0..7 {
        let (file, mut memory) = fixture();
        torrent_state_fixture(&mut memory, 1);
        match mode {
            0 => memory.put(0x1200960, &[4]),
            1 => memory.put64(0x1200000, BASE + 0x29e8fb0),
            2 => {
                memory.bytes.remove(&0x1200960);
            }
            3 => memory.put64(BASE + 0x3d61f98, 0x1100001),
            4 => memory.put64(BASE + 0x3d61f98, u64::MAX - 7),
            5 => memory.put64(0x1100008, 0x1200001),
            _ => {
                memory.put64(0x1100008, u64::MAX - 7);
                memory.put64(u64::MAX - 7, BASE + 0x29e8fa8);
            }
        }
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .torrent_selector(7)
                .is_err(),
            "mode {mode}"
        );
    }
}

#[test]
fn torrent_selector_rechecks_byte_and_selected_links_without_copying_unrelated_fields() {
    for watched in [BASE + 0x3d61f98, 0x1100008, 0x1200000, 0x1200960] {
        let (file, mut memory) = fixture();
        torrent_state_fixture(&mut memory, 1);
        memory.watch_address = watched;
        memory.change_slot = true;
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .torrent_selector(7)
                .is_err(),
            "watched {watched:#x}"
        );
    }
}

fn flags_fixture(memory: &mut SparseMemory, kind: u32) {
    memory.put64(BASE + 0x3d6c4b8, 0xf00000);
    memory.put64(0xf00000, BASE + 0x2a74660);
    memory.put(0xf0001c, &1000u32.to_le_bytes());
    memory.put(0xf00020, &125u32.to_le_bytes());
    memory.put(0xf00024, &2u32.to_le_bytes());
    memory.put64(0xf00028, 0xf30000);
    memory.put64(0xf00038, 0xf10000);
    memory.put64(0xf00040, 1);
    memory.put(0xf10019, &[1]);
    memory.put64(0xf10008, 0xf20000);
    memory.put64(0xf20000, 0xf10000);
    memory.put64(0xf20010, 0xf10000);
    memory.put(0xf20019, &[0]);
    memory.put(0xf20020, &6u32.to_le_bytes());
    memory.put(0xf20028, &kind.to_le_bytes());
    match kind {
        1 => {
            memory.put(0xf20030, &1u32.to_le_bytes());
        }
        2 => {
            memory.put64(0xf20030, 0xf40000);
        }
        _ => {}
    }
    // Only requested bytes are mapped. +2c padding, unused payload upper word, other groups,
    // manager tail and allocator are deliberately unreadable.
}

#[test]
fn event_flags_read_pool_and_external_storage_with_native_group_and_msb_bit_order() {
    use event_flags::EventFlagStorage as S;
    for kind in [1, 2] {
        let (file, mut memory) = fixture();
        flags_fixture(&mut memory, kind);
        let data = if kind == 1 { 0xf30000 + 125 } else { 0xf40000 };
        memory.put(data, &[0b10000001]);
        memory.put(data + 1, &[0b10000000]);
        memory.put(data + 124, &[1]);
        let checked = verified(&file);
        let objects = object_catalog(&checked, &memory);
        for (id, expected) in [
            (6000, true),
            (6001, false),
            (6006, false),
            (6007, true),
            (6008, true),
            (6999, true),
        ] {
            let result = objects.event_flag(id, 1).unwrap().unwrap();
            assert_eq!(
                (
                    result.id,
                    result.group_id,
                    result.bit_in_group,
                    result.value
                ),
                (id, 6, id - 6000, expected)
            );
            assert_eq!(result.node_address, Some(0xf20000));
            assert_eq!(
                result.storage,
                if kind == 1 {
                    S::Pool {
                        index: 1,
                        group_address: data,
                    }
                } else {
                    S::External {
                        group_address: data,
                    }
                }
            );
        }
    }
}

#[test]
fn event_flags_distinguish_absent_manager_missing_group_unbacked_and_null_external() {
    use event_flags::EventFlagStorage as S;
    let (file, mut memory) = fixture();
    flags_fixture(&mut memory, 0);
    let checked = verified(&file);
    for id in [0, 1000, u32::MAX] {
        let x = object_catalog(&checked, &memory)
            .event_flag(id, 1)
            .unwrap()
            .unwrap();
        assert_eq!(x.storage, S::MissingGroup);
        assert!(!x.value);
        assert_eq!(x.node_address, None);
    }
    for kind in [0, 3, u32::MAX] {
        memory.put(0xf20028, &kind.to_le_bytes());
        let x = object_catalog(&checked, &memory)
            .event_flag(6700, 1)
            .unwrap()
            .unwrap();
        assert_eq!(x.storage, S::Unbacked { native_kind: kind });
        assert!(!x.value);
    }
    memory.put(0xf20028, &2u32.to_le_bytes());
    memory.put64(0xf20030, 0);
    let x = object_catalog(&checked, &memory)
        .event_flag(6700, 1)
        .unwrap()
        .unwrap();
    assert_eq!(x.storage, S::External { group_address: 0 });
    assert!(!x.value);
    memory.put64(BASE + 0x3d6c4b8, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .event_flag(6700, 1)
            .unwrap(),
        None
    );
}

#[test]
fn event_flags_reject_bad_metadata_pool_bounds_pointer_overflow_type_and_tree_cycles() {
    for (address, bytes) in [
        (0xf0001c, 0u32.to_le_bytes().to_vec()),
        (0xf0001c, 999u32.to_le_bytes().to_vec()),
        (0xf00020, 124u32.to_le_bytes().to_vec()),
        (0xf00024, 65537u32.to_le_bytes().to_vec()),
        (0xf00024, 1u32.to_le_bytes().to_vec()),
        (0xf00028, 0u64.to_le_bytes().to_vec()),
        (0xf00028, u64::MAX.to_le_bytes().to_vec()),
        (0xf20030, 2u32.to_le_bytes().to_vec()),
        (0xf00000, (BASE + 0x2a310c8).to_le_bytes().to_vec()),
        (0xf00038, 0u64.to_le_bytes().to_vec()),
        (0xf00040, 2u64.to_le_bytes().to_vec()),
        (0xf10019, vec![0]),
        (0xf10008, 0u64.to_le_bytes().to_vec()),
    ] {
        let (file, mut memory) = fixture();
        flags_fixture(&mut memory, 1);
        memory.put(0xf30000 + 125 + 87, &[8]);
        memory.put(address, &bytes);
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .event_flag(6700, 1)
                .is_err(),
            "address{address:x}"
        );
    }
    let (file, mut memory) = fixture();
    flags_fixture(&mut memory, 1);
    memory.put64(0xf20000, 0xf20000);
    let checked = verified(&file);
    assert!(
        object_catalog(&checked, &memory)
            .event_flag(5000, 1)
            .is_err()
    );
    for limit in [0, 65537] {
        assert!(
            object_catalog(&checked, &memory)
                .event_flag(6700, limit)
                .is_err()
        );
    }
    memory.put(0xf20028, &2u32.to_le_bytes());
    memory.put64(0xf20030, u64::MAX - 124);
    assert!(
        object_catalog(&checked, &memory)
            .event_flag(6700, 1)
            .is_err()
    );
}

#[test]
fn event_flags_reject_truncation_and_visible_changes_without_reading_unused_payload() {
    struct Changed<'a> {
        memory: &'a SparseMemory,
        address: u64,
        reads: Cell<u32>,
    }
    impl AddressSpace for Changed<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if address == self.address {
                let n = self.reads.get();
                self.reads.set(n + 1);
                if n > 0 {
                    output[0] ^= 1;
                }
            }
            Ok(())
        }
    }
    for address in [
        BASE + 0x3d6c4b8,
        0xf00000,
        0xf0001c,
        0xf00020,
        0xf00024,
        0xf00028,
        0xf00038,
        0xf00040,
        0xf10008,
        0xf20028,
        0xf20030,
        0xf40000 + 87,
    ] {
        let (file, mut memory) = fixture();
        flags_fixture(&mut memory, 2);
        memory.put(0xf40000 + 87, &[8]);
        let changed = Changed {
            memory: &memory,
            address,
            reads: Cell::new(0),
        };
        let checked = verified(&file);
        let module = checked
            .bind(
                &changed,
                BASE,
                &[CodeRange {
                    rva: 0x1000,
                    length: 1,
                }],
            )
            .unwrap();
        assert!(
            GameObjectCatalog { module }.event_flag(6700, 1).is_err(),
            "address{address:x}"
        );
    }
    for address in [0xf0001f, 0xf0002f, 0xf20033, 0xf40000 + 87] {
        let (file, mut memory) = fixture();
        flags_fixture(&mut memory, 2);
        memory.put(0xf40000 + 87, &[8]);
        memory.bytes.remove(&address);
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .event_flag(6700, 1)
                .is_err(),
            "truncated{address:x}"
        );
    }
}

fn skeleton_mapping_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    let target = visual_fixture(memory);
    memory.put64(0xe50088, 0xf00000);
    memory.put(0xf00004, &6u32.to_le_bytes());
    memory.put64(0xf00008, 0xf10000);
    for (index, value) in [0i16, i16::MAX, -1, -2, i16::MIN, 3]
        .into_iter()
        .enumerate()
    {
        memory.put(0xf10000 + index as u64 * 2, &value.to_le_bytes());
    }
    // Unknown table word0, references, source bones and fallback payload are unmapped.
    target
}

#[test]
fn skeleton_mapping_preserves_signed_indices_fallback_values_and_outside_distinction() {
    use crate::skeleton_mapping::ModelObjectPoseIndex as I;
    let (file, mut memory) = fixture();
    let target = skeleton_mapping_fixture(&mut memory);
    let checked = verified(&file);
    let copy = object_catalog(&checked, &memory)
        .skeleton_mapping(target, 6)
        .unwrap()
        .unwrap();
    assert_eq!(copy.native_indices, [0, i16::MAX, -1, -2, i16::MIN, 3]);
    for (index, expected) in [
        (0, I::Mapped(0)),
        (1, I::Mapped(32767)),
        (2, I::Fallback(-1)),
        (3, I::Fallback(-2)),
        (4, I::Fallback(i16::MIN)),
        (5, I::Mapped(3)),
    ] {
        assert_eq!(copy.model_object(index), Some(expected));
    }
    assert_eq!(copy.model_object(6), None);
    assert_eq!(copy.model_object(u32::MAX), None);
}

#[test]
fn skeleton_mapping_distinguishes_empty_table_missing_mapper_and_missing_table() {
    let (file, mut memory) = fixture();
    let target = skeleton_mapping_fixture(&mut memory);
    let checked = verified(&file);
    memory.put(0xf00004, &0u32.to_le_bytes());
    memory.put64(0xf00008, 0);
    assert!(
        object_catalog(&checked, &memory)
            .skeleton_mapping(target, 1)
            .unwrap()
            .unwrap()
            .native_indices
            .is_empty()
    );
    memory.put64(0xe50088, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .skeleton_mapping(target, 1)
            .unwrap(),
        None
    );
    memory.put64(0x6103b0, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .skeleton_mapping(target, 1)
            .unwrap(),
        None
    );
}

#[test]
fn skeleton_mapping_rejects_limits_invalid_counts_pointers_extent_and_truncation() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for (address, bytes) in [
        (0xf00004, u32::MAX.to_le_bytes().to_vec()),
        (0xf00004, 7u32.to_le_bytes().to_vec()),
        (0xf00008, 0u64.to_le_bytes().to_vec()),
        (0xf00008, 0xf10001u64.to_le_bytes().to_vec()),
        (0xf00008, (u64::MAX - 1).to_le_bytes().to_vec()),
        (0xe50088, 0xf00001u64.to_le_bytes().to_vec()),
    ] {
        let (_, mut memory) = fixture();
        let target = skeleton_mapping_fixture(&mut memory);
        memory.put(address, &bytes);
        assert!(
            object_catalog(&checked, &memory)
                .skeleton_mapping(target, 6)
                .is_err()
        );
    }
    for address in [0xf00007, 0xf0000f, 0xf1000b] {
        let (_, mut memory) = fixture();
        let target = skeleton_mapping_fixture(&mut memory);
        memory.bytes.remove(&address);
        assert!(
            object_catalog(&checked, &memory)
                .skeleton_mapping(target, 6)
                .is_err()
        );
    }
    let (_, mut memory) = fixture();
    let target = skeleton_mapping_fixture(&mut memory);
    for limit in [0, 5, 65537] {
        assert!(
            object_catalog(&checked, &memory)
                .skeleton_mapping(target, limit)
                .is_err()
        );
    }
}

#[test]
fn skeleton_mapping_rejects_visible_index_table_type_or_complete_identity_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for watch in [
        0xf10000, 0xf00004, 0xf00008, 0xe50088, 0xe50000, 0x6103b0, 0x610008,
    ] {
        let (_, mut memory) = fixture();
        let target = skeleton_mapping_fixture(&mut memory);
        memory.watch_address = watch;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .skeleton_mapping(target, 6)
                .is_err(),
            "{watch:x}"
        );
    }
}

fn pose_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    let target = visual_fixture(memory);
    for address in [0xe20000, 0xe30000, 0xe40000] {
        for (offset, data) in [(0x50, 0xf00000), (0x60, 0xf10000)] {
            memory.put64(address + offset, data);
            memory.put(address + offset + 8, &2i32.to_le_bytes());
            memory.put(address + offset + 12, &0xc000_0004u32.to_le_bytes());
        }
    }
    target
}

fn pose_state_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    let target = pose_fixture(memory);
    for importer in [0xe20000, 0xe30000, 0xe40000] {
        memory.put64(importer + 0x48, 0xf20000);
        memory.put64(importer + 0x70, 0xf40000);
        memory.put(importer + 0x78, &2i32.to_le_bytes());
        memory.put(importer + 0x7c, &0x80000004u32.to_le_bytes());
        memory.put(importer + 0x80, &[0, 1]);
    }
    memory.put64(0xf20000, BASE + 0x2b4c750);
    memory.put64(0xf20020, 0xf30000);
    memory.put(0xf20028, &2i32.to_le_bytes());
    memory.put(0xf2002c, &0xc0000002u32.to_le_bytes());
    memory.put(0xf20038, &2i32.to_le_bytes());
    target
}

#[test]
fn pose_state_copies_typed_skeleton_and_all_bounds_without_reading_uninitialized_arrays() {
    use crate::character_visuals::CharacterVisualLink as V;
    let (file, mut memory) = fixture();
    let target = pose_state_fixture(&mut memory);
    let checked = verified(&file);
    for link in [V::Pose, V::AlternatePose, V::ClothPose] {
        let state = object_catalog(&checked, &memory)
            .pose_state(target, link, 2)
            .unwrap()
            .unwrap();
        assert_eq!(state.skeleton.kind, ObjectKind::AnimationSkeleton);
        assert_eq!(
            (state.bone_count, state.local.count, state.model.count),
            (2, 2, 2)
        );
        assert_eq!(
            (state.parents_address, state.flags_address),
            (0xf30000, 0xf40000)
        );
        assert_eq!(state.parents_capacity_and_flags, 0xc0000002);
        assert_eq!(state.cache_bytes, [0, 1]);
        let mut next = state;
        next.cache_bytes = [1, 0];
        assert!(state.same_storage(next));
        next.flags_address += 4;
        assert!(!state.same_storage(next));
    }
    memory.put64(0xe20048, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .pose_state(target, V::Pose, 2)
            .unwrap(),
        None
    );
}

#[test]
fn pose_state_rejects_count_disagreement_foreign_type_changes_and_truncation() {
    use crate::character_visuals::CharacterVisualLink as V;
    let (file, _) = fixture();
    let checked = verified(&file);
    for (address, bytes) in [
        (0xf20038, 3u32.to_le_bytes().to_vec()),
        (0xf20028, 1u32.to_le_bytes().to_vec()),
        (0xe20078, (-1i32).to_le_bytes().to_vec()),
        (0xe20068, 1u32.to_le_bytes().to_vec()),
        (0xe2007c, 1u32.to_le_bytes().to_vec()),
        (0xf20020, 1u64.to_le_bytes().to_vec()),
        (0xe20070, (u64::MAX - 3).to_le_bytes().to_vec()),
        (0xf20000, (BASE + 0x2a7fbb0).to_le_bytes().to_vec()),
    ] {
        let (_, mut memory) = fixture();
        let target = pose_state_fixture(&mut memory);
        memory.put(address, &bytes);
        assert!(
            object_catalog(&checked, &memory)
                .pose_state(target, V::Pose, 2)
                .is_err()
        );
    }
    for watched in [
        0xe20048, 0xf20000, 0xf20038, 0xf20020, 0xe20070, 0xe20080, 0xe20060, 0x610398, 0x610008,
    ] {
        let (_, mut memory) = fixture();
        let target = pose_state_fixture(&mut memory);
        memory.watch_address = watched;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .pose_state(target, V::Pose, 2)
                .is_err(),
            "watch {watched:#x}"
        );
        memory.change_slot = false;
        memory.bytes.remove(&watched);
        assert!(
            object_catalog(&checked, &memory)
                .pose_state(target, V::Pose, 2)
                .is_err()
        );
    }
}

#[test]
fn pose_metadata_preserves_capacity_flags_without_reading_records_or_unproven_skeleton() {
    use crate::{character_visuals::CharacterVisualLink as V, pose_buffers::PoseBuffer as B};
    let (file, mut memory) = fixture();
    let target = pose_fixture(&mut memory);
    let checked = verified(&file);
    for link in [V::Pose, V::AlternatePose, V::ClothPose] {
        for (buffer, data) in [(B::Primary, 0xf00000), (B::Secondary, 0xf10000)] {
            let copy = object_catalog(&checked, &memory)
                .pose_buffer(target, link, buffer, 2)
                .unwrap()
                .unwrap();
            assert_eq!(copy.count, 2);
            assert_eq!(copy.capacity(), 4);
            assert_eq!(copy.capacity_and_flags, 0xc000_0004);
            assert_eq!(copy.data_address, data);
        }
    }
    memory.put64(0x610398, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .pose_buffer(target, V::Pose, B::Primary, 2)
            .unwrap(),
        None
    );
}

#[test]
fn pose_copy_reads_only_requested_records_and_preserves_every_original_bit() {
    use crate::{
        character_visuals::CharacterVisualLink as V,
        pose_buffers::{PoseBuffer as B, PoseRecordBits},
    };
    let (file, mut memory) = fixture();
    let target = pose_fixture(&mut memory);
    let bits = [
        [0x8000_0000u32, 0x7fc0_1234, 1, 0xffff_ffff],
        [0, 1, 2, 3],
        [4, 5, 6, 7],
    ];
    for (index, word) in bits.into_iter().flatten().enumerate() {
        memory.put(0xf00030 + index as u64 * 4, &word.to_le_bytes());
    }
    let checked = verified(&file);
    let objects = object_catalog(&checked, &memory);
    // SAFETY: Only requested record1 is original initialized synthetic storage.
    let copy = unsafe { objects.copy_pose_records(target, V::Pose, B::Primary, 1, 1, 2) }
        .unwrap()
        .unwrap();
    assert_eq!(copy.first, 1);
    assert_eq!(copy.records, [PoseRecordBits(bits)]);
    // SAFETY: Empty range performs no record access; all metadata is original test storage.
    assert!(
        // SAFETY: Empty original synthetic range; no payload access.
        unsafe { objects.copy_pose_records(target, V::Pose, B::Primary, 2, 0, 2) }
            .unwrap()
            .unwrap()
            .records
            .is_empty()
    );
    // SAFETY: Rejected ranges never access payload storage.
    for (first, count) in [(2, 1), (u32::MAX, 2), (0, 3)] {
        assert!(
            // SAFETY: Original test adapter rejects ranges before payload access.
            unsafe { objects.copy_pose_records(target, V::Pose, B::Primary, first, count, 2) }
                .is_err()
        );
    }
}

#[test]
fn pose_metadata_rejects_negative_counts_caps_bad_pointers_truncation_and_invalid_requests() {
    use crate::{character_visuals::CharacterVisualLink as V, pose_buffers::PoseBuffer as B};
    let (file, _) = fixture();
    let checked = verified(&file);
    for (address, bytes) in [
        (0xe20058, (-1i32).to_le_bytes().to_vec()),
        (0xe20058, 3i32.to_le_bytes().to_vec()),
        (0xe2005c, 1u32.to_le_bytes().to_vec()),
        (0xe20050, 0u64.to_le_bytes().to_vec()),
        (0xe20050, 1u64.to_le_bytes().to_vec()),
        (0xe20050, (u64::MAX - 3).to_le_bytes().to_vec()),
    ] {
        let (_, mut memory) = fixture();
        let target = pose_fixture(&mut memory);
        memory.put(address, &bytes);
        assert!(
            object_catalog(&checked, &memory)
                .pose_buffer(target, V::Pose, B::Primary, 2)
                .is_err()
        );
    }
    let (_, mut memory) = fixture();
    let target = pose_fixture(&mut memory);
    memory.bytes.remove(&0xe2005f);
    assert!(
        object_catalog(&checked, &memory)
            .pose_buffer(target, V::Pose, B::Primary, 2)
            .is_err()
    );
    for (link, limit) in [
        (V::Model, 2),
        (V::SkeletonMapper, 2),
        (V::Pose, 0),
        (V::Pose, 65537),
    ] {
        assert!(
            object_catalog(&checked, &memory)
                .pose_buffer(target, link, B::Primary, limit)
                .is_err()
        );
    }
}

#[test]
fn pose_copy_rejects_truncated_records_and_visible_payload_metadata_link_changes() {
    use crate::{character_visuals::CharacterVisualLink as V, pose_buffers::PoseBuffer as B};
    let (file, _) = fixture();
    let checked = verified(&file);
    for watch in [0xf00030, 0xe20050, 0x610398, 0xe20000, 0x610008] {
        let (_, mut memory) = fixture();
        let target = pose_fixture(&mut memory);
        memory.put(0xf00030, &[1; 48]);
        memory.watch_address = watch;
        memory.change_slot = true;
        // SAFETY: Original initialized synthetic record only; deliberate changes must reject.
        assert!(
            // SAFETY: Original synthetic record; injected changes are expected to reject.
            unsafe {
                object_catalog(&checked, &memory).copy_pose_records(
                    target,
                    V::Pose,
                    B::Primary,
                    1,
                    1,
                    2,
                )
            }
            .is_err()
        );
    }
    let (_, mut memory) = fixture();
    let target = pose_fixture(&mut memory);
    memory.put(0xf00030, &[1; 47]);
    // SAFETY: Test adapter reports the missing byte, without accessing native storage.
    assert!(
        // SAFETY: Original test adapter reports missing byte; no game memory.
        unsafe {
            object_catalog(&checked, &memory).copy_pose_records(
                target,
                V::Pose,
                B::Primary,
                1,
                1,
                2,
            )
        }
        .is_err()
    );
}

fn visual_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    let target = module_fixture(memory, CharacterModule::Physics);
    for (offset, address, table) in [
        (0x50, 0xe10000, 0x2b35c68),
        (0x398, 0xe20000, 0x2b70360),
        (0x3a0, 0xe30000, 0x2b70360),
        (0x3a8, 0xe40000, 0x2b70360),
        (0x3b0, 0xe50000, 0x2b6f1a0),
    ] {
        memory.put64(target.address + offset, address);
        memory.put64(address, BASE + table);
        // No unproven owner, refcount, pose buffers or skeleton payload are mapped.
    }
    target
}

#[test]
fn visuals_copy_selected_typed_links_without_claiming_pose_or_skeleton_payload() {
    use crate::character_visuals::CharacterVisualLink as V;
    let (file, mut memory) = fixture();
    let target = visual_fixture(&mut memory);
    let checked = verified(&file);
    for (link, address, kind) in [
        (V::Model, 0xe10000, ObjectKind::CharacterModel),
        (V::Pose, 0xe20000, ObjectKind::PoseImporter),
        (V::AlternatePose, 0xe30000, ObjectKind::PoseImporter),
        (V::ClothPose, 0xe40000, ObjectKind::PoseImporter),
        (V::SkeletonMapper, 0xe50000, ObjectKind::SkeletonMapper),
    ] {
        let copy = object_catalog(&checked, &memory)
            .character_visual(target, link)
            .unwrap()
            .unwrap();
        assert_eq!(copy.character, target);
        assert_eq!(copy.link, link);
        assert_eq!(copy.address, address);
        assert_eq!(copy.kind, kind);
    }
}

#[test]
fn visuals_distinguish_missing_links_from_stale_identity_and_unexpected_subtypes() {
    use crate::character_visuals::CharacterVisualLink as V;
    let (file, _) = fixture();
    let checked = verified(&file);
    for (link, offset) in [
        (V::Model, 0x50),
        (V::Pose, 0x398),
        (V::AlternatePose, 0x3a0),
        (V::ClothPose, 0x3a8),
        (V::SkeletonMapper, 0x3b0),
    ] {
        let (_, mut memory) = fixture();
        let target = visual_fixture(&mut memory);
        let address = memory.read_u64(target.address + offset).unwrap();
        memory.put64(address, BASE + 0x2a3c890);
        assert!(
            object_catalog(&checked, &memory)
                .character_visual(target, link)
                .is_err()
        );
        memory.put64(target.address + offset, 0);
        assert_eq!(
            object_catalog(&checked, &memory)
                .character_visual(target, link)
                .unwrap(),
            None
        );
    }
    let (_, mut memory) = fixture();
    let target = visual_fixture(&mut memory);
    memory.put64(0x610008, target.native_identifier + 1);
    assert!(
        object_catalog(&checked, &memory)
            .character_visual(target, V::Model)
            .is_err()
    );
    memory.put64(BASE + 0x3d69ff8, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_visual(target, V::Model)
            .unwrap(),
        None
    );
}

#[test]
fn visuals_reject_truncation_alignment_and_observable_pointer_type_identity_changes() {
    use crate::character_visuals::CharacterVisualLink as V;
    let (file, _) = fixture();
    let checked = verified(&file);
    for (link, offset) in [
        (V::Model, 0x50),
        (V::Pose, 0x398),
        (V::AlternatePose, 0x3a0),
        (V::ClothPose, 0x3a8),
        (V::SkeletonMapper, 0x3b0),
    ] {
        let (_, mut memory) = fixture();
        let target = visual_fixture(&mut memory);
        let address = memory.read_u64(target.address + offset).unwrap();
        for watch in [target.address + offset, address, target.address + 8] {
            memory.slot_reads.set(0);
            memory.watch_address = watch;
            memory.change_slot = true;
            assert!(
                object_catalog(&checked, &memory)
                    .character_visual(target, link)
                    .is_err()
            );
        }
        memory.change_slot = false;
        memory.bytes.remove(&(address + 7));
        assert!(
            object_catalog(&checked, &memory)
                .character_visual(target, link)
                .is_err()
        );
        memory.put64(target.address + offset, address + 1);
        assert!(
            object_catalog(&checked, &memory)
                .character_visual(target, link)
                .is_err()
        );
    }
}

fn physics_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    let target = module_fixture(memory, CharacterModule::Physics);
    for (offset, values) in [
        (0x330, vec![2f32.to_bits(), 0.5f32.to_bits()]),
        (0x340, vec![3f32.to_bits(), 0.75f32.to_bits()]),
        (0xc8, vec![(-1f32).to_bits()]),
        (0x1c8, vec![10f32.to_bits()]),
    ] {
        for (index, bits) in values.into_iter().enumerate() {
            memory.put(0xd00000 + offset + index as u64 * 4, &bits.to_le_bytes());
        }
    }
    target
}

#[test]
fn character_scalar_snapshots_use_exact_typed_storage_and_selected_field_bits() {
    use crate::character_scalars::{CharacterScalar as F, CharacterScalarKey};
    let (file, mut memory) = fixture();
    let target = physics_fixture(&mut memory);
    assert_eq!(controller_fixture(&mut memory, ObjectKind::Player), target);
    let checked = verified(&file);
    for (field, address, value, kind, container) in [
        (F::AxisX, 0xe00000, 2.0, ObjectKind::PlayerController, 0),
        (F::AxisY, 0xe00000, 3.0, ObjectKind::PlayerController, 0),
        (F::AxisZ, 0xe00000, 4.0, ObjectKind::PlayerController, 0),
        (
            F::CharacterHitHeight,
            0xd00000,
            2.0,
            ObjectKind::CharacterPhysics,
            0xc00000,
        ),
        (
            F::CharacterHitRadius,
            0xd00000,
            0.5,
            ObjectKind::CharacterPhysics,
            0xc00000,
        ),
        (
            F::MapHitHeight,
            0xd00000,
            3.0,
            ObjectKind::CharacterPhysics,
            0xc00000,
        ),
        (
            F::MapHitRadius,
            0xd00000,
            0.75,
            ObjectKind::CharacterPhysics,
            0xc00000,
        ),
        (
            F::Weight,
            0xd00000,
            -1.0,
            ObjectKind::CharacterPhysics,
            0xc00000,
        ),
    ] {
        let key = CharacterScalarKey {
            character: target,
            field,
        };
        let copy = object_catalog(&checked, &memory)
            .character_scalar(key)
            .unwrap()
            .unwrap();
        assert_eq!(copy.key, key);
        assert_eq!(copy.storage.address, address);
        assert_eq!(copy.storage.kind, kind);
        assert_eq!(copy.storage.container_address, container);
        assert_eq!(f32::from_bits(copy.bits), value);
    }
}

#[test]
fn physics_copies_separate_dimensions_and_weights_without_reading_collision_objects() {
    let (file, mut memory) = fixture();
    let target = physics_fixture(&mut memory);
    let checked = verified(&file);
    let read = object_catalog(&checked, &memory)
        .character_physics(target)
        .unwrap()
        .unwrap();
    assert_eq!(read.collision_dimensions(), [[2.0, 0.5], [3.0, 0.75]]);
    assert_eq!(
        read.multiplied_dimensions(2.0).unwrap(),
        [[4.0, 1.0], [6.0, 1.5]]
    );
    assert_eq!(
        read.multiplied_dimensions(0.5).unwrap(),
        [[1.0, 0.25], [1.5, 0.375]]
    );
    assert_eq!(read.collision_dimensions(), [[2.0, 0.5], [3.0, 0.75]]);
    assert_eq!(f32::from_bits(read.weight_bits), -1.0);
    assert_eq!(f32::from_bits(read.initial_weight_bits), 10.0);
    for multiplier in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::MAX] {
        assert!(read.multiplied_dimensions(multiplier).is_err());
    }
    for (offset, bits) in [(0x330, 0x7fc0_1234u32), (0x334, 0x8000_0000), (0xc8, 1)] {
        memory.put(0xd00000 + offset, &bits.to_le_bytes());
    }
    let read = object_catalog(&checked, &memory)
        .character_physics(target)
        .unwrap()
        .unwrap();
    assert_eq!(read.character_collision_bits, [0x7fc0_1234, 0x8000_0000]);
    assert_eq!(read.weight_bits, 1);
    assert!(read.multiplied_dimensions(1.0).is_err());
}

#[test]
fn physics_rejects_payload_truncation_changes_and_replaced_module_ownership() {
    struct ChangeWithin<'a> {
        memory: &'a SparseMemory,
        address: u64,
        reads: Cell<u32>,
    }
    impl AddressSpace for ChangeWithin<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if address <= self.address && self.address < address + output.len() as u64 {
                let previous = self.reads.get();
                self.reads.set(previous + 1);
                if previous > 0 {
                    output[(self.address - address) as usize] ^= 1;
                }
            }
            Ok(())
        }
    }
    let (file, _) = fixture();
    let checked = verified(&file);
    for offset in [0xc8, 0x1c8, 0x330, 0x334, 0x340, 0x344] {
        let (_, mut memory) = fixture();
        let target = physics_fixture(&mut memory);
        memory.bytes.remove(&(0xd00000 + offset + 3));
        assert!(
            object_catalog(&checked, &memory)
                .character_physics(target)
                .is_err()
        );
        let (_, mut memory) = fixture();
        let target = physics_fixture(&mut memory);
        let changed = ChangeWithin {
            memory: &memory,
            address: 0xd00000 + offset,
            reads: Cell::new(0),
        };
        let objects = GameObjectCatalog {
            module: checked
                .bind(
                    &changed,
                    BASE,
                    &[CodeRange {
                        rva: 0x1000,
                        length: 1,
                    }],
                )
                .unwrap(),
        };
        assert!(objects.character_physics(target).is_err());
    }
    for address in [0xd00008, 0xc00068, 0x610190, 0x610008] {
        let (_, mut memory) = fixture();
        let target = physics_fixture(&mut memory);
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .character_physics(target)
                .is_err()
        );
    }
    let (_, mut memory) = fixture();
    let target = physics_fixture(&mut memory);
    memory.put64(0xc00068, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_physics(target)
            .unwrap(),
        None
    );
}

#[test]
fn npc_collision_fields_preserve_unsigned_weight_and_exact_scalar_source_offsets() {
    use crate::parameter_fields::{FieldEvidence, ParameterField as F, ScalarValue as V};
    for (field, offset, bytes, expected) in [
        (F::NpcHitHeight, 0x14, 2f32.to_le_bytes(), V::F32(2.0)),
        (F::NpcHitRadius, 0x18, 0.5f32.to_le_bytes(), V::F32(0.5)),
        (F::NpcWeight, 0x1c, u32::MAX.to_le_bytes(), V::U32(u32::MAX)),
        (
            F::NpcCharacterHitHeight,
            0x154,
            3f32.to_le_bytes(),
            V::F32(3.0),
        ),
        (
            F::NpcCharacterHitRadius,
            0x158,
            0.75f32.to_le_bytes(),
            V::F32(0.75),
        ),
    ] {
        let (file, mut memory) = fixture();
        field_row_fixture(&mut memory, 6);
        memory.put(PARAM_HEADER + 0x200 + offset, &bytes);
        let checked = verified(&file);
        let value = object_catalog(&checked, &memory)
            .experimental_parameter_field(field, 0, 99, 2)
            .unwrap()
            .unwrap();
        assert_eq!(value.value, expected);
        assert_eq!(value.evidence, FieldEvidence::DefinitionAndNative);
    }
}

fn module_fixture(
    memory: &mut SparseMemory,
    kind: CharacterModule,
) -> character_sets::CharacterIdentity {
    singleton_fixture(memory);
    memory.put64(0x610008, 0x1234_5678_9000_0002);
    memory.put64(0x610190, 0xc00000);
    let (index, table) = match kind {
        CharacterModule::Resistance => (4, 0x2a391d0),
        CharacterModule::TimeAct => (3, 0x2a37670),
        CharacterModule::SuperArmor => (8, 0x2a3df98),
        CharacterModule::Toughness => (9, 0x2a3ef30),
        CharacterModule::ActionFlags => (1, 0x2a32438),
        CharacterModule::ActionRequests => (0x10, 0x2a32780),
        CharacterModule::Damage => (0x13, 0x2a3a3e0),
        CharacterModule::Event => (0xb, 0x2a38368),
        CharacterModule::Talk => (0xa, 0x2a39350),
        CharacterModule::Physics => (0xd, 0x2a3c890),
        CharacterModule::Ride => (0x1d, 0x2a3d098),
        CharacterModule::ModelParameterModifier => (0x1a, 0x2a38608),
    };
    memory.put64(0xc00000 + index * 8, 0xd00000);
    memory.put64(0xd00000, BASE + table);
    memory.put64(0xd00008, 0x610000);
    character_sets::CharacterIdentity {
        address: 0x610000,
        world_address: 0x600000,
        native_identifier: 0x1234_5678_9000_0002,
        epoch: 3,
    }
}

#[test]
fn posture_copies_only_selected_words_for_six_exact_modules_and_preserves_raw_floats() {
    use crate::character_posture::PostureKind;
    use crate::character_scalars::{CharacterScalar, CharacterScalarKey};
    let (file, _) = fixture();
    let checked = verified(&file);
    for (kind, module, scalar, pairs) in [
        (
            PostureKind::SuperArmor,
            CharacterModule::SuperArmor,
            CharacterScalar::SuperArmorCurrent,
            [
                (0x2a310c8, 0x2a3dc58),
                (0x2a47090, 0x2a3de00),
                (0x2a7fbb0, 0x2a3df98),
            ],
        ),
        (
            PostureKind::Toughness,
            CharacterModule::Toughness,
            CharacterScalar::ToughnessCurrent,
            [
                (0x2a310c8, 0x2a3ebc0),
                (0x2a47090, 0x2a3ed70),
                (0x2a7fbb0, 0x2a3ef30),
            ],
        ),
    ] {
        for (character_table, module_table) in pairs {
            let (_, mut memory) = fixture();
            let target = module_fixture(&mut memory, module);
            memory.put64(target.address, BASE + character_table);
            memory.put64(0xd00000, BASE + module_table);
            let words = [(-0.0f32).to_bits(), 0x7fc01234, (-2.0f32).to_bits()];
            for (index, word) in words.iter().enumerate() {
                memory.put(0xd00010 + index as u64 * 4, &word.to_le_bytes());
            }
            let copied = object_catalog(&checked, &memory)
                .character_posture(target, kind)
                .unwrap()
                .unwrap();
            assert_eq!(
                [
                    copied.current_bits,
                    copied.previous_bits,
                    copied.reference_bits
                ],
                words
            );
            assert_eq!(copied.module.module, module);
            let selected = object_catalog(&checked, &memory)
                .character_scalar(CharacterScalarKey {
                    character: target,
                    field: scalar,
                })
                .unwrap()
                .unwrap();
            assert_eq!(selected.bits, words[0]);
            assert_eq!(selected.storage.address, 0xd00000);
            assert_eq!(selected.storage.container_address, 0xc00000);
            for (_, wrong_table) in pairs {
                if wrong_table == module_table {
                    continue;
                }
                memory.put64(0xd00000, BASE + wrong_table);
                assert!(
                    object_catalog(&checked, &memory)
                        .character_posture(target, kind)
                        .is_err()
                );
            }
        }
    }
}

#[test]
fn posture_rejects_truncation_changed_words_owner_slot_and_full_character_identity() {
    use crate::character_posture::PostureKind;
    let (file, _) = fixture();
    let checked = verified(&file);
    for (kind, module, slot) in [
        (
            PostureKind::SuperArmor,
            CharacterModule::SuperArmor,
            0xc00040,
        ),
        (PostureKind::Toughness, CharacterModule::Toughness, 0xc00048),
    ] {
        for watch in [
            0xd00010, 0xd00014, 0xd00018, 0xd00008, 0xd00000, slot, 0x610008, 0x610190,
        ] {
            let (_, mut memory) = fixture();
            let target = module_fixture(&mut memory, module);
            for offset in [0x10, 0x14, 0x18] {
                memory.put(0xd00000 + offset, &100f32.to_bits().to_le_bytes());
            }
            memory.watch_address = watch;
            memory.change_slot = true;
            assert!(
                object_catalog(&checked, &memory)
                    .character_posture(target, kind)
                    .is_err(),
                "{watch:x}"
            );
        }
        for removed in [0xd00010, 0xd00017, 0xd0001b, 0xd0000f, slot + 7] {
            let (_, mut memory) = fixture();
            let target = module_fixture(&mut memory, module);
            for offset in [0x10, 0x14, 0x18] {
                memory.put(0xd00000 + offset, &100f32.to_bits().to_le_bytes());
            }
            memory.bytes.remove(&removed);
            assert!(
                object_catalog(&checked, &memory)
                    .character_posture(target, kind)
                    .is_err()
            );
        }
        let (_, mut memory) = fixture();
        let target = module_fixture(&mut memory, module);
        memory.put64(slot, 0);
        assert_eq!(
            object_catalog(&checked, &memory)
                .character_posture(target, kind)
                .unwrap(),
            None
        );
    }
}

#[test]
fn action_reader_checks_real_module_slot_type_owner_and_selected_spans() {
    let (file, mut memory) = fixture();
    let target = module_fixture(&mut memory, CharacterModule::ActionRequests);
    for offset in [
        0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x40, 0x98, 0xa0, 0x108, 0x110, 0x118, 0x120, 0x128,
    ] {
        memory.put64(0xd00000 + offset, 1u64 << 63);
    }
    for offset in [0x90, 0xf4, 0x130, 0xfc, 0x100] {
        memory.put(0xd00000 + offset, &u32::MAX.to_le_bytes());
    }
    let checked = verified(&file);
    let observation = object_catalog(&checked, &memory)
        .character_actions(target)
        .unwrap()
        .unwrap();
    assert_eq!(observation.module.character, target);
    assert_eq!(observation.words.readback_new_presses, 1u64 << 63);
    assert_eq!(observation.words.queue_selector, -1);
    for (address, value) in [
        (0xd00008, 0x620000),
        (0xd00000, BASE + 0x2a38368),
        (0xc00080, 0xd00001),
    ] {
        let original = memory.read_u64(address).unwrap();
        memory.put64(address, value);
        assert!(
            object_catalog(&checked, &memory)
                .character_actions(target)
                .is_err()
        );
        memory.put64(address, original);
    }
    memory.put64(0xc00080, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_actions(target)
            .unwrap(),
        None
    );
}

#[test]
fn aim_input_reader_uses_exact_flag_module_and_refuses_missing_wrong_owner_or_changed_identity() {
    let (file, mut memory) = fixture();
    let target = module_fixture(&mut memory, CharacterModule::ActionFlags);
    memory.put64(0xd00040, 1u64 << 63 | 1 << 18);
    let checked = verified(&file);
    let copied = object_catalog(&checked, &memory)
        .character_aim_input(target)
        .unwrap()
        .unwrap();
    assert_eq!(copied.module.character, target);
    assert_eq!(copied.input_modifier_bits, 1u64 << 63 | 1 << 18);
    assert!(copied.manual_aim_input_admitted());
    // No unrequested queue, module10 or container neighbors are mapped by this fixture.
    for (address, value) in [
        (0xd00008, 0x620000),
        (0xd00000, BASE + 0x2a32780),
        (0xc00008, 0xd00001),
        (0x610008, target.native_identifier + 1),
    ] {
        let previous = memory.read_u64(address).unwrap();
        memory.put64(address, value);
        assert!(
            object_catalog(&checked, &memory)
                .character_aim_input(target)
                .is_err()
        );
        memory.put64(address, previous);
    }
    memory.bytes.remove(&0xd00040);
    assert!(
        object_catalog(&checked, &memory)
            .character_aim_input(target)
            .is_err()
    );
    memory.put64(0xc00008, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_aim_input(target)
            .unwrap(),
        None
    );
}

#[test]
fn animation_request_observation_copies_signed_words_without_neighbor_storage() {
    for table in [0x2a310c8u64, 0x2a47090, 0x2a7fbb0] {
        for (latched, requested) in [(-1i32, -1i32), (20012, 20010), (i32::MIN, i32::MAX)] {
            let (file, mut memory) = fixture();
            let target = module_fixture(&mut memory, CharacterModule::Event);
            memory.put64(target.address, BASE + table);
            memory.put(0xd00018, &latched.to_le_bytes());
            memory.put(0xd00020, &requested.to_le_bytes());
            // +10, +1c and +24 stay unmapped: request observation has no input/padding read.
            let checked = verified(&file);
            let result = object_catalog(&checked, &memory)
                .character_animation(target)
                .unwrap()
                .unwrap();
            assert_eq!(
                (result.latched_id, result.requested_id),
                (latched, requested)
            );
            assert_eq!(result.module.character, target);
            assert_eq!(result.module.module, CharacterModule::Event);
            memory.put64(0xc00058, 0);
            assert_eq!(
                object_catalog(&checked, &memory)
                    .character_animation(target)
                    .unwrap(),
                None
            );
        }
    }
}

#[test]
fn animation_requests_reject_foreign_storage_truncation_and_visible_changes() {
    for watched in [
        BASE + 0x3d69ff8,
        0x610008,
        0x610190,
        0xc00058,
        0xd00000,
        0xd00008,
        0xd00018,
        0xd00020,
    ] {
        for changed in [false, true] {
            let (file, mut memory) = fixture();
            let target = module_fixture(&mut memory, CharacterModule::Event);
            memory.put(0xd00018, &20012i32.to_le_bytes());
            memory.put(0xd00020, &(-1i32).to_le_bytes());
            if changed {
                memory.watch_address = watched;
                memory.change_slot = true;
            } else {
                memory.bytes.remove(&watched);
            }
            let checked = verified(&file);
            assert!(
                object_catalog(&checked, &memory)
                    .character_animation(target)
                    .is_err(),
                "{watched:#x} changed={changed}"
            );
        }
    }
    for (address, value) in [
        (0xc00058, 0xd00001),
        (0xc00058, u64::MAX - 7),
        (0xd00000, BASE + 0x2a38608),
        (0xd00008, 0x620000),
    ] {
        let (file, mut memory) = fixture();
        let target = module_fixture(&mut memory, CharacterModule::Event);
        memory.put(0xd00018, &1i32.to_le_bytes());
        memory.put(0xd00020, &2i32.to_le_bytes());
        memory.put64(address, value);
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .character_animation(target)
                .is_err()
        );
    }
}

fn controller_fixture(
    memory: &mut SparseMemory,
    owner: ObjectKind,
) -> character_sets::CharacterIdentity {
    let target = module_fixture(memory, CharacterModule::Physics);
    let (character, controller) = match owner {
        ObjectKind::Character => (0x2a310c8, 0x2a2c6a0),
        ObjectKind::Enemy => (0x2a47090, 0x2a2d200),
        ObjectKind::Player => (0x2a7fbb0, 0x2a2eb10),
        _ => panic!("invalid synthetic owner"),
    };
    memory.put64(target.address, BASE + character);
    memory.put64(target.address + 0x58, 0xe00000);
    memory.put64(0xe00000, BASE + controller);
    memory.put64(0xe00010, target.address);
    for (index, value) in [2f32, 3.0, 4.0].into_iter().enumerate() {
        memory.put(0xe002d4 + index as u64 * 4, &value.to_bits().to_le_bytes());
    }
    target
}

#[test]
fn controllers_copy_exact_subclass_and_preserve_non_numeric_native_scale_bits() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for (owner, controller) in [
        (ObjectKind::Character, ObjectKind::CharacterController),
        (ObjectKind::Enemy, ObjectKind::EnemyController),
        (ObjectKind::Player, ObjectKind::PlayerController),
    ] {
        let (_, mut memory) = fixture();
        let target = controller_fixture(&mut memory, owner);
        let bits = [0x80000000u32, 0x7fc01234, 1];
        for (i, value) in bits.into_iter().enumerate() {
            memory.put(0xe002d4 + i as u64 * 4, &value.to_le_bytes());
        }
        let result = object_catalog(&checked, &memory)
            .character_controller(target)
            .unwrap()
            .unwrap();
        assert_eq!(result.kind, controller);
        assert_eq!(result.address, 0xe00000);
        assert_eq!(result.character, target);
        assert_eq!(result.axis_scale_bits, bits);
        assert_eq!(result.axis_scale().map(f32::to_bits), bits);
        assert!(result.multiplied_axes(1.0).is_err());
    }
}

#[test]
fn controller_absence_is_distinct_from_cross_subclass_owner_staleness_and_alignment() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for address in [BASE + 0x3d69ff8, 0x610058] {
        let (_, mut memory) = fixture();
        let target = controller_fixture(&mut memory, ObjectKind::Player);
        memory.put64(address, 0);
        assert_eq!(
            object_catalog(&checked, &memory)
                .character_controller(target)
                .unwrap(),
            None
        );
    }
    for (address, value) in [
        (0x610008, 1),
        (0x610058, 0xe00004),
        (0xe00000, BASE + 0x2a2d200),
        (0xe00010, 0x620000),
    ] {
        let (_, mut memory) = fixture();
        let target = controller_fixture(&mut memory, ObjectKind::Player);
        memory.put64(address, value);
        assert!(
            object_catalog(&checked, &memory)
                .character_controller(target)
                .is_err()
        );
    }
}

#[test]
fn controller_copy_rejects_truncation_and_visible_scale_link_owner_or_identity_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for address in [0x610008u64, 0x610058, 0xe00000, 0xe00010, 0xe002d4] {
        for changed in [false, true] {
            let (_, mut memory) = fixture();
            let target = controller_fixture(&mut memory, ObjectKind::Player);
            if changed {
                memory.watch_address = address;
                memory.change_slot = true;
            } else {
                memory.bytes.remove(&address);
            }
            assert!(
                object_catalog(&checked, &memory)
                    .character_controller(target)
                    .is_err()
            );
        }
    }
    let (_, mut memory) = fixture();
    let target = controller_fixture(&mut memory, ObjectKind::Player);
    memory.bytes.remove(&0xe002df);
    assert!(
        object_catalog(&checked, &memory)
            .character_controller(target)
            .is_err()
    );
}

#[test]
fn controller_scale_math_uses_original_baseline_and_rejects_unrepresentable_results() {
    let (file, mut memory) = fixture();
    let target = controller_fixture(&mut memory, ObjectKind::Player);
    let checked = verified(&file);
    let original = object_catalog(&checked, &memory)
        .character_controller(target)
        .unwrap()
        .unwrap();
    assert_eq!(original.multiplied_axes(3.0).unwrap(), [6.0, 9.0, 12.0]);
    assert_eq!(original.multiplied_axes(0.5).unwrap(), [1.0, 1.5, 2.0]);
    assert_eq!(original.multiplied_axes(3.0).unwrap(), [6.0, 9.0, 12.0]);
    assert_eq!(original.axis_scale(), [2.0, 3.0, 4.0]);
    for multiplier in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY, f32::MAX] {
        assert!(original.multiplied_axes(multiplier).is_err());
    }
    let mut tiny = original;
    tiny.axis_scale_bits = [1; 3];
    assert!(tiny.multiplied_axes(0.1).is_err());
}

#[test]
fn module_observations_follow_only_selected_links_for_all_proven_character_types() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for kind in [
        CharacterModule::Talk,
        CharacterModule::Event,
        CharacterModule::Physics,
        CharacterModule::Ride,
        CharacterModule::ModelParameterModifier,
    ] {
        for table in [0x2a310c8u64, 0x2a47090, 0x2a7fbb0] {
            let (_, mut memory) = fixture();
            let target = module_fixture(&mut memory, kind);
            memory.put64(target.address, BASE + table);
            let result = object_catalog(&checked, &memory)
                .character_module(target, kind)
                .unwrap()
                .unwrap();
            assert_eq!(result.character, target);
            assert_eq!(result.module, kind);
            assert_eq!(result.container_address, 0xc00000);
            assert_eq!(result.address, 0xd00000);
        }
    }
}

#[test]
fn damage_module_pairs_exact_character_types_and_rechecks_every_identity_link() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for (character_table, damage_table) in [
        (0x2a310c8, 0x2a39a90),
        (0x2a47090, 0x2a3a0d8),
        (0x2a7fbb0, 0x2a3a3e0),
    ] {
        let (_, mut memory) = fixture();
        let target = module_fixture(&mut memory, CharacterModule::Damage);
        memory.put64(target.address, BASE + character_table);
        memory.put64(0xd00000, BASE + damage_table);
        let copy = object_catalog(&checked, &memory)
            .character_module(target, CharacterModule::Damage)
            .unwrap()
            .unwrap();
        assert_eq!(copy.character, target);
        assert_eq!(copy.address, 0xd00000);
        assert_eq!(copy.container_address, 0xc00000);
        // Only this slot, primary tables and +8 owner are mapped; attack data is absent.
        for other in [0x2a39a90, 0x2a3a0d8, 0x2a3a3e0, 0x2a3c890] {
            if other == damage_table {
                continue;
            }
            memory.put64(0xd00000, BASE + other);
            assert!(
                object_catalog(&checked, &memory)
                    .character_module(target, CharacterModule::Damage)
                    .is_err()
            );
        }
        memory.put64(0xd00000, BASE + damage_table);
        for watch in [
            target.address,
            target.address + 8,
            target.address + 0x190,
            0xc00098,
            0xd00000,
            0xd00008,
        ] {
            memory.watch_address = watch;
            memory.slot_reads.set(0);
            memory.change_slot = true;
            assert!(
                object_catalog(&checked, &memory)
                    .character_module(target, CharacterModule::Damage)
                    .is_err()
            );
        }
        memory.change_slot = false;
        for missing in [0xc00098, target.address + 0x190, BASE + 0x3d69ff8] {
            let saved = memory.read_u64(missing).unwrap();
            memory.put64(missing, 0);
            assert_eq!(
                object_catalog(&checked, &memory)
                    .character_module(target, CharacterModule::Damage)
                    .unwrap(),
                None
            );
            memory.put64(missing, saved);
        }
        memory.put64(0xd00008, target.address + 8);
        assert!(
            object_catalog(&checked, &memory)
                .character_module(target, CharacterModule::Damage)
                .is_err()
        );
        memory.put64(0xd00008, target.address);
        memory.bytes.remove(&0xd0000f);
        assert!(
            object_catalog(&checked, &memory)
                .character_module(target, CharacterModule::Damage)
                .is_err()
        );
    }
}

#[test]
fn module_observations_distinguish_absent_links_from_wrong_type_owner_and_identity() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for address in [BASE + 0x3d69ff8, 0x610190, 0xc00068] {
        let (_, mut memory) = fixture();
        let target = module_fixture(&mut memory, CharacterModule::Physics);
        memory.put64(address, 0);
        assert_eq!(
            object_catalog(&checked, &memory)
                .character_module(target, CharacterModule::Physics)
                .unwrap(),
            None
        );
    }
    for (address, value) in [
        (0x610008, 1),
        (0x610190, 0xc00004),
        (0xc00068, 0xd00004),
        (0xd00000, BASE + 0x2a3d098),
        (0xd00008, 0x620000),
    ] {
        let (_, mut memory) = fixture();
        let target = module_fixture(&mut memory, CharacterModule::Physics);
        memory.put64(address, value);
        assert!(
            object_catalog(&checked, &memory)
                .character_module(target, CharacterModule::Physics)
                .is_err()
        );
    }
}

#[test]
fn module_observations_reject_truncated_or_visibly_replaced_owner_chains() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for address in [0x610008u64, 0x610190, 0xc00068, 0xd00000, 0xd00008] {
        for changed in [false, true] {
            let (_, mut memory) = fixture();
            let target = module_fixture(&mut memory, CharacterModule::Physics);
            if changed {
                memory.watch_address = address;
                memory.change_slot = true;
            } else {
                memory.bytes.remove(&address);
            }
            assert!(
                object_catalog(&checked, &memory)
                    .character_module(target, CharacterModule::Physics)
                    .is_err()
            );
        }
    }
}

fn bullet_fixture(memory: &mut SparseMemory) -> (u64, u64) {
    let (manager, pool, spill) = (0x2000000, 0x2100000, 0x2300000);
    let fixed = pool + 127 * 0xe30;
    memory.put64(BASE + 0x3d667a8, manager);
    memory.put64(manager, pool);
    memory.put64(manager + 8, fixed);
    memory.put(manager + 0x18, &1u32.to_le_bytes());
    memory.put(manager + 0x1c, &2u32.to_le_bytes());
    for (bullet, next, identifier) in [
        (fixed, spill, 0x123456783001007f),
        (spill, 0, 0x98765432300200fe),
    ] {
        memory.put64(bullet, BASE + 0x2a28ec0);
        memory.put64(bullet + 8, identifier);
        memory.put64(bullet + 0xe18, next);
        memory.put64(bullet + 0x4c0, 0xfeedbeef10000003);
        memory.put64(bullet + 0xb30, BASE + 0x2a2a560);
        memory.put64(bullet + 0xb38, bullet);
        memory.put64(bullet + 0xb40, 0xcafef00d10000004);
        for (offset, bits) in [
            (
                0x10,
                [1f32.to_bits(), 2f32.to_bits(), 3f32.to_bits(), 0x7fc12345],
            ),
            (
                0x30,
                [4f32.to_bits(), 5f32.to_bits(), 6f32.to_bits(), 0x7fc23456],
            ),
        ] {
            for (i, bit) in bits.into_iter().enumerate() {
                memory.put(bullet + offset + i as u64 * 4, &bit.to_le_bytes());
            }
        }
        memory.put(bullet + 0xb24, &0.025f32.to_bits().to_le_bytes());
        for (offset, table, id) in [
            (0xd90, 0x2a2a4b0, -1i32),
            (0xdc0, 0x2a28530, 421),
            (0xdf0, 0x2a28470, 87),
        ] {
            memory.put64(bullet + offset, BASE + table);
            memory.put64(bullet + offset + 8, 0xdeadbeef00000008);
            memory.put(bullet + offset + 0x10, &id.to_le_bytes());
        }
    }
    (fixed, spill)
}

#[test]
fn bullets_copy_sparse_fixed_and_spill_slots_without_unknown_resources_or_wildcards() {
    let (file, mut memory) = fixture();
    let checked = verified(&file);
    let (fixed, spill) = bullet_fixture(&mut memory);
    let objects = object_catalog(&checked, &memory);
    let list = objects.bullets(7, 2).unwrap().unwrap();
    assert_eq!(list.members.len(), 2);
    assert_eq!(list.members[0].identity.address, fixed);
    assert_eq!(list.members[1].identity.address, spill);
    let member = list.members[0];
    assert_eq!(member.owner_identifier, 0xfeedbeef10000003);
    assert_eq!(member.targeting_owner_identifier, 0xcafef00d10000004);
    assert_eq!(member.finite_position().unwrap(), [1.0, 2.0, 3.0]);
    assert_eq!(member.finite_velocity().unwrap(), [4.0, 5.0, 6.0]);
    assert_eq!(member.wait.parameter_id, -1);
    assert_eq!(member.fly.parameter_id, 421);
    assert_eq!(member.explosion.parameter_id, 87);
    assert_eq!(member.elapsed_step_bits, 0.025f32.to_bits());
    assert_eq!(objects.bullet(member.identity, 2).unwrap(), Some(member));
    let mut stale = member.identity;
    stale.native_identifier ^= 1u64 << 32;
    assert_eq!(objects.bullet(stale, 2).unwrap(), None);
    stale.native_identifier = 0xffffffffffffffff;
    assert_eq!(objects.bullet(stale, 2).unwrap(), None);
    stale.module_base += 8;
    assert_eq!(objects.bullet(stale, 2).unwrap(), None);
    // Native retirement keeps the slot but alters its selector byte; list does not mean alive.
    memory.put64(fixed + 8, 0x123456783001ff7f);
    let retired = object_catalog(&checked, &memory)
        .bullets(7, 2)
        .unwrap()
        .unwrap();
    assert_eq!(
        retired.members[0].identity.native_identifier,
        0x123456783001ff7f
    );
    assert_eq!(
        object_catalog(&checked, &memory)
            .bullet(member.identity, 2)
            .unwrap(),
        None
    );
    memory.put64(0x2000008, 0);
    assert!(
        object_catalog(&checked, &memory)
            .bullets(7, 1)
            .unwrap()
            .unwrap()
            .members
            .is_empty()
    );
    memory.put64(BASE + 0x3d667a8, 0);
    assert_eq!(
        object_catalog(&checked, &memory).bullets(7, 1).unwrap(),
        None
    );
}

#[test]
fn bullets_reject_cycles_capacity_mismatches_stale_fields_and_truncated_storage() {
    let (file, mut memory) = fixture();
    let checked = verified(&file);
    let (fixed, spill) = bullet_fixture(&mut memory);
    for (epoch, limit) in [(0, 2), (7, 0), (7, 1), (7, 257)] {
        assert!(
            object_catalog(&checked, &memory)
                .bullets(epoch, limit)
                .is_err()
        );
    }
    for (address, replacement) in [
        (spill + 0xe18, fixed),
        (fixed + 8, 0x123456783001007e),
        (fixed + 8, 0x123456784001007f),
        (spill + 8, 0x98765432300200ff),
        (fixed + 0xdc0, BASE + 0x2a28470),
        (fixed, BASE + 0x2a310c8),
        (fixed + 0xb30, BASE + 0x2a28ec0),
        (fixed + 0xb38, spill),
        (0x2000000, 0x2100001),
        (0x2000008, fixed + 1),
    ] {
        bullet_fixture(&mut memory);
        memory.put64(address, replacement);
        assert!(
            object_catalog(&checked, &memory).bullets(7, 2).is_err(),
            "{address:x}"
        );
    }
    for count in [0u32, 129] {
        bullet_fixture(&mut memory);
        memory.put(0x2000018, &count.to_le_bytes());
        assert!(object_catalog(&checked, &memory).bullets(7, 2).is_err());
    }
    for watched in [
        BASE + 0x3d667a8,
        0x2000000,
        0x2000008,
        0x2000018,
        0x200001c,
        fixed,
        fixed + 8,
        fixed + 0xe18,
        fixed + 0x4c0,
        fixed + 0x10,
        fixed + 0x30,
        fixed + 0xb24,
        fixed + 0xb30,
        fixed + 0xb38,
        fixed + 0xb40,
        fixed + 0xd90,
        fixed + 0xd98,
        fixed + 0xda0,
        fixed + 0xdc0,
        fixed + 0xdc8,
        fixed + 0xdd0,
        fixed + 0xdf0,
        fixed + 0xdf8,
        fixed + 0xe00,
    ] {
        bullet_fixture(&mut memory);
        memory.watch_address = watched;
        memory.change_slot = true;
        memory.slot_reads.set(0);
        assert!(
            object_catalog(&checked, &memory).bullets(7, 2).is_err(),
            "{watched:x}"
        );
    }
    memory.change_slot = false;
    for address in [fixed + 0x38, fixed + 0xdd3, spill + 0xe1f] {
        bullet_fixture(&mut memory);
        memory.bytes.remove(&address);
        assert!(object_catalog(&checked, &memory).bullets(7, 2).is_err());
    }
}

#[test]
fn bullet_velocity_stores_preserve_neighbors_and_retain_changed_receipts() {
    use crate::bullets::{commit_velocity, velocity_plan};
    let (file, mut memory) = fixture();
    let checked = verified(&file);
    bullet_fixture(&mut memory);
    let source = object_catalog(&checked, &memory)
        .bullets(7, 2)
        .unwrap()
        .unwrap()
        .members[0];
    for current in [
        None,
        Some({
            let mut changed = source;
            changed.owner_identifier += 1;
            changed
        }),
    ] {
        assert!(velocity_plan(current, source, [1f32.to_bits(); 3]).is_err());
    }
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(velocity_plan(Some(source), source, [invalid.to_bits(); 3]).is_err());
    }
    #[repr(align(8))]
    struct Storage([u32; 12]);
    let mut storage = Storage([0x7fcabcd1; 12]);
    let mut copied = source;
    copied.identity.address = (&mut storage.0 as *mut [u32; 12] as u64) - 0x10;
    storage.0[8..12].copy_from_slice(&source.velocity_bits);
    let before = storage.0;
    let bits = [9f32, -0.0, -12.0].map(f32::to_bits);
    let plan = velocity_plan(Some(copied), copied, bits).unwrap();
    let mut after = copied;
    after.velocity_bits[..3].copy_from_slice(&bits);
    let write = commit_velocity(
        plan,
        |address, bits| {
            // SAFETY: Original aligned stack storage contains the exclusive twelve-byte destination.
            unsafe { (address as *mut [u32; 3]).write(bits) }
        },
        || Ok(Some(after)),
    );
    assert!(write.stored);
    assert_eq!(write.postflight.unwrap(), after);
    assert_eq!(&storage.0[..8], &before[..8]);
    assert_eq!(storage.0[11], before[11]);
    assert_eq!(&storage.0[8..11], &bits);
    let plan = velocity_plan(Some(after), after, bits).unwrap();
    assert!(
        !commit_velocity(
            plan,
            |_, _| panic!("unchanged should not store"),
            || Ok(Some(after))
        )
        .stored
    );
    let plan = velocity_plan(Some(copied), copied, bits).unwrap();
    let called = Cell::new(false);
    let failed = commit_velocity(
        plan,
        |_, _| called.set(true),
        || Err(Error::AddressOverflow),
    );
    assert!(called.get() && failed.stored && failed.postflight.is_err());
    assert_eq!(failed.source, copied);
}

fn bullet_spawn_fixture(memory: &mut SparseMemory, address: u64) {
    for (offset, length) in [
        (0, 0x48),
        (0x50, 0x59),
        (0xb0, 1),
        (0xb8, 0x10),
        (0xd0, 0x24),
        (0x100, 1),
    ] {
        let bytes: Vec<u8> = (0..length).map(|i| (offset + i) as u8).collect();
        memory.put(address + offset, &bytes);
    }
    memory.put64(address, 0x1234567810000003);
    memory.put(address + 0x14, &421i32.to_le_bytes());
    memory.put64(address + 0x20, 0xfeedbeef10000004);
    memory.put64(address + 0x98, 0x7000000);
    memory.put64(address + 0xa0, 0x7001000);
    memory.put64(address + 0xc0, 0x7002000);
    memory.put(address + 0x100, &[1]);
}
#[test]
fn bullet_spawn_capture_preserves_consumed_fields_while_skipping_padding_and_opaque_graphs() {
    let (file, mut memory) = fixture();
    let checked = verified(&file);
    let address = 0x2500000;
    bullet_spawn_fixture(&mut memory, address);
    let observed = object_catalog(&checked, &memory)
        .bullet_spawn_input(address, 7)
        .unwrap();
    assert_eq!(observed.input.owner_identifier(), 0x1234567810000003);
    assert_eq!(observed.input.target_identifier(), 0xfeedbeef10000004);
    assert_eq!(observed.input.bullet_parameter_id(), 421);
    assert_eq!(
        observed.input.reference_addresses(),
        [0x7000000, 0x7001000, 0x7002000]
    );
    assert_eq!(std::mem::align_of_val(&observed.input), 16);
    assert_eq!(std::mem::size_of_val(&observed.input), 0x110);
    let clone = observed.input.with_target(0x1234567810000006);
    assert_eq!(clone.owner_identifier(), observed.input.owner_identifier());
    assert_eq!(
        clone.reference_addresses(),
        observed.input.reference_addresses()
    );
    assert_eq!(
        clone.bullet_parameter_id(),
        observed.input.bullet_parameter_id()
    );
    assert_eq!(clone.target_identifier(), 0x1234567810000006);
    for (bad_address, epoch) in [(0, 7), (address + 8, 7), (address, 0)] {
        assert!(
            object_catalog(&checked, &memory)
                .bullet_spawn_input(bad_address, epoch)
                .is_err()
        );
    }
    for missing in [0x14, 0x27, 0x44, 0x8f, 0xa8, 0xb0, 0xc7, 0xf3, 0x100] {
        bullet_spawn_fixture(&mut memory, address);
        memory.bytes.remove(&(address + missing));
        assert!(
            object_catalog(&checked, &memory)
                .bullet_spawn_input(address, 7)
                .is_err()
        );
    }
    bullet_spawn_fixture(&mut memory, address);
    for offset in [0, 0x50, 0xb0, 0xb8, 0xd0, 0x100] {
        memory.watch_address = address + offset;
        memory.change_slot = true;
        memory.slot_reads.set(0);
        assert!(
            object_catalog(&checked, &memory)
                .bullet_spawn_input(address, 7)
                .is_err()
        );
    }
}
#[test]
fn bullet_spawn_target_writes_preserve_all_other_input_and_report_postflight_failures() {
    use crate::bullet_spawn::{commit_target, target_plan};
    let (file, mut memory) = fixture();
    let checked = verified(&file);
    #[repr(align(16))]
    struct Input([u8; 0x110]);
    let mut storage = Input([0x55; 0x110]);
    let address = &mut storage.0 as *mut [u8; 0x110] as u64;
    bullet_spawn_fixture(&mut memory, address);
    let source = object_catalog(&checked, &memory)
        .bullet_spawn_input(address, 7)
        .unwrap();
    storage.0[0x20..0x28].copy_from_slice(&source.input.target_identifier().to_le_bytes());
    let before = storage.0;
    let target = 0x9876543210000008;
    let plan = target_plan(source.clone(), source.clone(), target).unwrap();
    let mut after = source.clone();
    after.input = after.input.with_target(target);
    let written = commit_target(
        plan,
        |address, bits| {
            // SAFETY: Exclusive aligned stack storage owns this initialized eight-byte destination.
            unsafe { (address as *mut u64).write(bits) }
        },
        || Ok(after.clone()),
    );
    assert!(written.stored);
    assert_eq!(written.postflight.unwrap(), after);
    assert_eq!(&storage.0[..0x20], &before[..0x20]);
    assert_eq!(&storage.0[0x28..], &before[0x28..]);
    let stale = source.clone();
    assert!(target_plan(after.clone(), stale, target).is_err());
    let unchanged = target_plan(after.clone(), after.clone(), target).unwrap();
    assert!(
        !commit_target(
            unchanged,
            |_, _| panic!("unchanged should not store"),
            || Ok(after)
        )
        .stored
    );
    let plan = target_plan(source.clone(), source, target).unwrap();
    let called = Cell::new(false);
    let failed = commit_target(
        plan,
        |_, _| called.set(true),
        || Err(Error::AddressOverflow),
    );
    assert!(called.get() && failed.stored && failed.postflight.is_err());
}

#[test]
fn bullet_spawn_native_abi_preserves_two_outputs_once_even_after_postflight_failure() {
    use crate::bullet_spawn::{BulletSpawnInput, invoke_spawn};
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    unsafe extern "win64" fn native(
        manager: u64,
        output: *mut u32,
        input: *const BulletSpawnInput,
        status: *mut u32,
    ) -> *mut u32 {
        assert_eq!(manager, 0x2000000);
        assert!((input as usize).is_multiple_of(16));
        // SAFETY: invoke_spawn supplies retained aligned capture and initialized local outputs.
        unsafe {
            assert_eq!((*input).target_identifier(), 0xfeedbeef10000004);
            assert_eq!(
                (*input).reference_addresses(),
                [0x7000000, 0x7001000, 0x7002000]
            );
            assert_eq!(*output, u32::MAX);
            assert_eq!(*status, u32::MAX);
            output.write(0x1234003f);
            status.write(0);
        }
        CALLS.fetch_add(1, Ordering::SeqCst);
        output
    }
    unsafe extern "win64" fn unchanged(
        _: u64,
        output: *mut u32,
        _: *const BulletSpawnInput,
        _: *mut u32,
    ) -> *mut u32 {
        output
    }
    unsafe extern "win64" fn foreign(
        _: u64,
        _: *mut u32,
        _: *const BulletSpawnInput,
        _: *mut u32,
    ) -> *mut u32 {
        std::ptr::null_mut()
    }
    let (file, mut memory) = fixture();
    let checked = verified(&file);
    bullet_spawn_fixture(&mut memory, 0x2500000);
    let source = object_catalog(&checked, &memory)
        .bullet_spawn_input(0x2500000, 7)
        .unwrap();
    let count = CALLS.load(Ordering::SeqCst);
    // SAFETY: Original synthetic ABI receives only test-owned initialized fields; no game call.
    let receipt = unsafe {
        invoke_spawn(native, 0x2000000, &source.input, || {
            Err(Error::AddressOverflow)
        })
    };
    assert_eq!(CALLS.load(Ordering::SeqCst), count + 1);
    assert_eq!(receipt.native_output, 0x1234003f);
    assert_eq!(receipt.native_status, 0);
    assert!(receipt.returned_output_pointer && receipt.postflight.is_err());
    // SAFETY: Same synthetic local-only ABI and valid output lifetime; no native side effects.
    let empty = unsafe { invoke_spawn(unchanged, 0x2000000, &source.input, || Ok(())) };
    assert_eq!(empty.native_output, u32::MAX);
    assert_eq!(empty.native_status, u32::MAX);
    assert!(empty.postflight.is_ok());
    // SAFETY: Synthetic null return is inspected but never dereferenced; outputs are valid locals.
    let malformed = unsafe { invoke_spawn(foreign, 0x2000000, &source.input, || Ok(())) };
    assert!(!malformed.returned_output_pointer && malformed.postflight.is_err());
}

struct SparseMemory {
    bytes: BTreeMap<u64, u8>,
    slot_reads: Cell<usize>,
    change_slot: bool,
    watch_address: u64,
}

impl SparseMemory {
    fn put(&mut self, address: u64, bytes: &[u8]) {
        for (index, byte) in bytes.iter().enumerate() {
            self.bytes.insert(address + index as u64, *byte);
        }
    }
    fn put64(&mut self, address: u64, value: u64) {
        self.put(address, &value.to_le_bytes());
    }
}

impl AddressSpace for SparseMemory {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        for (index, byte) in output.iter_mut().enumerate() {
            *byte = *self
                .bytes
                .get(&(address + index as u64))
                .ok_or(Error::OutOfBounds { address, length: 1 })?;
        }
        if address == self.watch_address {
            let previous = self.slot_reads.get();
            self.slot_reads.set(previous + 1);
            if previous > 0 && self.change_slot {
                output.fill(0);
            }
        }
        Ok(())
    }
}

// Original synthetic identity bypasses the real-game gate only inside these private tests.
fn fixture() -> (Vec<u8>, SparseMemory) {
    let mut file = vec![0; 0x800];
    file[..2].copy_from_slice(b"MZ");
    put32(&mut file, 0x3c, 0x80);
    file[0x80..0x84].copy_from_slice(b"PE\0\0");
    file[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
    file[0x86..0x88].copy_from_slice(&3u16.to_le_bytes());
    file[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes());
    file[0x98..0x9a].copy_from_slice(&0x20bu16.to_le_bytes());
    file[0xb0..0xb8].copy_from_slice(&0x140000000u64.to_le_bytes());
    put32(&mut file, 0xd0, 0x4870000);
    put32(&mut file, 0xd4, 0x200);
    for (table, rva, raw, size, flags) in [
        (0x188, 0x1000, 0x200, 0x200, 0x60000020),
        (0x1b0, 0x3d50000, 0x400, 0xb20000, 0xc0000040),
        (0x1d8, 0x2000, 0x600, 0x200, 0x40000040),
    ] {
        for (offset, value) in [(8, size), (12, rva), (16, 0x200), (20, raw), (36, flags)] {
            put32(&mut file, table + offset, value);
        }
    }
    file[0x200] = 0xc3;
    let mut memory = SparseMemory {
        bytes: BTreeMap::new(),
        slot_reads: Cell::new(0),
        change_slot: false,
        watch_address: MANAGER + 8 + 168 * 8,
    };
    memory.put(BASE, &file[..0x200]);
    memory.put(BASE + 0x1000, &[0xc3]);
    memory.put64(BASE + GROUP_GLOBAL as u64, MANAGER);
    memory.put64(MANAGER, BASE + GROUP_MANAGER_VTABLE as u64);
    memory.put64(MANAGER + 8 + 168 * 8, GROUP);
    memory.put64(GROUP, BASE + GROUP_INSTANCE_VTABLE as u64);
    for (index, word) in [0x900000a8u32, 2, 3, 4].iter().enumerate() {
        memory.put(GROUP + 0x48 + index as u64 * 4, &word.to_le_bytes());
    }
    (file, memory)
}

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn verified(file: &[u8]) -> VerifiedFile<'_> {
    VerifiedFile::verify(
        file,
        ExecutableIdentity {
            name: "synthetic task test",
            file_size: file.len(),
            sha256: Sha256::digest(file).into(),
        },
    )
    .unwrap()
}

fn capacity_fixture() -> (Vec<u8>, SparseMemory) {
    let (mut file, mut memory) = fixture();
    // Large sparse virtual .text with original synthetic bytes only at queried ranges.
    file.resize(0x10ba800, 0);
    for (offset, value) in [(8, 0x10ba000), (12, 0xe83000), (16, 0x10ba000), (20, 0x800)] {
        put32(&mut file, 0x188 + offset, value);
    }
    for span in animation_capacity::CAPACITY_EVIDENCE {
        let raw = (span.rva - 0xe83000 + 0x800) as usize;
        file[raw..raw + span.length].fill(0x90);
        memory.put(BASE + span.rva as u64, &file[raw..raw + span.length]);
    }
    memory.put(BASE, &file[..0x200]);
    memory.put64(BASE + 0x3d69ff8, 0);
    (file, memory)
}

#[test]
fn capacity_prepare_is_read_only_and_manifest_accepts_only_complete_owned_replacements() {
    let (file, mut memory) = capacity_fixture();
    let checked = Ww171File {
        file: verified(&file),
    };
    let plan = checked.prepare_animation_capacity(&memory, BASE).unwrap();
    let before = memory.bytes.clone();
    // SAFETY: Original synthetic code/storage, no native objects or function calls.
    let owned = unsafe { plan.into_owned() };
    assert_eq!(memory.bytes, before);
    for patch in animation_capacity::animation_capacity_patches() {
        memory.put(BASE + patch.rva as u64, &patch.replacement);
    }
    checked
        .file()
        .bind_with_patches(
            &memory,
            BASE,
            animation_capacity::CAPACITY_EVIDENCE,
            Some(&owned),
        )
        .unwrap();
    assert!(checked.prepare_animation_capacity(&memory, BASE).is_err());
    memory.put(BASE + 0x15351c0, &[0xcc]);
    assert!(
        checked
            .file()
            .bind_with_patches(
                &memory,
                BASE,
                animation_capacity::CAPACITY_EVIDENCE,
                Some(&owned)
            )
            .is_err()
    );
}

#[test]
fn capacity_prepare_rejects_existing_world_missing_code_and_wrong_base() {
    let (file, mut memory) = capacity_fixture();
    let checked = Ww171File {
        file: verified(&file),
    };
    memory.put64(BASE + 0x3d69ff8, 0xdeadbeef);
    assert!(checked.prepare_animation_capacity(&memory, BASE).is_err());
    memory.put64(BASE + 0x3d69ff8, 0);
    assert!(
        checked
            .prepare_animation_capacity(&memory, BASE + 0x1000)
            .is_err()
    );
    memory.bytes.remove(&(BASE + 0x1462320));
    assert!(checked.prepare_animation_capacity(&memory, BASE).is_err());
}

#[test]
fn capacity_prepare_rechecks_world_and_transitive_unpatched_code() {
    struct AppearingWorld<'a>(&'a SparseMemory, Cell<usize>);
    impl AddressSpace for AppearingWorld<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.0.read_exact(address, output)?;
            if address == BASE + 0x3d69ff8 {
                let previous = self.1.get();
                self.1.set(previous + 1);
                if previous > 0 {
                    output[0] = 1;
                }
            }
            Ok(())
        }
    }
    let (file, mut memory) = capacity_fixture();
    let checked = Ww171File {
        file: verified(&file),
    };
    assert!(
        checked
            .prepare_animation_capacity(&AppearingWorld(&memory, Cell::new(0)), BASE)
            .is_err()
    );
    memory.put(BASE + 0x1ebd3f0, &[0xcc]);
    assert!(checked.prepare_animation_capacity(&memory, BASE).is_err());
}

fn catalog<'b, 'a>(
    file: &'b VerifiedFile<'a>,
    memory: &'b SparseMemory,
) -> TaskCatalog<'b, 'a, SparseMemory> {
    TaskCatalog {
        module: file
            .bind(
                memory,
                BASE,
                &[CodeRange {
                    rva: 0x1000,
                    length: 1,
                }],
            )
            .unwrap(),
    }
}

#[test]
fn observes_bounded_group_using_same_memory_interface_as_process_adapter() {
    let (file, memory) = fixture();
    let verified = verified(&file);
    let observation = catalog(&verified, &memory).observe(168).unwrap().unwrap();
    assert_eq!(observation.registration_index, 168);
    assert_eq!(observation.native_pointer, GROUP);
    assert_eq!(observation.native_words, [0x900000a8, 2, 3, 4]);
    assert!(catalog(&verified, &memory).observe(169).is_err());
}

#[test]
fn uninitialized_manager_or_slot_is_reported_without_pointer_dereference() {
    for target in [BASE + GROUP_GLOBAL as u64, MANAGER + 8 + 168 * 8] {
        let (file, mut memory) = fixture();
        memory.put64(target, 0);
        let verified = verified(&file);
        assert_eq!(catalog(&verified, &memory).observe(168).unwrap(), None);
    }
}

#[test]
fn mismatched_vtables_unreadable_fields_and_changed_slots_are_rejected() {
    for target in [MANAGER, GROUP] {
        let (file, mut memory) = fixture();
        memory.put64(target, 1);
        let verified = verified(&file);
        assert!(catalog(&verified, &memory).observe(168).is_err());
    }
    let (file, mut memory) = fixture();
    memory.bytes.remove(&(GROUP + 0x54));
    let verified = verified(&file);
    assert!(catalog(&verified, &memory).observe(168).is_err());
    let (_, mut memory) = fixture();
    memory.change_slot = true;
    assert!(catalog(&verified, &memory).observe(168).is_err());
}

#[test]
fn unknown_game_file_is_rejected_by_public_interface() {
    let (file, _) = fixture();
    assert!(matches!(
        Ww171File::verify(&file),
        Err(Error::UnsupportedImage { .. })
    ));
}

#[test]
fn invocation_copies_only_proven_fields_and_preserves_full_group_id() {
    let (file, mut memory) = fixture();
    let argument = 0x300000;
    memory.put64(argument, BASE + TIME_VTABLE as u64);
    memory.put(argument + 8, &0.125f32.to_le_bytes());
    // Native +12 padding deliberately has no mapped bytes in this fixture.
    memory.put(argument + 16, &0x900000a8u32.to_le_bytes());
    memory.put(argument + 20, &5u32.to_le_bytes());
    let verified = verified(&file);
    assert_eq!(
        catalog(&verified, &memory)
            .read_invocation(argument)
            .unwrap(),
        TaskInvocation {
            time_value: 0.125,
            group_id: 0x900000a8,
            scheduler_index: 5
        }
    );
}

#[test]
fn typed_callback_owns_copied_arguments_after_the_memory_adapter_is_gone() {
    use shikifsrs_runtime::callback::{InvocationOutcome, WaitOutcome};
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    let invocation = {
        let (file, mut memory) = fixture();
        let argument = 0x300000;
        memory.put64(argument, BASE + TIME_VTABLE as u64);
        memory.put(argument + 8, &0.125f32.to_le_bytes());
        memory.put(argument + 16, &0x900000a8u32.to_le_bytes());
        memory.put(argument + 20, &5u32.to_le_bytes());
        let verified = verified(&file);
        catalog(&verified, &memory)
            .read_invocation(argument)
            .unwrap()
    };
    let calls = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&calls);
    let callback = TaskCallback::new(move |value| captured.lock().unwrap().push(value));
    let control: TaskCallbackControl = callback.control();
    assert_eq!(callback.invoke(invocation), InvocationOutcome::Executed);
    assert_eq!(*calls.lock().unwrap(), vec![invocation]);
    control.cancel();
    assert_eq!(
        control.wait_stopped(Duration::ZERO),
        WaitOutcome::ResourcesReleased
    );
    assert_eq!(callback.invoke(invocation), InvocationOutcome::Stopped);
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[test]
fn invocation_rejects_null_wrong_type_and_unreadable_argument() {
    let (file, mut memory) = fixture();
    let argument = 0x300000;
    memory.put64(argument, BASE + GROUP_INSTANCE_VTABLE as u64);
    let verified = verified(&file);
    assert!(catalog(&verified, &memory).read_invocation(0).is_err());
    assert!(
        catalog(&verified, &memory)
            .read_invocation(argument)
            .is_err()
    );
    memory.put64(argument, BASE + TIME_VTABLE as u64);
    assert!(
        catalog(&verified, &memory)
            .read_invocation(argument)
            .is_err()
    );
    assert!(
        catalog(&verified, &memory)
            .read_invocation(u64::MAX - 7)
            .is_err()
    );
}

fn registry_fixture(memory: &mut SparseMemory) {
    let system = 0x400000;
    let registry = 0x500000;
    let head = 0x600000;
    let node = 0x700000;
    memory.put64(BASE + TASK_SYSTEM_GLOBAL as u64, system);
    memory.put64(system + 0x48, registry);
    memory.put64(registry, BASE + REGISTRY_VTABLE as u64);
    memory.put64(registry + 0x18, head);
    memory.put64(registry + 0x20, 1);
    memory.put64(head + 8, node);
    memory.put(head + 25, &[1]);
    memory.put64(node, head);
    memory.put64(node + 16, head);
    memory.put(node + 25, &[0]);
    memory.put(node + 32, &0x900000a8u32.to_le_bytes());
    memory.put64(node + 40, 0x800000);
}

#[test]
fn registered_group_uses_full_native_id_and_validated_registry_tree() {
    let (file, mut memory) = fixture();
    registry_fixture(&mut memory);
    let verified = verified(&file);
    let catalog = catalog(&verified, &memory);
    assert_eq!(
        catalog.registered_group(0x900000a8, 1).unwrap(),
        Some(0x800000)
    );
    assert_eq!(catalog.registered_group(0xa8, 1).unwrap(), None);
    assert_eq!(catalog.registered_group(0, 1).unwrap(), None);
    assert!(catalog.registered_group(0x900000a8, 0).is_err());
}

#[test]
fn registered_group_refuses_wrong_registry_type_or_missing_initialization() {
    for target in [BASE + TASK_SYSTEM_GLOBAL as u64, 0x400048, 0x700028] {
        let (file, mut memory) = fixture();
        registry_fixture(&mut memory);
        memory.put64(target, 0);
        let verified = verified(&file);
        assert_eq!(
            catalog(&verified, &memory)
                .registered_group(0x900000a8, 1)
                .unwrap(),
            None
        );
    }
    let (file, mut memory) = fixture();
    registry_fixture(&mut memory);
    memory.put64(0x500000, BASE + GROUP_INSTANCE_VTABLE as u64);
    let verified = verified(&file);
    assert!(
        catalog(&verified, &memory)
            .registered_group(0x900000a8, 1)
            .is_err()
    );
}

#[test]
fn changed_registry_and_invocation_types_are_rejected_at_the_end_of_read() {
    for address in [BASE + TASK_SYSTEM_GLOBAL as u64, 0x400048, 0x500000] {
        let (file, mut memory) = fixture();
        registry_fixture(&mut memory);
        memory.watch_address = address;
        memory.change_slot = true;
        let verified = verified(&file);
        assert!(
            catalog(&verified, &memory)
                .registered_group(0x900000a8, 1)
                .is_err()
        );
    }
    let (file, mut memory) = fixture();
    let argument = 0x300000;
    memory.put64(argument, BASE + TIME_VTABLE as u64);
    memory.put(argument + 8, &0.125f32.to_le_bytes());
    memory.put(argument + 16, &0x900000a8u32.to_le_bytes());
    memory.put(argument + 20, &5u32.to_le_bytes());
    memory.watch_address = argument;
    memory.change_slot = true;
    let verified = verified(&file);
    assert!(
        catalog(&verified, &memory)
            .read_invocation(argument)
            .is_err()
    );
}

fn phase_fixture() -> Vec<u8> {
    let (mut file, _) = fixture();
    for (index, name) in ["FrameBegin", "FrameEnd"].iter().enumerate() {
        let at = 0x600 + index * 32;
        put32(&mut file, at, index as u32);
        let label = 0x680 + index * 32;
        let address = 0x140002000u64 + (label - 0x600) as u64;
        file[at + 8..at + 16].copy_from_slice(&address.to_le_bytes());
        for (offset, word) in name.encode_utf16().chain([0]).enumerate() {
            file[label + offset * 2..label + offset * 2 + 2].copy_from_slice(&word.to_le_bytes());
        }
        put32(&mut file, at + 16, 0x90000000 + index as u32);
    }
    file
}

#[test]
fn phase_definitions_map_file_pointers_and_keep_raw_seed_words() {
    let file = phase_fixture();
    let image = shikifsrs_runtime::pe::FileImage::parse(&file).unwrap();
    let rows = phases::definitions(&image, 0x2000, 2).unwrap();
    assert_eq!(rows[0].name, "FrameBegin");
    assert_eq!(rows[1].name, "FrameEnd");
    assert_eq!(rows[1].phase.registration_index(), 1);
    assert_eq!(rows[1].native_seed_words, [0x90000001, 0, 0, 0]);
}

#[test]
fn native_phase_table_order_is_not_registration_order() {
    let mut file = phase_fixture();
    put32(&mut file, 0x600, 1);
    put32(&mut file, 0x620, 0);
    let image = shikifsrs_runtime::pe::FileImage::parse(&file).unwrap();
    let rows = phases::definitions(&image, 0x2000, 2).unwrap();
    assert_eq!(rows[0].name, "FrameEnd");
    assert_eq!(rows[0].phase.registration_index(), 0);
    assert_eq!(rows[1].name, "FrameBegin");
    assert_eq!(rows[1].phase.registration_index(), 1);
}

#[test]
fn malformed_phase_indices_names_and_pointers_are_rejected() {
    for variant in 0..8 {
        let mut file = phase_fixture();
        match variant {
            0 => put32(&mut file, 0x620, 0),
            1 => {
                let text = file[0x680..0x6a0].to_vec();
                file[0x6a0..0x6c0].copy_from_slice(&text);
            }
            2 => file[0x608..0x610].fill(0),
            3 => file[0x608..0x610].copy_from_slice(&0x140001000u64.to_le_bytes()),
            4 => file[0x608..0x610].copy_from_slice(&0x140002081u64.to_le_bytes()),
            5 => file[0x680..0x682].copy_from_slice(&0xd800u16.to_le_bytes()),
            6 => file[0x680..0x682].fill(0),
            7 => {
                for raw in file[0x680..].chunks_exact_mut(2) {
                    raw.copy_from_slice(&1u16.to_le_bytes());
                }
            }
            _ => unreachable!(),
        }
        let image = shikifsrs_runtime::pe::FileImage::parse(&file).unwrap();
        assert!(
            phases::definitions(&image, 0x2000, 2).is_err(),
            "variant {variant}"
        );
    }
    let file = phase_fixture();
    let image = shikifsrs_runtime::pe::FileImage::parse(&file).unwrap();
    assert!(phases::definitions(&image, 0x2000, 0).is_err());
    assert!(phases::definitions(&image, 0x2000, 170).is_err());
    assert!(phases::definitions(&image, 0x21f0, 2).is_err());
    assert!(phases::definitions(&image, 0x1000, 2).is_err());
}

#[test]
fn group_id_assignment_is_distinct_from_registration_index_and_activity() {
    let observation = GroupObservation {
        registration_index: 168,
        native_pointer: 0x200000,
        native_words: [1, 2, 3, 0],
    };
    assert_eq!(observation.native_group_id(), None);
    let assigned = GroupObservation {
        native_words: [1, 2, 3, 0x900000a8],
        ..observation
    };
    assert_eq!(assigned.native_group_id(), Some(0x900000a8));
}

fn registrations_fixture(memory: &mut SparseMemory, count: usize) {
    registry_fixture(memory);
    let group = 0x800000;
    let begin = 0x810000;
    memory.put64(group, BASE + REGISTERED_GROUP_VTABLE as u64);
    memory.put(group + 0x80, &0x900000a8u32.to_le_bytes());
    for (offset, value) in [
        (0x10, begin),
        (0x18, begin + count as u64 * 32),
        (0x20, begin + 4 * 32),
    ] {
        memory.put64(group + offset, value);
    }
    for index in 0..count {
        let entry = begin + index as u64 * 32;
        memory.put64(entry, BASE + REGISTRATION_RECORD_VTABLE as u64);
        memory.put64(entry + 8, 0x820000 + index as u64 * 8);
        memory.put64(entry + 16, if index == 0 { 0 } else { 0x830000 });
        memory.put(entry + 24, &0x900000a8u32.to_le_bytes());
        // No padding or task/callbacker object bytes are mapped.
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn character_clear_fixture(
    memory: &mut SparseMemory,
) -> character_tasks::CharacterTaskRegistration {
    registrations_fixture(memory, 1);
    memory.put64(BASE + 0x458fe88, 0xd00000);
    memory.put64(0xd00000, BASE + 0x2c04b00);
    memory.put64(0x400000, BASE + 0x3296e10);
    memory.put64(0x400040, 0xa00000);
    memory.put(0x400038, &[0]);
    memory.put(GROUP + 0x54, &0x900000a8u32.to_le_bytes());
    character_tasks::CharacterTaskRegistration {
        address: 0x820000,
        phase_index: 168,
        captures_character_task: true,
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn character_clear_preflight_requires_exact_initialized_manager_system_and_inserted_proxy() {
    let (file, mut memory) = fixture();
    let registration = character_clear_fixture(&mut memory);
    let checked = verified(&file);
    assert_eq!(
        current_character_tasks::clear_preconditions(
            &object_catalog(&checked, &memory),
            &catalog(&checked, &memory),
            registration
        )
        .unwrap(),
        (0xd00000, 0x900000a8)
    );
    let pending = character_tasks::CharacterTaskRegistration {
        address: 0x820010,
        ..registration
    };
    assert!(
        current_character_tasks::clear_preconditions(
            &object_catalog(&checked, &memory),
            &catalog(&checked, &memory),
            pending
        )
        .is_err()
    );
    memory.put64(0x800018, 0x810000); // Empty registered group, with allocation retained.
    assert!(
        current_character_tasks::clear_preconditions(
            &object_catalog(&checked, &memory),
            &catalog(&checked, &memory),
            registration
        )
        .is_err()
    );
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn character_clear_preflight_rejects_missing_or_contradictory_native_dependencies() {
    for (address, value, width) in [
        (BASE + 0x458fe88, 0, 8),
        (0xd00000, BASE + 0x2c049f8, 8),
        (BASE + 0x4861280, 0, 8),
        (0x400000, BASE + 0x2c04b00, 8),
        (0x400040, 0, 8),
        (0x400048, 0, 8),
        (0x500000, BASE + 0x2c049f8, 8),
        (0x400038, 2, 1),
        (BASE + GROUP_GLOBAL as u64, 0, 8),
        (GROUP + 0x54, 0, 4),
        (0x810008, 0x820010, 8),
    ] {
        let (file, mut memory) = fixture();
        let registration = character_clear_fixture(&mut memory);
        memory.put(address, &value.to_le_bytes()[..width]);
        let checked = verified(&file);
        assert!(
            current_character_tasks::clear_preconditions(
                &object_catalog(&checked, &memory),
                &catalog(&checked, &memory),
                registration
            )
            .is_err(),
            "{address:x}"
        );
    }
}

#[test]
fn registration_snapshot_copies_only_proven_fields_without_borrowing_tasks() {
    let (file, mut memory) = fixture();
    registrations_fixture(&mut memory, 2);
    let verified = verified(&file);
    let catalog = catalog(&verified, &memory);
    let rows = catalog
        .task_registrations(0x900000a8, 1, 2)
        .unwrap()
        .unwrap();
    assert_eq!(
        rows,
        [
            TaskRegistration {
                task_address: 0x820000,
                callbacker_address: None,
                group_id: 0x900000a8
            },
            TaskRegistration {
                task_address: 0x820008,
                callbacker_address: Some(0x830000),
                group_id: 0x900000a8
            },
        ]
    );
    assert_eq!(catalog.task_registrations(0xa8, 1, 2).unwrap(), None);
    assert!(catalog.task_registrations(0x900000a8, 1, 1).is_err());
}

#[test]
fn empty_registration_group_is_distinct_from_an_absent_group() {
    for empty_reserved in [false, true] {
        let (file, mut memory) = fixture();
        registrations_fixture(&mut memory, 0);
        if !empty_reserved {
            for offset in [0x10, 0x18, 0x20] {
                memory.put64(0x800000 + offset, 0);
            }
        }
        let verified = verified(&file);
        assert_eq!(
            catalog(&verified, &memory)
                .task_registrations(0x900000a8, 1, 1)
                .unwrap(),
            Some(vec![])
        );
    }
}

#[test]
fn registration_snapshot_rejects_bad_types_ids_null_and_duplicate_tasks() {
    for variant in 0..6 {
        let (file, mut memory) = fixture();
        registrations_fixture(&mut memory, 2);
        match variant {
            0 => memory.put64(0x800000, BASE + GROUP_INSTANCE_VTABLE as u64),
            1 => memory.put(0x800080, &0xa8u32.to_le_bytes()),
            2 => memory.put64(0x810000, BASE + REGISTERED_GROUP_VTABLE as u64),
            3 => memory.put64(0x810008, 0),
            4 => memory.put64(0x810028, 0x820000),
            5 => memory.put(0x810018, &0xa8u32.to_le_bytes()),
            _ => unreachable!(),
        }
        let verified = verified(&file);
        assert!(
            catalog(&verified, &memory)
                .task_registrations(0x900000a8, 1, 2)
                .is_err(),
            "variant {variant}"
        );
    }
}

#[test]
fn registration_snapshot_rejects_invalid_vector_ranges_limits_and_truncation() {
    for (offset, value) in [
        (0x10, 0),
        (0x10, 0x810041),
        (0x18, 0x810001),
        (0x18, 0x8100a0),
        (0x20, 0x810010),
        (0x20, 0x810081),
    ] {
        let (file, mut memory) = fixture();
        registrations_fixture(&mut memory, 1);
        memory.put64(0x800000 + offset, value);
        let verified = verified(&file);
        assert!(
            catalog(&verified, &memory)
                .task_registrations(0x900000a8, 1, 2)
                .is_err()
        );
    }
    let (file, mut memory) = fixture();
    registrations_fixture(&mut memory, 1);
    memory.bytes.remove(&0x810018);
    let verified = verified(&file);
    assert!(
        catalog(&verified, &memory)
            .task_registrations(0x900000a8, 1, 2)
            .is_err()
    );
    for limit in [0, 65_537] {
        assert!(
            catalog(&verified, &memory)
                .task_registrations(0x900000a8, 1, limit)
                .is_err()
        );
    }
}

#[test]
fn registration_snapshot_rejects_changes_to_headers_types_and_group_identity() {
    for target in [0x800000, 0x800010, 0x800018, 0x800020, 0x800080, 0x810000] {
        let (file, mut memory) = fixture();
        registrations_fixture(&mut memory, 1);
        memory.watch_address = target;
        memory.change_slot = true;
        let verified = verified(&file);
        assert!(
            catalog(&verified, &memory)
                .task_registrations(0x900000a8, 1, 2)
                .is_err(),
            "target {target:#x}"
        );
    }
}

#[test]
fn registration_snapshot_rejects_address_overflow() {
    let (file, mut memory) = fixture();
    registrations_fixture(&mut memory, 1);
    let overflow = u64::MAX - 16;
    memory.put64(0x700028, overflow);
    memory.put64(overflow, BASE + REGISTERED_GROUP_VTABLE as u64);
    let verified = verified(&file);
    assert!(matches!(
        catalog(&verified, &memory).task_registrations(0x900000a8, 1, 2),
        Err(Error::AddressOverflow)
    ));
}

#[test]
fn character_and_repository_interfaces_classify_only_proven_primary_types() {
    let (file, mut memory) = fixture();
    let address = 0x900000;
    let verified = verified(&file);
    for (table, kind) in [
        (0x2a310c8, ObjectKind::Character),
        (0x2a47090, ObjectKind::Enemy),
        (0x2a7fbb0, ObjectKind::Player),
    ] {
        memory.put64(address, BASE + table);
        let objects = GameObjectCatalog {
            module: catalog(&verified, &memory).module,
        };
        assert_eq!(objects.character(address).unwrap().unwrap().kind, kind);
        assert!(objects.solo_parameters(address).is_err());
    }
    memory.put64(address, BASE + 0x2a4e980);
    let objects = GameObjectCatalog {
        module: catalog(&verified, &memory).module,
    };
    assert_eq!(
        objects.world_characters(address).unwrap().unwrap().kind,
        ObjectKind::WorldCharacters
    );
    assert!(objects.character(address).is_err());
    memory.put64(address, BASE + 0x2bb84c8);
    let objects = GameObjectCatalog {
        module: catalog(&verified, &memory).module,
    };
    assert_eq!(
        objects.solo_parameters(address).unwrap().unwrap().kind,
        ObjectKind::SoloParameters
    );
    assert!(objects.world_characters(address).is_err());
}

#[test]
fn object_observation_rejects_unknown_null_truncated_and_changing_types() {
    let (file, mut memory) = fixture();
    let verified = verified(&file);
    let address = 0x900000;
    for table in [0, BASE - 8, BASE + TIME_VTABLE as u64] {
        memory.put64(address, table);
        let objects = GameObjectCatalog {
            module: catalog(&verified, &memory).module,
        };
        assert!(objects.character(address).is_err());
        assert_eq!(objects.character(0).unwrap(), None);
    }
    memory.put64(address, BASE + 0x2a7fbb0);
    memory.watch_address = address;
    memory.change_slot = true;
    let objects = GameObjectCatalog {
        module: catalog(&verified, &memory).module,
    };
    assert!(objects.character(address).is_err());
    memory.change_slot = false;
    memory.bytes.remove(&(address + 7));
    let objects = GameObjectCatalog {
        module: catalog(&verified, &memory).module,
    };
    assert!(objects.character(address).is_err());
}

#[test]
fn parameter_resource_copies_initialized_fields_without_interpreting_argument() {
    let (file, mut memory) = fixture();
    let address = 0x900000;
    memory.put64(address, BASE + 0x2bb29f8);
    memory.put(address + 0x78, &5u32.to_le_bytes());
    memory.put64(address + 0x80, 0xabcdef01); // Deliberately has no mapped target bytes.
    let verified = verified(&file);
    let objects = GameObjectCatalog {
        module: catalog(&verified, &memory).module,
    };
    let value = objects.parameter_resource(address).unwrap().unwrap();
    assert_eq!(value.native_storage_tag, 5);
    assert_eq!(value.native_constructor_argument, 0xabcdef01);
    assert_eq!(objects.parameter_resource(0).unwrap(), None);
}

#[test]
fn parameter_resource_rejects_truncation_overflow_and_changes_to_copied_fields() {
    for watched in [0x900078, 0x900080] {
        let (file, mut memory) = fixture();
        memory.put64(0x900000, BASE + 0x2bb29f8);
        memory.put(0x900078, &5u32.to_le_bytes());
        memory.put64(0x900080, 0xabcdef);
        memory.watch_address = watched;
        memory.change_slot = true;
        let verified = verified(&file);
        let objects = GameObjectCatalog {
            module: catalog(&verified, &memory).module,
        };
        assert!(objects.parameter_resource(0x900000).is_err());
    }
    let (file, mut memory) = fixture();
    memory.put64(0x900000, BASE + 0x2bb29f8);
    let verified = verified(&file);
    let objects = GameObjectCatalog {
        module: catalog(&verified, &memory).module,
    };
    assert!(objects.parameter_resource(0x900000).is_err());
    let overflow = u64::MAX - 0x7f;
    memory.put64(overflow, BASE + 0x2bb29f8);
    let objects = GameObjectCatalog {
        module: catalog(&verified, &memory).module,
    };
    assert_eq!(
        objects.parameter_resource(overflow).unwrap_err(),
        Error::AddressOverflow
    );
}

fn object_catalog<'b, 'a>(
    file: &'b VerifiedFile<'a>,
    memory: &'b SparseMemory,
) -> GameObjectCatalog<'b, 'a, SparseMemory> {
    GameObjectCatalog {
        module: catalog(file, memory).module,
    }
}

fn singleton_fixture(memory: &mut SparseMemory) {
    memory.put64(BASE + 0x3d69ff8, 0x600000);
    memory.put64(0x600000, BASE + 0x2a4e980);
    memory.put64(0x600000 + 0x1e508, 0x610000);
    memory.put64(0x610000, BASE + 0x2a7fbb0);
    memory.put64(BASE + 0x3d85f58, 0x800000);
    memory.put64(0x800000, BASE + 0x2bb84c8);
}

fn character_set_fixture(memory: &mut SparseMemory) -> u64 {
    singleton_fixture(memory);
    let set = 0x600000 + 0x10ee0;
    memory.put64(set, BASE + 0x2a41348);
    memory.put(set + 0x10, &3u32.to_le_bytes());
    memory.put64(set + 0x18, 0xd00000);
    memory.put(0xd00000, &[0u8; 48]);
    memory.put64(0xd00000, 0x610000);
    memory.put64(0xd00020, 0x620000);
    memory.put64(0x620000, BASE + 0x2a7fbb0);
    memory.put64(0x610008, 0xffffffff10000001);
    memory.put64(0x620008, 0x1234567810100002);
    set
}

fn character_task_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    singleton_fixture(memory);
    memory.put64(0x610008, 0xabcdef0100000001);
    let target = character_sets::CharacterIdentity {
        address: 0x610000,
        world_address: 0x600000,
        native_identifier: 0xabcdef0100000001,
        epoch: 1,
    };
    for (ordinal, (offset, callback)) in [
        (0x418, 0x3f3ff0),
        (0x448, 0x3f8700),
        (0x478, 0x3f9390),
        (0x4a8, 0x3f9470),
        (0x4d8, 0x3f92b0),
        (0x508, 0x3f8fd0),
    ]
    .into_iter()
    .enumerate()
    {
        let task = target.address + offset;
        memory.put64(task, BASE + 0x2a31090);
        memory.put64(task + 0x20, target.address);
        memory.put64(task + 0x28, BASE + callback);
        let registration = 0xf00000 + ordinal as u64 * 0x20;
        memory.put64(task + 0x10, registration);
        memory.put64(registration, BASE + 0x2c049f8);
        memory.put64(registration + 0x10, task);
        memory.put(registration + 0x18, &(ordinal as u32 * 30).to_le_bytes());
        // Unknown task base+8, pending flag/padding and proxy+8 remain unmapped.
    }
    target
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn clear_ticket_fixture(
    memory: &mut SparseMemory,
) -> character_task_clear::CharacterTaskClearTicket {
    let target = character_task_fixture(memory);
    character_clear_fixture(memory);
    memory.put64(target.address + 0x418 + 0x10, 0);
    character_task_clear::CharacterTaskClearTicket {
        target,
        ordinal: 0,
        phase_index: 168,
        group_id: 0x900000a8,
        previous_proxy: 0x820000,
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn character_clear_status_never_reads_old_proxy_and_distinguishes_registry_and_slot() {
    let (file, mut memory) = fixture();
    let ticket = clear_ticket_fixture(&mut memory);
    let checked = verified(&file);
    let status = object_catalog(&checked, &memory)
        .character_task_clear_status(ticket, &catalog(&checked, &memory), 1)
        .unwrap()
        .unwrap();
    assert!(status.previous_proxy_in_registry); // Old proxy has no mapped bytes.
    assert_eq!(status.slot_registration, None);
    memory.put64(0x800018, 0x810000);
    let status = object_catalog(&checked, &memory)
        .character_task_clear_status(ticket, &catalog(&checked, &memory), 1)
        .unwrap()
        .unwrap();
    assert!(!status.previous_proxy_in_registry);
    memory.put64(0x610428, 0xf00020); // Wrong capture is rejected, not a cleared observation.
    assert!(
        object_catalog(&checked, &memory)
            .character_task_clear_status(ticket, &catalog(&checked, &memory), 1)
            .is_err()
    );
    memory.put64(0x610428, 0xf00000); // Independently valid new registration.
    let status = object_catalog(&checked, &memory)
        .character_task_clear_status(ticket, &catalog(&checked, &memory), 1)
        .unwrap()
        .unwrap();
    assert_eq!(status.slot_registration.unwrap().address, 0xf00000);
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn character_clear_status_rejects_transition_missing_group_limits_and_changed_records() {
    for (address, value, width) in [
        (GROUP + 0x54, 0x900000a9u64, 4),
        (BASE + GROUP_GLOBAL as u64, 0, 8),
        (0x700028, 0, 8),
        (0x610008, 0xabcd, 8),
    ] {
        let (file, mut memory) = fixture();
        let ticket = clear_ticket_fixture(&mut memory);
        memory.put(address, &value.to_le_bytes()[..width]);
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .character_task_clear_status(ticket, &catalog(&checked, &memory), 1)
                .is_err(),
            "{address:x}"
        );
    }
    for watched in [0x810008, GROUP + 0x54] {
        let (file, mut memory) = fixture();
        let ticket = clear_ticket_fixture(&mut memory);
        memory.watch_address = watched;
        memory.change_slot = true;
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .character_task_clear_status(ticket, &catalog(&checked, &memory), 1)
                .is_err()
        );
    }
    let (file, mut memory) = fixture();
    let ticket = clear_ticket_fixture(&mut memory);
    let checked = verified(&file);
    for limit in [0, 65537] {
        assert!(
            object_catalog(&checked, &memory)
                .character_task_clear_status(ticket, &catalog(&checked, &memory), limit)
                .is_err()
        );
    }
    let (_, other_memory) = fixture();
    assert!(
        object_catalog(&checked, &memory)
            .character_task_clear_status(ticket, &catalog(&checked, &other_memory), 1)
            .is_err()
    );
    memory.put64(BASE + 0x3d69ff8, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_task_clear_status(ticket, &catalog(&checked, &memory), 1)
            .unwrap(),
        None
    );
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn character_clear_postflight_retains_tracking_ticket_on_read_failure_or_occupied_slot() {
    for slot_value in [Some(0), Some(0xface00), None] {
        let (_, mut memory) = fixture();
        let ticket = clear_ticket_fixture(&mut memory);
        if let Some(value) = slot_value {
            memory.put64(0x610428, value);
        } else {
            memory.bytes.remove(&0x61042f);
        }
        let result = current_character_tasks::clear_postflight(&memory, 0x610418, ticket);
        let current_character_tasks::CharacterTaskClear::Requested {
            ticket: retained,
            slot_clear,
        } = result
        else {
            panic!("submitted clear must retain a ticket");
        };
        assert_eq!(retained, ticket);
        assert_eq!(slot_clear.is_ok(), slot_value == Some(0));
    }
}

#[test]
fn character_tasks_copy_six_links_without_unknown_base_pending_or_padding_reads() {
    let (file, mut memory) = fixture();
    let target = character_task_fixture(&mut memory);
    let checked = verified(&file);
    let result = object_catalog(&checked, &memory)
        .character_tasks(target)
        .unwrap()
        .unwrap();
    assert_eq!(result.character, target);
    for (ordinal, task) in result.tasks.iter().enumerate() {
        assert_eq!(task.ordinal as usize, ordinal);
        let registration = task.registration.unwrap();
        assert_eq!(registration.address, 0xf00000 + ordinal as u64 * 0x20);
        assert_eq!(registration.phase_index, ordinal as u32 * 30);
        assert!(registration.captures_character_task);
    }
    memory.put64(0x610418 + 0x10, 0);
    memory.put64(0xf00020 + 0x10, 0);
    let result = object_catalog(&checked, &memory)
        .character_tasks(target)
        .unwrap()
        .unwrap();
    assert_eq!(result.tasks[0].registration, None);
    assert!(
        !result.tasks[1]
            .registration
            .unwrap()
            .captures_character_task
    );
}

#[test]
fn character_tasks_distinguish_missing_target_world_and_stale_full_identity() {
    let (file, mut memory) = fixture();
    let target = character_task_fixture(&mut memory);
    let checked = verified(&file);
    let stale = character_sets::CharacterIdentity {
        native_identifier: target.native_identifier ^ 1,
        ..target
    };
    assert!(
        object_catalog(&checked, &memory)
            .character_tasks(stale)
            .is_err()
    );
    let absent = character_sets::CharacterIdentity {
        address: 0,
        ..target
    };
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_tasks(absent)
            .unwrap(),
        None
    );
    memory.put64(BASE + 0x3d69ff8, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_tasks(target)
            .unwrap(),
        None
    );
}

#[test]
fn character_tasks_reject_wrong_types_owners_callbacks_capture_and_phase() {
    for (address, value, width) in [
        (0x610418, BASE + 0x2c049f8, 8),
        (0x610438, 0x610008, 8),
        (0x610440, BASE + 0x3f3ff1, 8),
        (0xf00000, BASE + 0x2a31090, 8),
        (0xf00010, 0x610448, 8),
        (0xf00018, 169, 4),
        (0x610428, 0xf00001, 8),
    ] {
        let (file, mut memory) = fixture();
        let target = character_task_fixture(&mut memory);
        memory.put(address, &value.to_le_bytes()[..width]);
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .character_tasks(target)
                .is_err(),
            "{address:x}"
        );
    }
}

#[test]
fn character_tasks_reject_truncated_observed_links_and_last_proxy_phase() {
    for missing in [0x61041f, 0x61043f, 0x610447, 0x61042f, 0xf00017, 0xf000bb] {
        let (file, mut memory) = fixture();
        let target = character_task_fixture(&mut memory);
        memory.bytes.remove(&missing);
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .character_tasks(target)
                .is_err(),
            "{missing:x}"
        );
    }
}

#[test]
fn character_tasks_reject_detectable_registration_capture_phase_owner_callback_changes() {
    for watched in [0x610428, 0xf00010, 0xf00038, 0x610438, 0x610440, 0x610008] {
        let (file, mut memory) = fixture();
        let target = character_task_fixture(&mut memory);
        memory.watch_address = watched;
        memory.change_slot = true;
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .character_tasks(target)
                .is_err(),
            "{watched:x}"
        );
    }
}

fn world_reference_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    singleton_fixture(memory);
    let target = character_sets::CharacterIdentity {
        address: 0xdead00,
        world_address: 0x600000,
        native_identifier: 0x123456789abcdef0,
        epoch: 7,
    }; // Target deliberately has no mapped bytes; detached storage is not dereferenced.
    memory.put64(0x600000 + 0x1e508, target.address);
    for (offset, begin, stride) in [
        (0x1f1b0, 0xe00000, 8),
        (0x1f1d0, 0xe10000, 16),
        (0x1f200, 0xe20000, 8),
    ] {
        // Allocator bytes and 16-byte record tails deliberately have no mapping.
        memory.put64(0x600000 + offset + 8, begin);
        memory.put64(0x600000 + offset + 16, begin + 3 * stride);
        memory.put64(0x600000 + offset + 24, begin + 8 * stride);
        memory.put64(begin, target.address);
        memory.put64(begin + stride, 0);
        memory.put64(begin + 2 * stride, target.address);
    }
    for (offset, node) in [
        (0x1e688, 0xe30000),
        (0x1e698, 0xe40000),
        (0x1e6a8, 0xe50000),
    ] {
        memory.put64(0x600000 + offset, node);
        memory.put64(node, target.address);
        memory.put64(node + 8, node + 16);
        memory.put64(node + 16, 0xabc008); // Another opaque, unmapped target.
        memory.put64(node + 24, 0);
    }
    target
}

#[test]
fn world_references_count_duplicates_without_target_allocator_or_record_tail_reads() {
    let (file, mut memory) = fixture();
    let target = world_reference_fixture(&mut memory);
    let checked = verified(&file);
    let result = object_catalog(&checked, &memory)
        .world_character_references(target, 15)
        .unwrap()
        .unwrap();
    assert_eq!(result.target, target);
    assert!(result.primary_slot_matches);
    assert_eq!(
        result.collections.map(|c| (c.visited, c.matches)),
        [(3, 2), (3, 2), (3, 2), (2, 1), (2, 1), (2, 1)]
    );
    assert!(
        object_catalog(&checked, &memory)
            .world_character_references(target, 14)
            .is_err()
    );
}

#[test]
fn world_references_allow_empty_storage_without_reading_capacity_or_tail() {
    let (file, mut memory) = fixture();
    let target = world_reference_fixture(&mut memory);
    for offset in [0x1f1b0, 0x1f1d0, 0x1f200] {
        for field in [8, 16, 24] {
            memory.put64(0x600000 + offset + field, 0);
        }
    }
    for offset in [0x1e688, 0x1e698, 0x1e6a8] {
        memory.put64(0x600000 + offset, 0);
    }
    memory.put64(0x600000 + 0x1e508, 0);
    let checked = verified(&file);
    let result = object_catalog(&checked, &memory)
        .world_character_references(target, 1)
        .unwrap()
        .unwrap();
    assert!(!result.primary_slot_matches);
    assert!(
        result
            .collections
            .iter()
            .all(|c| c.visited == 0 && c.matches == 0)
    );
    // Reserved, empty storage is valid; its allocation is never read.
    memory.put64(0x600000 + 0x1f1b8, 0xf00000);
    memory.put64(0x600000 + 0x1f1c0, 0xf00000);
    memory.put64(0x600000 + 0x1f1c8, 0xf10000);
    assert!(
        object_catalog(&checked, &memory)
            .world_character_references(target, 1)
            .is_ok()
    );
}

#[test]
fn world_references_reject_wrong_world_invalid_identity_and_invalid_limits() {
    let (file, mut memory) = fixture();
    let target = world_reference_fixture(&mut memory);
    let checked = verified(&file);
    for changed in [
        character_sets::CharacterIdentity {
            address: 0,
            ..target
        },
        character_sets::CharacterIdentity {
            address: target.address + 1,
            ..target
        },
        character_sets::CharacterIdentity {
            world_address: 0x700000,
            ..target
        },
        character_sets::CharacterIdentity { epoch: 0, ..target },
    ] {
        assert!(
            object_catalog(&checked, &memory)
                .world_character_references(changed, 15)
                .is_err()
        );
    }
    for limit in [0, 65537] {
        assert!(
            object_catalog(&checked, &memory)
                .world_character_references(target, limit)
                .is_err()
        );
    }
    memory.put64(BASE + 0x3d69ff8, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .world_character_references(target, 15)
            .unwrap(),
        None
    );
}

#[test]
fn world_references_reject_invalid_vector_order_alignment_stride_and_truncation() {
    for (address, value) in [
        (0x61f1c0, 0xdffff8),
        (0x61f1c8, 0xe00008),
        (0x61f1b8, 0xe00001),
        (0x61f1d8, 0),
        (0x61f1e0, 0xe10008),
    ] {
        let (file, mut memory) = fixture();
        let target = world_reference_fixture(&mut memory);
        memory.put64(address, value);
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .world_character_references(target, 15)
                .is_err()
        );
    }
    for missing in [0xe00007, 0xe10027, 0xe20017, 0xe3001f] {
        let (file, mut memory) = fixture();
        let target = world_reference_fixture(&mut memory);
        memory.bytes.remove(&missing);
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .world_character_references(target, 15)
                .is_err()
        );
    }
}

#[test]
fn world_references_reject_cycles_unaligned_nodes_and_overflow() {
    for (address, value) in [
        (0xe30018, 0xe30000),
        (0xe30018, 0xe30011),
        (0x61e688, u64::MAX - 7),
    ] {
        let (file, mut memory) = fixture();
        let target = world_reference_fixture(&mut memory);
        memory.put64(address, value);
        memory.put64(u64::MAX - 7, target.address);
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .world_character_references(target, 15)
                .is_err()
        );
    }
}

#[test]
fn world_references_reject_detectable_header_entry_node_primary_and_world_changes() {
    for watched in [
        0x61f1b8,
        0xe00000,
        0xe10020,
        0xe20000,
        0x61e688,
        0xe30000,
        0xe30008,
        0x61e508,
        BASE + 0x3d69ff8,
    ] {
        let (file, mut memory) = fixture();
        let target = world_reference_fixture(&mut memory);
        memory.watch_address = watched;
        memory.change_slot = true;
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .world_character_references(target, 15)
                .is_err(),
            "{watched:x}"
        );
    }
}

#[test]
fn character_set_preserves_holes_ordinals_and_full_opaque_identifiers() {
    let (file, mut memory) = fixture();
    let set = character_set_fixture(&mut memory);
    let file = verified(&file);
    let objects = object_catalog(&file, &memory);
    let players = objects.world_players(3).unwrap().unwrap();
    assert_eq!(players.world_address, 0x600000);
    assert_eq!(players.set.address, set);
    assert_eq!(players.set.capacity, 3);
    assert_eq!(
        players
            .set
            .characters
            .iter()
            .map(|entry| entry.ordinal)
            .collect::<Vec<_>>(),
        [0, 2]
    );
    assert_eq!(
        players.set.characters[0].native_identifier,
        0xffffffff10000001
    );
    assert_eq!(
        players.set.characters[1].native_identifier,
        0x1234567810100002
    );
    assert_eq!(players.set.characters[1].entry_address, 0xd00020);
}

fn registration_fixture(memory: &mut SparseMemory) -> (u64, character_sets::CharacterIdentity) {
    let set = character_set_fixture(memory);
    memory.put64(0x610010, 0xd00000);
    memory.put64(0x620010, 0xd00020);
    (
        set,
        character_sets::CharacterIdentity {
            address: 0x620000,
            world_address: 0x600000,
            native_identifier: 0x1234567810100002,
            epoch: 1,
        },
    )
}

#[test]
fn character_registration_resolves_the_matching_member_across_holes() {
    let (bytes, mut memory) = fixture();
    let (set, target) = registration_fixture(&mut memory);
    let file = verified(&bytes);
    let registration = object_catalog(&file, &memory)
        .character_registration(target, set, 3)
        .unwrap()
        .unwrap();
    assert_eq!(registration.character, target);
    assert_eq!(registration.set_address, set);
    assert_eq!(registration.entry_address, 0xd00020);
    assert_eq!(registration.ordinal, 2);
    // Entry counters/flags/padding are not part of this observation.
    for entry in [0xd00000, 0xd00010, 0xd00020] {
        for address in entry + 8..entry + 16 {
            memory.bytes.remove(&address);
        }
    }
    assert_eq!(
        object_catalog(&file, &memory)
            .character_registration(target, set, 3)
            .unwrap(),
        Some(registration)
    );
}

#[test]
fn character_registration_distinguishes_missing_member_and_set() {
    let (bytes, _) = fixture();
    let file = verified(&bytes);
    for missing in [0, 1, 2] {
        let (_, mut memory) = fixture();
        let (mut set, target) = registration_fixture(&mut memory);
        match missing {
            0 => memory.put64(0xd00020, 0),
            1 => set = 0,
            _ => memory.put64(BASE + 0x3d69ff8, 0),
        }
        assert_eq!(
            object_catalog(&file, &memory)
                .character_registration(target, set, 3)
                .unwrap(),
            None
        );
    }
}

#[test]
fn character_registration_rejects_contradictory_links_and_stale_identity() {
    let (bytes, _) = fixture();
    let file = verified(&bytes);
    for link in [0, 1, 0xd00000] {
        let (_, mut memory) = fixture();
        let (set, target) = registration_fixture(&mut memory);
        memory.put64(0x620010, link);
        assert!(
            object_catalog(&file, &memory)
                .character_registration(target, set, 3)
                .is_err()
        );
    }
    let (_, mut memory) = fixture();
    let (set, mut target) = registration_fixture(&mut memory);
    target.native_identifier ^= 1;
    assert!(
        object_catalog(&file, &memory)
            .character_registration(target, set, 3)
            .is_err()
    );
}

#[test]
fn character_registration_rejects_detectable_entry_link_and_membership_changes() {
    let (bytes, _) = fixture();
    let file = verified(&bytes);
    for watched in [0x620010, 0xd00000, 0x620008] {
        let (_, mut memory) = fixture();
        let (set, target) = registration_fixture(&mut memory);
        memory.watch_address = watched;
        memory.change_slot = true;
        assert!(
            object_catalog(&file, &memory)
                .character_registration(target, set, 3)
                .is_err()
        );
    }
}

#[test]
fn primary_player_identity_keeps_world_identifier_and_caller_epoch_without_claiming_a_lease() {
    let (bytes, mut memory) = fixture();
    character_set_fixture(&mut memory);
    let file = verified(&bytes);
    let identity = object_catalog(&file, &memory)
        .primary_player_identity(7)
        .unwrap()
        .unwrap();
    assert_eq!(identity.address, 0x610000);
    assert_eq!(identity.world_address, 0x600000);
    assert_eq!(identity.native_identifier, 0xffffffff10000001);
    assert_eq!(identity.epoch, 7);
    assert!(
        object_catalog(&file, &memory)
            .primary_player_identity(0)
            .is_err()
    );
    memory.put64(0x610008, 0x1234567810000001);
    assert_ne!(
        object_catalog(&file, &memory)
            .primary_player_identity(7)
            .unwrap()
            .unwrap(),
        identity
    );
    memory.slot_reads.set(0);
    memory.watch_address = 0x610008;
    memory.change_slot = true;
    assert!(
        object_catalog(&file, &memory)
            .primary_player_identity(7)
            .is_err()
    );
}

#[test]
fn character_set_empty_storage_and_absent_world_are_distinct() {
    let (file, mut memory) = fixture();
    let set = character_set_fixture(&mut memory);
    memory.put(set + 0x10, &0u32.to_le_bytes());
    memory.put64(set + 0x18, 0);
    let file = verified(&file);
    assert!(
        object_catalog(&file, &memory)
            .world_players(1)
            .unwrap()
            .unwrap()
            .set
            .characters
            .is_empty()
    );
    assert!(
        object_catalog(&file, &memory)
            .character_set(0, 1)
            .unwrap()
            .is_none()
    );
    memory.put64(BASE + 0x3d69ff8, 0);
    assert!(
        object_catalog(&file, &memory)
            .world_players(1)
            .unwrap()
            .is_none()
    );
}

#[test]
fn character_set_rejects_bad_counts_null_alignment_duplicates_and_truncation() {
    let (bytes, _) = fixture();
    let file = verified(&bytes);
    for kind in 0..7 {
        let (_, mut memory) = fixture();
        let set = character_set_fixture(&mut memory);
        match kind {
            0 => memory.put(set + 0x10, &(-1i32).to_le_bytes()),
            1 => memory.put(set + 0x10, &4u32.to_le_bytes()),
            2 => memory.put64(set + 0x18, 0),
            3 => memory.put64(set + 0x18, 0xd00001),
            4 => memory.put64(0xd00020, 0x610000),
            5 => {
                memory.bytes.remove(&0xd00027);
            }
            6 => memory.put64(0xd00020, 0x620001),
            _ => unreachable!(),
        }
        assert!(
            object_catalog(&file, &memory)
                .character_set(set, 3)
                .is_err(),
            "case {kind}"
        );
    }
    let (_, mut memory) = fixture();
    let set = character_set_fixture(&mut memory);
    assert!(
        object_catalog(&file, &memory)
            .character_set(set, 0)
            .is_err()
    );
    assert!(
        object_catalog(&file, &memory)
            .character_set(set, 65537)
            .is_err()
    );
}

#[test]
fn character_set_checks_primary_types_and_world_players_rejects_enemy_members() {
    let (bytes, mut memory) = fixture();
    let set = character_set_fixture(&mut memory);
    let file = verified(&bytes);
    memory.put64(0x620000, BASE + 0x2a47090);
    assert_eq!(
        object_catalog(&file, &memory)
            .character_set(set, 3)
            .unwrap()
            .unwrap()
            .characters[1]
            .character
            .kind,
        ObjectKind::Enemy
    );
    assert!(object_catalog(&file, &memory).world_players(3).is_err());
    memory.put64(set, BASE + 0x2a4b620);
    assert!(
        object_catalog(&file, &memory)
            .character_set(set, 3)
            .is_err()
    );
}

#[test]
fn character_set_rejects_detectable_storage_identifier_and_world_changes() {
    let (bytes, _) = fixture();
    let file = verified(&bytes);
    for watch in [
        0xd00000,
        0x610008,
        0x600000 + 0x10ee0 + 0x18,
        BASE + 0x3d69ff8,
    ] {
        let (_, mut memory) = fixture();
        character_set_fixture(&mut memory);
        memory.watch_address = watch;
        memory.change_slot = true;
        assert!(
            object_catalog(&file, &memory).world_players(3).is_err(),
            "watch {watch:#x}"
        );
    }
}

fn character_links_fixture(memory: &mut SparseMemory) {
    singleton_fixture(memory);
    memory.put64(0x610190, 0xc00000);
    memory.put64(0xc00000, 0xc01000);
    memory.put64(0xc01000, BASE + 0x2a380b8);
    memory.put64(0xc01008, 0x610000);
    memory.put64(0x610178, 0xc10000);
    memory.put64(0xc10000, BASE + 0x2a4d490);
    memory.put64(0xc10010, 0x610000);
    memory.put64(0xc10008, 0xc11000);
    memory.put(0xc11008, &899000i32.to_le_bytes());
    memory.put64(0xc11030, 0xc12000);
    memory.put(0xc12008, &899001i32.to_le_bytes());
    memory.put64(0xc12030, 0);
}

#[test]
fn character_data_copies_role_and_owner_and_distinguishes_absent_links() {
    let (file, mut memory) = fixture();
    character_links_fixture(&mut memory);
    let verified = verified(&file);
    let data = object_catalog(&verified, &memory)
        .character_data(0x610000)
        .unwrap()
        .unwrap();
    assert_eq!(data.address, 0xc01000);
    assert_eq!(data.owner_address, 0x610000);
    assert_eq!(data.container_address, 0xc00000);
    for target in [0x610190, 0xc00000] {
        let (_, mut memory) = fixture();
        character_links_fixture(&mut memory);
        memory.put64(target, 0);
        assert_eq!(
            object_catalog(&verified, &memory)
                .character_data(0x610000)
                .unwrap(),
            None
        );
    }
    assert_eq!(
        object_catalog(&verified, &memory)
            .character_data(0)
            .unwrap(),
        None
    );
}

#[test]
fn character_data_rejects_wrong_role_owner_unaligned_and_changed_links() {
    for (target, value) in [
        (0xc01000, BASE + 0x2a7fbb0),
        (0xc01008, 0x620000),
        (0x610190, 1),
        (0xc00000, 1),
    ] {
        let (file, mut memory) = fixture();
        character_links_fixture(&mut memory);
        memory.put64(target, value);
        let verified = verified(&file);
        assert!(
            object_catalog(&verified, &memory)
                .character_data(0x610000)
                .is_err()
        );
    }
    for target in [0x610190, 0xc00000, 0xc01000, 0xc01008, 0x610000] {
        let (file, mut memory) = fixture();
        character_links_fixture(&mut memory);
        memory.watch_address = target;
        memory.change_slot = true;
        let verified = verified(&file);
        assert!(
            object_catalog(&verified, &memory)
                .character_data(0x610000)
                .is_err()
        );
    }
}

fn vitals_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    character_links_fixture(memory);
    memory.put64(0x610008, 0x1234_5678_9000_0002);
    for (offset, current, maximum) in [(0x138, 0i32, 900i32), (0x148, 55, 99), (0x154, -50, 120)] {
        memory.put(0xc01000 + offset, &current.to_le_bytes());
        memory.put(0xc01000 + offset + 4, &maximum.to_le_bytes());
    }
    character_sets::CharacterIdentity {
        address: 0x610000,
        world_address: 0x600000,
        native_identifier: 0x1234_5678_9000_0002,
        epoch: 3,
    }
}

#[test]
fn character_vitals_copy_three_pairs_without_clamping_or_padding_reads() {
    let (file, mut memory) = fixture();
    let target = vitals_fixture(&mut memory);
    let checked = verified(&file);
    let result = object_catalog(&checked, &memory)
        .character_vitals(target)
        .unwrap()
        .unwrap();
    assert_eq!(result.character, target);
    assert_eq!(result.data.address, 0xc01000);
    assert_eq!(result.data.container_address, 0xc00000);
    assert_eq!(
        result.hp,
        character_vitals::CurrentMaximum {
            current: 0,
            maximum: 900
        }
    );
    assert_eq!(
        result.mp,
        character_vitals::CurrentMaximum {
            current: 55,
            maximum: 99
        }
    );
    assert_eq!(
        result.stamina,
        character_vitals::CurrentMaximum {
            current: -50,
            maximum: 120
        }
    );
    // The other initialized words between these pairs are deliberately unmapped.
    memory.put(0xc01138, &i32::MIN.to_le_bytes());
    memory.put(0xc0113c, &i32::MAX.to_le_bytes());
    let result = object_catalog(&checked, &memory)
        .character_vitals(target)
        .unwrap()
        .unwrap();
    assert_eq!(result.hp.current, i32::MIN);
    assert_eq!(result.hp.maximum, i32::MAX);
}

#[test]
fn character_vitals_distinguish_absence_from_stale_identity() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for missing in [BASE + 0x3d69ff8, 0x610190, 0xc00000] {
        let (_, mut memory) = fixture();
        let target = vitals_fixture(&mut memory);
        memory.put64(missing, 0);
        assert_eq!(
            object_catalog(&checked, &memory)
                .character_vitals(target)
                .unwrap(),
            None
        );
    }
    for stale in [0, 1, 2] {
        let (_, mut memory) = fixture();
        let mut target = vitals_fixture(&mut memory);
        match stale {
            0 => target.epoch = 0,
            1 => target.world_address += 8,
            _ => target.native_identifier ^= 1,
        }
        assert!(
            object_catalog(&checked, &memory)
                .character_vitals(target)
                .is_err()
        );
    }
}

#[test]
fn character_vitals_reject_truncated_pairs_wrong_owner_and_wrong_module() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for offset in [0x138, 0x148, 0x154] {
        let (_, mut memory) = fixture();
        let target = vitals_fixture(&mut memory);
        memory.bytes.remove(&(0xc01000 + offset + 7));
        assert!(
            object_catalog(&checked, &memory)
                .character_vitals(target)
                .is_err()
        );
    }
    for (address, wrong) in [(0xc01008, 0x620000), (0xc01000, BASE + 0x2a7fbb0)] {
        let (_, mut memory) = fixture();
        let target = vitals_fixture(&mut memory);
        memory.put64(address, wrong);
        assert!(
            object_catalog(&checked, &memory)
                .character_vitals(target)
                .is_err()
        );
    }
}

#[test]
fn character_vitals_reject_detectable_values_links_and_identifier_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for watched in [0xc01138, 0xc01148, 0xc01154, 0x610190, 0xc01008, 0x610008] {
        let (_, mut memory) = fixture();
        let target = vitals_fixture(&mut memory);
        memory.watch_address = watched;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .character_vitals(target)
                .is_err()
        );
    }
}

#[test]
fn effect_membership_matches_prefix_without_reading_an_unused_tail() {
    let (file, mut memory) = fixture();
    character_links_fixture(&mut memory);
    let verified = verified(&file);
    let objects = object_catalog(&verified, &memory);
    assert_eq!(
        objects.has_sp_effect(0x610000, 899000, 1).unwrap(),
        Some(true)
    );
    assert_eq!(
        objects.has_sp_effect(0x610000, 899001, 2).unwrap(),
        Some(true)
    );
    assert_eq!(objects.has_sp_effect(0x610000, -1, 2).unwrap(), Some(false));
    assert!(objects.has_sp_effect(0x610000, 899001, 1).is_err());
    assert_eq!(objects.has_sp_effect(0, 899000, 1).unwrap(), None);
    memory.bytes.remove(&0xc11030);
    assert_eq!(
        object_catalog(&verified, &memory)
            .has_sp_effect(0x610000, 899000, 1)
            .unwrap(),
        Some(true)
    );
    memory.put64(0xc10008, 0);
    assert_eq!(
        object_catalog(&verified, &memory)
            .has_sp_effect(0x610000, 899000, 1)
            .unwrap(),
        Some(false)
    );
    memory.put64(0x610178, 0);
    assert_eq!(
        object_catalog(&verified, &memory)
            .has_sp_effect(0x610000, 899000, 1)
            .unwrap(),
        None
    );
}

#[test]
fn effect_membership_rejects_cycles_truncation_bad_limits_and_metadata_changes() {
    for target in [0x610178, 0xc10008, 0xc11030] {
        let (file, mut memory) = fixture();
        character_links_fixture(&mut memory);
        memory.put64(target, 1);
        let verified = verified(&file);
        assert!(
            object_catalog(&verified, &memory)
                .has_sp_effect(0x610000, -1, 4)
                .is_err()
        );
    }
    let (file, mut memory) = fixture();
    character_links_fixture(&mut memory);
    memory.put64(0xc12030, 0xc11000);
    let checked_file = verified(&file);
    assert!(
        object_catalog(&checked_file, &memory)
            .has_sp_effect(0x610000, -1, 4)
            .is_err()
    );
    for limit in [0, 65537] {
        assert!(
            object_catalog(&checked_file, &memory)
                .has_sp_effect(0x610000, 899000, limit)
                .is_err()
        );
    }
    memory.bytes.remove(&0xc11008);
    assert!(
        object_catalog(&checked_file, &memory)
            .has_sp_effect(0x610000, -1, 4)
            .is_err()
    );
    for target in [
        0x610178, 0xc10000, 0xc10010, 0xc10008, 0xc11008, 0xc11030, 0xc12008, 0x610000,
    ] {
        let (file, mut memory) = fixture();
        character_links_fixture(&mut memory);
        memory.watch_address = target;
        memory.change_slot = true;
        let verified = verified(&file);
        assert!(
            object_catalog(&verified, &memory)
                .has_sp_effect(0x610000, -1, 4)
                .is_err()
        );
    }
}

#[test]
fn singleton_and_primary_player_interfaces_copy_checked_nullable_addresses() {
    let (file, mut memory) = fixture();
    singleton_fixture(&mut memory);
    let verified = verified(&file);
    let objects = object_catalog(&verified, &memory);
    assert_eq!(objects.world().unwrap().unwrap().address, 0x600000);
    assert_eq!(
        objects.parameter_repository().unwrap().unwrap().address,
        0x800000
    );
    assert_eq!(objects.primary_player().unwrap().unwrap().address, 0x610000);
    memory.put64(0x600000 + 0x1e508, 0);
    assert_eq!(
        object_catalog(&verified, &memory).primary_player().unwrap(),
        None
    );
    memory.put64(BASE + 0x3d69ff8, 0);
    memory.put64(BASE + 0x3d85f58, 0);
    let objects = object_catalog(&verified, &memory);
    assert_eq!(objects.world().unwrap(), None);
    assert_eq!(objects.primary_player().unwrap(), None);
    assert_eq!(objects.parameter_repository().unwrap(), None);
}

#[test]
fn singleton_and_primary_player_reject_wrong_types_changes_and_missing_bytes() {
    for (target, value) in [
        (0x600000, BASE + 0x2a7fbb0),
        (0x610000, BASE + 0x2a47090),
        (0x800000, BASE + 0x2bb29f8),
    ] {
        let (file, mut memory) = fixture();
        singleton_fixture(&mut memory);
        memory.put64(target, value);
        let verified = verified(&file);
        let objects = object_catalog(&verified, &memory);
        assert!(
            if target == 0x800000 {
                objects.parameter_repository()
            } else {
                objects.primary_player()
            }
            .is_err()
        );
    }
    for target in [
        BASE + 0x3d69ff8,
        BASE + 0x3d85f58,
        0x600000 + 0x1e508,
        0x610000,
    ] {
        let (file, mut memory) = fixture();
        singleton_fixture(&mut memory);
        memory.watch_address = target;
        memory.change_slot = true;
        let verified = verified(&file);
        let objects = object_catalog(&verified, &memory);
        assert!(
            if target == BASE + 0x3d85f58 {
                objects.parameter_repository()
            } else {
                objects.primary_player()
            }
            .is_err()
        );
    }
    let (file, mut memory) = fixture();
    singleton_fixture(&mut memory);
    memory.bytes.remove(&0x610007);
    let verified = verified(&file);
    assert!(object_catalog(&verified, &memory).primary_player().is_err());
}

const PARAM_HEADER: u64 = 0xb00000;
const PARAM_SLOT: u64 = 0x800080 + 12 * 72;

fn equipment_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    singleton_fixture(memory);
    memory.put64(0x610008, 0x1234_5678_9000_0002);
    memory.put64(0x610638, 0xc00004); // Valid native four-byte allocation alignment.
    memory.put(0xc0000c, &0xffff_ffffu32.to_le_bytes()); // Unknown arm style is preserved.
    for (index, selected) in [2u32, 1, 0, 1, 0, 1].into_iter().enumerate() {
        memory.put(0xc00010 + index as u64 * 4, &selected.to_le_bytes());
    }
    for index in 0..22u64 {
        memory.put(
            0xc00080 + index * 4,
            &(100001i32 + index as i32 * 100).to_le_bytes(),
        );
    }
    character_sets::CharacterIdentity {
        address: 0x610000,
        world_address: 0x600000,
        native_identifier: 0x1234_5678_9000_0002,
        epoch: 3,
    }
}

#[test]
fn player_equipment_copies_only_proven_fields_and_selects_the_native_weapon_pair() {
    let (file, mut memory) = fixture();
    let target = equipment_fixture(&mut memory);
    memory.put(0xc00080, &(-1i32).to_le_bytes());
    memory.put(0xc00084, &0i32.to_le_bytes());
    let checked = verified(&file);
    let result = object_catalog(&checked, &memory)
        .character_equipment(target)
        .unwrap()
        .unwrap();
    assert_eq!(result.native_arm_style, u32::MAX);
    assert_eq!(result.assembly_address, 0xc00004);
    assert_eq!(&result.parameter_ids[..2], &[-1, 0]);
    assert_eq!(result.parameter_ids[21], 102101);
    assert_eq!(
        result.selected_weapon_parameters().unwrap(),
        character_equipment::SelectedWeaponParameters {
            even_slot: 4,
            even_id: 100401,
            odd_slot: 3,
            odd_id: 100301
        }
    );
    let mut altered = result;
    for [even, odd] in [[0, 0], [1, 2], [2, 1]] {
        altered.native_selected_slots[0] = even;
        altered.native_selected_slots[1] = odd;
        let weapons = altered.selected_weapon_parameters().unwrap();
        assert_eq!(weapons.even_id, altered.parameter_ids[even as usize * 2]);
        assert_eq!(weapons.odd_id, altered.parameter_ids[odd as usize * 2 + 1]);
    }
    for [even, odd] in [[3, 0], [0, 3], [u32::MAX, 2], [2, u32::MAX]] {
        altered.native_selected_slots[0] = even;
        altered.native_selected_slots[1] = odd;
        assert!(altered.selected_weapon_parameters().is_err());
    }
}

#[test]
fn player_equipment_distinguishes_absence_from_wrong_type_stale_identity_and_alignment() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for absent in [0x610638u64, BASE + 0x3d69ff8] {
        let (_, mut memory) = fixture();
        let target = equipment_fixture(&mut memory);
        memory.put64(absent, 0);
        assert_eq!(
            object_catalog(&checked, &memory)
                .character_equipment(target)
                .unwrap(),
            None
        );
    }
    for (address, value) in [
        (0x610000u64, BASE + 0x2a47090),
        (0x610008, 1),
        (0x610638, 0xc00002),
    ] {
        let (_, mut memory) = fixture();
        let target = equipment_fixture(&mut memory);
        memory.put64(address, value);
        assert!(
            object_catalog(&checked, &memory)
                .character_equipment(target)
                .is_err()
        );
    }
}

#[test]
fn player_equipment_rejects_truncation_and_visible_field_link_or_identity_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for address in [
        0x610638u64,
        0x610008,
        0xc0000c,
        0xc00010,
        0xc00024,
        0xc00080,
        0xc000d4,
    ] {
        let (_, mut memory) = fixture();
        let target = equipment_fixture(&mut memory);
        memory.bytes.remove(&address);
        assert!(
            object_catalog(&checked, &memory)
                .character_equipment(target)
                .is_err()
        );
        let (_, mut memory) = fixture();
        let target = equipment_fixture(&mut memory);
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .character_equipment(target)
                .is_err()
        );
    }
}

#[test]
fn native_weapon_base_queries_preserve_zero_and_reject_negative_ids() {
    use character_equipment::weapon_base_parameter_id;
    for (input, output) in [
        (-1, None),
        (i32::MIN, None),
        (0, Some(0)),
        (99, Some(0)),
        (100, Some(100)),
        (100199, Some(100100)),
        (i32::MAX, Some(2147483600)),
    ] {
        assert_eq!(weapon_base_parameter_id(input), output);
    }
}

fn npc_association_fixture(
    memory: &mut SparseMemory,
    enemy: bool,
) -> character_sets::CharacterIdentity {
    field_row_fixture(memory, 6);
    memory.put64(0x610000, BASE + if enemy { 0x2a47090 } else { 0x2a7fbb0 });
    memory.put64(0x610008, 0x1234_5678_9000_0002);
    memory.put64(0x610000 + if enemy { 0x598 } else { 0x5f0 }, 0xc00000);
    memory.put64(0xc00000, PARAM_HEADER + 0x180);
    memory.put(0xc00008, &7i32.to_le_bytes());
    memory.put(0xc0000c, &9u32.to_le_bytes());
    memory.put(PARAM_HEADER + 8, &9u16.to_le_bytes());
    character_sets::CharacterIdentity {
        address: 0x610000,
        world_address: 0x600000,
        native_identifier: 0x1234_5678_9000_0002,
        epoch: 3,
    }
}

#[test]
fn npc_associations_cross_check_each_character_kind_and_repository_without_payload_reads() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for enemy in [false, true] {
        let (_, mut memory) = fixture();
        let target = npc_association_fixture(&mut memory, enemy);
        // No required bytes are mapped for the other subclass's handle or extra handle padding.
        memory.bytes.remove(&(PARAM_HEADER + 0x180));
        let result = object_catalog(&checked, &memory)
            .character_npc_parameter(target, 2)
            .unwrap()
            .unwrap();
        assert_eq!(result.id, 7);
        assert_eq!(result.data_version, 9);
        assert_eq!(result.handle_address, 0xc00000);
        assert_eq!(
            result.resolved.unwrap(),
            character_parameters::NpcParameterRow {
                resource_address: 0x900000,
                header_address: PARAM_HEADER,
                row: parameters::ParameterRow {
                    id: 7,
                    ordinal: 1,
                    address: PARAM_HEADER + 0x180
                }
            }
        );
    }
}

#[test]
fn npc_associations_distinguish_absence_unresolved_and_zero_id_fallback() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for (id, version, resource) in [(-1i32, 0u32, true), (8, 9, true), (7, 0, false)] {
        let (_, mut memory) = fixture();
        let target = npc_association_fixture(&mut memory, true);
        memory.put64(0xc00000, 0);
        memory.put(0xc00008, &id.to_le_bytes());
        memory.put(0xc0000c, &version.to_le_bytes());
        if !resource {
            memory.put64(0x800080 + 6 * 72 + 8, 0);
        }
        let result = object_catalog(&checked, &memory)
            .character_npc_parameter(target, 2)
            .unwrap()
            .unwrap();
        assert_eq!(result.id, id);
        assert_eq!(result.resolved, None);
    }
    for missing in [0x6105f0u64, BASE + 0x3d69ff8] {
        let (_, mut memory) = fixture();
        let target = npc_association_fixture(&mut memory, false);
        memory.put64(missing, 0);
        assert_eq!(
            object_catalog(&checked, &memory)
                .character_npc_parameter(target, 2)
                .unwrap(),
            None
        );
    }
    let (_, mut memory) = fixture();
    let target = npc_association_fixture(&mut memory, true);
    memory.put(PARAM_HEADER + 0x40 + 12, &0u32.to_le_bytes());
    memory.put(PARAM_HEADER + 0x710, &0u32.to_le_bytes());
    memory.put(0xc00008, &0u32.to_le_bytes());
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_npc_parameter(target, 2)
            .unwrap()
            .unwrap()
            .id,
        0
    );
}

#[test]
fn npc_associations_reject_stale_or_inconsistent_handle_and_row_metadata() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for (at, bytes) in [
        (0xc00000u64, (PARAM_HEADER + 0x200).to_le_bytes().to_vec()),
        (0xc00008, (-1i32).to_le_bytes().to_vec()),
        (0xc00008, 8u32.to_le_bytes().to_vec()),
        (0xc0000c, 0x1_0009u32.to_le_bytes().to_vec()),
        (PARAM_HEADER + 8, 8u16.to_le_bytes().to_vec()),
        (0x610598, 0xc00001u64.to_le_bytes().to_vec()),
        (0x610008, 1u64.to_le_bytes().to_vec()),
    ] {
        let (_, mut memory) = fixture();
        let target = npc_association_fixture(&mut memory, true);
        memory.put(at, &bytes);
        assert!(
            object_catalog(&checked, &memory)
                .character_npc_parameter(target, 2)
                .is_err()
        );
    }
    let (_, mut memory) = fixture();
    let target = npc_association_fixture(&mut memory, true);
    memory.put64(0xc00000, 0);
    assert!(
        object_catalog(&checked, &memory)
            .character_npc_parameter(target, 2)
            .is_err()
    );
}

#[test]
fn npc_associations_reject_bounds_truncation_and_detectable_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for limit in [0, 1, 65_536] {
        let (_, mut memory) = fixture();
        let target = npc_association_fixture(&mut memory, true);
        assert!(
            object_catalog(&checked, &memory)
                .character_npc_parameter(target, limit)
                .is_err()
        );
    }
    for address in [
        0x610598,
        0xc00000,
        0xc00008,
        0xc0000c,
        PARAM_HEADER + 8,
        PARAM_HEADER + 0x710,
    ] {
        let (_, mut memory) = fixture();
        let target = npc_association_fixture(&mut memory, true);
        memory.bytes.remove(&address);
        assert!(
            object_catalog(&checked, &memory)
                .character_npc_parameter(target, 2)
                .is_err()
        );
        let (_, mut memory) = fixture();
        let target = npc_association_fixture(&mut memory, true);
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .character_npc_parameter(target, 2)
                .is_err()
        );
    }
}

fn row_fixture(memory: &mut SparseMemory, version: u8, flags: u8) {
    singleton_fixture(memory);
    memory.put(PARAM_SLOT, &2u32.to_le_bytes());
    memory.put64(PARAM_SLOT + 8, 0x900000);
    memory.put64(PARAM_SLOT + 16, 0);
    memory.put64(0x900000, BASE + 0x2bb29f8);
    memory.put64(0x900080, 0xa00000);
    memory.put64(0xa00080, PARAM_HEADER);
    memory.put(PARAM_HEADER - 0x10, &0x301u32.to_le_bytes()); // Rounded to +0x310 by native lookup.
    memory.put(PARAM_HEADER - 0xc, &2u32.to_le_bytes());
    memory.put(PARAM_HEADER + 0xa, &2u16.to_le_bytes());
    memory.put(PARAM_HEADER + 0x2d, &[version, flags]);
    let mode = version & 0x7f;
    let wide = flags & 2 != 0 && (mode == 4 || (mode == 5 && version >= 0x80));
    let start = if mode == 2 { 0x30 } else { 0x40 };
    let stride = if wide { 24 } else { 12 };
    for (ordinal, id, offset) in [(0, 99u32, 0x200u64), (1, 7, 0x180)] {
        let descriptor = PARAM_HEADER + start + ordinal * stride;
        memory.put(descriptor, &id.to_le_bytes());
        if wide {
            memory.put64(descriptor + 8, offset);
        } else {
            memory.put(descriptor + 4, &(offset as u32).to_le_bytes());
        }
    }
    memory.put(PARAM_HEADER + 0x310, &7u32.to_le_bytes());
    memory.put(PARAM_HEADER + 0x314, &1u32.to_le_bytes());
    memory.put(PARAM_HEADER + 0x318, &99u32.to_le_bytes());
    memory.put(PARAM_HEADER + 0x31c, &0u32.to_le_bytes());
    memory.put(PARAM_HEADER + 0x180, &[10, 20, 30, 40]);
}

fn field_row_fixture(memory: &mut SparseMemory, slot: u16) {
    row_fixture(memory, 3, 0);
    let item = 0x800080 + slot as u64 * 72;
    memory.put(item, &2u32.to_le_bytes());
    memory.put64(item + 8, 0x900000);
    memory.put64(item + 16, 0);
    memory.put(PARAM_HEADER - 0x10, &0x701u32.to_le_bytes());
    memory.put(PARAM_HEADER + 0x710, &7u32.to_le_bytes());
    memory.put(PARAM_HEADER + 0x714, &1u32.to_le_bytes());
    memory.put(PARAM_HEADER + 0x718, &99u32.to_le_bytes());
    memory.put(PARAM_HEADER + 0x71c, &0u32.to_le_bytes());
    memory.put(PARAM_HEADER + 0x200, &[0; 512]);
}

#[test]
fn named_parameter_scalars_use_the_correct_table_and_preserve_width_sign_and_float() {
    use crate::parameter_fields::{FieldEvidence, ParameterField as F, ScalarValue as V};
    for (field, slot, offset, bytes, expected) in [
        (F::NpcTeamType, 6, 0x133, vec![255], V::U8(255)),
        (
            F::NpcHp,
            6,
            0x24,
            u32::MAX.to_le_bytes().to_vec(),
            V::U32(u32::MAX),
        ),
        (
            F::NpcMp,
            6,
            0x28,
            0x8000_0001u32.to_le_bytes().to_vec(),
            V::U32(0x8000_0001),
        ),
        (
            F::NpcStamina,
            6,
            0xf4,
            u16::MAX.to_le_bytes().to_vec(),
            V::U16(u16::MAX),
        ),
        (
            F::WeaponSpAttackCategory,
            0,
            0xee,
            1320u16.to_le_bytes().to_vec(),
            V::U16(1320),
        ),
        (
            F::NpcLockCamera,
            6,
            0x1e4,
            (-1i32).to_le_bytes().to_vec(),
            V::I32(-1),
        ),
        (
            F::CameraHeight,
            31,
            0xc,
            3.75f32.to_le_bytes().to_vec(),
            V::F32(3.75),
        ),
        (
            F::BuddyRiddenThink,
            128,
            0x14,
            (-1i32).to_le_bytes().to_vec(),
            V::I32(-1),
        ),
        (
            F::EffectStateInfo,
            15,
            0x156,
            293u16.to_le_bytes().to_vec(),
            V::U16(293),
        ),
    ] {
        let (file, mut memory) = fixture();
        field_row_fixture(&mut memory, slot);
        memory.put(PARAM_HEADER + 0x200 + offset, &bytes);
        let checked = verified(&file);
        let result = object_catalog(&checked, &memory)
            .experimental_parameter_field(field, 0, 99, 2)
            .unwrap()
            .unwrap();
        assert_eq!(result.value, expected);
        assert_eq!(result.row_address, PARAM_HEADER + 0x200);
        assert_eq!(
            result.evidence,
            if matches!(
                field,
                F::EffectStateInfo | F::NpcHp | F::NpcMp | F::NpcStamina | F::NpcTeamType
            ) {
                FieldEvidence::DefinitionAndNative
            } else {
                FieldEvidence::Definition
            }
        );
    }
}

#[test]
fn npc_team_field_reads_exact_prefix_without_neighbor_byte_or_padding() {
    use parameter_fields::{ParameterField, ScalarValue};
    let (file, _) = fixture();
    let checked = verified(&file);
    for byte in [0, 1, 2, 128, 255] {
        let (_, mut memory) = fixture();
        field_row_fixture(&mut memory, 6);
        memory.put(PARAM_HEADER + 0x333, &[byte]);
        memory.bytes.remove(&(PARAM_HEADER + 0x334));
        let observed = object_catalog(&checked, &memory)
            .experimental_parameter_field(ParameterField::NpcTeamType, 0, 99, 2)
            .unwrap()
            .unwrap();
        assert_eq!(observed.value, ScalarValue::U8(byte));
        memory.bytes.remove(&(PARAM_HEADER + 0x333));
        assert!(
            object_catalog(&checked, &memory)
                .experimental_parameter_field(ParameterField::NpcTeamType, 0, 99, 2)
                .is_err()
        );
    }
}

#[test]
fn named_parameter_reader_rejects_nonfinite_truncated_and_changed_rows() {
    use crate::parameter_fields::ParameterField;
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let (file, mut memory) = fixture();
        field_row_fixture(&mut memory, 31);
        memory.put(PARAM_HEADER + 0x200, &value.to_le_bytes());
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .experimental_parameter_field(ParameterField::CameraDistance, 0, 99, 2)
                .is_err()
        );
    }
    for corrupt in [false, true] {
        let (file, mut memory) = fixture();
        field_row_fixture(&mut memory, 31);
        if corrupt {
            memory.watch_address = PARAM_HEADER + 0x200;
            memory.change_slot = true;
        } else {
            memory.bytes.remove(&(PARAM_HEADER + 0x203));
        }
        let checked = verified(&file);
        // A changed payload cannot be detected atomically, but changed metadata must be rejected.
        if corrupt {
            memory.watch_address = PARAM_HEADER + 0xa;
        }
        assert!(
            object_catalog(&checked, &memory)
                .experimental_parameter_field(ParameterField::CameraDistance, 0, 99, 2)
                .is_err()
        );
    }
}

#[test]
fn named_parameter_reader_keeps_absence_variant_and_copy_bounds_explicit() {
    use crate::parameter_fields::ParameterField;
    let (file, mut memory) = fixture();
    field_row_fixture(&mut memory, 0);
    let checked = verified(&file);
    let objects = object_catalog(&checked, &memory);
    assert_eq!(
        objects
            .experimental_parameter_field(ParameterField::WeaponSpAttackCategory, 0, 8, 2)
            .unwrap(),
        None
    );
    assert_eq!(
        objects
            .experimental_parameter_field(ParameterField::WeaponSpAttackCategory, 1, 99, 2)
            .unwrap(),
        None
    );
    assert!(
        objects
            .experimental_parameter_field(ParameterField::WeaponSpAttackCategory, 0, 7, 2)
            .is_err()
    );
}

#[test]
fn native_parameter_rows_cover_all_proven_layout_branches_and_permuted_ordinals() {
    use crate::parameters::{ParameterRowLayout, ParameterSlot};
    for (version, flags, expected) in [
        (2, 2, ParameterRowLayout::Short32),
        (3, 2, ParameterRowLayout::Extended32),
        (4, 0, ParameterRowLayout::Extended32),
        (4, 2, ParameterRowLayout::Extended64),
        (5, 2, ParameterRowLayout::Extended32),
        (0x85, 0, ParameterRowLayout::Extended32),
        (0x85, 2, ParameterRowLayout::Extended64),
    ] {
        let (file, mut memory) = fixture();
        row_fixture(&mut memory, version, flags);
        let verified = verified(&file);
        let objects = object_catalog(&verified, &memory);
        let slot = ParameterSlot::new(12).unwrap();
        let rows = objects.parameter_rows(slot, 0, 2).unwrap().unwrap();
        assert_eq!(rows.layout, expected);
        assert_eq!(
            rows.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![7, 99]
        );
        assert_eq!(rows.rows[0].ordinal, 1);
        assert_eq!(rows.rows[0].address, PARAM_HEADER + 0x180);
        assert_eq!(rows.native_index_address, PARAM_HEADER + 0x310);
        assert_eq!(
            objects
                .copy_parameter_row(slot, 0, 7, 4, 2)
                .unwrap()
                .unwrap()
                .bytes,
            vec![10, 20, 30, 40]
        );
        assert_eq!(objects.copy_parameter_row(slot, 0, 8, 4, 2).unwrap(), None);
        assert_eq!(objects.parameter_rows(slot, 1, 2).unwrap(), None);
        assert_eq!(objects.parameter_rows(slot, 2, 2).unwrap(), None);
    }
}

#[test]
fn parameter_empty_tables_and_missing_repositories_are_supported() {
    use crate::parameters::ParameterSlot;
    let (file, mut memory) = fixture();
    row_fixture(&mut memory, 3, 0);
    memory.put(PARAM_HEADER + 0xa, &0u16.to_le_bytes());
    memory.put(PARAM_HEADER - 0xc, &0u32.to_le_bytes());
    let verified = verified(&file);
    let slot = ParameterSlot::new(12).unwrap();
    assert!(
        object_catalog(&verified, &memory)
            .parameter_rows(slot, 0, 1)
            .unwrap()
            .unwrap()
            .rows
            .is_empty()
    );
    memory.put64(BASE + 0x3d85f58, 0);
    assert_eq!(
        object_catalog(&verified, &memory)
            .parameter_rows(slot, 0, 1)
            .unwrap(),
        None
    );
}

#[test]
fn parameter_reader_rejects_corrupt_counts_indices_ids_offsets_links_and_layouts() {
    use crate::parameters::ParameterSlot;
    for (target, bytes) in [
        (PARAM_SLOT, 9u32.to_le_bytes().to_vec()),
        (PARAM_HEADER + 0x2d, vec![1]),
        (PARAM_HEADER - 0xc, 1u32.to_le_bytes().to_vec()),
        (PARAM_HEADER - 0x10, 0u32.to_le_bytes().to_vec()),
        (PARAM_HEADER - 0x10, u32::MAX.to_le_bytes().to_vec()),
        (PARAM_HEADER + 0x314, 2u32.to_le_bytes().to_vec()),
        (PARAM_HEADER + 0x31c, 1u32.to_le_bytes().to_vec()),
        (PARAM_HEADER + 0x318, 7u32.to_le_bytes().to_vec()),
        (PARAM_HEADER + 0x318, 6u32.to_le_bytes().to_vec()),
        (PARAM_HEADER + 0x310, u32::MAX.to_le_bytes().to_vec()),
        (PARAM_HEADER + 0x40, 100u32.to_le_bytes().to_vec()),
        (PARAM_HEADER + 0x44, 0x180u32.to_le_bytes().to_vec()),
        (PARAM_HEADER + 0x44, 0x40u32.to_le_bytes().to_vec()),
        (PARAM_HEADER + 0x44, 0x310u32.to_le_bytes().to_vec()),
        (
            PARAM_HEADER + 0x44,
            (64u32 * 1024 * 1024).to_le_bytes().to_vec(),
        ),
        (0x900080, 0u64.to_le_bytes().to_vec()),
        (0xa00080, 1u64.to_le_bytes().to_vec()),
        (0x900000, (BASE + 0x2a7fbb0).to_le_bytes().to_vec()),
    ] {
        let (file, mut memory) = fixture();
        row_fixture(&mut memory, 3, 0);
        memory.put(target, &bytes);
        let verified = verified(&file);
        assert!(
            object_catalog(&verified, &memory)
                .parameter_rows(ParameterSlot::new(12).unwrap(), 0, 2)
                .is_err(),
            "target {target:#x}"
        );
    }
}

#[test]
fn parameter_reader_rejects_observation_changes_and_truncated_metadata() {
    use crate::parameters::ParameterSlot;
    for target in [
        BASE + 0x3d85f58,
        PARAM_SLOT,
        PARAM_SLOT + 8,
        0x900000,
        0x900080,
        0xa00080,
        PARAM_HEADER + 0xa,
        PARAM_HEADER + 0x2d,
        PARAM_HEADER - 0xc,
        PARAM_HEADER + 0x310,
        PARAM_HEADER + 0x40,
        PARAM_HEADER + 0x44,
    ] {
        let (file, mut memory) = fixture();
        row_fixture(&mut memory, 3, 0);
        memory.watch_address = target;
        memory.change_slot = true;
        let verified = verified(&file);
        assert!(
            object_catalog(&verified, &memory)
                .parameter_rows(ParameterSlot::new(12).unwrap(), 0, 2)
                .is_err(),
            "target {target:#x}"
        );
    }
    let (file, mut memory) = fixture();
    row_fixture(&mut memory, 3, 0);
    memory.bytes.remove(&(PARAM_HEADER + 0x31f));
    let verified = verified(&file);
    assert!(
        object_catalog(&verified, &memory)
            .parameter_rows(ParameterSlot::new(12).unwrap(), 0, 2)
            .is_err()
    );
}

#[test]
fn parameter_copy_enforces_limits_row_boundary_and_metadata_boundary() {
    use crate::parameters::ParameterSlot;
    let (file, mut memory) = fixture();
    row_fixture(&mut memory, 3, 0);
    let verified = verified(&file);
    let objects = object_catalog(&verified, &memory);
    let slot = ParameterSlot::new(12).unwrap();
    assert!(ParameterSlot::new(194).is_err());
    for limit in [0, 1, 65536] {
        assert!(objects.parameter_rows(slot, 0, limit).is_err());
    }
    assert!(objects.parameter_rows(slot, 8, 2).is_err());
    for (id, length) in [
        (7, 0),
        (7, 65537),
        (u32::MAX, 1),
        (7, 129),
        (99, 273),
        (7, 5),
    ] {
        assert!(
            objects.copy_parameter_row(slot, 0, id, length, 2).is_err(),
            "id {id} length {length}"
        );
    }
}

fn definition_fixture() -> Vec<u8> {
    let (mut file, _) = fixture();
    for (ordinal, slot, name, rva) in [
        (0, 17u32, "SyntheticWeapon", 0x2040u32),
        (1, 3, "SyntheticNpc", 0x2080),
    ] {
        let record = 0x600 + ordinal * 16;
        file[record..record + 8].copy_from_slice(&(0x140000000 + rva as u64).to_le_bytes());
        file[record + 8..record + 12].copy_from_slice(&slot.to_le_bytes());
        for (index, unit) in name.encode_utf16().chain(std::iter::once(0)).enumerate() {
            let at = 0x600 + rva as usize - 0x2000 + index * 2;
            file[at..at + 2].copy_from_slice(&unit.to_le_bytes());
        }
    }
    file
}

#[test]
fn parameter_names_come_from_file_slots_not_table_ordinals() {
    let file = definition_fixture();
    let image = shikifsrs_runtime::pe::FileImage::parse(&file).unwrap();
    let definitions = crate::parameters::definitions(&image, 0x2000, 2).unwrap();
    assert_eq!(definitions[0].name, "SyntheticWeapon");
    assert_eq!(definitions[0].slot.index(), 17);
    assert_eq!(definitions[1].slot.index(), 3);
}

#[test]
fn parameter_name_definitions_reject_corrupt_slots_pointers_names_sentinel_and_sections() {
    for (at, bytes) in [
        (0x608, 193u32.to_le_bytes().to_vec()),
        (0x618, 17u32.to_le_bytes().to_vec()),
        (0x620, 1u64.to_le_bytes().to_vec()),
        (0x600, 1u64.to_le_bytes().to_vec()),
        (0x600, 0x140002041u64.to_le_bytes().to_vec()),
        (0x610, 0x140002040u64.to_le_bytes().to_vec()),
        (0x640, 0u16.to_le_bytes().to_vec()),
        (0x640, 0xd800u16.to_le_bytes().to_vec()),
        (0x1d8 + 36, 0xc0000040u32.to_le_bytes().to_vec()),
    ] {
        let mut file = definition_fixture();
        file[at..at + bytes.len()].copy_from_slice(&bytes);
        let image = shikifsrs_runtime::pe::FileImage::parse(&file).unwrap();
        assert!(
            crate::parameters::definitions(&image, 0x2000, 2).is_err(),
            "at {at:#x}"
        );
    }
    let file = definition_fixture();
    let image = shikifsrs_runtime::pe::FileImage::parse(&file).unwrap();
    for (table, count) in [(0x2000, 0), (0x2000, 194), (0x21f0, 2), (0x1000, 2)] {
        assert!(crate::parameters::definitions(&image, table, count).is_err());
    }
}

#[test]
fn parameter_copy_rechecks_metadata_after_payload_read() {
    struct ChangeOnData<'a> {
        memory: &'a SparseMemory,
        touched: Cell<bool>,
    }
    impl AddressSpace for ChangeOnData<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if address == PARAM_HEADER + 0x180 {
                self.touched.set(true);
            }
            if self.touched.get() && address == PARAM_HEADER + 0xa {
                output.fill(0);
            }
            Ok(())
        }
    }
    let (file, mut memory) = fixture();
    row_fixture(&mut memory, 3, 0);
    let changed = ChangeOnData {
        memory: &memory,
        touched: Cell::new(false),
    };
    let verified = verified(&file);
    let objects = GameObjectCatalog {
        module: verified
            .bind(
                &changed,
                BASE,
                &[CodeRange {
                    rva: 0x1000,
                    length: 1,
                }],
            )
            .unwrap(),
    };
    assert!(
        objects
            .copy_parameter_row(
                crate::parameters::ParameterSlot::new(12).unwrap(),
                0,
                7,
                4,
                2
            )
            .is_err()
    );
    assert!(changed.touched.get());
}

#[test]
fn native_handles_preserve_full_bits_and_native_shift_and_area_rules() {
    use character_handles::{HandleDecoderRecord as R, NativeCharacterHandle as H};
    for kind in 0..16u64 {
        let h = H(0xa5b6_c7d8_0123_4567 | (kind << 28));
        assert_eq!(h.kind(), kind as u8);
        for shift in 0..=127 {
            let r = R {
                set_mask: 0x17f,
                set_shift: shift,
                ordinal_mask: 0xabcde,
            };
            let d = h.decode(r);
            assert_eq!(d.handle, h);
            assert_eq!(d.set_index, ((h.0 >> (shift % 64)) & 0x17f) as u32);
            assert_eq!(d.ordinal, h.0 as u32 & 0xabcde);
        }
    }
    for upper in [0, 1, 0x12345678, u32::MAX] {
        let h = H((u64::from(upper) << 32) | 0x10000002);
        assert_eq!(
            h.area(),
            if upper == 0 || upper == u32::MAX {
                None
            } else {
                Some(upper)
            }
        );
        assert!(!h.is_absent());
        assert!(H((u64::from(upper) << 32) | u64::from(u32::MAX)).is_absent());
    }
}

fn handle_record(memory: &mut SparseMemory, kind: u8, mask: u32, shift: u32, ordinal: u32) -> u64 {
    let address = BASE
        + if kind < 9 {
            0x3b37920 + u64::from(kind) * 24
        } else {
            0x2a7c450
        };
    for (i, word) in [mask, shift, ordinal].into_iter().enumerate() {
        memory.put(address + i as u64 * 4, &word.to_le_bytes());
    }
    address
}

#[test]
fn handle_decoding_copies_selected_live_record_and_rechecks_all_three_words() {
    use character_handles::NativeCharacterHandle as H;
    for kind in 0..16u8 {
        let (file, mut memory) = fixture();
        let address = handle_record(&mut memory, kind, 0x123, 84, 0x55555);
        let checked = verified(&file);
        let h = H(0xfedc_ba98_0123_4567 | (u64::from(kind) << 28));
        let r = object_catalog(&checked, &memory)
            .decode_character_handle(h)
            .unwrap();
        assert_eq!(r.record_address, address);
        assert_eq!(r.decoded, h.decode(r.record));
        for offset in [0, 4, 8] {
            memory.watch_address = address + offset;
            memory.slot_reads.set(0);
            memory.change_slot = true;
            assert!(
                object_catalog(&checked, &memory)
                    .decode_character_handle(h)
                    .is_err()
            );
        }
    }
}

#[test]
fn global_handle_lookup_preserves_holes_full_identifier_and_rejects_area_route() {
    use character_handles::NativeCharacterHandle as H;
    for upper in [0, u32::MAX] {
        let (file, mut memory) = fixture();
        let set = character_set_fixture(&mut memory);
        handle_record(&mut memory, 1, 255, 20, 0xfffff);
        memory.put64(0x600000 + 0x1ded8 + 3 * 8, set);
        let h = H((u64::from(upper) << 32) | 0x10300002);
        memory.put64(0x620008, h.0);
        let checked = verified(&file);
        let objects = object_catalog(&checked, &memory);
        let target = objects.global_character_handle(h, 9, 3).unwrap().unwrap();
        assert_eq!(target.address, 0x620000);
        assert_eq!(target.native_identifier, h.0);
        assert_eq!(target.epoch, 9);
        assert_eq!(
            objects
                .global_character_handle(H((h.0 & !0xfffff) | 1), 9, 3)
                .unwrap(),
            None
        );
        assert!(
            objects
                .global_character_handle(H(0x1234_5678_1030_0002), 9, 3)
                .is_err()
        );
        assert!(objects.global_character_handle(h, 0, 3).is_err());
        assert!(objects.global_character_handle(h, 9, 2).is_err());
        memory.put64(0x620008, h.0 ^ (1u64 << 32));
        assert!(
            object_catalog(&checked, &memory)
                .global_character_handle(h, 9, 3)
                .is_err()
        );
    }
}

#[test]
fn global_handle_lookup_handles_absence_bounds_type_truncation_and_visible_change() {
    use character_handles::NativeCharacterHandle as H;
    for mode in 0..9 {
        let (file, mut memory) = fixture();
        let set = character_set_fixture(&mut memory);
        let decoder = handle_record(&mut memory, 1, 255, 20, 0xfffff);
        let slot = 0x600000 + 0x1ded8 + 3 * 8;
        memory.put64(slot, set);
        let h = H(0x10300002);
        memory.put64(0x620008, h.0);
        match mode {
            0 => memory.put64(BASE + 0x3d69ff8, 0),
            1 => memory.put64(slot, 0),
            2 => memory.put64(set, BASE + 0x2a7fbb0),
            3 => {
                memory.bytes.remove(&slot);
            }
            4..=8 => {
                memory.change_slot = true;
                memory.watch_address = [decoder, slot, set + 0x10, 0xd00020, 0x620008][mode - 4];
            }
            _ => unreachable!(),
        }
        let checked = verified(&file);
        let objects = object_catalog(&checked, &memory);
        if mode < 2 {
            assert_eq!(objects.global_character_handle(h, 1, 3).unwrap(), None);
        } else {
            assert!(
                objects.global_character_handle(h, 1, 3).is_err(),
                "mode {mode}"
            );
        }
        assert_eq!(
            objects.global_character_handle(H(u64::MAX), 1, 3).unwrap(),
            None
        );
        assert_eq!(
            objects
                .global_character_handle(H(0x20000000), 1, 3)
                .unwrap(),
            None
        );
    }
    let (file, mut memory) = fixture();
    singleton_fixture(&mut memory);
    handle_record(&mut memory, 1, 255, 20, 0xfffff);
    let checked = verified(&file);
    assert_eq!(
        object_catalog(&checked, &memory)
            .global_character_handle(H(0x1c500002), 1, 3)
            .unwrap(),
        None
    );
}

fn ride_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    let target = module_fixture(memory, CharacterModule::Ride);
    memory.put64(0xd00010, 0xe00000);
    memory.put64(0xe00000, BASE + 0x2a3d458);
    memory.put(0xe00050, &u32::MAX.to_le_bytes());
    memory.put64(0xd00028, 0x0123456710300002);
    memory.put(0xd00030, &[0xfe]);
    target
}

#[test]
fn ride_observation_preserves_related_handle_unknown_state_and_missing_node() {
    let (file, mut memory) = fixture();
    let target = ride_fixture(&mut memory);
    let checked = verified(&file);
    let r = object_catalog(&checked, &memory)
        .character_ride(target)
        .unwrap()
        .unwrap();
    assert_eq!(r.related_handle.0, 0x0123456710300002);
    assert_eq!(r.raw_flag, 0xfe);
    assert_eq!(r.node_state, Some(u32::MAX));
    memory.put64(0xd00010, 0);
    memory.put64(0xd00028, u64::MAX);
    let r = object_catalog(&checked, &memory)
        .character_ride(target)
        .unwrap()
        .unwrap();
    assert_eq!(r.node_state, None);
    assert_eq!(r.node_address, None);
    assert!(r.related_handle.is_absent());
    memory.put64(0xc00000 + 0x1d * 8, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_ride(target)
            .unwrap(),
        None
    );
}

#[test]
fn ride_observation_rejects_stale_identity_foreign_type_truncation_and_visible_change() {
    for mode in 0..9 {
        let (file, mut memory) = fixture();
        let mut target = ride_fixture(&mut memory);
        match mode {
            0 => target.native_identifier ^= 1,
            1 => memory.put64(0xe00000, BASE + 0x2a3d098),
            2 => memory.put64(0xd00010, 0xe00001),
            3 => {
                memory.bytes.remove(&0xe00050);
            }
            4..=8 => {
                memory.change_slot = true;
                memory.watch_address = [0xd00010, 0xd00028, 0xd00030, 0xe00000, 0xe00050][mode - 4];
            }
            _ => unreachable!(),
        }
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .character_ride(target)
                .is_err(),
            "mode {mode}"
        );
    }
}

#[test]
fn weapon_side_auto_selection_checks_only_used_slot_and_rejects_unknown_styles() {
    use character_equipment::WeaponSelectionMode as W;
    let (_, mut memory) = fixture();
    let target = equipment_fixture(&mut memory);
    let mut observation = character_equipment::CharacterEquipmentObservation {
        character: target,
        assembly_address: 0xc00004,
        native_arm_style: 1,
        native_selected_slots: [0; 6],
        parameter_ids: std::array::from_fn(|i| i as i32),
    };
    for even in 0..3 {
        for odd in 0..3 {
            observation.native_selected_slots[..2].copy_from_slice(&[even, odd]);
            for style in 0..4 {
                observation.native_arm_style = style;
                let selected = observation.selected_weapon(W::AutoTwoHand).unwrap();
                let expected = if style == 2 { even * 2 } else { odd * 2 + 1 };
                assert_eq!(selected.slot, expected as usize);
                assert_eq!(selected.id, expected as i32);
                assert_eq!(observation.is_two_handed().unwrap(), style >= 2);
            }
        }
    }
    for style in [4, u32::MAX] {
        observation.native_arm_style = style;
        assert!(observation.selected_weapon(W::AutoTwoHand).is_err());
        assert!(observation.is_two_handed().is_err());
        observation.native_selected_slots[..2].copy_from_slice(&[2, 99]);
        assert_eq!(observation.selected_weapon(W::Even).unwrap().slot, 4);
        assert!(observation.selected_weapon(W::Odd).is_err());
        observation.native_selected_slots[..2].copy_from_slice(&[99, 2]);
        assert_eq!(observation.selected_weapon(W::Odd).unwrap().slot, 5);
        assert!(observation.selected_weapon(W::Even).is_err());
    }
}

#[test]
fn complete_character_effects_preserve_signed_ids_duplicates_and_empty_vs_absent() {
    let (file, mut memory) = fixture();
    character_links_fixture(&mut memory);
    let target = equipment_fixture(&mut memory);
    memory.put(0xc11008, &(-1i32).to_le_bytes());
    memory.put(0xc12008, &(-1i32).to_le_bytes());
    let checked = verified(&file);
    let read =
        |memory: &SparseMemory| object_catalog(&checked, memory).character_effects(target, 2);
    let copy = read(&memory).unwrap().unwrap();
    assert_eq!(copy.character, target);
    assert_eq!(copy.container_address, 0xc10000);
    assert_eq!(
        copy.entries
            .iter()
            .map(|e| (e.address, e.native_id))
            .collect::<Vec<_>>(),
        vec![(0xc11000, -1), (0xc12000, -1)]
    );
    memory.put64(0xc10008, 0);
    assert!(read(&memory).unwrap().unwrap().entries.is_empty());
    memory.put64(0x610178, 0);
    assert_eq!(read(&memory).unwrap(), None);
}

#[test]
fn complete_character_effects_fail_on_unread_tail_cycle_stale_identity_and_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for (address, value) in [
        (0xc12030, 0xc11000),
        (0xc12030, 1),
        (0xc10010, 0x620000),
        (0x610008, 1),
    ] {
        let (_, mut memory) = fixture();
        character_links_fixture(&mut memory);
        let target = equipment_fixture(&mut memory);
        memory.put64(address, value);
        assert!(
            object_catalog(&checked, &memory)
                .character_effects(target, 2)
                .is_err()
        );
    }
    for address in [0xc12008, 0xc11030, 0x610008] {
        let (_, mut memory) = fixture();
        character_links_fixture(&mut memory);
        let target = equipment_fixture(&mut memory);
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .character_effects(target, 2)
                .is_err()
        );
    }
    let (_, mut memory) = fixture();
    character_links_fixture(&mut memory);
    let target = equipment_fixture(&mut memory);
    for limit in [0, 1, 65_537] {
        assert!(
            object_catalog(&checked, &memory)
                .character_effects(target, limit)
                .is_err()
        );
    }
    memory.bytes.remove(&0xc12030);
    assert_eq!(
        object_catalog(&checked, &memory)
            .has_sp_effect(target.address, 899000, 1)
            .unwrap(),
        Some(true)
    );
    assert!(
        object_catalog(&checked, &memory)
            .character_effects(target, 2)
            .is_err()
    );
}

fn sound_conditions_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    character_links_fixture(memory);
    field_row_fixture(memory, 0);
    let target = equipment_fixture(memory);
    memory.put(0xc0000c, &1u32.to_le_bytes());
    // Both chosen sides resolve 199/100 to the independent PARAM row 100.
    memory.put(0xc00090, &199i32.to_le_bytes());
    memory.put(0xc0008c, &100i32.to_le_bytes());
    memory.put(PARAM_HEADER + 0x40, &100u32.to_le_bytes());
    memory.put(PARAM_HEADER + 0x718, &100u32.to_le_bytes());
    memory.put(PARAM_HEADER + 0x200 + 0xee, &u16::MAX.to_le_bytes());
    target
}

#[test]
fn sound_player_conditions_link_selected_weapon_row_and_complete_effect_chain() {
    use character_equipment::WeaponSelectionMode as W;
    use sound_conditions::WeaponCategoryObservation as C;
    let (file, mut memory) = fixture();
    let target = sound_conditions_fixture(&mut memory);
    let checked = verified(&file);
    for mode in [W::Even, W::Odd, W::AutoTwoHand] {
        let copy = object_catalog(&checked, &memory)
            .sound_player_conditions(target, mode, 2, 2)
            .unwrap()
            .unwrap();
        assert_eq!(
            copy.weapon,
            C::Category {
                parameter_id: 100,
                row_address: PARAM_HEADER + 0x200,
                category: u16::MAX
            }
        );
        assert_eq!(copy.effects.unwrap().entries.len(), 2);
    }
    memory.put(0xc0008c, &777i32.to_le_bytes());
    assert_eq!(
        object_catalog(&checked, &memory)
            .sound_player_conditions(target, W::Odd, 2, 2)
            .unwrap()
            .unwrap()
            .weapon,
        C::RowMissing { parameter_id: 700 }
    );
    // Nonpositive IDs do not query a parameter repository; no PARAM allocation is mapped.
    for id in [0, -1, i32::MIN] {
        let (_, mut memory) = fixture();
        character_links_fixture(&mut memory);
        let target = equipment_fixture(&mut memory);
        memory.put(0xc0008c, &id.to_le_bytes());
        assert_eq!(
            object_catalog(&checked, &memory)
                .sound_player_conditions(target, W::Odd, 2, 2)
                .unwrap()
                .unwrap()
                .weapon,
            C::NotEquipped { native_id: id }
        );
    }
    memory.put64(0x610638, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .sound_player_conditions(target, W::Odd, 2, 2)
            .unwrap()
            .unwrap()
            .weapon,
        C::AssemblyMissing
    );
}

#[test]
fn sound_conditions_do_not_return_partial_success_after_unknown_style_or_read_failure() {
    use character_equipment::WeaponSelectionMode as W;
    let (file, _) = fixture();
    let checked = verified(&file);
    for address in [0xc0000c, 0xc0008c, 0xc11030, PARAM_HEADER + 0x200, 0x610008] {
        let (_, mut memory) = fixture();
        let target = sound_conditions_fixture(&mut memory);
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .sound_player_conditions(target, W::AutoTwoHand, 2, 2)
                .is_err(),
            "{address:x}"
        );
    }
    let (_, mut memory) = fixture();
    let target = sound_conditions_fixture(&mut memory);
    memory.put(0xc0000c, &4u32.to_le_bytes());
    assert!(
        object_catalog(&checked, &memory)
            .sound_player_conditions(target, W::AutoTwoHand, 2, 2)
            .is_err()
    );
    for (rows, effects) in [(0, 2), (2, 0), (1, 2), (2, 1), (65_537, 2)] {
        assert!(
            object_catalog(&checked, &memory)
                .sound_player_conditions(target, W::Odd, rows, effects)
                .is_err()
        );
    }
}

#[test]
fn world_sound_conditions_reject_late_changes_to_an_earlier_player_after_its_local_recheck() {
    use character_equipment::WeaponSelectionMode as W;
    let (bytes, mut memory) = fixture();
    character_set_fixture(&mut memory);
    sound_conditions_fixture(&mut memory);
    memory.put64(0x620638, 0);
    memory.put64(0x620178, 0);
    struct Later<'a> {
        memory: &'a SparseMemory,
        changed: Cell<bool>,
        watch: u64,
    }
    impl AddressSpace for Later<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if address == 0x620638 {
                self.changed.set(true);
            }
            if self.changed.get() && address == self.watch {
                // The second player remains stable; only the already copied first
                // player's selected input changes consistently for later reads.
                if self.watch == PARAM_HEADER + 0x200 {
                    output[0xee..0xf0].copy_from_slice(&73u16.to_le_bytes());
                } else if output.len() == 4 {
                    output.copy_from_slice(&899123i32.to_le_bytes());
                } else {
                    output.fill(0);
                }
            }
            Ok(())
        }
    }
    let file = verified(&bytes);
    for watch in [PARAM_HEADER + 0x200, 0xc11008, 0x610638] {
        let changed = Later {
            memory: &memory,
            changed: Cell::new(false),
            watch,
        };
        let objects = GameObjectCatalog {
            module: file
                .bind(
                    &changed,
                    BASE,
                    &[CodeRange {
                        rva: 0x1000,
                        length: 1,
                    }],
                )
                .unwrap(),
        };
        assert!(
            objects
                .world_sound_conditions(3, W::AutoTwoHand, 3, 2, 2)
                .is_err(),
            "watch {watch:x}"
        );
        assert!(changed.changed.get());
    }
}

#[test]
fn current_sound_condition_adapter_validates_bounds_before_any_file_or_process_read() {
    use current_sound_conditions::{CurrentSoundConditions, SoundConditionReadConfig as Config};
    for config in [
        Config {
            player_limit: 0,
            ..Default::default()
        },
        Config {
            row_limit: 65537,
            ..Default::default()
        },
        Config {
            effect_limit: 0,
            ..Default::default()
        },
        Config {
            weapon_mappings: vec![
                sound_conditions::ConditionBankMapping {
                    condition_id: 0,
                    bank_id: 0
                };
                65537
            ],
            ..Default::default()
        },
    ] {
        assert!(matches!(
            // SAFETY: Invalid input rejects before current-process binding or any native call.
            unsafe { CurrentSoundConditions::bind(std::sync::Arc::from([]), 0, None, config) },
            Err(Error::InvalidProfile(
                "invalid current sound condition bounds"
            ))
        ));
    }
    assert!(
        // SAFETY: Empty file cannot pass the fixed game hash; no process read occurs.
        unsafe {
            CurrentSoundConditions::bind(std::sync::Arc::from([]), 0, None, Config::default())
        }
        .is_err()
    );
}

#[test]
fn world_sound_conditions_keep_player_holes_all_players_and_world_absence_distinct() {
    use character_equipment::WeaponSelectionMode as W;
    let (file, mut memory) = fixture();
    let set = character_set_fixture(&mut memory);
    sound_conditions_fixture(&mut memory);
    memory.put64(0x620638, 0);
    memory.put64(0x620178, 0);
    let checked = verified(&file);
    let copy = object_catalog(&checked, &memory)
        .world_sound_conditions(3, W::AutoTwoHand, 3, 2, 2)
        .unwrap()
        .unwrap();
    assert_eq!(copy.players().len(), 2);
    assert_eq!(copy.players()[1].character.address, 0x620000);
    assert_eq!(
        copy.players()[1].weapon,
        sound_conditions::WeaponCategoryObservation::AssemblyMissing
    );
    let mapping = |condition_id, bank_id| sound_conditions::ConditionBankMapping {
        condition_id,
        bank_id,
    };
    assert_eq!(
        copy.bank_targets(
            &[mapping(65535, 1)],
            &[mapping(899001, 2), mapping(899000, 1)]
        )
        .unwrap(),
        std::collections::BTreeSet::from([1, 2])
    );
    memory.put64(0xd00000, 0);
    memory.put64(0xd00020, 0);
    assert!(
        object_catalog(&checked, &memory)
            .world_sound_conditions(3, W::AutoTwoHand, 3, 2, 2)
            .unwrap()
            .unwrap()
            .players()
            .is_empty()
    );
    memory.put64(BASE + 0x3d69ff8, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .world_sound_conditions(3, W::AutoTwoHand, 3, 2, 2)
            .unwrap(),
        None
    );
    memory.put64(BASE + 0x3d69ff8, 0x600000);
    memory.put64(set + 0x18, 1);
    assert!(
        object_catalog(&checked, &memory)
            .world_sound_conditions(3, W::AutoTwoHand, 3, 2, 2)
            .is_err()
    );
    assert!(
        object_catalog(&checked, &memory)
            .world_sound_conditions(0, W::Odd, 3, 2, 2)
            .is_err()
    );
}

fn ride_phase_fixture() -> (Vec<u8>, SparseMemory, character_sets::CharacterIdentity) {
    let (mut file, mut memory) = fixture();
    let target = ride_fixture(&mut memory);
    put32(&mut file, 0x1d8 + 12, 0x2a3d520);
    let roles = [0u32, 1, 2, 1, 2, 1, 2, 1, 2, 0];
    let bytes: Vec<_> = roles.iter().flat_map(|role| role.to_le_bytes()).collect();
    file[0x600..0x628].copy_from_slice(&bytes);
    memory.put(BASE, &file[..0x200]);
    memory.put(BASE + 0x2a3d520, &bytes);
    memory.put(0xe00050, &1u32.to_le_bytes());
    memory.put(0xe00070, &[1]);
    (file, memory, target)
}

#[test]
fn ride_phase_copies_only_proven_flag_role_and_native_predicates_without_semantic_names() {
    let (file, mut memory, target) = ride_phase_fixture();
    let checked = verified(&file);
    for state in 0..10u32 {
        memory.put(0xe00050, &state.to_le_bytes());
        for flag in [0u8, 1, 255] {
            memory.put(0xe00070, &[flag]);
            let result = object_catalog(&checked, &memory)
                .character_ride_phase(target)
                .unwrap()
                .unwrap();
            assert_eq!(result.ride.node_state, Some(state));
            assert_eq!(result.node_flag, flag);
            assert_eq!(
                result.native_role,
                if matches!(state, 0 | 9) {
                    0
                } else if state % 2 == 0 {
                    2
                } else {
                    1
                }
            );
            assert_eq!(result.native_active, state != 0);
            assert_eq!(
                result.native_endpoint_one_query,
                matches!(state, 1 | 3) || (matches!(state, 5 | 7) && flag != 0)
            );
        }
    }
}

#[test]
fn ride_phase_rejects_unknown_state_changed_readonly_table_and_unread_flag() {
    for corrupt in 0..5 {
        let (mut file, mut memory, target) = ride_phase_fixture();
        match corrupt {
            0 => memory.put(0xe00050, &u32::MAX.to_le_bytes()),
            1 => memory.put(BASE + 0x2a3d524, &3u32.to_le_bytes()),
            2 => {
                memory.bytes.remove(&0xe00070);
            }
            3 => put32(&mut file, 0x1d8 + 36, 0xc0000040),
            _ => {
                file[0x604] = 3;
                memory.put(BASE + 0x2a3d524, &3u32.to_le_bytes());
            }
        }
        memory.put(BASE, &file[..0x200]);
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .character_ride_phase(target)
                .is_err(),
            "{corrupt}"
        );
    }
}

#[test]
fn ride_phase_distinguishes_missing_storage_and_detects_flag_table_and_identity_changes() {
    for missing in [0xd00010, 0x610190, BASE + 0x3d69ff8] {
        let (file, mut memory, target) = ride_phase_fixture();
        memory.put64(missing, 0);
        let checked = verified(&file);
        assert_eq!(
            object_catalog(&checked, &memory)
                .character_ride_phase(target)
                .unwrap(),
            None
        );
    }
    for watched in [0xe00070, 0xe00050, BASE + 0x2a3d520, 0x610008] {
        let (file, mut memory, target) = ride_phase_fixture();
        memory.watch_address = watched;
        memory.change_slot = true;
        let checked = verified(&file);
        assert!(
            object_catalog(&checked, &memory)
                .character_ride_phase(target)
                .is_err(),
            "{watched:x}"
        );
    }
}

fn physics_query_graph_fixture(memory: &mut SparseMemory) {
    memory.put64(BASE + 0x3d7a0d0, 0x1340000);
    memory.put64(0x1340000, BASE + 0x2b934e8);
    memory.put64(0x1340098, 0x1350000);
    memory.put64(0x1350000, BASE + 0x2b96978);
    memory.put64(0x1350008, 0x1360000);
    memory.put64(0x1360000, BASE + 0x2ef0dd8);
    memory.put64(0x13400c0, u64::MAX);
    memory.put64(0x13604d0, 3);
    memory.put64(0x13604d8, 5);
    memory.put64(0x1360b00, 7);
    // Manager+8 allocator, opaque destinations, query receiver/table and scene absent.
}

#[test]
fn physics_query_graph_copies_exact_chain_and_opaque_dependencies_without_dereferencing() {
    let (file, mut memory) = fixture();
    physics_query_graph_fixture(&mut memory);
    let checked = verified(&file);
    let observed = object_catalog(&checked, &memory)
        .physics_query_graph(29)
        .unwrap()
        .unwrap();
    assert_eq!((observed.module_base, observed.epoch), (BASE, 29));
    assert_eq!(
        (
            observed.manager.kind,
            observed.physics_world.kind,
            observed.havok_world.kind
        ),
        (
            ObjectKind::HavokManager,
            ObjectKind::PhysicsWorld,
            ObjectKind::HavokWorld
        )
    );
    assert_eq!(
        (
            observed.special_filter,
            observed.world_filter,
            observed.query_dispatch,
            observed.query_context
        ),
        (u64::MAX, 3, 5, 7)
    );
    assert!(
        object_catalog(&checked, &memory)
            .physics_query_graph(0)
            .is_err()
    );
    for slot in [BASE + 0x3d7a0d0, 0x1340098, 0x1350008] {
        physics_query_graph_fixture(&mut memory);
        memory.put64(slot, 0);
        assert_eq!(
            object_catalog(&checked, &memory)
                .physics_query_graph(29)
                .unwrap(),
            None
        );
    }
    physics_query_graph_fixture(&mut memory);
    for slot in [0x13400c0, 0x13604d0, 0x13604d8, 0x1360b00] {
        memory.put64(slot, 0);
    }
    let copied = object_catalog(&checked, &memory)
        .physics_query_graph(29)
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            copied.special_filter,
            copied.world_filter,
            copied.query_dispatch,
            copied.query_context
        ),
        (0, 0, 0, 0)
    );
}

#[test]
fn physics_query_graph_rejects_foreign_types_missing_reads_and_visible_link_replacement() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for address in [
        BASE + 0x3d7a0d0,
        0x1340000,
        0x1340098,
        0x1350000,
        0x1350008,
        0x1360000,
        0x13400c0,
        0x13604d0,
        0x13604d8,
        0x1360b00,
    ] {
        for changed in [false, true] {
            let (_, mut memory) = fixture();
            physics_query_graph_fixture(&mut memory);
            if changed {
                memory.watch_address = address;
                memory.change_slot = true;
            } else {
                memory.bytes.remove(&address);
            }
            assert!(
                object_catalog(&checked, &memory)
                    .physics_query_graph(29)
                    .is_err(),
                "{address:x}/{changed}"
            );
        }
    }
    for (slot, wrong_table) in [
        (0x1340000, 0x2b96978),
        (0x1350000, 0x2b934e8),
        (0x1360000, 0x2b96978),
    ] {
        let (_, mut memory) = fixture();
        physics_query_graph_fixture(&mut memory);
        memory.put64(slot, BASE + wrong_table);
        assert!(
            object_catalog(&checked, &memory)
                .physics_query_graph(29)
                .is_err()
        );
    }
    for slot in [BASE + 0x3d7a0d0, 0x1340098, 0x1350008] {
        let (_, mut memory) = fixture();
        physics_query_graph_fixture(&mut memory);
        memory.put64(slot, u64::MAX - 6);
        assert!(
            object_catalog(&checked, &memory)
                .physics_query_graph(29)
                .is_err()
        );
    }
}

fn camera_fixture(memory: &mut SparseMemory) {
    memory.put64(BASE + 0x3d669c8, 0x1300000);
    memory.put64(0x1300000, BASE + 0x2a2aff0);
    memory.put(0x1300028, &0xfedcba98u32.to_le_bytes());
    for slot in 0..4u64 {
        let address = 0x1310000 + slot * 0x100;
        memory.put64(0x1300008 + slot * 8, address);
        memory.put64(address, BASE + 0x2aa0a40);
        for index in 0..16u64 {
            memory.put(
                address + 0x10 + index * 4,
                &(index as u32 + 1).to_le_bytes(),
            );
        }
        for (index, value) in [0.8f32, 16.0 / 9.0, 0.05, 10000.0].into_iter().enumerate() {
            memory.put(
                address + 0x50 + index as u64 * 4,
                &value.to_bits().to_le_bytes(),
            );
        }
    }
}

fn window_fixture(memory: &mut SparseMemory) {
    memory.put64(BASE + 0x458f910, 0x1320000);
    memory.put64(0x1320000, BASE + 0x2c011e8);
    memory.put64(0x1320008, 0xabcdef0123456789);
    memory.put(0x1320028, &2560i32.to_le_bytes());
    memory.put(0x132002c, &1440i32.to_le_bytes());
    // Position +20/+24, runtime descriptor, global scale and render target absent.
}

fn character_camera_fixture(memory: &mut SparseMemory) {
    singleton_fixture(memory);
    memory.put64(0x6000f8, 0x1340000);
    for (index, table) in [0x2a2aa08u64, 0x2a2aca0, 0x2a2a7f0, 0x2a2bad0]
        .into_iter()
        .enumerate()
    {
        let address = 0x1340000 + index as u64 * 0x200;
        memory.put64(address, BASE + table);
        if index != 0 {
            memory.put64(0x1340060 + (index as u64 - 1) * 8, address);
        }
        for word in 0..16u64 {
            memory.put(
                address + 0x10 + word * 4,
                &((word + 1) as f32).to_bits().to_le_bytes(),
            );
        }
        for (word, value) in [0.7f32, 1.5, 0.1, 1000.0].into_iter().enumerate() {
            memory.put(
                address + 0x50 + word as u64 * 4,
                &value.to_bits().to_le_bytes(),
            );
        }
    }
}

#[test]
fn camera_scalars_resolve_all_eight_typed_slots_and_full_known_owner_paths() {
    use crate::camera_adjustments::{CameraScalar, CameraScalarKey, CameraSlot};
    let (file, mut memory) = fixture();
    camera_fixture(&mut memory);
    character_camera_fixture(&mut memory);
    let checked = verified(&file);
    let objects = object_catalog(&checked, &memory);
    for (index, slot) in [
        CameraSlot::Perspective0,
        CameraSlot::Perspective1,
        CameraSlot::Perspective2,
        CameraSlot::Perspective3,
        CameraSlot::Character,
        CameraSlot::Follow,
        CameraSlot::Aim,
        CameraSlot::Distance,
    ]
    .into_iter()
    .enumerate()
    {
        let key = CameraScalarKey {
            epoch: 55,
            slot,
            field: CameraScalar::FieldOfView,
        };
        let copied = objects.camera_scalar(key).unwrap().unwrap();
        assert_eq!(copied.key, key);
        assert_eq!(copied.module_base, BASE);
        if index < 4 {
            assert_eq!(copied.owner.address, 0x1300000);
            assert_eq!(copied.parent, copied.camera);
            assert_eq!(copied.camera.address, 0x1310000 + index as u64 * 0x100);
            assert_eq!(copied.bits, 0.8f32.to_bits());
        } else {
            assert_eq!(copied.owner.address, 0x600000);
            assert_eq!(copied.parent.address, 0x1340000);
            assert_eq!(
                copied.camera.address,
                0x1340000 + (index as u64 - 4) * 0x200
            );
            assert_eq!(copied.bits, 0.7f32.to_bits());
        }
        for (field, expected) in [
            (CameraScalar::PositionX, 13.0f32),
            (CameraScalar::PositionY, 14.0),
            (CameraScalar::PositionZ, 15.0),
        ] {
            let copied = objects
                .camera_scalar(CameraScalarKey { field, ..key })
                .unwrap()
                .unwrap();
            assert_eq!(
                copied.bits,
                if index < 4 {
                    (expected as u32).to_le()
                } else {
                    expected.to_bits()
                }
            );
        }
    }
}

#[test]
fn character_camera_absence_wrong_subtype_and_unreadable_selected_word_are_distinct() {
    use crate::camera_adjustments::{CameraScalar, CameraScalarKey, CameraSlot};
    let (file, mut memory) = fixture();
    character_camera_fixture(&mut memory);
    let checked = verified(&file);
    let key = CameraScalarKey {
        epoch: 1,
        slot: CameraSlot::Aim,
        field: CameraScalar::FieldOfView,
    };
    memory.put64(0x1340068, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .camera_scalar(key)
            .unwrap(),
        None
    );
    memory.put64(0x1340068, 0x1340200); // Proved follow type is not an aim camera.
    assert!(
        object_catalog(&checked, &memory)
            .camera_scalar(key)
            .is_err()
    );
    memory.put64(0x1340068, 0x1340400);
    memory.bytes.remove(&0x1340450);
    assert!(
        object_catalog(&checked, &memory)
            .camera_scalar(key)
            .is_err()
    );
    assert!(
        object_catalog(&checked, &memory)
            .camera_scalar(CameraScalarKey { epoch: 0, ..key })
            .is_err()
    );
    memory.put64(0x6000f8, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .camera_scalar(key)
            .unwrap(),
        None
    );
}

fn camera_override_fixture(memory: &mut SparseMemory) {
    memory.put64(BASE + 0x3d6d988, 0x1330000);
    memory.put64(0x1330000, BASE + 0x2a82110);
    for (i, bits) in [
        (-1.0f32).to_bits(),
        0,
        0x7fc12345,
        (-0.0f32).to_bits(),
        0x7f800000,
    ]
    .into_iter()
    .enumerate()
    {
        memory.put(0x1330058 + i as u64 * 4, &bits.to_le_bytes());
    }
    memory.put(0x133006c, &[0xfe, 1]);
    // No GameMan task, unknown fields, padding +6e/+6f, world or camera link mapped.
}

fn game_camera_state_fixture(memory: &mut SparseMemory) {
    memory.put64(BASE + 0x3d6d988, 0x1330000);
    memory.put64(0x1330000, BASE + 0x2a82110);
    for (i, id) in [-1i32, 120, i32::MIN, 45, i32::MAX, -7]
        .into_iter()
        .enumerate()
    {
        memory.put(0x1330040 + i as u64 * 4, &id.to_le_bytes());
    }
    for (i, bits) in [f32::MAX.to_bits(), 0x7fc12345, 42, 0x80000000u32]
        .into_iter()
        .enumerate()
    {
        memory.put(0x1330070 + i as u64 * 4, &bits.to_le_bytes());
    }
    memory.put(0x1330080, &[0xfe, 2, 7]);
    // Overrides +58..6d, padding83 and actual camera/PARAM graph remain unreadable.
}

#[test]
fn camera_state_preserves_six_signed_ids_raw_force_sentinels_method_and_three_bytes() {
    let (file, mut memory) = fixture();
    game_camera_state_fixture(&mut memory);
    let checked = verified(&file);
    let copied = object_catalog(&checked, &memory)
        .game_camera_state(23)
        .unwrap()
        .unwrap();
    assert_eq!((copied.module_base, copied.epoch), (BASE, 23));
    assert_eq!(copied.parameter_ids, [-1, 120, i32::MIN, 45, i32::MAX, -7]);
    assert_eq!(copied.force_angle_bits, [f32::MAX.to_bits(), 0x7fc12345]);
    assert_eq!(copied.force_progress_bits, 0x80000000);
    assert_eq!(copied.force_flag_bytes, [0xfe, 2, 7]);
    for method in [0u32, 1, 42, u32::MAX] {
        memory.put(0x1330078, &method.to_le_bytes());
        let copied = object_catalog(&checked, &memory)
            .game_camera_state(23)
            .unwrap()
            .unwrap();
        assert_eq!(copied.force_method_raw, method);
        assert_eq!(copied.force_requested(), method != 0);
    }
    assert!(
        object_catalog(&checked, &memory)
            .game_camera_state(0)
            .is_err()
    );
    memory.put64(BASE + 0x3d6d988, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .game_camera_state(23)
            .unwrap(),
        None
    );
}

#[test]
fn camera_state_rejects_truncated_and_visibly_changed_parameter_force_and_type_sources() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for address in [
        0x1330040,
        0x1330044,
        0x1330048,
        0x133004c,
        0x1330050,
        0x1330054,
        0x1330070,
        0x1330074,
        0x1330078,
        0x133007c,
        0x1330080,
        0x1330081,
        0x1330082,
        0x1330000,
        BASE + 0x3d6d988,
    ] {
        for changed in [false, true] {
            let (_, mut memory) = fixture();
            game_camera_state_fixture(&mut memory);
            if changed {
                memory.watch_address = address;
                memory.change_slot = true;
            } else {
                memory.bytes.remove(&address);
            }
            assert!(
                object_catalog(&checked, &memory)
                    .game_camera_state(23)
                    .is_err(),
                "{address:x}/{changed}"
            );
        }
    }
    let (_, mut memory) = fixture();
    game_camera_state_fixture(&mut memory);
    memory.put64(0x1330000, BASE + 0x2a2aff0);
    assert!(
        object_catalog(&checked, &memory)
            .game_camera_state(23)
            .is_err()
    );
}

#[test]
fn camera_overrides_copy_native_raw_sentinels_flags_and_epoch_with_missing_distinct() {
    let (file, mut memory) = fixture();
    camera_override_fixture(&mut memory);
    let checked = verified(&file);
    let copied = object_catalog(&checked, &memory)
        .game_camera_overrides(19)
        .unwrap()
        .unwrap();
    assert_eq!((copied.module_base, copied.epoch), (BASE, 19));
    assert_eq!(copied.manager.kind, ObjectKind::GameManager);
    assert_eq!(
        copied.payload.scalar_bits,
        [0xbf800000, 0, 0x7fc12345, 0x80000000, 0x7f800000]
    );
    assert_eq!(copied.payload.flag_bytes, [0xfe, 1]);
    assert!(
        object_catalog(&checked, &memory)
            .game_camera_overrides(0)
            .is_err()
    );
    memory.put64(BASE + 0x3d6d988, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .game_camera_overrides(19)
            .unwrap(),
        None
    );
}

#[test]
fn camera_overrides_reject_foreign_truncated_and_changed_seven_field_sources() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for mode in 0..18 {
        let (_, mut memory) = fixture();
        camera_override_fixture(&mut memory);
        match mode {
            0 => memory.put64(0x1330000, BASE + 0x2c011e8),
            1 => memory.put64(BASE + 0x3d6d988, 0x1330001),
            2 => memory.put64(BASE + 0x3d6d988, u64::MAX - 7),
            3..=8 => {
                memory.bytes.remove(
                    &[
                        0x133005b, 0x133005f, 0x1330063, 0x1330067, 0x133006b, 0x133006d,
                    ][mode - 3],
                );
            }
            _ => {
                memory.watch_address = [
                    BASE + 0x3d6d988,
                    0x1330000,
                    0x1330058,
                    0x133005c,
                    0x1330060,
                    0x1330064,
                    0x1330068,
                    0x133006c,
                    0x133006d,
                ][mode - 9];
                // SparseMemory changes watched reads to zero; seed the normally
                // zero +5c slot so this scenario represents an actual change.
                if memory.watch_address == 0x133005c {
                    memory.put(0x133005c, &1.0f32.to_bits().to_le_bytes());
                }
                memory.change_slot = true;
            }
        }
        assert!(
            object_catalog(&checked, &memory)
                .game_camera_overrides(19)
                .is_err(),
            "{mode}"
        );
    }
}

#[test]
fn window_preserves_opaque_handle_and_signed_sizes_without_reading_position_or_descriptor() {
    let (file, mut memory) = fixture();
    window_fixture(&mut memory);
    let checked = verified(&file);
    let copied = object_catalog(&checked, &memory).window().unwrap().unwrap();
    assert_eq!(copied.window.kind, ObjectKind::Window);
    assert_eq!(copied.handle, 0xabcdef0123456789);
    assert_eq!(
        copied.configured_extent().unwrap(),
        cameras::PixelExtent {
            width: 2560,
            height: 1440,
        }
    );
    for invalid in [0i32, -1, i32::MIN] {
        memory.put(0x1320028, &invalid.to_le_bytes());
        memory.put64(0x1320008, 0);
        let copied = object_catalog(&checked, &memory).window().unwrap().unwrap();
        assert_eq!(copied.handle, 0);
        assert_eq!(copied.configured_width, invalid);
        assert!(copied.configured_extent().is_err());
    }
    memory.put64(BASE + 0x458f910, 0);
    assert_eq!(object_catalog(&checked, &memory).window().unwrap(), None);
}

#[test]
fn window_refuses_foreign_truncated_unaligned_and_visibly_changed_storage() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for mode in 0..12 {
        let (_, mut memory) = fixture();
        window_fixture(&mut memory);
        match mode {
            0 => memory.put64(0x1320000, BASE + 0x2aa0a40),
            1 => memory.put64(BASE + 0x458f910, 0x1320001),
            2 => memory.put64(BASE + 0x458f910, u64::MAX - 7),
            3..=6 => {
                memory
                    .bytes
                    .remove(&[0x132000f, 0x1320028, 0x132002c, 0x132002f][mode - 3]);
            }
            _ => {
                memory.watch_address =
                    [BASE + 0x458f910, 0x1320000, 0x1320008, 0x1320028, 0x132002c][mode - 7];
                memory.change_slot = true;
            }
        }
        assert!(
            object_catalog(&checked, &memory).window().is_err(),
            "{mode}"
        );
    }
}

#[test]
fn camera_copies_four_exact_links_raw_flags_matrix_and_frustum_without_padding_reads() {
    let (file, mut memory) = fixture();
    camera_fixture(&mut memory);
    let checked = verified(&file);
    let objects = object_catalog(&checked, &memory);
    let root = objects.cameras().unwrap().unwrap();
    assert_eq!(root.manager.kind, ObjectKind::CameraManager);
    assert_eq!(root.native_flags, 0xfedcba98);
    for (i, camera) in root.perspectives.into_iter().enumerate() {
        let camera = camera.unwrap();
        assert_eq!(camera.address, 0x1310000 + i as u64 * 0x100);
        let snapshot = objects.perspective_camera(camera.address).unwrap().unwrap();
        assert_eq!(
            snapshot
                .matrix_bits
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
            (1..=16).collect::<Vec<u32>>()
        );
        assert_eq!(
            snapshot.frustum().unwrap(),
            cameras::PerspectiveFrustum {
                fov_radians: 0.8,
                aspect_ratio: 16.0 / 9.0,
                near_clip: 0.05,
                far_clip: 10000.0
            }
        );
    }
    memory.put64(0x1300010, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .cameras()
            .unwrap()
            .unwrap()
            .perspectives[1],
        None
    );
    assert_eq!(
        object_catalog(&checked, &memory)
            .perspective_camera(0)
            .unwrap(),
        None
    );
    memory.put64(BASE + 0x3d669c8, 0);
    assert_eq!(object_catalog(&checked, &memory).cameras().unwrap(), None);
}

#[test]
fn camera_refuses_foreign_primary_types_bad_alignment_overflow_and_truncated_payloads() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for (address, value) in [
        (0x1300000, BASE + 0x2a4e980),
        (0x1310000, BASE + 0x2a3d458),
        (0x1300008, 0x1310008),
        (BASE + 0x3d669c8, u64::MAX - 7),
    ] {
        let (_, mut memory) = fixture();
        camera_fixture(&mut memory);
        memory.put64(address, value);
        assert!(object_catalog(&checked, &memory).cameras().is_err());
    }
    for address in [0x1310008, u64::MAX - 15] {
        let (_, mut memory) = fixture();
        camera_fixture(&mut memory);
        memory.put64(address, BASE + 0x2aa0a40);
        assert!(
            object_catalog(&checked, &memory)
                .perspective_camera(address)
                .is_err()
        );
    }
    for byte in [0x1310010, 0x131004c, 0x1310050, 0x131005f] {
        let (_, mut memory) = fixture();
        camera_fixture(&mut memory);
        memory.bytes.remove(&byte);
        assert!(
            object_catalog(&checked, &memory)
                .perspective_camera(0x1310000)
                .is_err()
        );
    }
}

#[test]
fn camera_detects_visible_singleton_link_flags_matrix_frustum_and_type_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for watched in [BASE + 0x3d669c8, 0x1300000, 0x1300008, 0x1300028, 0x1310000] {
        let (_, mut memory) = fixture();
        camera_fixture(&mut memory);
        memory.watch_address = watched;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory).cameras().is_err(),
            "{watched:x}"
        );
    }
    for watched in [0x1310000, 0x1310010, 0x131004c, 0x1310050, 0x131005c] {
        let (_, mut memory) = fixture();
        camera_fixture(&mut memory);
        memory.watch_address = watched;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .perspective_camera(0x1310000)
                .is_err(),
            "{watched:x}"
        );
    }
}

#[test]
fn camera_nonfinite_bits_remain_observable_but_frustum_validation_is_explicit() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for (index, value) in [
        (0, f32::NAN),
        (0, 0.0),
        (0, -0.0),
        (0, std::f32::consts::PI),
        (1, 0.0),
        (2, -1.0),
        (3, f32::INFINITY),
        (3, 0.05),
    ] {
        let (_, mut memory) = fixture();
        camera_fixture(&mut memory);
        memory.put(0x1310050 + index * 4, &value.to_bits().to_le_bytes());
        let snapshot = object_catalog(&checked, &memory)
            .perspective_camera(0x1310000)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.frustum_bits[index as usize], value.to_bits());
        assert!(snapshot.frustum().is_err());
    }
    let (_, mut memory) = fixture();
    camera_fixture(&mut memory);
    memory.put(0x1310010, &0x7fc12345u32.to_le_bytes());
    let snapshot = object_catalog(&checked, &memory)
        .perspective_camera(0x1310000)
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.matrix_bits[0][0], 0x7fc12345);
    assert!(snapshot.frustum().is_ok()); // Frustum validation does not pretend to validate axes.
}

fn control_channels_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    let target = controller_fixture(memory, ObjectKind::Player);
    memory.put64(0xe00018, 0x1400000);
    memory.put64(0xe003b0, 0x1400100);
    memory.put64(0x1400000, BASE + 0x2a2e788);
    memory.put64(0x14000a8, target.address);
    memory.put64(0x1400100, BASE + 0x2a2c8e8);
    memory.put64(0x14001a8, 0);
    // No ordinary +20, input padding, flags or native callback fields are readable.
    target
}

#[test]
fn control_channels_preserve_null_foreign_and_all_proved_primary_types() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for (table, kind) in [
        (0x2a2c8e8, ObjectKind::CharacterManipulator),
        (0x2a2cd70, ObjectKind::CommandManipulator),
        (0x2a2d400, ObjectKind::FollowManipulator),
        (0x2a2d858, ObjectKind::NetworkAiManipulator),
        (0x2a2e1b8, ObjectKind::NetworkManipulator),
        (0x2a2e788, ObjectKind::PadManipulator),
        (0x2a2eda0, ObjectKind::ReplayManipulator),
        (0x2a2f118, ObjectKind::RideManipulator),
    ] {
        let (_, mut memory) = fixture();
        let target = control_channels_fixture(&mut memory);
        memory.put64(0x1400100, BASE + table);
        let copy = object_catalog(&checked, &memory)
            .character_control_channels(target)
            .unwrap()
            .unwrap();
        assert_eq!(copy.ordinary.unwrap().character, Some(target));
        assert_eq!(copy.override_input.unwrap().character, None);
        assert_eq!(copy.override_input.unwrap().object.kind, kind);
        assert_eq!(copy.preferred_manipulator(), copy.override_input);
        // Another live identity is copied, never silently reassigned to the controller owner.
        memory.put64(0x620000, BASE + 0x2a47090);
        memory.put64(0x620008, 0x1234567890000003);
        memory.put64(0x14001a8, 0x620000);
        let input = object_catalog(&checked, &memory)
            .manipulator(0x1400100, target.epoch)
            .unwrap()
            .unwrap();
        assert_eq!(input.native_character_address, 0x620000);
        assert_eq!(input.character.unwrap().address, 0x620000);
    }
}

#[test]
fn control_channels_distinguish_no_controller_from_empty_or_shared_input_channels() {
    let (file, mut memory) = fixture();
    let target = control_channels_fixture(&mut memory);
    let checked = verified(&file);
    memory.put64(0xe003b0, 0);
    let channels = object_catalog(&checked, &memory)
        .character_control_channels(target)
        .unwrap()
        .unwrap();
    assert_eq!(channels.preferred_manipulator(), channels.ordinary);
    memory.put64(0xe003b0, 0x1400000);
    let channels = object_catalog(&checked, &memory)
        .character_control_channels(target)
        .unwrap()
        .unwrap();
    assert_eq!(channels.ordinary, channels.override_input);
    memory.put64(0xe00018, 0);
    memory.put64(0xe003b0, 0);
    let channels = object_catalog(&checked, &memory)
        .character_control_channels(target)
        .unwrap()
        .unwrap();
    assert_eq!(channels.preferred_manipulator(), None);
    memory.put64(target.address + 0x58, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_control_channels(target)
            .unwrap(),
        None
    );
    assert!(object_catalog(&checked, &memory).manipulator(0, 0).is_err());
}

#[test]
fn control_channels_reject_unknown_types_truncated_links_and_changed_complete_identities() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for (address, value) in [
        (0xe00018, 0x1400001),
        (0xe003b0, u64::MAX - 7),
        (0x1400000, BASE + 0x2a310c8),
        (0x14001a8, 0x610001),
        (0x14000a8, 0x1400100),
    ] {
        let (_, mut memory) = fixture();
        let target = control_channels_fixture(&mut memory);
        memory.put64(address, value);
        assert!(
            object_catalog(&checked, &memory)
                .character_control_channels(target)
                .is_err()
        );
    }
    for address in [
        0xe00018, 0xe003b0, 0x1400000, 0x14000a8, 0x1400100, 0x14001a8,
    ] {
        let (_, mut memory) = fixture();
        let target = control_channels_fixture(&mut memory);
        memory.bytes.remove(&(address + 7));
        assert!(
            object_catalog(&checked, &memory)
                .character_control_channels(target)
                .is_err()
        );
    }
}

#[test]
fn control_channels_detect_changes_to_each_link_owner_type_and_selected_controller() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for address in [
        0xe00018, 0xe003b0, 0x1400000, 0x14000a8, 0x1400100, 0x610008, 0x610058, 0xe00010, 0xe002d4,
    ] {
        let (_, mut memory) = fixture();
        let target = control_channels_fixture(&mut memory);
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .character_control_channels(target)
                .is_err(),
            "watch {address:#x}"
        );
    }
}

fn menu_state_fixture(memory: &mut SparseMemory) {
    memory.put64(BASE + 0x3d6f820, 0x1500000);
    memory.put64(0x1500000, BASE + 0x2a9eed8);
    memory.put64(0x1500720, BASE + 0x2a9ee90);
    memory.put(0x1500734, &7i32.to_le_bytes());
    for (i, value) in [-2.0f32, 6.0, 4.0, 1.5].into_iter().enumerate() {
        memory.put(0x1500738 + i as u64 * 4, &value.to_bits().to_le_bytes());
    }
    memory.put64(BASE + 0x3d64f28, 0x1501000);
    memory.put64(0x1501000, BASE + 0x29fc918);
    memory.put(0x15010ec, &[2, 255]);
    // Unused transition fields and helper +ee/+ef padding are deliberately unreadable.
}

#[test]
fn loading_sequence_preserves_signed_raw_choice_without_scene_or_array_access() {
    let (file, mut memory) = fixture();
    menu_state_fixture(&mut memory);
    let checked = verified(&file);
    for sequence in [i32::MIN, -1, 0, 33, 34, i32::MAX] {
        memory.put(0x15010d0, &sequence.to_le_bytes());
        let value = object_catalog(&checked, &memory)
            .loading_sequence(9)
            .unwrap()
            .unwrap();
        assert_eq!(value.native_sequence, sequence);
        assert_eq!(value.epoch, 9);
        assert_eq!(value.helper.address, 0x1501000);
    }
    assert!(
        object_catalog(&checked, &memory)
            .loading_sequence(0)
            .is_err()
    );
    memory.put64(BASE + 0x3d64f28, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .loading_sequence(9)
            .unwrap(),
        None
    );
}

#[test]
fn loading_sequence_refuses_changed_source_wrong_type_and_truncated_word() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for watched in [BASE + 0x3d64f28, 0x1501000, 0x15010d0] {
        let (_, mut memory) = fixture();
        menu_state_fixture(&mut memory);
        memory.put(0x15010d0, &33i32.to_le_bytes());
        memory.watch_address = watched;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .loading_sequence(9)
                .is_err(),
            "{watched:x}"
        );
    }
    let (_, mut memory) = fixture();
    menu_state_fixture(&mut memory);
    memory.put(0x15010d0, &33i32.to_le_bytes());
    memory.put64(0x1501000, BASE + 0x2aa0a08);
    assert!(
        object_catalog(&checked, &memory)
            .loading_sequence(9)
            .is_err()
    );
    memory.put64(0x1501000, BASE + 0x29fc918);
    memory.bytes.remove(&0x15010d3);
    assert!(
        object_catalog(&checked, &memory)
            .loading_sequence(9)
            .is_err()
    );
}

#[test]
fn menu_state_copies_native_selector_interpolation_and_exact_two_pulse_bytes() {
    let (file, mut memory) = fixture();
    menu_state_fixture(&mut memory);
    let checked = verified(&file);
    let objects = object_catalog(&checked, &memory);
    let menu = objects.menu_transition().unwrap().unwrap();
    assert_eq!(menu.native_selector, 7);
    assert_eq!(menu.value_bits.map(f32::from_bits), [-2.0, 6.0, 4.0, 1.5]);
    assert_eq!(menu.interpolated_value().unwrap(), 1.0);
    let pulses = objects.loading_helper_pulses().unwrap().unwrap();
    assert_eq!((pulses.current, pulses.previous), (2, 255));
    memory.put64(BASE + 0x3d6f820, 0);
    assert_eq!(
        object_catalog(&checked, &memory).menu_transition().unwrap(),
        None
    );
    assert!(
        object_catalog(&checked, &memory)
            .loading_helper_pulses()
            .unwrap()
            .is_some()
    );
    memory.put64(BASE + 0x3d64f28, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .loading_helper_pulses()
            .unwrap(),
        None
    );
}

#[test]
fn menu_interpolation_obeys_signed_gate_used_operands_and_native_branch_endpoints() {
    let (file, mut memory) = fixture();
    menu_state_fixture(&mut memory);
    let checked = verified(&file);
    let mut menu = object_catalog(&checked, &memory)
        .menu_transition()
        .unwrap()
        .unwrap();
    for selector in [i32::MIN, -1] {
        menu.native_selector = selector;
        menu.value_bits = [f32::NAN.to_bits(); 4];
        assert_eq!(menu.interpolated_value().unwrap(), 0.0);
    }
    menu.native_selector = i32::MAX;
    for (values, expected) in [
        ([3.0, f32::NAN, f32::NAN, -1.0], 3.0),
        ([3.0, 8.0, 4.0, -0.0], 3.0),
        ([3.0, 8.0, 4.0, 0.0], 3.0),
        ([f32::NAN, 8.0, 4.0, 4.0], 8.0),
        ([f32::NAN, 8.0, -1.0, 1.0], 8.0),
        ([1.0, 9.0, 8.0, 3.0], 4.0),
    ] {
        menu.value_bits = values.map(f32::to_bits);
        assert_eq!(menu.interpolated_value().unwrap(), expected);
    }
    for values in [
        [0.0, 1.0, 1.0, f32::NAN],
        [0.0, 1.0, f32::NAN, 1.0],
        [f32::NAN, 1.0, 1.0, 0.0],
        [0.0, f32::INFINITY, 1.0, 2.0],
        [-f32::MAX, f32::MAX, 1.0, 0.5],
    ] {
        menu.value_bits = values.map(f32::to_bits);
        assert!(menu.interpolated_value().is_err());
    }
}

#[test]
fn menu_state_refuses_foreign_types_bad_extent_and_truncated_selected_fields() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for (address, value) in [
        (BASE + 0x3d6f820, 0x1500001),
        (BASE + 0x3d64f28, u64::MAX - 7),
        (0x1500000, BASE + 0x29fc918),
        (0x1501000, BASE + 0x2a9eed8),
        (0x1500720, BASE + 0x2a9ee98),
    ] {
        let (_, mut memory) = fixture();
        menu_state_fixture(&mut memory);
        memory.put64(address, value);
        let objects = object_catalog(&checked, &memory);
        assert!(objects.menu_transition().is_err() || objects.loading_helper_pulses().is_err());
    }
    for address in [0x1500727, 0x1500737, 0x1500747, 0x15010ed] {
        let (_, mut memory) = fixture();
        menu_state_fixture(&mut memory);
        memory.bytes.remove(&address);
        let objects = object_catalog(&checked, &memory);
        assert!(objects.menu_transition().is_err() || objects.loading_helper_pulses().is_err());
    }
}

#[test]
fn menu_state_detects_visible_globals_primary_types_and_selected_payload_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for address in [BASE + 0x3d6f820, 0x1500000, 0x1500720, 0x1500734, 0x1500738] {
        let (_, mut memory) = fixture();
        menu_state_fixture(&mut memory);
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory).menu_transition().is_err(),
            "watch {address:#x}"
        );
    }
    for address in [BASE + 0x3d64f28, 0x1501000, 0x15010ec] {
        let (_, mut memory) = fixture();
        menu_state_fixture(&mut memory);
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .loading_helper_pulses()
                .is_err(),
            "watch {address:#x}"
        );
    }
}

fn spatial_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    let target = module_fixture(memory, CharacterModule::Physics);
    for (offset, bits) in [
        (0x50, [0, 0, 0, 1f32.to_bits()]),
        (0x60, [0x80000000, 0x7fc01234, 5, 6]),
        (
            0x70,
            [
                2f32.to_bits(),
                (-3f32).to_bits(),
                4f32.to_bits(),
                0x7fc05678,
            ],
        ),
        (
            0x80,
            [5f32.to_bits(), 6f32.to_bits(), 7f32.to_bits(), 0x80000000],
        ),
    ] {
        for (index, value) in bits.into_iter().enumerate() {
            memory.put(0xd00000 + offset + index as u64 * 4, &value.to_le_bytes());
        }
    }
    target
}
#[test]
fn spatial_copies_independent_rotation_vectors_position_and_preserves_fourth_lane() {
    let (file, mut memory) = fixture();
    let target = spatial_fixture(&mut memory);
    let checked = verified(&file);
    let value = object_catalog(&checked, &memory)
        .character_spatial(target)
        .unwrap()
        .unwrap();
    assert_eq!(value.primary_rotation_bits, [0, 0, 0, 1f32.to_bits()]);
    assert_eq!(
        value.secondary_rotation_bits,
        [0x80000000, 0x7fc01234, 5, 6]
    );
    assert_eq!(value.position_bits[3], 0x7fc05678);
    assert_eq!(
        value.secondary_position_bits,
        [5f32.to_bits(), 6f32.to_bits(), 7f32.to_bits(), 0x80000000]
    );
    assert_eq!(value.finite_position().unwrap(), [2.0, -3.0, 4.0]);
    memory.put(0xd00074, &f32::INFINITY.to_bits().to_le_bytes());
    let value = object_catalog(&checked, &memory)
        .character_spatial(target)
        .unwrap()
        .unwrap();
    assert!(value.finite_position().is_err());
    memory.put64(0xc00068, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_spatial(target)
            .unwrap(),
        None
    );
}
#[test]
fn spatial_refuses_unreadable_lanes_foreign_owner_type_and_complete_identity() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for address in [0xd0005f, 0xd0006f, 0xd0007f, 0xd0008f] {
        let (_, mut memory) = fixture();
        let target = spatial_fixture(&mut memory);
        memory.bytes.remove(&address);
        assert!(
            object_catalog(&checked, &memory)
                .character_spatial(target)
                .is_err()
        );
    }
    for (address, value) in [
        (0xd00000, BASE + 0x2a3d098),
        (0xd00008, 0x610010),
        (0x610008, 7),
    ] {
        let (_, mut memory) = fixture();
        let target = spatial_fixture(&mut memory);
        memory.put64(address, value);
        assert!(
            object_catalog(&checked, &memory)
                .character_spatial(target)
                .is_err()
        );
    }
}
#[test]
fn spatial_detects_visible_rotation_position_links_and_identity_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for address in [
        0xd00050, 0xd00060, 0xd00070, 0xd00080, 0xc00068, 0xd00008, 0x610008,
    ] {
        let (_, mut memory) = fixture();
        let target = spatial_fixture(&mut memory);
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .character_spatial(target)
                .is_err(),
            "watch {address:#x}"
        );
    }
}

fn debug_world_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    let target = control_channels_fixture(memory);
    memory.put64(BASE + 0x3d6a208, 0x1600000);
    memory.put64(0x1600000, BASE + 0x2a4ec48);
    memory.put64(0x16000a8, 0x1400000);
    memory.put64(0x16000b8, target.address);
    // Unrelated b0, field padding and the rest of debug object are deliberately unreadable.
    target
}
#[test]
fn debug_world_copies_typed_input_character_override_and_independent_null_links() {
    let (file, mut memory) = fixture();
    let target = debug_world_fixture(&mut memory);
    let checked = verified(&file);
    let value = object_catalog(&checked, &memory)
        .debug_world_links(target.epoch)
        .unwrap()
        .unwrap();
    assert_eq!(value.object.kind, ObjectKind::DebugWorld);
    assert_eq!(value.character_override, Some(target));
    assert_eq!(value.input.unwrap().character, Some(target));
    memory.put64(0x16000b8, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .debug_world_links(target.epoch)
            .unwrap()
            .unwrap()
            .character_override,
        None
    );
    memory.put64(0x16000a8, 0);
    let value = object_catalog(&checked, &memory)
        .debug_world_links(target.epoch)
        .unwrap()
        .unwrap();
    assert_eq!(value.input, None);
    memory.put64(BASE + 0x3d6a208, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .debug_world_links(target.epoch)
            .unwrap(),
        None
    );
}
#[test]
fn debug_world_rejects_bad_epoch_foreign_types_unreadable_links_and_unidentified_character() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for (address, value) in [
        (BASE + 0x3d6a208, u64::MAX - 7),
        (0x1600000, BASE + 0x2a2aff0),
        (0x16000a8, 0x1400001),
        (0x16000b8, 0x1400000),
        (BASE + 0x3d69ff8, 0),
    ] {
        let (_, mut memory) = fixture();
        let target = debug_world_fixture(&mut memory);
        memory.put64(address, value);
        assert!(
            object_catalog(&checked, &memory)
                .debug_world_links(target.epoch)
                .is_err()
        );
    }
    for address in [0x16000af, 0x16000bf, 0x14000af, 0x61000f] {
        let (_, mut memory) = fixture();
        let target = debug_world_fixture(&mut memory);
        memory.bytes.remove(&address);
        assert!(
            object_catalog(&checked, &memory)
                .debug_world_links(target.epoch)
                .is_err()
        );
    }
    let (_, mut memory) = fixture();
    debug_world_fixture(&mut memory);
    assert!(
        object_catalog(&checked, &memory)
            .debug_world_links(0)
            .is_err()
    );
}
#[test]
fn debug_world_detects_visible_singleton_input_override_and_complete_pointee_identity_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for address in [
        BASE + 0x3d6a208,
        0x1600000,
        0x16000a8,
        0x16000b8,
        0x1400000,
        0x14000a8,
        0x610008,
    ] {
        let (_, mut memory) = fixture();
        let target = debug_world_fixture(&mut memory);
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .debug_world_links(target.epoch)
                .is_err(),
            "watch {address:#x}"
        );
    }
}

fn debug_creator_fixture(memory: &mut SparseMemory, units: &[u16], capacity: u64) {
    memory.put64(BASE + 0x3d69ff8, 0x600000);
    memory.put64(0x600000, BASE + 0x2a4e980);
    memory.put64(0x61e648, 0x1700000);
    memory.put64(0x1700000, BASE + 0x2a51220);
    memory.put(0x1700040, &0i32.to_le_bytes());
    memory.put(0x1700044, &0i32.to_le_bytes());
    memory.put(0x1700048, &[0]);
    for index in 0..16u32 {
        memory.put(
            0x17000b0 + u64::from(index) * 4,
            &(0x7fc01230 + index).to_le_bytes(),
        );
    }
    for (index, value) in [1000i32, 2000, -1, 7].into_iter().enumerate() {
        memory.put(0x17000f0 + index as u64 * 4, &value.to_le_bytes());
    }
    let buffer = if capacity > 7 {
        memory.put64(0x1700148, 0x1710000);
        0x1710000
    } else {
        0x1700148
    };
    let bytes: Vec<_> = units
        .iter()
        .copied()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    memory.put(buffer, &bytes);
    memory.put64(0x1700158, units.len() as u64);
    memory.put64(0x1700160, capacity);
    memory.put64(0x1700170, u64::MAX); // Opaque borrowed argument stays undereferenced.
    memory.put(0x1700178, &[2, 0]);
    memory.put(0x170017c, &42i32.to_le_bytes());
    memory.put(0x1700180, &5u32.to_le_bytes());
    memory.put(0x170019c, &1i32.to_le_bytes());
    memory.put(0x17001a4, &(-1i32).to_le_bytes());
    memory.put(0x17001a8, &[1]);
    memory.put64(0x17001b0, u64::MAX); // Stale/unattributed result is not dereferenced.
    // Fixed UI name, object padding, and all other fields deliberately stay unmapped.
}

#[test]
fn debug_creator_dispatch_copies_fixed_table_only_and_rejects_late_header_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    let populate = |memory: &mut SparseMemory| {
        debug_creator_fixture(memory, &[99; 5], 7);
        memory.put64(0x1700008, BASE + 0x3d6a468);
        memory.put64(0x1700028, 9);
        memory.put64(0x1700058, u64::MAX);
        memory.put(0x1700061, &[0xff]);
        for (i, rva) in [
            0x52b540, 0x2a50b00, 0x52b5b0, 0x2a50b40, 0x52b5e0, 0x2a50b80,
        ]
        .into_iter()
        .enumerate()
        {
            memory.put64(BASE + 0x3d6a468 + i as u64 * 8, BASE + rva);
        }
        for (offset, rva) in [(0x18, 0x52b500), (0x20, 0x52bbc0), (0x30, 0x52baa0)] {
            memory.put64(BASE + 0x2a51220 + offset, BASE + rva);
        }
    };
    let (_, mut memory) = fixture();
    populate(&mut memory);
    let state = object_catalog(&checked, &memory)
        .debug_creator_dispatch(9)
        .unwrap()
        .unwrap();
    assert_eq!(state.dependency_count, 9);
    assert_eq!(state.waiting_queue, u64::MAX);
    assert_eq!(state.special_mode, 0xff);
    assert_eq!(state.descriptors[4], BASE + 0x52b5e0);
    // Unknown counters/pointers are copied, never traversed or called by this reader.
    for address in [
        0x1700008,
        0x1700028,
        0x1700058,
        0x1700061,
        BASE + 0x3d6a468,
        BASE + 0x3d6a490,
        BASE + 0x2a51250,
    ] {
        let (_, mut changing) = fixture();
        populate(&mut changing);
        changing.watch_address = address;
        changing.change_slot = true;
        assert!(
            object_catalog(&checked, &changing)
                .debug_creator_dispatch(9)
                .is_err(),
            "{address:#x}"
        );
    }
    for pointer in [0, 1, BASE + 0x3d6a470, u64::MAX - 7] {
        let (_, mut foreign) = fixture();
        populate(&mut foreign);
        foreign.put64(0x1700008, pointer);
        assert!(
            object_catalog(&checked, &foreign)
                .debug_creator_dispatch(9)
                .is_err()
        );
    }
    memory.bytes.remove(&(BASE + 0x3d6a497));
    assert!(
        object_catalog(&checked, &memory)
            .debug_creator_dispatch(9)
            .is_err()
    );
}

#[test]
fn debug_creator_copies_only_consumed_input_and_keeps_results_unattributed() {
    for (units, capacity) in [
        (vec![], 7),
        (vec![99, 48, 48, 49, 48], 7),
        (vec![0xd800, 0, 0xffff, 0xdc00, 1, 2, 3], 7),
        (vec![99; 31], 31),
    ] {
        let (file, mut memory) = fixture();
        debug_creator_fixture(&mut memory, &units, capacity);
        let checked = verified(&file);
        let objects = object_catalog(&checked, &memory);
        let value = objects.debug_creator(9).unwrap().unwrap();
        assert_eq!(value.object.kind, ObjectKind::DebugCharacterCreator);
        assert_eq!(value.input.name_units, units);
        assert_eq!(value.input.name_capacity, capacity);
        assert_eq!(
            value.input.position_bits,
            [0x7fc01230, 0x7fc01231, 0x7fc01232, 0x7fc01233]
        );
        assert_eq!(value.input.rotation_bits[0], 0x7fc01234);
        assert_eq!(value.input.auxiliary30_bits[3], 0x7fc0123f);
        assert_eq!(
            (value.input.npc_param_id, value.input.npc_think_param_id),
            (1000, 2000)
        );
        assert_eq!(
            [
                value.input.event_entity_id as i32,
                value.input.auxiliary_id4c
            ],
            [-1, 7]
        );
        assert_eq!(value.input.native_argument, u64::MAX);
        assert_eq!(value.input.kind_bytes, [2, 0]);
        assert_eq!(
            (value.count, value.animation_id, value.phase_byte),
            (1, -1, 1)
        );
        assert_eq!(value.last_created_address, u64::MAX);
        memory.put64(0x61e648, 0);
        assert_eq!(
            object_catalog(&checked, &memory).debug_creator(9).unwrap(),
            None
        );
        memory.put64(BASE + 0x3d69ff8, 0);
        assert_eq!(
            object_catalog(&checked, &memory).debug_creator(9).unwrap(),
            None
        );
    }
}

#[test]
fn debug_creator_rejects_invalid_name_storage_without_truncating_or_repairing_it() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for (address, value) in [
        (0x1700158, 32),
        (0x1700158, 8),
        (0x1700160, 6),
        (0x1700160, 32),
        (0x1700148, 0),
        (0x1700148, 0x1710001),
        (0x1700148, u64::MAX - 1),
    ] {
        let (_, mut memory) = fixture();
        debug_creator_fixture(
            &mut memory,
            &[99; 5],
            if address == 0x1700148 { 31 } else { 7 },
        );
        memory.put64(address, value);
        assert!(
            object_catalog(&checked, &memory).debug_creator(9).is_err(),
            "{address:#x}/{value:#x}"
        );
    }
    let (_, mut memory) = fixture();
    debug_creator_fixture(&mut memory, &[99; 5], 7);
    memory.put(0x1700152, &[1, 0]);
    assert!(object_catalog(&checked, &memory).debug_creator(9).is_err());
}

#[test]
fn debug_creator_refuses_foreign_truncated_unaligned_and_overflowed_storage() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for (address, value) in [
        (0x1700000, BASE + 0x2a4ec48),
        (0x600000, BASE + 0x2a51220),
        (0x61e648, 0x1700001),
        (0x61e648, u64::MAX - 7),
    ] {
        let (_, mut memory) = fixture();
        debug_creator_fixture(&mut memory, &[99; 5], 31);
        memory.put64(address, value);
        assert!(object_catalog(&checked, &memory).debug_creator(9).is_err());
    }
    for address in [
        0x1700043, 0x1700047, 0x1700048, 0x17000bf, 0x17000ff, 0x170015f, 0x1700177, 0x1700179,
        0x1700183, 0x170019f, 0x17001a8, 0x17001b7, 0x171000b,
    ] {
        let (_, mut memory) = fixture();
        debug_creator_fixture(&mut memory, &[99; 5], 31);
        memory.bytes.remove(&address);
        assert!(
            object_catalog(&checked, &memory).debug_creator(9).is_err(),
            "{address:#x}"
        );
    }
    let (_, mut memory) = fixture();
    debug_creator_fixture(&mut memory, &[99; 5], 7);
    assert!(object_catalog(&checked, &memory).debug_creator(0).is_err());
}

#[test]
fn debug_creator_detects_visible_world_link_name_payload_and_result_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for watched in [
        BASE + 0x3d69ff8,
        0x600000,
        0x61e648,
        0x1700000,
        0x1700040,
        0x1700044,
        0x1700048,
        0x17000b0,
        0x17000c0,
        0x17000d0,
        0x17000e0,
        0x17000f0,
        0x1700148,
        0x1700158,
        0x1700160,
        0x1700170,
        0x1700178,
        0x170017c,
        0x1700180,
        0x170019c,
        0x17001a4,
        0x17001a8,
        0x17001b0,
        0x1710000,
    ] {
        let (_, mut memory) = fixture();
        debug_creator_fixture(&mut memory, &[99; 5], 31);
        if matches!(watched, 0x1700040 | 0x1700044) {
            memory.put(watched, &1i32.to_le_bytes());
        }
        if watched == 0x1700048 {
            memory.put(watched, &[1]);
        }
        memory.watch_address = watched;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory).debug_creator(9).is_err(),
            "{watched:#x}"
        );
    }
}

fn event_mapping_fixture(memory: &mut SparseMemory) -> (u64, character_sets::CharacterIdentity) {
    let (set, target) = registration_fixture(memory);
    memory.put(target.address + 0x1e8, &0x80000000u32.to_le_bytes());
    for (offset, head, root, count) in [
        (0x28, 0x1600000, 0x1600040, 1),
        (0x40, 0x1700000, 0x1700040, 3),
    ] {
        memory.put64(set + offset + 8, head);
        memory.put64(set + offset + 16, count);
        memory.put64(head + 8, root);
        memory.put(head + 25, &[1]);
        for node in [root, root + 0x40, root + 0x80] {
            memory.put64(node, head);
            memory.put64(node + 16, head);
            memory.put(node + 25, &[0]);
            memory.put(node + 32, &0x80000000u32.to_le_bytes());
            memory.put64(node + 40, 0xd00020);
        }
    }
    memory.put64(0x1700040, 0x1700080);
    memory.put64(0x1700050, 0x17000c0);
    // A foreign entry shares the same group key; value stays opaque.
    memory.put64(0x17000e8, u64::MAX);
    (set, target)
}
#[test]
fn event_maps_preserve_full_unsigned_id_unique_entity_and_repeated_group_memberships() {
    let (bytes, mut memory) = fixture();
    let (set, target) = event_mapping_fixture(&mut memory);
    let checked = verified(&bytes);
    let observed = object_catalog(&checked, &memory)
        .character_event_mappings(target, set, 3)
        .unwrap()
        .unwrap();
    assert_eq!(observed.id.stored_id, 0x80000000);
    assert_eq!(observed.entity_entry, Some(0xd00020));
    assert_eq!(observed.entities.len(), 1);
    assert_eq!(
        observed.entity_aliases().collect::<Vec<_>>(),
        observed.entities
    );
    assert_eq!(observed.groups.len(), 2);
    assert_eq!(observed.group_entries.len(), 3);
    assert_eq!(observed.group_entries[2].value, u64::MAX);
    assert!(
        observed
            .groups
            .iter()
            .all(|x| x.key == 0x80000000 && x.value == 0xd00020)
    );
    memory.put64(0x1600068, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_event_mappings(target, set, 3)
            .unwrap()
            .unwrap()
            .entity_entry,
        Some(0)
    );
    memory.put(target.address + 0x1e8, &0u32.to_le_bytes());
    let absent = object_catalog(&checked, &memory)
        .character_event_mappings(target, set, 3)
        .unwrap()
        .unwrap();
    assert_eq!(absent.id.stored_id, 0);
    assert_eq!(absent.entity_entry, None);
    assert_eq!(absent.groups.len(), 2); // model groups need not equal the stored field.
    memory.put64(0xd00020, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_event_mappings(target, set, 3)
            .unwrap(),
        None
    );
}
#[test]
fn entity_aliases_copy_all_keys_without_using_stored_field_or_dereferencing_foreign_values() {
    let (bytes, mut memory) = fixture();
    let (set, target) = event_mapping_fixture(&mut memory);
    memory.put64(set + 0x38, 3);
    memory.put64(0x1600040, 0x1600080);
    memory.put64(0x1600050, 0x16000c0);
    memory.put(0x16000a0, &1u32.to_le_bytes());
    memory.put(0x16000e0, &u32::MAX.to_le_bytes());
    memory.put64(0x1600068, u64::MAX); // Stored ID maps to an opaque foreign value.
    let checked = verified(&bytes);
    let observed = object_catalog(&checked, &memory)
        .character_event_mappings(target, set, 3)
        .unwrap()
        .unwrap();
    assert_eq!(observed.entity_entry, Some(u64::MAX));
    assert_eq!(observed.entities.len(), 3);
    assert_eq!(
        observed
            .entity_aliases()
            .map(|entry| entry.key)
            .collect::<Vec<_>>(),
        vec![1, u32::MAX]
    );
    assert_eq!(observed.groups.len(), 2);
    memory.put(target.address + 0x1e8, &0u32.to_le_bytes());
    let zero = object_catalog(&checked, &memory)
        .character_event_mappings(target, set, 3)
        .unwrap()
        .unwrap();
    assert_eq!(zero.entity_entry, None);
    assert_eq!(zero.entity_aliases().count(), 2);
    assert!(
        object_catalog(&checked, &memory)
            .character_event_mappings(target, set, 2)
            .is_err()
    );
    for watched in [0x16000a0, 0x16000e0, 0x1600068, 0x16000a8, 0x16000e8] {
        memory.watch_address = watched;
        memory.slot_reads.set(0);
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .character_event_mappings(target, set, 3)
                .is_err(),
            "{watched:#x}"
        );
        memory.change_slot = false;
    }
}
#[test]
fn event_maps_reject_duplicate_entity_keys_corrupt_links_and_unavailable_payload() {
    let (bytes, _) = fixture();
    let checked = verified(&bytes);
    for variant in 0..6 {
        let (_, mut memory) = fixture();
        let (set, target) = event_mapping_fixture(&mut memory);
        match variant {
            0 => {
                memory.put64(set + 0x38, 2);
                memory.put64(0x1600040, 0x1600080);
            }
            1 => memory.put64(0x1700080, 0x1700080),
            2 => memory.put64(set + 0x50, 4),
            3 => {
                memory.bytes.remove(&(target.address + 0x1eb));
            }
            4 => memory.put64(target.address + 0x10, 0xd00000),
            5 => memory.put64(set, BASE + 0x2a7fbb0),
            _ => unreachable!(),
        }
        assert!(
            object_catalog(&checked, &memory)
                .character_event_mappings(target, set, 3)
                .is_err()
        );
    }
}
#[test]
fn event_maps_refuse_visible_id_mapping_storage_and_identity_changes() {
    let (bytes, _) = fixture();
    let checked = verified(&bytes);
    for watched in [
        0x6201e8, 0x1600068, 0x17000a8, 0x1700080, 0x620008, 0x620010,
    ] {
        let (_, mut memory) = fixture();
        let (set, target) = event_mapping_fixture(&mut memory);
        memory.watch_address = watched;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .character_event_mappings(target, set, 3)
                .is_err(),
            "{watched:x}"
        );
    }
    let (_, mut memory) = fixture();
    let (set, target) = event_mapping_fixture(&mut memory);
    for limit in [0, 65537] {
        assert!(
            object_catalog(&checked, &memory)
                .character_event_mappings(target, set, limit)
                .is_err()
        );
    }
}

#[test]
fn creator_schedule_preserves_unknown_states_without_inferring_resource_ready() {
    use crate::debug_creator::DebugCreatorStep as S;
    let (file, _) = fixture();
    let checked = verified(&file);
    for (current, next, advance) in [
        (0, 1, 0),
        (1, 2, 1),
        (2, 0, 255),
        (-1, i32::MAX, 2),
        (i32::MIN, -2, 0),
    ] {
        let (_, mut memory) = fixture();
        debug_creator_fixture(&mut memory, &[99; 5], 7);
        memory.put(0x1700040, &current.to_le_bytes());
        memory.put(0x1700044, &next.to_le_bytes());
        memory.put(0x1700048, &[advance]);
        let observed = object_catalog(&checked, &memory)
            .debug_creator(9)
            .unwrap()
            .unwrap();
        assert_eq!(observed.schedule.current_step, current);
        assert_eq!(observed.schedule.next_step, next);
        assert_eq!(observed.schedule.advance_byte, advance);
        assert_eq!(observed.phase_byte, 1); // Phase marker alone does not classify current step.
    }
    for (raw, step) in [
        (0, S::Idle),
        (1, S::Load),
        (2, S::LoadWait),
        (-1, S::Other(-1)),
        (3, S::Other(3)),
        (i32::MIN, S::Other(i32::MIN)),
    ] {
        assert_eq!(S::from_raw(raw), step);
    }
}

#[test]
fn character_status_bytes_are_independent_raw_copies_with_full_identity_rechecks() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for bytes in [
        [0, 0, 0, 0, 0, 0],
        [0x80, 8, 1, 0, 2, 1],
        [255, 0x7f, 0x80, 0x38, 255, 0xfe],
    ] {
        let (_, mut memory) = fixture();
        controller_fixture(&mut memory, ObjectKind::Player);
        for (offset, value) in [0x1c4, 0x1c5, 0x1c8, 0x538, 0x6c, 0x539]
            .into_iter()
            .zip(bytes)
        {
            memory.put(0x610000 + offset, &[value]);
        }
        let objects = object_catalog(&checked, &memory);
        let target = objects.character_identity(0x610000, 7).unwrap().unwrap();
        assert_eq!(
            objects.character_flags(target).unwrap().unwrap().bytes,
            bytes
        );
        let mut foreign = target;
        foreign.native_identifier += 1;
        assert!(objects.character_flags(foreign).is_err());
    }
}
#[test]
fn character_status_bytes_refuse_truncated_storage_and_visible_identity_payload_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for watched in [
        0x6101c4, 0x6101c5, 0x6101c8, 0x610538, 0x61006c, 0x610539, 0x610008,
    ] {
        let (_, mut memory) = fixture();
        controller_fixture(&mut memory, ObjectKind::Player);
        for offset in [0x1c4, 0x1c5, 0x1c8, 0x538, 0x6c, 0x539] {
            memory.put(0x610000 + offset, &[255]);
        }
        let target = object_catalog(&checked, &memory)
            .character_identity(0x610000, 7)
            .unwrap()
            .unwrap();
        memory.watch_address = watched;
        memory.change_slot = true;
        memory.slot_reads.set(0);
        assert!(
            object_catalog(&checked, &memory)
                .character_flags(target)
                .is_err()
        );
        memory.change_slot = false;
        memory.bytes.remove(&watched);
        assert!(
            object_catalog(&checked, &memory)
                .character_flags(target)
                .is_err()
        );
    }
}

#[test]
fn character_update_group_copies_gate_and_group_and_rejects_whole_graph_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for watched in [
        0,
        0x6100b4,
        BASE + 0x3b1bffe,
        BASE + 0x3d6a208,
        0x1600000,
        0x6101c4,
        0x610008,
    ] {
        let (_, mut memory) = fixture();
        controller_fixture(&mut memory, ObjectKind::Player);
        for offset in [0x1c4, 0x1c5, 0x1c8, 0x538, 0x6c, 0x539] {
            memory.put(0x610000 + offset, &[0xa5]);
        }
        memory.put(0x6100b4, &u32::MAX.to_le_bytes());
        memory.put(BASE + 0x3b1bffe, &[255]);
        memory.put64(BASE + 0x3d6a208, 0x1600000);
        memory.put64(0x1600000, BASE + 0x2a4ec48);
        let target = object_catalog(&checked, &memory)
            .character_identity(0x610000, 7)
            .unwrap()
            .unwrap();
        let before = object_catalog(&checked, &memory)
            .character_update_group(target)
            .unwrap()
            .unwrap();
        assert_eq!(before.group_id, u32::MAX);
        assert_eq!(before.debug_switch, 255);
        if watched != 0 {
            memory.watch_address = watched;
            memory.change_slot = true;
            memory.slot_reads.set(0);
            assert!(
                object_catalog(&checked, &memory)
                    .character_update_group(target)
                    .is_err(),
                "watch {watched:#x}"
            );
        } else {
            memory.put(BASE + 0x3b1bffe, &[0]);
            assert_eq!(
                object_catalog(&checked, &memory)
                    .character_update_group(target)
                    .unwrap()
                    .unwrap()
                    .debug_switch,
                0
            );
            memory.bytes.remove(&(BASE + 0x3b1bffe));
            assert!(
                object_catalog(&checked, &memory)
                    .character_update_group(target)
                    .is_err()
            );
            memory.put(BASE + 0x3b1bffe, &[1]);
            memory.put64(BASE + 0x3d6a208, 0);
            assert!(
                object_catalog(&checked, &memory)
                    .character_update_group(target)
                    .unwrap()
                    .is_none()
            );
        }
    }
}

#[test]
fn primary_target_selection_preserves_raw_handle_and_rechecks_primary_world_identity() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for watched in [
        0,
        0x6106b0,
        0x610008,
        0x610000,
        BASE + 0x3d69ff8,
        0x600000 + 0x1e508,
    ] {
        let (_, mut memory) = fixture();
        let target = controller_fixture(&mut memory, ObjectKind::Player);
        memory.put64(target.world_address + 0x1e508, target.address);
        memory.put64(target.address + 0x6b0, 0x1234567810000003);
        let before = object_catalog(&checked, &memory)
            .primary_player_target(target.epoch)
            .unwrap()
            .unwrap();
        assert_eq!(before.player, target);
        assert_eq!(before.handle.0, 0x1234567810000003);
        if watched != 0 {
            memory.watch_address = watched;
            memory.change_slot = true;
            memory.slot_reads.set(0);
            assert!(
                object_catalog(&checked, &memory)
                    .primary_player_target(target.epoch)
                    .is_err(),
                "watch {watched:#x}"
            );
        } else {
            for raw in [u64::MAX, 0x12345678ffffffff, 0] {
                memory.put64(target.address + 0x6b0, raw);
                assert_eq!(
                    object_catalog(&checked, &memory)
                        .primary_player_target(target.epoch)
                        .unwrap()
                        .unwrap()
                        .handle
                        .0,
                    raw
                );
            }
            memory.bytes.remove(&(target.address + 0x6b0));
            assert!(
                object_catalog(&checked, &memory)
                    .primary_player_target(target.epoch)
                    .is_err()
            );
            memory.put64(target.world_address + 0x1e508, 0);
            assert!(
                object_catalog(&checked, &memory)
                    .primary_player_target(target.epoch)
                    .unwrap()
                    .is_none()
            );
            assert!(
                object_catalog(&checked, &memory)
                    .primary_player_target(0)
                    .is_err()
            );
        }
    }
}

#[test]
fn effect_source_preserves_opaque_words_and_rejects_truncation_or_identity_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for watched in [0, 0x610180, 0x610008, 0x610000, BASE + 0x3d69ff8] {
        let (_, mut memory) = fixture();
        let target = controller_fixture(&mut memory, ObjectKind::Player);
        memory.put64(target.address + 0x180, 0x1234567810000003);
        let before = object_catalog(&checked, &memory)
            .last_effect_source(target)
            .unwrap()
            .unwrap();
        assert_eq!(before.character, target);
        assert_eq!(before.handle.0, 0x1234567810000003);
        if watched != 0 {
            memory.watch_address = watched;
            memory.change_slot = true;
            memory.slot_reads.set(0);
            assert!(
                object_catalog(&checked, &memory)
                    .last_effect_source(target)
                    .is_err()
            );
        } else {
            for raw in [u64::MAX, 0x12345678ffffffff, 0] {
                memory.put64(target.address + 0x180, raw);
                assert_eq!(
                    object_catalog(&checked, &memory)
                        .last_effect_source(target)
                        .unwrap()
                        .unwrap()
                        .handle
                        .0,
                    raw
                );
            }
            let mut stale = target;
            stale.native_identifier ^= 1 << 32;
            assert!(
                object_catalog(&checked, &memory)
                    .last_effect_source(stale)
                    .is_err()
            );
            memory.bytes.remove(&(target.address + 0x180));
            assert!(
                object_catalog(&checked, &memory)
                    .last_effect_source(target)
                    .is_err()
            );
        }
    }
}

#[test]
fn physics_suppression_copies_raw_request_and_independent_engine_result() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for bytes in [[0, 0, 0, 0], [0x81, 0x42, 1, 0], [255, 2, 0, 255]] {
        let (_, mut memory) = fixture();
        let target = module_fixture(&mut memory, CharacterModule::Physics);
        memory.put(0xd001d0, &bytes);
        let objects = object_catalog(&checked, &memory);
        let observed = objects.physics_suppression(target).unwrap().unwrap();
        assert_eq!(observed.bytes, bytes);
        assert_eq!(observed.effective_byte(), bytes[2]);
        assert_eq!(observed.request_byte(), bytes[3]);
        let mut foreign = target;
        foreign.native_identifier ^= 1;
        assert!(objects.physics_suppression(foreign).is_err());
        memory.put64(0xc00068, 0);
        assert_eq!(
            object_catalog(&checked, &memory)
                .physics_suppression(target)
                .unwrap(),
            None
        );
    }
}

#[test]
fn physics_suppression_refuses_incomplete_payload_and_changed_owner_or_module() {
    struct Changed<'a> {
        memory: &'a SparseMemory,
        watched: u64,
        reads: Cell<u32>,
    }
    impl AddressSpace for Changed<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if address <= self.watched && self.watched < address + output.len() as u64 {
                let prior = self.reads.replace(self.reads.get() + 1);
                if prior > 0 {
                    output[(self.watched - address) as usize] ^= 1;
                }
            }
            Ok(())
        }
    }
    let (file, _) = fixture();
    let checked = verified(&file);
    for watched in [
        0xd001d0, 0xd001d1, 0xd001d2, 0xd001d3, 0xd00008, 0xc00068, 0x610008,
    ] {
        let (_, mut memory) = fixture();
        let target = module_fixture(&mut memory, CharacterModule::Physics);
        memory.put(0xd001d0, &[255; 4]);
        let changed = Changed {
            memory: &memory,
            watched,
            reads: Cell::new(0),
        };
        let objects = GameObjectCatalog {
            module: checked
                .bind(
                    &changed,
                    BASE,
                    &[CodeRange {
                        rva: 0x1000,
                        length: 1,
                    }],
                )
                .unwrap(),
        };
        assert!(objects.physics_suppression(target).is_err());
        memory.bytes.remove(&watched);
        assert!(
            object_catalog(&checked, &memory)
                .physics_suppression(target)
                .is_err()
        );
    }
}

fn controller_policy_fixture(
    memory: &mut SparseMemory,
    kind: ObjectKind,
) -> character_sets::CharacterIdentity {
    let target = controller_fixture(memory, kind);
    memory.put(0xe000e8, &[0x60]);
    memory.put64(0xe000c8, 0x1800000);
    memory.put64(0x1800000, 0xe00000); // Nonpolymorphic owner pointer, not a vtable.
    memory.put(0x1800018, &0x98765434u32.to_le_bytes());
    memory.put(0x180001c, &0x80000400u32.to_le_bytes());
    memory.put(0x1800024, &0x12345671u32.to_le_bytes());
    target
}
#[test]
fn controller_policy_copies_nonpolymorphic_owned_modifier_and_optional_absence() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for kind in [ObjectKind::Character, ObjectKind::Enemy, ObjectKind::Player] {
        let (_, mut memory) = fixture();
        let target = controller_policy_fixture(&mut memory, kind);
        let observed = object_catalog(&checked, &memory)
            .controller_policy(target)
            .unwrap()
            .unwrap();
        assert_eq!(observed.low_byte, 0x60);
        assert_eq!(observed.modifier_word10, Some(0x98765434));
        assert_eq!(observed.lock_target_suppression_requested(), Some(true));
        assert_eq!(observed.modifier_word14, Some(0x80000400));
        assert_eq!(observed.modifier_word1c, Some(0x12345671));
        assert_eq!(observed.event_dead_marked(), Some(true));
        memory.put64(0xe000c8, 0);
        let observed = object_catalog(&checked, &memory)
            .controller_policy(target)
            .unwrap()
            .unwrap();
        assert_eq!(observed.modifier_address, 0);
        assert_eq!(observed.modifier_word10, None);
        assert_eq!(observed.lock_target_suppression_requested(), None);
        assert_eq!(observed.modifier_word14, None);
        assert_eq!(observed.modifier_word1c, None);
        assert_eq!(observed.event_dead_marked(), None);
        assert_eq!(observed.low_byte, 0x60);
        memory.put64(0x610058, 0);
        assert_eq!(
            object_catalog(&checked, &memory)
                .controller_policy(target)
                .unwrap(),
            None
        );
    }
}
#[test]
fn controller_policy_refuses_foreign_owner_and_detectable_link_payload_identity_changes() {
    struct Changed<'a> {
        memory: &'a SparseMemory,
        watched: u64,
        reads: Cell<u32>,
    }
    impl AddressSpace for Changed<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if address <= self.watched && self.watched < address + output.len() as u64 {
                let prior = self.reads.replace(self.reads.get() + 1);
                if prior > 0 {
                    output[(self.watched - address) as usize] ^= 1;
                }
            }
            Ok(())
        }
    }
    let (file, _) = fixture();
    let checked = verified(&file);
    for watched in [
        0xe000e8, 0xe000c8, 0x1800000, 0x180001c, 0x180001d, 0x180001e, 0x180001f, 0xe002d4,
        0x610008, 0x1800024, 0x1800025, 0x1800026, 0x1800027,
    ] {
        let (_, mut memory) = fixture();
        let target = controller_policy_fixture(&mut memory, ObjectKind::Player);
        let changed = Changed {
            memory: &memory,
            watched,
            reads: Cell::new(0),
        };
        let objects = GameObjectCatalog {
            module: checked
                .bind(
                    &changed,
                    BASE,
                    &[CodeRange {
                        rva: 0x1000,
                        length: 1,
                    }],
                )
                .unwrap(),
        };
        assert!(objects.controller_policy(target).is_err());
        memory.bytes.remove(&watched);
        assert!(
            object_catalog(&checked, &memory)
                .controller_policy(target)
                .is_err()
        );
    }
    for pointer in [0x1800001, u64::MAX - 7] {
        let (_, mut memory) = fixture();
        let target = controller_policy_fixture(&mut memory, ObjectKind::Enemy);
        memory.put64(0xe000c8, pointer);
        assert!(
            object_catalog(&checked, &memory)
                .controller_policy(target)
                .is_err()
        );
    }
    let (_, mut memory) = fixture();
    let target = controller_policy_fixture(&mut memory, ObjectKind::Enemy);
    memory.put64(0x1800000, 0x610000);
    assert!(
        object_catalog(&checked, &memory)
            .controller_policy(target)
            .is_err()
    );
}

fn opacity_fixture(
    memory: &mut SparseMemory,
    kind: ObjectKind,
    bits: [u32; 4],
) -> character_sets::CharacterIdentity {
    let target = controller_fixture(memory, kind);
    for (offset, value) in [0x24c, 0x238, 0x240, 0x248].into_iter().zip(bits) {
        memory.put(target.address + offset, &value.to_le_bytes());
    }
    for (offset, value) in
        [0x23c, 0x244, 0x250]
            .into_iter()
            .zip([0x7fc12345u32, 0xbf000000, 0x80000000])
    {
        memory.put(target.address + offset, &value.to_le_bytes());
    }
    target
}
#[test]
fn opacity_factors_and_scalar_storage_preserve_raw_bits_for_all_character_types() {
    use character_scalars::{CharacterScalar as F, CharacterScalarKey};
    let (file, _) = fixture();
    let checked = verified(&file);
    for kind in [ObjectKind::Character, ObjectKind::Enemy, ObjectKind::Player] {
        for bits in [
            [0; 4],
            [0x3f800000, 0x3f000000, 0x40000000, 0x80000000],
            [0x7fc12345, 1, 0xff800000, 0x7f800000],
        ] {
            let (_, mut memory) = fixture();
            let target = opacity_fixture(&mut memory, kind, bits);
            let objects = object_catalog(&checked, &memory);
            let observed = objects.character_opacity(target).unwrap().unwrap();
            assert_eq!(observed.kind, kind);
            assert_eq!(observed.factor_bits, bits);
            assert_eq!(observed.control_bits, [0x7fc12345, 0xbf000000, 0x80000000]);
            assert_eq!(observed.factors()[3].to_bits(), bits[3]);
            for (field, bits) in [
                F::BaseOpacity,
                F::OpacityFactor0,
                F::OpacityFactor1,
                F::OpacityFactor2,
            ]
            .into_iter()
            .zip(bits)
            .chain([
                (F::OpacityControl244, 0xbf000000),
                (F::BaseOpacityControl250, 0x80000000),
                (F::OpacityWord23c, 0x7fc12345),
            ]) {
                let scalar = objects
                    .character_scalar(CharacterScalarKey {
                        character: target,
                        field,
                    })
                    .unwrap()
                    .unwrap();
                assert_eq!(scalar.storage.address, target.address);
                assert_eq!(scalar.storage.kind, kind);
                assert_eq!(scalar.storage.container_address, 0);
                assert_eq!(scalar.bits, bits);
            }
        }
    }
}
#[test]
fn opacity_copy_refuses_truncation_and_visible_payload_type_or_identity_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for watched in [
        0x61024c, 0x610238, 0x610240, 0x610248, 0x610000, 0x610008, 0x61023c, 0x610244, 0x610250,
    ] {
        let (_, mut memory) = fixture();
        let target = opacity_fixture(&mut memory, ObjectKind::Player, [0x3f800000; 4]);
        memory.watch_address = watched;
        memory.change_slot = true;
        memory.slot_reads.set(0);
        assert!(
            object_catalog(&checked, &memory)
                .character_opacity(target)
                .is_err()
        );
        memory.change_slot = false;
        memory.bytes.remove(&watched);
        assert!(
            object_catalog(&checked, &memory)
                .character_opacity(target)
                .is_err()
        );
    }
    let (_, mut memory) = fixture();
    let target = opacity_fixture(&mut memory, ObjectKind::Enemy, [0; 4]);
    let mut foreign = target;
    foreign.native_identifier ^= 1;
    assert!(
        object_catalog(&checked, &memory)
            .character_opacity(foreign)
            .is_err()
    );
}

#[test]
fn motion_observation_combines_typed_spatial_and_control_graphs_and_detects_changes() {
    let (file, mut memory) = fixture();
    let target = spatial_fixture(&mut memory);
    assert_eq!(control_channels_fixture(&mut memory), target);
    let checked = verified(&file);
    let motion = object_catalog(&checked, &memory)
        .character_motion(target)
        .unwrap()
        .unwrap();
    assert_eq!(
        motion.spatial,
        object_catalog(&checked, &memory)
            .character_spatial(target)
            .unwrap()
            .unwrap()
    );
    assert_eq!(
        motion.controls,
        object_catalog(&checked, &memory)
            .character_control_channels(target)
            .unwrap()
            .unwrap()
    );
    memory.put64(0xe00018, 0);
    assert!(
        object_catalog(&checked, &memory)
            .character_motion(target)
            .unwrap()
            .unwrap()
            .controls
            .ordinary
            .is_none()
    );
    memory.put64(target.address + 0x58, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_motion(target)
            .unwrap(),
        None
    );
    for address in [0xd00070, 0xd00080, 0xe00018, 0xe003b0, 0x14000a8, 0x610008] {
        let (_, mut memory) = fixture();
        let target = spatial_fixture(&mut memory);
        control_channels_fixture(&mut memory);
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .character_motion(target)
                .is_err(),
            "watch {address:#x}"
        );
    }
}

#[test]
fn placement_observation_copies_raw_owner_caches_and_rechecks_full_graph() {
    fn setup(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
        let target = spatial_fixture(memory);
        control_channels_fixture(memory);
        for offset in [0x80, 0x90, 0xa0, 0xd0] {
            for (lane, bits) in [0x7fc01234u32, 0x80000000, 0x7f800000, offset as u32]
                .into_iter()
                .enumerate()
            {
                memory.put(
                    target.address + offset + lane as u64 * 4,
                    &bits.to_le_bytes(),
                );
            }
        }
        memory.put(target.address + 0x350, &0xff800000u32.to_le_bytes());
        target
    }
    let (file, mut memory) = fixture();
    let target = setup(&mut memory);
    let checked = verified(&file);
    let value = object_catalog(&checked, &memory)
        .character_placement(target)
        .unwrap()
        .unwrap();
    assert_eq!(
        value.position80_bits,
        [0x7fc01234, 0x80000000, 0x7f800000, 0x80]
    );
    assert_eq!(value.position90_bits[3], 0x90);
    assert_eq!(value.anglea0_bits[3], 0xa0);
    assert_eq!(value.positiond0_bits[3], 0xd0);
    assert_eq!(value.scalar350_bits, 0xff800000);
    for address in [
        target.address + 0x80,
        target.address + 0x90,
        target.address + 0xa0,
        target.address + 0xd0,
        target.address + 0x350,
        0xd00070,
        0xe00018,
    ] {
        let (_, mut memory) = fixture();
        let target = setup(&mut memory);
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .character_placement(target)
                .is_err(),
            "watch {address:#x}"
        );
    }
    memory.put64(target.address + 0x58, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_placement(target)
            .unwrap(),
        None
    );
}

#[test]
fn character_translation_observation_reads_only_proved_records_and_rechecks_linked_storage() {
    fn setup(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
        let target = spatial_fixture(memory);
        control_channels_fixture(memory);
        memory.put64(target.address + 0x18, 0x1900000);
        for address in [
            target.address + 0x80,
            target.address + 0x90,
            target.address + 0xa0,
            target.address + 0xd0,
            0x1900040,
            0xe00100,
            0xe00260,
        ] {
            for (lane, bits) in [0x7fc01234u32, 0x80000000, 0x7f800000, 0xff800000]
                .into_iter()
                .enumerate()
            {
                memory.put(address + lane as u64 * 4, &bits.to_le_bytes());
            }
        }
        memory.put(target.address + 0x350, &0x7fc12345u32.to_le_bytes());
        memory.put(target.address + 0x2e8, &0x80000000u32.to_le_bytes());
        target
    }
    let (file, mut memory) = fixture();
    let target = setup(&mut memory);
    let checked = verified(&file);
    let value = object_catalog(&checked, &memory)
        .character_translation(target)
        .unwrap()
        .unwrap();
    assert_eq!(value.kind, ObjectKind::Player);
    assert_eq!(value.position_state_address, 0x1900000);
    assert_eq!(
        value.position_state_bits,
        [0x7fc01234, 0x80000000, 0x7f800000, 0xff800000]
    );
    assert_eq!(value.controller_position100_bits, value.position_state_bits);
    assert_eq!(value.controller_position260_bits, value.position_state_bits);
    assert_eq!(value.scalar2e8_bits, 0x80000000);
    for address in [
        target.address + 0x18,
        target.address + 0x2e8,
        0x1900040,
        0xe00100,
        0xe00260,
    ] {
        let (_, mut memory) = fixture();
        let target = setup(&mut memory);
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .character_translation(target)
                .is_err(),
            "watch {address:#x}"
        );
    }
    memory.put64(target.address + 0x18, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_translation(target)
            .unwrap(),
        None
    );
    for address in [1, u64::MAX - 15] {
        memory.put64(target.address + 0x18, address);
        assert!(
            object_catalog(&checked, &memory)
                .character_translation(target)
                .is_err()
        );
    }
}

#[test]
fn world_creation_set_copies_exact_embedded_members_with_reciprocal_links() {
    fn setup(memory: &mut SparseMemory) -> u64 {
        character_set_fixture(memory);
        let set = 0x600000 + 0x10fe8;
        memory.put64(set, BASE + 0x2a41348);
        memory.put(set + 0x10, &3u32.to_le_bytes());
        memory.put64(set + 0x18, 0xd00000);
        memory.put64(0x610010, 0xd00000);
        memory.put64(0x620010, 0xd00020);
        set
    }
    let (file, mut memory) = fixture();
    let set = setup(&mut memory);
    let checked = verified(&file);
    let observed = object_catalog(&checked, &memory)
        .world_creation_set(3)
        .unwrap()
        .unwrap();
    assert_eq!(observed.world_address, 0x600000);
    assert_eq!(observed.set.address, set);
    assert_eq!(observed.set.characters.len(), 2);
    assert_eq!(observed.set.characters[1].ordinal, 2);
    for address in [set + 0x18, 0xd00000, 0x610010, 0x620010, BASE + 0x3d69ff8] {
        let (_, mut memory) = fixture();
        setup(&mut memory);
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .world_creation_set(3)
                .is_err(),
            "watch {address:#x}"
        );
    }
    memory.put64(0x620010, 0xd00000);
    assert!(
        object_catalog(&checked, &memory)
            .world_creation_set(3)
            .is_err()
    );
    memory.put64(set, BASE + 0x2a7fbb0);
    assert!(
        object_catalog(&checked, &memory)
            .world_creation_set(3)
            .is_err()
    );
    memory.put64(BASE + 0x3d69ff8, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .world_creation_set(3)
            .unwrap(),
        None
    );
}

fn creation_backread_fixture(
    memory: &mut SparseMemory,
) -> character_sets::CharacterRegistrationObservation {
    character_set_fixture(memory);
    let set = 0x600000 + 0x10fe8;
    memory.put64(set, BASE + 0x2a41348);
    memory.put(set + 0x10, &3u32.to_le_bytes());
    memory.put64(set + 0x18, 0xd00000);
    memory.put64(0x610010, 0xd00000);
    memory.put64(0x620010, 0xd00020);
    for offset in [0x1c4, 0x1c5, 0x1c8, 0x538, 0x6c, 0x539] {
        memory.put(0x620000 + offset, &[0xff]);
    }
    character_sets::CharacterRegistrationObservation {
        character: character_sets::CharacterIdentity {
            address: 0x620000,
            world_address: 0x600000,
            native_identifier: 0x1234567810100002,
            epoch: 7,
        },
        set_address: set,
        entry_address: 0xd00020,
        ordinal: 2,
    }
}

#[test]
fn creation_backread_retains_holes_and_native_false_without_granting_a_resource_guard() {
    let (file, mut memory) = fixture();
    let expected = creation_backread_fixture(&mut memory);
    let checked = verified(&file);
    let observation = object_catalog(&checked, &memory)
        .creation_backread(expected, 3)
        .unwrap()
        .unwrap();
    assert_eq!(observation.registration, expected);
    assert!(observation.native_ready());
    memory.put(expected.character.address + 0x1c8, &[0xef]);
    let observation = object_catalog(&checked, &memory)
        .creation_backread(expected, 3)
        .unwrap()
        .unwrap();
    assert!(!observation.native_ready());
    assert_eq!(
        observation.flags.bytes,
        [0xff, 0xff, 0xef, 0xff, 0xff, 0xff]
    );
    for index in 0..5 {
        let mut foreign = expected;
        match index {
            0 => foreign.set_address -= 0x108,
            1 => foreign.character.world_address += 8,
            2 => foreign.character.epoch = 0,
            3 => foreign.ordinal = 1,
            _ => foreign.entry_address -= 0x10,
        }
        assert!(
            object_catalog(&checked, &memory)
                .creation_backread(foreign, 3)
                .is_err()
        );
    }
    assert!(
        object_catalog(&checked, &memory)
            .creation_backread(expected, 2)
            .is_err()
    );
    memory.put64(0xd00020, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .creation_backread(expected, 3)
            .unwrap(),
        None
    );
    memory.put64(BASE + 0x3d69ff8, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .creation_backread(expected, 3)
            .unwrap(),
        None
    );
}

#[test]
fn creation_backread_rejects_changed_membership_identity_status_and_truncated_storage() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for watched in [
        0x6201c8,
        0x620008,
        0x620000,
        0x620010,
        0xd00020,
        0x610fe8 + 0x18,
        BASE + 0x3d69ff8,
    ] {
        let (_, mut memory) = fixture();
        let expected = creation_backread_fixture(&mut memory);
        memory.watch_address = watched;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .creation_backread(expected, 3)
                .is_err(),
            "watch {watched:#x}"
        );
        memory.change_slot = false;
        memory.bytes.remove(&watched);
        assert!(
            object_catalog(&checked, &memory)
                .creation_backread(expected, 3)
                .is_err()
        );
    }
}

fn character_resources_fixture(memory: &mut SparseMemory) -> character_sets::CharacterIdentity {
    let (_, target) = registration_fixture(memory);
    memory.put(0xd00028, &[4, 4, 0x85]);
    memory.put64(target.address + 0x28, 0x3400000);
    memory.put64(0x3400000, BASE + 0x2a400c8);
    memory.put(0x3400040, &3i32.to_le_bytes());
    memory.put(0x3400044, &3i32.to_le_bytes());
    memory.put(0x3400048, &[1]);
    memory.put(0x3400118, &[1, 1, 1, 0, 0, 1, 1]);
    for i in 0..4 {
        // Deliberately unmapped opaque references. The observer must never follow them.
        memory.put64(0x3400130 + i * 8, 0xfedc000000000000 + i);
    }
    memory.put(target.address + 0x1c8, &[0x90]);
    target
}

#[test]
fn character_resources_copy_independent_steps_without_following_acquired_references() {
    use character_sets::WorldCollectionSelection as Selection;
    let (bytes, mut memory) = fixture();
    let target = character_resources_fixture(&mut memory);
    let checked = verified(&bytes);
    for step in -1i32..=10 {
        memory.put(0x3400040, &step.to_le_bytes());
        let result = object_catalog(&checked, &memory)
            .character_resources(target, Selection::Players, 3)
            .unwrap()
            .unwrap();
        assert_eq!(result.current_step, step);
        assert_eq!(result.native_resource_step_ready(), (3..=5).contains(&step));
        assert_eq!(result.native_presentation_marks(), (3..=5).contains(&step));
        assert_eq!(result.acquired_references[3], 0xfedc000000000003);
    }
    memory.put(0x3400040, &3i32.to_le_bytes());
    for (address, value) in [(0xd00028, 2), (target.address + 0x1c8, 0x80)] {
        memory.put(address, &[value]);
        assert!(
            !object_catalog(&checked, &memory)
                .character_resources(target, Selection::Players, 3)
                .unwrap()
                .unwrap()
                .native_presentation_marks()
        );
        memory.put(address, &[if address == 0xd00028 { 4 } else { 0x90 }]);
    }
}

#[test]
fn character_resources_reject_type_identity_link_payload_changes_and_truncation() {
    use character_sets::WorldCollectionSelection as Selection;
    let (bytes, _) = fixture();
    let checked = verified(&bytes);
    for watched in [
        0x620028, 0x3400000, 0x3400040, 0x3400044, 0x3400048, 0x3400118, 0x3400130, 0x3400148,
        0x6201c8, 0xd00028, 0x620008,
    ] {
        let (_, mut memory) = fixture();
        let target = character_resources_fixture(&mut memory);
        memory.watch_address = watched;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .character_resources(target, Selection::Players, 3)
                .is_err(),
            "{watched:x}"
        );
        memory.change_slot = false;
        memory.bytes.remove(&watched);
        assert!(
            object_catalog(&checked, &memory)
                .character_resources(target, Selection::Players, 3)
                .is_err()
        );
    }
    let (_, mut memory) = fixture();
    let target = character_resources_fixture(&mut memory);
    memory.put64(0x3400000, BASE + 0x2a47090);
    assert!(
        object_catalog(&checked, &memory)
            .character_resources(target, Selection::Players, 3)
            .is_err()
    );
    memory.put64(target.address + 0x28, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .character_resources(target, Selection::Players, 3)
            .unwrap(),
        None
    );
}

#[test]
fn character_backread_request_preserves_entry_state_flags_and_keeps_failed_store_receipt() {
    use character_resources::{CharacterBackreadRequest as Request, commit_request, request_plan};
    use character_sets::WorldCollectionSelection as Selection;
    use std::cell::{Cell, RefCell};
    let (bytes, mut memory) = fixture();
    let target = character_resources_fixture(&mut memory);
    let checked = verified(&bytes);
    let objects = object_catalog(&checked, &memory);
    let source = objects
        .world_member_state(target, Selection::Players, 3)
        .unwrap()
        .unwrap();
    let primary = objects.primary_player_identity(1).unwrap();
    let copy = RefCell::new(source);
    let calls = Cell::new(0);
    for request in [Request::Unload, Request::Resources, Request::Present] {
        let before = *copy.borrow();
        let plan = request_plan(Some(before), before, request, primary).unwrap();
        let receipt = commit_request(
            plan,
            |address, value| {
                assert_eq!(address, 0xd00029);
                copy.borrow_mut().native_state_bytes[1] = value;
                calls.set(calls.get() + 1);
            },
            || Ok(*copy.borrow()),
        );
        assert!(receipt.postflight.is_ok());
        assert_eq!(copy.borrow().native_state_bytes[0], 4);
        assert_eq!(copy.borrow().native_state_bytes[2], 0x85);
    }
    let plan = request_plan(Some(source), source, Request::Unload, primary).unwrap();
    let receipt = commit_request(
        plan,
        |_, _| calls.set(calls.get() + 1),
        || Err(Error::InvalidProfile("synthetic postflight failure")),
    );
    assert!(receipt.changed && receipt.postflight.is_err());
    assert_eq!(receipt.source, source);
    assert_eq!(receipt.request, Request::Unload);
    assert_eq!(calls.get(), 4);
    for primary in [
        None,
        Some(target),
        primary.map(|mut p| {
            p.epoch += 1;
            p
        }),
    ] {
        assert!(request_plan(Some(source), source, Request::Present, primary).is_err());
    }
    let mut foreign = source;
    foreign.native_state_bytes[1] = 255;
    assert!(request_plan(Some(foreign), foreign, Request::Present, primary).is_err());
    assert!(request_plan(None, source, Request::Present, primary).is_err());
}

fn world_collection_fixture(memory: &mut SparseMemory) {
    registration_fixture(memory);
    for offset in [0x10f38, 0x10f90, 0x10fe8] {
        let address = 0x600000 + offset;
        memory.put64(address, BASE + 0x2a41348);
        memory.put(address + 0x10, &0u32.to_le_bytes());
        memory.put64(address + 0x18, 0);
    }
    let open = 0x600000 + 0x11040;
    memory.put64(open, BASE + 0x2a4b620);
    memory.put(open + 0x10, &3u32.to_le_bytes());
    memory.put64(open + 0x18, 0xe00000);
    for pointer in [0xe00000, 0xe00010, 0xe00020] {
        memory.put64(pointer, 0);
    }
    memory.put64(0xe00020, 0x630000);
    memory.put64(0x630000, BASE + 0x2a47090);
    memory.put64(0x630008, 0xffeeabcd12345678);
    memory.put64(0x630010, 0xe00020);
    memory.put64(0x600000 + 0x1ded8, 0x600000 + 0x10ee0);
    memory.put64(0x600000 + 0x1ded8 + 195 * 8, 0);
    memory.put64(0x600000 + 0x1ded8 + 196 * 8, open);
}

#[test]
fn world_collections_copy_all_embedded_types_and_last_native_slot_without_reading_padding() {
    use character_sets::WorldCollectionSelection as Selection;
    let (bytes, mut memory) = fixture();
    world_collection_fixture(&mut memory);
    let file = verified(&bytes);
    let objects = object_catalog(&file, &memory);
    for selection in [
        Selection::Players,
        Selection::Embedded10f38,
        Selection::Embedded10f90,
        Selection::Creation,
        Selection::OpenField,
        Selection::Indexed(0),
        Selection::Indexed(196),
    ] {
        let result = objects.world_collection(selection, 71, 3).unwrap().unwrap();
        assert_eq!(
            (result.world_address, result.epoch, result.selection),
            (0x600000, 71, selection)
        );
        match selection {
            Selection::OpenField | Selection::Indexed(196) => {
                assert_eq!(result.kind, ObjectKind::OpenFieldCharacterSet);
                assert_eq!(result.set.characters.len(), 1);
                assert_eq!(result.set.characters[0].ordinal, 2);
                assert_eq!(
                    result.set.characters[0].native_identifier,
                    0xffeeabcd12345678
                );
                assert_eq!(result.set.characters[0].character.kind, ObjectKind::Enemy);
            }
            Selection::Players | Selection::Indexed(0) => {
                assert_eq!(result.set.characters.len(), 2)
            }
            _ => assert_eq!(result.set.capacity, 0),
        }
    }
    assert_eq!(
        objects
            .world_collection(Selection::Indexed(195), 71, 3)
            .unwrap(),
        None
    );
    let open = 0x600000 + 0x11040;
    assert!(objects.character_set(open, 3).is_err());
    assert!(
        objects
            .open_field_characters(0x600000 + 0x10ee0, 3)
            .is_err()
    );
    assert_eq!(
        objects
            .open_field_characters(open, 3)
            .unwrap()
            .unwrap()
            .characters
            .len(),
        1
    );
    // Sparse fixture intentionally omits unknown header fields, entry state bytes,
    // scene graphs, unused indexed slots, and all character payload except type/id/link.
}

#[test]
fn world_collections_reject_foreign_types_reciprocal_changes_and_truncated_selected_storage() {
    use character_sets::WorldCollectionSelection as Selection;
    let (bytes, _) = fixture();
    let file = verified(&bytes);
    let open = 0x600000 + 0x11040;
    for watched in [
        BASE + 0x3d69ff8,
        open,
        open + 0x10,
        open + 0x18,
        0xe00020,
        0x630000,
        0x630008,
        0x630010,
        0x600000 + 0x1ded8 + 196 * 8,
    ] {
        let (_, mut memory) = fixture();
        world_collection_fixture(&mut memory);
        memory.watch_address = watched;
        memory.change_slot = true;
        assert!(
            object_catalog(&file, &memory)
                .world_collection(Selection::Indexed(196), 71, 3)
                .is_err(),
            "{watched:x}"
        );
    }
    for (address, value) in [
        (open, BASE + 0x2a41348),
        (open, BASE + 0x2a7fbb0),
        (0x630010, 0xe00000),
        (open + 0x18, 1),
        (0x600000 + 0x1ded8 + 196 * 8, 1),
    ] {
        let (_, mut memory) = fixture();
        world_collection_fixture(&mut memory);
        memory.put64(address, value);
        let selection = if address == open {
            Selection::OpenField
        } else {
            Selection::Indexed(196)
        };
        assert!(
            object_catalog(&file, &memory)
                .world_collection(selection, 71, 3)
                .is_err()
        );
    }
    for address in [open + 0x10, open + 0x18, 0xe00020, 0x630008, 0x630010] {
        let (_, mut memory) = fixture();
        world_collection_fixture(&mut memory);
        memory.bytes.remove(&address);
        assert!(
            object_catalog(&file, &memory)
                .world_collection(Selection::OpenField, 71, 3)
                .is_err()
        );
    }
    let (_, mut memory) = fixture();
    world_collection_fixture(&mut memory);
    memory.put64(0x600000 + 0x1ded8 + 196 * 8, 0x630000);
    assert!(
        object_catalog(&file, &memory)
            .world_collection(Selection::Indexed(196), 71, 3)
            .is_err()
    );
}

#[test]
fn world_collection_selection_bounds_fail_even_with_missing_world_and_missing_slots_stay_none() {
    use character_sets::WorldCollectionSelection as Selection;
    let (bytes, mut memory) = fixture();
    memory.put64(BASE + 0x3d69ff8, 0);
    let file = verified(&bytes);
    let objects = object_catalog(&file, &memory);
    for (selection, epoch, limit) in [
        (Selection::Indexed(197), 1, 3),
        (Selection::Indexed(u16::MAX), 1, 3),
        (Selection::Players, 0, 3),
        (Selection::Players, 1, 0),
        (Selection::Players, 1, 65537),
    ] {
        assert!(objects.world_collection(selection, epoch, limit).is_err());
    }
    assert_eq!(
        objects
            .world_collection(Selection::OpenField, 1, 3)
            .unwrap(),
        None
    );
    let (_, mut memory) = fixture();
    world_collection_fixture(&mut memory);
    assert!(
        object_catalog(&file, &memory)
            .world_collection(Selection::OpenField, 1, 2)
            .is_err()
    );
    memory.put64(0x600000 + 0x1ded8 + 196 * 8, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .world_collection(Selection::Indexed(196), 1, 3)
            .unwrap(),
        None
    );
}

#[test]
fn registered_world_member_state_preserves_unknown_values_without_inventing_lifecycle_status() {
    use character_sets::WorldCollectionSelection as Selection;
    let (bytes, mut memory) = fixture();
    world_collection_fixture(&mut memory);
    let target = character_sets::CharacterIdentity {
        address: 0x630000,
        world_address: 0x600000,
        native_identifier: 0xffeeabcd12345678,
        epoch: 83,
    };
    let file = verified(&bytes);
    for state in [0, 1, 2, 3, 4, 5, 6, 127, 255] {
        memory.put(0xe00028, &[state, 0xfd, 0xe7]);
        let result = object_catalog(&file, &memory)
            .world_member_state(target, Selection::Indexed(196), 3)
            .unwrap()
            .unwrap();
        assert_eq!(result.native_state_bytes, [state, 0xfd, 0xe7]);
        assert_eq!(result.entry_state_is_four(), state == 4);
        assert_eq!(result.set_kind, ObjectKind::OpenFieldCharacterSet);
        assert_eq!(result.registration.character, target);
        assert_eq!(
            (
                result.registration.entry_address,
                result.registration.ordinal
            ),
            (0xe00020, 2)
        );
    }
    assert_eq!(
        object_catalog(&file, &memory)
            .world_member_state(target, Selection::Creation, 3)
            .unwrap(),
        None
    );
    // State is independent of character BackReadReady/HP/controller/render flags:
    // none of those fields is mapped in this original sparse fixture.
    for selection in [Selection::Players, Selection::Indexed(0)] {
        let primary = character_sets::CharacterIdentity {
            address: 0x610000,
            world_address: 0x600000,
            native_identifier: 0xffffffff10000001,
            epoch: 83,
        };
        memory.put(0xd00008, &[4, 0, 0xff]);
        assert!(
            object_catalog(&file, &memory)
                .world_member_state(primary, selection, 3)
                .unwrap()
                .unwrap()
                .entry_state_is_four()
        );
    }
}

#[test]
fn registered_world_member_state_rejects_stale_membership_raw_byte_changes_and_missing_state() {
    use character_sets::WorldCollectionSelection as Selection;
    let (bytes, _) = fixture();
    let file = verified(&bytes);
    let target = character_sets::CharacterIdentity {
        address: 0x630000,
        world_address: 0x600000,
        native_identifier: 0xffeeabcd12345678,
        epoch: 83,
    };
    for watched in [
        0xe00028,
        0xe00020,
        0x630008,
        0x630010,
        0x600000 + 0x1ded8 + 196 * 8,
    ] {
        let (_, mut memory) = fixture();
        world_collection_fixture(&mut memory);
        memory.put(0xe00028, &[4, 9, 0xff]);
        memory.watch_address = watched;
        memory.change_slot = true;
        assert!(
            object_catalog(&file, &memory)
                .world_member_state(target, Selection::Indexed(196), 3)
                .is_err(),
            "{watched:x}"
        );
    }
    for offset in 0..3 {
        let (_, mut memory) = fixture();
        world_collection_fixture(&mut memory);
        memory.put(0xe00028, &[4, 9, 0xff]);
        memory.bytes.remove(&(0xe00028 + offset));
        assert!(
            object_catalog(&file, &memory)
                .world_member_state(target, Selection::OpenField, 3)
                .is_err()
        );
    }
    let (_, mut memory) = fixture();
    world_collection_fixture(&mut memory);
    memory.put(0xe00028, &[4, 9, 0xff]);
    for foreign in [
        character_sets::CharacterIdentity {
            native_identifier: target.native_identifier ^ (1 << 63),
            ..target
        },
        character_sets::CharacterIdentity {
            world_address: target.world_address + 8,
            ..target
        },
        character_sets::CharacterIdentity { epoch: 0, ..target },
    ] {
        assert!(
            object_catalog(&file, &memory)
                .world_member_state(foreign, Selection::OpenField, 3)
                .is_err()
        );
    }
    memory.put64(0xe00020, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .world_member_state(target, Selection::OpenField, 3)
            .unwrap(),
        None
    );
    memory.put64(BASE + 0x3d69ff8, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .world_member_state(target, Selection::OpenField, 3)
            .unwrap(),
        None
    );
}

#[test]
fn ragdoll_observation_copies_exact_link_for_each_owner_and_distinguishes_absence() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for owner in [ObjectKind::Character, ObjectKind::Enemy, ObjectKind::Player] {
        let (_, mut memory) = fixture();
        let target = controller_fixture(&mut memory, owner);
        memory.put64(0xe00028, 0xf00000);
        memory.put64(0xf00000, BASE + 0x2b97168);
        memory.put64(0xf00010, target.address);
        let observed = object_catalog(&checked, &memory)
            .character_ragdoll(target)
            .unwrap()
            .unwrap();
        assert_eq!(observed.controller.character, target);
        assert_eq!(observed.ragdoll.address, 0xf00000);
        assert_eq!(observed.ragdoll.kind, ObjectKind::CharacterRagdoll);
        // No other ragdoll/Havok bytes are mapped: the reader must not follow them.
        for absent_link in [BASE + 0x3d69ff8, target.address + 0x58, 0xe00028] {
            let original = memory.read_u64(absent_link).unwrap();
            memory.put64(absent_link, 0);
            assert_eq!(
                object_catalog(&checked, &memory)
                    .character_ragdoll(target)
                    .unwrap(),
                None
            );
            memory.put64(absent_link, original);
        }
    }
}

#[test]
fn ragdoll_observation_rejects_wrong_owner_type_epoch_truncation_and_graph_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    let make = || {
        let (_, mut memory) = fixture();
        let target = controller_fixture(&mut memory, ObjectKind::Player);
        memory.put64(0xe00028, 0xf00000);
        memory.put64(0xf00000, BASE + 0x2b97168);
        memory.put64(0xf00010, target.address);
        (memory, target)
    };
    for (address, value) in [
        (0xe00028, 0xf00004),
        (0xf00000, BASE + 0x2a3c890),
        (0xf00010, 0x620000),
        (0x610008, 1),
    ] {
        let (mut memory, target) = make();
        memory.put64(address, value);
        assert!(
            object_catalog(&checked, &memory)
                .character_ragdoll(target)
                .is_err()
        );
    }
    let (memory, mut target) = make();
    target.epoch = 0;
    assert!(
        object_catalog(&checked, &memory)
            .character_ragdoll(target)
            .is_err()
    );
    for address in [0xe00028u64, 0xf00000, 0xf00010] {
        for byte in 0..8 {
            let (mut memory, target) = make();
            memory.bytes.remove(&(address + byte));
            assert!(
                object_catalog(&checked, &memory)
                    .character_ragdoll(target)
                    .is_err()
            );
        }
    }
    for address in [
        0x610008, 0x610058, 0xe00000, 0xe00010, 0xe002d4, 0xe00028, 0xf00000, 0xf00010,
    ] {
        let (mut memory, target) = make();
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .character_ragdoll(target)
                .is_err()
        );
    }
}

#[test]
fn controller_storage_preserves_all_words_without_treating_sentinels_as_pointers() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for kind in [ObjectKind::Character, ObjectKind::Enemy, ObjectKind::Player] {
        let (_, mut memory) = fixture();
        let target = controller_fixture(&mut memory, kind);
        memory.put64(0xe00020, 0xf10000);
        let words = [0, u64::MAX, 0x8000000000000000, 0x123456789abcdef0];
        for (i, word) in words.into_iter().enumerate() {
            memory.put64(0xf10000 + i as u64 * 8, word);
        }
        let observed = object_catalog(&checked, &memory)
            .controller_storage(target)
            .unwrap()
            .unwrap();
        assert_eq!(observed.address, 0xf10000);
        assert_eq!(observed.controller.character, target);
        assert_eq!(observed.words, words);
        for link in [BASE + 0x3d69ff8, target.address + 0x58, 0xe00020] {
            let original = memory.read_u64(link).unwrap();
            memory.put64(link, 0);
            assert_eq!(
                object_catalog(&checked, &memory)
                    .controller_storage(target)
                    .unwrap(),
                None
            );
            memory.put64(link, original);
        }
    }
}

#[test]
fn controller_storage_rejects_partial_records_stale_identity_alignment_and_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    let make = || {
        let (_, mut memory) = fixture();
        let target = controller_fixture(&mut memory, ObjectKind::Player);
        memory.put64(0xe00020, 0xf10000);
        memory.put(0xf10000, &[0xff; 32]);
        (memory, target)
    };
    for byte in 0..32 {
        let (mut memory, target) = make();
        memory.bytes.remove(&(0xf10000 + byte));
        assert!(
            object_catalog(&checked, &memory)
                .controller_storage(target)
                .is_err()
        );
    }
    for (slot, value) in [
        (0xe00020, 0xf10004),
        (0xe00020, u64::MAX - 7),
        (0xe00010, 0x620000),
        (0x610008, 1),
    ] {
        let (mut memory, target) = make();
        memory.put64(slot, value);
        assert!(
            object_catalog(&checked, &memory)
                .controller_storage(target)
                .is_err()
        );
    }
    for slot in [0xe00020, 0xf10000, 0x610058, 0xe00010] {
        let (mut memory, target) = make();
        memory.watch_address = slot;
        memory.change_slot = true;
        assert!(
            object_catalog(&checked, &memory)
                .controller_storage(target)
                .is_err()
        );
    }
    let (memory, mut target) = make();
    target.epoch = 0;
    assert!(
        object_catalog(&checked, &memory)
            .controller_storage(target)
            .is_err()
    );
}

#[test]
fn primary_unit_reader_checks_epoch_and_postflights_primary_slot_after_full_copy() {
    let (bytes, mut memory) = fixture();
    let (target, plan) = unit_fixture(&mut memory);
    let checked = verified(&bytes);
    assert_eq!(
        object_catalog(&checked, &memory)
            .primary_unit_snapshot(target.epoch, &plan)
            .unwrap(),
        object_catalog(&checked, &memory)
            .character_unit_snapshot(target, &plan)
            .unwrap()
    );
    // Slot is inspected before the copy, then changed after all components have read.
    struct Switched<'a> {
        memory: &'a SparseMemory,
        ready: Cell<bool>,
    }
    impl AddressSpace for Switched<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if address == 0x610060 {
                self.ready.set(true);
            }
            if address == 0x600000 + 0x1e508 && self.ready.get() {
                output.fill(0);
            }
            Ok(())
        }
    }
    let switched = Switched {
        memory: &memory,
        ready: Cell::new(false),
    };
    let objects = GameObjectCatalog {
        module: checked
            .bind(
                &switched,
                BASE,
                &[CodeRange {
                    rva: 0x1000,
                    length: 1,
                }],
            )
            .unwrap(),
    };
    assert!(objects.primary_unit_snapshot(target.epoch, &plan).is_err());
    memory.put64(BASE + 0x3d69ff8, 0);
    assert_eq!(
        object_catalog(&checked, &memory)
            .primary_unit_snapshot(target.epoch, &plan)
            .unwrap(),
        None
    );
    assert!(
        object_catalog(&checked, &memory)
            .primary_unit_snapshot(0, &plan)
            .is_err()
    );
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn current_unit_bind_rejects_invalid_plan_before_inspecting_file_or_current_process() {
    use character_units::CharacterUnitReadPlan as Plan;
    use current_character_units::CurrentCharacterUnits;
    let plan = Plan {
        effects_limit: Some(0),
        ..Plan::default()
    };
    assert!(matches!(
        // SAFETY: Invalid configuration returns before touching process state or invoking game code.
        unsafe { CurrentCharacterUnits::bind(std::sync::Arc::from([]), 0, None, plan) },
        Err(Error::InvalidProfile("invalid character unit read plan"))
    ));
}

#[test]
fn torrent_flag_state_preserves_ordered_demands_missing_groups_and_native_mirror() {
    let (bytes, mut memory) = fixture();
    flags_fixture(&mut memory, 2);
    torrent_state_fixture(&mut memory, 3);
    memory.put(0xf40000 + 87, &[8]);
    memory.put(0xf40000 + 88, &[2]);
    let file = verified(&bytes);
    let ids = [6700, 6710, 6700, 0, u32::MAX];
    let objects = object_catalog(&file, &memory);
    let copy = objects.torrent_flag_state(7, &ids, 1).unwrap().unwrap();
    assert_eq!(copy.saved.selector, 3);
    assert_eq!(copy.flags.iter().map(|f| f.id).collect::<Vec<_>>(), ids);
    assert_eq!(
        copy.flags.iter().map(|f| f.value).collect::<Vec<_>>(),
        [true, true, true, false, false]
    );
    assert_eq!(objects.event_flags(&ids, 1).unwrap().unwrap(), copy.flags);
    // Event selection remains readable before the saved selector allocation is ready.
    memory.put64(BASE + 0x3d61f98, 0);
    let before_saved = object_catalog(&file, &memory);
    assert_eq!(before_saved.torrent_flag_state(7, &ids, 1).unwrap(), None);
    assert_eq!(
        before_saved.event_flags(&ids, 1).unwrap().unwrap(),
        copy.flags
    );
    memory.put64(BASE + 0x3d6c4b8, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .torrent_flag_state(7, &ids, 1)
            .unwrap(),
        None
    );
    assert!(
        object_catalog(&file, &memory)
            .torrent_flag_state(0, &ids, 1)
            .is_err()
    );
    assert!(object_catalog(&file, &memory).event_flags(&[], 1).is_err());
    assert!(
        object_catalog(&file, &memory)
            .torrent_flag_state(7, &[1; 257], 1)
            .is_err()
    );
    assert!(
        object_catalog(&file, &memory)
            .torrent_flag_state(7, &ids, 0)
            .is_err()
    );
}
#[test]
fn torrent_composite_flags_reject_earlier_flag_or_saved_selector_changes_during_later_flag_read() {
    let (bytes, mut memory) = fixture();
    flags_fixture(&mut memory, 2);
    torrent_state_fixture(&mut memory, 3);
    memory.put(0xf40000 + 87, &[8]);
    memory.put(0xf40000 + 88, &[2]);
    let file = verified(&bytes);
    struct Later<'a> {
        memory: &'a SparseMemory,
        armed: Cell<bool>,
        saved: bool,
    }
    impl AddressSpace for Later<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if address == 0xf40000 + 88 {
                self.armed.set(true);
            }
            if self.armed.get() {
                if self.saved && address == 0x1200960 {
                    output[0] = 2;
                }
                if !self.saved && address == 0xf40000 + 87 {
                    output[0] ^= 8;
                }
            }
            Ok(())
        }
    }
    for saved in [false, true] {
        let later = Later {
            memory: &memory,
            armed: Cell::new(false),
            saved,
        };
        let objects = GameObjectCatalog {
            module: file
                .bind(
                    &later,
                    BASE,
                    &[CodeRange {
                        rva: 0x1000,
                        length: 1,
                    }],
                )
                .unwrap(),
        };
        assert!(objects.torrent_flag_state(7, &[6700, 6710], 1).is_err());
        assert!(later.armed.get());
    }
    let later = Later {
        memory: &memory,
        armed: Cell::new(false),
        saved: false,
    };
    let objects = GameObjectCatalog {
        module: file
            .bind(
                &later,
                BASE,
                &[CodeRange {
                    rva: 0x1000,
                    length: 1,
                }],
            )
            .unwrap(),
    };
    assert!(objects.event_flags(&[6700, 6710], 1).is_err());
}

fn phase_state_fixture(memory: &mut SparseMemory) {
    memory.put64(BASE + 0x458fe88, 0x2500000);
    memory.put64(0x2500000, BASE + 0x2c04b00);
    memory.put64(0x2500008, 0x2510000);
    memory.put64(0x2510000, BASE + 0x329caf0);
    memory.put64(0x2510018, 0x2520000);
    memory.put64(0x2510020, 0x2520000 + 3 * 0x88);
    memory.put(0x2520000, &168u32.to_le_bytes());
    memory.put(0x2520088, &4u32.to_le_bytes());
    memory.put(0x252010c, &[255]);
    // First matching ID is 4, not registration index168. All names/padding, the
    // first record's flag, capacity and records after that match are unreadable.
}
#[test]
fn phase_state_uses_assigned_id_first_matching_prefix_and_keeps_missing_separate() {
    let (bytes, mut memory) = fixture();
    phase_state_fixture(&mut memory);
    let file = verified(&bytes);
    let copy = catalog(&file, &memory)
        .phase_group_state(TaskPhase::FRAME_END, 3)
        .unwrap()
        .unwrap();
    assert_eq!(copy.native_group_id, 4);
    assert_eq!(copy.group.registration_index, 168);
    assert_eq!(copy.record_address, 0x2520088);
    assert_eq!(copy.native_enabled, 255);
    assert!(copy.enabled());
    memory.put(0x252010c, &[0]);
    assert!(
        !catalog(&file, &memory)
            .phase_group_state(TaskPhase::FRAME_END, 3)
            .unwrap()
            .unwrap()
            .enabled()
    );
    memory.put(0x2520088, &9u32.to_le_bytes());
    memory.put(0x2520110, &0u32.to_le_bytes());
    assert_eq!(
        catalog(&file, &memory)
            .phase_group_state(TaskPhase::FRAME_END, 3)
            .unwrap(),
        None
    );
    memory.put(GROUP + 0x54, &0u32.to_le_bytes());
    memory.bytes.remove(&(BASE + 0x458fe88));
    assert_eq!(
        catalog(&file, &memory)
            .phase_group_state(TaskPhase::FRAME_END, 3)
            .unwrap(),
        None
    );
    for limit in [0, 65_537] {
        assert!(
            catalog(&file, &memory)
                .phase_group_state(TaskPhase::FRAME_END, limit)
                .is_err()
        );
    }
}
#[test]
fn phase_state_refuses_bad_graph_bounds_missing_bytes_and_visible_changes() {
    let (bytes, _) = fixture();
    let file = verified(&bytes);
    for (address, value) in [
        (0x2500000, BASE + 0x329caf0),
        (0x2510000, BASE + 0x2c04b00),
        (0x2500008, 0x2510001),
        (0x2510018, 0x2520001),
        (0x2510020, 0x2520000 - 8),
        (0x2510020, 0x2520000 + 1),
        (0x2510020, 0x2520000 + 4 * 0x88),
        (0x2510018, 0),
    ] {
        let (_, mut memory) = fixture();
        phase_state_fixture(&mut memory);
        memory.put64(address, value);
        assert!(
            catalog(&file, &memory)
                .phase_group_state(TaskPhase::FRAME_END, 3)
                .is_err()
        );
    }
    let (_, mut memory) = fixture();
    phase_state_fixture(&mut memory);
    for address in (0x2520000..0x2520004)
        .chain(0x2520088..0x252008c)
        .chain([0x252010c])
    {
        let value = memory.bytes.remove(&address).unwrap();
        assert!(
            catalog(&file, &memory)
                .phase_group_state(TaskPhase::FRAME_END, 3)
                .is_err()
        );
        memory.put(address, &[value]);
    }
    for watch in [
        BASE + 0x458fe88,
        0x2500000,
        0x2500008,
        0x2510000,
        0x2510018,
        0x2510020,
        0x2520000,
        0x2520088,
        0x252010c,
        GROUP + 0x54,
    ] {
        memory.watch_address = watch;
        memory.slot_reads.set(0);
        memory.change_slot = true;
        assert!(
            catalog(&file, &memory)
                .phase_group_state(TaskPhase::FRAME_END, 3)
                .is_err(),
            "{watch:#x}"
        );
    }
    memory.change_slot = false;
    for address in [BASE + 0x458fe88, 0x2500008, MANAGER + 8 + 168 * 8] {
        let saved = memory.bytes.clone();
        memory.put64(address, 0);
        assert_eq!(
            catalog(&file, &memory)
                .phase_group_state(TaskPhase::FRAME_END, 3)
                .unwrap(),
            None
        );
        memory.bytes = saved;
    }
    memory.put64(0x2510018, 0);
    memory.put64(0x2510020, 0);
    assert_eq!(
        catalog(&file, &memory)
            .phase_group_state(TaskPhase::FRAME_END, 3)
            .unwrap(),
        None
    );
}
#[test]
fn title_inputs_copy_raw_components_and_reject_later_component_changes() {
    use title_runtime::TitleRuntimeCatalog;
    let (bytes, mut memory) = fixture();
    phase_state_fixture(&mut memory);
    menu_state_fixture(&mut memory);
    character_set_fixture(&mut memory);
    memory.put64(BASE + 0x3d6f8f0, 0x2300000);
    memory.put64(0x2300000, BASE + 0x2aa0a08);
    memory.put(0x230653c, &(-1i32).to_le_bytes());
    let file = verified(&bytes);
    let bind = || TitleRuntimeCatalog {
        objects: object_catalog(&file, &memory),
        tasks: catalog(&file, &memory),
        phases: [TaskPhase::FRAME_END; 4],
    };
    let copy = bind().snapshot(7, 3).unwrap();
    assert_eq!(copy.frontend.unwrap().native_state, -1);
    assert_eq!(copy.menu.unwrap().native_selector, 7);
    assert_eq!(copy.loading_pulses.unwrap().previous, 255);
    assert_eq!(copy.primary_player.unwrap().epoch, 7);
    assert!(copy.groups.iter().all(|x| x.unwrap().native_enabled == 255));
    assert!(bind().snapshot(0, 3).is_err());
    assert!(bind().snapshot(7, 0).is_err());
    struct Late<'a> {
        memory: &'a SparseMemory,
        trigger: Cell<bool>,
        changed: u64,
    }
    impl AddressSpace for Late<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if address == 0x252010c {
                self.trigger.set(true);
            }
            if self.trigger.get() && address == self.changed {
                output[0] ^= 1;
            }
            Ok(())
        }
    }
    for changed in [0x230653c, 0x1500734, 0x15010ec, 0x610008] {
        let late = Late {
            memory: &memory,
            trigger: Cell::new(false),
            changed,
        };
        let objects = GameObjectCatalog {
            module: file
                .bind(
                    &late,
                    BASE,
                    &[CodeRange {
                        rva: 0x1000,
                        length: 1,
                    }],
                )
                .unwrap(),
        };
        let tasks = TaskCatalog {
            module: file
                .bind(
                    &late,
                    BASE,
                    &[CodeRange {
                        rva: 0x1000,
                        length: 1,
                    }],
                )
                .unwrap(),
        };
        assert!(
            TitleRuntimeCatalog {
                objects,
                tasks,
                phases: [TaskPhase::FRAME_END; 4]
            }
            .snapshot(7, 3)
            .is_err(),
            "{changed:#x}"
        );
    }
    for global in [0x3d6f8f0, 0x3d6f820, 0x3d64f28, 0x3d69ff8, 0x458fe88] {
        memory.put64(BASE + global, 0);
    }
    let copy = TitleRuntimeCatalog {
        objects: object_catalog(&file, &memory),
        tasks: catalog(&file, &memory),
        phases: [TaskPhase::FRAME_END; 4],
    }
    .snapshot(7, 3)
    .unwrap();
    assert!(
        copy.frontend.is_none()
            && copy.menu.is_none()
            && copy.loading_pulses.is_none()
            && copy.primary_player.is_none()
            && copy.groups.iter().all(Option::is_none)
    );
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        // SAFETY: Invalid limit must reject before file verification or any process reads.
        let result = unsafe {
            current_title_runtime::CurrentTitleRuntime::bind(std::sync::Arc::from([]), 0, 0, None)
        };
        assert!(result.is_err());
    }
}

#[test]
fn primary_title_policy_read_does_not_require_valid_diagnostic_sources() {
    use title_runtime::TitleRuntimeCatalog;
    let (bytes, mut memory) = fixture();
    character_set_fixture(&mut memory);
    menu_state_fixture(&mut memory);
    let file = verified(&bytes);
    // Unknown diagnostic type must reject combined observations while primary remains live.
    memory.put64(0x1500000, 0);
    let catalog = TitleRuntimeCatalog {
        objects: object_catalog(&file, &memory),
        tasks: catalog(&file, &memory),
        phases: [TaskPhase::FRAME_END; 4],
    };
    assert!(catalog.snapshot(7, 3).is_err());
    assert_eq!(
        catalog.primary_player_identity(7).unwrap().unwrap().epoch,
        7
    );
    assert!(catalog.primary_player_identity(0).is_err());
}

#[test]
fn network_session_storage_copies_exact_holder_and_opaque_identity_then_rejects_replacement() {
    let (bytes, mut memory) = fixture();
    let (target, _) = unit_fixture(&mut memory);
    let holder = target.address + 0x598;
    memory.put64(holder, BASE + 0x2ba1d90);
    memory.put64(holder + 0x10, 0x2800000);
    memory.put64(0x2800000, BASE + 0x2ba1c20);
    memory.put64(0x2800008, u64::MAX);
    let file = verified(&bytes);
    let copied = object_catalog(&file, &memory)
        .player_network_session(target)
        .unwrap()
        .unwrap();
    assert_eq!(
        (copied.session_address, copied.native_identifier),
        (0x2800000, u64::MAX)
    );
    memory.put64(holder + 0x10, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .player_network_session(target)
            .unwrap(),
        None
    );
    memory.put64(holder + 0x10, 0x2800000);
    memory.put64(0x2800000, BASE + 0x2ba1a80); // Debug session cannot occupy the network API.
    assert!(
        object_catalog(&file, &memory)
            .player_network_session(target)
            .is_err()
    );
    memory.put64(0x2800000, BASE + 0x2ba1c20);
    memory.watch_address = holder + 0x10;
    memory.change_slot = true;
    memory.slot_reads.set(0);
    assert!(
        object_catalog(&file, &memory)
            .player_network_session(target)
            .is_err()
    );
}

#[test]
fn character_state_words_copy_signed_transition_and_raw_policy_and_reject_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for kind in [ObjectKind::Character, ObjectKind::Enemy, ObjectKind::Player] {
        let (_, mut memory) = fixture();
        let target = opacity_fixture(&mut memory, kind, [0; 4]);
        memory.put(target.address + 0x1c0, &(-1i32).to_le_bytes());
        memory.put(target.address + 0x1cc, &0x80000019u32.to_le_bytes());
        let copied = object_catalog(&checked, &memory)
            .character_state_words(target)
            .unwrap()
            .unwrap();
        assert_eq!(copied.transition_step, -1);
        assert_eq!(copied.policy_bits, 0x80000019);
        for offset in [0x1c0, 0x1cc] {
            memory.watch_address = target.address + offset;
            memory.change_slot = true;
            memory.slot_reads.set(0);
            assert!(
                object_catalog(&checked, &memory)
                    .character_state_words(target)
                    .is_err()
            );
            memory.change_slot = false;
        }
        memory.bytes.remove(&(target.address + 0x1cc));
        assert!(
            object_catalog(&checked, &memory)
                .character_state_words(target)
                .is_err()
        );
    }
}

fn update_list_fixture(memory: &mut SparseMemory) {
    character_set_fixture(memory);
    memory.put64(0x600000 + 0x1f1d8, 0xe00000);
    memory.put64(0x600000 + 0x1f1e0, 0xe00020);
    memory.put64(0x600000 + 0x1f1e8, 0xe00040);
    for (index, (address, priority, class)) in [
        (0x620000u64, 0x7fc12345u32, -7i32),
        (0x610000, 0x80000000, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let at = 0xe00000 + index as u64 * 16;
        memory.put64(at, address);
        memory.put(at + 8, &priority.to_le_bytes());
        memory.put(at + 12, &class.to_le_bytes());
    }
}
#[test]
fn world_update_list_preserves_native_order_full_identity_and_raw_priority_classes() {
    let (bytes, mut memory) = fixture();
    update_list_fixture(&mut memory);
    let file = verified(&bytes);
    let value = object_catalog(&file, &memory)
        .world_update_list(7, 2)
        .unwrap()
        .unwrap();
    assert_eq!(value.capacity, 4);
    assert_eq!(
        value
            .entries
            .iter()
            .map(|v| v.character.address)
            .collect::<Vec<_>>(),
        [0x620000, 0x610000]
    );
    assert_eq!(
        value.entries[0].character.native_identifier,
        0x1234567810100002
    );
    assert_eq!(value.entries[0].priority_bits, 0x7fc12345);
    assert_eq!(value.entries[0].update_class, -7);
    assert_eq!(value.entries[1].ordinal, 1);
    assert_eq!(value.entries[1].priority_bits, 0x80000000);
    assert_eq!(value.entries[1].character.epoch, 7);
    for offset in [0x1f1d8, 0x1f1e0, 0x1f1e8] {
        memory.put64(0x600000 + offset, 0);
    }
    assert!(
        object_catalog(&file, &memory)
            .world_update_list(7, 2)
            .unwrap()
            .unwrap()
            .entries
            .is_empty()
    );
}
#[test]
fn world_update_list_rejects_excessive_misaligned_reversed_and_truncated_storage() {
    let (bytes, mut memory) = fixture();
    update_list_fixture(&mut memory);
    let file = verified(&bytes);
    for (epoch, limit) in [(0, 2), (7, 0), (7, 1), (7, 65537)] {
        assert!(
            object_catalog(&file, &memory)
                .world_update_list(epoch, limit)
                .is_err()
        );
    }
    for (offset, value) in [
        (0x1f1d8, 0),
        (0x1f1d8, 0xe00001),
        (0x1f1e0, 0xdffff0),
        (0x1f1e0, 0xe00011),
        (0x1f1e0, 0xe00050),
        (0x1f1e8, 0xe00021),
        (0x1f1e8, 0xf00010),
    ] {
        update_list_fixture(&mut memory);
        memory.put64(0x600000 + offset, value);
        assert!(
            object_catalog(&file, &memory)
                .world_update_list(7, 2)
                .is_err(),
            "{offset:x} {value:x}"
        );
    }
    update_list_fixture(&mut memory);
    memory.bytes.remove(&0xe0001f);
    assert!(
        object_catalog(&file, &memory)
            .world_update_list(7, 2)
            .is_err()
    );
}
#[test]
fn world_update_list_rejects_detectable_header_entry_type_world_and_identity_changes() {
    let (bytes, mut memory) = fixture();
    update_list_fixture(&mut memory);
    let file = verified(&bytes);
    for watched in [
        BASE + 0x3d69ff8,
        0x600000,
        0x600000 + 0x1f1d8,
        0x600000 + 0x1f1e0,
        0x600000 + 0x1f1e8,
        0xe00000,
        0xe00010,
        0x610000,
        0x620008,
    ] {
        memory.watch_address = watched;
        memory.slot_reads.set(0);
        memory.change_slot = true;
        assert!(
            object_catalog(&file, &memory)
                .world_update_list(7, 2)
                .is_err(),
            "{watched:x}"
        );
    }
    memory.change_slot = false;
    memory.put64(0x620000, BASE + 0x2a4e980);
    assert!(
        object_catalog(&file, &memory)
            .world_update_list(7, 2)
            .is_err()
    );
}

#[test]
fn resistance_copies_signed_selected_words_without_reading_other_arrays() {
    let (bytes, mut memory) = fixture();
    let target = module_fixture(&mut memory, CharacterModule::Resistance);
    let words = [i32::MIN, -1, 0, 1, 999999, 1000000, i32::MAX];
    for (index, word) in words.into_iter().enumerate() {
        memory.put(0xd00010 + index as u64 * 4, &word.to_le_bytes());
    }
    let file = verified(&bytes);
    let value = object_catalog(&file, &memory)
        .character_resistance(target)
        .unwrap()
        .unwrap();
    assert_eq!(value.words, words);
    assert_eq!(value.module.address, 0xd00000);
    assert!(
        object_catalog(&file, &memory)
            .character_resistance(character_sets::CharacterIdentity { epoch: 0, ..target })
            .is_err()
    );
    memory.put64(0xc00020, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .character_resistance(target)
            .unwrap(),
        None
    );
}
#[test]
fn resistance_rejects_word_owner_slot_type_identity_changes_and_truncation() {
    let (bytes, mut memory) = fixture();
    let target = module_fixture(&mut memory, CharacterModule::Resistance);
    memory.put(0xd00010, &[0xa5; 28]);
    let file = verified(&bytes);
    for watched in [0xd00010, 0xd00008, 0xd00000, 0xc00020, 0x610190, 0x610008] {
        memory.watch_address = watched;
        memory.slot_reads.set(0);
        memory.change_slot = true;
        assert!(
            object_catalog(&file, &memory)
                .character_resistance(target)
                .is_err(),
            "{watched:x}"
        );
    }
    memory.change_slot = false;
    memory.bytes.remove(&0xd0002b);
    assert!(
        object_catalog(&file, &memory)
            .character_resistance(target)
            .is_err()
    );
}

#[test]
fn character_distance_words_preserve_raw_values_and_never_read_unrequested_neighbors() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for kind in [ObjectKind::Character, ObjectKind::Enemy, ObjectKind::Player] {
        let (_, mut memory) = fixture();
        let target = opacity_fixture(&mut memory, kind, [0; 4]);
        let words = [0x7fc01234, 0x80000000, 0xff800000, 0x00000001];
        for (offset, word) in [0x3fc, 0x400, 0x408, 0x40c].into_iter().zip(words) {
            memory.put(target.address + offset, &u32::to_le_bytes(word));
        }
        let value = object_catalog(&checked, &memory)
            .character_distance_words(target)
            .unwrap()
            .unwrap();
        assert_eq!(value.words, words);
        assert_eq!(value.character, target);
        assert!(!memory.bytes.contains_key(&(target.address + 0x404)));
        assert!(!memory.bytes.contains_key(&(target.address + 0x410)));
    }
}
#[test]
fn character_distance_words_reject_selected_truncations_and_detectable_identity_changes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for offset in [0x3fc, 0x400, 0x408, 0x40c] {
        for truncated in [false, true] {
            let (_, mut memory) = fixture();
            let target = opacity_fixture(&mut memory, ObjectKind::Enemy, [0; 4]);
            for field in [0x3fc, 0x400, 0x408, 0x40c] {
                memory.put(target.address + field, &7u32.to_le_bytes());
            }
            if truncated {
                memory.bytes.remove(&(target.address + offset + 3));
            } else {
                memory.watch_address = target.address + offset;
                memory.change_slot = true;
                memory.slot_reads.set(0);
            }
            assert!(
                object_catalog(&checked, &memory)
                    .character_distance_words(target)
                    .is_err()
            );
        }
    }
    for offset in [0, 8] {
        let (_, mut memory) = fixture();
        let target = opacity_fixture(&mut memory, ObjectKind::Player, [0; 4]);
        for field in [0x3fc, 0x400, 0x408, 0x40c] {
            memory.put(target.address + field, &7u32.to_le_bytes());
        }
        memory.watch_address = target.address + offset;
        memory.change_slot = true;
        memory.slot_reads.set(0);
        assert!(
            object_catalog(&checked, &memory)
                .character_distance_words(target)
                .is_err()
        );
    }
}

#[test]
fn character_model_translation_preserves_selected_cached_row_without_reading_other_matrix_rows() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for kind in [ObjectKind::Character, ObjectKind::Enemy, ObjectKind::Player] {
        let (_, mut memory) = fixture();
        let target = controller_fixture(&mut memory, kind);
        let words = [0x7fc01234u32, 0x80000000, 0xff800000, 1];
        for (i, word) in words.into_iter().enumerate() {
            memory.put(0xe00260 + i as u64 * 4, &word.to_le_bytes());
        }
        let value = object_catalog(&checked, &memory)
            .character_model_translation(target)
            .unwrap()
            .unwrap();
        assert_eq!(value.translation_bits, words);
        assert_eq!(value.controller.character, target);
        assert!(!memory.bytes.contains_key(&0xe00230));
        assert!(!memory.bytes.contains_key(&0xe00100));
        assert!(
            object_catalog(&checked, &memory)
                .character_model_translation(character_sets::CharacterIdentity {
                    epoch: 0,
                    ..target
                })
                .is_err()
        );
        memory.put64(target.address + 0x58, 0);
        assert_eq!(
            object_catalog(&checked, &memory)
                .character_model_translation(target)
                .unwrap(),
            None
        );
    }
}

#[test]
fn frontend_anchor_rows_preserve_raw_selected_lanes_and_apply_native_xyz_equation() {
    let (bytes, _) = fixture();
    let file = verified(&bytes);
    for kind in [ObjectKind::Character, ObjectKind::Enemy, ObjectKind::Player] {
        let (_, mut memory) = fixture();
        let target = controller_fixture(&mut memory, kind);
        let up = [
            1f32.to_bits(),
            2f32.to_bits(),
            (-1f32).to_bits(),
            0x7fc01234,
        ];
        let base = [
            10f32.to_bits(),
            20f32.to_bits(),
            30f32.to_bits(),
            0x80000000,
        ];
        for (address, words) in [(0xe00240, up), (0xe00260, base)] {
            for (i, word) in words.into_iter().enumerate() {
                memory.put(address + i as u64 * 4, &word.to_le_bytes());
            }
        }
        let copy = object_catalog(&file, &memory)
            .character_model_anchor_rows(target)
            .unwrap()
            .unwrap();
        assert_eq!((copy.up_bits, copy.translation_bits), (up, base));
        assert_eq!(copy.controller.character, target);
        assert_eq!(
            copy.frontend_displacement_xyz(3.0).unwrap(),
            [3.0, 6.0, -3.0]
        );
        assert_eq!(copy.lifted_xyz(3.0).unwrap(), [13.0, 26.0, 27.0]);
        assert!(!memory.bytes.contains_key(&0xe00230));
        assert!(!memory.bytes.contains_key(&0xe00250));
        for height in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, f32::MAX] {
            assert!(copy.lifted_xyz(height).is_err());
        }
        for field in 0..3 {
            for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut changed = copy;
                changed.up_bits[field] = bad.to_bits();
                assert!(changed.lifted_xyz(0.0).is_err());
                let mut changed = copy;
                changed.translation_bits[field] = bad.to_bits();
                assert!(changed.lifted_xyz(0.0).is_err());
            }
        }
        memory.put64(target.address + 0x58, 0);
        assert_eq!(
            object_catalog(&file, &memory)
                .character_model_anchor_rows(target)
                .unwrap(),
            None
        );
        assert!(
            object_catalog(&file, &memory)
                .character_model_anchor_rows(character_sets::CharacterIdentity {
                    epoch: 0,
                    ..target
                })
                .is_err()
        );
    }
}

#[test]
fn frontend_anchor_rows_reject_each_truncation_or_visible_selected_byte_and_owner_change() {
    struct Changed<'a> {
        memory: &'a SparseMemory,
        address: u64,
        reads: Cell<usize>,
    }
    impl AddressSpace for Changed<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if address <= self.address && self.address - address < output.len() as u64 {
                let count = self.reads.get();
                self.reads.set(count + 1);
                if count > 0 {
                    output[(self.address - address) as usize] ^= 0x80;
                }
            }
            Ok(())
        }
    }
    let (bytes, _) = fixture();
    let file = verified(&bytes);
    for selected in (0xe00240..0xe00250).chain(0xe00260..0xe00270) {
        let (_, mut memory) = fixture();
        let target = controller_fixture(&mut memory, ObjectKind::Player);
        memory.put(0xe00240, &[0xa5; 16]);
        memory.put(0xe00260, &[0x5a; 16]);
        let changed = Changed {
            memory: &memory,
            address: selected,
            reads: Cell::new(0),
        };
        assert!(
            GameObjectCatalog {
                module: file
                    .bind(
                        &changed,
                        BASE,
                        &[CodeRange {
                            rva: 0x1000,
                            length: 1
                        }]
                    )
                    .unwrap()
            }
            .character_model_anchor_rows(target)
            .is_err(),
            "changed {selected:x}"
        );
        memory.bytes.remove(&selected);
        assert!(
            object_catalog(&file, &memory)
                .character_model_anchor_rows(target)
                .is_err(),
            "missing {selected:x}"
        );
    }
    for selected in [
        0xe00000,
        0xe00010,
        0xe002d4,
        0xe002d8,
        0xe002dc,
        0x610000,
        0x610008,
        0x610058,
        BASE + 0x3d69ff8,
    ] {
        let (_, mut memory) = fixture();
        let target = controller_fixture(&mut memory, ObjectKind::Enemy);
        memory.put(0xe00240, &[0xa5; 16]);
        memory.put(0xe00260, &[0x5a; 16]);
        let changed = Changed {
            memory: &memory,
            address: selected,
            reads: Cell::new(0),
        };
        assert!(
            GameObjectCatalog {
                module: file
                    .bind(
                        &changed,
                        BASE,
                        &[CodeRange {
                            rva: 0x1000,
                            length: 1
                        }]
                    )
                    .unwrap()
            }
            .character_model_anchor_rows(target)
            .is_err(),
            "owner {selected:x}"
        );
    }
}
#[test]
fn character_model_translation_rejects_truncated_row_and_changed_controller_owner_type_link_or_identity()
 {
    let (file, _) = fixture();
    let checked = verified(&file);
    for watched in [0xe00260, 0xe00000, 0xe00010, 0xe002d4, 0x610058, 0x610008] {
        let (_, mut memory) = fixture();
        let target = controller_fixture(&mut memory, ObjectKind::Player);
        memory.put(0xe00260, &[0xa5; 16]);
        memory.watch_address = watched;
        memory.change_slot = true;
        memory.slot_reads.set(0);
        assert!(
            object_catalog(&checked, &memory)
                .character_model_translation(target)
                .is_err(),
            "{watched:x}"
        );
    }
    for missing in 0xe00260..0xe00270 {
        let (_, mut memory) = fixture();
        let target = controller_fixture(&mut memory, ObjectKind::Player);
        memory.put(0xe00260, &[0; 16]);
        memory.bytes.remove(&missing);
        assert!(
            object_catalog(&checked, &memory)
                .character_model_translation(target)
                .is_err(),
            "{missing:x}"
        );
    }
}

fn model_bounds_fixture(
    memory: &mut SparseMemory,
    exported: bool,
) -> character_sets::CharacterIdentity {
    let target = visual_fixture(memory);
    memory.put64(0xe10010, 0xe60000);
    memory.put64(0xe60660, 0xe70000);
    memory.put64(0xe70028, if exported { 0xe80000 } else { 0 });
    let bounds = if exported {
        0xe80060
    } else {
        memory.put64(0xe60658, 0xe90000);
        0xe90090
    };
    for (offset, words) in [
        (0, [0x7fc01234u32, 0x80000000, 0xff800000]),
        (0x10, [1u32, 2, 3]),
    ] {
        for (i, word) in words.into_iter().enumerate() {
            memory.put(bounds + offset + i as u64 * 4, &word.to_le_bytes());
        }
    }
    target
}
#[test]
fn model_bounds_preserve_selected_words_and_distinguish_exported_and_fallback_routes() {
    let (file, _) = fixture();
    let checked = verified(&file);
    for exported in [false, true] {
        let (_, mut memory) = fixture();
        let target = model_bounds_fixture(&mut memory, exported);
        let value = object_catalog(&checked, &memory)
            .character_model_bounds(target)
            .unwrap()
            .unwrap();
        assert_eq!(value.minimum_bits, [0x7fc01234, 0x80000000, 0xff800000]);
        assert_eq!(value.maximum_bits, [1, 2, 3]);
        assert_eq!(
            value.fallback_address,
            if exported { None } else { Some(0xe90000) }
        );
        assert_eq!(value.model.character, target);
        assert!(!memory.bytes.contains_key(&(value.bounds_address + 12)));
        assert!(!memory.bytes.contains_key(&(value.bounds_address + 28)));
        if exported {
            assert!(!memory.bytes.contains_key(&0xe60658));
        }
        assert!(
            object_catalog(&checked, &memory)
                .character_model_bounds(character_sets::CharacterIdentity { epoch: 0, ..target })
                .is_err()
        );
        memory.put64(target.address + 0x50, 0);
        assert_eq!(
            object_catalog(&checked, &memory)
                .character_model_bounds(target)
                .unwrap(),
            None
        );
    }
}
#[test]
fn model_bounds_reject_every_selected_truncation_and_detectable_route_or_identity_change() {
    struct Changed<'a> {
        memory: &'a SparseMemory,
        watched: u64,
        reads: Cell<u32>,
    }
    impl AddressSpace for Changed<'_> {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            self.memory.read_exact(address, output)?;
            if address <= self.watched && self.watched < address + output.len() as u64 {
                let prior = self.reads.replace(self.reads.get() + 1);
                if prior > 0 {
                    output[(self.watched - address) as usize] ^= 1;
                }
            }
            Ok(())
        }
    }
    let (file, _) = fixture();
    let checked = verified(&file);
    for exported in [false, true] {
        let bounds = if exported { 0xe80060 } else { 0xe90090 };
        for missing in (bounds..bounds + 12).chain(bounds + 16..bounds + 28) {
            let (_, mut memory) = fixture();
            let target = model_bounds_fixture(&mut memory, exported);
            memory.bytes.remove(&missing);
            assert!(
                object_catalog(&checked, &memory)
                    .character_model_bounds(target)
                    .is_err(),
                "{exported}/{missing:x}"
            );
        }
        for watched in [
            0xe10000,
            0xe10010,
            0xe60660,
            0xe70028,
            bounds,
            bounds + 16,
            0x610050,
            0x610008,
        ] {
            let (_, mut memory) = fixture();
            let target = model_bounds_fixture(&mut memory, exported);
            let changed = Changed {
                memory: &memory,
                watched,
                reads: Cell::new(0),
            };
            let objects = GameObjectCatalog {
                module: checked
                    .bind(
                        &changed,
                        BASE,
                        &[CodeRange {
                            rva: 0x1000,
                            length: 1,
                        }],
                    )
                    .unwrap(),
            };
            assert!(
                objects.character_model_bounds(target).is_err(),
                "{exported}/{watched:x}"
            );
        }
    }
    for (link, value) in [
        (0xe10010, 0),
        (0xe10010, 0xe60001),
        (0xe60660, 0),
        (0xe70028, 1),
        (0xe60658, 0),
    ] {
        let (_, mut memory) = fixture();
        let target = model_bounds_fixture(&mut memory, false);
        memory.put64(link, value);
        assert!(
            object_catalog(&checked, &memory)
                .character_model_bounds(target)
                .is_err()
        );
    }
}

fn kind_record_fixture() -> (Vec<u8>, SparseMemory, character_sets::CharacterIdentity) {
    let (mut file, mut memory) = fixture();
    let (_, target) = registration_fixture(&mut memory);
    memory.put(target.address + 0x68, &1i32.to_le_bytes());
    memory.put(target.address + 0x188, &1u32.to_le_bytes());
    put32(&mut file, 0x1d8 + 12, 0x3b1bc00);
    put32(&mut file, 0x1d8 + 36, 0xc0000040);
    for row in 0..24usize {
        for offset in [1usize, 8, 9, 10] {
            let value = (row as u8).wrapping_mul(7).wrapping_add(offset as u8);
            file[0x600 + row * 20 + offset] = value;
            // Unselected table bytes are deliberately unreadable in the address space.
            memory.put(BASE + 0x3b1bc00 + (row * 20 + offset) as u64, &[value]);
        }
    }
    memory.put(BASE, &file[..0x200]);
    (file, memory, target)
}
#[test]
fn kind_records_preserve_raw_bytes_full_selection_and_default_signed_index_without_unselected_reads()
 {
    let (file, mut memory, target) = kind_record_fixture();
    let file = verified(&file);
    for kind in [
        i32::MIN,
        -1,
        0,
        1,
        3,
        4,
        10,
        11,
        14,
        17,
        19,
        22,
        23,
        i32::MAX,
    ] {
        memory.put(target.address + 0x68, &kind.to_le_bytes());
        memory.put(target.address + 0x188, &u32::MAX.to_le_bytes());
        let actual = object_catalog(&file, &memory)
            .character_kind_record(target)
            .unwrap()
            .unwrap();
        let row = if (kind as u32) < 23 { kind as u32 } else { 23 };
        assert_eq!(actual.selection.character, target);
        assert_eq!(actual.selection.raw_kind, kind);
        assert_eq!(actual.selection.raw_character_number, u32::MAX);
        assert_eq!(actual.record_rva, 0x3b1bc00 + row * 20);
        assert_eq!(
            actual.selected_bytes,
            [1u8, 8, 9, 10].map(|offset| (row as u8).wrapping_mul(7).wrapping_add(offset))
        );
    }
}
#[test]
fn kind_records_reject_every_selected_byte_truncation_change_and_changed_full_source() {
    for offset in [1u64, 8, 9, 10] {
        let (file, mut memory, target) = kind_record_fixture();
        memory.put(target.address + 0x68, &22i32.to_le_bytes());
        let file = verified(&file);
        let address = BASE + 0x3b1bc00 + 22 * 20 + offset;
        let value = memory.bytes.remove(&address).unwrap();
        assert!(
            object_catalog(&file, &memory)
                .character_kind_record(target)
                .is_err()
        );
        memory.put(address, &[value ^ 1]);
        let changed = object_catalog(&file, &memory)
            .character_kind_record(target)
            .unwrap()
            .unwrap();
        let index = [1u64, 8, 9, 10]
            .iter()
            .position(|known| *known == offset)
            .unwrap();
        assert_eq!(changed.selected_bytes[index], value ^ 1);
        memory.put(address, &[value]);
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&file, &memory)
                .character_kind_record(target)
                .is_err()
        );
    }
    for address in [0x600000u64, 0x600008, 0x600068, 0x600188, BASE + 0x3d69ff8] {
        let (file, mut memory, target) = kind_record_fixture();
        let file = verified(&file);
        // Use the fixture's real target address rather than assuming its allocation.
        let address = if address < BASE {
            target.address + address - 0x600000
        } else {
            address
        };
        memory.watch_address = address;
        memory.change_slot = true;
        assert!(
            object_catalog(&file, &memory)
                .character_kind_record(target)
                .is_err(),
            "changed selected source {address:x}"
        );
    }
}
#[test]
fn kind_records_reject_wrong_section_permissions_cross_section_table_and_stale_identity() {
    for (offset, value) in [
        (0x1d8 + 36, 0x40000040),
        (0x1d8 + 36, 0xe0000040),
        (0x1d8 + 36, 0x80000040),
        (0x1d8 + 16, 479),
    ] {
        let (mut file, mut memory, target) = kind_record_fixture();
        put32(&mut file, offset, value);
        memory.put(BASE, &file[..0x200]);
        let file = verified(&file);
        assert!(
            object_catalog(&file, &memory)
                .character_kind_record(target)
                .is_err()
        );
    }
    let (file, mut memory, target) = kind_record_fixture();
    let file = verified(&file);
    let wrong = character_sets::CharacterIdentity {
        native_identifier: target.native_identifier ^ 1,
        ..target
    };
    assert!(
        object_catalog(&file, &memory)
            .character_kind_record(wrong)
            .is_err()
    );
    assert!(
        object_catalog(&file, &memory)
            .character_kind_record(character_sets::CharacterIdentity { epoch: 0, ..target })
            .is_err()
    );
    memory.put64(BASE + 0x3d69ff8, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .character_kind_record(target)
            .unwrap(),
        None
    );
}

#[test]
fn settings_prefix_preserves_raw_bytes_and_requires_only_the_requested_extent() {
    let (bytes, mut memory) = fixture();
    let file = verified(&bytes);
    memory.put64(BASE + 0x3d61f98, 0x4200000);
    memory.put64(0x4200058, 0x4300000);
    let raw: Vec<_> = (0..0xb0).map(|i| (i as u8).wrapping_mul(17)).collect();
    memory.put(0x4300000, &raw);
    let objects = object_catalog(&file, &memory);
    let value = objects.game_settings_prefix(7, 0xb0).unwrap().unwrap();
    assert_eq!(value.bytes, raw);
    assert_eq!(
        (value.manager_address, value.settings_address, value.epoch),
        (0x4200000, 0x4300000, 7)
    );
    assert_eq!(
        objects.game_settings_prefix(7, 9).unwrap().unwrap().bytes,
        raw[..9]
    );
    assert!(objects.game_settings_prefix(7, 0xb1).is_err());
    for length in [0, 0x141, usize::MAX] {
        assert!(objects.game_settings_prefix(7, length).is_err());
    }
    assert!(objects.game_settings_prefix(0, 0xb0).is_err());
    for offset in 0..0xb0 {
        let address = 0x4300000 + offset;
        let byte = memory.bytes.remove(&address).unwrap();
        assert!(
            object_catalog(&file, &memory)
                .game_settings_prefix(7, 0xb0)
                .is_err()
        );
        memory.put(address, &[byte]);
    }
    memory.put64(0x4200058, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .game_settings_prefix(7, 0xb0)
            .unwrap(),
        None
    );
    memory.put64(BASE + 0x3d61f98, 0);
    assert_eq!(
        object_catalog(&file, &memory)
            .game_settings_prefix(7, 0xb0)
            .unwrap(),
        None
    );
}
#[test]
fn settings_prefix_rejects_visible_changes_and_unaligned_links() {
    let (bytes, mut memory) = fixture();
    let file = verified(&bytes);
    memory.put64(BASE + 0x3d61f98, 0x4200000);
    memory.put64(0x4200058, 0x4300000);
    memory.put(0x4300000, &[0xa5; 0xb0]);
    for watched in [BASE + 0x3d61f98, 0x4200058, 0x4300000] {
        memory.watch_address = watched;
        memory.change_slot = true;
        memory.slot_reads.set(0);
        assert!(
            object_catalog(&file, &memory)
                .game_settings_prefix(7, 0xb0)
                .is_err()
        );
    }
    memory.change_slot = false;
    for bad in [1, 0x4300001] {
        memory.put64(0x4200058, bad);
        assert!(
            object_catalog(&file, &memory)
                .game_settings_prefix(7, 0xb0)
                .is_err()
        );
    }
    memory.put64(BASE + 0x3d61f98, 0x4200001);
    assert!(
        object_catalog(&file, &memory)
            .game_settings_prefix(7, 0xb0)
            .is_err()
    );
}
