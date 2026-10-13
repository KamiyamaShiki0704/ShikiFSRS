use super::*;
use std::cell::{Cell, RefCell};

struct Memory {
    records: RefCell<Vec<CharacterScalarSnapshot>>,
    writes: RefCell<Vec<CharacterScalarKey>>,
    fail: Cell<Option<CharacterScalarKey>>,
    wrong_key: Cell<bool>,
    change_read: Cell<bool>,
    reads: Cell<u32>,
}
fn key(index: u64, field: CharacterScalar) -> CharacterScalarKey {
    CharacterScalarKey {
        character: CharacterIdentity {
            address: 0x1000 + index * 8,
            world_address: 0x2000,
            native_identifier: index + 1,
            epoch: 3,
        },
        field,
    }
}

#[test]
fn opacity_overlay_restores_first_raw_baseline_in_reverse_and_preserves_foreign_factor() {
    let memory = Memory::new();
    let fields = [
        CharacterScalar::BaseOpacity,
        CharacterScalar::OpacityFactor0,
        CharacterScalar::OpacityFactor1,
        CharacterScalar::OpacityFactor2,
        CharacterScalar::OpacityControl244,
        CharacterScalar::BaseOpacityControl250,
    ];
    let originals = [1.0f32, 0.5, -0.0, 2.0, -0.25, -1.0];
    let mut overlays = CharacterScalarOverlays::default();
    for (field, value) in fields.into_iter().zip(originals) {
        let k = key(0, field);
        memory.add(k, value);
        assert_eq!(
            // SAFETY: Original backend with fully exclusive owned record storage, no game access.
            unsafe { overlays.apply(&memory, k, 0.0) }.unwrap(),
            CharacterReplaceOutcome::Applied
        );
        assert_eq!(
            // SAFETY: Same original lifetime/exclusive phase; repeated update retains first baseline.
            unsafe { overlays.apply(&memory, k, 0.25) }.unwrap(),
            CharacterReplaceOutcome::Applied
        );
    }
    let foreign_key = key(0, fields[5]);
    memory
        .records
        .borrow_mut()
        .iter_mut()
        .find(|r| r.key == foreign_key)
        .unwrap()
        .bits = 0.75f32.to_bits();
    memory.writes.borrow_mut().clear();
    // SAFETY: Original exclusive records, conditional restores preserve foreign changed factor.
    let report = unsafe { overlays.restore(&memory) };
    assert_eq!(report.pending, 0);
    assert_eq!(report.superseded, vec![foreign_key]);
    assert_eq!(
        report.restored,
        vec![
            key(0, fields[4]),
            key(0, fields[3]),
            key(0, fields[2]),
            key(0, fields[1]),
            key(0, fields[0])
        ]
    );
    assert_eq!(*memory.writes.borrow(), report.restored);
    for (field, value) in fields[..5].iter().copied().zip(originals) {
        assert_eq!(memory.get(key(0, field)).to_bits(), value.to_bits());
    }
    assert_eq!(memory.get(foreign_key), 0.75);
}
impl Memory {
    fn new() -> Self {
        Self {
            records: RefCell::new(Vec::new()),
            writes: RefCell::new(Vec::new()),
            fail: Cell::new(None),
            wrong_key: Cell::new(false),
            change_read: Cell::new(false),
            reads: Cell::new(0),
        }
    }
    fn add(&self, key: CharacterScalarKey, value: f32) {
        self.records.borrow_mut().push(CharacterScalarSnapshot {
            key,
            bits: value.to_bits(),
            storage: CharacterScalarStorage {
                address: 0x3000,
                kind: ObjectKind::PlayerController,
                container_address: 0,
            },
        });
    }
    fn get(&self, key: CharacterScalarKey) -> f32 {
        f32::from_bits(
            self.records
                .borrow()
                .iter()
                .find(|r| r.key == key)
                .unwrap()
                .bits,
        )
    }
}
// SAFETY: Original single-threaded test storage. Compares key, storage and bits before
// changing one record; injected failures and rejected writes have no side effects.
unsafe impl CharacterScalarAccess for Memory {
    fn observe(&self, key: CharacterScalarKey) -> Result<Option<CharacterScalarSnapshot>> {
        let mut value = self.records.borrow().iter().find(|r| r.key == key).copied();
        let previous = self.reads.get();
        self.reads.set(previous + 1);
        if let Some(value) = value.as_mut() {
            if self.wrong_key.get() {
                value.key.character.native_identifier += 1;
            }
            if self.change_read.get() && previous > 0 {
                value.bits = 4f32.to_bits();
            }
        }
        Ok(value)
    }
    unsafe fn compare_replace(
        &self,
        expected: CharacterScalarSnapshot,
        bits: u32,
    ) -> Result<CharacterReplaceOutcome> {
        if self.fail.get() == Some(expected.key) {
            return Err(Error::InvalidProfile("injected write failure"));
        }
        let mut records = self.records.borrow_mut();
        let Some(current) = records.iter_mut().find(|r| r.key == expected.key) else {
            return Ok(CharacterReplaceOutcome::Unavailable);
        };
        if current.storage != expected.storage {
            return Ok(CharacterReplaceOutcome::Unavailable);
        }
        if current.bits != expected.bits {
            return Ok(CharacterReplaceOutcome::Conflict);
        }
        current.bits = bits;
        self.writes.borrow_mut().push(expected.key);
        Ok(CharacterReplaceOutcome::Applied)
    }
}

#[test]
fn multiplier_uses_first_baseline_then_restores_exactly_in_reverse_order() {
    let memory = Memory::new();
    let x = key(0, CharacterScalar::AxisX);
    let height = key(0, CharacterScalar::CharacterHitHeight);
    memory.add(x, 2.0);
    memory.add(height, 3.0);
    let mut owner = CharacterScalarOverlays::default();
    // SAFETY: Original exclusive test storage only; no game access.
    unsafe {
        assert_eq!(
            owner.multiply(&memory, x, 2.0).unwrap(),
            CharacterReplaceOutcome::Applied
        );
        assert_eq!(
            owner.multiply(&memory, x, 3.0).unwrap(),
            CharacterReplaceOutcome::Applied
        );
        assert_eq!(memory.get(x), 6.0);
        owner.apply(&memory, height, 9.0).unwrap();
        owner.apply(&memory, height, 6.0).unwrap();
        assert_eq!(owner.pending(), 2);
        let report = owner.restore(&memory);
        assert_eq!(report.restored, [height, x]);
        assert_eq!(report.pending, 0);
    }
    assert_eq!(memory.get(x), 2.0);
    assert_eq!(memory.get(height), 3.0);
    assert_eq!(memory.writes.borrow().len(), 6);
}

#[test]
fn cleanup_preserves_foreign_values_replaced_storage_and_failed_obligations() {
    let memory = Memory::new();
    let keys = [
        key(0, CharacterScalar::AxisX),
        key(0, CharacterScalar::AxisY),
        key(0, CharacterScalar::AxisZ),
    ];
    for key in keys {
        memory.add(key, 1.0);
    }
    let mut owner = CharacterScalarOverlays::default();
    // SAFETY: Original exclusive test storage only.
    unsafe {
        for key in keys {
            owner.apply(&memory, key, 2.0).unwrap();
        }
    }
    memory.records.borrow_mut()[0].bits = 7f32.to_bits();
    memory.records.borrow_mut()[1].storage.address += 8;
    memory.fail.set(Some(keys[2]));
    // SAFETY: Original exclusive test storage only.
    let report = unsafe { owner.restore(&memory) };
    assert_eq!(report.superseded, [keys[0]]);
    assert_eq!(report.unavailable, [keys[1]]);
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.pending, 1);
    assert_eq!(memory.get(keys[0]), 7.0);
    assert_eq!(memory.get(keys[1]), 2.0);
    memory.fail.set(None);
    // SAFETY: Same original storage; retry does not replay successful/foreign entries.
    let report = unsafe { owner.restore(&memory) };
    assert_eq!(report.restored, [keys[2]]);
    assert_eq!(report.pending, 0);
    assert_eq!(memory.writes.borrow().len(), 4);
}

#[test]
fn missing_changed_epoch_and_drop_never_write_old_character_storage() {
    let memory = Memory::new();
    let x = key(0, CharacterScalar::AxisX);
    memory.add(x, 1.0);
    let mut owner = CharacterScalarOverlays::default();
    // SAFETY: Original exclusive test storage only.
    unsafe {
        assert_eq!(
            owner
                .apply(&memory, key(1, CharacterScalar::AxisX), 2.0)
                .unwrap(),
            CharacterReplaceOutcome::Unavailable
        );
        owner.apply(&memory, x, 2.0).unwrap();
    }
    memory.records.borrow_mut()[0].key.character.epoch += 1;
    // SAFETY: Original exclusive test storage; old key resolves unavailable.
    let report = unsafe { owner.restore(&memory) };
    assert_eq!(report.unavailable, [x]);
    assert_eq!(report.pending, 0);
    let current = memory.records.borrow()[0].key;
    // SAFETY: Original test storage's new key.
    unsafe {
        owner.apply(&memory, current, 3.0).unwrap();
    }
    let before = memory.writes.borrow().len();
    drop(owner);
    assert_eq!(memory.writes.borrow().len(), before);
    assert_eq!(memory.get(current), 3.0);
}

#[test]
fn multiplier_rejects_nonfinite_overflow_underflow_and_changed_initial_baseline() {
    let memory = Memory::new();
    let x = key(0, CharacterScalar::AxisX);
    memory.add(x, 2.0);
    let mut owner = CharacterScalarOverlays::default();
    // SAFETY: Original exclusive test storage only.
    unsafe {
        for multiplier in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::MAX] {
            assert!(owner.multiply(&memory, x, multiplier).is_err());
        }
        memory.change_read.set(true);
        memory.reads.set(0);
        assert!(owner.multiply(&memory, x, 2.0).is_err());
        memory.change_read.set(false);
        memory.records.borrow_mut()[0].bits = 1;
        assert!(owner.multiply(&memory, x, 0.1).is_err());
    }
    assert!(memory.writes.borrow().is_empty());
    assert_eq!(owner.pending(), 0);
}

#[test]
fn owner_enforces_bound_invalid_keys_and_originals_but_preserves_zero_physics_bits() {
    let memory = Memory::new();
    let zero = key(0, CharacterScalar::Weight);
    memory.add(zero, -0.0);
    let mut owner = CharacterScalarOverlays::default();
    // SAFETY: Original exclusive test storage only.
    unsafe {
        owner.apply(&memory, zero, 0.0).unwrap();
        assert_eq!(owner.pending(), 1);
        owner.restore(&memory);
        assert_eq!(memory.records.borrow()[0].bits, (-0f32).to_bits());
        memory.wrong_key.set(true);
        assert!(owner.apply(&memory, zero, 1.0).is_err());
        memory.wrong_key.set(false);
        let invalid = CharacterScalarKey {
            character: CharacterIdentity {
                epoch: 0,
                ..zero.character
            },
            ..zero
        };
        assert!(owner.apply(&memory, invalid, 1.0).is_err());
        assert!(owner.apply(&memory, zero, f32::NAN).is_err());
        memory.records.borrow_mut()[0].bits = (-1f32).to_bits();
        assert_eq!(
            owner.apply(&memory, zero, 1.0).unwrap(),
            CharacterReplaceOutcome::Applied
        );
        assert_eq!(owner.restore(&memory).pending, 0);
        assert_eq!(memory.get(zero), -1.0);
        for index in 1..=65 {
            let x = key(index, CharacterScalar::AxisX);
            memory.add(x, 1.0);
            let result = owner.apply(&memory, x, 2.0);
            if index <= 64 {
                assert_eq!(result.unwrap(), CharacterReplaceOutcome::Applied);
            } else {
                assert!(result.is_err());
            }
        }
    }
    assert_eq!(owner.pending(), 64);
    assert_eq!(memory.get(key(65, CharacterScalar::AxisX)), 1.0);
}
