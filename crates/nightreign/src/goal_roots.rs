//! Experimental copied selection from the Goal manager embedded at Think+0x40.
//! Selection and published request fields do not prove action execution.
use crate::{
    character_think::CharacterThinkAssociation,
    goal_actions::GoalActionState,
    reader::{NightreignReader, ReaderCore},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
pub const CONTEXT_SLOT_COUNT: u8 = 32;
pub const CONTEXT_SLOT_STRIDE: u64 = 0x3d0;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedGoalContext {
    pub slot_index: u8,
    /// Context +0xa0; separate from the action-request ID in its controller.
    pub raw_goal_id: i32,
    pub control: GoalActionState,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterGoalRoot {
    pub association: CharacterThinkAssociation,
    pub manager_address: u64,
    pub raw_binding_address: u64,
    pub context_pool_address: u64,
    pub raw_definition_address: u64,
    pub raw_word_10: i32,
    pub raw_word_14: i32,
    pub raw_flag: u8,
    pub selected: Option<SelectedGoalContext>,
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// None means no bound ComThink. A manager can have no selected context.
    /// Reads only the selected slot from the exact 32-slot pool. Other slots, Goal
    /// trees and definition objects are not traversed. No allocation or native calls.
    pub fn character_goal_root(&self, character: u64) -> Result<Option<CharacterGoalRoot>> {
        self.core().character_goal_root(character)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn character_goal_root(&self, character: u64) -> Result<Option<CharacterGoalRoot>> {
        let Some(association) = self.character_com_think(character)? else {
            return Ok(None);
        };
        let think = association.think_address;
        let manager = think.checked_add(0x40).ok_or(Error::AddressOverflow)?;
        if self.u64(manager, 0)? != think {
            return Err(Error::InvalidProfile("NR Goal manager Think owner differs"));
        }
        let binding = self.u64(manager, 8)?;
        let pool = self.u64(manager, 0x20)?;
        let definition = self.u64(manager, 0x28)?;
        let context = self.u64(manager, 0x30)?;
        let words_address = manager.checked_add(0x10).ok_or(Error::AddressOverflow)?;
        let mut words = [0; 9];
        self.memory.read_exact(words_address, &mut words)?;
        if pool != 0 && !pool.is_multiple_of(8) {
            return Err(Error::InvalidProfile("unaligned NR Goal context pool"));
        }
        let selected = if context == 0 {
            None
        } else {
            let offset = context.checked_sub(pool).ok_or(Error::InvalidProfile(
                "NR selected Goal precedes context pool",
            ))?;
            if pool == 0
                || offset >= CONTEXT_SLOT_COUNT as u64 * CONTEXT_SLOT_STRIDE
                || !offset.is_multiple_of(CONTEXT_SLOT_STRIDE)
            {
                return Err(Error::InvalidProfile(
                    "NR selected Goal is outside a context slot",
                ));
            }
            pool.checked_add(CONTEXT_SLOT_COUNT as u64 * CONTEXT_SLOT_STRIDE)
                .ok_or(Error::AddressOverflow)?;
            let control = self.goal_action_state(context)?;
            if control.raw_parent_address != think
                || ![0, think].contains(&control.raw_controller_link)
            {
                return Err(Error::InvalidProfile(
                    "NR selected Goal Think owner differs",
                ));
            }
            let raw_id_bits = self.u32(context, 0xa0)?;
            if self.u32(context, 0xa0)? != raw_id_bits
                || self.goal_action_state(context)? != control
            {
                return Err(Error::InvalidProfile(
                    "NR selected Goal changed during copy",
                ));
            }
            Some(SelectedGoalContext {
                slot_index: (offset / CONTEXT_SLOT_STRIDE) as u8,
                raw_goal_id: raw_id_bits as i32,
                control,
            })
        };
        let mut repeated = [0; 9];
        self.memory.read_exact(words_address, &mut repeated)?;
        if repeated != words {
            return Err(Error::InvalidProfile(
                "NR Goal manager state changed during copy",
            ));
        }
        for (offset, value) in [
            (0, think),
            (8, binding),
            (0x20, pool),
            (0x28, definition),
            (0x30, context),
        ] {
            self.unchanged_u64(manager, offset, value)?;
        }
        if self.character_com_think(character)? != Some(association) {
            return Err(Error::InvalidProfile(
                "NR Goal root character association changed",
            ));
        }
        Ok(Some(CharacterGoalRoot {
            association,
            manager_address: manager,
            raw_binding_address: binding,
            context_pool_address: pool,
            raw_definition_address: definition,
            raw_word_10: i32::from_le_bytes(words[..4].try_into().expect("word")),
            raw_word_14: i32::from_le_bytes(words[4..8].try_into().expect("word")),
            raw_flag: words[8],
            selected,
        }))
    }
}
