//! Experimental copied self-target association and unfiltered effect-ID predicate.
//! Target selector -1 resolves SelfTarget; other selector routes remain unexposed.
use crate::{
    reader::{CHARACTER_TABLE, ENEMY_TABLE, NightreignReader, PLAYER_TABLE, ReaderCore},
    think_actions::AI_FUNC_TABLE,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
pub(crate) const SELF_TARGET_TABLE: u32 = 0x2bf9180;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThinkSelfTarget {
    pub context_address: u64,
    pub think_address: u64,
    pub ai_interface_address: u64,
    pub target_address: u64,
    /// Native handle storage; the first word contains the character pointer.
    pub handle_address: u64,
    pub character_address: Option<u64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThinkSelfEffectPredicate {
    pub target: ThinkSelfTarget,
    /// Exact 32-bit ID comparison; zero and negative values are retained.
    pub requested_id: i32,
    pub present: bool,
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// None means no Goal parent. A present target with no character represents
    /// a null/unbound handle. Requires exact type and reciprocal character handle.
    pub fn think_self_target(&self, context: u64) -> Result<Option<ThinkSelfTarget>> {
        self.core().think_self_target(context)
    }
    /// Copies the self-target effect-ID predicate without calling its native slot.
    /// Excluded nodes still count. Uses the same memory dependencies and bounds as
    /// special_effects(), including its PARAM state reads; malformed data fails.
    /// A target with no character or no effect object produces present=false.
    pub fn think_self_effect_predicate(
        &self,
        context: u64,
        id: i32,
        limit: usize,
    ) -> Result<Option<ThinkSelfEffectPredicate>> {
        self.core().think_self_effect_predicate(context, id, limit)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn think_self_target(&self, context: u64) -> Result<Option<ThinkSelfTarget>> {
        let goal = self.goal_action_state(context)?;
        let think = goal.raw_parent_address;
        if think == 0 {
            if self.goal_action_state(context)? != goal {
                return Err(Error::InvalidProfile("NR Goal parent changed"));
            }
            return Ok(None);
        }
        if !think.is_multiple_of(8) {
            return Err(Error::InvalidProfile("unaligned NR Think parent"));
        }
        let ai = self.u64(think, 8)?;
        self.check_type(ai, &[AI_FUNC_TABLE])?;
        if self.u64(ai, 8)? != think {
            return Err(Error::InvalidProfile("NR Think interface owner differs"));
        }
        let target = think.checked_add(0xc958).ok_or(Error::AddressOverflow)?;
        self.check_type(target, &[SELF_TARGET_TABLE])?;
        let handle = self.u64(target, 8)?;
        let character = if handle == 0 {
            None
        } else {
            if !handle.is_multiple_of(8) {
                return Err(Error::InvalidProfile("unaligned NR self-target handle"));
            }
            let pointer = self.u64(handle, 0)?;
            if pointer == 0 {
                self.unchanged_u64(handle, 0, 0)?;
                None
            } else {
                let class =
                    self.check_type(pointer, &[CHARACTER_TABLE, ENEMY_TABLE, PLAYER_TABLE])?;
                if self.u64(pointer, 0x18)? != handle {
                    return Err(Error::InvalidProfile(
                        "NR self-target character handle differs",
                    ));
                }
                self.unchanged_u64(pointer, 0x18, handle)?;
                self.unchanged_u64(pointer, 0, self.base + class as u64)?;
                self.unchanged_u64(handle, 0, pointer)?;
                Some(pointer)
            }
        };
        self.unchanged_u64(target, 8, handle)?;
        self.check_type(target, &[SELF_TARGET_TABLE])?;
        self.unchanged_u64(ai, 8, think)?;
        self.unchanged_u64(think, 8, ai)?;
        self.check_type(ai, &[AI_FUNC_TABLE])?;
        if self.goal_action_state(context)? != goal {
            return Err(Error::InvalidProfile(
                "NR self-target Goal association changed",
            ));
        }
        Ok(Some(ThinkSelfTarget {
            context_address: context,
            think_address: think,
            ai_interface_address: ai,
            target_address: target,
            handle_address: handle,
            character_address: character,
        }))
    }
    pub(crate) fn think_self_effect_predicate(
        &self,
        context: u64,
        id: i32,
        limit: usize,
    ) -> Result<Option<ThinkSelfEffectPredicate>> {
        if !(1..=4096).contains(&limit) {
            return Err(Error::InvalidProfile(
                "NR self-target effect limit must be 1..=4096",
            ));
        }
        let Some(target) = self.think_self_target(context)? else {
            return Ok(None);
        };
        let present = if let Some(character) = target.character_address {
            let effect_address = self.u64(character, 0x190)?;
            let found = if let Some(effects) = self.special_effects(character, limit)? {
                if effects.effect_address != effect_address {
                    return Err(Error::InvalidProfile(
                        "NR self-target effect pointer changed",
                    ));
                }
                if effects.owner_address != character {
                    return Err(Error::InvalidProfile("NR self-target effect owner differs"));
                }
                effects.contains_unfiltered_id(id)
            } else if effect_address == 0 {
                false
            } else {
                return Err(Error::InvalidProfile("NR self-target effect disappeared"));
            };
            self.unchanged_u64(character, 0x190, effect_address)?;
            found
        } else {
            false
        };
        if self.think_self_target(context)? != Some(target) {
            return Err(Error::InvalidProfile(
                "NR self-target changed during predicate copy",
            ));
        }
        Ok(Some(ThinkSelfEffectPredicate {
            target,
            requested_id: id,
            present,
        }))
    }
}
