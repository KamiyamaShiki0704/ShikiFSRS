//! Experimental copied views of independently evidenced NR objects.
//!
//! Supply a stable address space and engine-owned root addresses. No root discovery,
//! native calls, writes or Rust references to native objects are provided. Rechecks
//! reject observed changes; they do not prove quiescence, lifetime or absence of ABA.
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::VerifiedModule};

use crate::{NightreignFile, parameters::ParameterDefinition};

pub(crate) const TASK_TABLE: u32 = 0x2cdf868;
pub(crate) const GROUP_TABLE: u32 = 0x2ce3fe8;
pub(crate) const TIMELINE_TABLE: u32 = 0x2ce4038;
pub(crate) const WORLD_TABLE: u32 = 0x2c1b930;
pub(crate) const PLAYER_POOL_TABLE: u32 = 0x2c15d70;
pub(crate) const PLAYER_TABLE: u32 = 0x2c307f8;
pub(crate) const ENEMY_TABLE: u32 = 0x2c18120;
pub(crate) const CHARACTER_TABLE: u32 = 0x2c0ba90;
pub(crate) const PARAMETER_TABLE: u32 = 0x2cb1e08;
pub(crate) const PARAMETER_CAP_TABLE: u32 = 0x2cade48;

/// An owned copy; seed words retain their native numeric meaning without guessed flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskGroupSnapshot {
    pub definition_index: usize,
    pub address: u64,
    pub name: String,
    pub seed_words: [u32; 3],
    pub timeline_seed: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterKind {
    Base,
    Player,
    Enemy,
}

/// Key compared by the world player lookup at RVA 0x5197e0.
/// Its network/account meaning is not established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerDataSnapshot {
    pub address: u64,
    pub lookup_key: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterSnapshot {
    pub address: u64,
    pub kind: CharacterKind,
    pub player_data: Option<PlayerDataSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldPlayersSnapshot {
    pub world_address: u64,
    /// Native slot positions are preserved, including empty entries.
    pub slots: Vec<Option<CharacterSnapshot>>,
}

impl WorldPlayersSnapshot {
    pub fn find_by_key(&self, key: u32) -> Option<&CharacterSnapshot> {
        self.slots.iter().flatten().find(|player| {
            player
                .player_data
                .is_some_and(|data| data.lookup_key == key)
        })
    }
}

/// A typed resource reference, not a decoded PARAM row or an ownership token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterCapsSnapshot {
    pub slot: u16,
    pub name: &'static str,
    pub addresses: Vec<u64>,
}

pub struct NightreignReader<'r, 'f, M> {
    pub(crate) module: VerifiedModule<'r, 'f, M>,
    pub(crate) file: &'r NightreignFile<'f>,
}

impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    fn core(&self) -> ReaderCore<'_, M> {
        ReaderCore {
            memory: self.module.memory(),
            base: self.module.base(),
        }
    }

    /// Copies one indexed group. Null slots are reported as absent.
    pub fn task_group(&self, root: u64, index: usize) -> Result<Option<TaskGroupSnapshot>> {
        let definition = self
            .file
            .task_definitions()
            .records()
            .get(index)
            .filter(|definition| definition.registration_index == index)
            .ok_or(Error::InvalidProfile(
                "NR task group index is outside the catalog",
            ))?;
        self.core().task_group(root, index, &definition.name)
    }

    pub fn task_groups(&self, root: u64) -> Result<Vec<Option<TaskGroupSnapshot>>> {
        let mut groups = Vec::with_capacity(crate::TASK_DEFINITION_COUNT);
        for index in 0..crate::TASK_DEFINITION_COUNT {
            groups.push(self.task_group(root, index)?);
        }
        Ok(groups)
    }

    /// Copies the player pool at world + 0x9a40. Other world pools are not interpreted.
    pub fn world_players(&self, root: u64, limit: usize) -> Result<WorldPlayersSnapshot> {
        self.core().world_players(root, limit)
    }

    /// Recognizes exact base/player/enemy primary vtables; derived types fail closed.
    pub fn character(&self, address: u64) -> Result<CharacterSnapshot> {
        self.core().character(address)
    }

    /// Looks up an immutable file catalog name, then copies at most eight checked caps.
    /// Unknown names fail instead of using the native fallback slot 253.
    pub fn parameter_caps(&self, root: u64, name: &str) -> Result<ParameterCapsSnapshot> {
        let definition = self
            .file
            .parameter_definitions()
            .iter()
            .find(|definition| definition.name == name)
            .ok_or(Error::InvalidProfile("unknown NR parameter name"))?;
        self.core().parameter_caps(root, *definition)
    }
}

pub(crate) struct ReaderCore<'a, M> {
    pub memory: &'a M,
    pub base: u64,
}

fn at(address: u64, offset: u64) -> Result<u64> {
    address.checked_add(offset).ok_or(Error::AddressOverflow)
}

fn pointer(address: u64) -> Result<u64> {
    if address == 0 || !address.is_multiple_of(8) {
        return Err(Error::InvalidProfile(
            "NR object pointer is null or unaligned",
        ));
    }
    Ok(address)
}

impl<M: AddressSpace> ReaderCore<'_, M> {
    fn u64(&self, address: u64, offset: u64) -> Result<u64> {
        self.memory.read_u64(at(address, offset)?)
    }

    fn u32(&self, address: u64, offset: u64) -> Result<u32> {
        self.memory.read_u32(at(address, offset)?)
    }

    fn check_type(&self, address: u64, allowed: &[u32]) -> Result<u32> {
        pointer(address)?;
        let table = self.u64(address, 0)?;
        for &rva in allowed {
            if table == at(self.base, rva as u64)? {
                return Ok(rva);
            }
        }
        Err(Error::InvalidProfile("unexpected NR object primary vtable"))
    }

    fn unchanged_u64(&self, address: u64, offset: u64, expected: u64) -> Result<()> {
        if self.u64(address, offset)? != expected {
            return Err(Error::InvalidProfile("NR object changed during snapshot"));
        }
        Ok(())
    }

    fn group_name(&self, address: u64, expected: &str) -> Result<String> {
        let length = self.u64(address, 0x28)?;
        let capacity = self.u64(address, 0x30)?;
        if length == 0 || length > 256 || capacity < length || capacity > 4096 {
            return Err(Error::InvalidProfile("invalid NR group string bounds"));
        }
        let storage = if capacity < 8 {
            at(address, 0x18)?
        } else {
            let storage = self.u64(address, 0x18)?;
            if storage == 0 || !storage.is_multiple_of(2) {
                return Err(Error::InvalidProfile("invalid NR group string pointer"));
            }
            storage
        };
        let mut bytes = vec![0; (length as usize + 1) * 2];
        self.memory.read_exact(storage, &mut bytes)?;
        let words: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|word| u16::from_le_bytes([word[0], word[1]]))
            .collect();
        if words.last() != Some(&0) || words[..length as usize].contains(&0) {
            return Err(Error::InvalidProfile("invalid NR group string termination"));
        }
        let name = String::from_utf16(&words[..length as usize])
            .map_err(|_| Error::InvalidProfile("invalid NR group UTF-16"))?;
        if name != expected {
            return Err(Error::InvalidProfile("NR group name/index mismatch"));
        }
        self.unchanged_u64(address, 0x28, length)?;
        self.unchanged_u64(address, 0x30, capacity)?;
        if capacity >= 8 {
            self.unchanged_u64(address, 0x18, storage)?;
        }
        Ok(name)
    }

    fn task_group(&self, root: u64, index: usize, name: &str) -> Result<Option<TaskGroupSnapshot>> {
        if index >= crate::TASK_DEFINITION_COUNT {
            return Err(Error::InvalidProfile("NR group index out of range"));
        }
        self.check_type(root, &[TASK_TABLE])?;
        let offset = 8 + index as u64 * 8;
        let address = self.u64(root, offset)?;
        if address == 0 {
            return Ok(None);
        }
        let kind = self.check_type(address, &[GROUP_TABLE, TIMELINE_TABLE])?;
        let name = self.group_name(address, name)?;
        let seed_words = [
            self.u32(address, 0x48)?,
            self.u32(address, 0x4c)?,
            self.u32(address, 0x50)?,
        ];
        let timeline_seed = if kind == TIMELINE_TABLE {
            Some(self.u32(address, 0x78)?)
        } else {
            None
        };
        self.unchanged_u64(root, offset, address)?;
        self.check_type(root, &[TASK_TABLE])?;
        self.check_type(address, &[kind])?;
        Ok(Some(TaskGroupSnapshot {
            definition_index: index,
            address,
            name,
            seed_words,
            timeline_seed,
        }))
    }

    fn character(&self, address: u64) -> Result<CharacterSnapshot> {
        let table = self.check_type(address, &[CHARACTER_TABLE, PLAYER_TABLE, ENEMY_TABLE])?;
        let kind = match table {
            PLAYER_TABLE => CharacterKind::Player,
            ENEMY_TABLE => CharacterKind::Enemy,
            _ => CharacterKind::Base,
        };
        let player_data = if kind == CharacterKind::Player {
            let data = self.u64(address, 0x5a0)?;
            let copied = if data == 0 {
                None
            } else {
                pointer(data)?;
                Some(PlayerDataSnapshot {
                    address: data,
                    lookup_key: self.u32(data, 8)?,
                })
            };
            self.unchanged_u64(address, 0x5a0, data)?;
            copied
        } else {
            None
        };
        self.check_type(address, &[table])?;
        Ok(CharacterSnapshot {
            address,
            kind,
            player_data,
        })
    }

    fn world_players(&self, root: u64, limit: usize) -> Result<WorldPlayersSnapshot> {
        if limit == 0 || limit > 4096 {
            return Err(Error::InvalidProfile(
                "NR player snapshot limit must be 1..=4096",
            ));
        }
        self.check_type(root, &[WORLD_TABLE])?;
        let pool = at(root, 0x9a40)?;
        self.check_type(pool, &[PLAYER_POOL_TABLE])?;
        let count = self.u32(pool, 0x10)? as usize;
        if count > limit {
            return Err(Error::InvalidProfile(
                "NR player pool exceeds snapshot limit",
            ));
        }
        let storage = self.u64(pool, 0x18)?;
        if count > 0 {
            pointer(storage)?;
        }
        let mut slots = Vec::with_capacity(count);
        for index in 0..count {
            let offset = index as u64 * 16;
            let address = self.u64(storage, offset)?;
            let character = if address == 0 {
                None
            } else {
                let character = self.character(address)?;
                if character.kind != CharacterKind::Player {
                    return Err(Error::InvalidProfile(
                        "NR player pool contains a non-player",
                    ));
                }
                Some(character)
            };
            self.unchanged_u64(storage, offset, address)?;
            slots.push(character);
        }
        if self.u32(pool, 0x10)? as usize != count {
            return Err(Error::InvalidProfile(
                "NR player count changed during snapshot",
            ));
        }
        self.unchanged_u64(pool, 0x18, storage)?;
        self.check_type(pool, &[PLAYER_POOL_TABLE])?;
        self.check_type(root, &[WORLD_TABLE])?;
        Ok(WorldPlayersSnapshot {
            world_address: root,
            slots,
        })
    }

    fn parameter_caps(
        &self,
        root: u64,
        definition: ParameterDefinition,
    ) -> Result<ParameterCapsSnapshot> {
        if definition.slot >= 253 {
            return Err(Error::InvalidProfile(
                "NR fallback parameter slot is not named",
            ));
        }
        self.check_type(root, &[PARAMETER_TABLE])?;
        let offset = definition.slot as u64 * 72;
        let count = self.u32(root, offset + 0x80)? as usize;
        if count > 8 {
            return Err(Error::InvalidProfile(
                "NR parameter cap count exceeds eight",
            ));
        }
        let mut addresses = Vec::with_capacity(count);
        for index in 0..count {
            let slot = offset + 0x88 + index as u64 * 8;
            let address = self.u64(root, slot)?;
            self.check_type(address, &[PARAMETER_CAP_TABLE])?;
            self.unchanged_u64(root, slot, address)?;
            addresses.push(address);
        }
        if self.u32(root, offset + 0x80)? as usize != count {
            return Err(Error::InvalidProfile(
                "NR parameter cap count changed during snapshot",
            ));
        }
        self.check_type(root, &[PARAMETER_TABLE])?;
        Ok(ParameterCapsSnapshot {
            slot: definition.slot,
            name: definition.name,
            addresses,
        })
    }
}

#[cfg(test)]
mod tests;
