//! Copied controller ownership and native transform-axis scale.
use crate::{
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const CONTROLLER_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x4cee70,
        length: 0x5f8,
    },
    CodeRange {
        rva: 0x650c90,
        length: 0x85f,
    },
    CodeRange {
        rva: 0x3d15b0,
        length: 0x4f,
    },
    CodeRange {
        rva: 0x3ddf10,
        length: 0x2f,
    },
    CodeRange {
        rva: 0x3c4d60,
        length: 0x547,
    },
    CodeRange {
        rva: 0x3ca960,
        length: 0x54e,
    },
    CodeRange {
        rva: 0x3c5b60,
        length: 0x981,
    },
    CodeRange {
        rva: 0x3f1240,
        length: 0x33,
    },
    CodeRange {
        rva: 0x3efe50,
        length: 0x10,
    },
    CodeRange {
        rva: 0x45e590,
        length: 0x9,
    },
    CodeRange {
        rva: 0x778200,
        length: 0xd71,
    },
];
pub(crate) const CONTROLLER_OFFSET: u32 = 0x58;
pub(crate) const CONTROLLER_OWNER_OFFSET: u32 = 0x10;
pub(crate) const AXIS_SCALE_OFFSET: u32 = 0x2d4;
const MODEL_MATRIX_TRANSLATION_OFFSET: u32 = 0x260;
const MODEL_MATRIX_UP_OFFSET: u32 = 0x240;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterControllerObservation {
    pub character: CharacterIdentity,
    pub address: u64,
    pub kind: ObjectKind,
    /// Raw native f32 bits preserve NaN payloads, signed zero and subnormal values.
    pub axis_scale_bits: [u32; 3],
}

impl CharacterControllerObservation {
    pub fn axis_scale(&self) -> [f32; 3] {
        self.axis_scale_bits.map(f32::from_bits)
    }

    /// Compute from the copied baseline, without writes or compounding prior results.
    /// This covers controller-axis math only, not collision, bones, cloth or motion.
    pub fn multiplied_axes(&self, multiplier: f32) -> Result<[f32; 3]> {
        let axes = self.axis_scale();
        let scaled = axes.map(|value| value * multiplier);
        if !multiplier.is_finite()
            || multiplier <= 0.0
            || axes
                .into_iter()
                .chain(scaled)
                .any(|v| !v.is_finite() || v <= 0.0)
        {
            return Err(Error::InvalidProfile(
                "invalid or unrepresentable controller scale",
            ));
        }
        Ok(scaled)
    }
}

fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Verify exact subclass and reciprocal owner, copying only three scale fields.
    /// Missing links return None; stale identity, mismatch or visible changes fail.
    /// This does not pin the controller or authorize transform writes.
    pub fn character_controller(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterControllerObservation>> {
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale character controller identity"));
        }
        let character = self
            .character(target.address)?
            .ok_or(Error::InvalidProfile("character disappeared"))?;
        let expected = match character.kind {
            ObjectKind::Character => ObjectKind::CharacterController,
            ObjectKind::Enemy => ObjectKind::EnemyController,
            ObjectKind::Player => ObjectKind::PlayerController,
            _ => {
                return Err(Error::InvalidProfile(
                    "wrong character controller owner type",
                ));
            }
        };
        let memory = self.module.memory();
        let link = at(target.address, CONTROLLER_OFFSET)?;
        let address = memory.read_u64(link)?;
        let object = self.require(address, expected)?;
        let check_owner = || -> Result<()> {
            if address != 0
                && memory.read_u64(at(address, CONTROLLER_OWNER_OFFSET)?)? != target.address
            {
                return Err(Error::InvalidProfile(
                    "controller belongs to another character",
                ));
            }
            Ok(())
        };
        check_owner()?;
        let read_scale = || -> Result<[u32; 3]> {
            let mut bytes = [0; 12];
            memory.read_exact(at(address, AXIS_SCALE_OFFSET)?, &mut bytes)?;
            Ok(std::array::from_fn(|i| {
                u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
            }))
        };
        let result = if object.is_some() {
            let scale = read_scale()?;
            if read_scale()? != scale {
                return Err(Error::InvalidProfile(
                    "controller scale changed during observation",
                ));
            }
            Some(CharacterControllerObservation {
                character: target,
                address,
                kind: expected,
                axis_scale_bits: scale,
            })
        } else {
            None
        };
        if memory.read_u64(link)? != address
            || self.require(address, expected)? != object
            || self.character(target.address)? != Some(character)
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile(
                "controller changed during observation",
            ));
        }
        check_owner()?;
        Ok(result)
    }
}

/// Copied fourth row of the controller's native 230..26f model matrix.
/// This cached transform can differ from physics position and does not prove rendering or a lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterModelTranslationObservation {
    pub controller: CharacterControllerObservation,
    pub translation_bits: [u32; 4],
}

/// The two selected model rows consumed by the native frontend's anchor equation.
/// This is not a DmyPoly lookup, a lock target cache or an engine-owned reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterModelAnchorRows {
    pub controller: CharacterControllerObservation,
    pub up_bits: [u32; 4],
    pub translation_bits: [u32; 4],
}
impl CharacterModelAnchorRows {
    /// Pure XYZ component of frontend778200: model row1 * physics map height + row3.
    /// The camera-dependent UI displacement and projection are separate. The fourth
    /// lanes remain raw observations and are not assumed to be canonical affine values.
    pub fn lifted_xyz(&self, map_height: f32) -> Result<[f32; 3]> {
        let delta = self.frontend_displacement_xyz(map_height)?;
        let result = std::array::from_fn(|i| delta[i] + f32::from_bits(self.translation_bits[i]));
        if result.into_iter().any(|v| !v.is_finite()) {
            return Err(Error::InvalidProfile("nonfinite frontend model anchor"));
        }
        Ok(result)
    }
    /// Copied model-up contribution; no legacy field-name or cache equivalence is inferred.
    pub fn frontend_displacement_xyz(&self, map_height: f32) -> Result<[f32; 3]> {
        let up: [f32; 3] = std::array::from_fn(|i| f32::from_bits(self.up_bits[i]));
        let result = up.map(|v| v * map_height);
        if !map_height.is_finite()
            || map_height < 0.0
            || up.into_iter().chain(result).any(|v| !v.is_finite())
        {
            return Err(Error::InvalidProfile(
                "nonfinite frontend model displacement",
            ));
        }
        Ok(result)
    }
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy only row1 and row3; reject detectable changes in either row and the
    /// exact controller/owner/character graph. This does not establish frame freshness,
    /// acquire a pin or exclude same-value ABA. Host retains readable graph and phase.
    pub fn character_model_anchor_rows(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterModelAnchorRows>> {
        if target.epoch == 0 {
            return Err(Error::InvalidProfile("zero model anchor epoch"));
        }
        let Some(controller) = self.character_controller(target)? else {
            return Ok(None);
        };
        let read = || -> Result<[[u32; 4]; 2]> {
            let mut rows = [[0; 4]; 2];
            for (offset, row) in [MODEL_MATRIX_UP_OFFSET, MODEL_MATRIX_TRANSLATION_OFFSET]
                .into_iter()
                .zip(&mut rows)
            {
                let mut bytes = [0; 16];
                self.module
                    .memory()
                    .read_exact(at(controller.address, offset)?, &mut bytes)?;
                *row = std::array::from_fn(|i| {
                    u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                });
            }
            Ok(rows)
        };
        let [up_bits, translation_bits] = read()?;
        if read()? != [up_bits, translation_bits]
            || self.character_controller(target)? != Some(controller)
        {
            return Err(Error::InvalidProfile(
                "frontend model rows or controller changed",
            ));
        }
        Ok(Some(CharacterModelAnchorRows {
            controller,
            up_bits,
            translation_bits,
        }))
    }
    /// Copy only the selected row, with exact controller type, reciprocal owner, scale and identity
    /// rechecks. No position-state allocation, other matrix rows or native getters are touched.
    /// Host retains the full readable graph and epoch; same-value ABA and atomicity are not acquired.
    pub fn character_model_translation(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterModelTranslationObservation>> {
        if target.epoch == 0 {
            return Err(Error::InvalidProfile("zero model translation epoch"));
        }
        let Some(controller) = self.character_controller(target)? else {
            return Ok(None);
        };
        let address = at(controller.address, MODEL_MATRIX_TRANSLATION_OFFSET)?;
        let read = || -> Result<[u32; 4]> {
            let mut bytes = [0; 16];
            self.module.memory().read_exact(address, &mut bytes)?;
            Ok(std::array::from_fn(|i| {
                u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
            }))
        };
        let translation_bits = read()?;
        if read()? != translation_bits || self.character_controller(target)? != Some(controller) {
            return Err(Error::InvalidProfile(
                "model translation or controller changed",
            ));
        }
        Ok(Some(CharacterModelTranslationObservation {
            controller,
            translation_bits,
        }))
    }
}
