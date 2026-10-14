//! Experimental Goal float-parameter copies matching the native signed-index getter.
use crate::{
    goal_actions::GoalActionState,
    reader::{NightreignReader, ReaderCore},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
/// Definition IDs selected by the exact native builtin Goal factory.
/// These are Goal kinds; they are not controller request or animation IDs.
pub const EXEC_ACTION_GOAL_ID: i32 = 2034;
pub const FOLLOW_PATH_GOAL_ID: i32 = 2035;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalParametersSnapshot {
    pub control: GoalActionState,
    pub inline_bits: [u32; 8],
    pub extra_bits: Vec<u32>,
    pub raw_allocator_address: u64,
    pub vector_begin: u64,
    pub vector_end: u64,
    pub vector_capacity_end: u64,
}
impl GoalParametersSnapshot {
    /// RVA 0x2d0960: negative and missing indices return positive-zero bits.
    /// Existing NaN/infinity/signed-zero payload bits are preserved.
    pub fn parameter_bits(&self, index: i32) -> u32 {
        if index < 0 {
            return 0;
        }
        let index = index as usize;
        if index < 8 {
            self.inline_bits[index]
        } else {
            self.extra_bits.get(index - 8).copied().unwrap_or(0)
        }
    }
    pub fn parameter(&self, index: i32) -> f32 {
        f32::from_bits(self.parameter_bits(index))
    }
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// Copies the eight inline values and at most 4096 extra values. Requires
    /// aligned, ordered vector pointers with a four-byte used/capacity stride.
    /// Allocator/capacity storage is not traversed. No allocation ownership is borrowed.
    pub fn goal_parameters(
        &self,
        context: u64,
        extra_limit: usize,
    ) -> Result<GoalParametersSnapshot> {
        self.core().goal_parameters(context, extra_limit)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn goal_parameters(
        &self,
        context: u64,
        extra_limit: usize,
    ) -> Result<GoalParametersSnapshot> {
        if !(1..=4096).contains(&extra_limit) {
            return Err(Error::InvalidProfile(
                "NR Goal extra-parameter limit must be 1..=4096",
            ));
        }
        let control = self.goal_action_state(context)?;
        let inline_address = context.checked_add(0x60).ok_or(Error::AddressOverflow)?;
        let mut inline = [0; 32];
        self.memory.read_exact(inline_address, &mut inline)?;
        let allocator = self.u64(context, 0x80)?;
        let begin = self.u64(context, 0x88)?;
        let end = self.u64(context, 0x90)?;
        let capacity = self.u64(context, 0x98)?;
        if begin > end
            || end > capacity
            || !begin.is_multiple_of(4)
            || !end.is_multiple_of(4)
            || !capacity.is_multiple_of(4)
            || begin == 0 && (end != 0 || capacity != 0)
        {
            return Err(Error::InvalidProfile("invalid NR Goal parameter vector"));
        }
        let count = (end - begin) / 4;
        if count > extra_limit as u64 {
            return Err(Error::InvalidProfile(
                "NR Goal extra-parameter count exceeds bound",
            ));
        }
        let mut raw = vec![0; count as usize * 4];
        if !raw.is_empty() {
            self.memory.read_exact(begin, &mut raw)?;
        }
        let mut repeated_inline = [0; 32];
        self.memory
            .read_exact(inline_address, &mut repeated_inline)?;
        let mut repeated = vec![0; raw.len()];
        if !repeated.is_empty() {
            self.memory.read_exact(begin, &mut repeated)?;
        }
        if repeated_inline != inline || repeated != raw {
            return Err(Error::InvalidProfile(
                "NR Goal parameters changed during copy",
            ));
        }
        for (offset, value) in [
            (0x80, allocator),
            (0x88, begin),
            (0x90, end),
            (0x98, capacity),
        ] {
            self.unchanged_u64(context, offset, value)?;
        }
        if self.goal_action_state(context)? != control {
            return Err(Error::InvalidProfile(
                "NR Goal context changed during parameter copy",
            ));
        }
        Ok(GoalParametersSnapshot {
            control,
            inline_bits: std::array::from_fn(|j| {
                u32::from_le_bytes(inline[j * 4..j * 4 + 4].try_into().expect("word"))
            }),
            extra_bits: raw
                .chunks_exact(4)
                .map(|word| u32::from_le_bytes(word.try_into().expect("word")))
                .collect(),
            raw_allocator_address: allocator,
            vector_begin: begin,
            vector_end: end,
            vector_capacity_end: capacity,
        })
    }
}
