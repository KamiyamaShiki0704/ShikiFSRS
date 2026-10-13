//! Explicit current Gparam production host; submissions happen only at DrawParamUpdate.
pub use crate::task_resident::ResidentTaskState as ResidentGparamState;
use crate::{
    TaskPhase,
    current_gparam_task::CurrentGparamTaskBackend,
    current_tasks::CurrentTaskEngine,
    gparam_task::{GparamPreloadConfig, GparamTask, GparamTaskControl, GparamTaskReport},
    native_task::{NativeTaskControl, NativeTaskStatus, TaskEngine},
    task_resident::ResidentTaskSlot,
};
use shikifsrs_runtime::{Result, code_patches::OwnedCodePatches};
use std::sync::Arc;

#[derive(Clone)]
pub struct ResidentGparamControl {
    pipeline: GparamTaskControl,
    task: NativeTaskControl,
}
impl ResidentGparamControl {
    pub fn last_report(&self) -> Option<GparamTaskReport> {
        self.pipeline.last_report()
    }
    pub fn task_status(&self) -> NativeTaskStatus {
        self.task.status()
    }
    /// Future callbacks cancel pending work. Prior native receipts/references and the resident
    /// native task remain retained; this neither cancels resource requests nor permits unload.
    pub fn request_stop(&self) {
        self.pipeline.request_stop();
    }
}
pub struct ResidentGparamInstall {
    pub module_handle: u64,
    pub registration: Result<ResidentGparamControl>,
}
pub struct ResidentGparamSlot {
    slot: ResidentTaskSlot<ResidentGparamControl>,
}
impl Default for ResidentGparamSlot {
    fn default() -> Self {
        Self::new()
    }
}
impl ResidentGparamSlot {
    pub const fn new() -> Self {
        Self {
            slot: ResidentTaskSlot::new(),
        }
    }
    pub fn state(&self) -> ResidentGparamState {
        self.slot.state()
    }
    pub fn control(&self) -> Option<ResidentGparamControl> {
        self.slot.control()
    }
    /// Prepare once, pin this SDK module, then allocate/register exactly at DrawParamUpdate.
    /// Outer Err is pre-allocation; actual pin plus any registration failure is a retained receipt
    /// and permanently claimed slot. Dropping every public control never stops the callback.
    /// # Safety
    /// Static slot; call outside loader lock on the legal exclusive registration/allocator phase.
    /// TaskEngine's receipt-before-execution and GparamTask's complete future resource/phase/
    /// lifetime/allocator contracts hold until process exit. Retain non-SDK callback/capture DLLs,
    /// native managers, resource graphs and external patch owners; pinning covers only SDK code.
    /// Do not externally unregister this task or use settlement as proof of native resource ready.
    pub unsafe fn install_task(
        &'static self,
        engine: Arc<dyn TaskEngine>,
        task: GparamTask,
    ) -> Result<ResidentGparamInstall> {
        // SAFETY: Same static host and all current/future native contracts supplied by caller.
        unsafe { self.install(|| Ok((engine, task))) }
    }
    /// Fixed WW 1.17.1 native bindings plus the complete configured scheduler. No worker, game
    /// call, resource request or prime is performed during preparation. Delay starts at backend bind.
    /// # Safety
    /// install_task and CurrentTaskEngine/CurrentGparamTaskBackend bind/call obligations hold.
    /// Provide actual current-process module base, fixed executable bytes and full initialized
    /// resource/allocator graph in every future exclusive DrawParamUpdate phase.
    pub unsafe fn install_current(
        &'static self,
        bytes: Arc<[u8]>,
        base: u64,
        config: GparamPreloadConfig,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<ResidentGparamInstall> {
        // SAFETY: Caller supplies complete production registration and future resource contracts.
        unsafe {
            self.install(|| {
                // SAFETY: Same actual module and legal exclusive allocator/registration phase.
                let engine = CurrentTaskEngine::bind_with_patches(
                    Arc::clone(&bytes),
                    base,
                    patches.clone(),
                )?;
                // SAFETY: Same retained native resource graph and every future DrawParamUpdate phase.
                let backend = CurrentGparamTaskBackend::bind(bytes, base, patches)?;
                // SAFETY: Binder obligations hold for every eventual callback; no request occurs here.
                let task = GparamTask::new(backend, config, 0)?;
                Ok((engine as Arc<dyn TaskEngine>, task))
            })
        }
    }
    unsafe fn install(
        &'static self,
        prepare: impl FnOnce() -> Result<(Arc<dyn TaskEngine>, GparamTask)>,
    ) -> Result<ResidentGparamInstall> {
        // SAFETY: Closed public installer supplies static owner, residency and native contracts.
        let installed = unsafe {
            self.slot.install(
                TaskPhase::DRAW_PARAMETER_UPDATE,
                || {
                    let (engine, task) = prepare()?;
                    let (callback, pipeline) = task.into_parts();
                    Ok((engine, callback, pipeline))
                },
                |pipeline, task| ResidentGparamControl { pipeline, task },
            )
        }?;
        Ok(ResidentGparamInstall {
            module_handle: installed.module_handle,
            registration: installed.registration,
        })
    }
}
