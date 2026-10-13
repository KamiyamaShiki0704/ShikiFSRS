//! Current ER resource backend. Construction verifies; native work is callback-only.
use crate::{
    TaskPhase, WW_171, Ww171File,
    current_gparam::CurrentGparam,
    gparam::{CommonEventId, NativeGparamCall},
    gparam_task::GparamTaskBackend,
};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{sync::Arc, time::Instant};

pub struct CurrentGparamTaskBackend {
    file: Arc<OwnedVerifiedFile>,
    base: u64,
    patches: Option<Arc<OwnedCodePatches>>,
    clock: Instant,
}
impl CurrentGparamTaskBackend {
    /// # Safety
    /// CurrentGparam bind/call contracts hold on every eventual resource callback.
    /// Native managers/graph/allocators remain initialized and stable in the permitted
    /// exclusive DrawParam phase; no ABA, unload, aliases or producer teardown.
    /// No native session crosses a thread. Immutable file proof is retained and each
    /// callback binds a fresh thread-confined session. No request/prime occurs here.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        let file = Arc::new(OwnedVerifiedFile::verify(bytes, WW_171)?);
        let view = Ww171File::from_verified(file.borrow()?)?;
        if view.phase("DrawParamUpdate")? != TaskPhase::DRAW_PARAMETER_UPDATE {
            return Err(Error::InvalidProfile(
                "DrawParamUpdate phase disagrees with fixed file",
            ));
        }
        view.bind_objects_with_patches(&CurrentProcess, base, patches.as_deref())?;
        Ok(Self {
            file,
            base,
            patches,
            clock: Instant::now(),
        })
    }
    fn session(&self) -> Result<CurrentGparam> {
        CurrentGparam::from_verified(Arc::clone(&self.file), self.base, self.patches.clone())
    }
}
// SAFETY: Err is from pre-invocation verification; actual-call receipts retain all
// native results/postflight. Full callback-phase safety is the unsafe binder's contract.
unsafe impl GparamTaskBackend for CurrentGparamTaskBackend {
    fn elapsed_ms(&self) -> Result<u64> {
        u64::try_from(self.clock.elapsed().as_millis()).map_err(|_| Error::AddressOverflow)
    }
    unsafe fn request(&mut self, id: CommonEventId) -> Result<NativeGparamCall<Option<u64>>> {
        let session = self.session()?;
        // SAFETY: Task creator and binder retain the complete live native callback contract.
        unsafe { session.request_common_event(id) }
    }
    unsafe fn prime(&mut self, id: CommonEventId) -> Result<NativeGparamCall<()>> {
        let session = self.session()?;
        // SAFETY: Same exclusive DrawParam graph; consumer's finite original time is 0.0.
        unsafe { session.prime_common_event(id, 0.0) }
    }
}
