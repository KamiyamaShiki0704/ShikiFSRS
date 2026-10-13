//! Copied native update candidates in their existing order, never a character lease.
//! Native 50f9e0 conditionally sorts the priority word. No unconditional distance order is promised.
use crate::{
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectObservation},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const UPDATE_LIST_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x510970,
        length: 0x29b,
    },
    CodeRange {
        rva: 0x50e0f0,
        length: 0x94,
    },
    CodeRange {
        rva: 0x50f9e0,
        length: 0x402,
    },
    CodeRange {
        rva: 0x5048b0,
        length: 0x110,
    },
    CodeRange {
        rva: 0x504bc0,
        length: 0x77,
    },
];
const VECTOR_BEGIN_OFFSET: u32 = 0x1f1d8;
const VECTOR_END_OFFSET: u32 = 0x1f1e0;
const VECTOR_CAPACITY_OFFSET: u32 = 0x1f1e8;
const ENTRY_STRIDE: u32 = 16;
const MAX_CAPACITY: u32 = 0x10000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldUpdateEntry {
    pub ordinal: u32,
    pub character: CharacterIdentity,
    /// Raw priority f32 bits returned by 3ee920; not the character's current geometric distance.
    pub priority_bits: u32,
    /// Raw class produced by 400c60 and consumed by 50f9e0. No role/ownership interpretation.
    pub update_class: i32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldUpdateList {
    pub world: ObjectObservation,
    pub epoch: u64,
    pub entries_address: u64,
    pub capacity: u32,
    pub entries: Vec<WorldUpdateEntry>,
}
fn extent([begin, end, capacity]: [u64; 3], limit: usize) -> Result<(u32, u32)> {
    if !(1..=MAX_CAPACITY as usize).contains(&limit) {
        return Err(Error::InvalidProfile("invalid update-list limit"));
    }
    if begin == 0 {
        return if end == 0 && capacity == 0 {
            Ok((0, 0))
        } else {
            Err(Error::InvalidProfile(
                "null update-list begin with nonnull extent",
            ))
        };
    }
    let used = end.checked_sub(begin).ok_or(Error::AddressOverflow)?;
    let allocated = capacity.checked_sub(begin).ok_or(Error::AddressOverflow)?;
    if begin % 8 != 0
        || used % u64::from(ENTRY_STRIDE) != 0
        || allocated % u64::from(ENTRY_STRIDE) != 0
        || used > allocated
        || allocated / u64::from(ENTRY_STRIDE) > u64::from(MAX_CAPACITY)
        || used / u64::from(ENTRY_STRIDE) > limit as u64
    {
        return Err(Error::InvalidProfile(
            "invalid or excessive update-list extent",
        ));
    }
    Ok((
        (used / u64::from(ENTRY_STRIDE)) as u32,
        (allocated / u64::from(ENTRY_STRIDE)) as u32,
    ))
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    fn copy_update_list(
        &self,
        world: ObjectObservation,
        epoch: u64,
        limit: usize,
    ) -> Result<WorldUpdateList> {
        let memory = self.module.memory();
        let header = || -> Result<[u64; 3]> {
            let mut result = [0; 3];
            for (word, offset) in result.iter_mut().zip([
                VECTOR_BEGIN_OFFSET,
                VECTOR_END_OFFSET,
                VECTOR_CAPACITY_OFFSET,
            ]) {
                *word = memory.read_u64(
                    world
                        .address
                        .checked_add(u64::from(offset))
                        .ok_or(Error::AddressOverflow)?,
                )?;
            }
            Ok(result)
        };
        let before = header()?;
        let (count, capacity) = extent(before, limit)?;
        let mut entries = Vec::with_capacity(count as usize);
        for ordinal in 0..count {
            let address = before[0]
                .checked_add(u64::from(ordinal) * u64::from(ENTRY_STRIDE))
                .ok_or(Error::AddressOverflow)?;
            let mut bytes = [0; 16];
            memory.read_exact(address, &mut bytes)?;
            let target = u64::from_le_bytes(bytes[..8].try_into().unwrap());
            let character = self
                .character_identity(target, epoch)?
                .ok_or(Error::InvalidProfile("update-list character missing"))?;
            if character.world_address != world.address {
                return Err(Error::InvalidProfile("update-list world changed"));
            }
            entries.push(WorldUpdateEntry {
                ordinal,
                character,
                priority_bits: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
                update_class: i32::from_le_bytes(bytes[12..].try_into().unwrap()),
            });
        }
        if header()? != before || self.world()? != Some(world) {
            return Err(Error::InvalidProfile("update-list header or world changed"));
        }
        Ok(WorldUpdateList {
            world,
            epoch,
            entries_address: before[0],
            capacity,
            entries,
        })
    }
    /// Copy the bounded complete native vector twice, preserving every entry and its order.
    /// Missing world returns None; invalid extents/types and detectable payload/identity changes fail.
    /// Caller supplies full readable graph lifetime, exclusive phase and epoch on reuse. Same-value
    /// ABA, native locks and quiescence are not acquired; this method never sorts or calls the engine.
    pub fn world_update_list(&self, epoch: u64, limit: usize) -> Result<Option<WorldUpdateList>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero update-list epoch"));
        }
        extent([0; 3], limit)?;
        let Some(world) = self.world()? else {
            return Ok(None);
        };
        let value = self.copy_update_list(world, epoch, limit)?;
        if self.copy_update_list(world, epoch, limit)? != value {
            return Err(Error::InvalidProfile(
                "update-list payload or identity changed",
            ));
        }
        Ok(Some(value))
    }
}
