//! Experimental copied NPC associations, checked against the current repository.
use crate::{
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind},
    parameters::{ParameterRow, ParameterSlot},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const NPC_ASSOCIATION_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3e6e30,
        length: 0x99f,
    },
    CodeRange {
        rva: 0x4cee70,
        length: 0x5f8,
    },
    CodeRange {
        rva: 0x650c90,
        length: 0x85f,
    },
    CodeRange {
        rva: 0xd42d50,
        length: 0x1a0,
    },
    CodeRange {
        rva: 0x3eee30,
        length: 0xc,
    },
    CodeRange {
        rva: 0x3eee40,
        length: 0x9,
    },
];
const ENEMY_NPC_HANDLE_OFFSET: u32 = 0x598;
pub(crate) const PLAYER_NPC_HANDLE_OFFSET: u32 = 0x5f0;
const NPC_SLOT: u16 = 6;
pub(crate) const RAW_CHARACTER_NUMBER_OFFSET: u32 = 0x188;
pub(crate) const RAW_KIND_OFFSET: u32 = 0x68;
pub(crate) const STORED_NPC_REQUEST_ID_OFFSET: u32 = 0x60;
const RESOURCE_NUMBER_OFFSET: u32 = 0x64;
const ENEMY_THINK_REQUEST_OFFSET: u32 = 0x5a0;

/// Select one independently traced initializer word. These words do not prove
/// a current PARAM association, a loaded resource or a local/network role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterInitializerWord {
    /// Number returned by the initializer's resource object getter. Separate
    /// from the independently mutable character-number word at 0x188.
    ResourceNumber,
    /// Enemy initializer's stored Think query. Not the resolved Think row.
    EnemyThinkRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterInitializerWordObservation {
    pub character: CharacterIdentity,
    pub field: CharacterInitializerWord,
    pub value: i32,
}

/// Original initializer NPC query copied into the base character. This can
/// differ from the association's later fallback query, and does not prove a
/// resolved PARAM row, current resource availability or engine ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterNpcRequestObservation {
    pub character: CharacterIdentity,
    pub stored_id: i32,
}

/// Native getter copies used for caller-owned selection policies. Exact object
/// class, stored kind and character number remain separate; unknown values are
/// preserved. No local/network role, initialization or engine ownership implied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterSelectionObservation {
    pub character: CharacterIdentity,
    pub object_kind: ObjectKind,
    pub raw_kind: i32,
    pub raw_character_number: u32,
}

/// Metadata copied from the current slot-6 lookup; no row storage ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NpcParameterRow {
    pub resource_address: u64,
    pub header_address: u64,
    pub row: ParameterRow,
}

/// A present native handle can have no resolved row. This is distinct from no handle.
/// The ID is the handle's last query, including native fallback to zero; it is not
/// the original spawn request, model ID, NPC think ID, or a lifetime guarantee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterNpcParameterObservation {
    pub character: CharacterIdentity,
    pub handle_address: u64,
    pub id: i32,
    pub data_version: u32,
    pub resolved: Option<NpcParameterRow>,
}

fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

fn data_version(memory: &impl AddressSpace, header: u64) -> Result<u16> {
    let mut bytes = [0; 2];
    memory.read_exact(at(header, 8)?, &mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy only the requested word with exact class and full identity rechecks.
    /// EnemyThinkRequest returns None for a supported non-enemy class without
    /// reading its enemy tail. Copies do not pin storage or prevent ABA.
    pub fn character_initializer_word(
        &self,
        target: CharacterIdentity,
        field: CharacterInitializerWord,
    ) -> Result<Option<CharacterInitializerWordObservation>> {
        if target.epoch == 0 {
            return Err(Error::InvalidProfile("zero character initializer epoch"));
        }
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile(
                "stale character initializer identity",
            ));
        }
        let object = self
            .character(target.address)?
            .ok_or(Error::InvalidProfile("initializer character disappeared"))?;
        let offset = match field {
            CharacterInitializerWord::ResourceNumber => RESOURCE_NUMBER_OFFSET,
            CharacterInitializerWord::EnemyThinkRequest if object.kind == ObjectKind::Enemy => {
                ENEMY_THINK_REQUEST_OFFSET
            }
            CharacterInitializerWord::EnemyThinkRequest => return Ok(None),
        };
        let slot = at(target.address, offset)?;
        let value = self.module.memory().read_u32(slot)? as i32;
        if self.module.memory().read_u32(slot)? as i32 != value
            || self.character(target.address)? != Some(object)
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile("character initializer word changed"));
        }
        Ok(Some(CharacterInitializerWordObservation {
            character: target,
            field,
            value,
        }))
    }

    pub fn character_npc_request_id(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterNpcRequestObservation>> {
        if target.epoch == 0 {
            return Err(Error::InvalidProfile("zero stored NPC request epoch"));
        }
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale stored NPC request identity"));
        }
        let object = self
            .character(target.address)?
            .ok_or(Error::InvalidProfile("NPC request character disappeared"))?;
        let slot = at(target.address, STORED_NPC_REQUEST_ID_OFFSET)?;
        let stored_id = self.module.memory().read_u32(slot)? as i32;
        if self.module.memory().read_u32(slot)? as i32 != stored_id
            || self.character(target.address)? != Some(object)
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile("stored NPC request changed"));
        }
        Ok(Some(CharacterNpcRequestObservation {
            character: target,
            stored_id,
        }))
    }

    /// Copy the two independent leaf getter fields with full identity and exact
    /// class rechecks. No pointers, NPC row IDs or role labels are substituted.
    pub fn character_selection(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterSelectionObservation>> {
        if target.epoch == 0 {
            return Err(Error::InvalidProfile("zero character selection epoch"));
        }
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale character selection identity"));
        }
        let object = self
            .character(target.address)?
            .ok_or(Error::InvalidProfile("selection object disappeared"))?;
        let memory = self.module.memory();
        let read = || -> Result<(i32, u32)> {
            Ok((
                memory.read_u32(at(target.address, RAW_KIND_OFFSET)?)? as i32,
                memory.read_u32(at(target.address, RAW_CHARACTER_NUMBER_OFFSET)?)?,
            ))
        };
        let (raw_kind, raw_character_number) = read()?;
        if read()? != (raw_kind, raw_character_number)
            || self.character(target.address)? != Some(object)
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile("character selection inputs changed"));
        }
        Ok(Some(CharacterSelectionObservation {
            character: target,
            object_kind: object.kind,
            raw_kind,
            raw_character_number,
        }))
    }

    /// Observe a supported EnemyIns/PlayerIns NPC association using variant zero.
    /// Limit is the maximum repository rows (1..=65535). No row payload is read.
    /// Missing world/character/handle or a plain ChrIns returns None. A present
    /// unresolved handle returns Some with resolved=None. Repository mismatch,
    /// stale identity and detectable changes fail. Rechecks do not pin storage,
    /// prevent ABA or give an atomic game snapshot.
    pub fn character_npc_parameter(
        &self,
        target: CharacterIdentity,
        limit: usize,
    ) -> Result<Option<CharacterNpcParameterObservation>> {
        if !(1..=65_535).contains(&limit) {
            return Err(Error::InvalidProfile("invalid NPC association row limit"));
        }
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale NPC association identity"));
        }
        let character = self
            .character(target.address)?
            .ok_or(Error::InvalidProfile("NPC character disappeared"))?;
        let offset = match character.kind {
            ObjectKind::Enemy => ENEMY_NPC_HANDLE_OFFSET,
            ObjectKind::Player => PLAYER_NPC_HANDLE_OFFSET,
            _ => return Ok(None),
        };
        let link = at(target.address, offset)?;
        let memory = self.module.memory();
        let handle = memory.read_u64(link)?;
        if !handle.is_multiple_of(8) {
            return Err(Error::InvalidProfile("unaligned NPC parameter handle"));
        }
        let read = || -> Result<(u64, i32, u32)> {
            Ok((
                memory.read_u64(handle)?,
                memory.read_u32(at(handle, 8)?)? as i32,
                memory.read_u32(at(handle, 12)?)?,
            ))
        };
        let result = if handle == 0 {
            None
        } else {
            let fields = read()?;
            let (pointer, id, version) = fields;
            let mut resolved = None;
            if id < 0 {
                if pointer != 0 || version != 0 {
                    return Err(Error::InvalidProfile(
                        "negative NPC query has resolved metadata",
                    ));
                }
            } else {
                let slot = ParameterSlot::new(NPC_SLOT)?;
                let rows = self.parameter_rows(slot, 0, limit)?;
                let expected_version = if let Some(rows) = &rows {
                    let version = data_version(memory, rows.header_address)?;
                    if version < 9 {
                        return Err(Error::InvalidProfile(
                            "unsupported NPC parameter data version",
                        ));
                    }
                    if let Ok(index) = rows.rows.binary_search_by_key(&(id as u32), |row| row.id) {
                        resolved = Some(NpcParameterRow {
                            resource_address: rows.resource_address,
                            header_address: rows.header_address,
                            row: rows.rows[index],
                        });
                    }
                    version as u32
                } else {
                    0
                };
                if version != expected_version
                    || pointer != resolved.map_or(0, |row| row.row.address)
                {
                    return Err(Error::InvalidProfile(
                        "NPC handle differs from current repository",
                    ));
                }
                if self.parameter_rows(slot, 0, limit)? != rows {
                    return Err(Error::InvalidProfile(
                        "NPC repository changed during observation",
                    ));
                }
                if let Some(rows) = rows
                    && u32::from(data_version(memory, rows.header_address)?) != expected_version
                {
                    return Err(Error::InvalidProfile(
                        "NPC data version changed during observation",
                    ));
                }
            }
            if read()? != fields {
                return Err(Error::InvalidProfile(
                    "NPC handle changed during observation",
                ));
            }
            Some(CharacterNpcParameterObservation {
                character: target,
                handle_address: handle,
                id,
                data_version: version,
                resolved,
            })
        };
        if memory.read_u64(link)? != handle
            || self.character(target.address)? != Some(character)
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile(
                "NPC association changed during observation",
            ));
        }
        Ok(result)
    }
}
