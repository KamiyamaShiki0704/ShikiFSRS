//! Explicit conditional ownership of independently proved character scale fields.
use crate::{
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::memory::AddressSpace;
use shikifsrs_runtime::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterScalar {
    AxisX,
    AxisY,
    AxisZ,
    CharacterHitHeight,
    CharacterHitRadius,
    MapHitHeight,
    MapHitRadius,
    /// Native signed-integer-to-float weight; finite negative baselines must be restorable.
    Weight,
    /// Raw native factors; changing them does not establish complete rendered visibility.
    BaseOpacity,
    OpacityFactor0,
    OpacityFactor1,
    OpacityFactor2,
    /// Raw initialized opacity word, copied separately from the multiplicative factors.
    /// Caller provides numerical meaning; this does not prove a complete opacity update.
    OpacityWord23c,
    /// Raw native opacity controls, separate from the multiplicative factors. Signed values allowed.
    OpacityControl244,
    BaseOpacityControl250,
    /// Selected current posture words; these stores do not run native damage logic.
    SuperArmorCurrent,
    ToughnessCurrent,
}
impl CharacterScalar {
    pub(crate) fn offset(self) -> u64 {
        use crate::character_controller::AXIS_SCALE_OFFSET;
        use crate::character_opacity::{
            BASE_OPACITY_CONTROL250_OFFSET, BASE_OPACITY_OFFSET, OPACITY_CONTROL244_OFFSET,
            OPACITY_FACTOR0_OFFSET, OPACITY_FACTOR1_OFFSET, OPACITY_FACTOR2_OFFSET,
            OPACITY_WORD23C_OFFSET,
        };
        use crate::character_physics::{
            CHARACTER_COLLISION_OFFSET, MAP_COLLISION_OFFSET, WEIGHT_OFFSET,
        };
        let (base, index) = match self {
            Self::AxisX => (AXIS_SCALE_OFFSET, 0),
            Self::AxisY => (AXIS_SCALE_OFFSET, 1),
            Self::AxisZ => (AXIS_SCALE_OFFSET, 2),
            Self::CharacterHitHeight => (CHARACTER_COLLISION_OFFSET, 0),
            Self::CharacterHitRadius => (CHARACTER_COLLISION_OFFSET, 1),
            Self::MapHitHeight => (MAP_COLLISION_OFFSET, 0),
            Self::MapHitRadius => (MAP_COLLISION_OFFSET, 1),
            Self::Weight => (WEIGHT_OFFSET, 0),
            Self::BaseOpacity => (BASE_OPACITY_OFFSET, 0),
            Self::OpacityFactor0 => (OPACITY_FACTOR0_OFFSET, 0),
            Self::OpacityFactor1 => (OPACITY_FACTOR1_OFFSET, 0),
            Self::OpacityFactor2 => (OPACITY_FACTOR2_OFFSET, 0),
            Self::OpacityWord23c => (OPACITY_WORD23C_OFFSET, 0),
            Self::OpacityControl244 => (OPACITY_CONTROL244_OFFSET, 0),
            Self::BaseOpacityControl250 => (BASE_OPACITY_CONTROL250_OFFSET, 0),
            Self::SuperArmorCurrent | Self::ToughnessCurrent => (0x10, 0),
        };
        u64::from(base) + index * std::mem::size_of::<u32>() as u64
    }
    pub(crate) fn valid(self, bits: u32) -> bool {
        let value = f32::from_bits(bits);
        value.is_finite()
            && match self {
                Self::AxisX | Self::AxisY | Self::AxisZ => value > 0.0,
                Self::Weight
                | Self::SuperArmorCurrent
                | Self::ToughnessCurrent
                | Self::OpacityWord23c
                | Self::OpacityControl244
                | Self::BaseOpacityControl250 => true,
                _ => value >= 0.0,
            }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterScalarKey {
    pub character: CharacterIdentity,
    pub field: CharacterScalar,
}

/// The character identity includes caller epoch. This does not pin any allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterScalarStorage {
    pub address: u64,
    pub kind: ObjectKind,
    /// Zero for a controller; otherwise the identified module container.
    pub container_address: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterScalarSnapshot {
    pub key: CharacterScalarKey,
    pub storage: CharacterScalarStorage,
    pub bits: u32,
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Reuse independently proved typed links and copied payload. No permission to store.
    pub fn character_scalar(
        &self,
        key: CharacterScalarKey,
    ) -> Result<Option<CharacterScalarSnapshot>> {
        use CharacterScalar as F;
        let (storage, bits) = match key.field {
            F::SuperArmorCurrent | F::ToughnessCurrent => {
                use crate::character_posture::PostureKind;
                let posture_kind = if key.field == F::SuperArmorCurrent {
                    PostureKind::SuperArmor
                } else {
                    PostureKind::Toughness
                };
                let Some(posture) = self.character_posture(key.character, posture_kind)? else {
                    return Ok(None);
                };
                let kind = match (posture_kind, posture.object_kind) {
                    (PostureKind::SuperArmor, ObjectKind::Player) => ObjectKind::PlayerSuperArmor,
                    (PostureKind::SuperArmor, ObjectKind::Enemy) => ObjectKind::EnemySuperArmor,
                    (PostureKind::SuperArmor, _) => ObjectKind::CharacterSuperArmor,
                    (PostureKind::Toughness, ObjectKind::Player) => ObjectKind::PlayerToughness,
                    (PostureKind::Toughness, ObjectKind::Enemy) => ObjectKind::EnemyToughness,
                    (PostureKind::Toughness, _) => ObjectKind::CharacterToughness,
                };
                (
                    CharacterScalarStorage {
                        address: posture.module.address,
                        kind,
                        container_address: posture.module.container_address,
                    },
                    posture.current_bits,
                )
            }
            F::BaseOpacity
            | F::OpacityFactor0
            | F::OpacityFactor1
            | F::OpacityFactor2
            | F::OpacityWord23c
            | F::OpacityControl244
            | F::BaseOpacityControl250 => {
                let Some(opacity) = self.character_opacity(key.character)? else {
                    return Ok(None);
                };
                let index = match key.field {
                    F::BaseOpacity => 0,
                    F::OpacityFactor0 => 1,
                    F::OpacityFactor1 => 2,
                    F::OpacityFactor2 => 3,
                    F::OpacityControl244 => 4,
                    F::OpacityWord23c => 6,
                    _ => 5,
                };
                (
                    CharacterScalarStorage {
                        address: opacity.character.address,
                        kind: opacity.kind,
                        container_address: 0,
                    },
                    if index == 6 {
                        opacity.control_bits[0]
                    } else if index < 4 {
                        opacity.factor_bits[index]
                    } else {
                        opacity.control_bits[index - 3]
                    },
                )
            }
            F::AxisX | F::AxisY | F::AxisZ => {
                let Some(controller) = self.character_controller(key.character)? else {
                    return Ok(None);
                };
                let index = match key.field {
                    F::AxisX => 0,
                    F::AxisY => 1,
                    _ => 2,
                };
                (
                    CharacterScalarStorage {
                        address: controller.address,
                        kind: controller.kind,
                        container_address: 0,
                    },
                    controller.axis_scale_bits[index],
                )
            }
            _ => {
                let Some(physics) = self.character_physics(key.character)? else {
                    return Ok(None);
                };
                let bits = match key.field {
                    F::CharacterHitHeight => physics.character_collision_bits[0],
                    F::CharacterHitRadius => physics.character_collision_bits[1],
                    F::MapHitHeight => physics.map_collision_bits[0],
                    F::MapHitRadius => physics.map_collision_bits[1],
                    F::Weight => physics.weight_bits,
                    _ => unreachable!(),
                };
                (
                    CharacterScalarStorage {
                        address: physics.module.address,
                        kind: ObjectKind::CharacterPhysics,
                        container_address: physics.module.container_address,
                    },
                    bits,
                )
            }
        };
        Ok(Some(CharacterScalarSnapshot { key, storage, bits }))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterReplaceOutcome {
    Applied,
    Conflict,
    Unavailable,
}

/// # Safety
/// Re-resolve the complete character and reciprocal typed storage before every store.
/// Compare identity and exact bits while caller supplies exclusive access and stable
/// live writable allocations; epochs and read rechecks are not locks or engine references.
/// Errors/Conflict/Unavailable perform no writes. Applied has no later fallible step.
pub unsafe trait CharacterScalarAccess {
    fn observe(&self, key: CharacterScalarKey) -> Result<Option<CharacterScalarSnapshot>>;
    /// # Safety
    /// Caller holds this backend's exclusive engine phase, object lifetimes and writable
    /// extent. No concurrent update, callback, resource swap, unload, or Rust alias overlaps.
    unsafe fn compare_replace(
        &self,
        expected: CharacterScalarSnapshot,
        bits: u32,
    ) -> Result<CharacterReplaceOutcome>;
}

#[derive(Debug, Clone, Copy)]
struct Override {
    original: CharacterScalarSnapshot,
    applied: u32,
}
/// No Drop writes. The consumer must explicitly restore before its stable access ends.
#[derive(Debug, Default)]
pub struct CharacterScalarOverlays {
    entries: Vec<Override>,
}
#[derive(Debug, Default)]
pub struct CharacterScalarRestoreReport {
    pub restored: Vec<CharacterScalarKey>,
    pub superseded: Vec<CharacterScalarKey>,
    pub unavailable: Vec<CharacterScalarKey>,
    pub failures: Vec<(CharacterScalarKey, Error)>,
    pub pending: usize,
}

impl CharacterScalarOverlays {
    pub fn pending(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn expected(&self, key: CharacterScalarKey) -> Option<CharacterScalarSnapshot> {
        self.entries
            .iter()
            .find(|entry| entry.original.key == key)
            .map(|entry| CharacterScalarSnapshot {
                bits: entry.applied,
                ..entry.original
            })
    }

    /// Store one finite scalar, remembering the first original across updates.
    /// Multiple stores are sequential; this does not rebuild collision objects.
    /// # Safety
    /// Caller satisfies CharacterScalarAccess for all reads and the conditional write.
    pub unsafe fn apply<A: CharacterScalarAccess>(
        &mut self,
        access: &A,
        key: CharacterScalarKey,
        value: f32,
    ) -> Result<CharacterReplaceOutcome> {
        if key.character.epoch == 0 || !key.field.valid(value.to_bits()) {
            return Err(Error::InvalidProfile("invalid character scalar override"));
        }
        // SAFETY: Forwarded caller contract.
        unsafe { self.apply_bits(access, key, value.to_bits(), None) }
    }

    pub(crate) unsafe fn apply_bits<A: CharacterScalarAccess>(
        &mut self,
        access: &A,
        key: CharacterScalarKey,
        bits: u32,
        baseline: Option<CharacterScalarSnapshot>,
    ) -> Result<CharacterReplaceOutcome> {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.original.key == key) {
            let expected = CharacterScalarSnapshot {
                bits: entry.applied,
                ..entry.original
            };
            // SAFETY: Forwarded caller contract, retaining original storage and identity.
            let outcome = unsafe { access.compare_replace(expected, bits)? };
            if outcome == CharacterReplaceOutcome::Applied {
                entry.applied = bits;
            }
            return Ok(outcome);
        }
        if self.entries.len() == 64 {
            return Err(Error::InvalidProfile(
                "character scalar override limit exceeded",
            ));
        }
        let Some(original) = access.observe(key)? else {
            return Ok(CharacterReplaceOutcome::Unavailable);
        };
        if original.key != key || !key.field.valid(original.bits) {
            return Err(Error::InvalidProfile(
                "character backend returned an invalid original",
            ));
        }
        if baseline.is_some_and(|expected| expected != original) {
            return Err(Error::InvalidProfile(
                "character multiplier baseline changed",
            ));
        }
        // SAFETY: Caller supplies exclusive lifetime/phase; compare occurs in backend.
        let outcome = unsafe { access.compare_replace(original, bits)? };
        if outcome == CharacterReplaceOutcome::Applied && original.bits != bits {
            self.entries.push(Override {
                original,
                applied: bits,
            });
        }
        Ok(outcome)
    }

    /// Compute from the first original, never from the last scaled value.
    /// Zero physics dimensions/weight are preserved. Each field's multiplier is explicit;
    /// this API does not choose a mass law or claim uniform full-body scaling.
    /// # Safety
    /// Caller satisfies CharacterScalarAccess throughout the operation.
    pub unsafe fn multiply<A: CharacterScalarAccess>(
        &mut self,
        access: &A,
        key: CharacterScalarKey,
        multiplier: f32,
    ) -> Result<CharacterReplaceOutcome> {
        if !multiplier.is_finite() || multiplier <= 0.0 || key.character.epoch == 0 {
            return Err(Error::InvalidProfile("invalid character scalar multiplier"));
        }
        let original = match self.entries.iter().find(|e| e.original.key == key) {
            Some(entry) => entry.original,
            None => match access.observe(key)? {
                Some(original) => original,
                None => return Ok(CharacterReplaceOutcome::Unavailable),
            },
        };
        if original.key != key || !key.field.valid(original.bits) {
            return Err(Error::InvalidProfile(
                "character backend returned an invalid baseline",
            ));
        }
        let value = f32::from_bits(original.bits) * multiplier;
        if !key.field.valid(value.to_bits()) || (original.bits & 0x7fff_ffff != 0 && value == 0.0) {
            return Err(Error::InvalidProfile(
                "unrepresentable character scalar multiplier",
            ));
        }
        // SAFETY: Forwarded exclusive access contract. First use rechecks the same baseline.
        unsafe { self.apply_bits(access, key, value.to_bits(), Some(original)) }
    }

    /// Restore reverse order. Foreign writes/replaced storage are released without writes;
    /// errors stay owned for retry. No zero-reference or pointer-only free assumption.
    /// # Safety
    /// Caller satisfies CharacterScalarAccess for the complete cleanup.
    pub unsafe fn restore<A: CharacterScalarAccess>(
        &mut self,
        access: &A,
    ) -> CharacterScalarRestoreReport {
        let mut report = CharacterScalarRestoreReport::default();
        for index in (0..self.entries.len()).rev() {
            let entry = self.entries[index];
            let expected = CharacterScalarSnapshot {
                bits: entry.applied,
                ..entry.original
            };
            // SAFETY: Forwarded exclusive lifetime/phase; backend freshly resolves storage.
            match unsafe { access.compare_replace(expected, entry.original.bits) } {
                Ok(CharacterReplaceOutcome::Applied) => report.restored.push(expected.key),
                Ok(CharacterReplaceOutcome::Conflict) => report.superseded.push(expected.key),
                Ok(CharacterReplaceOutcome::Unavailable) => report.unavailable.push(expected.key),
                Err(error) => {
                    report.failures.push((expected.key, error));
                    continue;
                }
            }
            self.entries.remove(index);
        }
        report.pending = self.entries.len();
        report
    }
}

#[cfg(test)]
mod tests;
