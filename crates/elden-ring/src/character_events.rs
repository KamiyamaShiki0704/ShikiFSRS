//! Stored event IDs and exact ChrSet mapping copies; no script visibility or lifetime claim.
use crate::{
    character_sets::{CharacterIdentity, CharacterRegistrationObservation},
    objects::GameObjectCatalog,
};
use shikifsrs_runtime::{
    Error, Result,
    memory::AddressSpace,
    native_tree::{PointerTreeEntry, TreeKeys, U32PointerTree},
    profile::CodeRange,
};

pub(crate) const EVENT_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3efb20,
        length: 0xc,
    },
    CodeRange {
        rva: 0x656900,
        length: 0x77,
    },
    CodeRange {
        rva: 0x3efa90,
        length: 0x1b,
    },
    CodeRange {
        rva: 0x4947a0,
        length: 0x58,
    },
    CodeRange {
        rva: 0x494800,
        length: 0x198,
    },
    CodeRange {
        rva: 0x492c80,
        length: 0x72,
    },
    CodeRange {
        rva: 0x4916b0,
        length: 0x47,
    },
    CodeRange {
        rva: 0x492450,
        length: 0x15f,
    },
    CodeRange {
        rva: 0x491660,
        length: 0x47,
    },
    CodeRange {
        rva: 0x4923b0,
        length: 0x9a,
    },
    CodeRange {
        rva: 0x495ab0,
        length: 0x67,
    },
    CodeRange {
        rva: 0x495b20,
        length: 0x315,
    },
];
pub(crate) const EVENT_GETTER_SLOT: u32 = 0x300;
pub(crate) const STORED_EVENT_ID_OFFSET: u32 = 0x1e8;
const ENTITY_TREE_OFFSET: u32 = 0x28;
const GROUP_TREE_OFFSET: u32 = 0x40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterEventIdObservation {
    pub character: CharacterIdentity,
    /// Base/enemy getter returns this. Player's virtual getter may replace it.
    pub stored_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterEventMappingObservation {
    pub registration: CharacterRegistrationObservation,
    pub id: CharacterEventIdObservation,
    /// Exact unique-key lookup for stored_id. Foreign/null values are copied, not dereferenced.
    pub entity_entry: Option<u64>,
    /// Complete unique entity map, including foreign/null values copied without dereferencing.
    /// Keeping every node permits removal postflight to preserve all other mappings.
    pub entities: Vec<PointerTreeEntry>,
    /// All group-map nodes whose value is this entry, retaining duplicate keys/nodes.
    /// Absence of this stored ID does not imply the engine omitted model-group registration.
    pub groups: Vec<PointerTreeEntry>,
    /// Complete repeated-key group map. Foreign nodes stay opaque and preserve duplicate keys.
    pub group_entries: Vec<PointerTreeEntry>,
}
impl CharacterEventMappingObservation {
    /// All unique-map aliases pointing to the exact registered entry, in unsigned key order.
    /// This does not include model-group registrations or acquire a native reference.
    pub fn entity_aliases(&self) -> impl Iterator<Item = PointerTreeEntry> + '_ {
        self.entities
            .iter()
            .filter(|entry| entry.value == self.registration.entry_address)
            .copied()
    }
}
fn at(base: u64, offset: u32) -> Result<u64> {
    base.checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    pub fn character_event_id(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterEventIdObservation>> {
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale event ID identity"));
        }
        let slot = at(target.address, STORED_EVENT_ID_OFFSET)?;
        let stored_id = self.module.memory().read_u32(slot)?;
        if self.module.memory().read_u32(slot)? != stored_id
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile("character event ID changed"));
        }
        Ok(Some(CharacterEventIdObservation {
            character: target,
            stored_id,
        }))
    }

    /// Copies both complete bounded mapping trees, validates exact set membership/backlink
    /// and repeats the observations. Does not establish world ownership, lock, pin or ABA safety.
    pub fn character_event_mappings(
        &self,
        target: CharacterIdentity,
        set: u64,
        limit: usize,
    ) -> Result<Option<CharacterEventMappingObservation>> {
        if !(1..=65536).contains(&limit) {
            return Err(Error::InvalidProfile("invalid event map limit"));
        }
        let Some(registration) = self.character_registration(target, set, limit)? else {
            return Ok(None);
        };
        let id = self
            .character_event_id(target)?
            .ok_or(Error::InvalidProfile("event target vanished"))?;
        let entity_tree =
            U32PointerTree::new(self.module.memory(), at(set, ENTITY_TREE_OFFSET)?, limit)?;
        let group_tree =
            U32PointerTree::new(self.module.memory(), at(set, GROUP_TREE_OFFSET)?, limit)?;
        let entities = entity_tree.snapshot(TreeKeys::Unique)?;
        let groups = group_tree.snapshot(TreeKeys::Repeated)?;
        let entity_entry = entities
            .iter()
            .find(|x| x.key == id.stored_id)
            .map(|x| x.value);
        let result = CharacterEventMappingObservation {
            registration,
            id,
            entity_entry,
            entities: entities.clone(),
            groups: groups
                .iter()
                .filter(|x| x.value == registration.entry_address)
                .copied()
                .collect(),
            group_entries: groups.clone(),
        };
        if entity_tree.snapshot(TreeKeys::Unique)? != entities
            || group_tree.snapshot(TreeKeys::Repeated)? != groups
            || self.character_event_id(target)? != Some(id)
            || self.character_registration(target, set, limit)? != Some(registration)
        {
            return Err(Error::InvalidProfile("event mappings changed during copy"));
        }
        Ok(Some(result))
    }
}
