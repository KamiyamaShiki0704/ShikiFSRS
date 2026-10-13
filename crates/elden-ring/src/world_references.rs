//! Copied references in six independently identified world collections.
//! An empty observation is never a character destruction or storage-release permission.
use crate::{character_sets::CharacterIdentity, objects::GameObjectCatalog};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
use std::collections::BTreeSet;

pub(crate) const REFERENCE_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x50f9e0,
        length: 0x402,
    },
    CodeRange {
        rva: 0x50fea0,
        length: 0x447,
    },
    CodeRange {
        rva: 0x510d70,
        length: 0xc3,
    },
    CodeRange {
        rva: 0x510e40,
        length: 0x7c,
    },
    CodeRange {
        rva: 0x511120,
        length: 0xace,
    },
    CodeRange {
        rva: 0x50e0f0,
        length: 0x94,
    },
    CodeRange {
        rva: 0x512c60,
        length: 0x92,
    },
];
const PRIMARY_OFFSET: u32 = 0x1e508;
const VECTOR_0_OFFSET: u32 = 0x1f1b0;
const VECTOR_1_OFFSET: u32 = 0x1f1d0;
const VECTOR_2_OFFSET: u32 = 0x1f200;
const LIST_0_OFFSET: u32 = 0x1e688;
const LIST_1_OFFSET: u32 = 0x1e698;
const LIST_2_OFFSET: u32 = 0x1e6a8;

/// Names distinguish observed storage; they do not promise a scheduler phase or ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldReferenceSource {
    Vector0,
    Vector1,
    Vector2,
    UpdateList0,
    UpdateList1,
    UpdateList2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldReferenceCount {
    pub source: WorldReferenceSource,
    pub visited: usize,
    pub matches: usize,
}

/// Counts addresses only. Does not dereference the target, decode its identifier, pin it,
/// inspect native task captures, enumerate every world reference, or authorize freeing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldReferenceObservation {
    pub target: CharacterIdentity,
    pub primary_slot_matches: bool,
    pub collections: [WorldReferenceCount; 6],
}

#[derive(Debug, PartialEq, Eq)]
struct CollectionSnapshot {
    source: WorldReferenceSource,
    header: [u64; 3],
    // Vector: (slot, pointer, 0). List: (node, pointer, next).
    entries: Vec<(u64, u64, u64)>,
}

fn at(base: u64, offset: u32) -> Result<u64> {
    base.checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

fn aligned(pointer: u64) -> Result<()> {
    if pointer & 7 != 0 {
        return Err(Error::InvalidProfile("unaligned world reference storage"));
    }
    Ok(())
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Count a possibly detached target address in the known primary slot and six collections.
    /// `limit` bounds the combined visits (1..=65536), including null/duplicate entries.
    /// Caller epoch is retained, not independently verified without reading the target.
    /// Missing world returns None; a different world, incomplete read or visible change fails.
    /// Caller must provide a valid read phase; repeated reads cannot prevent ABA or concurrency.
    pub fn world_character_references(
        &self,
        target: CharacterIdentity,
        limit: usize,
    ) -> Result<Option<WorldReferenceObservation>> {
        if target.address == 0
            || target.world_address == 0
            || target.epoch == 0
            || !(1..=65536).contains(&limit)
        {
            return Err(Error::InvalidProfile(
                "invalid world reference observation input",
            ));
        }
        aligned(target.address)?;
        let Some(world) = self.world()? else {
            return Ok(None);
        };
        if world.address != target.world_address {
            return Err(Error::InvalidProfile("stale world reference identity"));
        }
        let primary_address = at(world.address, PRIMARY_OFFSET)?;
        let primary = self.module.memory().read_u64(primary_address)?;
        let snapshots = self.reference_snapshots(world.address, limit)?;
        if self.reference_snapshots(world.address, limit)? != snapshots
            || self.module.memory().read_u64(primary_address)? != primary
            || self.world()? != Some(world)
        {
            return Err(Error::InvalidProfile(
                "world references changed during observation",
            ));
        }
        let collections = snapshots.map(|snapshot| WorldReferenceCount {
            source: snapshot.source,
            visited: snapshot.entries.len(),
            matches: snapshot
                .entries
                .iter()
                .filter(|entry| entry.1 == target.address)
                .count(),
        });
        Ok(Some(WorldReferenceObservation {
            target,
            primary_slot_matches: primary == target.address,
            collections,
        }))
    }

    fn reference_snapshots(&self, world: u64, limit: usize) -> Result<[CollectionSnapshot; 6]> {
        use WorldReferenceSource::*;
        let mut remaining = limit;
        Ok([
            self.reference_vector(world, VECTOR_0_OFFSET, 8, Vector0, &mut remaining)?,
            self.reference_vector(world, VECTOR_1_OFFSET, 16, Vector1, &mut remaining)?,
            self.reference_vector(world, VECTOR_2_OFFSET, 8, Vector2, &mut remaining)?,
            self.reference_list(world, LIST_0_OFFSET, UpdateList0, &mut remaining)?,
            self.reference_list(world, LIST_1_OFFSET, UpdateList1, &mut remaining)?,
            self.reference_list(world, LIST_2_OFFSET, UpdateList2, &mut remaining)?,
        ])
    }

    fn reference_vector(
        &self,
        world: u64,
        offset: u32,
        stride: u64,
        source: WorldReferenceSource,
        remaining: &mut usize,
    ) -> Result<CollectionSnapshot> {
        let memory = self.module.memory();
        let header_address = at(world, offset)?;
        // Allocator at +0 is not needed and is deliberately not read.
        let header = [
            memory.read_u64(at(header_address, 8)?)?,
            memory.read_u64(at(header_address, 16)?)?,
            memory.read_u64(at(header_address, 24)?)?,
        ];
        let [begin, end, capacity_end] = header;
        for pointer in header {
            aligned(pointer)?;
        }
        if end < begin
            || capacity_end < end
            || (begin == 0 && capacity_end != 0)
            || (end - begin) % stride != 0
            || (capacity_end - begin) % stride != 0
        {
            return Err(Error::InvalidProfile(
                "invalid world reference vector bounds",
            ));
        }
        let count = usize::try_from((end - begin) / stride).map_err(|_| Error::AddressOverflow)?;
        if count > *remaining {
            return Err(Error::InvalidProfile(
                "world reference visit limit exceeded",
            ));
        }
        *remaining -= count;
        let mut entries = Vec::with_capacity(count);
        for ordinal in 0..count {
            // Bounds proved above; unknown tail bytes of a 16-byte record are never read.
            let address = begin + ordinal as u64 * stride;
            entries.push((address, memory.read_u64(address)?, 0));
        }
        Ok(CollectionSnapshot {
            source,
            header,
            entries,
        })
    }

    fn reference_list(
        &self,
        world: u64,
        offset: u32,
        source: WorldReferenceSource,
        remaining: &mut usize,
    ) -> Result<CollectionSnapshot> {
        let memory = self.module.memory();
        let head = memory.read_u64(at(world, offset)?)?;
        let mut node = head;
        let mut seen = BTreeSet::new();
        let mut entries = Vec::new();
        while node != 0 {
            aligned(node)?;
            if !seen.insert(node) {
                return Err(Error::InvalidProfile("cyclic world reference list"));
            }
            if *remaining == 0 {
                return Err(Error::InvalidProfile(
                    "world reference visit limit exceeded",
                ));
            }
            *remaining -= 1;
            let pointer = memory.read_u64(node)?;
            let next = memory.read_u64(at(node, 8)?)?;
            entries.push((node, pointer, next));
            node = next;
        }
        Ok(CollectionSnapshot {
            source,
            header: [head, 0, 0],
            entries,
        })
    }
}
