//! Shared process-lifetime registration for the SDK's closed resident pipelines.
use crate::{
    TaskCallback, TaskPhase,
    native_task::{NativeTaskControl, PreparedTask, RegisteredTask, TaskEngine},
};
use shikifsrs_runtime::{Error, Result};
use std::{
    ffi::c_void,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidentTaskState {
    Vacant,
    Preparing,
    ModulePinned,
    Registered,
    RegistrationFailed,
}
impl ResidentTaskState {
    fn byte(self) -> u8 {
        match self {
            Self::Vacant => 0,
            Self::Preparing => 1,
            Self::ModulePinned => 2,
            Self::Registered => 3,
            Self::RegistrationFailed => 4,
        }
    }
    fn from_byte(value: u8) -> Self {
        match value {
            0 => Self::Vacant,
            1 => Self::Preparing,
            2 => Self::ModulePinned,
            3 => Self::Registered,
            _ => Self::RegistrationFailed,
        }
    }
}
struct Retained<C> {
    _tasks: Vec<RegisteredTask>,
    control: Option<C>,
}
pub(crate) struct ResidentTaskSlot<C> {
    state: AtomicU8,
    retained: Mutex<Option<Retained<C>>>,
}
pub(crate) struct ResidentTaskInstall<C> {
    pub module_handle: u64,
    pub registration: Result<C>,
}
type PreparedResident<P> = (Arc<dyn TaskEngine>, TaskCallback, P);
impl<C: Clone> ResidentTaskSlot<C> {
    pub(crate) const fn new() -> Self {
        Self {
            state: AtomicU8::new(0),
            retained: Mutex::new(None),
        }
    }
    pub(crate) fn state(&self) -> ResidentTaskState {
        ResidentTaskState::from_byte(self.state.load(Ordering::Acquire))
    }
    pub(crate) fn control(&self) -> Option<C> {
        self.retained
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|v| v.control.clone())
    }
    /// # Safety
    /// The public pipeline installer supplies a static slot, serialized registration/allocator,
    /// receipt-before-execution and every future callback's complete native contracts. Called
    /// outside loader lock; all non-SDK code/graphs/captures remain resident until process exit.
    pub(crate) unsafe fn install<P>(
        &'static self,
        phase: TaskPhase,
        prepare: impl FnOnce() -> Result<PreparedResident<P>>,
        controls: impl FnOnce(P, NativeTaskControl) -> C,
    ) -> Result<ResidentTaskInstall<C>> {
        // SAFETY: The same closed pipeline contracts; one callback preserves its existing API.
        unsafe {
            self.install_group(
                [phase],
                || {
                    let (engine, callback, pipeline) = prepare()?;
                    Ok((engine, [callback], pipeline))
                },
                |pipeline, [task]| {
                    let task =
                        task.ok_or(Error::InvalidProfile("resident task not attempted"))??;
                    Ok(controls(pipeline, task))
                },
            )
        }
    }
    /// Closed one/two-phase pipelines only; stop registration after its first failure.
    /// Successful task owners survive every partial result and all public controls being dropped.
    /// # Safety
    /// install's complete contract applies to each phase. Callback owners/control construction
    /// are SDK-owned, do not invoke native operations and cannot release registered obligations.
    pub(crate) unsafe fn install_group<P, const N: usize>(
        &'static self,
        phases: [TaskPhase; N],
        prepare: impl FnOnce() -> Result<(Arc<dyn TaskEngine>, [TaskCallback; N], P)>,
        controls: impl FnOnce(P, [Option<Result<NativeTaskControl>>; N]) -> Result<C>,
    ) -> Result<ResidentTaskInstall<C>> {
        if !(1..=2).contains(&N) {
            return Err(Error::InvalidProfile("invalid resident phase group"));
        }
        self.state
            .compare_exchange(
                ResidentTaskState::Vacant.byte(),
                ResidentTaskState::Preparing.byte(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| {
                Error::NativeOperationRejected("resident task installation already claimed")
            })?;
        let _claim = Claim(&self.state);
        let (engine, callbacks, pipeline) = prepare()?;
        // Allocate the bounded Rust owner journal before module pin/native allocation.
        let mut retained = Vec::with_capacity(N);
        let module_handle = pin_sdk_module()?;
        self.state
            .store(ResidentTaskState::ModulePinned.byte(), Ordering::Release);
        let mut tasks = std::array::from_fn(|_| None);
        let mut failed = false;
        for (index, (phase, callback)) in phases.into_iter().zip(callbacks).enumerate() {
            if failed {
                break;
            }
            // SAFETY: Closed installer supplies allocator/phase/residency for this callback.
            let result =
                unsafe { PreparedTask::new(Arc::clone(&engine), callback) }.and_then(|prepared| {
                    // SAFETY: Same serialized registration and receipt-before-execution contract.
                    unsafe { prepared.register(phase) }
                });
            tasks[index] = Some(match result {
                Ok(registered) => {
                    let control = registered.control();
                    retained.push(registered);
                    Ok(control)
                }
                Err(error) => {
                    failed = true;
                    Err(error)
                }
            });
        }
        let registration = controls(pipeline, tasks);
        // Sole publisher; no native/user callback runs under this lock. Partial successful
        // owners remain retained even if the closed control constructor reports an error.
        *self.retained.lock().unwrap_or_else(|e| e.into_inner()) = Some(Retained {
            _tasks: retained,
            control: registration.as_ref().ok().cloned(),
        });
        self.state.store(
            if !failed && registration.is_ok() {
                ResidentTaskState::Registered.byte()
            } else {
                ResidentTaskState::RegistrationFailed.byte()
            },
            Ordering::Release,
        );
        Ok(ResidentTaskInstall {
            module_handle,
            registration,
        })
    }
}
/// Process-resident owner for one or two consumer-authored callbacks.
/// This is a task lifetime mechanism, not a native graph lease or shutdown policy.
pub struct ResidentCallbackGroupSlot<const N: usize> {
    slot: ResidentTaskSlot<ResidentCallbackGroupControl<N>>,
}
#[derive(Clone)]
pub struct ResidentCallbackGroupControl<const N: usize> {
    outcomes: [Option<Result<NativeTaskControl>>; N],
}
impl<const N: usize> ResidentCallbackGroupControl<N> {
    pub fn outcomes(&self) -> &[Option<Result<NativeTaskControl>>; N] {
        &self.outcomes
    }
    pub fn registered_all(&self) -> bool {
        self.outcomes
            .iter()
            .all(|outcome| outcome.as_ref().is_some_and(Result::is_ok))
    }
    /// Stop Rust closures only. Callers must first close their resource obligations,
    /// or keep them resident. Does not unregister, free tasks, unpin or unload modules.
    pub fn stop_callbacks(&self) {
        for task in self
            .outcomes
            .iter()
            .flatten()
            .filter_map(|outcome| outcome.as_ref().ok())
        {
            task.stop_callback();
        }
    }
}
pub struct ResidentCallbackGroupInstall<const N: usize> {
    pub module_handle: u64,
    pub registration: Result<ResidentCallbackGroupControl<N>>,
}
impl<const N: usize> Default for ResidentCallbackGroupSlot<N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<const N: usize> ResidentCallbackGroupSlot<N> {
    pub const fn new() -> Self {
        Self {
            slot: ResidentTaskSlot::new(),
        }
    }
    pub fn state(&self) -> ResidentTaskState {
        self.slot.state()
    }
    pub fn control(&self) -> Option<ResidentCallbackGroupControl<N>> {
        self.slot.control()
    }
    /// Register in order, stopping after the first failure. Both individual receipts
    /// remain observable, and successful task owners survive dropped public controls.
    /// # Safety
    /// Static slot outside loader lock. Caller provides actual serialized allocator/
    /// registration phases and every TaskEngine receipt-before-execution obligation.
    /// Engine/heap/module, capture code and complete callback native graphs remain live
    /// until process exit. Independently establish each future callback's phase, thread
    /// affinity and synchronization. No external unregister/unload. Partial installation
    /// requires consumer-specific reconciliation; never infer close from Rust cancellation.
    pub unsafe fn install(
        &'static self,
        engine: Arc<dyn TaskEngine>,
        phases: [TaskPhase; N],
        callbacks: [TaskCallback; N],
    ) -> Result<ResidentCallbackGroupInstall<N>> {
        // SAFETY: Caller supplies all allocator, registration and future callback contracts.
        let installed = unsafe {
            self.slot.install_group(
                phases,
                || Ok((engine, callbacks, ())),
                |(), outcomes| Ok(ResidentCallbackGroupControl { outcomes }),
            )?
        };
        Ok(ResidentCallbackGroupInstall {
            module_handle: installed.module_handle,
            registration: installed.registration,
        })
    }
}
struct Claim<'a>(&'a AtomicU8);
impl Drop for Claim<'_> {
    fn drop(&mut self) {
        // Only pre-pin preparation can reopen a claim; no native teardown/repeated attempt.
        let _ = self.0.compare_exchange(
            ResidentTaskState::Preparing.byte(),
            ResidentTaskState::Vacant.byte(),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}
#[inline(never)]
fn pin_sdk_module() -> Result<u64> {
    let mut module: *mut c_void = std::ptr::null_mut();
    // SAFETY: FROM_ADDRESS consumes a live SDK function address; output is live. PIN retains
    // this code module until process exit, without any game process lookup or native graph pin.
    let ok =
        unsafe { GetModuleHandleExW(0x5, pin_sdk_module as *const () as *const u16, &mut module) };
    if ok == 0 {
        // SAFETY: Immediate error query after the failed Windows lookup/pin.
        let code = unsafe { GetLastError() };
        return Err(Error::WindowsCall {
            operation: "GetModuleHandleExW(PIN|FROM_ADDRESS)",
            code,
        });
    }
    if module.is_null() {
        return Err(Error::InvalidProfile("resident module pin returned null"));
    }
    Ok(module as usize as u64)
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut *mut c_void) -> i32;
    fn GetLastError() -> u32;
}
