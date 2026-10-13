//! Explicit process-lifetime input/effect installation. No automatic worker or game launch.
use crate::task_resident::ResidentTaskSlot;
pub use crate::task_resident::ResidentTaskState as ResidentInputEffectState;
use crate::{
    TaskPhase,
    current_input_effect_task::{CurrentInputEffectBackend, CurrentInputEffectEpochControl},
    current_tasks::CurrentTaskEngine,
    input_effect_task::{InputEffectTask, InputEffectTaskControl, InputEffectTaskReport},
    keyboard_effects::keyboard_ex_bindings,
    native_task::{NativeTaskControl, NativeTaskStatus, TaskEngine},
};
use shikifsrs_runtime::{Result, code_patches::OwnedCodePatches};
use std::sync::Arc;

#[derive(Clone)]
pub struct ResidentInputEffectControl {
    pipeline: InputEffectTaskControl,
    task: NativeTaskControl,
    epochs: Option<CurrentInputEffectEpochControl>,
}
impl ResidentInputEffectControl {
    pub fn last_report(&self) -> Option<InputEffectTaskReport> {
        self.pipeline.last_report()
    }
    pub fn task_status(&self) -> NativeTaskStatus {
        self.task.status()
    }
    /// Effects are cleaned at future FrameBegin invocations. The resident native task stays installed.
    pub fn request_cleanup(&self) {
        self.pipeline.request_cleanup();
    }
    /// Present for the built-in current keyboard backend; generic prepared tasks own their controls.
    pub fn epoch_control(&self) -> Option<CurrentInputEffectEpochControl> {
        self.epochs.clone()
    }
}
pub struct ResidentInputEffectInstall {
    /// Actual PIN|FROM_ADDRESS result; pinning persists even when registration subsequently fails.
    pub module_handle: u64,
    pub registration: Result<ResidentInputEffectControl>,
}
/// Declare one static slot per process-lifetime pipeline. Successful installation has no unload
/// path. The static slot owns its registered Rust handle, even after all public controls are dropped.
pub struct ResidentInputEffectSlot {
    slot: ResidentTaskSlot<ResidentInputEffectControl>,
}
impl Default for ResidentInputEffectSlot {
    fn default() -> Self {
        Self::new()
    }
}
impl ResidentInputEffectSlot {
    pub const fn new() -> Self {
        Self {
            slot: ResidentTaskSlot::new(),
        }
    }
    pub fn state(&self) -> ResidentInputEffectState {
        self.slot.state()
    }
    pub fn control(&self) -> Option<ResidentInputEffectControl> {
        self.slot.control()
    }
    /// Install a prepared input/effect task exactly once, retaining its Rust registration owner.
    /// Outer Err means preparation/pinning failed before any native task allocation/registration.
    /// After an actual pin, the receipt retains a registration error and the slot refuses retries.
    /// # Safety
    /// Call on a supported serialized allocator/registration phase, outside the loader lock.
    /// InputEffectTask's full future mutation-phase, target/epoch, graph lifetime, alias exclusion
    /// and TaskEngine receipt-before-execution contracts hold until process exit. SDK module pinning
    /// does not pin native objects, other callback/capture DLLs, heaps, PARAM or hook owners; caller
    /// retains those dependencies and provides genuine epochs across known lifetime transitions.
    /// Do not stop/unregister this task externally or unload other retained callback code.
    pub unsafe fn install_task(
        &'static self,
        engine: Arc<dyn TaskEngine>,
        task: InputEffectTask,
    ) -> Result<ResidentInputEffectInstall> {
        // SAFETY: Caller supplies this registration and every future invocation's complete contract.
        unsafe { self.install(|| Ok((engine, task, None))) }
    }
    /// Fixed WW 1.17.1 production bindings, all 87 keyboard-ex inputs, FrameBegin/pad0/>30.
    /// Preparation performs file/loaded evidence reads, never game calls or input sampling.
    /// # Safety
    /// install_task and all CurrentTaskEngine/CurrentInputEffectBackend bind obligations hold.
    /// Caller supplies the actual process module base, PID and true epoch, full phase/lifetime
    /// guarantees and every effect's PARAM setup. This does not spawn an installer thread or
    /// establish game-runtime acceptance, and cannot be called under DllMain's loader lock.
    pub unsafe fn install_current_keyboard(
        &'static self,
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        effect_limit: usize,
        process_id: u32,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<ResidentInputEffectInstall> {
        // SAFETY: Caller supplies all production host obligations, including future callback epochs.
        unsafe {
            self.install(|| {
                // SAFETY: Same fixed current-process module/allocator/residency and registration phase.
                let engine = CurrentTaskEngine::bind_with_patches(
                    Arc::clone(&bytes),
                    base,
                    patches.clone(),
                )?;
                // SAFETY: Caller provides each eventual FrameBegin's native graph and exclusive effect phase.
                let backend =
                    CurrentInputEffectBackend::bind(bytes, base, epoch, effect_limit, patches)?;
                let epochs = backend.epoch_control();
                // SAFETY: Same future target/PARAM/allocator/phase/epoch/residency guarantee.
                let task = InputEffectTask::new(backend, process_id, keyboard_ex_bindings()?)?;
                Ok((engine as Arc<dyn TaskEngine>, task, Some(epochs)))
            })
        }
    }
    unsafe fn install(
        &'static self,
        prepare: impl FnOnce() -> Result<PreparedResidentInput>,
    ) -> Result<ResidentInputEffectInstall> {
        // SAFETY: Public installer supplies static ownership and all future native contracts.
        let installed = unsafe {
            self.slot.install(
                TaskPhase::FRAME_BEGIN,
                || {
                    let (engine, task, epochs) = prepare()?;
                    let (callback, pipeline) = task.into_parts();
                    Ok((engine, callback, (pipeline, epochs)))
                },
                |(pipeline, epochs), task| ResidentInputEffectControl {
                    pipeline,
                    task,
                    epochs,
                },
            )
        }?;
        Ok(ResidentInputEffectInstall {
            module_handle: installed.module_handle,
            registration: installed.registration,
        })
    }
}
type PreparedResidentInput = (
    Arc<dyn TaskEngine>,
    InputEffectTask,
    Option<CurrentInputEffectEpochControl>,
);
