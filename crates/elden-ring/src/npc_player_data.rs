//! Engine-owned NPC PlayerGameData pool observations. Copies grant no release authority.
use crate::{
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const NPC_DATA_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x2563a0,
        length: 0x186,
    },
    CodeRange {
        rva: 0x254d10,
        length: 0x44f,
    },
    CodeRange {
        rva: 0x650c90,
        length: 0x85f,
    },
    CodeRange {
        rva: 0x257850,
        length: 0xa0,
    },
    CodeRange {
        rva: 0x6515f0,
        length: 0x391,
    },
];
const GAME_DATA_GLOBAL: u32 = 0x3d61f98;
const POOL_OFFSET: u32 = 0x28;
const IDENTIFIER_OFFSET: u32 = 8;
const PLAYER_DATA_OFFSET: u32 = 0x580;
const SLOT_COUNT: u32 = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NpcPlayerDataSlot {
    pub index: u32,
    pub data_address: u64,
    /// Raw signed word. Allocation publishes 10000000+index, but this is not a liveness flag.
    pub native_identifier: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcPlayerDataPoolObservation {
    pub module_base: u64,
    pub manager_address: u64,
    pub slots_address: u64,
    pub epoch: u64,
    pub slots: [Option<NpcPlayerDataSlot>; SLOT_COUNT as usize],
}

impl NpcPlayerDataPoolObservation {
    pub fn occupied(&self) -> impl Iterator<Item = NpcPlayerDataSlot> + '_ {
        self.slots.iter().flatten().copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerNpcDataObservation {
    pub character: CharacterIdentity,
    pub pool: NpcPlayerDataPoolObservation,
    pub slot: NpcPlayerDataSlot,
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy all forty slots, exact data types and raw identifiers twice. Missing manager
    /// or pool returns None. Empty slots remain explicit; adjacent storage is not read.
    /// Concurrent ABA, engine references, readiness and allocation ownership are not proved.
    pub fn npc_player_data_pool(&self, epoch: u64) -> Result<Option<NpcPlayerDataPoolObservation>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero NPC player data epoch"));
        }
        let manager = self.module.global_pointer(GAME_DATA_GLOBAL)?;
        let Some(manager) = manager else {
            if self.module.global_pointer(GAME_DATA_GLOBAL)?.is_some() {
                return Err(Error::InvalidProfile("NPC player data manager appeared"));
            }
            return Ok(None);
        };
        aligned(manager)?;
        let memory = self.module.memory();
        let link = at(manager, POOL_OFFSET)?;
        let slots_address = memory.read_u64(link)?;
        if slots_address == 0 {
            if memory.read_u64(link)? != 0
                || self.module.global_pointer(GAME_DATA_GLOBAL)? != Some(manager)
            {
                return Err(Error::InvalidProfile("NPC player data pool appeared"));
            }
            return Ok(None);
        }
        aligned(slots_address)?;
        let read = || self.read_npc_data_slots(slots_address, None);
        let slots = read()?;
        if read()? != slots
            || memory.read_u64(link)? != slots_address
            || self.module.global_pointer(GAME_DATA_GLOBAL)? != Some(manager)
        {
            return Err(Error::InvalidProfile(
                "NPC player data pool changed during observation",
            ));
        }
        Ok(Some(NpcPlayerDataPoolObservation {
            module_base: self.module.base(),
            manager_address: manager,
            slots_address,
            epoch,
            slots,
        }))
    }

    /// Confirm a native Player destructor removed exactly its copied NPC data slot.
    /// Never dereference the released data address, including aliases appearing on recheck.
    /// The remaining allocations must stay live under the caller's AddressSpace/lifetime
    /// contract; this grants no ownership, drain, free or unload permission.
    pub fn npc_player_data_pool_after_release(
        &self,
        before: &NpcPlayerDataPoolObservation,
        removed: NpcPlayerDataSlot,
    ) -> Result<NpcPlayerDataPoolObservation> {
        if before.module_base != self.module.base()
            || before.epoch == 0
            || before.manager_address == 0
            || before.slots_address == 0
            || removed.data_address == 0
            || !removed.data_address.is_multiple_of(8)
            || before.slots.get(removed.index as usize) != Some(&Some(removed))
        {
            return Err(Error::InvalidProfile("invalid NPC data release source"));
        }
        aligned(before.manager_address)?;
        aligned(before.slots_address)?;
        let link = at(before.manager_address, POOL_OFFSET)?;
        let memory = self.module.memory();
        if self.module.global_pointer(GAME_DATA_GLOBAL)? != Some(before.manager_address)
            || memory.read_u64(link)? != before.slots_address
        {
            return Err(Error::InvalidProfile("NPC data release pool replaced"));
        }
        let read = || self.read_npc_data_slots(before.slots_address, Some(removed.data_address));
        let slots = read()?;
        let mut expected = before.clone();
        expected.slots[removed.index as usize] = None;
        if slots != expected.slots
            || read()? != slots
            || memory.read_u64(link)? != before.slots_address
            || self.module.global_pointer(GAME_DATA_GLOBAL)? != Some(before.manager_address)
        {
            return Err(Error::InvalidProfile(
                "NPC data release did not preserve the other slots",
            ));
        }
        Ok(expected)
    }

    fn read_npc_data_slots(
        &self,
        slots_address: u64,
        released: Option<u64>,
    ) -> Result<[Option<NpcPlayerDataSlot>; SLOT_COUNT as usize]> {
        let mut pointers = [0; SLOT_COUNT as usize * 8];
        slots_address
            .checked_add(pointers.len() as u64)
            .ok_or(Error::AddressOverflow)?;
        self.module
            .memory()
            .read_exact(slots_address, &mut pointers)?;
        let mut slots = [None; SLOT_COUNT as usize];
        for (index, chunk) in pointers.chunks_exact(8).enumerate() {
            let address = u64::from_le_bytes(chunk.try_into().expect("eight byte slot"));
            if released.is_some_and(|value| address == value) {
                return Err(Error::InvalidProfile(
                    "released NPC data address remains in pool",
                ));
            }
            if self.require(address, ObjectKind::PlayerGameData)?.is_none() {
                continue;
            }
            if slots[..index]
                .iter()
                .flatten()
                .any(|slot: &NpcPlayerDataSlot| slot.data_address == address)
            {
                return Err(Error::InvalidProfile(
                    "duplicated NPC player data allocation",
                ));
            }
            slots[index] = Some(NpcPlayerDataSlot {
                index: index as u32,
                data_address: address,
                native_identifier: self
                    .module
                    .memory()
                    .read_u32(at(address, IDENTIFIER_OFFSET)?)?
                    as i32,
            });
        }
        Ok(slots)
    }

    /// Require an exact PlayerIns identity and typed +580 data link, then copy its unique
    /// NPC-pool membership. A typed data object outside the pool (including primary player
    /// data) returns None and cannot be treated as an NPC allocation. No network role implied.
    pub fn npc_player_data_for_character(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<PlayerNpcDataObservation>> {
        if self.character_identity(target.address, target.epoch)? != Some(target)
            || self.require(target.address, ObjectKind::Player)?.is_none()
        {
            return Err(Error::InvalidProfile(
                "NPC player data source is not the current PlayerIns",
            ));
        }
        let memory = self.module.memory();
        let link = at(target.address, PLAYER_DATA_OFFSET)?;
        let data = memory.read_u64(link)?;
        let typed = self.require(data, ObjectKind::PlayerGameData)?;
        let pool = self.npc_player_data_pool(target.epoch)?;
        let slot = pool
            .as_ref()
            .and_then(|pool| pool.occupied().find(|slot| slot.data_address == data));
        if memory.read_u64(link)? != data
            || self.require(data, ObjectKind::PlayerGameData)? != typed
            || self.npc_player_data_pool(target.epoch)? != pool
            || self.character_identity(target.address, target.epoch)? != Some(target)
            || self.require(target.address, ObjectKind::Player)?.is_none()
        {
            return Err(Error::InvalidProfile(
                "PlayerIns NPC data association changed during observation",
            ));
        }
        Ok(match (pool, slot) {
            (Some(pool), Some(slot)) => Some(PlayerNpcDataObservation {
                character: target,
                pool,
                slot,
            }),
            _ => None,
        })
    }
}

fn at(base: u64, offset: u32) -> Result<u64> {
    base.checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}
fn aligned(address: u64) -> Result<()> {
    if !address.is_multiple_of(8) {
        return Err(Error::InvalidProfile("unaligned NPC player data storage"));
    }
    Ok(())
}
