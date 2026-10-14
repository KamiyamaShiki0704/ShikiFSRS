//! Experimental input plans for FollowPathExecAction's original activation.
//! No path request, engine callback or movement is performed by this interface.
use crate::{
    goal_actions::GoalActionState,
    goal_parameters::FOLLOW_PATH_GOAL_ID,
    reader::{NightreignReader, ReaderCore},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
pub(crate) const FOLLOW_PATH_TABLE: u32 = 0x2c03e90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FollowPathActionPlan {
    pub control: GoalActionState,
    pub implementation_address: u64,
    pub interface_address: u64,
    /// Original getter indices 0..=5. Float argument roles remain unproved.
    pub parameter_bits: [u32; 6],
    /// CVTTSS2SI results for getter indices 0, 1, 2 and 4, in that order.
    /// Index zero is published through the original Goal context setter.
    pub integer_arguments: [i32; 4],
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    pub fn goal_follow_path_action_plan(&self, context: u64) -> Result<FollowPathActionPlan> {
        self.core().goal_follow_path_action_plan(context)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn goal_follow_path_action_plan(
        &self,
        context: u64,
    ) -> Result<FollowPathActionPlan> {
        let control = self.goal_action_state(context)?;
        if self.u32(context, 0xa0)? as i32 != FOLLOW_PATH_GOAL_ID {
            return Err(Error::InvalidProfile(
                "NR context is not FollowPathExecAction",
            ));
        }
        let implementation = self.u64(context, 0xa8)?;
        self.check_type(implementation, &[FOLLOW_PATH_TABLE])?;
        if self.u32(implementation, 0xf8)? as i32 != FOLLOW_PATH_GOAL_ID {
            return Err(Error::InvalidProfile("NR FollowPath definition ID differs"));
        }
        let address = context.checked_add(0x60).ok_or(Error::AddressOverflow)?;
        let mut raw = [0; 24];
        self.memory.read_exact(address, &mut raw)?;
        let parameter_bits: [u32; 6] = std::array::from_fn(|j| {
            u32::from_le_bytes(raw[j * 4..j * 4 + 4].try_into().expect("parameter"))
        });
        let mut integer_arguments = [0; 4];
        for (output, index) in integer_arguments.iter_mut().zip([0, 1, 2, 4]) {
            *output = crate::goal_requests::integer_bits(parameter_bits[index])? as i32;
        }
        let mut repeated = [0; 24];
        self.memory.read_exact(address, &mut repeated)?;
        self.unchanged_u64(context, 0xa8, implementation)?;
        self.check_type(implementation, &[FOLLOW_PATH_TABLE])?;
        if raw != repeated
            || self.u32(context, 0xa0)? as i32 != FOLLOW_PATH_GOAL_ID
            || self.u32(implementation, 0xf8)? as i32 != FOLLOW_PATH_GOAL_ID
            || self.goal_action_state(context)? != control
        {
            return Err(Error::InvalidProfile(
                "NR FollowPath input changed during plan",
            ));
        }
        Ok(FollowPathActionPlan {
            control,
            implementation_address: implementation,
            interface_address: context.checked_add(0x18).ok_or(Error::AddressOverflow)?,
            parameter_bits,
            integer_arguments,
        })
    }
}
