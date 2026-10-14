//! Experimental reverse association for the exact NR ComManipulator implementation.
//! Other manipulator kinds and discovery of a Goal context remain unexposed.
use crate::{
    reader::{ENEMY_TABLE, NightreignReader, PLAYER_TABLE, ReaderCore},
    think_actions::AI_FUNC_TABLE,
    think_targets::SELF_TARGET_TABLE,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
pub(crate) const COM_MANIPULATOR_TABLE: u32 = 0x2c09558;
pub(crate) const COM_OWNER_TABLE: u32 = 0x2bf9860;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterThinkAssociation {
    pub character_address: u64,
    pub manipulator_address: u64,
    pub owner_interface_address: u64,
    pub think_address: u64,
    pub ai_interface_address: u64,
    pub self_target_address: u64,
    pub handle_address: u64,
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// Enemy uses +0x5a0; Player uses +0x5a8. No manipulator/Think returns None.
    /// A different manipulator type or partially initialized association fails.
    /// Returns a copied, fully reciprocal association, not a lifetime or phase token.
    pub fn character_com_think(&self, character: u64) -> Result<Option<CharacterThinkAssociation>> {
        self.core().character_com_think(character)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn character_com_think(
        &self,
        character: u64,
    ) -> Result<Option<CharacterThinkAssociation>> {
        let class = self.check_type(character, &[ENEMY_TABLE, PLAYER_TABLE])?;
        let offset = if class == ENEMY_TABLE { 0x5a0 } else { 0x5a8 };
        let manipulator = self.u64(character, offset)?;
        if manipulator == 0 {
            self.unchanged_u64(character, offset, 0)?;
            self.unchanged_u64(character, 0, self.base + class as u64)?;
            return Ok(None);
        }
        self.check_type(manipulator, &[COM_MANIPULATOR_TABLE])?;
        if self.u64(manipulator, 0xa8)? != character {
            return Err(Error::InvalidProfile(
                "NR ComManipulator character owner differs",
            ));
        }
        let owner = manipulator
            .checked_add(0xc8)
            .ok_or(Error::AddressOverflow)?;
        self.check_type(owner, &[COM_OWNER_TABLE])?;
        if self.u64(owner, 8)? != manipulator {
            return Err(Error::InvalidProfile(
                "NR ComThinkOwner manipulator differs",
            ));
        }
        let think = self.u64(manipulator, 0xc0)?;
        let association = if think == 0 {
            None
        } else {
            if !think.is_multiple_of(8) || self.u64(think, 0)? != owner {
                return Err(Error::InvalidProfile("NR Think owner interface differs"));
            }
            let ai = self.u64(think, 8)?;
            self.check_type(ai, &[AI_FUNC_TABLE])?;
            if self.u64(ai, 8)? != think {
                return Err(Error::InvalidProfile("NR Think AI interface owner differs"));
            }
            let target = think.checked_add(0xc958).ok_or(Error::AddressOverflow)?;
            self.check_type(target, &[SELF_TARGET_TABLE])?;
            let handle = self.u64(target, 8)?;
            if handle == 0 || !handle.is_multiple_of(8) || self.u64(handle, 0)? != character {
                return Err(Error::InvalidProfile(
                    "NR Think self-target character differs",
                ));
            }
            if self.u64(character, 0x18)? != handle {
                return Err(Error::InvalidProfile("NR Think character handle differs"));
            }
            self.unchanged_u64(character, 0x18, handle)?;
            self.unchanged_u64(handle, 0, character)?;
            self.unchanged_u64(target, 8, handle)?;
            self.check_type(target, &[SELF_TARGET_TABLE])?;
            self.unchanged_u64(ai, 8, think)?;
            self.check_type(ai, &[AI_FUNC_TABLE])?;
            self.unchanged_u64(think, 8, ai)?;
            self.unchanged_u64(think, 0, owner)?;
            Some(CharacterThinkAssociation {
                character_address: character,
                manipulator_address: manipulator,
                owner_interface_address: owner,
                think_address: think,
                ai_interface_address: ai,
                self_target_address: target,
                handle_address: handle,
            })
        };
        self.unchanged_u64(manipulator, 0xc0, think)?;
        self.unchanged_u64(owner, 8, manipulator)?;
        self.check_type(owner, &[COM_OWNER_TABLE])?;
        self.unchanged_u64(manipulator, 0xa8, character)?;
        self.check_type(manipulator, &[COM_MANIPULATOR_TABLE])?;
        self.unchanged_u64(character, offset, manipulator)?;
        self.unchanged_u64(character, 0, self.base + class as u64)?;
        Ok(association)
    }
}
