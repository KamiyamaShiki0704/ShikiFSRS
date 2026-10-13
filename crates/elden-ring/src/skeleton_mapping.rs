//! Copied model-object to pose indices, never a complete HkaSkeleton or bone transform.
use crate::{
    character_sets::CharacterIdentity,
    character_visuals::{CharacterVisualLink, CharacterVisualObservation},
    objects::GameObjectCatalog,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const MAPPING_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0xb453f0,
        length: 0x9e,
    },
    CodeRange {
        rva: 0xb45540,
        length: 0xcf,
    },
    CodeRange {
        rva: 0xb45610,
        length: 0x15f,
    },
    CodeRange {
        rva: 0xb45770,
        length: 0xe3,
    },
];
const TABLE_OFFSET: u32 = 0x88;
const COUNT_OFFSET: u32 = 4;
const INDICES_OFFSET: u32 = 8;
const INDEX_STRIDE: u32 = 2;
const INDEX_LIMIT: u32 = 65536;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelObjectPoseIndex {
    /// Native signed index is nonnegative. No target-pose count or lifetime proof.
    Mapped(u16),
    /// Native getters choose a fallback for negative values; retain exact value.
    Fallback(i16),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkeletonMappingObservation {
    pub visual: CharacterVisualObservation,
    pub table_address: u64,
    pub indices_address: u64,
    pub native_indices: Vec<i16>,
}
impl SkeletonMappingObservation {
    /// None means outside the copied table, distinct from an in-table negative fallback.
    pub fn model_object(&self, index: u32) -> Option<ModelObjectPoseIndex> {
        self.native_indices.get(index as usize).map(|&value| {
            if value < 0 {
                ModelObjectPoseIndex::Fallback(value)
            } else {
                ModelObjectPoseIndex::Mapped(value as u16)
            }
        })
    }
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy bounded signed indices using the exact mapper link. Missing mapper/table returns
    /// None; an initialized zero-count table returns Some(empty). Does not read unknown table
    /// word0, fallback location, Hka pose/skeleton, affine records, names or dirty state.
    pub fn skeleton_mapping(
        &self,
        target: CharacterIdentity,
        limit: u32,
    ) -> Result<Option<SkeletonMappingObservation>> {
        if !(1..=INDEX_LIMIT).contains(&limit) {
            return Err(Error::InvalidProfile("invalid skeleton mapping limit"));
        }
        let Some(visual) = self.character_visual(target, CharacterVisualLink::SkeletonMapper)?
        else {
            return Ok(None);
        };
        let memory = self.module.memory();
        let link = visual
            .address
            .checked_add(u64::from(TABLE_OFFSET))
            .ok_or(Error::AddressOverflow)?;
        let table = memory.read_u64(link)?;
        if !table.is_multiple_of(std::mem::align_of::<u32>() as u64) {
            return Err(Error::InvalidProfile("unaligned skeleton mapping table"));
        }
        let read_header = || -> Result<(u32, u64)> {
            let count = memory.read_u32(
                table
                    .checked_add(u64::from(COUNT_OFFSET))
                    .ok_or(Error::AddressOverflow)?,
            )?;
            let data = memory.read_u64(
                table
                    .checked_add(u64::from(INDICES_OFFSET))
                    .ok_or(Error::AddressOverflow)?,
            )?;
            Ok((count, data))
        };
        let result = if table == 0 {
            None
        } else {
            let (count, data) = read_header()?;
            if count > limit
                || !data.is_multiple_of(std::mem::align_of::<i16>() as u64)
                || (count != 0 && data == 0)
            {
                return Err(Error::InvalidProfile("invalid skeleton mapping bounds"));
            }
            let extent = u64::from(count) * u64::from(INDEX_STRIDE);
            data.checked_add(extent).ok_or(Error::AddressOverflow)?;
            let mut bytes = vec![0; extent as usize];
            if !bytes.is_empty() {
                memory.read_exact(data, &mut bytes)?;
            }
            let mut check = vec![0; bytes.len()];
            if !check.is_empty() {
                memory.read_exact(data, &mut check)?;
            }
            if bytes != check || read_header()? != (count, data) {
                return Err(Error::InvalidProfile(
                    "skeleton mapping changed during copy",
                ));
            }
            Some(SkeletonMappingObservation {
                visual,
                table_address: table,
                indices_address: data,
                native_indices: bytes
                    .chunks_exact(INDEX_STRIDE as usize)
                    .map(|b| i16::from_le_bytes(b.try_into().expect("two bytes")))
                    .collect(),
            })
        };
        if memory.read_u64(link)? != table
            || self.character_visual(target, CharacterVisualLink::SkeletonMapper)? != Some(visual)
        {
            return Err(Error::InvalidProfile(
                "skeleton mapper link changed during copy",
            ));
        }
        Ok(result)
    }
}
