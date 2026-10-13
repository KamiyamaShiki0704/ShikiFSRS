use super::*;
use crate::{character_scalars::CharacterScalarStorage, objects::ObjectKind};
use std::cell::{Cell, RefCell};
struct Memory {
    records: RefCell<Vec<CharacterScalarSnapshot>>,
    writes: RefCell<Vec<CharacterScalarKey>>,
    fail: Cell<Option<F>>,
    reads: Cell<usize>,
    late_change: Cell<bool>,
}
fn target() -> CharacterIdentity {
    CharacterIdentity {
        address: 0x1000,
        world_address: 0x2000,
        native_identifier: 5,
        epoch: 7,
    }
}
fn policy() -> CharacterScalePolicy {
    CharacterScalePolicy {
        collision_dimensions: true,
        weight: WeightScalePolicy::Multiply { floor: Some(1.0) },
    }
}
fn fields() -> [F; 8] {
    [
        F::AxisX,
        F::AxisY,
        F::AxisZ,
        F::CharacterHitHeight,
        F::CharacterHitRadius,
        F::MapHitHeight,
        F::MapHitRadius,
        F::Weight,
    ]
}
impl Memory {
    fn new() -> Self {
        let records = fields()
            .into_iter()
            .zip([2.0f32, 3.0, 4.0, 0.0, -0.0, 5.0, 6.0, -1.0])
            .map(|(field, value)| CharacterScalarSnapshot {
                key: CharacterScalarKey {
                    character: target(),
                    field,
                },
                storage: CharacterScalarStorage {
                    address: if matches!(field, F::AxisX | F::AxisY | F::AxisZ) {
                        0x3000
                    } else {
                        0x4000
                    },
                    kind: if matches!(field, F::AxisX | F::AxisY | F::AxisZ) {
                        ObjectKind::PlayerController
                    } else {
                        ObjectKind::CharacterPhysics
                    },
                    container_address: 0,
                },
                bits: value.to_bits(),
            })
            .collect();
        Self {
            records: RefCell::new(records),
            writes: RefCell::new(Vec::new()),
            fail: Cell::new(None),
            reads: Cell::new(0),
            late_change: Cell::new(false),
        }
    }
    fn bits(&self, field: F) -> u32 {
        self.records
            .borrow()
            .iter()
            .find(|r| r.key.field == field)
            .unwrap()
            .bits
    }
}
// SAFETY: Original exclusively owned RefCell records; failed compares write nothing and Applied
// performs one recorded store with no later fallible step. No game/threads/native memory involved.
unsafe impl CharacterScalarAccess for Memory {
    fn observe(&self, key: CharacterScalarKey) -> Result<Option<CharacterScalarSnapshot>> {
        let count = self.reads.replace(self.reads.get() + 1);
        let mut copy = self.records.borrow().iter().find(|r| r.key == key).copied();
        if self.late_change.get()
            && count >= 8
            && let Some(copy) = &mut copy
        {
            copy.bits ^= 1;
        }
        Ok(copy)
    }
    unsafe fn compare_replace(
        &self,
        expected: CharacterScalarSnapshot,
        bits: u32,
    ) -> Result<CharacterReplaceOutcome> {
        if self.fail.get() == Some(expected.key.field) {
            return Err(Error::InvalidProfile("original injected scalar failure"));
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
fn scale_session_updates_from_original_preserves_zero_and_restores_signed_weight_exactly() {
    let memory = Memory::new();
    let mut session = CharacterScaleSession::capture(&memory, target(), policy())
        .unwrap()
        .unwrap();
    // SAFETY: Original exclusive mock records throughout all operations; no native calls.
    unsafe {
        let first = session.apply(&memory, 2.0).unwrap();
        assert!(first.complete());
        assert_eq!(first.applied.len(), 6);
        assert_eq!(first.unchanged.len(), 2);
        assert_eq!(memory.bits(F::Weight), 1f32.to_bits());
        assert_eq!(memory.bits(F::CharacterHitRadius), (-0f32).to_bits());
        let second = session.apply(&memory, 3.0).unwrap();
        assert!(second.complete());
        assert_eq!(memory.bits(F::AxisX), 6f32.to_bits());
        assert_eq!(memory.bits(F::MapHitHeight), 15f32.to_bits());
        let count = memory.writes.borrow().len();
        assert_eq!(session.apply(&memory, 3.0).unwrap().unchanged.len(), 8);
        assert_eq!(memory.writes.borrow().len(), count);
        memory.writes.borrow_mut().clear();
        let report = session.restore(&memory);
        assert_eq!(report.pending, 0);
        assert_eq!(report.restored.len(), 6);
        assert_eq!(memory.bits(F::Weight), (-1f32).to_bits());
        assert_eq!(memory.bits(F::AxisX), 2f32.to_bits());
        assert_eq!(report.restored, *memory.writes.borrow());
        assert_eq!(report.restored.first().unwrap().field, F::Weight);
        assert!(session.apply(&memory, 2.0).is_err());
    }
}
#[test]
fn scale_session_retains_partial_prefix_retries_restore_and_preserves_foreign_changes() {
    let memory = Memory::new();
    let mut session = CharacterScaleSession::capture(&memory, target(), policy())
        .unwrap()
        .unwrap();
    memory.fail.set(Some(F::AxisZ));
    // SAFETY: Exclusive original storage and compare contract; injected errors have no stores.
    unsafe {
        let report = session.apply(&memory, 2.0).unwrap();
        assert!(!report.complete());
        assert_eq!(report.applied.len(), 2);
        assert_eq!(report.pending_restore, 2);
        assert_eq!(report.stopped.unwrap().key.field, F::AxisZ);
        assert_eq!(memory.bits(F::AxisX), 4f32.to_bits());
        assert_eq!(memory.bits(F::AxisZ), 4f32.to_bits());
        memory.fail.set(None);
        assert!(session.apply(&memory, 3.0).unwrap().complete());
        memory
            .records
            .borrow_mut()
            .iter_mut()
            .find(|r| r.key.field == F::AxisX)
            .unwrap()
            .bits = 11f32.to_bits();
        memory.fail.set(Some(F::AxisY));
        let report = session.restore(&memory);
        assert_eq!(report.pending, 1);
        assert_eq!(report.superseded[0].field, F::AxisX);
        assert_eq!(memory.bits(F::AxisX), 11f32.to_bits());
        memory.fail.set(None);
        assert_eq!(session.restore(&memory).pending, 0);
        assert_eq!(memory.bits(F::AxisY), 3f32.to_bits());
    }
}
#[test]
fn scale_session_numeric_whole_baseline_and_policy_failures_perform_no_writes() {
    let memory = Memory::new();
    memory.late_change.set(true);
    assert!(CharacterScaleSession::capture(&memory, target(), policy()).is_err());
    memory.late_change.set(false);
    memory.reads.set(0);
    let mut session = CharacterScaleSession::capture(&memory, target(), policy())
        .unwrap()
        .unwrap();
    // SAFETY: Original record storage; all invalid operations fail before any mock store.
    unsafe {
        for multiplier in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::MAX] {
            assert!(session.apply(&memory, multiplier).is_err());
        }
        assert!(memory.writes.borrow().is_empty());
        memory.records.borrow_mut()[0].storage.address += 8;
        assert!(session.apply(&memory, 2.0).is_err());
        assert!(memory.writes.borrow().is_empty());
    }
    for floor in [-1.0, f32::NAN, f32::INFINITY] {
        assert!(
            CharacterScaleSession::capture(
                &memory,
                target(),
                CharacterScalePolicy {
                    weight: WeightScalePolicy::Multiply { floor: Some(floor) },
                    ..policy()
                }
            )
            .is_err()
        );
    }
    let tiny = Memory::new();
    tiny.records.borrow_mut()[0].bits = 1;
    let mut tiny_session = CharacterScaleSession::capture(&tiny, target(), policy())
        .unwrap()
        .unwrap();
    assert!(
        // SAFETY: Original exclusive records; no game memory or calls.
        unsafe { tiny_session.apply(&tiny, 0.5) }.is_err()
    );
    assert!(tiny.writes.borrow().is_empty());
    let other = Memory::new();
    other
        .records
        .borrow_mut()
        .retain(|r| matches!(r.key.field, F::AxisX | F::AxisY | F::AxisZ));
    let mut session = CharacterScaleSession::capture(
        &other,
        target(),
        CharacterScalePolicy {
            collision_dimensions: false,
            weight: WeightScalePolicy::Unchanged,
        },
    )
    .unwrap()
    .unwrap();
    assert!(
        // SAFETY: Exclusive three original axis records; unselected physics storage is absent.
        unsafe { session.apply(&other, 2.0) }.unwrap().complete()
    );
    assert_eq!(other.writes.borrow().len(), 3);
}
