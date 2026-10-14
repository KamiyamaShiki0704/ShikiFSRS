//! Experimental direct Goal child-slot copies. No recursive traversal or mutation.
use crate::{
    goal_actions::GoalActionState,
    reader::{NightreignReader, ReaderCore},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalChildContext {
    pub control: GoalActionState,
    pub raw_goal_id: i32,
    /// Context +8 is the parent installed by AddSubGoal; checked against this container.
    /// Copying the link does not establish a lifetime or engine phase.
    pub raw_context_link_08: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalChildSlot {
    pub logical_index: u64,
    pub pointer_slot_address: u64,
    /// Null native slots are retained in their original positions.
    pub context: Option<GoalChildContext>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalChildrenSnapshot {
    pub parent: GoalActionState,
    pub block_map_address: u64,
    pub block_count: u64,
    pub start_index: u64,
    pub slots: Vec<GoalChildSlot>,
}
impl GoalChildrenSnapshot {
    /// Mirrors the first-slot behavior of RVA 0x2d09f0, including a null first slot.
    pub fn first_context(&self) -> Option<&GoalChildContext> {
        self.slots.first().and_then(|slot| slot.context.as_ref())
    }
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// Reads at most 4096 direct slots using the native two-pointer block formula.
    /// Requires exact Goal headers and the same copied Think owner for each child.
    /// The provider must preserve object/container lifetimes; repeated observations
    /// detect changes but do not establish a quiescent phase or prevent ABA reuse.
    pub fn goal_children(&self, context: u64, limit: usize) -> Result<GoalChildrenSnapshot> {
        self.core().goal_children(context, limit)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn goal_children(&self, context: u64, limit: usize) -> Result<GoalChildrenSnapshot> {
        if !(1..=4096).contains(&limit) {
            return Err(Error::InvalidProfile(
                "NR Goal child limit must be 1..=4096",
            ));
        }
        let parent = self.goal_action_state(context)?;
        let map = self.u64(context, 0x40)?;
        let blocks = self.u64(context, 0x48)?;
        let start = self.u64(context, 0x50)?;
        let count = self.u64(context, 0x58)?;
        if count > limit as u64 {
            return Err(Error::InvalidProfile("NR Goal child count exceeds bound"));
        }
        if count != 0
            && (map == 0
                || !map.is_multiple_of(8)
                || !blocks.is_power_of_two()
                || blocks > 4096
                || count > blocks * 2)
        {
            return Err(Error::InvalidProfile("invalid NR Goal child block map"));
        }
        let end = start.checked_add(count).ok_or(Error::AddressOverflow)?;
        let mut slots = Vec::new();
        let mut observations = Vec::new();
        for logical_index in start..end {
            let map_offset = ((logical_index >> 1) & (blocks - 1)) * 8;
            let block = self.u64(map, map_offset)?;
            if block == 0 || !block.is_multiple_of(8) {
                return Err(Error::InvalidProfile("invalid NR Goal child block pointer"));
            }
            let pointer_offset = (logical_index & 1) * 8;
            let address = block
                .checked_add(pointer_offset)
                .ok_or(Error::AddressOverflow)?;
            let child = self.u64(block, pointer_offset)?;
            let child_context = if child == 0 {
                None
            } else {
                if child == context {
                    return Err(Error::InvalidProfile("NR Goal is its own direct child"));
                }
                let control = self.goal_action_state(child)?;
                if control.raw_parent_address != parent.raw_parent_address
                    || ![0, parent.raw_parent_address].contains(&control.raw_controller_link)
                {
                    return Err(Error::InvalidProfile("NR Goal child Think owner differs"));
                }
                let goal_id = self.u32(child, 0xa0)?;
                let link = self.u64(child, 8)?;
                if link != context {
                    return Err(Error::InvalidProfile("NR Goal child parent link differs"));
                }
                Some(GoalChildContext {
                    control,
                    raw_goal_id: goal_id as i32,
                    raw_context_link_08: link,
                })
            };
            observations.push((map_offset, block, address, child));
            slots.push(GoalChildSlot {
                logical_index,
                pointer_slot_address: address,
                context: child_context,
            });
        }
        for ((map_offset, block, address, child), slot) in observations.into_iter().zip(&slots) {
            self.unchanged_u64(map, map_offset, block)?;
            self.unchanged_u64(address, 0, child)?;
            if let Some(copied) = slot.context {
                if self.u32(child, 0xa0)? != copied.raw_goal_id as u32
                    || self.goal_action_state(child)? != copied.control
                {
                    return Err(Error::InvalidProfile(
                        "NR child Goal state/ID changed during copy",
                    ));
                }
                self.unchanged_u64(child, 8, copied.raw_context_link_08)?;
            }
        }
        for (offset, value) in [(0x40, map), (0x48, blocks), (0x50, start), (0x58, count)] {
            self.unchanged_u64(context, offset, value)?;
        }
        if self.goal_action_state(context)? != parent {
            return Err(Error::InvalidProfile(
                "NR parent Goal changed during child copy",
            ));
        }
        Ok(GoalChildrenSnapshot {
            parent,
            block_map_address: map,
            block_count: blocks,
            start_index: start,
            slots,
        })
    }
}
