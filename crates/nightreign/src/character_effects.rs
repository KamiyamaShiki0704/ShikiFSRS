//! Experimental bounded SpecialEffect node copies. Reads never mutate or free nodes.
use crate::reader::{CHARACTER_TABLE, ENEMY_TABLE, NightreignReader, PLAYER_TABLE, ReaderCore};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
use std::collections::BTreeSet;

pub(crate) const EFFECT_TABLE: u32 = 0x2c1b5d8;
const EXCLUDED_FLAGS: u32 = 0x800c0003;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpecialEffectNode {
    pub address: u64,
    pub parameter_address: u64,
    pub previous_address: u64,
    pub id: i32,
    pub raw_flags: u32,
    /// Native PARAM stateInfo (+0x156), copied only for query-eligible nodes.
    pub state_info: Option<u16>,
}
impl SpecialEffectNode {
    /// Eligibility for the native ID/state queries; does not prove gameplay application.
    pub fn query_eligible(self) -> bool {
        self.raw_flags & EXCLUDED_FLAGS == 0
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecialEffectsSnapshot {
    pub character_address: u64,
    pub effect_address: u64,
    pub owner_address: u64,
    pub native_state_summary: u64,
    pub nodes: Vec<SpecialEffectNode>,
}
impl SpecialEffectsSnapshot {
    pub(crate) fn check_owner_links(&self) -> Result<()> {
        if self.owner_address != self.character_address {
            return Err(Error::InvalidProfile(
                "NR effect owner differs from character",
            ));
        }
        let mut previous = 0;
        for node in &self.nodes {
            if node.previous_address != previous {
                return Err(Error::InvalidProfile("NR effect previous link differs"));
            }
            previous = node.address;
        }
        Ok(())
    }
    pub fn contains_id(&self, id: i32) -> bool {
        self.nodes
            .iter()
            .any(|node| node.query_eligible() && node.id == id)
    }
    /// Mirrors RVA 0x508e20, including its modulo-15 summary buckets and null PARAM case.
    pub fn contains_state(&self, state: u16) -> bool {
        let shift = ((state as u32 % 15) + u32::from(state != 0)) * 4;
        self.native_state_summary & (0xfu64 << shift) != 0
            && self
                .nodes
                .iter()
                .any(|node| node.query_eligible() && node.state_info.unwrap_or(0) == state)
    }
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    pub fn special_effects(
        &self,
        character: u64,
        limit: usize,
    ) -> Result<Option<SpecialEffectsSnapshot>> {
        self.core().special_effects(character, limit)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn special_effects(
        &self,
        character: u64,
        limit: usize,
    ) -> Result<Option<SpecialEffectsSnapshot>> {
        if !(1..=4096).contains(&limit) {
            return Err(Error::InvalidProfile("NR effect limit must be 1..=4096"));
        }
        let class = self.check_type(character, &[CHARACTER_TABLE, PLAYER_TABLE, ENEMY_TABLE])?;
        let effect = self.u64(character, 0x190)?;
        if effect == 0 {
            return Ok(None);
        }
        self.check_type(effect, &[EFFECT_TABLE])?;
        let head = self.u64(effect, 8)?;
        let owner = self.u64(effect, 0x10)?;
        let summary = self.u64(effect, 0x28)?;
        let mut next = head;
        let mut seen = BTreeSet::new();
        let mut nodes = Vec::new();
        let mut observations = Vec::new();
        while next != 0 {
            if nodes.len() == limit || !next.is_multiple_of(8) || !seen.insert(next) {
                return Err(Error::InvalidProfile(
                    "NR effect list exceeds bound, cycles or is unaligned",
                ));
            }
            let address = next;
            let mut raw = [0; 0x6c];
            self.memory.read_exact(address, &mut raw)?;
            let parameter = u64::from_le_bytes(raw[..8].try_into().expect("node"));
            let id = i32::from_le_bytes(raw[8..12].try_into().expect("node"));
            let flags = u32::from_le_bytes(raw[0x68..0x6c].try_into().expect("node"));
            next = u64::from_le_bytes(raw[0x38..0x40].try_into().expect("node"));
            let state_info = if flags & EXCLUDED_FLAGS == 0 && parameter != 0 {
                let mut state = [0; 2];
                self.memory.read_exact(
                    parameter.checked_add(0x156).ok_or(Error::AddressOverflow)?,
                    &mut state,
                )?;
                Some(u16::from_le_bytes(state))
            } else {
                None
            };
            nodes.push(SpecialEffectNode {
                address,
                parameter_address: parameter,
                previous_address: u64::from_le_bytes(raw[0x40..0x48].try_into().expect("node")),
                id,
                raw_flags: flags,
                state_info,
            });
            observations.push(raw);
        }
        for (node, raw) in nodes.iter().zip(observations) {
            let mut repeated = [0; 0x6c];
            self.memory.read_exact(node.address, &mut repeated)?;
            if repeated != raw {
                return Err(Error::InvalidProfile("NR effect node changed during copy"));
            }
            if let Some(state) = node.state_info {
                let mut repeated_state = [0; 2];
                self.memory.read_exact(
                    node.parameter_address
                        .checked_add(0x156)
                        .ok_or(Error::AddressOverflow)?,
                    &mut repeated_state,
                )?;
                if u16::from_le_bytes(repeated_state) != state {
                    return Err(Error::InvalidProfile("NR effect PARAM state changed"));
                }
            }
        }
        self.unchanged_u64(effect, 8, head)?;
        self.unchanged_u64(effect, 0x10, owner)?;
        self.unchanged_u64(effect, 0x28, summary)?;
        self.check_type(effect, &[EFFECT_TABLE])?;
        self.unchanged_u64(character, 0x190, effect)?;
        self.unchanged_u64(character, 0, self.base + class as u64)?;
        Ok(Some(SpecialEffectsSnapshot {
            character_address: character,
            effect_address: effect,
            owner_address: owner,
            native_state_summary: summary,
            nodes,
        }))
    }
}
