//! Two native phases sharing one explicitly thread-confined bank owner.
use crate::{
    TaskCallback, TaskInvocation, TaskPhase,
    sound_bank_schedule::{
        BankAssetSource, SoundBankSchedule, SoundBankScheduleConfig, SoundBankScheduleReport,
    },
    sound_banks::{BankCloseProgress, SoundBankEngine, SoundBankOwner},
    task_owner::{NativeTaskOwner, OwnedTaskId},
};
use shikifsrs_runtime::{Error, Result};
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, ThreadId},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundBankTaskState {
    Dormant,
    Running,
    Closing,
    Closed,
    Failed,
}
#[derive(Debug, Clone)]
pub struct SoundBankTaskReport {
    pub phase: TaskPhase,
    pub invocation: TaskInvocation,
    pub state: SoundBankTaskState,
    pub schedule: Option<SoundBankScheduleReport>,
    pub close: Option<BankCloseProgress>,
    pub error: Option<Error>,
}
struct Reports {
    stop: AtomicBool,
    last: Mutex<Option<SoundBankTaskReport>>,
}
#[derive(Clone)]
pub struct SoundBankTaskControl {
    reports: Arc<Reports>,
}
impl SoundBankTaskControl {
    /// Queues close for the original task thread. Callbacks must remain registered
    /// until Closed; stopping/unregistering them first cannot release bank references.
    /// This is no DLL-unload permission and performs no native operation here.
    pub fn request_stop(&self) {
        self.reports.stop.store(true, Ordering::Release);
    }
    pub fn last_report(&self) -> Option<SoundBankTaskReport> {
        self.reports
            .last
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}
/// Inputs are created only on the first callback, and may contain !Send objects.
/// The factory itself is Send; the native engine/session never moves to another thread.
pub struct SoundBankTaskInputs<E, S, O, C> {
    pub engine: E,
    pub source: S,
    pub observe: O,
    pub clock: C,
}
struct Session<E: SoundBankEngine, S, O, C> {
    owner: SoundBankOwner<E>,
    schedule: SoundBankSchedule,
    source: S,
    observe: O,
    clock: C,
}
type BankTaskFactory<E, S, O, C> =
    Box<dyn FnOnce() -> Result<SoundBankTaskInputs<E, S, O, C>> + Send>;
struct Host<E: SoundBankEngine, S, O, C> {
    factory: Option<BankTaskFactory<E, S, O, C>>,
    config: SoundBankScheduleConfig,
    started_ms: u64,
    limit: usize,
    thread: Option<ThreadId>,
    address: Option<usize>,
    state: SoundBankTaskState,
    failure: Option<Error>,
}
// address is an opaque boxed Session handle. It is only dereferenced or freed on
// its creating ThreadId under the host mutex; ThreadIds are never reused. No Send
// implementation, native session transfer, foreign-thread Drop or implicit cleanup.
// Unclosed/uncertain sessions intentionally remain resident if callbacks disappear.
impl<
    E: SoundBankEngine,
    S: BankAssetSource,
    O: FnMut() -> Result<Option<BTreeSet<u32>>>,
    C: FnMut() -> Result<u64>,
> Host<E, S, O, C>
{
    unsafe fn run(
        &mut self,
        phase: TaskPhase,
        stop: bool,
    ) -> (
        Option<SoundBankScheduleReport>,
        Option<BankCloseProgress>,
        Option<Error>,
    ) {
        if let Some(original) = self.thread
            && original != thread::current().id()
        {
            return (
                None,
                None,
                Some(Error::NativeOperationRejected(
                    "bank task moved to another thread",
                )),
            );
        }
        if self.state == SoundBankTaskState::Closed || self.state == SoundBankTaskState::Failed {
            return (None, None, self.failure.clone());
        }
        if self.address.is_none() {
            if stop || !self.config.enabled {
                self.factory = None;
                self.state = SoundBankTaskState::Closed;
                return (None, None, None);
            }
            let create = || -> Result<Session<E, S, O, C>> {
                let inputs = self.factory.take().expect("single bank factory")()?;
                let owner = SoundBankOwner::new(inputs.engine, self.limit)?;
                let schedule =
                    SoundBankSchedule::new(self.config.clone(), self.started_ms, &owner)?;
                Ok(Session {
                    owner,
                    schedule,
                    source: inputs.source,
                    observe: inputs.observe,
                    clock: inputs.clock,
                })
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(create));
            let session = match result {
                Ok(Ok(session)) => session,
                Ok(Err(error)) => {
                    self.state = SoundBankTaskState::Failed;
                    self.failure = Some(error.clone());
                    return (None, None, Some(error));
                }
                Err(_) => {
                    let error = Error::NativeOperationRejected("bank task factory panicked");
                    self.state = SoundBankTaskState::Failed;
                    self.failure = Some(error.clone());
                    return (None, None, Some(error));
                }
            };
            self.thread = Some(thread::current().id());
            self.address = Some(Box::into_raw(Box::new(session)) as usize);
            self.state = SoundBankTaskState::Running;
        }
        let address = self.address.expect("live thread session");
        // SAFETY: Stable Box, exact creator thread, exclusive host mutex. It has
        // never been freed and no references escape this callback.
        let session = unsafe { &mut *(address as *mut Session<E, S, O, C>) };
        let operation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || -> Result<Option<BankCloseProgress>> {
                if stop {
                    self.state = SoundBankTaskState::Closing;
                    // SAFETY: Constructor's caller retains the original unload phase.
                    return unsafe { session.schedule.close_step(&mut session.owner) }.map(Some);
                }
                if phase == TaskPhase::SOUND_STEP {
                    let now = (session.clock)()?;
                    // SAFETY: Original-thread callback and constructor's native graph contract.
                    unsafe {
                        session.schedule.startup_frame(
                            now,
                            &mut session.owner,
                            &mut session.source,
                        )?;
                    }
                } else {
                    // SAFETY: Same original thread; independently permitted FrameBegin
                    // observation AND synchronous load/unload phase are caller obligations.
                    unsafe {
                        session.schedule.conditional_frame(
                            &mut session.owner,
                            &mut session.source,
                            &mut session.observe,
                        )?;
                    }
                }
                Ok(None)
            },
        ));
        let schedule = session.schedule.report(&session.owner).ok();
        match operation {
            Ok(Ok(close)) => {
                if close.as_ref().is_some_and(|c| {
                    c.remaining_requests == 0
                        && c.remaining_references == 0
                        && c.uncertain_loads.is_empty()
                        && c.uncertain_banks.is_empty()
                        && c.failures.is_empty()
                }) {
                    self.address = None;
                    self.state = SoundBankTaskState::Closed;
                    // SAFETY: Correct thread, no session references remain in use,
                    // all original obligations closed; Engine Drop has no native calls.
                    unsafe {
                        drop(Box::from_raw(address as *mut Session<E, S, O, C>));
                    }
                }
                (schedule, close, None)
            }
            Ok(Err(error)) => (schedule, None, Some(error)),
            Err(_) => {
                let error = Error::NativeOperationRejected(
                    "bank task operation panicked; retained session requires reconciliation",
                );
                self.state = SoundBankTaskState::Failed;
                self.failure = Some(error.clone());
                (schedule, None, Some(error))
            }
        }
    }
}
pub struct SoundBankTasks {
    startup: TaskCallback,
    conditional: TaskCallback,
    control: SoundBankTaskControl,
}
pub struct SoundBankTaskRegistration {
    pub startup: Result<OwnedTaskId>,
    pub conditional: Option<Result<OwnedTaskId>>,
    pub control: SoundBankTaskControl,
}
impl SoundBankTasks {
    /// No factory, clock, asset, observation or native operation runs here.
    /// # Safety
    /// Both phases must execute on one stable thread; mismatch rejects before
    /// accessing the session. Every invocation has independently permitted bank
    /// loading/unloading, observation, original allocator and protected dependency
    /// contracts. Factory only binds/copies; it must not acquire native obligations
    /// before returning its engine. Host/SDK/module and unknown sessions remain
    /// resident until every bank and callback obligation is independently resolved.
    /// Do not unregister callbacks before queued bank close completes. No hot unload.
    pub unsafe fn new<E, S, O, C, F>(
        factory: F,
        config: SoundBankScheduleConfig,
        started_ms: u64,
        limit: usize,
    ) -> Result<Self>
    where
        E: SoundBankEngine + 'static,
        S: BankAssetSource + 'static,
        O: FnMut() -> Result<Option<BTreeSet<u32>>> + 'static,
        C: FnMut() -> Result<u64> + 'static,
        F: FnOnce() -> Result<SoundBankTaskInputs<E, S, O, C>> + Send + 'static,
    {
        if config.static_ids.len() > 256
            || config.attempts_per_id == 0
            || config.retry_every_frames == 0
            || config.poll_every_frames == 0
            || !(1..=65536).contains(&limit)
        {
            return Err(Error::InvalidProfile("invalid bank task bounds"));
        }
        started_ms
            .checked_add(config.start_delay_ms)
            .ok_or(Error::AddressOverflow)?;
        let host = Arc::new(Mutex::new(Host {
            factory: Some(Box::new(factory)),
            config,
            started_ms,
            limit,
            thread: None,
            address: None,
            state: SoundBankTaskState::Dormant,
            failure: None,
        }));
        let reports = Arc::new(Reports {
            stop: AtomicBool::new(false),
            last: Mutex::new(None),
        });
        let make = |phase| {
            let host = Arc::clone(&host);
            let reports = Arc::clone(&reports);
            TaskCallback::new(move |invocation| {
                let mut host = match host.try_lock() {
                    Ok(host) => host,
                    Err(_) => return,
                };
                // SAFETY: Constructor's caller provides every callback graph/phase contract.
                let (schedule, close, error) =
                    unsafe { host.run(phase, reports.stop.load(Ordering::Acquire)) };
                let mut previous = reports.last.lock().unwrap_or_else(|e| e.into_inner());
                let schedule = schedule.or_else(|| {
                    previous
                        .as_ref()
                        .and_then(|r: &SoundBankTaskReport| r.schedule.clone())
                });
                let close = close.or_else(|| previous.as_ref().and_then(|r| r.close.clone()));
                *previous = Some(SoundBankTaskReport {
                    phase,
                    invocation,
                    state: host.state,
                    schedule,
                    close,
                    error,
                });
            })
        };
        Ok(Self {
            startup: make(TaskPhase::SOUND_STEP),
            conditional: make(TaskPhase::FRAME_BEGIN),
            control: SoundBankTaskControl { reports },
        })
    }
    pub fn control(&self) -> SoundBankTaskControl {
        self.control.clone()
    }
    pub(crate) fn into_parts(self) -> ([TaskCallback; 2], SoundBankTaskControl) {
        ([self.startup, self.conditional], self.control)
    }
    /// Both outcomes survive partial registration. A second failure queues close
    /// through the retained startup callback; owner still tracks its task receipt.
    /// # Safety
    /// NativeTaskOwner registration, allocator, serialization and residency hold,
    /// along with new's obligations for every eventual phase invocation.
    pub unsafe fn register(self, owner: &mut NativeTaskOwner) -> SoundBankTaskRegistration {
        // SAFETY: Same explicit owner registration contract.
        let startup = unsafe { owner.register(TaskPhase::SOUND_STEP, self.startup) };
        let conditional = if startup.is_ok() {
            // SAFETY: Same owner contract, now for FrameBegin.
            Some(unsafe { owner.register(TaskPhase::FRAME_BEGIN, self.conditional) })
        } else {
            None
        };
        if startup.is_err() || conditional.as_ref().is_some_and(Result::is_err) {
            self.control.request_stop();
        }
        SoundBankTaskRegistration {
            startup,
            conditional,
            control: self.control,
        }
    }
}
