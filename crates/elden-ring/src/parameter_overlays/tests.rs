use super::*;
use std::cell::{Cell, RefCell};

struct Backend {
    fields: RefCell<Vec<FieldSnapshot>>,
    calls: RefCell<Vec<FieldKey>>,
    fail: Cell<bool>,
    conflict: Cell<bool>,
}
fn key(field: ParameterField) -> FieldKey {
    FieldKey {
        field,
        variant: 0,
        row_id: 10,
    }
}

#[test]
fn signed_byte_restore_preserves_raw_negative_original_and_rejects_wide_bits() {
    let field = ParameterField::NpcAttackSfxDirection;
    let backend = Backend::new(vec![snapshot(field, 0xff)]);
    let mut owner = ParameterOverlays::default();
    // SAFETY: Synthetic exclusive byte observations; no native process is accessed.
    unsafe {
        assert_eq!(
            owner
                .apply(&backend, key(field), ScalarValue::I8(3))
                .unwrap(),
            ReplaceOutcome::Applied
        );
        assert_eq!(owner.restore(&backend).restored, vec![key(field)]);
    }
    assert_eq!(backend.bits(field), 0xff);
    let wide = Backend::new(vec![snapshot(field, 0xffff_ffff)]);
    // SAFETY: Malformed synthetic backend must fail before any replacement call.
    assert!(unsafe { owner.apply(&wide, key(field), ScalarValue::I8(0)) }.is_err());
    assert!(wide.calls.borrow().is_empty());
    assert_eq!(owner.pending(), 0);
}

#[test]
fn byte_team_overlays_keep_full_range_first_original_and_refuse_wide_backend_bits() {
    let field = ParameterField::NpcTeamType;
    let backend = Backend::new(vec![snapshot(field, 255)]);
    let mut owner = ParameterOverlays::default();
    // SAFETY: Original exclusive field records, no game memory or callbacks.
    unsafe {
        assert!(
            owner
                .apply(&backend, key(field), ScalarValue::U32(1))
                .is_err()
        );
        assert!(backend.calls.borrow().is_empty());
        assert_eq!(
            owner
                .apply(&backend, key(field), ScalarValue::U8(1))
                .unwrap(),
            ReplaceOutcome::Applied
        );
        assert_eq!(
            owner
                .apply(&backend, key(field), ScalarValue::U8(2))
                .unwrap(),
            ReplaceOutcome::Applied
        );
        assert_eq!(owner.pending(), 1);
        assert_eq!(owner.restore(&backend).restored, vec![key(field)]);
    }
    assert_eq!(backend.bits(field), 255);
    let bad = Backend::new(vec![snapshot(field, 256)]);
    // SAFETY: Original exclusive malformed copy must be rejected before backend writes.
    let rejected = unsafe { owner.apply(&bad, key(field), ScalarValue::U8(0)) };
    assert!(rejected.is_err());
    assert!(bad.calls.borrow().is_empty());
    assert_eq!(owner.pending(), 0);
}
fn snapshot(field: ParameterField, bits: u32) -> FieldSnapshot {
    FieldSnapshot {
        key: key(field),
        bits,
        identity: RowIdentity {
            epoch: 1,
            resource: 0x1000,
            header: 0x2000,
            index: 0x2800,
            row: 0x3000,
        },
    }
}
impl Backend {
    fn new(fields: Vec<FieldSnapshot>) -> Self {
        Self {
            fields: RefCell::new(fields),
            calls: RefCell::new(Vec::new()),
            fail: Cell::new(false),
            conflict: Cell::new(false),
        }
    }
    fn bits(&self, field: ParameterField) -> u32 {
        self.fields
            .borrow()
            .iter()
            .find(|item| item.key == key(field))
            .unwrap()
            .bits
    }
}
// SAFETY: Tests exclusively own RefCell-backed copied storage, never native objects. Error
// and comparison rejection precede mutation; conditional replacement is one borrow operation.
unsafe impl ParameterAccess for Backend {
    fn observe(&self, key: FieldKey) -> Result<Option<FieldSnapshot>> {
        Ok(self
            .fields
            .borrow()
            .iter()
            .find(|item| item.key == key)
            .copied())
    }
    unsafe fn compare_replace(&self, expected: FieldSnapshot, bits: u32) -> Result<ReplaceOutcome> {
        self.calls.borrow_mut().push(expected.key);
        if self.fail.replace(false) {
            return Err(Error::NativeOperationRejected("synthetic write failure"));
        }
        let mut fields = self.fields.borrow_mut();
        let Some(current) = fields.iter_mut().find(|item| item.key == expected.key) else {
            return Ok(ReplaceOutcome::Unavailable);
        };
        if current.identity != expected.identity {
            return Ok(ReplaceOutcome::Unavailable);
        }
        if current.bits != expected.bits || self.conflict.replace(false) {
            return Ok(ReplaceOutcome::Conflict);
        }
        current.bits = bits;
        Ok(ReplaceOutcome::Applied)
    }
}

#[test]
fn camera_updates_restore_first_original_values_in_reverse_order() {
    let backend = Backend::new(vec![
        snapshot(ParameterField::CameraDistance, 4f32.to_bits()),
        snapshot(ParameterField::CameraHeight, 1.42f32.to_bits()),
    ]);
    let mut owner = ParameterOverlays::default();
    // SAFETY: Synthetic exclusively owned field storage for this test.
    unsafe {
        assert_eq!(
            owner
                .apply(
                    &backend,
                    key(ParameterField::CameraDistance),
                    ScalarValue::F32(8.0)
                )
                .unwrap(),
            ReplaceOutcome::Applied
        );
        owner
            .apply(
                &backend,
                key(ParameterField::CameraHeight),
                ScalarValue::F32(2.0),
            )
            .unwrap();
        owner
            .apply(
                &backend,
                key(ParameterField::CameraDistance),
                ScalarValue::F32(12.0),
            )
            .unwrap();
        assert_eq!(owner.pending(), 2);
        let report = owner.restore(&backend);
        assert_eq!(
            report.restored,
            [
                key(ParameterField::CameraHeight),
                key(ParameterField::CameraDistance)
            ]
        );
        assert_eq!(report.pending, 0);
    }
    assert_eq!(backend.bits(ParameterField::CameraDistance), 4f32.to_bits());
    assert_eq!(
        backend.bits(ParameterField::CameraHeight),
        1.42f32.to_bits()
    );
}

#[test]
fn foreign_modification_is_reported_without_overwriting_it() {
    let backend = Backend::new(vec![snapshot(
        ParameterField::BuddyTriggerEffect,
        (-1i32) as u32,
    )]);
    let mut owner = ParameterOverlays::default();
    // SAFETY: Synthetic exclusively owned storage.
    unsafe {
        owner
            .apply(
                &backend,
                key(ParameterField::BuddyTriggerEffect),
                ScalarValue::I32(899000),
            )
            .unwrap();
    }
    backend.fields.borrow_mut()[0].bits = 899001;
    // SAFETY: Synthetic exclusively owned storage; compare detects the foreign value.
    let report = unsafe { owner.restore(&backend) };
    assert_eq!(report.superseded, [key(ParameterField::BuddyTriggerEffect)]);
    assert_eq!(report.pending, 0);
    assert_eq!(backend.bits(ParameterField::BuddyTriggerEffect), 899001);
}

#[test]
fn failed_restore_retries_but_reused_row_epoch_is_never_written() {
    let backend = Backend::new(vec![snapshot(ParameterField::BuddyNpc, (-1i32) as u32)]);
    let mut owner = ParameterOverlays::default();
    // SAFETY: Synthetic exclusive field storage.
    unsafe {
        owner
            .apply(
                &backend,
                key(ParameterField::BuddyNpc),
                ScalarValue::I32(25000000),
            )
            .unwrap();
    }
    backend.fail.set(true);
    // SAFETY: Synthetic exclusive field storage.
    let first = unsafe { owner.restore(&backend) };
    assert_eq!(first.failures.len(), 1);
    assert_eq!(first.pending, 1);
    assert_eq!(backend.bits(ParameterField::BuddyNpc), 25000000);
    backend.fields.borrow_mut()[0].identity.epoch = 2;
    // SAFETY: The new epoch is compared without using the old row address.
    let retry = unsafe { owner.restore(&backend) };
    assert_eq!(retry.unavailable, [key(ParameterField::BuddyNpc)]);
    assert_eq!(retry.pending, 0);
    assert_eq!(backend.bits(ParameterField::BuddyNpc), 25000000);
}

#[test]
fn absent_row_compare_conflict_and_failed_apply_do_not_create_ownership() {
    let backend = Backend::new(vec![snapshot(ParameterField::BuddyThink, 10)]);
    let mut owner = ParameterOverlays::default();
    // SAFETY: All storage is synthetic and exclusively owned.
    unsafe {
        assert_eq!(
            owner
                .apply(
                    &backend,
                    key(ParameterField::BuddyNpc),
                    ScalarValue::I32(20)
                )
                .unwrap(),
            ReplaceOutcome::Unavailable
        );
        backend.conflict.set(true);
        assert_eq!(
            owner
                .apply(
                    &backend,
                    key(ParameterField::BuddyThink),
                    ScalarValue::I32(20)
                )
                .unwrap(),
            ReplaceOutcome::Conflict
        );
        backend.fail.set(true);
        assert!(
            owner
                .apply(
                    &backend,
                    key(ParameterField::BuddyThink),
                    ScalarValue::I32(20)
                )
                .is_err()
        );
    }
    assert_eq!(owner.pending(), 0);
    assert_eq!(backend.bits(ParameterField::BuddyThink), 10);
}

#[test]
fn floating_zero_is_compared_and_restored_bitwise_and_drop_never_writes() {
    let backend = Backend::new(vec![snapshot(
        ParameterField::CameraDistance,
        (-0.0f32).to_bits(),
    )]);
    let mut owner = ParameterOverlays::default();
    // SAFETY: Synthetic field storage exclusively owned by this test.
    unsafe {
        owner
            .apply(
                &backend,
                key(ParameterField::CameraDistance),
                ScalarValue::F32(0.0),
            )
            .unwrap();
        assert_eq!(owner.pending(), 1);
        owner.restore(&backend);
    }
    assert_eq!(
        backend.bits(ParameterField::CameraDistance),
        (-0.0f32).to_bits()
    );
    // SAFETY: Same synthetic exclusive storage.
    unsafe {
        owner
            .apply(
                &backend,
                key(ParameterField::CameraDistance),
                ScalarValue::F32(8.0),
            )
            .unwrap();
    }
    let calls = backend.calls.borrow().len();
    drop(owner);
    assert_eq!(backend.calls.borrow().len(), calls);
    assert_eq!(backend.bits(ParameterField::CameraDistance), 8f32.to_bits());
}

#[test]
fn invalid_types_nonfinite_originals_and_configuration_bounds_fail_before_writes() {
    let backend = Backend::new(vec![snapshot(
        ParameterField::CameraDistance,
        f32::NAN.to_bits(),
    )]);
    let mut owner = ParameterOverlays::default();
    // SAFETY: Synthetic exclusively owned storage, all requests must reject.
    unsafe {
        assert!(
            owner
                .apply(
                    &backend,
                    key(ParameterField::CameraDistance),
                    ScalarValue::I32(1)
                )
                .is_err()
        );
        assert!(
            owner
                .apply(
                    &backend,
                    key(ParameterField::CameraDistance),
                    ScalarValue::F32(f32::INFINITY)
                )
                .is_err()
        );
        assert!(
            owner
                .apply(
                    &backend,
                    key(ParameterField::CameraDistance),
                    ScalarValue::F32(1.0)
                )
                .is_err()
        );
    }
    assert!(backend.calls.borrow().is_empty());
    let fields = (0..65)
        .map(|id| FieldSnapshot {
            key: FieldKey {
                row_id: id,
                ..key(ParameterField::BuddyNpc)
            },
            ..snapshot(ParameterField::BuddyNpc, 0)
        })
        .collect();
    let backend = Backend::new(fields);
    // SAFETY: Synthetic exclusive field storage, configuration has an enforced finite bound.
    unsafe {
        for id in 0..64 {
            owner
                .apply(
                    &backend,
                    FieldKey {
                        row_id: id,
                        ..key(ParameterField::BuddyNpc)
                    },
                    ScalarValue::I32(1),
                )
                .unwrap();
        }
        assert!(
            owner
                .apply(
                    &backend,
                    FieldKey {
                        row_id: 64,
                        ..key(ParameterField::BuddyNpc)
                    },
                    ScalarValue::I32(1)
                )
                .is_err()
        );
    }
    assert_eq!(owner.pending(), 64);
    assert_eq!(backend.calls.borrow().len(), 64);
}
