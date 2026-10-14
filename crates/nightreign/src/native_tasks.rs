//! Experimental NR task ownership. Acceptance, insertion and destruction are separate.
use shikifsrs_runtime::{
    Error, Result,
    callback::{Callback, CallbackControl, CallbackStatus},
};
use std::{
    alloc::Layout,
    ffi::c_void,
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    rc::Rc,
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskPhase(u16);
impl TaskPhase {
    pub const FRAME_BEGIN: Self = Self(0);
    pub const SOUND_STEP: Self = Self(94);
    pub const CHR_POST_PHYSICS: Self = Self(98);
    pub const FRAME_END: Self = Self(154);
    pub fn new(index: u16) -> Result<Self> {
        if index >= 156 {
            return Err(Error::InvalidProfile("NR task phase out of range"));
        }
        Ok(Self(index))
    }
    pub fn index(self) -> u16 {
        self.0
    }
}

/// Copied FD4Time fields. No meaning is assigned to the scalar or worker word.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TaskInvocation {
    pub group_id: u32,
    pub scalar_08: f32,
    pub word_14: u32,
}
pub type TaskCallback = Callback<TaskInvocation>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    Sdk,
    EnginePending,
    EngineInserted,
    Destroying,
    Destroyed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeapRelease {
    NotRequested,
    EngineResponsible,
    Released,
    Failed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    WaitingForInsertion,
    Submitted,
    AlreadySubmitted,
    Destroyed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistrationReceipt {
    group_id: u32,
    pending: bool,
}
impl RegistrationReceipt {
    pub fn new(group_id: u32, pending: bool) -> Result<Self> {
        if group_id == 0 {
            return Err(Error::InvalidProfile("NR task group id is unassigned"));
        }
        Ok(Self { group_id, pending })
    }
    pub fn group_id(self) -> u32 {
        self.group_id
    }
    pub fn pending(self) -> bool {
        self.pending
    }
}

/// Allocation, native scheduling and destruction seam for current-process and synthetic engines.
/// # Safety
/// Use one live engine heap with the requested writable/aligned allocation layout. Methods
/// must not unwind. Registration rejection retains no pointer; acceptance transfers ownership
/// and prevents execution/destruction until the receipt is recorded. Each object is submitted
/// once to one group. Accepted queued commands retain its allocation. A positive insertion
/// observation identifies this exact object. Destruction occurs exactly once after ALL execute
/// frames return; flags zero requires the engine to free separately. Heap, code, type metadata
/// and SDK DLL remain resident through destruction, native free and every foreign return frame.
pub unsafe trait TaskEngine: Send + Sync {
    fn type_info_address(&self) -> usize;
    /// # Safety
    /// Caller owns a valid allocator phase and keeps this engine/heap resident.
    unsafe fn allocate(&self, layout: Layout) -> Result<NonNull<u8>>;
    /// # Safety
    /// Unique live allocation from this exact heap/layout, after payload destruction.
    unsafe fn deallocate(&self, allocation: NonNull<u8>, layout: Layout) -> Result<()>;
    /// # Safety
    /// Unique prepared SDK task; caller serializes scheduling through receipt recording.
    unsafe fn register(
        &self,
        phase: TaskPhase,
        task: NonNull<NativeTaskObject>,
    ) -> Result<RegistrationReceipt>;
    /// # Safety
    /// Caller owns a stable observation phase and prevents teardown/address reuse.
    unsafe fn inserted(&self, group_id: u32, address: usize) -> Result<bool>;
    /// # Safety
    /// Confirmed insertion into the full group; caller owns cancellation phase/residency.
    /// Success means accepted cancellation, never destruction or heap-free acknowledgement.
    unsafe fn unregister(&self, group_id: u32, address: usize) -> Result<()>;
    /// Checked copying only; unreadable native context must return an error.
    fn invocation(&self, address: usize) -> Result<TaskInvocation>;
}

/// NR dispatch reads +0x10 and native deletion invokes +8 with scalar flags.
/// Slot zero returns the native FD4TaskBase type metadata, not an allocator pointer.
#[repr(C)]
pub struct NativeTaskVTable {
    pub type_info: unsafe extern "system" fn(*mut NativeTaskObject) -> *mut c_void,
    pub destructor: unsafe extern "system" fn(*mut NativeTaskObject, u32) -> *mut NativeTaskObject,
    pub execute: unsafe extern "system" fn(*mut NativeTaskObject, *const c_void),
}
#[repr(C)]
pub struct NativeTaskObject {
    table: *const NativeTaskVTable,
    payload: *mut Payload,
}
impl NativeTaskObject {
    /// # Safety
    /// This is a live uniquely identified SDK task; no destruction overlaps the call.
    pub unsafe fn vtable(task: NonNull<Self>) -> &'static NativeTaskVTable {
        // SAFETY: Caller guarantees our initialized live task; table has static storage.
        unsafe { &*(*task.as_ptr()).table }
    }
}
static TABLE: NativeTaskVTable = NativeTaskVTable {
    type_info,
    destructor,
    execute,
};

#[derive(Debug, Clone)]
pub struct TaskStatus {
    pub ownership: Ownership,
    pub group_id: Option<u32>,
    pub cancellation_submitted: bool,
    pub heap_release: HeapRelease,
    pub rejected_invocations: u64,
    pub last_error: Option<Error>,
    pub callback: CallbackStatus,
}
struct State {
    ownership: Ownership,
    group_id: Option<u32>,
    cancellation_submitted: bool,
    heap_release: HeapRelease,
    rejected: u64,
    error: Option<Error>,
}
struct Shared {
    state: Mutex<State>,
    callback: CallbackControl<TaskInvocation>,
}
impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
}
struct Payload {
    engine: Arc<dyn TaskEngine>,
    shared: Arc<Shared>,
    callback: TaskCallback,
}

pub struct PreparedTask {
    object: Option<NonNull<NativeTaskObject>>,
    control: TaskControl,
    _thread: PhantomData<Rc<()>>,
}
/// Dropping this handle stops callbacks, without making foreign engine calls or freeing storage.
pub struct RegisteredTask {
    control: TaskControl,
    _thread: PhantomData<Rc<()>>,
}
#[derive(Clone)]
pub struct TaskControl {
    address: usize,
    engine: Arc<dyn TaskEngine>,
    shared: Arc<Shared>,
}

impl PreparedTask {
    /// # Safety
    /// Caller owns allocator/registration phases, heap/code/type metadata/DLL residency,
    /// and scheduling serialization through receipt recording or prepared-task disposal.
    pub unsafe fn new(engine: Arc<dyn TaskEngine>, callback: TaskCallback) -> Result<Self> {
        if engine.type_info_address() == 0 {
            return Err(Error::InvalidProfile("NR base task metadata is missing"));
        }
        // SAFETY: Caller supplies allocator phase; unsafe backend supplies heap/layout contract.
        let allocation = unsafe { engine.allocate(Layout::new::<NativeTaskObject>())? };
        if !(allocation.as_ptr() as usize).is_multiple_of(std::mem::align_of::<NativeTaskObject>())
        {
            return Err(Error::InvalidProfile("NR task allocation is unaligned"));
        }
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                ownership: Ownership::Sdk,
                group_id: None,
                cancellation_submitted: false,
                heap_release: HeapRelease::NotRequested,
                rejected: 0,
                error: None,
            }),
            callback: callback.control(),
        });
        let payload = Box::into_raw(Box::new(Payload {
            engine: Arc::clone(&engine),
            shared: Arc::clone(&shared),
            callback,
        }));
        let object = allocation.cast::<NativeTaskObject>();
        // SAFETY: Backend guarantees writable aligned allocation; these are SDK-owned fields.
        unsafe {
            object.as_ptr().write(NativeTaskObject {
                table: &TABLE,
                payload,
            });
        }
        let control = TaskControl {
            address: object.as_ptr() as usize,
            engine,
            shared,
        };
        Ok(Self {
            object: Some(object),
            control,
            _thread: PhantomData,
        })
    }

    /// # Safety
    /// Caller fulfils the backend's serialized submission and residency guarantees.
    pub unsafe fn register(mut self, phase: TaskPhase) -> Result<RegisteredTask> {
        let object = self.object.expect("prepared object");
        // SAFETY: Unique unsubmitted allocation; external scheduling contract is forwarded.
        let receipt = unsafe { self.control.engine.register(phase, object)? };
        {
            let mut state = self.control.shared.lock();
            state.group_id = Some(receipt.group_id);
            state.ownership = if receipt.pending {
                Ownership::EnginePending
            } else {
                Ownership::EngineInserted
            };
        }
        self.object = None;
        Ok(RegisteredTask {
            control: self.control.clone(),
            _thread: PhantomData,
        })
    }
}
impl Drop for PreparedTask {
    fn drop(&mut self) {
        if let Some(object) = self.object.take() {
            // SAFETY: Unsubmitted unique object; new/register callers guarantee disposal phase.
            unsafe {
                destructor(object.as_ptr(), 1);
            }
        }
    }
}
impl RegisteredTask {
    pub fn control(&self) -> TaskControl {
        self.control.clone()
    }
}
impl Drop for RegisteredTask {
    fn drop(&mut self) {
        self.control.stop();
    }
}

impl TaskControl {
    pub fn address(&self) -> usize {
        self.address
    }
    pub fn stop(&self) {
        self.shared.callback.cancel();
    }
    pub fn status(&self) -> TaskStatus {
        let state = self.shared.lock();
        TaskStatus {
            ownership: state.ownership,
            group_id: state.group_id,
            cancellation_submitted: state.cancellation_submitted,
            heap_release: state.heap_release,
            rejected_invocations: state.rejected,
            last_error: state.error.clone(),
            callback: self.shared.callback.status(),
        }
    }
    /// # Safety
    /// Stable observation phase and no concurrent teardown/address reuse.
    pub unsafe fn confirm_insertion(&self) -> Result<bool> {
        let group_id = {
            let state = self.shared.lock();
            if state.ownership == Ownership::EngineInserted {
                return Ok(true);
            }
            if state.ownership != Ownership::EnginePending {
                return Ok(false);
            }
            state.group_id.expect("accepted receipt")
        };
        // SAFETY: Caller supplies stable observation; backend checks this precise allocation.
        if !unsafe { self.engine.inserted(group_id, self.address)? } {
            return Ok(false);
        }
        let mut state = self.shared.lock();
        if state.ownership == Ownership::EnginePending {
            state.ownership = Ownership::EngineInserted;
        }
        Ok(state.ownership == Ownership::EngineInserted)
    }
    /// # Safety
    /// Caller serializes this control's cancellation/observation, owns supported engine
    /// phases and prevents unrelated teardown. DLL residency includes every foreign frame.
    pub unsafe fn unregister(&self) -> Result<CancelOutcome> {
        self.stop();
        // SAFETY: Same caller-provided observation phase.
        unsafe {
            self.confirm_insertion()?;
        }
        let group_id = {
            let state = self.shared.lock();
            if state.ownership == Ownership::Destroyed {
                return Ok(CancelOutcome::Destroyed);
            }
            if state.cancellation_submitted {
                return Ok(CancelOutcome::AlreadySubmitted);
            }
            if state.ownership != Ownership::EngineInserted {
                return Ok(CancelOutcome::WaitingForInsertion);
            }
            state.group_id.expect("inserted receipt")
        };
        // SAFETY: Exact inserted object; caller serializes cancellation and engine lifetime.
        unsafe {
            self.engine.unregister(group_id, self.address)?;
        }
        self.shared.lock().cancellation_submitted = true;
        Ok(CancelOutcome::Submitted)
    }
}

fn contain(action: impl FnOnce()) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(action)) {
        std::mem::forget(payload);
    }
}
unsafe extern "system" fn type_info(object: *mut NativeTaskObject) -> *mut c_void {
    // SAFETY: Engine contract guarantees a live initialized task through this virtual frame.
    unsafe { (*(*object).payload).engine.type_info_address() as *mut c_void }
}
unsafe extern "system" fn execute(object: *mut NativeTaskObject, context: *const c_void) {
    contain(|| {
        // SAFETY: Backend guarantees object/payload lifetime and no overlapping destruction.
        let payload = unsafe { &*(*object).payload };
        let invocation = match payload.engine.invocation(context as usize) {
            Ok(invocation) => invocation,
            Err(error) => {
                let mut state = payload.shared.lock();
                state.rejected = state.rejected.saturating_add(1);
                state.error = Some(error);
                return;
            }
        };
        {
            let mut state = payload.shared.lock();
            if state.group_id != Some(invocation.group_id)
                || !matches!(
                    state.ownership,
                    Ownership::EnginePending | Ownership::EngineInserted
                )
            {
                state.rejected = state.rejected.saturating_add(1);
                return;
            }
            state.ownership = Ownership::EngineInserted;
        }
        payload.callback.invoke(invocation);
    });
}
unsafe extern "system" fn destructor(
    object: *mut NativeTaskObject,
    flags: u32,
) -> *mut NativeTaskObject {
    contain(|| {
        // SAFETY: Engine guarantees unique destructor after all execution frames returned.
        let payload = unsafe { Box::from_raw((*object).payload) };
        let engine = Arc::clone(&payload.engine);
        let shared = Arc::clone(&payload.shared);
        shared.lock().ownership = Ownership::Destroying;
        // Callback module contains capture destructor unwinds, outside state locks.
        drop(payload);
        let release = if flags & 1 != 0 {
            // SAFETY: Unique SDK allocation after payload destruction, on original heap/layout.
            match unsafe {
                engine.deallocate(
                    NonNull::new(object.cast()).expect("live task"),
                    Layout::new::<NativeTaskObject>(),
                )
            } {
                Ok(()) => HeapRelease::Released,
                Err(error) => {
                    shared.lock().error = Some(error);
                    HeapRelease::Failed
                }
            }
        } else {
            HeapRelease::EngineResponsible
        };
        let mut state = shared.lock();
        state.heap_release = release;
        state.ownership = Ownership::Destroyed;
    });
    object
}

/// Host-thread collection. Close stops callbacks, then retries pending insertions/cancellations.
/// Payload completion is not permission to unload the SDK DLL.
pub struct TaskOwner {
    engine: Arc<dyn TaskEngine>,
    tasks: Vec<RegisteredTask>,
    closing: bool,
    _thread: PhantomData<Rc<()>>,
}
impl TaskOwner {
    pub fn new(engine: Arc<dyn TaskEngine>) -> Self {
        Self {
            engine,
            tasks: Vec::new(),
            closing: false,
            _thread: PhantomData,
        }
    }
    /// # Safety
    /// PreparedTask's scheduling/allocator/residency guarantees apply to the whole owner.
    pub unsafe fn register(
        &mut self,
        phase: TaskPhase,
        callback: TaskCallback,
    ) -> Result<TaskControl> {
        if self.closing {
            return Err(Error::NativeOperationRejected("NR task owner is closing"));
        }
        // SAFETY: Forwarded caller guarantees cover allocation and serialized registration.
        let prepared = unsafe { PreparedTask::new(Arc::clone(&self.engine), callback)? };
        // SAFETY: Unique fresh task under the same registration/receipt contract.
        let registered = unsafe { prepared.register(phase)? };
        let control = registered.control();
        self.tasks.push(registered);
        Ok(control)
    }
    pub fn stop_all(&mut self) {
        self.closing = true;
        for task in &self.tasks {
            task.control.stop();
        }
    }
    /// # Safety
    /// Caller owns observation/cancellation phases and all transitive residency guarantees.
    pub unsafe fn close_step(&mut self) -> Vec<Result<CancelOutcome>> {
        self.stop_all();
        let result = self
            .tasks
            .iter()
            .map(|task| {
                // SAFETY: Forwarded close phase; no new registration after closing.
                unsafe { task.control.unregister() }
            })
            .collect();
        self.tasks.retain(|task| {
            let status = task.control.status();
            status.ownership != Ownership::Destroyed || status.heap_release == HeapRelease::Failed
        });
        result
    }
    pub fn remaining(&self) -> usize {
        self.tasks.len()
    }
}
#[cfg(test)]
mod tests;
