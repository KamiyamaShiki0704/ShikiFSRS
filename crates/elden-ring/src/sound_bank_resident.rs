//! Explicit process-resident two-phase audio pipeline, with each registration outcome retained.
pub use crate::task_resident::ResidentTaskState as ResidentSoundBankState;
use crate::{
    TaskPhase,
    current_sound_bank_api::CurrentSoundBankApi,
    current_sound_banks::CurrentSoundBankEngine,
    current_sound_conditions::{CurrentSoundConditions, SoundConditionReadConfig},
    current_tasks::CurrentTaskEngine,
    native_task::{NativeTaskControl, NativeTaskStatus, TaskEngine},
    sound_bank_schedule::{BankAssetSource, SoundBankScheduleConfig},
    sound_bank_tasks::{
        SoundBankTaskControl, SoundBankTaskInputs, SoundBankTaskReport, SoundBankTasks,
    },
    task_resident::ResidentTaskSlot,
};
use shikifsrs_runtime::{Error, Result, code_patches::OwnedCodePatches};
use std::{sync::Arc, time::Instant};

#[derive(Clone)]
pub struct ResidentSoundBankControl {
    pipeline: SoundBankTaskControl,
    startup: Result<NativeTaskControl>,
    conditional: Option<Result<NativeTaskControl>>,
}
impl ResidentSoundBankControl {
    pub fn registered_all(&self) -> bool {
        self.startup.is_ok() && self.conditional.as_ref().is_some_and(Result::is_ok)
    }
    pub fn startup_status(&self) -> Result<NativeTaskStatus> {
        self.startup
            .as_ref()
            .map(NativeTaskControl::status)
            .map_err(Clone::clone)
    }
    /// None means unattempted after startup failure, never a successful registration.
    pub fn conditional_status(&self) -> Option<Result<NativeTaskStatus>> {
        self.conditional.as_ref().map(|task| {
            task.as_ref()
                .map(NativeTaskControl::status)
                .map_err(Clone::clone)
        })
    }
    pub fn last_report(&self) -> Option<SoundBankTaskReport> {
        self.pipeline.last_report()
    }
    /// Close through future callbacks on their original thread. No native operation here;
    /// preserve unknown bank receipts, resident callbacks/modules and outstanding references.
    pub fn request_stop(&self) {
        self.pipeline.request_stop();
    }
}
pub struct ResidentSoundBankInstall {
    pub module_handle: u64,
    /// Ok contains BOTH individual outcomes, including partial failure. Inspect registered_all
    /// and status accessors; the outer Result only describes construction of the retained control.
    pub registration: Result<ResidentSoundBankControl>,
}
#[derive(Debug, Clone)]
pub struct ResidentSoundBankConfig {
    pub epoch: u64,
    pub schedule: SoundBankScheduleConfig,
    pub conditions: SoundConditionReadConfig,
    pub reference_limit: usize,
}
pub struct ResidentSoundBankSlot {
    slot: ResidentTaskSlot<ResidentSoundBankControl>,
}
impl Default for ResidentSoundBankSlot {
    fn default() -> Self {
        Self::new()
    }
}
impl ResidentSoundBankSlot {
    pub const fn new() -> Self {
        Self {
            slot: ResidentTaskSlot::new(),
        }
    }
    pub fn state(&self) -> ResidentSoundBankState {
        self.slot.state()
    }
    pub fn control(&self) -> Option<ResidentSoundBankControl> {
        self.slot.control()
    }
    /// Pin the SDK module once and register SoundStep, then FrameBegin. Stop after the first
    /// registration failure; queue bank close through any retained startup callback. A partial
    /// receipt is terminal for installation, including when every bank later closes normally.
    /// # Safety
    /// Static host, outside loader lock, legal exclusive allocator/registration phase and all
    /// TaskEngine receipt-before-execution guarantees. SoundBankTasks' complete native graph,
    /// audio wait/phase, original allocator and one stable callback-thread obligations hold for
    /// every eventual invocation until process exit. Retain non-SDK capture/hook/DLLs and native
    /// managers/plugins/bank obligations. Never externally unregister or unload these tasks.
    pub unsafe fn install_tasks(
        &'static self,
        engine: Arc<dyn TaskEngine>,
        tasks: SoundBankTasks,
    ) -> Result<ResidentSoundBankInstall> {
        // SAFETY: Static owner and full registration/future audio contracts supplied by caller.
        unsafe { self.install(|| Ok((engine, tasks))) }
    }
    /// Actual current task engine and callback-created thread-confined Wwise/condition adapters.
    /// Only the consumer's asset source resolves/reads files. No worker, asset read, bank/audio
    /// call or condition poll occurs during preparation. Delay uses one installation clock.
    /// # Safety
    /// install_tasks' contracts and all current audio/condition bind/call obligations hold.
    /// Provide the actual fixed WW executable/module and full initialized stable audio manager,
    /// allocator/plugins/synchronous queue and every future observation graph/phase. The epoch
    /// identifies each fresh read's supplied scope; no character identities are retained between
    /// condition polls. Caller excludes reuse/teardown while copying. Asset bytes are valid and
    /// compatible banks; successful loading does not independently prove resource/audio readiness.
    pub unsafe fn install_current<S: BankAssetSource + Send + 'static>(
        &'static self,
        bytes: Arc<[u8]>,
        base: u64,
        config: ResidentSoundBankConfig,
        source: S,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<ResidentSoundBankInstall> {
        let ResidentSoundBankConfig {
            epoch,
            schedule,
            conditions,
            reference_limit,
        } = config;
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero resident sound condition epoch"));
        }
        conditions.validate()?;
        let started = Instant::now();
        // SAFETY: Caller supplies all production current/future graph, phase and residency obligations.
        unsafe {
            self.install(|| {
                // SAFETY: Actual native module and serialized registration/allocator phase.
                let engine = CurrentTaskEngine::bind_with_patches(
                    Arc::clone(&bytes),
                    base,
                    patches.clone(),
                )?;
                // SAFETY: Future callbacks share one stable original thread and the full audio contracts.
                let tasks = SoundBankTasks::new(
                    move || {
                        // SAFETY: On the callback's original thread, caller retains complete Wwise graph/phase.
                        let api =
                            CurrentSoundBankApi::bind(Arc::clone(&bytes), base, patches.clone())?;
                        // SAFETY: Same stable original manager, allocator/plugins and permitted synchronous queue.
                        let audio = CurrentSoundBankEngine::new(api)?;
                        // SAFETY: Caller retains each fresh observation graph and excludes reuse during its copies.
                        let reader =
                            CurrentSoundConditions::bind(bytes, base, patches, conditions)?;
                        Ok(SoundBankTaskInputs {
                            engine: audio,
                            source,
                            observe: move || reader.targets(epoch),
                            clock: move || {
                                u64::try_from(started.elapsed().as_millis())
                                    .map_err(|_| Error::AddressOverflow)
                            },
                        })
                    },
                    schedule,
                    0,
                    reference_limit,
                )?;
                Ok((engine as Arc<dyn TaskEngine>, tasks))
            })
        }
    }
    unsafe fn install(
        &'static self,
        prepare: impl FnOnce() -> Result<(Arc<dyn TaskEngine>, SoundBankTasks)>,
    ) -> Result<ResidentSoundBankInstall> {
        // SAFETY: Closed pair installer supplies static owner and all native contracts for both phases.
        let installed = unsafe {
            self.slot.install_group(
                [TaskPhase::SOUND_STEP, TaskPhase::FRAME_BEGIN],
                || {
                    let (engine, tasks) = prepare()?;
                    let (callbacks, pipeline) = tasks.into_parts();
                    Ok((engine, callbacks, pipeline))
                },
                |pipeline, [startup, conditional]| {
                    let startup = startup
                        .ok_or(Error::InvalidProfile("resident startup task not attempted"))?;
                    if startup.is_err() || conditional.as_ref().is_some_and(Result::is_err) {
                        pipeline.request_stop();
                    }
                    Ok(ResidentSoundBankControl {
                        pipeline,
                        startup,
                        conditional,
                    })
                },
            )
        }?;
        Ok(ResidentSoundBankInstall {
            module_handle: installed.module_handle,
            registration: installed.registration,
        })
    }
}
