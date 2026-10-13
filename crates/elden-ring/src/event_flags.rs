//! Read one native event bit through the manager's typed virtual-memory-flag base.
use crate::objects::{GameObjectCatalog, ObjectKind};
use shikifsrs_runtime::{
    Error, Result, memory::AddressSpace, native_tree::U32PointerTree, profile::CodeRange,
};

pub(crate) const FLAG_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x67a6d0,
        length: 0x259,
    },
    CodeRange {
        rva: 0x5cc960,
        length: 0x45b,
    },
    CodeRange {
        rva: 0x5f9a70,
        length: 0x177,
    },
    CodeRange {
        rva: 0x5fa250,
        length: 0xa6,
    },
    CodeRange {
        rva: 0x5d2180,
        length: 0x20,
    },
    CodeRange {
        rva: 0x5fa700,
        length: 0x74,
    },
    CodeRange {
        rva: 0x5fad20,
        length: 0x4c,
    },
];
const MANAGER_GLOBAL: u32 = 0x3d6c4b8;
const GROUP_BITS_OFFSET: u32 = 0x1c;
const STRIDE_OFFSET: u32 = 0x20;
const POOL_COUNT_OFFSET: u32 = 0x24;
const POOL_OFFSET: u32 = 0x28;
const TREE_OFFSET: u32 = 0x30;
const KIND_OFFSET: u32 = 0x28;
const PAYLOAD_OFFSET: u32 = 0x30;
const EXPECTED_GROUP_BITS: u32 = 1000;
const EXPECTED_STRIDE: u32 = 125;
const STORAGE_LIMIT: u32 = 65536;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventFlagStorage {
    MissingGroup,
    /// Native getter returns false for every kind other than1/2; payload union is unread.
    Unbacked {
        native_kind: u32,
    },
    Pool {
        index: u32,
        group_address: u64,
    },
    /// Native returns false for a null pointer. A nonnull group is read at one selected byte.
    External {
        group_address: u64,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventFlagObservation {
    pub manager_address: u64,
    pub id: u32,
    pub group_id: u32,
    pub bit_in_group: u32,
    pub node_address: Option<u64>,
    pub storage: EventFlagStorage,
    pub value: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    group_bits: u32,
    stride: u32,
    pool_count: u32,
    pool: u64,
}

pub(crate) fn validate_flag_batch(ids: &[u32], node_limit: u32) -> Result<()> {
    if ids.is_empty() || ids.len() > 256 || !(1..=STORAGE_LIMIT).contains(&node_limit) {
        return Err(Error::InvalidProfile("invalid event flag batch bounds"));
    }
    Ok(())
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy a nonempty bounded ordered selection twice; duplicates and id0 are preserved.
    /// None means absent manager; valid missing/unbacked groups remain observed false.
    /// This rejects visible cross-flag changes, without an engine lock or atomicity guarantee.
    pub fn event_flags(
        &self,
        ids: &[u32],
        node_limit: u32,
    ) -> Result<Option<Vec<EventFlagObservation>>> {
        validate_flag_batch(ids, node_limit)?;
        let first = self.copy_event_flags(ids, node_limit)?;
        if self.copy_event_flags(ids, node_limit)? != first {
            return Err(Error::InvalidProfile(
                "event flag batch changed during observation",
            ));
        }
        Ok(first)
    }
    pub(crate) fn copy_event_flags(
        &self,
        ids: &[u32],
        node_limit: u32,
    ) -> Result<Option<Vec<EventFlagObservation>>> {
        let mut result = Vec::with_capacity(ids.len());
        for &id in ids {
            let Some(copy) = self.event_flag(id, node_limit)? else {
                return Ok(None);
            };
            if result.first().is_some_and(|first: &EventFlagObservation| {
                first.manager_address != copy.manager_address
            }) {
                return Err(Error::InvalidProfile("event flag batch manager changed"));
            }
            result.push(copy);
        }
        Ok(Some(result))
    }

    /// None means the manager is absent. Missing/unbacked/null groups are observed false,
    /// matching the proved core getter; id0 is allowed by that core (wrapper rejects it).
    /// Does not copy an entire bank, initialize flags, lock the engine, invoke native or write.
    /// Repeated reads reject visible changes but do not establish lifetime/atomicity/ABA safety.
    pub fn event_flag(&self, id: u32, node_limit: u32) -> Result<Option<EventFlagObservation>> {
        if !(1..=STORAGE_LIMIT).contains(&node_limit) {
            return Err(Error::InvalidProfile("invalid event flag node limit"));
        }
        let Some(manager) = self.module.global_pointer(MANAGER_GLOBAL)? else {
            return Ok(None);
        };
        self.require(manager, ObjectKind::VirtualMemoryFlags)?;
        let memory = self.module.memory();
        let header = read_header(memory, manager)?;
        if header.group_bits != EXPECTED_GROUP_BITS
            || header.stride != EXPECTED_STRIDE
            || header.pool_count > STORAGE_LIMIT
            || (header.pool_count != 0 && header.pool == 0)
        {
            return Err(Error::InvalidProfile(
                "event flag header differs from supported initialized format",
            ));
        }
        header
            .pool
            .checked_add(u64::from(header.pool_count) * u64::from(header.stride))
            .ok_or(Error::AddressOverflow)?;
        let tree = U32PointerTree::new(memory, at(manager, TREE_OFFSET)?, node_limit as usize)?;
        let group_id = id / header.group_bits;
        let bit_in_group = id % header.group_bits;
        let node = tree.find_node(group_id)?;
        let mut payload = None;
        let storage = match node {
            None => EventFlagStorage::MissingGroup,
            Some(node) => {
                if !node.is_multiple_of(8) {
                    return Err(Error::InvalidProfile("unaligned event flag node"));
                }
                let kind = memory.read_u32(at(node, KIND_OFFSET)?)?;
                match kind {
                    1 => {
                        let index = memory.read_u32(at(node, PAYLOAD_OFFSET)?)?;
                        if index >= header.pool_count {
                            return Err(Error::InvalidProfile(
                                "event flag pool index outside allocation",
                            ));
                        }
                        let offset = index
                            .checked_mul(header.stride)
                            .ok_or(Error::AddressOverflow)?;
                        let group_address = header
                            .pool
                            .checked_add(u64::from(offset))
                            .ok_or(Error::AddressOverflow)?;
                        payload = Some((kind, u64::from(index)));
                        EventFlagStorage::Pool {
                            index,
                            group_address,
                        }
                    }
                    2 => {
                        let group_address = memory.read_u64(at(node, PAYLOAD_OFFSET)?)?;
                        payload = Some((kind, group_address));
                        EventFlagStorage::External { group_address }
                    }
                    native_kind => {
                        payload = Some((native_kind, 0));
                        EventFlagStorage::Unbacked { native_kind }
                    }
                }
            }
        };
        let group_address = match storage {
            EventFlagStorage::Pool { group_address, .. }
            | EventFlagStorage::External { group_address } => group_address,
            _ => 0,
        };
        let value = if group_address == 0 {
            false
        } else {
            group_address
                .checked_add(u64::from(header.stride))
                .ok_or(Error::AddressOverflow)?;
            let byte = group_address
                .checked_add(u64::from(bit_in_group / 8))
                .ok_or(Error::AddressOverflow)?;
            let bits = memory.read_u8(byte)?;
            if memory.read_u8(byte)? != bits {
                return Err(Error::InvalidProfile("event flag byte changed during copy"));
            }
            bits & (1 << (7 - (bit_in_group % 8))) != 0
        };
        if let (Some(node), Some((kind, expected_payload))) = (node, payload) {
            if memory.read_u32(at(node, KIND_OFFSET)?)? != kind {
                return Err(Error::InvalidProfile("event flag storage kind changed"));
            }
            let actual = match kind {
                1 => u64::from(memory.read_u32(at(node, PAYLOAD_OFFSET)?)?),
                2 => memory.read_u64(at(node, PAYLOAD_OFFSET)?)?,
                _ => 0,
            };
            if actual != expected_payload {
                return Err(Error::InvalidProfile("event flag group payload changed"));
            }
        }
        if tree.find_node(group_id)? != node
            || read_header(memory, manager)? != header
            || self.module.global_pointer(MANAGER_GLOBAL)? != Some(manager)
        {
            return Err(Error::InvalidProfile(
                "event flag manager or tree changed during copy",
            ));
        }
        self.require(manager, ObjectKind::VirtualMemoryFlags)?;
        Ok(Some(EventFlagObservation {
            manager_address: manager,
            id,
            group_id,
            bit_in_group,
            node_address: node,
            storage,
            value,
        }))
    }
}
fn at(base: u64, offset: u32) -> Result<u64> {
    base.checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}
fn read_header<M: AddressSpace>(memory: &M, manager: u64) -> Result<Header> {
    Ok(Header {
        group_bits: memory.read_u32(at(manager, GROUP_BITS_OFFSET)?)?,
        stride: memory.read_u32(at(manager, STRIDE_OFFSET)?)?,
        pool_count: memory.read_u32(at(manager, POOL_COUNT_OFFSET)?)?,
        pool: memory.read_u64(at(manager, POOL_OFFSET)?)?,
    })
}
