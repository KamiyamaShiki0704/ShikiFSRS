//! Copied physics spatial vectors and the contract for explicit native relative translation.
use crate::{
    character_control_channels::CharacterControlChannels,
    character_modules::{CharacterModule, CharacterModuleObservation},
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const SPATIAL_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x39b0c0,
        length: 0x1f9,
    },
    CodeRange {
        rva: 0x45dc60,
        length: 0xcb,
    },
    CodeRange {
        rva: 0x45e8f0,
        length: 0xb,
    },
    CodeRange {
        rva: 0x460750,
        length: 0x1e4,
    },
    CodeRange {
        rva: 0x45fee0,
        length: 0x2e,
    },
    CodeRange {
        rva: 0x45fd40,
        length: 0xa5,
    },
    CodeRange {
        rva: 0x461f60,
        length: 0x332,
    },
    CodeRange {
        rva: 0x463fa0,
        length: 0x6c9,
    },
    CodeRange {
        rva: 0x45fc80,
        length: 0x77,
    },
    CodeRange {
        rva: 0x3c7bf0,
        length: 0x117,
    },
    CodeRange {
        rva: 0x3f72b0,
        length: 0x58,
    },
    CodeRange {
        rva: 0x3c8290,
        length: 0x14,
    },
    CodeRange {
        rva: 0x45fe70,
        length: 0x22,
    },
    CodeRange {
        rva: 0x3c7ee0,
        length: 0x14,
    },
    CodeRange {
        rva: 0x45fd00,
        length: 0x39,
    },
    CodeRange {
        rva: 0x3f7a20,
        length: 0x259,
    },
    CodeRange {
        rva: 0x4d0ea0,
        length: 0x30,
    },
    CodeRange {
        rva: 0x65a680,
        length: 0x8d,
    },
    CodeRange {
        rva: 0x3c0060,
        length: 0x8d,
    },
    CodeRange {
        rva: 0x3c8450,
        length: 0xc2,
    },
];
pub(crate) const PRIMARY_ROTATION_OFFSET: u32 = 0x50;
const SECONDARY_ROTATION_OFFSET: u32 = 0x60;
pub(crate) const POSITION_OFFSET: u32 = 0x70;
const SECONDARY_POSITION_OFFSET: u32 = 0x80;
const CHARACTER_POSITION80_OFFSET: u32 = 0x80;
const CHARACTER_POSITION90_OFFSET: u32 = 0x90;
const CHARACTER_ANGLEA0_OFFSET: u32 = 0xa0;
const CHARACTER_POSITIOND0_OFFSET: u32 = 0xd0;
const CHARACTER_SCALAR350_OFFSET: u32 = 0x350;
const CHARACTER_POSITION_STATE_OFFSET: u32 = 0x18;
const POSITION_STATE_VECTOR_OFFSET: u32 = 0x40;
const CHARACTER_SCALAR2E8_OFFSET: u32 = 0x2e8;
const CONTROLLER_POSITION100_OFFSET: u32 = 0x100;
const CONTROLLER_POSITION260_OFFSET: u32 = 0x260;
pub(crate) const CHARACTER_TRANSLATION_SLOT: u32 = 0x1a0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterSpatialObservation {
    pub module: CharacterModuleObservation,
    /// Native getter builds a quaternion rotation matrix from these four components.
    pub primary_rotation_bits: [u32; 4],
    /// Separate native rotation cache. Not interpreted as Euler angles.
    pub secondary_rotation_bits: [u32; 4],
    pub position_bits: [u32; 4],
    /// Separately translated native position cache; no previous-frame semantics assumed.
    pub secondary_position_bits: [u32; 4],
}
impl CharacterSpatialObservation {
    /// Selected xyz only. Preserve the fourth lane even if nonfinite or noncanonical.
    pub fn finite_position(&self) -> Result<[f32; 3]> {
        let xyz = std::array::from_fn(|i| f32::from_bits(self.position_bits[i]));
        if xyz.into_iter().any(|v| !v.is_finite()) {
            return Err(Error::InvalidProfile("nonfinite character position"));
        }
        Ok(xyz)
    }
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Only independently initialized fields are copied. No getter is called or object pinned.
    pub fn character_spatial(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterSpatialObservation>> {
        let Some(module) = self.character_module(target, CharacterModule::Physics)? else {
            return Ok(None);
        };
        let read = || -> Result<[[u32; 4]; 4]> {
            let mut fields = [[0; 4]; 4];
            for (offset, field) in [
                PRIMARY_ROTATION_OFFSET,
                SECONDARY_ROTATION_OFFSET,
                POSITION_OFFSET,
                SECONDARY_POSITION_OFFSET,
            ]
            .into_iter()
            .zip(&mut fields)
            {
                let address = module
                    .address
                    .checked_add(u64::from(offset))
                    .ok_or(Error::AddressOverflow)?;
                let mut bytes = [0; 16];
                self.module.memory().read_exact(address, &mut bytes)?;
                *field = std::array::from_fn(|i| {
                    u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                });
            }
            Ok(fields)
        };
        let [
            primary_rotation_bits,
            secondary_rotation_bits,
            position_bits,
            secondary_position_bits,
        ] = read()?;
        if read()?
            != [
                primary_rotation_bits,
                secondary_rotation_bits,
                position_bits,
                secondary_position_bits,
            ]
            || self.character_module(target, CharacterModule::Physics)? != Some(module)
        {
            return Err(Error::InvalidProfile(
                "character spatial fields changed during observation",
            ));
        }
        Ok(Some(CharacterSpatialObservation {
            module,
            primary_rotation_bits,
            secondary_rotation_bits,
            position_bits,
            secondary_position_bits,
        }))
    }
}

#[derive(Debug)]
pub struct NativeTranslationCall {
    pub source: CharacterSpatialObservation,
    /// SDK-owned four-lane displacement, with an explicit zero fourth lane.
    pub delta_bits: [u32; 4],
    /// The native call occurred even on failure. Never automatically retry or reverse it.
    pub postflight: Result<CharacterSpatialObservation>,
}
pub(crate) fn translation_plan(
    current: Option<CharacterSpatialObservation>,
    expected: CharacterSpatialObservation,
    epoch: u64,
    displacement: [f32; 3],
) -> Result<([u32; 4], CharacterSpatialObservation)> {
    spatial_preflight(current, expected, epoch)?;
    if displacement.into_iter().any(|v| !v.is_finite()) {
        return Err(Error::InvalidProfile("nonfinite native translation delta"));
    }
    let delta = [displacement[0], displacement[1], displacement[2], 0.0];
    let add = |bits: [u32; 4]| -> Result<[u32; 4]> {
        let before = bits.map(f32::from_bits);
        let after: [f32; 4] = std::array::from_fn(|i| before[i] + delta[i]);
        if before.into_iter().chain(after).any(|v| !v.is_finite()) {
            return Err(Error::InvalidProfile(
                "nonfinite native translation position",
            ));
        }
        Ok(after.map(f32::to_bits))
    };
    let after = CharacterSpatialObservation {
        position_bits: add(expected.position_bits)?,
        secondary_position_bits: add(expected.secondary_position_bits)?,
        ..expected
    };
    Ok((delta.map(f32::to_bits), after))
}
pub(crate) fn translation_postflight(
    planned: CharacterSpatialObservation,
    after: CharacterSpatialObservation,
) -> Result<CharacterSpatialObservation> {
    if after != planned {
        return Err(Error::InvalidProfile(
            "native translation postflight changed",
        ));
    }
    Ok(after)
}

fn spatial_preflight(
    current: Option<CharacterSpatialObservation>,
    expected: CharacterSpatialObservation,
    epoch: u64,
) -> Result<()> {
    if epoch == 0
        || expected.module.character.epoch != epoch
        || expected.module.module != CharacterModule::Physics
        || expected.module.address == 0
        || !expected.module.address.is_multiple_of(16)
        || expected.module.container_address == 0
        || !expected.module.container_address.is_multiple_of(8)
    {
        return Err(Error::InvalidProfile("invalid native spatial source"));
    }
    expected
        .module
        .address
        .checked_add(u64::from(SECONDARY_POSITION_OFFSET) + 16)
        .ok_or(Error::AddressOverflow)?;
    if current != Some(expected) {
        return Err(Error::NativeOperationRejected(
            "native spatial source superseded",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationOperation {
    /// Four raw primary quaternion bits. Caller independently guarantees valid unit rotation
    /// and native conventions. SDK rejects nonfinite/all-zero, but does not normalize it.
    PrimaryBits([u32; 4]),
    /// Three native angle inputs. Their basis/units follow the pinned native numerical globals;
    /// no gameplay yaw or inverse-of-getter claim. Native constraints may adjust the rotation.
    NativeAngleBits([u32; 3]),
}
#[derive(Debug)]
pub struct NativeRotationCall {
    pub source: CharacterSpatialObservation,
    pub operation: RotationOperation,
    /// Native attempt persists on failure. No retry, implicit inverse or Drop restoration.
    pub postflight: Result<CharacterSpatialObservation>,
}
pub(crate) fn rotation_plan(
    current: Option<CharacterSpatialObservation>,
    expected: CharacterSpatialObservation,
    epoch: u64,
    operation: RotationOperation,
) -> Result<[u32; 4]> {
    spatial_preflight(current, expected, epoch)?;
    let bits = match operation {
        RotationOperation::PrimaryBits(bits) => {
            if bits.map(f32::from_bits).into_iter().all(|v| v == 0.0) {
                return Err(Error::InvalidProfile("zero native primary quaternion"));
            }
            bits
        }
        RotationOperation::NativeAngleBits(bits) => [bits[0], bits[1], bits[2], 0],
    };
    if bits.map(f32::from_bits).into_iter().any(|v| !v.is_finite()) {
        return Err(Error::InvalidProfile("nonfinite native rotation input"));
    }
    Ok(bits)
}
pub(crate) fn rotation_postflight(
    source: CharacterSpatialObservation,
    operation: RotationOperation,
    after: CharacterSpatialObservation,
) -> Result<CharacterSpatialObservation> {
    spatial_postflight(
        source,
        match operation {
            RotationOperation::PrimaryBits(bits) => Some(bits),
            RotationOperation::NativeAngleBits(_) => None,
        },
        after,
    )
}
fn spatial_postflight(
    source: CharacterSpatialObservation,
    primary: Option<[u32; 4]>,
    after: CharacterSpatialObservation,
) -> Result<CharacterSpatialObservation> {
    if after.module != source.module
        || after.position_bits != source.position_bits
        || after.secondary_position_bits != source.secondary_position_bits
        || after
            .primary_rotation_bits
            .into_iter()
            .chain(after.secondary_rotation_bits)
            .any(|v| !f32::from_bits(v).is_finite())
        || [after.primary_rotation_bits, after.secondary_rotation_bits]
            .into_iter()
            .any(|bits| bits.map(f32::from_bits).into_iter().all(|v| v == 0.0))
        || primary.is_some_and(|bits| after.primary_rotation_bits != bits)
    {
        return Err(Error::InvalidProfile("native rotation postflight changed"));
    }
    Ok(after)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativePoseInput {
    /// Four native position lanes, including an independently valid caller-selected fourth lane.
    pub position_bits: [u32; 4],
    /// Three inputs to native461f60. Native axes/conventions require caller establishment.
    pub angle_bits: [u32; 3],
}
#[derive(Debug)]
pub struct NativePoseCall {
    pub source: CharacterSpatialObservation,
    pub input: NativePoseInput,
    /// One explicit native attempt; failure does not retry, reverse or erase it.
    pub postflight: Result<CharacterSpatialObservation>,
}
pub(crate) fn pose_plan(
    current: Option<CharacterSpatialObservation>,
    expected: CharacterSpatialObservation,
    epoch: u64,
    input: NativePoseInput,
) -> Result<[u32; 4]> {
    let angles = rotation_plan(
        current,
        expected,
        epoch,
        RotationOperation::NativeAngleBits(input.angle_bits),
    )?;
    if input
        .position_bits
        .map(f32::from_bits)
        .into_iter()
        .any(|v| !v.is_finite())
    {
        return Err(Error::InvalidProfile("nonfinite native pose position"));
    }
    Ok(angles)
}
pub(crate) fn pose_postflight(
    source: CharacterSpatialObservation,
    input: NativePoseInput,
    after: CharacterSpatialObservation,
) -> Result<CharacterSpatialObservation> {
    let planned = CharacterSpatialObservation {
        position_bits: input.position_bits,
        secondary_position_bits: input.position_bits,
        ..source
    };
    spatial_postflight(planned, None, after)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterMotionObservation {
    pub spatial: CharacterSpatialObservation,
    pub controls: CharacterControlChannels,
}
#[derive(Debug)]
pub struct NativeControllerSyncCall {
    pub source: CharacterMotionObservation,
    /// Forwarded native byte, not assigned a guessed boolean/teleport gameplay meaning.
    pub raw_mode: u8,
    /// Native execution survives failed observation; never automatically retried or reversed.
    pub postflight: Result<CharacterMotionObservation>,
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Observe both full storage graphs and recheck the combined payload. No native dispatch,
    /// lifetime pin, synchronization barrier or complete world relocation is acquired.
    pub fn character_motion(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterMotionObservation>> {
        let Some(spatial) = self.character_spatial(target)? else {
            return Ok(None);
        };
        let Some(controls) = self.character_control_channels(target)? else {
            return Ok(None);
        };
        if self.character_spatial(target)? != Some(spatial)
            || self.character_control_channels(target)? != Some(controls)
        {
            return Err(Error::InvalidProfile(
                "character motion changed during observation",
            ));
        }
        Ok(Some(CharacterMotionObservation { spatial, controls }))
    }
}
pub(crate) fn controller_sync_plan(
    current: Option<CharacterMotionObservation>,
    expected: CharacterMotionObservation,
    epoch: u64,
) -> Result<()> {
    spatial_preflight(current.map(|v| v.spatial), expected.spatial, epoch)?;
    let controller = expected.controls.controller;
    if controller.character != expected.spatial.module.character
        || controller.address == 0
        || !controller.address.is_multiple_of(8)
        || !matches!(
            controller.kind,
            ObjectKind::CharacterController
                | ObjectKind::EnemyController
                | ObjectKind::PlayerController
        )
        || current != Some(expected)
    {
        return Err(Error::NativeOperationRejected(
            "native controller synchronization source superseded",
        ));
    }
    controller
        .address
        .checked_add(0x188)
        .ok_or(Error::AddressOverflow)?;
    if expected
        .spatial
        .position_bits
        .into_iter()
        .chain(expected.spatial.primary_rotation_bits)
        .any(|bits| !f32::from_bits(bits).is_finite())
        || expected
            .spatial
            .primary_rotation_bits
            .map(f32::from_bits)
            .into_iter()
            .all(|v| v == 0.0)
    {
        return Err(Error::InvalidProfile(
            "invalid native controller synchronization transform",
        ));
    }
    Ok(())
}
pub(crate) fn controller_sync_postflight(
    source: CharacterMotionObservation,
    after: CharacterMotionObservation,
) -> Result<CharacterMotionObservation> {
    if after.controls != source.controls {
        return Err(Error::InvalidProfile(
            "native controller synchronization identity changed",
        ));
    }
    let planned = CharacterSpatialObservation {
        secondary_position_bits: source.spatial.position_bits,
        ..source.spatial
    };
    spatial_postflight(planned, None, after.spatial)?;
    Ok(after)
}

/// Copied owner caches, independently produced by the constructor and placement path.
/// Offset names deliberately do not claim initial/current/chunk or world-registration semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterPlacementObservation {
    pub motion: CharacterMotionObservation,
    pub position80_bits: [u32; 4],
    pub position90_bits: [u32; 4],
    pub anglea0_bits: [u32; 4],
    pub positiond0_bits: [u32; 4],
    pub scalar350_bits: u32,
}
#[derive(Debug)]
pub struct NativeCharacterPlacementCall {
    pub source: CharacterPlacementObservation,
    pub input: NativePoseInput,
    /// A failed observation does not erase the native attempt or permit an automatic retry.
    pub postflight: Result<CharacterPlacementObservation>,
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy raw owner fields and recheck the entire controller/physics/owner graph.
    /// This acquires no engine pin, writable phase or world-registration barrier.
    pub fn character_placement(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterPlacementObservation>> {
        let Some(motion) = self.character_motion(target)? else {
            return Ok(None);
        };
        let read = || -> Result<([[u32; 4]; 4], u32)> {
            let mut fields = [[0; 4]; 4];
            for (offset, field) in [
                CHARACTER_POSITION80_OFFSET,
                CHARACTER_POSITION90_OFFSET,
                CHARACTER_ANGLEA0_OFFSET,
                CHARACTER_POSITIOND0_OFFSET,
            ]
            .into_iter()
            .zip(&mut fields)
            {
                let address = target
                    .address
                    .checked_add(u64::from(offset))
                    .ok_or(Error::AddressOverflow)?;
                let mut bytes = [0; 16];
                self.module.memory().read_exact(address, &mut bytes)?;
                *field = std::array::from_fn(|i| {
                    u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                });
            }
            let scalar = self.module.memory().read_u32(
                target
                    .address
                    .checked_add(u64::from(CHARACTER_SCALAR350_OFFSET))
                    .ok_or(Error::AddressOverflow)?,
            )?;
            Ok((fields, scalar))
        };
        let (fields, scalar350_bits) = read()?;
        if read()? != (fields, scalar350_bits) || self.character_motion(target)? != Some(motion) {
            return Err(Error::InvalidProfile(
                "character placement changed during observation",
            ));
        }
        let [
            position80_bits,
            position90_bits,
            anglea0_bits,
            positiond0_bits,
        ] = fields;
        Ok(Some(CharacterPlacementObservation {
            motion,
            position80_bits,
            position90_bits,
            anglea0_bits,
            positiond0_bits,
            scalar350_bits,
        }))
    }
}
pub(crate) fn placement_plan(
    current: Option<CharacterPlacementObservation>,
    expected: CharacterPlacementObservation,
    epoch: u64,
    input: NativePoseInput,
) -> Result<[u32; 4]> {
    placement_source_preflight(current, expected, epoch)?;
    pose_plan(
        current.map(|v| v.motion.spatial),
        expected.motion.spatial,
        epoch,
        input,
    )
}
fn placement_source_preflight(
    current: Option<CharacterPlacementObservation>,
    expected: CharacterPlacementObservation,
    epoch: u64,
) -> Result<()> {
    controller_sync_plan(current.map(|v| v.motion), expected.motion, epoch)?;
    let address = expected.motion.spatial.module.character.address;
    if address == 0 || !address.is_multiple_of(16) || current != Some(expected) {
        return Err(Error::NativeOperationRejected(
            "native character placement source superseded",
        ));
    }
    address
        .checked_add(u64::from(CHARACTER_SCALAR350_OFFSET) + 4)
        .ok_or(Error::AddressOverflow)?;
    Ok(())
}
pub(crate) fn placement_postflight(
    source: CharacterPlacementObservation,
    input: NativePoseInput,
    after: CharacterPlacementObservation,
) -> Result<CharacterPlacementObservation> {
    if after.motion.controls != source.motion.controls
        || after.position80_bits != input.position_bits
        || after.position90_bits != input.position_bits
        || after.anglea0_bits
            != [
                input.angle_bits[0],
                input.angle_bits[1],
                input.angle_bits[2],
                0,
            ]
        || after.positiond0_bits != source.positiond0_bits
        || after.scalar350_bits != 1.0f32.to_bits()
    {
        return Err(Error::InvalidProfile(
            "native character placement postflight changed",
        ));
    }
    pose_postflight(source.motion.spatial, input, after.motion.spatial)?;
    Ok(after)
}

/// Raw records on the complete typed character translation path. The linked +18 storage
/// is a constructor-owned nonpolymorphic allocation; no chunk/world identity is inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterTranslationObservation {
    pub placement: CharacterPlacementObservation,
    pub kind: ObjectKind,
    pub position_state_address: u64,
    pub position_state_bits: [u32; 4],
    pub controller_position100_bits: [u32; 4],
    pub controller_position260_bits: [u32; 4],
    pub scalar2e8_bits: u32,
}
#[derive(Debug)]
pub struct NativeCharacterTranslationCall {
    pub source: CharacterTranslationObservation,
    pub delta_bits: [u32; 4],
    /// Exactly one native attempt, including when a copied postcondition fails. No retry/Drop.
    pub postflight: Result<CharacterTranslationObservation>,
}
pub(crate) fn character_translation_entry(kind: ObjectKind) -> Result<CodeRange> {
    match kind {
        ObjectKind::Character => Ok(SPATIAL_EVIDENCE[15]),
        ObjectKind::Enemy => Ok(SPATIAL_EVIDENCE[16]),
        ObjectKind::Player => Ok(SPATIAL_EVIDENCE[17]),
        _ => Err(Error::InvalidProfile(
            "unsupported native translation character kind",
        )),
    }
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    pub fn character_translation(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterTranslationObservation>> {
        let Some(placement) = self.character_placement(target)? else {
            return Ok(None);
        };
        let Some(character) = self.character(target.address)? else {
            return Ok(None);
        };
        character_translation_entry(character.kind)?;
        let memory = self.module.memory();
        let link = target
            .address
            .checked_add(u64::from(CHARACTER_POSITION_STATE_OFFSET))
            .ok_or(Error::AddressOverflow)?;
        let state = memory.read_u64(link)?;
        if state == 0 {
            return Ok(None);
        }
        if !state.is_multiple_of(16) {
            return Err(Error::InvalidProfile("unaligned character position state"));
        }
        state.checked_add(0x90).ok_or(Error::AddressOverflow)?;
        let controller = placement.motion.controls.controller.address;
        let read = || -> Result<([[u32; 4]; 3], u32)> {
            let mut fields = [[0; 4]; 3];
            for ((base, offset), field) in [
                (state, POSITION_STATE_VECTOR_OFFSET),
                (controller, CONTROLLER_POSITION100_OFFSET),
                (controller, CONTROLLER_POSITION260_OFFSET),
            ]
            .into_iter()
            .zip(&mut fields)
            {
                let mut bytes = [0; 16];
                memory.read_exact(
                    base.checked_add(u64::from(offset))
                        .ok_or(Error::AddressOverflow)?,
                    &mut bytes,
                )?;
                *field = std::array::from_fn(|i| {
                    u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                });
            }
            let scalar = memory.read_u32(
                target
                    .address
                    .checked_add(u64::from(CHARACTER_SCALAR2E8_OFFSET))
                    .ok_or(Error::AddressOverflow)?,
            )?;
            Ok((fields, scalar))
        };
        let (fields, scalar2e8_bits) = read()?;
        if read()? != (fields, scalar2e8_bits)
            || memory.read_u64(link)? != state
            || self.character(target.address)? != Some(character)
            || self.character_placement(target)? != Some(placement)
        {
            return Err(Error::InvalidProfile(
                "character translation graph changed during observation",
            ));
        }
        let [
            position_state_bits,
            controller_position100_bits,
            controller_position260_bits,
        ] = fields;
        Ok(Some(CharacterTranslationObservation {
            placement,
            kind: character.kind,
            position_state_address: state,
            position_state_bits,
            controller_position100_bits,
            controller_position260_bits,
            scalar2e8_bits,
        }))
    }
}
pub(crate) fn character_translation_plan(
    current: Option<CharacterTranslationObservation>,
    expected: CharacterTranslationObservation,
    epoch: u64,
    displacement: [f32; 3],
) -> Result<([u32; 4], CharacterTranslationObservation)> {
    placement_source_preflight(current.map(|v| v.placement), expected.placement, epoch)?;
    let expected_controller = match expected.kind {
        ObjectKind::Character => ObjectKind::CharacterController,
        ObjectKind::Enemy => ObjectKind::EnemyController,
        ObjectKind::Player => ObjectKind::PlayerController,
        _ => {
            return Err(Error::InvalidProfile(
                "unsupported native translation character kind",
            ));
        }
    };
    if current != Some(expected)
        || expected.placement.motion.controls.controller.kind != expected_controller
        || expected.position_state_address == 0
        || !expected.position_state_address.is_multiple_of(16)
    {
        return Err(Error::NativeOperationRejected(
            "native character translation source superseded",
        ));
    }
    expected
        .position_state_address
        .checked_add(0x90)
        .ok_or(Error::AddressOverflow)?;
    expected
        .placement
        .motion
        .controls
        .controller
        .address
        .checked_add(u64::from(CONTROLLER_POSITION260_OFFSET) + 16)
        .ok_or(Error::AddressOverflow)?;
    let (delta, spatial) = translation_plan(
        current.map(|v| v.placement.motion.spatial),
        expected.placement.motion.spatial,
        epoch,
        displacement,
    )?;
    let add = |bits: [u32; 4]| -> Result<[u32; 4]> {
        let values = bits.map(f32::from_bits);
        let after: [f32; 4] = std::array::from_fn(|i| values[i] + f32::from_bits(delta[i]));
        if values.into_iter().chain(after).any(|v| !v.is_finite()) {
            return Err(Error::InvalidProfile(
                "nonfinite native character translation cache",
            ));
        }
        Ok(after.map(f32::to_bits))
    };
    let mut planned = expected;
    planned.placement.motion.spatial = spatial;
    planned.placement.position80_bits = add(expected.placement.position80_bits)?;
    planned.placement.position90_bits = add(expected.placement.position90_bits)?;
    planned.placement.positiond0_bits = add(expected.placement.positiond0_bits)?;
    planned.position_state_bits = add(expected.position_state_bits)?;
    planned.controller_position100_bits = add(expected.controller_position100_bits)?;
    planned.controller_position260_bits = add(expected.controller_position260_bits)?;
    let before = f32::from_bits(expected.scalar2e8_bits);
    let after = before + displacement[1];
    if !before.is_finite() || !after.is_finite() {
        return Err(Error::InvalidProfile(
            "nonfinite native character translation scalar",
        ));
    }
    planned.scalar2e8_bits = after.to_bits();
    Ok((delta, planned))
}
pub(crate) fn character_translation_postflight(
    planned: CharacterTranslationObservation,
    after: CharacterTranslationObservation,
) -> Result<CharacterTranslationObservation> {
    if after != planned {
        return Err(Error::InvalidProfile(
            "native character translation postflight changed",
        ));
    }
    Ok(after)
}
