//! Experimental bounded Goal tree observations from a selected ComThink root.
//! Native update processes the first child; observed descendants are not all active.
use crate::{
    goal_children::GoalChildrenSnapshot,
    goal_roots::{CONTEXT_SLOT_COUNT, CONTEXT_SLOT_STRIDE, CharacterGoalRoot},
    reader::{NightreignReader, ReaderCore},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
use std::collections::BTreeSet;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalTreeLimits {
    pub max_nodes: u8,
    pub max_depth: u8,
    pub max_child_slots: usize,
}
impl Default for GoalTreeLimits {
    fn default() -> Self {
        Self {
            max_nodes: 32,
            max_depth: 31,
            max_child_slots: 32,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalTreeNode {
    pub pool_slot_index: u8,
    pub depth: u8,
    pub raw_goal_id: i32,
    pub parent_context_address: u64,
    pub children: GoalChildrenSnapshot,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalTreeSnapshot {
    pub root: CharacterGoalRoot,
    /// Preorder; native slot order retained. Null slots remain in each node's children.
    pub nodes: Vec<GoalTreeNode>,
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// Copies descendants in the selected root's 32-slot pool. Bounds, cycles,
    /// shared nodes, foreign pools/owners and changing observations fail without
    /// returning a partial tree. No native creation, release or execution is called.
    pub fn character_goal_tree(
        &self,
        character: u64,
        limits: GoalTreeLimits,
    ) -> Result<Option<GoalTreeSnapshot>> {
        self.core().character_goal_tree(character, limits)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn character_goal_tree(
        &self,
        character: u64,
        limits: GoalTreeLimits,
    ) -> Result<Option<GoalTreeSnapshot>> {
        if !(1..=CONTEXT_SLOT_COUNT).contains(&limits.max_nodes)
            || limits.max_depth >= CONTEXT_SLOT_COUNT
            || !(1..=4096).contains(&limits.max_child_slots)
        {
            return Err(Error::InvalidProfile("invalid NR Goal tree limits"));
        }
        let Some(root) = self.character_goal_root(character)? else {
            return Ok(None);
        };
        let mut pending = Vec::new();
        if let Some(selected) = root.selected {
            pending.push((selected.control.context_address, None, 0u8));
        }
        let mut seen = BTreeSet::new();
        let mut nodes = Vec::new();
        while let Some((context, expected_parent, depth)) = pending.pop() {
            if depth > limits.max_depth || nodes.len() >= limits.max_nodes as usize {
                return Err(Error::InvalidProfile(
                    "NR Goal tree exceeds depth/node bound",
                ));
            }
            if !seen.insert(context) {
                return Err(Error::InvalidProfile(
                    "NR Goal tree has a cycle or shared node",
                ));
            }
            let offset = context
                .checked_sub(root.context_pool_address)
                .ok_or(Error::InvalidProfile("NR Goal tree node precedes pool"))?;
            if offset >= CONTEXT_SLOT_COUNT as u64 * CONTEXT_SLOT_STRIDE
                || !offset.is_multiple_of(CONTEXT_SLOT_STRIDE)
            {
                return Err(Error::InvalidProfile(
                    "NR Goal tree node is outside context pool",
                ));
            }
            let children = self.goal_children(context, limits.max_child_slots)?;
            if children.parent.raw_parent_address != root.association.think_address {
                return Err(Error::InvalidProfile("NR Goal tree Think owner differs"));
            }
            let id = self.u32(context, 0xa0)? as i32;
            let parent = self.u64(context, 8)?;
            if expected_parent.is_some_and(|expected| parent != expected) {
                return Err(Error::InvalidProfile("NR Goal tree parent link differs"));
            }
            if depth == 0
                && root.selected.is_some_and(|selected| {
                    children.parent != selected.control || id != selected.raw_goal_id
                })
            {
                return Err(Error::InvalidProfile(
                    "NR selected Goal changed during tree copy",
                ));
            }
            for slot in children.slots.iter().rev() {
                if let Some(child) = slot.context {
                    pending.push((child.control.context_address, Some(context), depth + 1));
                }
            }
            nodes.push(GoalTreeNode {
                pool_slot_index: (offset / CONTEXT_SLOT_STRIDE) as u8,
                depth,
                raw_goal_id: id,
                parent_context_address: parent,
                children,
            });
        }
        for node in &nodes {
            let context = node.children.parent.context_address;
            if self.goal_children(context, limits.max_child_slots)? != node.children
                || self.u32(context, 0xa0)? as i32 != node.raw_goal_id
                || self.u64(context, 8)? != node.parent_context_address
            {
                return Err(Error::InvalidProfile(
                    "NR Goal tree node changed during copy",
                ));
            }
        }
        if self.character_goal_root(character)? != Some(root) {
            return Err(Error::InvalidProfile(
                "NR Goal tree root association changed",
            ));
        }
        Ok(Some(GoalTreeSnapshot { root, nodes }))
    }
}
