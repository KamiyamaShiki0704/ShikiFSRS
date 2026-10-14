//! Experimental copied action-request masks. Bits retain native numeric identities.
//! These predicates describe requests, not successful animation or gameplay execution.
use crate::{
    character_modules::{ModuleKind, ModuleSnapshot},
    reader::{NightreignReader, ReaderCore},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionChannel {
    Default,
    Context,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionRequestsSnapshot {
    pub module: ModuleSnapshot,
    pub primary_mask: u64,
    pub default_secondary_mask: u64,
    pub default_filtered_mask: u64,
    pub context_secondary_mask: u64,
    pub context_filtered_mask: u64,
    pub context_key: i32,
    pub context_enabled: bool,
    pub raw_status: u32,
}
impl ActionRequestsSnapshot {
    fn contextual(self, channel: ActionChannel) -> bool {
        self.context_enabled && channel == ActionChannel::Context
    }
    /// Mirrors RVA 0x3e9270, including its low-37-bit contextual mask.
    pub fn has_any_requests(self, channel: ActionChannel) -> bool {
        self.primary_mask != 0
            || if self.contextual(channel) {
                self.context_secondary_mask & 0x1fffffffff != 0
            } else {
                self.default_secondary_mask != 0
            }
    }
    /// Tests the primary native request bit, corresponding to RVA 0x3e9250.
    pub fn primary_requested(self, bit: u8) -> Result<bool> {
        Ok(self.primary_mask & checked_bit(bit)? != 0)
    }
    /// Mirrors RVA 0x3e93b0's filtered mask and status-bit-11 suppression.
    pub fn filtered_requested(self, channel: ActionChannel, bit: u8) -> Result<bool> {
        let mask = if self.contextual(channel) {
            self.context_filtered_mask
        } else {
            self.default_filtered_mask
        };
        Ok(mask & checked_bit(bit)? != 0 && self.raw_status & 0x800 == 0)
    }
}
fn checked_bit(bit: u8) -> Result<u64> {
    if bit >= 64 {
        return Err(Error::InvalidProfile("NR action bit must be 0..=63"));
    }
    Ok(1u64 << bit)
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    pub fn action_requests(
        &self,
        character: u64,
        limit: usize,
    ) -> Result<Option<ActionRequestsSnapshot>> {
        self.core().action_requests(character, limit)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn action_requests(
        &self,
        character: u64,
        limit: usize,
    ) -> Result<Option<ActionRequestsSnapshot>> {
        if !(1..=4096).contains(&limit) {
            return Err(Error::InvalidProfile(
                "NR action context limit must be 1..=4096",
            ));
        }
        let Some(module) = self.character_module(character, ModuleKind::ActionRequests)? else {
            return Ok(None);
        };
        let at = module.module_address;
        // The constructor installs this back-reference; contextual native queries use it.
        if self.u64(at, 0x50)? != at {
            return Err(Error::InvalidProfile("NR action context owner differs"));
        }
        let mut header = [0; 0x149];
        self.memory.read_exact(
            at.checked_add(0x10).ok_or(Error::AddressOverflow)?,
            &mut header,
        )?;
        let q = |offset: usize| {
            u64::from_le_bytes(
                header[offset - 0x10..offset - 8]
                    .try_into()
                    .expect("checked span"),
            )
        };
        let d = |offset: usize| {
            u32::from_le_bytes(
                header[offset - 0x10..offset - 0xc]
                    .try_into()
                    .expect("checked span"),
            )
        };
        let enabled = header[0x158 - 0x10] != 0;
        let key = d(0x98) as i32;
        let context_secondary_mask = if enabled {
            self.action_context_mask(at, 0x60, key, limit)?
        } else {
            0
        };
        let context_filtered_mask = if enabled {
            self.action_context_mask(at, 0x80, key, limit)?
        } else {
            0
        };
        let mut repeated = [0; 0x149];
        self.memory.read_exact(
            at.checked_add(0x10).ok_or(Error::AddressOverflow)?,
            &mut repeated,
        )?;
        if q(0x50) != at
            || header != repeated
            || self.character_module(character, ModuleKind::ActionRequests)? != Some(module)
        {
            return Err(Error::InvalidProfile(
                "NR action requests changed during copy",
            ));
        }
        Ok(Some(ActionRequestsSnapshot {
            module,
            primary_mask: q(0x20),
            default_secondary_mask: q(0x30),
            default_filtered_mask: q(0xa8),
            context_secondary_mask,
            context_filtered_mask,
            context_key: key,
            context_enabled: enabled,
            raw_status: d(0x120),
        }))
    }
    fn action_context_mask(&self, module: u64, offset: u64, key: i32, limit: usize) -> Result<u64> {
        let begin = self.u64(module, offset)?;
        let end = self.u64(module, offset + 8)?;
        let capacity = self.u64(module, offset + 16)?;
        let size = end
            .checked_sub(begin)
            .ok_or(Error::InvalidProfile("reversed NR action context span"))?;
        if !begin.is_multiple_of(8)
            || !size.is_multiple_of(16)
            || end > capacity
            || (begin == 0 && (end != 0 || capacity != 0))
            || size / 16 > limit as u64
        {
            return Err(Error::InvalidProfile(
                "invalid or excessive NR action context span",
            ));
        }
        let mut bytes = vec![0; size as usize];
        if !bytes.is_empty() {
            self.memory.read_exact(begin, &mut bytes)?;
        }
        let mask = bytes
            .chunks_exact(16)
            .find(|row| i32::from_le_bytes(row[..4].try_into().expect("record")) == key)
            .map(|row| u64::from_le_bytes(row[8..16].try_into().expect("record")))
            .unwrap_or(0);
        let mut repeated = vec![0; bytes.len()];
        if !bytes.is_empty() {
            self.memory.read_exact(begin, &mut repeated)?;
        }
        if bytes != repeated {
            return Err(Error::InvalidProfile("NR action context records changed"));
        }
        self.unchanged_u64(module, offset, begin)?;
        self.unchanged_u64(module, offset + 8, end)?;
        self.unchanged_u64(module, offset + 16, capacity)?;
        Ok(mask)
    }
}
