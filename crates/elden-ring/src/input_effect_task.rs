//! FrameBegin input/effect pipeline with explicit cleanup before native task cancellation.
use crate::{
    TaskCallback, TaskInvocation, TaskPhase,
    character_sets::CharacterIdentity,
    effects::{EffectEngine, EffectFrameReport, InputEffect, InputEffectController},
    task_owner::{NativeTaskOwner, OwnedTaskId},
};
use shikifsrs_runtime::{
    Error, Result,
    input::{InputFrame, InputPoller, InputSource},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

/// Source determines a fresh copied target per invocation; it must not return a stale identity.
/// Mutation-phase and full engine lifetime safety remain the unsafe creator's obligations.
pub trait InputEffectTaskBackend: InputSource + EffectEngine + Send + 'static {
    fn target(&mut self) -> Result<Option<CharacterIdentity>>;
}
#[derive(Debug, Clone)]
pub struct InputEffectTaskReport {
    pub invocation: TaskInvocation,
    pub target: Option<CharacterIdentity>,
    pub previous_target: Option<CharacterIdentity>,
    pub target_error: Option<Error>,
    pub closing: bool,
    pub effects: EffectFrameReport,
    /// Latched if any prior target was abandoned. Later empty frames cannot erase this outcome.
    pub cleanup_uncertain: bool,
}
impl InputEffectTaskReport {
    /// Only this pipeline's effect obligations are complete. Not native task/DLL retirement.
    pub fn cleanup_complete(&self) -> bool {
        self.closing
            && !self.cleanup_uncertain
            && self.target_error.is_none()
            && self.effects.cleanup_pending.is_empty()
    }
}
struct Shared {
    closing: AtomicBool,
    report: Mutex<Option<InputEffectTaskReport>>,
}
#[derive(Clone)]
pub struct InputEffectTaskControl {
    shared: Arc<Shared>,
}
impl InputEffectTaskControl {
    /// One-way request; future FrameBegin calls remove owned active effects and retry failures.
    /// Does not stop the callback or invoke any native function from the calling thread.
    pub fn request_cleanup(&self) {
        self.shared.closing.store(true, Ordering::Release);
    }
    pub fn last_report(&self) -> Option<InputEffectTaskReport> {
        self.shared
            .report
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}
pub struct InputEffectTask {
    callback: TaskCallback,
    control: InputEffectTaskControl,
}
impl InputEffectTask {
    /// # Safety
    /// Every eventual FrameBegin invocation has an independently established exclusive effect
    /// mutation phase and live native target/module/PARAM/heap/fullgraph. Backend identities
    /// include a correct nonzero caller epoch, excluding ABA/teardown/aliasing. This factory
    /// does not invoke the backend. Host/SDK/hook residency covers all native frames.
    /// Active effects require phase cleanup before stopping/dropping this callback; abandoned
    /// targets require external reconciliation. No Drop/native cancellation cleans effects.
    pub unsafe fn new<B: InputEffectTaskBackend>(
        mut backend: B,
        process_id: u32,
        bindings: impl IntoIterator<Item = InputEffect>,
    ) -> Result<Self> {
        let bindings: Vec<_> = bindings.into_iter().take(257).collect();
        let mut controller = InputEffectController::new(bindings.iter().copied())?;
        let poller = InputPoller::new(process_id, bindings.iter().map(|x| x.input), 0, 30)?;
        let shared = Arc::new(Shared {
            closing: AtomicBool::new(false),
            report: Mutex::new(None),
        });
        let callback_shared = Arc::clone(&shared);
        let mut previous_target = None;
        let mut cleanup_uncertain = false;
        let callback = TaskCallback::new(move |invocation| {
            let closing = callback_shared.closing.load(Ordering::Acquire);
            let (target, target_error) = match backend.target() {
                Ok(target) => (target, None),
                Err(error) => (None, Some(error)),
            };
            let frame = if closing {
                Ok(InputFrame {
                    focused: false,
                    down: Default::default(),
                    gamepad: None,
                    gamepad_error: None,
                })
            } else {
                poller.poll(&backend)
            };
            // SAFETY: Factory caller supplies the per-invocation FrameBegin mutation/lifetime
            // contract; controller never calls an old target after a detected transition.
            let effects = unsafe { controller.update(&backend, target, frame) };
            cleanup_uncertain |= !effects.abandoned.is_empty();
            let report = InputEffectTaskReport {
                invocation,
                target,
                previous_target,
                target_error,
                closing,
                effects,
                cleanup_uncertain,
            };
            previous_target = target;
            *callback_shared
                .report
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(report);
        });
        Ok(Self {
            callback,
            control: InputEffectTaskControl { shared },
        })
    }
    pub fn control(&self) -> InputEffectTaskControl {
        self.control.clone()
    }
    pub(crate) fn into_parts(self) -> (TaskCallback, InputEffectTaskControl) {
        (self.callback, self.control)
    }
    /// Register exactly FrameBegin, retaining the pipeline control and native task owner.
    /// # Safety
    /// Factory obligations and NativeTaskOwner registration phase/residency/receipt-before-
    /// execution guarantees hold. Caller cleans effects in a live phase before owner close.
    pub unsafe fn register(
        self,
        owner: &mut NativeTaskOwner,
    ) -> Result<(OwnedTaskId, InputEffectTaskControl)> {
        // SAFETY: Caller supplies owner registration phase and all future callback obligations.
        let id = unsafe { owner.register(TaskPhase::FRAME_BEGIN, self.callback) }?;
        Ok((id, self.control))
    }
}
