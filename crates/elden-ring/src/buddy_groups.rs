//! Independent bounded copies of the summon manager's signed-key tree and member lists.
//! Copied storage establishes no engine references, lifetime, ownership or summon completion.
use crate::{
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind, ObjectObservation},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
use std::collections::BTreeSet;

pub(crate) const BUDDY_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x508960,
        length: 0x8,
    },
    CodeRange {
        rva: 0x4b6170,
        length: 0x32a,
    },
    CodeRange {
        rva: 0x4b7160,
        length: 0xda,
    },
    CodeRange {
        rva: 0x4b7240,
        length: 0xab,
    },
    CodeRange {
        rva: 0x4b7b10,
        length: 0xee,
    },
    CodeRange {
        rva: 0x4b7c00,
        length: 0x9b,
    },
    CodeRange {
        rva: 0x4b7930,
        length: 0x9e,
    },
    CodeRange {
        rva: 0x4b9b90,
        length: 0x4c,
    },
    CodeRange {
        rva: 0x4b86d0,
        length: 0x72e,
    },
    CodeRange {
        rva: 0x4b8160,
        length: 0x16b,
    },
    CodeRange {
        rva: 0x4bbdd0,
        length: 0xb79,
    },
];
const WORLD_MANAGER_OFFSET: u32 = 0x1e538;
const WORLD_SET_OFFSET: u32 = 0x10f90;
const MANAGER_SET_OFFSET: u32 = 0x30;
const MANAGER_TREE_OFFSET: u32 = 0x70;
const REQUEST_OFFSET: u32 = 0x20;
const COOLDOWN_OFFSET: u32 = 0x90;
const TREE_HEAD_OFFSET: u32 = 0x8;
const TREE_COUNT_OFFSET: u32 = 0x10;
const NODE_PARENT_OFFSET: u32 = 0x8;
const NODE_RIGHT_OFFSET: u32 = 0x10;
const NODE_NIL_OFFSET: u32 = 0x19;
const NODE_KEY_OFFSET: u32 = 0x20;
const NODE_LIST_OFFSET: u32 = 0x30;
const NODE_COUNT_OFFSET: u32 = 0x38;
const MEMBER_PREVIOUS_OFFSET: u32 = 0x8;
const MEMBER_CHARACTER_OFFSET: u32 = 0x10;
const MEMBER_PARAM_OFFSET: u32 = 0x1c;
const MEMBER_FLAG_OFFSET: u32 = 0x31;
const MAX_NODES: u32 = 4096;

fn at(base: u64, offset: u32) -> Result<u64> {
    base.checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}
fn pointer(value: u64) -> Result<u64> {
    if value == 0 || !value.is_multiple_of(8) {
        return Err(Error::InvalidProfile("null or unaligned buddy graph link"));
    }
    Ok(value)
}
fn limit(value: usize) -> Result<()> {
    if !(1..=MAX_NODES as usize).contains(&value) {
        return Err(Error::InvalidProfile("invalid buddy graph limit"));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuddyControlObservation {
    pub module_base: u64,
    pub world: ObjectObservation,
    pub epoch: u64,
    /// Opaque nonpolymorphic allocation, not a fabricated RTTI object or owner.
    pub manager_address: u64,
    pub set: ObjectObservation,
    /// Native update consumes nonnegative requests once and then stores -1.
    pub request_identifier: i32,
    /// Original native f32 word; storage success does not establish summon success.
    pub item_cooldown_bits: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuddyMemberObservation {
    pub node_address: u64,
    pub next_address: u64,
    pub previous_address: u64,
    pub native_character_address: u64,
    pub character: Option<CharacterIdentity>,
    pub buddy_param_id: i32,
    /// 4b8160 sets this byte before submitting retirement events; nonzero excludes
    /// the 4b7b10/4b8eb0 active branch. This does not prove completed disappearance.
    /// Unknown byte values remain intact, with no invented native bool representation.
    pub inactive_request_byte: u8,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuddyGroupObservation {
    pub node_address: u64,
    pub left_address: u64,
    pub parent_address: u64,
    pub right_address: u64,
    /// Signed native tree key; no inferred player/network ownership classification.
    pub native_key: i32,
    pub list_head: u64,
    pub members: Vec<BuddyMemberObservation>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuddyGroupsObservation {
    pub control: BuddyControlObservation,
    pub tree_head: u64,
    pub root_address: u64,
    pub first_address: u64,
    pub last_address: u64,
    pub groups: Vec<BuddyGroupObservation>,
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    fn copy_buddy_control(
        &self,
        world: ObjectObservation,
        epoch: u64,
    ) -> Result<Option<BuddyControlObservation>> {
        let memory = self.module.memory();
        let manager = memory.read_u64(at(world.address, WORLD_MANAGER_OFFSET)?)?;
        if manager == 0 {
            if self.world()? != Some(world)
                || memory.read_u64(at(world.address, WORLD_MANAGER_OFFSET)?)? != 0
            {
                return Err(Error::InvalidProfile("absent buddy manager changed"));
            }
            return Ok(None);
        }
        pointer(manager)?;
        let set_address = at(world.address, WORLD_SET_OFFSET)?;
        if memory.read_u64(at(manager, MANAGER_SET_OFFSET)?)? != set_address {
            return Err(Error::InvalidProfile("buddy manager set alias changed"));
        }
        let set = self
            .require(set_address, ObjectKind::CharacterSet)?
            .ok_or(Error::InvalidProfile("buddy set absent"))?;
        let value = BuddyControlObservation {
            module_base: self.module.base(),
            world,
            epoch,
            manager_address: manager,
            set,
            request_identifier: memory.read_u32(at(manager, REQUEST_OFFSET)?)? as i32,
            item_cooldown_bits: memory.read_u32(at(manager, COOLDOWN_OFFSET)?)?,
        };
        if self.world()? != Some(world)
            || memory.read_u64(at(world.address, WORLD_MANAGER_OFFSET)?)? != manager
            || memory.read_u64(at(manager, MANAGER_SET_OFFSET)?)? != set_address
            || self.require(set_address, ObjectKind::CharacterSet)? != Some(set)
        {
            return Err(Error::InvalidProfile("buddy manager path changed"));
        }
        Ok(Some(value))
    }
    /// Copy selected manager words and exact set alias twice. No native call or write.
    pub fn buddy_control(&self, epoch: u64) -> Result<Option<BuddyControlObservation>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero buddy epoch"));
        }
        let Some(world) = self.world()? else {
            return Ok(None);
        };
        let value = self.copy_buddy_control(world, epoch)?;
        if self.copy_buddy_control(world, epoch)? != value {
            return Err(Error::InvalidProfile("buddy control changed"));
        }
        Ok(value)
    }
    fn copy_buddy_groups(
        &self,
        control: BuddyControlObservation,
        group_limit: usize,
        member_limit: usize,
    ) -> Result<BuddyGroupsObservation> {
        let memory = self.module.memory();
        let tree = at(control.manager_address, MANAGER_TREE_OFFSET)?;
        let head = pointer(memory.read_u64(at(tree, TREE_HEAD_OFFSET)?)?)?;
        let count = memory.read_u64(at(tree, TREE_COUNT_OFFSET)?)?;
        if count > group_limit as u64 || memory.read_u8(at(head, NODE_NIL_OFFSET)?)? != 1 {
            return Err(Error::InvalidProfile("invalid buddy tree head/count"));
        }
        let first = pointer(memory.read_u64(head)?)?;
        let root = pointer(memory.read_u64(at(head, NODE_PARENT_OFFSET)?)?)?;
        let last = pointer(memory.read_u64(at(head, NODE_RIGHT_OFFSET)?)?)?;
        if (count == 0) != (root == head) {
            return Err(Error::InvalidProfile("buddy tree count/root disagree"));
        }
        let mut stack = vec![(root, head, None, None)];
        let mut seen = BTreeSet::new();
        let mut all_members = BTreeSet::new();
        let mut groups = Vec::with_capacity(count as usize);
        while let Some((node, expected_parent, lower, upper)) = stack.pop() {
            if node == head {
                continue;
            }
            pointer(node)?;
            if seen.len() as u64 >= count
                || !seen.insert(node)
                || memory.read_u8(at(node, NODE_NIL_OFFSET)?)? != 0
            {
                return Err(Error::InvalidProfile("cyclic or excessive buddy tree"));
            }
            let left = pointer(memory.read_u64(node)?)?;
            let parent = pointer(memory.read_u64(at(node, NODE_PARENT_OFFSET)?)?)?;
            let right = pointer(memory.read_u64(at(node, NODE_RIGHT_OFFSET)?)?)?;
            let key = memory.read_u32(at(node, NODE_KEY_OFFSET)?)? as i32;
            if parent != expected_parent
                || lower.is_some_and(|v| key <= v)
                || upper.is_some_and(|v| key >= v)
            {
                return Err(Error::InvalidProfile(
                    "buddy tree parent or signed order invalid",
                ));
            }
            let list_head = pointer(memory.read_u64(at(node, NODE_LIST_OFFSET)?)?)?;
            let member_count = memory.read_u64(at(node, NODE_COUNT_OFFSET)?)?;
            if member_count > member_limit.saturating_sub(all_members.len()) as u64 {
                return Err(Error::InvalidProfile("excessive buddy list count"));
            }
            let mut next = pointer(memory.read_u64(list_head)?)?;
            let tail = pointer(memory.read_u64(at(list_head, MEMBER_PREVIOUS_OFFSET)?)?)?;
            let mut previous = list_head;
            let mut members = Vec::with_capacity(member_count as usize);
            while next != list_head {
                if members.len() as u64 >= member_count || !all_members.insert(next) {
                    return Err(Error::InvalidProfile("cyclic or aliased buddy list"));
                }
                let node_address = next;
                next = pointer(memory.read_u64(node_address)?)?;
                let previous_address =
                    pointer(memory.read_u64(at(node_address, MEMBER_PREVIOUS_OFFSET)?)?)?;
                if previous_address != previous {
                    return Err(Error::InvalidProfile("buddy list previous link invalid"));
                }
                let address = memory.read_u64(at(node_address, MEMBER_CHARACTER_OFFSET)?)?;
                let character = if address == 0 {
                    None
                } else {
                    let target = self
                        .character_identity(pointer(address)?, control.epoch)?
                        .ok_or(Error::InvalidProfile("buddy character absent"))?;
                    if target.world_address != control.world.address {
                        return Err(Error::InvalidProfile("buddy character world changed"));
                    }
                    Some(target)
                };
                members.push(BuddyMemberObservation {
                    node_address,
                    next_address: next,
                    previous_address,
                    native_character_address: address,
                    character,
                    buddy_param_id: memory.read_u32(at(node_address, MEMBER_PARAM_OFFSET)?)? as i32,
                    inactive_request_byte: memory.read_u8(at(node_address, MEMBER_FLAG_OFFSET)?)?,
                });
                previous = node_address;
            }
            if members.len() as u64 != member_count || previous != tail {
                return Err(Error::InvalidProfile("buddy list count/tail disagree"));
            }
            groups.push(BuddyGroupObservation {
                node_address: node,
                left_address: left,
                parent_address: parent,
                right_address: right,
                native_key: key,
                list_head,
                members,
            });
            stack.push((right, node, Some(key), upper));
            stack.push((left, node, lower, Some(key)));
        }
        groups.sort_by_key(|group| group.native_key);
        if groups.len() as u64 != count
            || groups.first().map_or(head, |g| g.node_address) != first
            || groups.last().map_or(head, |g| g.node_address) != last
        {
            return Err(Error::InvalidProfile("buddy tree count/endpoints disagree"));
        }
        if self.buddy_control(control.epoch)? != Some(control) {
            return Err(Error::InvalidProfile(
                "buddy manager changed during graph copy",
            ));
        }
        Ok(BuddyGroupsObservation {
            control,
            tree_head: head,
            root_address: root,
            first_address: first,
            last_address: last,
            groups,
        })
    }
    /// Copy the entire bounded graph twice, preserving signed-key order and list order.
    /// Limits bound groups and total members. Retain the full readable graph in a permitted
    /// exclusive phase and supply a known epoch. No protection against same-value ABA/quiescence.
    pub fn buddy_groups(
        &self,
        epoch: u64,
        group_limit: usize,
        member_limit: usize,
    ) -> Result<Option<BuddyGroupsObservation>> {
        limit(group_limit)?;
        limit(member_limit)?;
        let Some(control) = self.buddy_control(epoch)? else {
            return Ok(None);
        };
        let value = self.copy_buddy_groups(control, group_limit, member_limit)?;
        if self.copy_buddy_groups(control, group_limit, member_limit)? != value {
            return Err(Error::InvalidProfile("buddy graph or identity changed"));
        }
        Ok(Some(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuddyControlWord {
    RequestIdentifier,
    ItemCooldown,
}
impl BuddyControlWord {
    fn offset(self) -> u32 {
        match self {
            Self::RequestIdentifier => REQUEST_OFFSET,
            Self::ItemCooldown => COOLDOWN_OFFSET,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuddyControlWrite {
    pub source: BuddyControlObservation,
    pub field: BuddyControlWord,
    pub destination: u64,
    pub replacement_bits: u32,
    pub stored: bool,
    pub postflight: Result<BuddyControlObservation>,
}
pub(crate) fn control_plan(
    actual: Option<BuddyControlObservation>,
    source: BuddyControlObservation,
    field: BuddyControlWord,
    replacement_bits: u32,
) -> Result<BuddyControlWrite> {
    if actual != Some(source) {
        return Err(Error::NativeOperationRejected(
            "buddy control superseded before store",
        ));
    }
    Ok(BuddyControlWrite {
        source,
        field,
        destination: at(source.manager_address, field.offset())?,
        replacement_bits,
        stored: false,
        postflight: Err(Error::NativeOperationRejected("buddy store not entered")),
    })
}
pub(crate) fn commit_control(
    mut plan: BuddyControlWrite,
    store: impl FnOnce(u64, u32),
    observe: impl FnOnce() -> Result<Option<BuddyControlObservation>>,
) -> BuddyControlWrite {
    store(plan.destination, plan.replacement_bits);
    plan.stored = true;
    let mut expected = plan.source;
    match plan.field {
        BuddyControlWord::RequestIdentifier => {
            expected.request_identifier = plan.replacement_bits as i32
        }
        BuddyControlWord::ItemCooldown => expected.item_cooldown_bits = plan.replacement_bits,
    }
    plan.postflight = observe().and_then(|actual| {
        let actual = actual.ok_or(Error::NativeOperationRejected(
            "buddy manager absent after store",
        ))?;
        if actual != expected {
            return Err(Error::NativeOperationRejected(
                "buddy control changed after store",
            ));
        }
        Ok(actual)
    });
    plan
}
