//! Bounded copied inputs of the native effective-team query and directed relations.
//! No native calls, writes, engine references or inferred local/network roles.
use crate::character_sets::CharacterIdentity;
use crate::objects::{GameObjectCatalog, ObjectKind, ObjectObservation};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
use std::collections::BTreeSet;

pub(crate) const TEAM_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3f1c90,
        length: 0x73,
    },
    CodeRange {
        rva: 0x51b5d0,
        length: 0x3d,
    },
    CodeRange {
        rva: 0x51b580,
        length: 0xd,
    },
    CodeRange {
        rva: 0x51b590,
        length: 0xf,
    },
    CodeRange {
        rva: 0x51b5a0,
        length: 0xe,
    },
    CodeRange {
        rva: 0x51b5b0,
        length: 0x1e,
    },
    CodeRange {
        rva: 0x4fa370,
        length: 0x79,
    },
    CodeRange {
        rva: 0x3ea1d0,
        length: 0x7,
    },
    CodeRange {
        rva: 0x49c250,
        length: 0x2b,
    },
    CodeRange {
        rva: 0x49c200,
        length: 0x49,
    },
    CodeRange {
        rva: 0x266cd40,
        length: 0x21,
    },
    CodeRange {
        rva: 0x4b1e10,
        length: 0x47,
    },
    CodeRange {
        rva: 0x2d4a70,
        length: 0x7,
    },
    // Includes both jump tables and every case; Ghidra's aggregate body is not contiguous.
    CodeRange {
        rva: 0x4b21e0,
        length: 0x128,
    },
    CodeRange {
        rva: 0x4b1b20,
        length: 0xa5,
    },
];
const TEAM_OFFSET: u32 = 0x6c;
const EFFECT_CONTAINER_OFFSET: u32 = 0x178;
const EFFECT_HEAD_OFFSET: u32 = 8;
const EFFECT_OWNER_OFFSET: u32 = 0x10;
const EFFECT_BUCKETS_OFFSET: u32 = 0x28;
const EFFECT_FLAGS_OFFSET: u32 = 0x60;
const EFFECT_NEXT_OFFSET: u32 = 0x30;
const EFFECT_CATEGORY_OFFSET: u32 = 0x156;
const TEMPORARY_SLOTS_OFFSET: u32 = 0x308;
const SLOT_HEAD_OFFSET: u32 = 0x10;
const SLOT_INDEX_OFFSET: u32 = 8;
const SLOT_KIND_OFFSET: u32 = 0xc;
const SLOT_NEXT_OFFSET: u32 = 0x10;
const SLOT_ACTIVE_OFFSET: u32 = 0x18;
const TEAM_SLOT_KIND: u32 = 0x18;
const TEAM_SLOT_ARRAY_OFFSET: u32 = 0x30;
const TEAM_SLOT_COUNT_OFFSET: u32 = 0x38;
const TEAM_SLOT_ROLE_OFFSET: u32 = 0x44;
const TEAM_VALUE_OFFSET: u32 = 0x14;
const RELATION_MATRIX_RVA: u32 = 0x3b283f0;
const TEAM_COUNT: u32 = 79;

#[derive(Debug, Clone, PartialEq, Eq)]
struct EffectInput {
    address: u64,
    parameter: Option<u64>,
    flags: u32,
    next: Option<u64>,
    category: Option<u16>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct SlotInput {
    address: u64,
    index: u32,
    kind: u32,
    active: u8,
    next: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct TemporaryInput {
    object: ObjectObservation,
    count: u64,
    array: Option<u64>,
    first_value: Option<(u64, u8)>,
    role: u8,
}

/// Original inputs are copied twice, including the exact prefixes consumed by
/// the native query. No allocation lease, atomic snapshot or readiness implied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterTeamObservation {
    pub module_base: u64,
    pub character: CharacterIdentity,
    pub stored_team: u8,
    /// Unknown values remain observable, but cannot index the native relation matrix.
    pub effective_team: u8,
    pub effect_override: bool,
    pub temporary_slot: Option<ObjectObservation>,
    effect_container: Option<ObjectObservation>,
    effect_buckets: Option<u64>,
    effect_inputs: Vec<EffectInput>,
    slot_head: u64,
    slot_inputs: Vec<SlotInput>,
    temporary_input: Option<TemporaryInput>,
}
impl CharacterTeamObservation {
    pub fn team_index(&self) -> Result<u8> {
        valid_team(self.effective_team)?;
        Ok(self.effective_team)
    }
}

/// The directed matrix cell for the supplied indices at observation time.
/// This does not prove those indices still describe any character afterward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeamRelationObservation {
    pub module_base: u64,
    pub subject_team: u8,
    pub other_team: u8,
    pub object: ObjectObservation,
    /// Matches native 51b5d0's query flags [1,0,0], with third argument zero.
    pub hostile: bool,
}

/// Two full identities and their freshly copied team inputs, with the directed
/// matrix cell checked around both observations. No pin or atomicity is implied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterRelationObservation {
    pub subject: CharacterTeamObservation,
    pub other: CharacterTeamObservation,
    pub relation: TeamRelationObservation,
}

fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(offset as u64)
        .ok_or(Error::AddressOverflow)
}
fn pointer(address: u64) -> Result<u64> {
    if !address.is_multiple_of(8) {
        return Err(Error::InvalidProfile("unaligned team input link"));
    }
    Ok(address)
}
fn valid_team(value: u8) -> Result<()> {
    if u32::from(value) >= TEAM_COUNT {
        return Err(Error::InvalidProfile("team index outside native matrix"));
    }
    Ok(())
}
fn remap_temporary(team: u8, role: u8) -> u8 {
    match (team, role) {
        (34, 6) => 34,
        (34, 7) => 36,
        (34, 8) => 37,
        (34, 9) => 38,
        (34, 11) => 39,
        (34, 24) => 35,
        (40, 6) => 40,
        (40, 7) => 42,
        (40, 8) => 43,
        (40, 9) => 44,
        (40, 11) => 45,
        (40, 24) => 41,
        (40, 28) => 46,
        _ => team,
    }
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Coordinate two characters from the same world and epoch. Missing world,
    /// character or relation cell is None; stale/mixed scopes, unknown team
    /// indices or any detectable input/cell change fail. Direction is preserved.
    pub fn character_relation(
        &self,
        subject: CharacterIdentity,
        other: CharacterIdentity,
        node_limit: usize,
    ) -> Result<Option<CharacterRelationObservation>> {
        if subject.epoch == 0
            || subject.epoch != other.epoch
            || subject.world_address != other.world_address
            || !(1..=65536).contains(&node_limit)
        {
            return Err(Error::InvalidProfile(
                "mixed character relation scope or invalid limit",
            ));
        }
        let Some(subject_team) = self.character_team(subject, node_limit)? else {
            return Ok(None);
        };
        let Some(other_team) = self.character_team(other, node_limit)? else {
            return Ok(None);
        };
        let subject_index = subject_team.team_index()?;
        let other_index = other_team.team_index()?;
        let relation = self.team_relation(subject_index, other_index)?;
        if self.character_team(subject, node_limit)? != Some(subject_team.clone())
            || self.character_team(other, node_limit)? != Some(other_team.clone())
            || self.team_relation(subject_index, other_index)? != relation
        {
            return Err(Error::InvalidProfile("character relation inputs changed"));
        }
        Ok(relation.map(|relation| CharacterRelationObservation {
            subject: subject_team,
            other: other_team,
            relation,
        }))
    }

    /// Copy native 3f1c90 inputs in its precedence order: stored byte, eligible
    /// effect category 0x84, then first active temporary slot(kind24,index0).
    /// Each linked prefix is limited to 1..65536 nodes; cycles/truncation/unknown
    /// selected-slot types and detectable changes fail. Missing character is None.
    pub fn character_team(
        &self,
        target: CharacterIdentity,
        node_limit: usize,
    ) -> Result<Option<CharacterTeamObservation>> {
        if target.epoch == 0 || !(1..=65536).contains(&node_limit) {
            return Err(Error::InvalidProfile("invalid team epoch or node limit"));
        }
        let first = self.copy_team_inputs(target, node_limit)?;
        if self.copy_team_inputs(target, node_limit)? != first {
            return Err(Error::InvalidProfile("effective team inputs changed"));
        }
        Ok(first)
    }

    fn copy_team_inputs(
        &self,
        target: CharacterIdentity,
        limit: usize,
    ) -> Result<Option<CharacterTeamObservation>> {
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale effective team identity"));
        }
        let memory = self.module.memory();
        let stored_team = memory.read_u8(at(target.address, TEAM_OFFSET)?)?;
        let container = pointer(memory.read_u64(at(target.address, EFFECT_CONTAINER_OFFSET)?)?)?;
        let effect_container = self.require(container, ObjectKind::EffectContainer)?;
        let mut result = CharacterTeamObservation {
            module_base: self.module.base(),
            character: target,
            stored_team,
            effective_team: stored_team,
            effect_override: false,
            temporary_slot: None,
            effect_container,
            effect_buckets: None,
            effect_inputs: Vec::new(),
            slot_head: 0,
            slot_inputs: Vec::new(),
            temporary_input: None,
        };
        if container != 0 {
            if memory.read_u64(at(container, EFFECT_OWNER_OFFSET)?)? != target.address {
                return Err(Error::InvalidProfile("foreign team effect owner"));
            }
            let buckets = memory.read_u64(at(container, EFFECT_BUCKETS_OFFSET)?)?;
            result.effect_buckets = Some(buckets);
            // Native category132: ((132 % 15) + 1) * 4 = 52. Cache-zero skips traversal.
            if buckets & (0xfu64 << 52) != 0 {
                let mut current = pointer(memory.read_u64(at(container, EFFECT_HEAD_OFFSET)?)?)?;
                let mut seen = BTreeSet::new();
                while current != 0 {
                    if seen.len() >= limit || !seen.insert(current) {
                        return Err(Error::InvalidProfile("team effect prefix cycle or limit"));
                    }
                    let flags = memory.read_u32(at(current, EFFECT_FLAGS_OFFSET)?)?;
                    let (parameter, category) = if flags & 0x800c0003 == 0 {
                        let parameter = memory.read_u64(current)?;
                        let mut bytes = [0; 2];
                        if parameter != 0 {
                            memory
                                .read_exact(at(parameter, EFFECT_CATEGORY_OFFSET)?, &mut bytes)?;
                        }
                        (Some(parameter), Some(u16::from_le_bytes(bytes)))
                    } else {
                        (None, None)
                    };
                    let next = if category == Some(0x84) {
                        None
                    } else {
                        Some(pointer(memory.read_u64(at(current, EFFECT_NEXT_OFFSET)?)?)?)
                    };
                    result.effect_inputs.push(EffectInput {
                        address: current,
                        parameter,
                        flags,
                        next,
                        category,
                    });
                    if category == Some(0x84) {
                        result.effect_override = true;
                        result.effective_team = 15;
                        break;
                    }
                    current = next.unwrap_or(0);
                }
            }
        }
        let slots = at(target.address, TEMPORARY_SLOTS_OFFSET)?;
        let mut current = pointer(memory.read_u64(at(slots, SLOT_HEAD_OFFSET)?)?)?;
        result.slot_head = current;
        let mut seen = BTreeSet::new();
        while current != 0 {
            if seen.len() >= limit || !seen.insert(current) {
                return Err(Error::InvalidProfile(
                    "temporary team prefix cycle or limit",
                ));
            }
            let input = SlotInput {
                address: current,
                index: memory.read_u32(at(current, SLOT_INDEX_OFFSET)?)?,
                kind: memory.read_u32(at(current, SLOT_KIND_OFFSET)?)?,
                active: memory.read_u8(at(current, SLOT_ACTIVE_OFFSET)?)?,
                next: pointer(memory.read_u64(at(current, SLOT_NEXT_OFFSET)?)?)?,
            };
            let found = input.active != 0 && input.kind == TEAM_SLOT_KIND && input.index == 0;
            let next = input.next;
            result.slot_inputs.push(input);
            if found {
                let object = self
                    .require(current, ObjectKind::TemporaryTeamSlot)?
                    .ok_or(Error::InvalidProfile("missing temporary team slot"))?;
                let count = memory.read_u64(at(current, TEAM_SLOT_COUNT_OFFSET)?)?;
                let role = memory.read_u8(at(current, TEAM_SLOT_ROLE_OFFSET)?)?;
                let (array, first_value) = if count != 0 {
                    let array = memory.read_u64(at(current, TEAM_SLOT_ARRAY_OFFSET)?)?;
                    if array == 0 {
                        return Err(Error::InvalidProfile("missing temporary team array"));
                    }
                    at(array, 8)?;
                    let value = memory.read_u64(array)?;
                    if value == 0 {
                        return Err(Error::InvalidProfile("missing temporary team value"));
                    }
                    (
                        Some(array),
                        Some((value, memory.read_u8(at(value, TEAM_VALUE_OFFSET)?)?)),
                    )
                } else {
                    (None, None)
                };
                let team = first_value.map_or(0, |(_, value)| value);
                result.effective_team = remap_temporary(team, role);
                result.temporary_slot = Some(object);
                result.temporary_input = Some(TemporaryInput {
                    object,
                    count,
                    array,
                    first_value,
                    role,
                });
                break;
            }
            current = next;
        }
        if self.character_identity(target.address, target.epoch)? != Some(target) {
            return Err(Error::InvalidProfile("effective team identity changed"));
        }
        Ok(Some(result))
    }

    /// Read one directed cell of the live 79x79 matrix. Known exact types plus
    /// their immutable dispatch slots determine native [1,0,0] hostility.
    /// Missing cell returns None; unknown types/functions and visible changes fail.
    pub fn team_relation(&self, subject: u8, other: u8) -> Result<Option<TeamRelationObservation>> {
        valid_team(subject)?;
        valid_team(other)?;
        let memory = self.module.memory();
        let offset = (u32::from(subject) * TEAM_COUNT + u32::from(other)) * 8;
        let cell = at(at(self.module.base(), RELATION_MATRIX_RVA)?, offset)?;
        let address = pointer(memory.read_u64(cell)?)?;
        let Some(object) = self.identify(address)? else {
            if memory.read_u64(cell)? != 0 {
                return Err(Error::InvalidProfile("relation cell changed"));
            }
            return Ok(None);
        };
        let (method_rva, hostile) = match object.kind {
            ObjectKind::TeamNeutral => (0x51b580, false),
            ObjectKind::TeamFriend => (0x51b590, false),
            ObjectKind::TeamEnemy => (0x51b5a0, true),
            ObjectKind::TeamRival => (0x51b5b0, true),
            _ => return Err(Error::InvalidProfile("foreign team relation object")),
        };
        let table = memory.read_u64(address)?;
        let method = memory.read_u64(table)?;
        if method != at(self.module.base(), method_rva)?
            || memory.read_u64(cell)? != address
            || memory.read_u64(address)? != table
            || memory.read_u64(table)? != method
            || self.identify(address)? != Some(object)
        {
            return Err(Error::InvalidProfile("team relation dispatch changed"));
        }
        Ok(Some(TeamRelationObservation {
            module_base: self.module.base(),
            subject_team: subject,
            other_team: other,
            object,
            hostile,
        }))
    }
}
