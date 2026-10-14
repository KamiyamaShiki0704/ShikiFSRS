//! Experimental resource counters from NR native initialization and clamping paths.
use crate::{
    character_modules::{ModuleKind, ModuleSnapshot},
    reader::{NightreignReader, ReaderCore},
    roots::RootKind,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceCounter {
    pub current: i32,
    pub effective_limit: i32,
    pub native_reference: i32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterAttributes {
    pub module: ModuleSnapshot,
    pub health: ResourceCounter,
    pub focus: ResourceCounter,
    pub stamina: ResourceCounter,
    /// An intermediate health value; no gameplay meaning is assigned to this word.
    pub health_word_148: i32,
}
impl CharacterAttributes {
    pub fn health_depleted(self) -> bool {
        self.health.current <= 0
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterState {
    pub attributes: CharacterAttributes,
    pub is_local_player: Option<bool>,
}

impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    pub fn character_attributes(&self, character: u64) -> Result<Option<CharacterAttributes>> {
        self.core().character_attributes(character)
    }
    pub fn character_state(&self, character: u64) -> Result<Option<CharacterState>> {
        self.core().character_state(character)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn character_attributes(
        &self,
        character: u64,
    ) -> Result<Option<CharacterAttributes>> {
        let Some(module) = self.character_module(character, ModuleKind::Data)? else {
            return Ok(None);
        };
        let address = module
            .module_address
            .checked_add(0x140)
            .ok_or(Error::AddressOverflow)?;
        let mut bytes = [0; 40];
        self.memory.read_exact(address, &mut bytes)?;
        let words: [i32; 10] = std::array::from_fn(|i| {
            i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("checked width"))
        });
        let mut repeated = [0; 40];
        self.memory.read_exact(address, &mut repeated)?;
        if repeated != bytes || self.character_module(character, ModuleKind::Data)? != Some(module)
        {
            return Err(Error::InvalidProfile(
                "NR character attribute graph changed",
            ));
        }
        Ok(Some(CharacterAttributes {
            module,
            health: ResourceCounter {
                current: words[0],
                effective_limit: words[1],
                native_reference: words[3],
            },
            health_word_148: words[2],
            focus: ResourceCounter {
                current: words[4],
                effective_limit: words[5],
                native_reference: words[6],
            },
            stamina: ResourceCounter {
                current: words[7],
                effective_limit: words[8],
                native_reference: words[9],
            },
        }))
    }
    pub(crate) fn character_state(&self, character: u64) -> Result<Option<CharacterState>> {
        let Some(attributes) = self.character_attributes(character)? else {
            return Ok(None);
        };
        let is_local_player = if let Some(world) = self.root(RootKind::World)? {
            let local = self.u64(world, 0x174e8)?;
            self.unchanged_u64(world, 0x174e8, local)?;
            if self.root(RootKind::World)? != Some(world) {
                return Err(Error::InvalidProfile(
                    "NR world changed during identity lookup",
                ));
            }
            Some(local == character)
        } else {
            None
        };
        if self.character_module(character, ModuleKind::Data)? != Some(attributes.module) {
            return Err(Error::InvalidProfile(
                "NR data module changed during state copy",
            ));
        }
        Ok(Some(CharacterState {
            attributes,
            is_local_player,
        }))
    }
}
