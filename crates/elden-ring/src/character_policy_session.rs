//! Selected raw policy edits with an actual-store journal and explicit conditional restoration.
//! This does not supply a complete hide, activation, death, control or retirement operation.
use crate::{
    character_flags::{CharacterFlagByte, CharacterFlagsObservation},
    character_modules::CharacterModuleObservation,
    character_scalars::{
        CharacterReplaceOutcome, CharacterScalar, CharacterScalarAccess, CharacterScalarKey,
        CharacterScalarSnapshot, CharacterScalarStorage,
    },
    character_sets::CharacterIdentity,
    controller_policy::{ControllerPolicyField, ControllerPolicyObservation},
    current_character_flags::CurrentCharacterFlags,
    current_character_scalars::CurrentCharacterScalarAccess,
    current_controller_policy::CurrentControllerPolicy,
    current_physics_suppression::CurrentPhysicsSuppression,
    objects::ObjectKind,
    physics_suppression::PhysicsSuppressionObservation,
};
use shikifsrs_runtime::{Error, Result};
use std::{marker::PhantomData, rc::Rc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualOpacityField {
    Control244,
    Control250,
    Factor240,
    Base24c,
}
impl ManualOpacityField {
    fn scalar(self) -> CharacterScalar {
        match self {
            Self::Control244 => CharacterScalar::OpacityControl244,
            Self::Control250 => CharacterScalar::BaseOpacityControl250,
            Self::Factor240 => CharacterScalar::OpacityFactor1,
            Self::Base24c => CharacterScalar::BaseOpacity,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterPolicyField {
    Flag(CharacterFlagByte),
    Controller(ControllerPolicyField),
    PhysicsRequest,
    ManualOpacity(ManualOpacityField),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterPolicyEdit {
    pub field: CharacterPolicyField,
    pub mask: u32,
    /// Raw masked bits; opacity requires a whole word and a valid finite scalar.
    pub bits: u32,
}
impl CharacterPolicyEdit {
    fn validate(self) -> Result<()> {
        let byte = matches!(
            self.field,
            CharacterPolicyField::Flag(_)
                | CharacterPolicyField::PhysicsRequest
                | CharacterPolicyField::Controller(ControllerPolicyField::ControllerLow)
        );
        if self.mask == 0 || self.bits & !self.mask != 0 || (byte && self.mask > 255) {
            return Err(Error::InvalidProfile("invalid selected policy mask/value"));
        }
        if let CharacterPolicyField::ManualOpacity(field) = self.field
            && (self.mask != u32::MAX || !field.scalar().valid(self.bits))
        {
            return Err(Error::InvalidProfile("invalid selected manual opacity"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterPolicyStorage {
    Character,
    Controller {
        address: u64,
        kind: ObjectKind,
        modifier_address: u64,
    },
    Physics(CharacterModuleObservation),
    Opacity(CharacterScalarStorage),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterPolicySnapshot {
    pub character: CharacterIdentity,
    pub field: CharacterPolicyField,
    pub storage: CharacterPolicyStorage,
    pub value: u32,
}
#[derive(Debug)]
pub struct CharacterPolicyWrite {
    pub source: CharacterPolicySnapshot,
    pub mask: u32,
    pub after_value: u32,
    /// True only after an actual store. Postflight failure does not erase it.
    pub changed: bool,
    pub postflight: Result<CharacterPolicySnapshot>,
}
#[derive(Debug)]
pub struct CharacterPolicyStop {
    pub field: Option<CharacterPolicyField>,
    pub error: Error,
}
#[derive(Debug, Default)]
pub struct CharacterPolicyReport {
    pub writes: Vec<CharacterPolicyWrite>,
    pub superseded: Vec<CharacterPolicyField>,
    /// A prior actual restore was confirmed by a later read, without another store.
    pub reconciled: Vec<CharacterPolicyField>,
    pub stopped: Option<CharacterPolicyStop>,
    pub pending_restore: usize,
}

/// Borrowed production readers/stores. Their bind contracts continue to apply throughout use.
pub struct CurrentCharacterPolicyAccess<'a> {
    pub flags: &'a CurrentCharacterFlags,
    pub controller: &'a CurrentControllerPolicy,
    pub physics: &'a CurrentPhysicsSuppression,
    pub scalars: &'a CurrentCharacterScalarAccess,
}
fn flag_copy(v: CharacterFlagsObservation, field: CharacterFlagByte) -> CharacterPolicySnapshot {
    CharacterPolicySnapshot {
        character: v.character,
        field: CharacterPolicyField::Flag(field),
        storage: CharacterPolicyStorage::Character,
        value: u32::from(v.value(field)),
    }
}
fn controller_copy(
    v: ControllerPolicyObservation,
    field: ControllerPolicyField,
) -> Option<CharacterPolicySnapshot> {
    Some(CharacterPolicySnapshot {
        character: v.controller.character,
        field: CharacterPolicyField::Controller(field),
        storage: CharacterPolicyStorage::Controller {
            address: v.controller.address,
            kind: v.controller.kind,
            modifier_address: v.modifier_address,
        },
        value: v.value(field)?,
    })
}
fn physics_copy(v: PhysicsSuppressionObservation) -> CharacterPolicySnapshot {
    CharacterPolicySnapshot {
        character: v.module.character,
        field: CharacterPolicyField::PhysicsRequest,
        storage: CharacterPolicyStorage::Physics(v.module),
        value: u32::from(v.request_byte()),
    }
}
fn opacity_copy(v: CharacterScalarSnapshot, field: ManualOpacityField) -> CharacterPolicySnapshot {
    CharacterPolicySnapshot {
        character: v.key.character,
        field: CharacterPolicyField::ManualOpacity(field),
        storage: CharacterPolicyStorage::Opacity(v.storage),
        value: v.bits,
    }
}
fn absent() -> Error {
    Error::NativeOperationRejected("selected policy unavailable")
}
fn changed() -> Error {
    Error::NativeOperationRejected("selected policy source changed")
}

// Closed adapter, not a public fallback or consumer-supplied mock implementation.
trait Access {
    fn observe(
        &self,
        character: CharacterIdentity,
        field: CharacterPolicyField,
    ) -> Result<Option<CharacterPolicySnapshot>>;
    // Production call sites are reached only from public unsafe apply/restore.
    fn write(
        &self,
        source: CharacterPolicySnapshot,
        mask: u32,
        bits: u32,
    ) -> Result<CharacterPolicyWrite>;
}
impl Access for CurrentCharacterPolicyAccess<'_> {
    fn observe(
        &self,
        character: CharacterIdentity,
        field: CharacterPolicyField,
    ) -> Result<Option<CharacterPolicySnapshot>> {
        Ok(match field {
            CharacterPolicyField::Flag(f) => {
                self.flags.observe(character)?.map(|v| flag_copy(v, f))
            }
            CharacterPolicyField::Controller(f) => self
                .controller
                .observe(character)?
                .and_then(|v| controller_copy(v, f)),
            CharacterPolicyField::PhysicsRequest => {
                self.physics.observe(character)?.map(physics_copy)
            }
            CharacterPolicyField::ManualOpacity(f) => self
                .scalars
                .observe(CharacterScalarKey {
                    character,
                    field: f.scalar(),
                })?
                .map(|v| opacity_copy(v, f)),
        })
    }
    fn write(
        &self,
        source: CharacterPolicySnapshot,
        mask: u32,
        bits: u32,
    ) -> Result<CharacterPolicyWrite> {
        CharacterPolicyEdit {
            field: source.field,
            mask,
            bits,
        }
        .validate()?;
        let (after_value, actual, postflight) = match source.field {
            CharacterPolicyField::Flag(f) => {
                let current = self.flags.observe(source.character)?.ok_or_else(absent)?;
                if flag_copy(current, f) != source {
                    return Err(changed());
                }
                // SAFETY: Only unsafe session mutation supplies the full live exclusive flags contract.
                let receipt =
                    unsafe { self.flags.compare_mask(current, f, mask as u8, bits as u8) }?;
                (
                    u32::from(receipt.after_value),
                    receipt.changed,
                    receipt.postflight.map(|v| flag_copy(v, f)),
                )
            }
            CharacterPolicyField::Controller(f) => {
                let current = self
                    .controller
                    .observe(source.character)?
                    .ok_or_else(absent)?;
                if controller_copy(current, f) != Some(source) {
                    return Err(changed());
                }
                // SAFETY: The session caller retains controller/modifier ownership, writable extent and phase.
                let receipt = unsafe { self.controller.compare_mask(current, f, mask, bits) }?;
                (
                    receipt.after_value,
                    receipt.changed,
                    receipt
                        .postflight
                        .and_then(|v| controller_copy(v, f).ok_or_else(absent)),
                )
            }
            CharacterPolicyField::PhysicsRequest => {
                let current = self.physics.observe(source.character)?.ok_or_else(absent)?;
                if physics_copy(current) != source {
                    return Err(changed());
                }
                let value = ((source.value & !mask) | bits) as u8;
                // SAFETY: Same exclusive pinned physics module and request byte phase supplied by the caller.
                let receipt = unsafe { self.physics.compare_request(current, value) }?;
                (
                    u32::from(receipt.after_request),
                    receipt.changed,
                    receipt.postflight.map(physics_copy),
                )
            }
            CharacterPolicyField::ManualOpacity(f) => {
                let current = self
                    .scalars
                    .observe(CharacterScalarKey {
                        character: source.character,
                        field: f.scalar(),
                    })?
                    .ok_or_else(absent)?;
                if opacity_copy(current, f) != source {
                    return Err(changed());
                }
                if source.value == bits {
                    return Ok(CharacterPolicyWrite {
                        source,
                        mask,
                        after_value: bits,
                        changed: false,
                        postflight: Ok(source),
                    });
                }
                // SAFETY: Session caller pins exact scalar destination and excludes all overlapping mutation.
                let outcome = unsafe { self.scalars.compare_replace(current, bits) }?;
                if outcome != CharacterReplaceOutcome::Applied {
                    return Err(changed());
                }
                // The actual store is established before any fallible observation; always return its receipt.
                let expected = CharacterPolicySnapshot {
                    value: bits,
                    ..source
                };
                let postflight = self.observe(source.character, source.field).and_then(|v| {
                    if v == Some(expected) {
                        Ok(expected)
                    } else {
                        Err(changed())
                    }
                });
                (bits, true, postflight)
            }
        };
        Ok(CharacterPolicyWrite {
            source,
            mask,
            after_value,
            changed: actual,
            postflight,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Prepared,
    Unchanged,
    Owned,
    RestoreCheck,
    Resolved,
    Superseded,
}
struct Entry {
    edit: CharacterPolicyEdit,
    original: CharacterPolicySnapshot,
    state: State,
}
/// One character, at most 32 distinct fields. Copies acquire no engine references or permission.
/// Keep this journal and every original allocation alive until all restore obligations resolve.
pub struct CharacterPolicySession {
    entries: Vec<Entry>,
    closing: bool,
    refresh_allowed: bool,
    _thread: PhantomData<Rc<()>>,
}
impl CharacterPolicySession {
    /// Capture selected raw fields twice before any mutation; missing required storage is None.
    pub fn capture(
        access: &CurrentCharacterPolicyAccess<'_>,
        character: CharacterIdentity,
        edits: &[CharacterPolicyEdit],
    ) -> Result<Option<Self>> {
        capture_with(access, character, edits)
    }
    pub fn pending_restore(&self) -> usize {
        self.entries
            .iter()
            .filter(|v| matches!(v.state, State::Owned | State::RestoreCheck))
            .count()
    }
    /// Explicit continuation writes only untouched selected fields, after fresh full batch checks.
    /// # Safety
    /// Every borrowed host's complete bind/store contract holds: true epoch, pinned full graph,
    /// exact writable storage and exclusive permitted engine phase with no aliases, callbacks,
    /// native updates, teardown, replacement, ABA, hooks or unload. Caller independently chooses
    /// appropriate raw bits/opacity semantics. This is not a transaction or gameplay permission.
    /// Retain this journal after failure; no automatic rollback or Drop mutation occurs.
    pub unsafe fn apply(
        &mut self,
        access: &CurrentCharacterPolicyAccess<'_>,
    ) -> CharacterPolicyReport {
        apply_with(self, access)
    }
    /// Reassert the same selected policy bits after intervening native updates. The first
    /// captured values remain the conditional restoration targets; unselected bits stay intact.
    /// # Safety
    /// Apply's full contract holds, and caller additionally reserves these exact selected bits
    /// for the entire active policy interval. This may overwrite changed selected bits, so it
    /// must not be used for control pointers, another owner's policy, or an uncertain attempt.
    /// A failed refresh disables further refreshes; explicit full-batch apply reconciliation
    /// or restore is required before further work.
    pub unsafe fn refresh(
        &mut self,
        access: &CurrentCharacterPolicyAccess<'_>,
    ) -> CharacterPolicyReport {
        refresh_with(self, access)
    }
    /// Reverse only still-owned bits in original storage; preserve current other bits and fields.
    /// # Safety
    /// Apply's complete graph lifetime, writable extent and exclusive phase guarantees hold.
    /// Retain uncertain actual restore receipts until a later observation resolves them. Pending0
    /// says nothing about engine references, callback drain, full reactivation or safe free.
    pub unsafe fn restore(
        &mut self,
        access: &CurrentCharacterPolicyAccess<'_>,
    ) -> CharacterPolicyReport {
        restore_with(self, access)
    }
}
fn capture_with(
    access: &impl Access,
    character: CharacterIdentity,
    edits: &[CharacterPolicyEdit],
) -> Result<Option<CharacterPolicySession>> {
    if character.epoch == 0
        || character.address == 0
        || character.world_address == 0
        || !character.address.is_multiple_of(8)
        || !character.world_address.is_multiple_of(8)
        || edits.is_empty()
        || edits.len() > 32
    {
        return Err(Error::InvalidProfile("invalid selected policy capture"));
    }
    for (i, edit) in edits.iter().enumerate() {
        edit.validate()?;
        if edits[..i].iter().any(|v| v.field == edit.field) {
            return Err(Error::InvalidProfile("duplicate selected policy field"));
        }
    }
    let read = || -> Result<Option<Vec<CharacterPolicySnapshot>>> {
        let mut result = Vec::with_capacity(edits.len());
        for edit in edits {
            let Some(value) = access.observe(character, edit.field)? else {
                return Ok(None);
            };
            if value.character != character || value.field != edit.field {
                return Err(changed());
            }
            if let CharacterPolicyField::ManualOpacity(f) = edit.field
                && !f.scalar().valid(value.value)
            {
                return Err(Error::InvalidProfile("invalid original manual opacity"));
            }
            result.push(value);
        }
        Ok(Some(result))
    };
    let Some(values) = read()? else {
        return Ok(None);
    };
    if read()? != Some(values.clone()) {
        return Err(changed());
    }
    Ok(Some(CharacterPolicySession {
        entries: edits
            .iter()
            .copied()
            .zip(values)
            .map(|(edit, original)| Entry {
                edit,
                original,
                state: State::Prepared,
            })
            .collect(),
        closing: false,
        refresh_allowed: false,
        _thread: PhantomData,
    }))
}
fn expected(entry: &Entry) -> CharacterPolicySnapshot {
    CharacterPolicySnapshot {
        value: if entry.state == State::Owned {
            (entry.original.value & !entry.edit.mask) | entry.edit.bits
        } else {
            entry.original.value
        },
        ..entry.original
    }
}
fn check_batch(session: &CharacterPolicySession, access: &impl Access) -> Result<()> {
    // Two complete selected-field rounds also detect late changes to earlier fields.
    for _ in 0..2 {
        for entry in &session.entries {
            match access.observe(entry.original.character, entry.edit.field)? {
                None => return Err(absent()),
                Some(value) if value == expected(entry) => {}
                Some(_) => return Err(changed()),
            }
        }
    }
    Ok(())
}
fn stop(report: &mut CharacterPolicyReport, field: Option<CharacterPolicyField>, error: Error) {
    report.stopped = Some(CharacterPolicyStop { field, error });
}
fn apply_with(session: &mut CharacterPolicySession, access: &impl Access) -> CharacterPolicyReport {
    session.refresh_allowed = false;
    let mut report = CharacterPolicyReport::default();
    if session.closing {
        stop(
            &mut report,
            None,
            Error::NativeOperationRejected("selected policy session closing"),
        );
    } else {
        for index in 0..session.entries.len() {
            if let Err(error) = check_batch(session, access) {
                stop(&mut report, None, error);
                break;
            }
            let entry = &mut session.entries[index];
            if entry.state != State::Prepared {
                continue;
            }
            match access.write(entry.original, entry.edit.mask, entry.edit.bits) {
                Err(error) => {
                    stop(&mut report, Some(entry.edit.field), error);
                    break;
                }
                Ok(receipt) => {
                    // Save the actual obligation before inspecting its fallible postflight.
                    entry.state = if receipt.changed {
                        State::Owned
                    } else {
                        State::Unchanged
                    };
                    let error = receipt.postflight.as_ref().err().cloned();
                    report.writes.push(receipt);
                    if let Some(error) = error {
                        stop(&mut report, Some(entry.edit.field), error);
                        break;
                    }
                }
            }
        }
        if report.stopped.is_none()
            && let Err(error) = check_batch(session, access)
        {
            stop(&mut report, None, error);
        }
    }
    report.pending_restore = session.pending_restore();
    session.refresh_allowed = report.stopped.is_none();
    report
}
fn refresh_with(
    session: &mut CharacterPolicySession,
    access: &impl Access,
) -> CharacterPolicyReport {
    let mut report = CharacterPolicyReport::default();
    if session.closing || !session.refresh_allowed {
        stop(
            &mut report,
            None,
            Error::NativeOperationRejected("selected policy refresh unavailable"),
        );
        report.pending_restore = session.pending_restore();
        return report;
    }
    session.refresh_allowed = false;
    for entry in &mut session.entries {
        let result = (|| -> Result<()> {
            let current = access
                .observe(entry.original.character, entry.edit.field)?
                .ok_or_else(absent)?;
            if current.character != entry.original.character
                || current.field != entry.edit.field
                || current.storage != entry.original.storage
            {
                return Err(changed());
            }
            // Whole fresh snapshot is the store precondition. Original restoration bits survive.
            if access.observe(entry.original.character, entry.edit.field)? != Some(current) {
                return Err(changed());
            }
            let receipt = access.write(current, entry.edit.mask, entry.edit.bits)?;
            if receipt.changed {
                entry.state = State::Owned;
            }
            let completion = receipt
                .postflight
                .as_ref()
                .map(|_| ())
                .map_err(Clone::clone);
            report.writes.push(receipt);
            completion
        })();
        if let Err(error) = result {
            stop(&mut report, Some(entry.edit.field), error);
            break;
        }
    }
    if report.stopped.is_none() {
        let read = || -> Result<Vec<CharacterPolicySnapshot>> {
            session
                .entries
                .iter()
                .map(|entry| {
                    let value = access
                        .observe(entry.original.character, entry.edit.field)?
                        .ok_or_else(absent)?;
                    if value.character != entry.original.character
                        || value.field != entry.edit.field
                        || value.storage != entry.original.storage
                        || value.value & entry.edit.mask != entry.edit.bits
                    {
                        return Err(changed());
                    }
                    Ok(value)
                })
                .collect()
        };
        if let Err(error) = read().and_then(|first| {
            if read()? == first {
                Ok(())
            } else {
                Err(changed())
            }
        }) {
            stop(&mut report, None, error);
        }
    }
    session.refresh_allowed = report.stopped.is_none();
    report.pending_restore = session.pending_restore();
    report
}
fn restore_with(
    session: &mut CharacterPolicySession,
    access: &impl Access,
) -> CharacterPolicyReport {
    session.closing = true;
    let mut report = CharacterPolicyReport::default();
    for entry in session.entries.iter_mut().rev() {
        if !matches!(entry.state, State::Owned | State::RestoreCheck) {
            continue;
        }
        let current = match access.observe(entry.original.character, entry.edit.field) {
            Ok(Some(value)) => value,
            Ok(None) => {
                stop(&mut report, Some(entry.edit.field), absent());
                break;
            }
            Err(error) => {
                stop(&mut report, Some(entry.edit.field), error);
                break;
            }
        };
        let claimed = if entry.state == State::RestoreCheck {
            entry.original.value & entry.edit.mask
        } else {
            entry.edit.bits
        };
        if current.character != entry.original.character
            || current.field != entry.edit.field
            || current.storage != entry.original.storage
            || current.value & entry.edit.mask != claimed
        {
            entry.state = State::Superseded;
            report.superseded.push(entry.edit.field);
            continue;
        }
        if entry.state == State::RestoreCheck {
            entry.state = State::Resolved;
            report.reconciled.push(entry.edit.field);
            continue;
        }
        match access.write(
            current,
            entry.edit.mask,
            entry.original.value & entry.edit.mask,
        ) {
            Err(error) => {
                stop(&mut report, Some(entry.edit.field), error);
                break;
            }
            Ok(receipt) => {
                // An actual restore must not be repeated after a failed observation.
                entry.state = State::RestoreCheck;
                let error = receipt.postflight.as_ref().err().cloned();
                report.writes.push(receipt);
                if let Some(error) = error {
                    stop(&mut report, Some(entry.edit.field), error);
                    break;
                }
                entry.state = State::Resolved;
            }
        }
    }
    report.pending_restore = session.pending_restore();
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character_modules::CharacterModule;
    use std::cell::{Cell, RefCell};
    fn character() -> CharacterIdentity {
        CharacterIdentity {
            address: 0x1000,
            world_address: 0x2000,
            native_identifier: 0xfedcba9876543210,
            epoch: 7,
        }
    }
    fn edits() -> Vec<CharacterPolicyEdit> {
        use CharacterPolicyField as F;
        vec![
            CharacterPolicyEdit {
                field: F::Flag(CharacterFlagByte::DebugHigh),
                mask: 1,
                bits: 1,
            },
            CharacterPolicyEdit {
                field: F::Controller(ControllerPolicyField::ModifierWord14),
                mask: 0xc,
                bits: 4,
            },
            CharacterPolicyEdit {
                field: F::Controller(ControllerPolicyField::ModifierWord1c),
                mask: 1,
                bits: 1,
            },
            CharacterPolicyEdit {
                field: F::Controller(ControllerPolicyField::ControllerLow),
                mask: 2,
                bits: 2,
            },
            CharacterPolicyEdit {
                field: F::PhysicsRequest,
                mask: 255,
                bits: 1,
            },
            CharacterPolicyEdit {
                field: F::ManualOpacity(ManualOpacityField::Control244),
                mask: u32::MAX,
                bits: 0,
            },
            CharacterPolicyEdit {
                field: F::ManualOpacity(ManualOpacityField::Control250),
                mask: u32::MAX,
                bits: 0,
            },
            CharacterPolicyEdit {
                field: F::ManualOpacity(ManualOpacityField::Factor240),
                mask: u32::MAX,
                bits: 0,
            },
            CharacterPolicyEdit {
                field: F::ManualOpacity(ManualOpacityField::Base24c),
                mask: u32::MAX,
                bits: 0,
            },
        ]
    }
    struct Mock {
        values: RefCell<Vec<CharacterPolicySnapshot>>,
        calls: Cell<usize>,
        stores: RefCell<Vec<CharacterPolicyField>>,
        pre_failure: Cell<Option<usize>>,
        post_failure: Cell<Option<usize>>,
        reads: Cell<usize>,
        read_failure: Cell<Option<usize>>,
        late_change: Cell<Option<usize>>,
        available: Cell<bool>,
    }
    impl Mock {
        fn new(edits: &[CharacterPolicyEdit]) -> Self {
            let values = edits
                .iter()
                .map(|edit| {
                    let (storage, value) = match edit.field {
                        CharacterPolicyField::Flag(_) => (CharacterPolicyStorage::Character, 0x80),
                        CharacterPolicyField::Controller(_) => (
                            CharacterPolicyStorage::Controller {
                                address: 0x3000,
                                kind: ObjectKind::PlayerController,
                                modifier_address: 0x4000,
                            },
                            0xa0,
                        ),
                        CharacterPolicyField::PhysicsRequest => (
                            CharacterPolicyStorage::Physics(CharacterModuleObservation {
                                character: character(),
                                module: CharacterModule::Physics,
                                container_address: 0x5000,
                                address: 0x6000,
                            }),
                            0xf0,
                        ),
                        CharacterPolicyField::ManualOpacity(_) => (
                            CharacterPolicyStorage::Opacity(CharacterScalarStorage {
                                address: 0x1000,
                                kind: ObjectKind::Player,
                                container_address: 0,
                            }),
                            1.0f32.to_bits(),
                        ),
                    };
                    CharacterPolicySnapshot {
                        character: character(),
                        field: edit.field,
                        storage,
                        value,
                    }
                })
                .collect();
            Self {
                values: RefCell::new(values),
                calls: Cell::new(0),
                stores: RefCell::new(Vec::new()),
                pre_failure: Cell::new(None),
                post_failure: Cell::new(None),
                reads: Cell::new(0),
                read_failure: Cell::new(None),
                late_change: Cell::new(None),
                available: Cell::new(true),
            }
        }
        fn capture(&self, edits: &[CharacterPolicyEdit]) -> CharacterPolicySession {
            capture_with(self, character(), edits).unwrap().unwrap()
        }
    }
    impl Access for Mock {
        fn observe(
            &self,
            _: CharacterIdentity,
            field: CharacterPolicyField,
        ) -> Result<Option<CharacterPolicySnapshot>> {
            let count = self.reads.get();
            self.reads.set(count + 1);
            if self.read_failure.get() == Some(count) {
                return Err(Error::InvalidProfile("original synthetic read failure"));
            }
            if !self.available.get() {
                return Ok(None);
            }
            if self.late_change.get() == Some(count) {
                self.values.borrow_mut()[0].value ^= 0x40;
            }
            Ok(self
                .values
                .borrow()
                .iter()
                .find(|v| v.field == field)
                .copied())
        }
        fn write(
            &self,
            source: CharacterPolicySnapshot,
            mask: u32,
            bits: u32,
        ) -> Result<CharacterPolicyWrite> {
            let count = self.calls.get();
            self.calls.set(count + 1);
            if self.pre_failure.get() == Some(count) {
                return Err(Error::InvalidProfile(
                    "original synthetic pre-store failure",
                ));
            }
            let mut values = self.values.borrow_mut();
            let current = values.iter_mut().find(|v| v.field == source.field).unwrap();
            assert_eq!(*current, source);
            let after_value = (source.value & !mask) | bits;
            let actual = source.value != after_value;
            if actual {
                current.value = after_value;
                self.stores.borrow_mut().push(source.field);
            }
            let postflight = if self.post_failure.get() == Some(self.stores.borrow().len()) {
                Err(Error::InvalidProfile(
                    "original synthetic post-store failure",
                ))
            } else {
                Ok(*current)
            };
            Ok(CharacterPolicyWrite {
                source,
                mask,
                after_value,
                changed: actual,
                postflight,
            })
        }
    }
    #[test]
    fn policy_session_restores_reverse_masked_bits_and_preserves_foreign_storage_and_values() {
        let plan = edits();
        let access = Mock::new(&plan);
        let original = access.values.borrow().clone();
        let mut session = access.capture(&plan);
        let applied = apply_with(&mut session, &access);
        assert!(applied.stopped.is_none());
        assert_eq!(applied.pending_restore, plan.len());
        assert_eq!(
            *access.stores.borrow(),
            plan.iter().map(|v| v.field).collect::<Vec<_>>()
        );
        assert!(apply_with(&mut session, &access).writes.is_empty());
        {
            let mut values = access.values.borrow_mut();
            values[0].value |= 0x40; // Unowned bits in the same byte must survive.
            values[1].value = (values[1].value & !0xc) | 8; // Foreign selected bits.
            if let CharacterPolicyStorage::Physics(ref mut module) = values[4].storage {
                module.address = 0x7000;
            }
        }
        let restored = restore_with(&mut session, &access);
        assert!(restored.stopped.is_none());
        assert_eq!(restored.pending_restore, 0);
        assert_eq!(restored.superseded, [plan[4].field, plan[1].field]);
        let written = plan
            .iter()
            .rev()
            .enumerate()
            .filter(|(_, v)| v.field != plan[4].field && v.field != plan[1].field)
            .map(|(_, v)| v.field)
            .collect::<Vec<_>>();
        assert_eq!(&access.stores.borrow()[plan.len()..], &written);
        let after = access.values.borrow();
        assert_eq!(after[0].value, original[0].value | 0x40);
        assert_eq!(after[1].value & 0xc, 8);
        assert_eq!(after[4].value, 1);
        for i in [2, 3, 5, 6, 7, 8] {
            assert_eq!(after[i], original[i]);
        }
        assert!(apply_with(&mut session, &access).stopped.is_some());
        assert!(restore_with(&mut session, &access).writes.is_empty());
        {
            let _finished = session;
        }
        assert_eq!(access.stores.borrow().len(), plan.len() * 2 - 2);
    }
    #[test]
    fn policy_session_retains_each_actual_prefix_and_reconciles_failed_restore_without_repeating_store()
     {
        let plan = edits();
        let n = plan.len();
        for failed in 0..n {
            for post in [false, true] {
                let access = Mock::new(&plan);
                let mut session = access.capture(&plan);
                if post {
                    access.post_failure.set(Some(failed + 1));
                } else {
                    access.pre_failure.set(Some(failed));
                }
                let partial = apply_with(&mut session, &access);
                assert!(partial.stopped.is_some());
                assert_eq!(partial.pending_restore, failed + usize::from(post));
                assert_eq!(access.stores.borrow().len(), partial.pending_restore);
                access.pre_failure.set(None);
                access.post_failure.set(None);
                assert!(apply_with(&mut session, &access).stopped.is_none());
                assert_eq!(access.stores.borrow().len(), n);
                assert_eq!(restore_with(&mut session, &access).pending_restore, 0);
                assert_eq!(access.stores.borrow().len(), n * 2);
            }
        }
        for failed in 0..n {
            for post in [false, true] {
                let access = Mock::new(&plan);
                let mut session = access.capture(&plan);
                assert!(apply_with(&mut session, &access).stopped.is_none());
                if post {
                    access.post_failure.set(Some(n + failed + 1));
                } else {
                    access.pre_failure.set(Some(access.calls.get() + failed));
                }
                let partial = restore_with(&mut session, &access);
                assert!(partial.stopped.is_some());
                assert_eq!(partial.pending_restore, n - failed);
                assert_eq!(access.stores.borrow().len(), n + failed + usize::from(post));
                access.pre_failure.set(None);
                access.post_failure.set(None);
                access.read_failure.set(Some(access.reads.get()));
                assert_eq!(
                    restore_with(&mut session, &access).pending_restore,
                    n - failed
                );
                access.read_failure.set(None);
                let final_report = restore_with(&mut session, &access);
                assert!(final_report.stopped.is_none());
                assert_eq!(final_report.pending_restore, 0);
                assert_eq!(final_report.reconciled.len(), usize::from(post));
                assert_eq!(access.stores.borrow().len(), n * 2);
            }
        }
        // A later foreign change after an uncertain actual restore is preserved and released.
        let access = Mock::new(&plan);
        let mut session = access.capture(&plan);
        apply_with(&mut session, &access);
        access.post_failure.set(Some(n + 1));
        assert!(restore_with(&mut session, &access).stopped.is_some());
        access.post_failure.set(None);
        access.values.borrow_mut()[n - 1].value = 0.25f32.to_bits();
        let report = restore_with(&mut session, &access);
        assert_eq!(report.superseded, [plan[n - 1].field]);
        assert_eq!(report.pending_restore, 0);
        assert_eq!(access.values.borrow()[n - 1].value, 0.25f32.to_bits());
        assert_eq!(access.stores.borrow().len(), n * 2);
    }
    #[test]
    fn reserved_policy_refresh_reasserts_selected_bits_keeps_unselected_changes_and_original_restore()
     {
        let plan = [edits()[0]];
        let access = Mock::new(&plan);
        let mut session = access.capture(&plan);
        assert!(refresh_with(&mut session, &access).stopped.is_some());
        assert!(access.stores.borrow().is_empty());
        assert!(apply_with(&mut session, &access).stopped.is_none());
        access.values.borrow_mut()[0].value = 0xc0; // Native reset selected bit; unrelated bit changed.
        let refresh = refresh_with(&mut session, &access);
        assert!(refresh.stopped.is_none());
        assert_eq!(access.values.borrow()[0].value, 0xc1);
        assert_eq!(refresh.pending_restore, 1);
        let restored = restore_with(&mut session, &access);
        assert!(restored.stopped.is_none());
        assert_eq!(restored.pending_restore, 0);
        assert_eq!(access.values.borrow()[0].value, 0xc0);
        assert!(refresh_with(&mut session, &access).stopped.is_some());
    }
    #[test]
    fn refresh_failed_postflight_blocks_repetition_and_retains_actual_restore_obligation() {
        let plan = [edits()[0]];
        let access = Mock::new(&plan);
        let mut session = access.capture(&plan);
        apply_with(&mut session, &access);
        access.values.borrow_mut()[0].value = 0x80;
        access.post_failure.set(Some(access.calls.get() + 1));
        let failed = refresh_with(&mut session, &access);
        assert!(failed.stopped.is_some());
        assert!(failed.writes[0].changed);
        assert_eq!(failed.pending_restore, 1);
        let calls = access.calls.get();
        assert!(refresh_with(&mut session, &access).stopped.is_some());
        assert_eq!(access.calls.get(), calls);
        access.post_failure.set(None);
        assert_eq!(restore_with(&mut session, &access).pending_restore, 0);
        assert_eq!(access.values.borrow()[0].value, 0x80);
    }
    #[test]
    fn refresh_refuses_replaced_storage_and_catches_late_selected_batch_changes() {
        let plan = [edits()[0], edits()[1]];
        let access = Mock::new(&plan);
        let mut session = access.capture(&plan);
        apply_with(&mut session, &access);
        access.values.borrow_mut()[0].character.epoch += 1;
        let calls = access.calls.get();
        assert!(refresh_with(&mut session, &access).stopped.is_some());
        assert_eq!(access.calls.get(), calls);
        let access = Mock::new(&plan);
        let mut session = access.capture(&plan);
        apply_with(&mut session, &access);
        access
            .late_change
            .set(Some(access.reads.get() + plan.len() * 2 + plan.len()));
        assert!(refresh_with(&mut session, &access).stopped.is_some());
    }
    #[test]
    fn policy_session_checks_all_inputs_and_selected_copies_before_any_store() {
        let plan = edits();
        let access = Mock::new(&plan);
        for invalid in [
            CharacterPolicyEdit { mask: 0, ..plan[0] },
            CharacterPolicyEdit {
                bits: 256,
                ..plan[0]
            },
            CharacterPolicyEdit {
                mask: 256,
                bits: 0,
                ..plan[0]
            },
            CharacterPolicyEdit {
                bits: f32::NAN.to_bits(),
                ..plan[8]
            },
            CharacterPolicyEdit {
                bits: (-1.0f32).to_bits(),
                ..plan[8]
            },
            CharacterPolicyEdit {
                mask: 1,
                bits: 0,
                ..plan[8]
            },
        ] {
            assert!(capture_with(&access, character(), &[invalid]).is_err());
        }
        assert!(capture_with(&access, character(), &[]).is_err());
        assert!(capture_with(&access, character(), &[plan[0], plan[0]]).is_err());
        assert!(capture_with(&access, character(), &vec![plan[0]; 33]).is_err());
        assert!(
            capture_with(
                &access,
                CharacterIdentity {
                    epoch: 0,
                    ..character()
                },
                &plan
            )
            .is_err()
        );
        access.available.set(false);
        assert!(capture_with(&access, character(), &plan).unwrap().is_none());
        access.available.set(true);
        access.values.borrow_mut()[8].value = f32::INFINITY.to_bits();
        assert!(capture_with(&access, character(), &plan).is_err());
        access.values.borrow_mut()[8].value = 1.0f32.to_bits();
        access
            .late_change
            .set(Some(access.reads.get() + plan.len()));
        assert!(capture_with(&access, character(), &plan).is_err());
        access.late_change.set(None);
        access.values.borrow_mut()[0].character.native_identifier ^= 1u64 << 48;
        assert!(capture_with(&access, character(), &plan).is_err());
        assert!(access.stores.borrow().is_empty());
        let access = Mock::new(&plan);
        let mut session = access.capture(&plan);
        access
            .late_change
            .set(Some(access.reads.get() + plan.len()));
        assert!(apply_with(&mut session, &access).stopped.is_some());
        assert!(access.stores.borrow().is_empty());
        let unchanged = [CharacterPolicyEdit { bits: 0, ..plan[0] }];
        let access = Mock::new(&unchanged);
        let mut session = access.capture(&unchanged);
        let result = apply_with(&mut session, &access);
        assert!(result.stopped.is_none());
        assert!(!result.writes[0].changed);
        assert_eq!(result.pending_restore, 0);
        assert!(restore_with(&mut session, &access).writes.is_empty());
        assert!(access.stores.borrow().is_empty());
    }
}
