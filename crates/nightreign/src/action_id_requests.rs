//! Experimental original ID-request bridge; publication/latches do not prove execution.
use crate::{
    NightreignFile, action_id_resolution::ActionIdResolutionPlan,
    action_request_ids::ActionRequestIdState, reader::NightreignReader,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
const REQUEST: CodeRange = CodeRange {
    rva: 0x2c0dc0,
    length: 9,
};
/// Invokes only the original bridge with its validated CSAiFunc interface and ID bits.
/// # Safety
/// Keep all engine objects/model/catalog/ID-table storage alive and writable where
/// required. Use an exclusive native mutation phase. Preserve original entry/arguments
/// and do not unwind through game code. The native helper owns its stack ID temporary;
/// no SDK buffer or allocation is lent/transferred. Errors can follow latch writes.
pub unsafe trait ActionIdRequestBackend: AddressSpace {
    /// # Safety
    /// Caller holds the original interface, code and valid phase described above.
    unsafe fn invoke_action_id_request(
        &self,
        entry: u64,
        interface: u64,
        requested_id_bits: u32,
    ) -> Result<()>;
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionIdRequestReceipt {
    pub before: ActionRequestIdState,
    pub resolution: ActionIdResolutionPlan,
    pub predicted_table_member: bool,
    /// A completed native call is not animation/action execution acceptance.
    pub native_result: Result<()>,
    /// Captured even if the backend reports an error after writes; no rollback.
    pub after: Result<ActionRequestIdState>,
    pub latches_match_prediction: Option<bool>,
}
pub struct ActionIdRequests<'r, 'f, B> {
    reader: NightreignReader<'r, 'f, B>,
}
impl<'f> NightreignFile<'f> {
    pub fn bind_action_id_requests<'r, B: ActionIdRequestBackend>(
        &'r self,
        backend: &'r B,
        base: u64,
    ) -> Result<ActionIdRequests<'r, 'f, B>> {
        Ok(ActionIdRequests {
            reader: self.bind_reader(backend, base)?,
        })
    }
}
impl<B: ActionIdRequestBackend> ActionIdRequests<'_, '_, B> {
    /// Calls the original bridge with the original ID, so the engine applies its
    /// adjustment exactly once. Bounds/preflight changes fail before invocation.
    /// # Safety
    /// Caller guarantees initialized same-engine objects, valid ID domain, resident
    /// source data and exclusive writable phase; no teardown/ABA reuse or live patches.
    /// Offline conditions and actual execution acceptance remain host responsibilities.
    pub unsafe fn request(
        &self,
        character: u64,
        id_bits: u32,
        record_limit: usize,
        path_limit: usize,
    ) -> Result<ActionIdRequestReceipt> {
        let before = self
            .reader
            .character_action_request_ids(character)?
            .ok_or(Error::InvalidProfile("NR ID request has no ComThink"))?;
        let resolution = self
            .reader
            .character_action_id_resolution_plan(character, id_bits, record_limit)?
            .ok_or(Error::InvalidProfile("NR ID resolution has no ComThink"))?;
        let lookup = self
            .reader
            .character_action_id_lookup(character, resolution.resolved_id_bits, path_limit)?
            .ok_or(Error::InvalidProfile("NR ID lookup has no ComThink"))?;
        if before.association != resolution.association || before.association != lookup.association
        {
            return Err(Error::InvalidProfile(
                "NR ID request actor changed during preflight",
            ));
        }
        let entry = self.reader.module.code_address(REQUEST)?;
        if self.reader.character_action_request_ids(character)? != Some(before)
            || self
                .reader
                .character_action_id_resolution_plan(character, id_bits, record_limit)?
                != Some(resolution.clone())
            || self.reader.character_action_id_lookup(
                character,
                resolution.resolved_id_bits,
                path_limit,
            )? != Some(lookup.clone())
        {
            return Err(Error::InvalidProfile("NR ID request preflight changed"));
        }
        // SAFETY: Exact original bridge/interface and value-only ABI checked. Host holds
        // lifecycle/residency/exclusive phase. Pass requested ID, not predicted adjusted ID.
        let native_result = unsafe {
            self.reader.module.memory().invoke_action_id_request(
                entry,
                before.association.ai_interface_address,
                id_bits,
            )
        };
        let after = self
            .reader
            .character_action_request_ids(character)
            .and_then(|state| {
                let state = state.ok_or(Error::InvalidProfile(
                    "NR ComThink disappeared after ID request",
                ))?;
                if state.association != before.association {
                    return Err(Error::InvalidProfile(
                        "NR ID request actor replaced after call",
                    ));
                }
                Ok(state)
            });
        let expected_member = lookup.contains_id();
        let latches_match_prediction = after.as_ref().ok().map(|state| {
            state.requested_id as u32 == id_bits
                && state.retained_requested_id as u32 == id_bits
                && state.resolved_id as u32 == resolution.resolved_id_bits
                && state.retained_resolved_id as u32 == resolution.resolved_id_bits
                && state.raw_request_flags_cde[1] == 1
                && state.raw_lookup_match_flag == u8::from(expected_member)
        });
        Ok(ActionIdRequestReceipt {
            before,
            resolution,
            predicted_table_member: expected_member,
            native_result,
            after,
            latches_match_prediction,
        })
    }
}
#[cfg(windows)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CurrentActionIdRequestBackend;
#[cfg(windows)]
impl AddressSpace for CurrentActionIdRequestBackend {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        shikifsrs_runtime::memory::CurrentProcess.read_exact(address, output)
    }
}
#[cfg(windows)]
// SAFETY: Original exact Win64 scalar bridge only; unsafe caller supplies engine phase.
unsafe impl ActionIdRequestBackend for CurrentActionIdRequestBackend {
    unsafe fn invoke_action_id_request(
        &self,
        entry: u64,
        interface: u64,
        requested_id_bits: u32,
    ) -> Result<()> {
        type Request = unsafe extern "system" fn(*mut std::ffi::c_void, u32);
        // SAFETY: Independently evidenced RCX=CSAiFunc*, EDX=original ID bits. Native
        // return-register residue is ignored; no execution success value is exposed.
        let function: Request = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Original interface/entry retained and exclusive native phase held.
        unsafe { function(interface as *mut std::ffi::c_void, requested_id_bits) };
        Ok(())
    }
}
