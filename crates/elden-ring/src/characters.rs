//! Copied character-data ownership and bounded effect membership observations.
//! No game writes, effects application/removal, or native calls are provided here.
use crate::character_sets::CharacterIdentity;
use crate::objects::{GameObjectCatalog, ObjectKind};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
use std::collections::BTreeSet;

pub(crate) const CHARACTER_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3fe100,
        length: 0x611,
    },
    CodeRange {
        rva: 0x4c5880,
        length: 0x1f,
    },
    CodeRange {
        rva: 0x43d560,
        length: 0x4d,
    },
    CodeRange {
        rva: 0x4364b0,
        length: 0x23c,
    },
    CodeRange {
        rva: 0x43cf70,
        length: 0x31,
    },
    CodeRange {
        rva: 0x4c5930,
        length: 0x5,
    },
    CodeRange {
        rva: 0x4c5940,
        length: 0x73,
    },
    CodeRange {
        rva: 0x3f21b0,
        length: 0xc,
    },
    CodeRange {
        rva: 0x4fa710,
        length: 0x24,
    },
    CodeRange {
        rva: 0x4f3df0,
        length: 0x91,
    },
    CodeRange {
        rva: 0x650c90,
        length: 0x85f,
    },
];

const MODULE_CONTAINER_OFFSET: u32 = 0x190;
const DATA_MODULE_INDEX: u32 = 0;
const MODULE_OWNER_OFFSET: u32 = 8;
const INITIALIZER_OVERRIDE_OFFSET: u32 = 0xc4;
pub(crate) const EFFECT_CONTAINER_OFFSET: u32 = 0x178;
const EFFECT_HEAD_OFFSET: u32 = 8;
const EFFECT_ID_OFFSET: u32 = 8;
const EFFECT_NEXT_OFFSET: u32 = 0x30;
const EFFECT_OWNER_OFFSET: u32 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterDataObservation {
    pub address: u64,
    pub owner_address: u64,
    /// Module container is part of copied storage identity, not an engine reference guard.
    pub container_address: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterInitializerOverrideObservation {
    pub character: CharacterIdentity,
    pub data: CharacterDataObservation,
    /// Raw stored creation override. Constructor4364b0 initializes -1;
    /// player650c90 copies a nonzero creation-input40 here. This neither resolves
    /// a CharaInitParam row nor identifies the currently loaded model.
    pub stored_id: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectNodeObservation {
    pub address: u64,
    pub native_id: i32,
}

/// Complete bounded chain, including duplicate/nonpositive IDs without assigning
/// gameplay meaning. Missing container differs from an existing empty chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterEffectsObservation {
    pub character: CharacterIdentity,
    pub container_address: u64,
    pub entries: Vec<EffectNodeObservation>,
}

fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(offset as u64)
        .ok_or(Error::AddressOverflow)
}

fn pointer(address: u64) -> Result<u64> {
    if !address.is_multiple_of(8) {
        return Err(Error::InvalidProfile("unaligned character link"));
    }
    Ok(address)
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy only dataC4 with exact owner, module and complete character identity
    /// rechecks. Repeated reads detect visible changes, without an ABA/lifetime lease.
    pub fn character_initializer_override(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterInitializerOverrideObservation>> {
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale initializer override identity"));
        }
        let Some(data) = self.character_data(target.address)? else {
            if self.character_identity(target.address, target.epoch)? != Some(target) {
                return Err(Error::InvalidProfile("initializer override owner changed"));
            }
            return Ok(None);
        };
        let field = at(data.address, INITIALIZER_OVERRIDE_OFFSET)?;
        let stored_id = self.module.memory().read_u32(field)? as i32;
        if self.module.memory().read_u32(field)? as i32 != stored_id
            || self.character_data(target.address)? != Some(data)
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile(
                "initializer override changed during copy",
            ));
        }
        Ok(Some(CharacterInitializerOverrideObservation {
            character: target,
            data,
            stored_id,
        }))
    }

    /// Copy a complete chain against the caller's full identity. No allocation
    /// lease, atomic snapshot, or effect acceptance is implied by repeated reads.
    pub fn character_effects(
        &self,
        target: CharacterIdentity,
        limit: usize,
    ) -> Result<Option<CharacterEffectsObservation>> {
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale character effects identity"));
        }
        let result = self.effect_observations(target.address, None, limit)?;
        if self.character_identity(target.address, target.epoch)? != Some(target) {
            return Err(Error::InvalidProfile("character effects identity changed"));
        }
        Ok(
            result.map(|(container, _, entries)| CharacterEffectsObservation {
                character: target,
                container_address: container,
                entries,
            }),
        )
    }

    /// Copy the proven data-module slot and back-reference. No gameplay fields or lifetime implied.
    pub fn character_data(&self, address: u64) -> Result<Option<CharacterDataObservation>> {
        let Some(character) = self.character(address)? else {
            return Ok(None);
        };
        let memory = self.module.memory();
        let link = at(address, MODULE_CONTAINER_OFFSET)?;
        let container = pointer(memory.read_u64(link)?)?;
        let slot = at(container, DATA_MODULE_INDEX * 8)?;
        let module = if container == 0 {
            0
        } else {
            memory.read_u64(slot)?
        };
        let result = if let Some(data) = self.require(module, ObjectKind::CharacterData)? {
            if memory.read_u64(at(data.address, MODULE_OWNER_OFFSET)?)? != address {
                return Err(Error::InvalidProfile(
                    "character data belongs to another owner",
                ));
            }
            Some(CharacterDataObservation {
                address: data.address,
                owner_address: address,
                container_address: container,
            })
        } else {
            None
        };
        if memory.read_u64(link)? != container
            || (container != 0 && memory.read_u64(slot)? != module)
            || self.character(address)? != Some(character)
            || (module != 0
                && (self.require(module, ObjectKind::CharacterData)?.is_none()
                    || memory.read_u64(at(module, MODULE_OWNER_OFFSET)?)? != address))
        {
            return Err(Error::InvalidProfile(
                "character module changed during observation",
            ));
        }
        Ok(result)
    }

    /// Observe membership along at most `limit` native nodes (1..=65536).
    /// None means the character/effect container is absent. A found prefix need not
    /// traverse later nodes; missing results require a complete, acyclic chain.
    /// Copies only visited fields and rechecks them; this does not prevent ABA or pin objects.
    pub fn has_sp_effect(&self, address: u64, id: i32, limit: usize) -> Result<Option<bool>> {
        Ok(self
            .effect_state(address, Some(id), limit)?
            .map(|(_, found)| found))
    }

    /// Internal complete-chain validation before an unsafe mutation, or prefix membership.
    pub(crate) fn effect_state(
        &self,
        address: u64,
        id: Option<i32>,
        limit: usize,
    ) -> Result<Option<(u64, bool)>> {
        Ok(self
            .effect_observations(address, id, limit)?
            .map(|(container, found, _)| (container, found)))
    }

    fn effect_observations(
        &self,
        address: u64,
        id: Option<i32>,
        limit: usize,
    ) -> Result<Option<(u64, bool, Vec<EffectNodeObservation>)>> {
        if !(1..=65_536).contains(&limit) {
            return Err(Error::InvalidProfile("invalid effect observation limit"));
        }
        let Some(character) = self.character(address)? else {
            return Ok(None);
        };
        let memory = self.module.memory();
        let link = at(address, EFFECT_CONTAINER_OFFSET)?;
        let container = pointer(memory.read_u64(link)?)?;
        let container_type = self.require(container, ObjectKind::EffectContainer)?;
        if container != 0 && memory.read_u64(at(container, EFFECT_OWNER_OFFSET)?)? != address {
            return Err(Error::InvalidProfile(
                "effect container belongs to another character",
            ));
        }
        let head = if container == 0 {
            0
        } else {
            memory.read_u64(at(container, EFFECT_HEAD_OFFSET)?)?
        };
        let mut node = head;
        let mut seen = BTreeSet::new();
        let mut observations = Vec::new();
        let mut found = false;
        while node != 0 {
            pointer(node)?;
            if observations.len() == limit || !seen.insert(node) {
                return Err(Error::InvalidProfile(
                    "effect chain is cyclic or exceeds limit",
                ));
            }
            let native_id = memory.read_u32(at(node, EFFECT_ID_OFFSET)?)? as i32;
            if Some(native_id) == id {
                observations.push((node, native_id, None));
                found = true;
                break;
            }
            let next = memory.read_u64(at(node, EFFECT_NEXT_OFFSET)?)?;
            observations.push((node, native_id, Some(next)));
            node = next;
        }
        for &(node, native_id, next) in &observations {
            if memory.read_u32(at(node, EFFECT_ID_OFFSET)?)? as i32 != native_id {
                return Err(Error::InvalidProfile(
                    "effect chain changed during observation",
                ));
            }
            if let Some(expected) = next
                && memory.read_u64(at(node, EFFECT_NEXT_OFFSET)?)? != expected
            {
                return Err(Error::InvalidProfile(
                    "effect chain changed during observation",
                ));
            }
        }
        if memory.read_u64(link)? != container
            || (container != 0 && memory.read_u64(at(container, EFFECT_HEAD_OFFSET)?)? != head)
            || self.require(container, ObjectKind::EffectContainer)? != container_type
            || (container != 0 && memory.read_u64(at(container, EFFECT_OWNER_OFFSET)?)? != address)
            || self.character(address)? != Some(character)
        {
            return Err(Error::InvalidProfile(
                "character effect links changed during observation",
            ));
        }
        Ok((container != 0).then(|| {
            (
                container,
                found,
                observations
                    .into_iter()
                    .map(|(address, native_id, _)| EffectNodeObservation { address, native_id })
                    .collect(),
            )
        }))
    }
}
