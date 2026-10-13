//! Copied typed model and pose links; no native updates or reference ownership.
use crate::{
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const VISUAL_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3f4e70,
        length: 0x59,
    },
    CodeRange {
        rva: 0xb728d0,
        length: 0x21,
    },
    CodeRange {
        rva: 0xb4ace0,
        length: 0x8,
    },
    CodeRange {
        rva: 0x3e8240,
        length: 0x629,
    },
    // Complete code through RET, padding and eight-entry inline switch table.
    CodeRange {
        rva: 0x3ff0e0,
        length: 0x2d4,
    },
    CodeRange {
        rva: 0x3ff550,
        length: 0x33,
    },
    // This is a destructor ending in a tail jump, not the model constructor.
    CodeRange {
        rva: 0x9f0310,
        length: 0x80,
    },
    CodeRange {
        rva: 0x9f1400,
        length: 0xbe,
    },
    CodeRange {
        rva: 0xb4b380,
        length: 0x282,
    },
    CodeRange {
        rva: 0xb453f0,
        length: 0x9e,
    },
];
pub(crate) const MODEL_OFFSET: u32 = 0x50;
const POSE_OFFSET: u32 = 0x398;
const ALTERNATE_POSE_OFFSET: u32 = 0x3a0;
const CLOTH_POSE_OFFSET: u32 = 0x3a8;
const SKELETON_MAPPER_OFFSET: u32 = 0x3b0;
const MODEL_PAYLOAD_OFFSET: u32 = 0x10;
const MODEL_EXPORTER_OFFSET: u32 = 0x660;
const MODEL_FALLBACK_OFFSET: u32 = 0x658;
const EXPORTER_BOUNDS_LINK_OFFSET: u32 = 0x28;
const EXPORTED_BOUNDS_OFFSET: u32 = 0x60;
const FALLBACK_BOUNDS_OFFSET: u32 = 0x90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterVisualLink {
    Model,
    Pose,
    /// Selected by the native update path. An unexpected subtype is rejected;
    /// this SDK does not claim every possible alternate source is a pose importer.
    AlternatePose,
    ClothPose,
    /// Native animation-skeleton to model-object-index modifier, not HkaSkeleton.
    SkeletonMapper,
}
impl CharacterVisualLink {
    fn profile(self) -> (u32, ObjectKind) {
        match self {
            Self::Model => (MODEL_OFFSET, ObjectKind::CharacterModel),
            Self::Pose => (POSE_OFFSET, ObjectKind::PoseImporter),
            Self::AlternatePose => (ALTERNATE_POSE_OFFSET, ObjectKind::PoseImporter),
            Self::ClothPose => (CLOTH_POSE_OFFSET, ObjectKind::PoseImporter),
            Self::SkeletonMapper => (SKELETON_MAPPER_OFFSET, ObjectKind::SkeletonMapper),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterVisualObservation {
    pub character: CharacterIdentity,
    pub link: CharacterVisualLink,
    pub address: u64,
    pub kind: ObjectKind,
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy only one selected pointer and its exact primary-vtable type. No reciprocal
    /// character-owner field has been proved for these objects; this is a link observation,
    /// not an ownership claim, engine reference, pose readiness or selection-state report.
    pub fn character_visual(
        &self,
        target: CharacterIdentity,
        link: CharacterVisualLink,
    ) -> Result<Option<CharacterVisualObservation>> {
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale character visual identity"));
        }
        let (offset, kind) = link.profile();
        let field = target
            .address
            .checked_add(u64::from(offset))
            .ok_or(Error::AddressOverflow)?;
        let address = self.module.memory().read_u64(field)?;
        let object = self.require(address, kind)?;
        if self.module.memory().read_u64(field)? != address
            || self.require(address, kind)? != object
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile(
                "character visual link changed during observation",
            ));
        }
        Ok(object.map(|_| CharacterVisualObservation {
            character: target,
            link,
            address,
            kind,
        }))
    }
}

/// Selected raw XYZ bounds from the native model route. No W/padding, clamp or native getter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterModelBoundsObservation {
    pub model: CharacterVisualObservation,
    pub payload_address: u64,
    pub exporter_address: u64,
    /// Zero selects the separate fallback model-item route.
    pub exported_bounds_address: u64,
    pub fallback_address: Option<u64>,
    pub bounds_address: u64,
    pub minimum_bits: [u32; 3],
    pub maximum_bits: [u32; 3],
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy six initialized selected words and recheck every used pointer, full identity and
    /// exact model primary type. Interior links are opaque, with no RTTI/ownership inference.
    /// Missing model returns None; the native getter's global default is never fabricated.
    /// Complete readable allocation graph, phase and same-value ABA remain caller duties.
    pub fn character_model_bounds(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterModelBoundsObservation>> {
        if target.epoch == 0 {
            return Err(Error::InvalidProfile("zero model bounds epoch"));
        }
        let Some(model) = self.character_visual(target, CharacterVisualLink::Model)? else {
            return Ok(None);
        };
        let memory = self.module.memory();
        let add = |address: u64, offset: u32| {
            address
                .checked_add(u64::from(offset))
                .ok_or(Error::AddressOverflow)
        };
        let link = |address: u64, offset: u32, nullable: bool| -> Result<u64> {
            let value = memory.read_u64(add(address, offset)?)?;
            if (!nullable && value == 0) || !value.is_multiple_of(8) {
                return Err(Error::InvalidProfile("invalid model bounds interior link"));
            }
            Ok(value)
        };
        let route = || -> Result<(u64, u64, u64, Option<u64>, u64)> {
            let payload = link(model.address, MODEL_PAYLOAD_OFFSET, false)?;
            let exporter = link(payload, MODEL_EXPORTER_OFFSET, false)?;
            let exported = link(exporter, EXPORTER_BOUNDS_LINK_OFFSET, true)?;
            if exported != 0 {
                Ok((
                    payload,
                    exporter,
                    exported,
                    None,
                    add(exported, EXPORTED_BOUNDS_OFFSET)?,
                ))
            } else {
                let fallback = link(payload, MODEL_FALLBACK_OFFSET, false)?;
                Ok((
                    payload,
                    exporter,
                    exported,
                    Some(fallback),
                    add(fallback, FALLBACK_BOUNDS_OFFSET)?,
                ))
            }
        };
        let (
            payload_address,
            exporter_address,
            exported_bounds_address,
            fallback_address,
            bounds_address,
        ) = route()?;
        let read_xyz = |address: u64| -> Result<[u32; 3]> {
            let mut bytes = [0; 12];
            memory.read_exact(address, &mut bytes)?;
            Ok(std::array::from_fn(|i| {
                u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
            }))
        };
        let read = || {
            Ok::<_, Error>((
                read_xyz(bounds_address)?,
                read_xyz(add(bounds_address, 0x10)?)?,
            ))
        };
        let (minimum_bits, maximum_bits) = read()?;
        if read()? != (minimum_bits, maximum_bits)
            || route()?
                != (
                    payload_address,
                    exporter_address,
                    exported_bounds_address,
                    fallback_address,
                    bounds_address,
                )
            || self.character_visual(target, CharacterVisualLink::Model)? != Some(model)
        {
            return Err(Error::InvalidProfile(
                "model bounds route or selected words changed",
            ));
        }
        Ok(Some(CharacterModelBoundsObservation {
            model,
            payload_address,
            exporter_address,
            exported_bounds_address,
            fallback_address,
            bounds_address,
            minimum_bits,
            maximum_bits,
        }))
    }
}
