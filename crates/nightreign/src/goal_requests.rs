//! Experimental original GoalExecAction request publication. Does not run a Goal pipeline
//! or establish which action/animation the engine will execute.
use crate::{
    NightreignFile,
    goal_actions::GoalActionState,
    reader::{NightreignReader, ReaderCore},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
pub(crate) const EXEC_GOAL_TABLE: u32 = 0x2c03e48;
const ACTIVATE: CodeRange = CodeRange {
    rva: 0x2dcdb0,
    length: 40,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalActionRequestPlan {
    pub implementation_address: u64,
    pub interface_address: u64,
    pub parameter_bits: u32,
    pub requested_id_bits: u32,
    pub before: GoalActionState,
}
fn integer_bits(parameter_bits: u32) -> Result<u32> {
    let value = f32::from_bits(parameter_bits);
    if !value.is_finite() || !(-2147483648.0..2147483648.0).contains(&value) {
        return Err(Error::InvalidProfile(
            "NR Goal parameter cannot be converted in the supported i32 domain",
        ));
    }
    Ok((value.trunc() as i32) as u32)
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    pub fn goal_action_request_plan(&self, context: u64) -> Result<GoalActionRequestPlan> {
        self.core().goal_action_request_plan(context)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn goal_action_request_plan(&self, context: u64) -> Result<GoalActionRequestPlan> {
        let before = self.goal_action_state(context)?;
        // The original Goal dispatcher reads the implementation at +0xa8.
        let implementation = self.u64(context, 0xa8)?;
        self.check_type(implementation, &[EXEC_GOAL_TABLE])?;
        // CSGoalFunc +8 is the containing context; parameter zero uses its inline +0x60.
        let parameter_bits = self.u32(context, 0x60)?;
        let requested_id_bits = integer_bits(parameter_bits)?;
        self.unchanged_u64(context, 0xa8, implementation)?;
        if self.u32(context, 0x60)? != parameter_bits || self.goal_action_state(context)? != before
        {
            return Err(Error::InvalidProfile(
                "NR Goal request input changed during plan",
            ));
        }
        self.check_type(implementation, &[EXEC_GOAL_TABLE])?;
        Ok(GoalActionRequestPlan {
            implementation_address: implementation,
            interface_address: context.checked_add(0x18).ok_or(Error::AddressOverflow)?,
            parameter_bits,
            requested_id_bits,
            before,
        })
    }
}
/// Invokes only the original `u8(GoalExecAction*, unused, CSGoalFunc*)` entry.
/// # Safety
/// All addresses and reads belong to one engine. Use the supplied original method and
/// interface; the second argument is unused in this exact 40-byte entry. Do not unwind or
/// substitute direct writes. The method reads Goal parameter zero, publishes an integer and
/// sets a byte; it does not transfer allocations or SDK buffers. Errors may follow writes.
pub unsafe trait GoalActionRequestBackend: AddressSpace {
    /// # Safety
    /// Caller holds the validated original Goal implementation, context and mutation phase.
    unsafe fn invoke_goal_request(
        &self,
        entry: u64,
        implementation: u64,
        interface: u64,
    ) -> Result<u8>;
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalActionRequestReceipt {
    pub plan: GoalActionRequestPlan,
    /// Original entry returns byte 1 after publication; this is not engine execution acceptance.
    pub native_result: Result<u8>,
    pub after: Result<GoalActionState>,
}
pub struct GoalActionRequests<'r, 'f, B> {
    reader: NightreignReader<'r, 'f, B>,
}
impl<'f> NightreignFile<'f> {
    pub fn bind_goal_action_requests<'r, B: GoalActionRequestBackend>(
        &'r self,
        backend: &'r B,
        base: u64,
    ) -> Result<GoalActionRequests<'r, 'f, B>> {
        Ok(GoalActionRequests {
            reader: self.bind_reader(backend, base)?,
        })
    }
}
impl<B: GoalActionRequestBackend> GoalActionRequests<'_, '_, B> {
    /// Publishes parameter zero of an existing, exact GoalExecAction context.
    /// # Safety
    /// Host supplies a live, initialized context owned by this Goal implementation, an offline
    /// serialized mutation phase and valid action ID for that context. Prevent teardown/ABA,
    /// concurrent consumers or patches; engine/Goal/module/SDK residency covers return frames.
    /// This bypasses no policy within the original entry, but caller must choose a valid
    /// activation point in the Goal lifecycle. Execution and ID mapping remain unaccepted.
    /// Never infer rollback or automatically retry a native/post-call error.
    pub unsafe fn publish_from_context(&self, context: u64) -> Result<GoalActionRequestReceipt> {
        let plan = self.reader.goal_action_request_plan(context)?;
        let entry = self.reader.module.code_address(ACTIVATE)?;
        let image = self.reader.file.file().image();
        let slot_rva = EXEC_GOAL_TABLE + 0x10;
        let expected = u64::from_le_bytes(image.read_rva(slot_rva, 8)?.try_into().expect("slot"));
        if expected != image.preferred_base + ACTIVATE.rva as u64 {
            return Err(Error::InvalidProfile(
                "NR Goal activation differs from file",
            ));
        }
        let slot = self
            .reader
            .module
            .base()
            .checked_add(slot_rva as u64)
            .ok_or(Error::AddressOverflow)?;
        if self.reader.module.memory().read_u64(slot)? != entry {
            return Err(Error::ImageMismatch { rva: slot_rva });
        }
        if self.reader.goal_action_request_plan(context)? != plan {
            return Err(Error::InvalidProfile(
                "NR Goal request changed before publication",
            ));
        }
        // SAFETY: Exact method/type/parameter and interface/self-link checked; host supplies
        // initialization, ID validity, lifecycle scheduling and native residency obligations.
        let native_result = unsafe {
            self.reader.module.memory().invoke_goal_request(
                entry,
                plan.implementation_address,
                plan.interface_address,
            )
        };
        let after = self.reader.goal_action_state(context).and_then(|state| {
            if self.reader.core().u64(context, 0xa8)? != plan.implementation_address {
                return Err(Error::InvalidProfile(
                    "NR Goal implementation replaced after publication",
                ));
            }
            self.reader
                .core()
                .check_type(plan.implementation_address, &[EXEC_GOAL_TABLE])?;
            Ok(state)
        });
        Ok(GoalActionRequestReceipt {
            plan,
            native_result,
            after,
        })
    }
}
#[cfg(windows)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CurrentGoalActionRequestBackend;
#[cfg(windows)]
impl AddressSpace for CurrentGoalActionRequestBackend {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        shikifsrs_runtime::memory::CurrentProcess.read_exact(address, output)
    }
}
#[cfg(windows)]
// SAFETY: Only the checked exact activation ABI; unsafe callers supply native lifecycle/phase.
unsafe impl GoalActionRequestBackend for CurrentGoalActionRequestBackend {
    unsafe fn invoke_goal_request(
        &self,
        entry: u64,
        implementation: u64,
        interface: u64,
    ) -> Result<u8> {
        type Activate = unsafe extern "system" fn(
            *mut std::ffi::c_void,
            *mut std::ffi::c_void,
            *mut std::ffi::c_void,
        ) -> u8;
        // SAFETY: Entry and AMD64 signature are independently evidenced for this exact build.
        let function: Activate = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Exact method ignores argument two; live original implementation/interface held.
        Ok(unsafe {
            function(
                implementation as *mut std::ffi::c_void,
                std::ptr::null_mut(),
                interface as *mut std::ffi::c_void,
            )
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversion_truncates_toward_zero_and_preserves_signed_id_bits() {
        for (value, id) in [
            (0.0, 0i32),
            (-0.0, 0),
            (42.75, 42),
            (-42.75, -42),
            (-2147483648.0, i32::MIN),
            (2147483520.0, 2147483520),
        ] {
            assert_eq!(integer_bits(f32::to_bits(value)).unwrap(), id as u32);
        }
    }
    #[test]
    fn invalid_x86_conversion_domain_is_rejected_without_saturating() {
        for value in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            2147483648.0,
            -2147483904.0,
        ] {
            assert!(integer_bits(value.to_bits()).is_err());
        }
    }
}
