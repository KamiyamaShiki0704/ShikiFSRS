//! Bounded copies of the native manager's settings allocation, without layout casts.
use crate::objects::GameObjectCatalog;
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const SETTINGS_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x254650,
        length: 0x5d1,
    },
    CodeRange {
        rva: 0x25be50,
        length: 0x76,
    },
    CodeRange {
        rva: 0x25c120,
        length: 0x3f7,
    },
];
const GAME_DATA_GLOBAL: u32 = 0x3d61f98;
const SETTINGS_LINK: u32 = 0x58;
const SETTINGS_CAPACITY: u32 = 0x140;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSettingsObservation {
    pub module_base: u64,
    pub epoch: u64,
    pub manager_address: u64,
    pub settings_address: u64,
    /// Requested prefix only. Unknown settings bytes remain raw; no padding is interpreted.
    pub bytes: Vec<u8>,
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Native 254650 allocates 140 bytes and publishes the 25be50 result at manager58.
    /// 25be50 / 25c120 initialize this allocation. The naked manager and settings
    /// have no primary RTTI table. Repeated copies detect visible changes, not ABA,
    /// lifetime ownership, persistence, render completion or a writable phase.
    pub fn game_settings_prefix(
        &self,
        epoch: u64,
        length: usize,
    ) -> Result<Option<GameSettingsObservation>> {
        if epoch == 0 || length == 0 || length > SETTINGS_CAPACITY as usize {
            return Err(Error::InvalidProfile("invalid settings prefix scope"));
        }
        let manager = self.module.global_pointer(GAME_DATA_GLOBAL)?;
        let Some(manager) = manager else {
            if self.module.global_pointer(GAME_DATA_GLOBAL)?.is_some() {
                return Err(Error::InvalidProfile("settings manager appeared"));
            }
            return Ok(None);
        };
        if !manager.is_multiple_of(8) {
            return Err(Error::InvalidProfile("unaligned settings manager"));
        }
        let link = manager
            .checked_add(u64::from(SETTINGS_LINK))
            .ok_or(Error::AddressOverflow)?;
        let memory = self.module.memory();
        let address = memory.read_u64(link)?;
        if address == 0 {
            if memory.read_u64(link)? != 0
                || self.module.global_pointer(GAME_DATA_GLOBAL)? != Some(manager)
            {
                return Err(Error::InvalidProfile("settings allocation appeared"));
            }
            return Ok(None);
        }
        if !address.is_multiple_of(8) {
            return Err(Error::InvalidProfile("unaligned settings allocation"));
        }
        let mut bytes = vec![0; length];
        let mut repeated = vec![0; length];
        memory.read_exact(address, &mut bytes)?;
        memory.read_exact(address, &mut repeated)?;
        if bytes != repeated
            || memory.read_u64(link)? != address
            || self.module.global_pointer(GAME_DATA_GLOBAL)? != Some(manager)
        {
            return Err(Error::InvalidProfile("settings prefix changed during copy"));
        }
        Ok(Some(GameSettingsObservation {
            module_base: self.module.base(),
            epoch,
            manager_address: manager,
            settings_address: address,
            bytes,
        }))
    }
}
