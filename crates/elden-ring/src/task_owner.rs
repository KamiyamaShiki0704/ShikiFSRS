//! Explicit host-phase shutdown of a collection of engine-owned tasks.
//! Completion is payload destruction, never permission to unload the SDK DLL.
use crate::{
    TaskCallback, TaskPhase,
    native_task::{
        MemoryRelease, NativeTaskControl, NativeTaskStatus, PreparedTask, RegisteredTask,
        TaskEngine, UnregisterOutcome,
    },
};
use shikifsrs_runtime::{Error, Result};
use std::{collections::BTreeMap, marker::PhantomData, rc::Rc, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OwnedTaskId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskOwnerState {
    Open,
    Closing,
    PayloadsDestroyed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownProgress {
    pub state: TaskOwnerState,
    pub remaining_tasks: usize,
    pub waiting_registration: usize,
    pub submitted: usize,
    pub pending_destruction: usize,
    pub failures: Vec<(OwnedTaskId, Error)>,
}

/// Single host-thread owner. Cancellation is explicit; Drop stops only Rust callbacks.
/// Independent controls can outlive the owner and retain native status/engine references.
pub struct NativeTaskOwner {
    engine: Arc<dyn TaskEngine>,
    tasks: BTreeMap<OwnedTaskId, RegisteredTask>,
    next_id: u64,
    state: TaskOwnerState,
    _thread: PhantomData<Rc<()>>,
}

impl NativeTaskOwner {
    pub fn new(engine: Arc<dyn TaskEngine>) -> Self {
        Self {
            engine,
            tasks: BTreeMap::new(),
            next_id: 1,
            state: TaskOwnerState::Open,
            _thread: PhantomData,
        }
    }

    pub fn state(&self) -> TaskOwnerState {
        self.state
    }

    pub fn control(&self, id: OwnedTaskId) -> Option<NativeTaskControl> {
        self.tasks.get(&id).map(RegisteredTask::control)
    }

    pub fn statuses(&self) -> Vec<(OwnedTaskId, NativeTaskStatus)> {
        self.tasks
            .iter()
            .map(|(id, task)| (*id, task.control().status()))
            .collect()
    }

    /// # Safety
    /// Caller owns allocator/registration phases and fulfils PreparedTask's residency,
    /// serialization and receipt-before-execution obligations.
    pub unsafe fn register(
        &mut self,
        phase: TaskPhase,
        callback: TaskCallback,
    ) -> Result<OwnedTaskId> {
        if self.state != TaskOwnerState::Open {
            return Err(Error::NativeOperationRejected("task owner is closing"));
        }
        let next = self.next_id.checked_add(1).ok_or(Error::AddressOverflow)?;
        // SAFETY: Caller supplies the same phase/residency contract as PreparedTask.
        let prepared = unsafe { PreparedTask::new(Arc::clone(&self.engine), callback)? };
        // SAFETY: Unique fresh task, caller serializes registration and receipt recording.
        let task = unsafe { prepared.register(phase)? };
        let id = OwnedTaskId(self.next_id);
        self.next_id = next;
        self.tasks.insert(id, task);
        Ok(id)
    }

    /// Stop all Rust callbacks before any native unregister operation. Closes the registration gate.
    pub fn stop_all(&mut self) {
        if self.state == TaskOwnerState::Open {
            self.state = TaskOwnerState::Closing;
        }
        for task in self.tasks.values() {
            task.control().stop_callback();
        }
    }

    /// Remove only controls whose payload destruction completed. Failed frees stay visible.
    /// EngineResponsible means native heap cleanup remains outside this acknowledgement.
    pub fn reap_destroyed(&mut self) -> usize {
        let before = self.tasks.len();
        self.tasks.retain(|_, task| {
            let status = task.control().status();
            !status.destructor_completed || status.memory_release == MemoryRelease::Failed
        });
        if self.tasks.is_empty() && self.state == TaskOwnerState::Closing {
            self.state = TaskOwnerState::PayloadsDestroyed;
        }
        before - self.tasks.len()
    }

    /// Nonblocking close step. Retry at later supported phases while tasks remain.
    /// Pending insertion never triggers an early native cancellation; one failure
    /// does not prevent submission of the other tasks.
    /// # Safety
    /// Caller owns cancellation/observation phases and prevents unrelated teardown.
    /// Heap/module/DLL residency extends through all foreign frames and native cleanup.
    pub unsafe fn close_step(&mut self) -> ShutdownProgress {
        self.stop_all();
        let mut waiting_registration = 0;
        let mut submitted = 0;
        let mut pending_destruction = 0;
        let mut failures = Vec::new();
        for (id, task) in &self.tasks {
            let control = task.control();
            let status = control.status();
            if status.destructor_completed && status.memory_release == MemoryRelease::Failed {
                failures.push((
                    *id,
                    status
                        .last_error
                        .unwrap_or(Error::NativeOperationRejected("task heap free failed")),
                ));
                continue;
            }
            // SAFETY: Forwarded host-phase contract; control enforces insertion and submission state.
            match unsafe { control.unregister() } {
                Ok(UnregisterOutcome::WaitingForRegistration) => waiting_registration += 1,
                Ok(UnregisterOutcome::Submitted) => submitted += 1,
                Ok(UnregisterOutcome::InProgress | UnregisterOutcome::AlreadySubmitted) => {
                    pending_destruction += 1
                }
                Ok(UnregisterOutcome::DestructorObserved) => {}
                Err(error) => failures.push((*id, error)),
            }
        }
        self.reap_destroyed();
        ShutdownProgress {
            state: self.state,
            remaining_tasks: self.tasks.len(),
            waiting_registration,
            submitted,
            pending_destruction,
            failures,
        }
    }
}
