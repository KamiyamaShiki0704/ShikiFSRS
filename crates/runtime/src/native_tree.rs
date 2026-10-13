//! Bounded byte-copy access to the independently verified u32-to-pointer tree format.
//! This is not a Rust view of a C++ container and does not establish object lifetime.
use crate::{Error, Result, memory::AddressSpace};
use std::collections::BTreeSet;

/// Format derived from native construction and unsigned lookup, not a library implementation.
/// Container: allocator +0, head +8, count +16. Node: left +0, parent +8,
/// right +16, nil byte +25, u32 key +32, pointer value +40.
pub struct U32PointerTree<'a, M> {
    memory: &'a M,
    address: u64,
    node_limit: usize,
}

/// Whether equal keys may occupy distinct nodes (including either side after rotations).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeKeys {
    Unique,
    Repeated,
}

/// Copied initialized node fields only; the value is never dereferenced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerTreeEntry {
    pub node: u64,
    pub key: u32,
    pub value: u64,
}

impl<'a, M: AddressSpace> U32PointerTree<'a, M> {
    pub fn new(memory: &'a M, address: u64, node_limit: usize) -> Result<Self> {
        if address == 0 || node_limit == 0 {
            return Err(Error::InvalidProfile(
                "tree address and node limit must be nonzero",
            ));
        }
        Ok(Self {
            memory,
            address,
            node_limit,
        })
    }

    /// Returns the stored pointer verbatim, including a stored null pointer.
    /// None means the key is absent. The path is bounded and checked for cycles and ordering.
    /// A stable head and count do not prove that concurrent mutation has not occurred.
    pub fn find(&self, key: u32) -> Result<Option<u64>> {
        self.lookup(key, |node| self.memory.read_u64(self.at(node, 40)?))
    }

    /// Copy the matching node address for independently proved non-pointer payload formats.
    /// Does not read +40 or acquire a node reference. Caller rechecks links/format after payload.
    pub fn find_node(&self, key: u32) -> Result<Option<u64>> {
        self.lookup(key, Ok)
    }

    /// Copy the complete bounded tree in unsigned key order. Checks every reached node,
    /// count, inclusive/strict ordering and all copied links/values again. This is not a lock,
    /// pin, atomic snapshot or protection against ABA. Repeated keys retain all values.
    pub fn snapshot(&self, keys: TreeKeys) -> Result<Vec<PointerTreeEntry>> {
        let head_slot = self.at(self.address, 8)?;
        let count_slot = self.at(self.address, 16)?;
        let head = self.memory.read_u64(head_slot)?;
        let count = self.memory.read_u64(count_slot)?;
        if head == 0
            || !head.is_multiple_of(8)
            || count > self.node_limit as u64
            || self.memory.read_u8(self.at(head, 25)?)? != 1
        {
            return Err(Error::InvalidProfile("invalid snapshot tree head/count"));
        }
        let root_slot = self.at(head, 8)?;
        let root = self.memory.read_u64(root_slot)?;
        if (count == 0) != (root == head) {
            return Err(Error::InvalidProfile("snapshot tree count/root disagree"));
        }
        let mut stack = vec![(root, None, None)];
        let mut seen = BTreeSet::new();
        let mut records = Vec::with_capacity(count as usize);
        while let Some((node, lower, upper)) = stack.pop() {
            if node == head {
                continue;
            }
            if node == 0
                || !node.is_multiple_of(8)
                || seen.len() as u64 >= count
                || !seen.insert(node)
                || self.memory.read_u8(self.at(node, 25)?)? != 0
            {
                return Err(Error::InvalidProfile("invalid snapshot tree node/path"));
            }
            let entry = PointerTreeEntry {
                node,
                key: self.memory.read_u32(self.at(node, 32)?)?,
                value: self.memory.read_u64(self.at(node, 40)?)?,
            };
            let strict = keys == TreeKeys::Unique;
            if lower.is_some_and(|v| entry.key < v || (strict && entry.key == v))
                || upper.is_some_and(|v| entry.key > v || (strict && entry.key == v))
            {
                return Err(Error::InvalidProfile("invalid snapshot key ordering"));
            }
            let left = self.memory.read_u64(node)?;
            let right = self.memory.read_u64(self.at(node, 16)?)?;
            records.push((entry, left, right));
            stack.push((right, Some(entry.key), upper));
            stack.push((left, lower, Some(entry.key)));
        }
        if records.len() as u64 != count {
            return Err(Error::InvalidProfile("snapshot node count disagrees"));
        }
        for (entry, left, right) in &records {
            if self.memory.read_u64(entry.node)? != *left
                || self.memory.read_u64(self.at(entry.node, 16)?)? != *right
                || self.memory.read_u8(self.at(entry.node, 25)?)? != 0
                || self.memory.read_u32(self.at(entry.node, 32)?)? != entry.key
                || self.memory.read_u64(self.at(entry.node, 40)?)? != entry.value
            {
                return Err(Error::InvalidProfile("snapshot node changed"));
            }
        }
        if self.memory.read_u64(head_slot)? != head
            || self.memory.read_u64(count_slot)? != count
            || self.memory.read_u64(root_slot)? != root
            || self.memory.read_u8(self.at(head, 25)?)? != 1
        {
            return Err(Error::InvalidProfile("snapshot tree changed"));
        }
        let mut entries: Vec<_> = records.into_iter().map(|x| x.0).collect();
        entries.sort_by_key(|entry| (entry.key, entry.node));
        Ok(entries)
    }

    fn lookup<T>(&self, key: u32, read: impl Fn(u64) -> Result<T>) -> Result<Option<T>> {
        let head_slot = self.at(self.address, 8)?;
        let count_slot = self.at(self.address, 16)?;
        let head = self.memory.read_u64(head_slot)?;
        let count = self.memory.read_u64(count_slot)?;
        if head == 0 || count > self.node_limit as u64 {
            return Err(Error::InvalidProfile(
                "invalid tree head or excessive node count",
            ));
        }
        if self.memory.read_u8(self.at(head, 25)?)? != 1 {
            return Err(Error::InvalidProfile("tree head is not a sentinel"));
        }
        let root_slot = self.at(head, 8)?;
        let root = self.memory.read_u64(root_slot)?;
        if (count == 0) != (root == head) {
            return Err(Error::InvalidProfile("tree count and root disagree"));
        }
        let mut node = root;
        let mut visited = BTreeSet::new();
        let mut lower = None;
        let mut upper = None;
        let mut found = None;
        while node != head {
            if node == 0 || visited.len() as u64 >= count || !visited.insert(node) {
                return Err(Error::InvalidProfile(
                    "tree path is null, cyclic or exceeds its count",
                ));
            }
            if self.memory.read_u8(self.at(node, 25)?)? != 0 {
                return Err(Error::InvalidProfile("unexpected sentinel in tree path"));
            }
            let actual = self.memory.read_u32(self.at(node, 32)?)?;
            if lower.is_some_and(|v| actual <= v) || upper.is_some_and(|v| actual >= v) {
                return Err(Error::InvalidProfile("tree key ordering is invalid"));
            }
            match key.cmp(&actual) {
                std::cmp::Ordering::Equal => {
                    found = Some(read(node)?);
                    break;
                }
                std::cmp::Ordering::Less => {
                    upper = Some(actual);
                    node = self.memory.read_u64(node)?;
                }
                std::cmp::Ordering::Greater => {
                    lower = Some(actual);
                    node = self.memory.read_u64(self.at(node, 16)?)?;
                }
            }
        }
        if self.memory.read_u64(head_slot)? != head
            || self.memory.read_u64(count_slot)? != count
            || self.memory.read_u64(root_slot)? != root
        {
            return Err(Error::InvalidProfile("tree changed during lookup"));
        }
        Ok(found)
    }

    fn at(&self, base: u64, offset: u64) -> Result<u64> {
        base.checked_add(offset).ok_or(Error::AddressOverflow)
    }
}
