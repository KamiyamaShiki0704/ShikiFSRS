//! Experimental bounded, read-only projection of the native unsigned ID-table query.
use crate::{
    character_think::CharacterThinkAssociation,
    reader::{NightreignReader, ReaderCore},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
use std::collections::BTreeSet;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionIdLookupNode {
    pub address: u64,
    pub left_address: u64,
    pub raw_link_08: u64,
    pub right_address: u64,
    pub raw_color: u8,
    pub id_bits: u32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionIdLookupSnapshot {
    pub association: CharacterThinkAssociation,
    pub lookup_address: u64,
    pub header_address: u64,
    pub declared_node_count: u64,
    pub queried_id_bits: u32,
    pub path: Vec<ActionIdLookupNode>,
    pub matched_node_address: Option<u64>,
}
impl ActionIdLookupSnapshot {
    /// Table membership only. Does not prove animation availability or execution.
    pub fn contains_id(&self) -> bool {
        self.matched_node_address.is_some()
    }
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// Mirrors RVA 0x26bdb0's unsigned comparisons without its latch writes.
    /// Reads only a bounded search path, not the entire table or node payloads.
    pub fn character_action_id_lookup(
        &self,
        character: u64,
        id_bits: u32,
        path_limit: usize,
    ) -> Result<Option<ActionIdLookupSnapshot>> {
        self.core()
            .character_action_id_lookup(character, id_bits, path_limit)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn character_action_id_lookup(
        &self,
        character: u64,
        id_bits: u32,
        path_limit: usize,
    ) -> Result<Option<ActionIdLookupSnapshot>> {
        if !(1..=4096).contains(&path_limit) {
            return Err(Error::InvalidProfile(
                "NR action ID path limit must be 1..=4096",
            ));
        }
        let Some(association) = self.character_com_think(character)? else {
            return Ok(None);
        };
        let think = association.think_address;
        let lookup = think.checked_add(0xde90).ok_or(Error::AddressOverflow)?;
        if self.u64(lookup, 0)? != think {
            return Err(Error::InvalidProfile(
                "NR action ID lookup Think owner differs",
            ));
        }
        let header = self.u64(lookup, 0xa38)?;
        let count = self.u64(lookup, 0xa40)?;
        if header == 0 || !header.is_multiple_of(8) {
            return Err(Error::InvalidProfile("invalid NR action ID tree header"));
        }
        let mut head = [0; 32];
        self.memory.read_exact(header, &mut head)?;
        if head[0x19] == 0 {
            return Err(Error::InvalidProfile(
                "NR action ID header is not a sentinel",
            ));
        }
        let mut current = u64::from_le_bytes(head[8..16].try_into().expect("root"));
        if (count == 0) != (current == header) {
            return Err(Error::InvalidProfile("NR action ID root/count disagree"));
        }
        let mut seen = BTreeSet::new();
        let mut path = Vec::new();
        let mut observations = Vec::new();
        let mut lower = None;
        let mut upper = None;
        let mut candidate = None;
        while current != header {
            if current == 0 || !current.is_multiple_of(8) || !seen.insert(current) {
                return Err(Error::InvalidProfile(
                    "NR action ID path is null, unaligned or cyclic",
                ));
            }
            if path.len() == path_limit || path.len() as u64 >= count {
                return Err(Error::InvalidProfile(
                    "NR action ID path exceeds bound or declared count",
                ));
            }
            let mut raw = [0; 32];
            self.memory.read_exact(current, &mut raw)?;
            if raw[0x19] != 0 {
                return Err(Error::InvalidProfile(
                    "NR action ID path has a foreign sentinel",
                ));
            }
            let word = |at: usize| u64::from_le_bytes(raw[at..at + 8].try_into().expect("link"));
            let node = ActionIdLookupNode {
                address: current,
                left_address: word(0),
                raw_link_08: word(8),
                right_address: word(0x10),
                raw_color: raw[0x18],
                id_bits: u32::from_le_bytes(raw[0x1c..0x20].try_into().expect("ID")),
            };
            if lower.is_some_and(|bound| node.id_bits <= bound)
                || upper.is_some_and(|bound| node.id_bits >= bound)
            {
                return Err(Error::InvalidProfile(
                    "NR action ID path violates unsigned key order",
                ));
            }
            observations.push((current, raw));
            path.push(node);
            if node.id_bits >= id_bits {
                candidate = Some(node);
                upper = Some(node.id_bits);
                current = node.left_address;
            } else {
                lower = Some(node.id_bits);
                current = node.right_address;
            }
        }
        if count == 0 && !path.is_empty() {
            return Err(Error::InvalidProfile("NR empty action ID table has nodes"));
        }
        let matched = candidate
            .filter(|node| node.id_bits == id_bits)
            .map(|node| node.address);
        for (address, raw) in observations {
            let mut repeated = [0; 32];
            self.memory.read_exact(address, &mut repeated)?;
            if repeated != raw {
                return Err(Error::InvalidProfile(
                    "NR action ID node changed during lookup",
                ));
            }
        }
        let mut repeated = [0; 32];
        self.memory.read_exact(header, &mut repeated)?;
        if repeated != head {
            return Err(Error::InvalidProfile(
                "NR action ID header changed during lookup",
            ));
        }
        self.unchanged_u64(lookup, 0xa38, header)?;
        self.unchanged_u64(lookup, 0xa40, count)?;
        self.unchanged_u64(lookup, 0, think)?;
        if self.character_com_think(character)? != Some(association) {
            return Err(Error::InvalidProfile(
                "NR action ID actor association changed",
            ));
        }
        Ok(Some(ActionIdLookupSnapshot {
            association,
            lookup_address: lookup,
            header_address: header,
            declared_node_count: count,
            queried_id_bits: id_bits,
            path,
            matched_node_address: matched,
        }))
    }
}
