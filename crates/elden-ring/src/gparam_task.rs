//! Bounded DrawParamUpdate submission scheduler. Submitted is not resource-ready.
use crate::{
    TaskCallback, TaskInvocation, TaskPhase,
    gparam::{CommonEventId, NativeGparamCall},
    task_owner::{NativeTaskOwner, OwnedTaskId},
};
use shikifsrs_runtime::{Error, Result};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone)]
pub struct GparamPreloadConfig {
    pub enabled: bool,
    pub ids: Vec<CommonEventId>,
    pub start_delay_ms: u64,
    pub attempts_per_id: u32,
    pub retry_every_frames: u32,
    pub request_filecap: bool,
    pub prime_drawparam: bool,
}
impl Default for GparamPreloadConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ids: vec![CommonEventId::new(5).expect("bounded default")],
            start_delay_ms: 5000,
            attempts_per_id: 120,
            retry_every_frames: 60,
            request_filecap: true,
            prime_drawparam: true,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GparamStage {
    FileRequest,
    Prime,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GparamSubmissionState {
    Pending,
    /// Stored before dispatch, including if the backend panics or never returns.
    Calling(GparamStage),
    Submitted,
    NoOperations,
    Disabled,
    Cancelled,
    Exhausted,
    /// Actual invocation may have changed native state. Never automatically retry.
    Uncertain(GparamStage),
}
impl GparamSubmissionState {
    fn terminal(self) -> bool {
        !matches!(self, Self::Pending | Self::Calling(_))
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GparamSubmission {
    pub ordinal: usize,
    pub id: CommonEventId,
    pub attempts: u32,
    pub state: GparamSubmissionState,
    /// Borrowed opaque address, not an owned resource reference.
    pub file_request: Option<NativeGparamCall<Option<u64>>>,
    pub prime: Option<NativeGparamCall<()>>,
    pub last_error: Option<Error>,
}

/// # Safety
/// Err is definitely before invocation, with no submission side effects; only
/// such failures may be retried. Ok always retains the actual result/postflight,
/// with the requested ID and independently checked manager. Backend calls run
/// synchronously; panic/nonreturn may have invoked and is never retryable.
pub unsafe trait GparamTaskBackend: Send + 'static {
    fn elapsed_ms(&self) -> Result<u64>;
    /// # Safety
    /// Caller holds the backend's live exclusive resource phase and full graph.
    unsafe fn request(&mut self, id: CommonEventId) -> Result<NativeGparamCall<Option<u64>>>;
    /// # Safety
    /// Caller holds the backend's live exclusive DrawParam phase and full graph.
    unsafe fn prime(&mut self, id: CommonEventId) -> Result<NativeGparamCall<()>>;
}

pub struct GparamPreloader {
    config: GparamPreloadConfig,
    entries: Vec<GparamSubmission>,
    deadline_ms: u64,
    last_ms: u64,
    frame: u64,
    next_due_frame: u64,
}
impl GparamPreloader {
    /// Preserves configured ordering, including duplicate IDs. No backend calls.
    pub fn new(config: GparamPreloadConfig, started_ms: u64) -> Result<Self> {
        if config.ids.len() > 256 || config.attempts_per_id == 0 || config.retry_every_frames == 0 {
            return Err(Error::InvalidProfile("invalid Gparam preload bounds"));
        }
        let deadline_ms = started_ms
            .checked_add(config.start_delay_ms)
            .ok_or(Error::AddressOverflow)?;
        let entries = config
            .ids
            .iter()
            .copied()
            .enumerate()
            .map(|(ordinal, id)| GparamSubmission {
                ordinal,
                id,
                attempts: 0,
                state: if config.enabled {
                    GparamSubmissionState::Pending
                } else {
                    GparamSubmissionState::Disabled
                },
                file_request: None,
                prime: None,
                last_error: None,
            })
            .collect();
        Ok(Self {
            config,
            entries,
            deadline_ms,
            last_ms: started_ms,
            frame: 0,
            next_due_frame: 1,
        })
    }
    pub fn submissions(&self) -> &[GparamSubmission] {
        &self.entries
    }
    pub fn frame(&self) -> u64 {
        self.frame
    }
    /// Includes failed/uncertain/cancelled terminal records; not success or readiness.
    pub fn settled(&self) -> bool {
        self.entries.iter().all(|entry| entry.state.terminal())
    }
    /// An uncertain actual call blocks all further submissions until the host
    /// independently reconciles it. This scheduler has no retry/reset escape.
    pub fn blocked(&self) -> bool {
        self.entries.iter().any(|entry| {
            matches!(
                entry.state,
                GparamSubmissionState::Calling(_) | GparamSubmissionState::Uncertain(_)
            )
        })
    }
    pub fn cancel_pending(&mut self) {
        for entry in &mut self.entries {
            if entry.state == GparamSubmissionState::Pending {
                entry.state = GparamSubmissionState::Cancelled;
            }
        }
    }
    /// False means delay/cadence/no remaining work. Clock rollback and arithmetic
    /// overflow reject before any invocation, without consuming an attempt.
    /// # Safety
    /// Caller holds a permitted live resource/DrawParam mutation phase, no ABA,
    /// teardown or aliases, and all backend transitive lifetime/allocator contracts.
    pub unsafe fn tick<B: GparamTaskBackend>(
        &mut self,
        now_ms: u64,
        backend: &mut B,
    ) -> Result<bool> {
        if now_ms < self.last_ms {
            return Err(Error::InvalidProfile("Gparam clock moved backwards"));
        }
        self.last_ms = now_ms;
        if self.settled() || self.blocked() || now_ms < self.deadline_ms {
            return Ok(false);
        }
        let frame = self.frame.checked_add(1).ok_or(Error::AddressOverflow)?;
        if frame < self.next_due_frame {
            self.frame = frame;
            return Ok(false);
        }
        let next = frame
            .checked_add(u64::from(self.config.retry_every_frames))
            .ok_or(Error::AddressOverflow)?;
        self.frame = frame;
        self.next_due_frame = next;
        for entry in &mut self.entries {
            if entry.state != GparamSubmissionState::Pending {
                continue;
            }
            if !self.config.request_filecap && !self.config.prime_drawparam {
                entry.state = GparamSubmissionState::NoOperations;
                continue;
            }
            entry.attempts += 1; // Limited by a nonzero u32 budget and terminal exhaustion.
            entry.last_error = None;
            if self.config.request_filecap && entry.file_request.is_none() {
                entry.state = GparamSubmissionState::Calling(GparamStage::FileRequest);
                // SAFETY: tick's caller establishes this invocation's full backend contract.
                let call = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    backend.request(entry.id)
                }));
                match call {
                    Ok(Err(error)) => {
                        preflight_failure(entry, error, self.config.attempts_per_id);
                        continue;
                    }
                    Err(_) => {
                        uncertain(
                            entry,
                            GparamStage::FileRequest,
                            Error::NativeOperationRejected(
                                "Gparam request panicked after dispatch began",
                            ),
                        );
                        break;
                    }
                    Ok(Ok(receipt)) => {
                        let failure = if receipt.id != entry.id {
                            Some(Error::NativeOperationRejected(
                                "Gparam request receipt has foreign ID",
                            ))
                        } else if let Err(error) = &receipt.postflight {
                            Some(error.clone())
                        } else if receipt.value.is_none() {
                            Some(Error::NativeOperationRejected(
                                "Gparam request returned no handle; submission is uncertain",
                            ))
                        } else {
                            None
                        };
                        entry.file_request = Some(receipt);
                        if let Some(error) = failure {
                            uncertain(entry, GparamStage::FileRequest, error);
                            break;
                        }
                    }
                }
            }
            if self.config.prime_drawparam && entry.prime.is_none() {
                entry.state = GparamSubmissionState::Calling(GparamStage::Prime);
                // SAFETY: Same live exclusive phase, retaining any prior file-request receipt.
                let call = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    backend.prime(entry.id)
                }));
                match call {
                    Ok(Err(error)) => {
                        preflight_failure(entry, error, self.config.attempts_per_id);
                        continue;
                    }
                    Err(_) => {
                        uncertain(
                            entry,
                            GparamStage::Prime,
                            Error::NativeOperationRejected(
                                "Gparam prime panicked after dispatch began",
                            ),
                        );
                        break;
                    }
                    Ok(Ok(receipt)) => {
                        let failure = if receipt.id != entry.id {
                            Some(Error::NativeOperationRejected(
                                "Gparam prime receipt has foreign ID",
                            ))
                        } else {
                            receipt.postflight.clone().err()
                        };
                        entry.prime = Some(receipt);
                        if let Some(error) = failure {
                            uncertain(entry, GparamStage::Prime, error);
                            break;
                        }
                    }
                }
            }
            entry.state = GparamSubmissionState::Submitted;
        }
        Ok(true)
    }
}
fn preflight_failure(entry: &mut GparamSubmission, error: Error, budget: u32) {
    entry.last_error = Some(error);
    entry.state = if entry.attempts >= budget {
        GparamSubmissionState::Exhausted
    } else {
        GparamSubmissionState::Pending
    };
}
fn uncertain(entry: &mut GparamSubmission, stage: GparamStage, error: Error) {
    entry.last_error = Some(error);
    entry.state = GparamSubmissionState::Uncertain(stage);
}

#[derive(Debug, Clone)]
pub struct GparamTaskReport {
    pub invocation: TaskInvocation,
    pub frame: u64,
    pub ran: bool,
    pub closing: bool,
    pub settled: bool,
    pub blocked: bool,
    pub error: Option<Error>,
    pub submissions: Vec<GparamSubmission>,
}
struct Shared {
    closing: AtomicBool,
    report: Mutex<Option<GparamTaskReport>>,
}
#[derive(Clone)]
pub struct GparamTaskControl {
    shared: Arc<Shared>,
}
impl GparamTaskControl {
    /// Stops future pending submissions; does not cancel prior native requests,
    /// free borrowed resources, unregister the task or certify DLL unload.
    pub fn request_stop(&self) {
        self.shared.closing.store(true, Ordering::Release);
    }
    pub fn last_report(&self) -> Option<GparamTaskReport> {
        self.shared
            .report
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}
pub struct GparamTask {
    callback: TaskCallback,
    control: GparamTaskControl,
}
impl GparamTask {
    /// No clock sample, native binding, resource request or thread is created here.
    /// # Safety
    /// Every eventual DrawParamUpdate invocation meets tick/backend native phase,
    /// lifetime and allocator contracts. SDK/host/backend reside through all frames.
    pub unsafe fn new<B: GparamTaskBackend>(
        mut backend: B,
        config: GparamPreloadConfig,
        started_ms: u64,
    ) -> Result<Self> {
        let mut preloader = GparamPreloader::new(config, started_ms)?;
        let shared = Arc::new(Shared {
            closing: AtomicBool::new(false),
            report: Mutex::new(None),
        });
        let callback_shared = Arc::clone(&shared);
        let callback = TaskCallback::new(move |invocation| {
            let closing = callback_shared.closing.load(Ordering::Acquire);
            let outcome = if closing {
                preloader.cancel_pending();
                Ok(false)
            } else if preloader.settled() || preloader.blocked() {
                Ok(false)
            } else {
                backend.elapsed_ms().and_then(|now| {
                    // SAFETY: new's caller guarantees the live native phase for every frame.
                    unsafe { preloader.tick(now, &mut backend) }
                })
            };
            let report = GparamTaskReport {
                invocation,
                frame: preloader.frame(),
                ran: outcome.as_ref().is_ok_and(|ran| *ran),
                closing,
                settled: preloader.settled(),
                blocked: preloader.blocked(),
                error: outcome.err(),
                submissions: preloader.submissions().to_vec(),
            };
            *callback_shared
                .report
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(report);
        });
        Ok(Self {
            callback,
            control: GparamTaskControl { shared },
        })
    }
    pub fn control(&self) -> GparamTaskControl {
        self.control.clone()
    }
    pub(crate) fn into_parts(self) -> (TaskCallback, GparamTaskControl) {
        (self.callback, self.control)
    }
    /// # Safety
    /// Owner registration/allocator/receipt-before-execution contracts hold, as
    /// do new's obligations for every eventual DrawParamUpdate callback.
    pub unsafe fn register(
        self,
        owner: &mut NativeTaskOwner,
    ) -> Result<(OwnedTaskId, GparamTaskControl)> {
        // SAFETY: Caller retains all owner and callback graph/phase/residency obligations.
        let id = unsafe { owner.register(TaskPhase::DRAW_PARAMETER_UPDATE, self.callback) }?;
        Ok((id, self.control))
    }
}

#[cfg(test)]
mod tests;
