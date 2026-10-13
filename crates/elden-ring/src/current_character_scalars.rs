//! Experimental stores of proved fields; caller supplies lifetime and exclusive engine phase.
use crate::{
    WW_171, Ww171File,
    character_opacity::{
        CharacterOpacityObservation, ManualOpacityWrite, commit_manual_opacity, manual_opacity_plan,
    },
    character_scalars::{
        CharacterReplaceOutcome, CharacterScalarAccess, CharacterScalarKey, CharacterScalarSnapshot,
    },
};
use shikifsrs_runtime::code_patches::OwnedCodePatches;
use shikifsrs_runtime::{Error, Result, memory::CurrentProcess, profile::OwnedVerifiedFile};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

/// Thread-confined session. Does not acquire a lock or engine reference guard.
pub struct CurrentCharacterScalarAccess {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentCharacterScalarAccess {
    pub fn observe_opacity(
        &self,
        target: crate::character_sets::CharacterIdentity,
    ) -> Result<Option<CharacterOpacityObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .character_opacity(target)
    }
    /// Sequentially replace only control244/control250/factor240/base24c, preserving
    /// the engine's word23c, animation factor238 and camouflage factor248.
    /// # Safety
    /// Bind's full lifetime/exclusive phase/extent contract holds for all seven words.
    /// Caller chooses independently justified control values; these are not four alpha
    /// multipliers. The stores are not atomic or a complete hide/reactivation operation.
    /// Failed postflight returns every actual store; reconcile before later steps.
    pub unsafe fn compare_manual_opacity(
        &self,
        expected: CharacterOpacityObservation,
        control244: f32,
        control250: f32,
        factor240: f32,
        base24c: f32,
    ) -> Result<ManualOpacityWrite> {
        let plan = manual_opacity_plan(
            self.observe_opacity(expected.character)?,
            expected,
            [control244, control250, factor240, base24c].map(f32::to_bits),
        )?;
        Ok(commit_manual_opacity(
            plan,
            |address, bits| {
                // SAFETY: Caller provides the exact exclusive pinned initialized u32 destinations.
                unsafe { store(address, bits) }
            },
            || {
                self.observe_opacity(expected.character)?
                    .ok_or(Error::InvalidProfile(
                        "character unavailable during manual opacity store",
                    ))
            },
        ))
    }
    /// Validate file and loaded native evidence; no field write at bind.
    /// # Safety
    /// Caller identifies ER and proves live writable extent of every used field, holds
    /// an exclusive permitted engine phase and stable character/controller/module allocations.
    /// No concurrent engine writes, callbacks, teardown, replacement, unload, patches,
    /// Rust aliases or address reuse overlap. Copied identities/epochs are not pinning.
    /// Field stores do not rebuild collision shapes or prove full-body scaling behavior.
    pub unsafe fn bind(bytes: Arc<[u8]>, base: u64, epoch: u64) -> Result<Self> {
        // SAFETY: Same caller obligations; no code replacements accepted by default.
        unsafe { Self::bind_with_patches(bytes, base, epoch, None) }
    }

    /// Bind with an explicitly owned code replacement manifest.
    /// # Safety
    /// All bind obligations apply, plus the manifest owner's ABI, semantic, lifetime and
    /// quiescence guarantees for every registered hook throughout all operations.
    pub unsafe fn bind_with_patches(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero character scalar epoch"));
        }
        let file = OwnedVerifiedFile::verify(bytes, WW_171)?;
        Ww171File::from_verified(file.borrow()?)?.bind_objects_with_patches(
            &CurrentProcess,
            base,
            patches.as_deref(),
        )?;
        Ok(Self {
            file,
            patches,
            base,
            epoch,
            _thread: PhantomData,
        })
    }

    /// Revoke old keys after a known scene/lifetime transition, before resuming writes.
    /// # Safety
    /// Caller proves the new epoch has the same exclusive access and independently stable
    /// objects as bind. Advancing the epoch does not release engine references.
    pub unsafe fn advance_epoch(&mut self, epoch: u64) -> Result<()> {
        if epoch <= self.epoch {
            return Err(Error::InvalidProfile("character scalar epoch must advance"));
        }
        self.epoch = epoch;
        Ok(())
    }
}

// SAFETY: Fresh typed reciprocal links and full identities are resolved for each store.
// bind caller supplies the exclusivity/lifetime/writable extent absent from observations.
unsafe impl CharacterScalarAccess for CurrentCharacterScalarAccess {
    fn observe(&self, key: CharacterScalarKey) -> Result<Option<CharacterScalarSnapshot>> {
        if key.character.epoch != self.epoch {
            return Ok(None);
        }
        let view = self.file.borrow()?;
        let file = Ww171File::from_verified(view)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        if objects.character_identity(key.character.address, self.epoch)? != Some(key.character) {
            return Ok(None);
        }
        objects.character_scalar(key)
    }
    unsafe fn compare_replace(
        &self,
        expected: CharacterScalarSnapshot,
        bits: u32,
    ) -> Result<CharacterReplaceOutcome> {
        let current = self.observe(expected.key)?;
        let address = match replacement_destination(current, expected, bits)? {
            Replacement::Store(address) => address,
            Replacement::Stop(outcome) => return Ok(outcome),
        };
        // SAFETY: Session caller supplies pinned writable aligned extent and exclusive access.
        // Exactly one scalar store occurs; no fallible step follows it.
        unsafe {
            store(address, bits);
        }
        Ok(CharacterReplaceOutcome::Applied)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Replacement {
    Stop(CharacterReplaceOutcome),
    Store(usize),
}
fn replacement_destination(
    current: Option<CharacterScalarSnapshot>,
    expected: CharacterScalarSnapshot,
    bits: u32,
) -> Result<Replacement> {
    if !expected.key.field.valid(bits) {
        return Err(Error::InvalidProfile("invalid character scalar bits"));
    }
    let Some(current) = current else {
        return Ok(Replacement::Stop(CharacterReplaceOutcome::Unavailable));
    };
    if current.key != expected.key {
        return Ok(Replacement::Stop(CharacterReplaceOutcome::Unavailable));
    }
    if current.storage != expected.storage {
        return Ok(Replacement::Stop(CharacterReplaceOutcome::Unavailable));
    }
    if current.bits != expected.bits {
        return Ok(Replacement::Stop(CharacterReplaceOutcome::Conflict));
    }
    let address = current
        .storage
        .address
        .checked_add(current.key.field.offset())
        .ok_or(Error::AddressOverflow)?;
    let address = usize::try_from(address).map_err(|_| Error::AddressOverflow)?;
    if address == 0 || !address.is_multiple_of(std::mem::align_of::<u32>()) {
        return Err(Error::InvalidProfile("unaligned character scalar store"));
    }
    Ok(Replacement::Store(address))
}

unsafe fn store(address: usize, bits: u32) {
    // SAFETY: Caller supplies a live writable aligned four-byte destination without aliases.
    unsafe {
        (address as *mut u32).write(bits);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        character_scalars::{CharacterScalar, CharacterScalarStorage},
        character_sets::CharacterIdentity,
        objects::ObjectKind,
    };

    fn snapshot(field: CharacterScalar) -> CharacterScalarSnapshot {
        CharacterScalarSnapshot {
            key: CharacterScalarKey {
                field,
                character: CharacterIdentity {
                    address: 0x1000,
                    world_address: 0x2000,
                    native_identifier: 4,
                    epoch: 3,
                },
            },
            storage: CharacterScalarStorage {
                address: 0x3000,
                kind: ObjectKind::PlayerController,
                container_address: 0,
            },
            bits: 1f32.to_bits(),
        }
    }
    #[test]
    fn store_plan_rejects_identity_storage_bit_changes_and_invalid_destinations_before_writes() {
        let expected = snapshot(CharacterScalar::AxisX);
        assert_eq!(
            replacement_destination(None, expected, 2f32.to_bits()).unwrap(),
            Replacement::Stop(CharacterReplaceOutcome::Unavailable)
        );
        for change in 0..7 {
            let mut current = expected;
            match change {
                0 => current.key.character.native_identifier += 1,
                1 => current.key.character.world_address += 8,
                2 => current.key.character.epoch += 1,
                3 => current.storage.address += 8,
                4 => current.storage.container_address += 8,
                5 => current.storage.kind = ObjectKind::EnemyController,
                6 => current.bits = 3f32.to_bits(),
                _ => unreachable!(),
            }
            assert_eq!(
                replacement_destination(Some(current), expected, 2f32.to_bits()).unwrap(),
                Replacement::Stop(if change == 6 {
                    CharacterReplaceOutcome::Conflict
                } else {
                    CharacterReplaceOutcome::Unavailable
                })
            );
        }
        for address in [1, u64::MAX - 3] {
            let mut current = expected;
            current.storage.address = address;
            assert!(replacement_destination(Some(current), current, 2f32.to_bits()).is_err());
        }
        for bits in [
            0f32.to_bits(),
            (-1f32).to_bits(),
            f32::NAN.to_bits(),
            f32::INFINITY.to_bits(),
        ] {
            assert!(replacement_destination(Some(expected), expected, bits).is_err());
        }
    }
    #[test]
    fn store_plan_uses_each_proved_offset_without_accessing_initial_weight() {
        for (field, offset) in [
            (CharacterScalar::AxisX, 0x2d4),
            (CharacterScalar::AxisY, 0x2d8),
            (CharacterScalar::AxisZ, 0x2dc),
            (CharacterScalar::CharacterHitHeight, 0x330),
            (CharacterScalar::CharacterHitRadius, 0x334),
            (CharacterScalar::MapHitHeight, 0x340),
            (CharacterScalar::MapHitRadius, 0x344),
            (CharacterScalar::Weight, 0xc8),
            (CharacterScalar::BaseOpacity, 0x24c),
            (CharacterScalar::OpacityFactor0, 0x238),
            (CharacterScalar::OpacityFactor1, 0x240),
            (CharacterScalar::OpacityFactor2, 0x248),
            (CharacterScalar::OpacityWord23c, 0x23c),
            (CharacterScalar::OpacityControl244, 0x244),
            (CharacterScalar::BaseOpacityControl250, 0x250),
            (CharacterScalar::SuperArmorCurrent, 0x10),
            (CharacterScalar::ToughnessCurrent, 0x10),
        ] {
            let copy = snapshot(field);
            assert_eq!(
                replacement_destination(Some(copy), copy, 2f32.to_bits()).unwrap(),
                Replacement::Store(0x3000 + offset)
            );
        }
    }
    #[test]
    fn native_scalar_store_changes_exactly_one_original_owned_word() {
        let mut original = [0x1122_3344u32, 1f32.to_bits(), 0x5566_7788];
        let address = original.as_mut_ptr().wrapping_add(1) as usize;
        // SAFETY: Only an aligned original test-owned array, never a game allocation.
        unsafe {
            super::store(address, 3f32.to_bits());
        }
        assert_eq!(original, [0x1122_3344, 3f32.to_bits(), 0x5566_7788]);
    }
}
