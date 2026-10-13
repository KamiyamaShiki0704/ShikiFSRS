//! Engine-allocated task objects and explicit ownership transfer.
//!
//! Unsafe engine operations require the caller's scheduling/lifetime guarantees.
//! Queued unregistration is never treated as destruction acknowledgement.
use crate::{TaskCallback, TaskCallbackControl, TaskInvocation, TaskPhase};
use shikifsrs_runtime::{
    Error, Result,
    callback::{CallbackStatus, WaitOutcome},
};
use std::{
    alloc::Layout,
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::{self, NonNull},
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
};

/// Backend contract shared by real current-process bindings and synthetic engines.
///
/// # Safety
/// Allocation must be aligned, writable and associated with `allocator_address`;
/// deallocation must use that same heap and layout. Ownership methods must not unwind.
/// Successful registration transfers ownership, with an explicit inserted/pending receipt;
/// rejection/error must retain no task pointer. Pending commands must retain storage;
/// insertion may fail without a destructor acknowledgement (never reclaim speculatively).
/// No execution/destruction may start before PreparedTask::register records its receipt.
/// An accepted unregister may be queued and is submitted only after insertion confirmation.
/// The engine must finish ALL execute calls before invoking the task destructor once.
/// If destructor flags are zero, the engine must free the allocation separately;
/// if bit zero is set, the destructor frees it. No call may use the object afterward.
/// Backend/module/heap lifetime and DLL residency must cover every retained task.
pub unsafe trait TaskEngine: Send + Sync {
    fn allocator_address(&self) -> usize;
    /// # Safety
    /// Caller must own an engine phase where this allocator can be used.
    unsafe fn allocate(&self, layout: Layout) -> Result<NonNull<u8>>;
    /// # Safety
    /// Allocation is live, uniquely owned, from this heap, with this exact layout.
    unsafe fn deallocate(&self, allocation: NonNull<u8>, layout: Layout) -> Result<()>;
    /// # Safety
    /// Object is prepared, uniquely submitted, and engine scheduling is serialized.
    unsafe fn register(
        &self,
        phase: TaskPhase,
        task: NonNull<NativeTaskObject>,
    ) -> Result<RegistrationReceipt>;
    /// # Safety
    /// Caller owns a serialized observation phase and prevents unrelated teardown.
    /// True must confirm this exact live task's insertion into the full group id.
    /// A backend without an observation method may rely on the first valid execute call.
    unsafe fn registration_inserted(&self, _group_id: u32, _task_address: usize) -> Result<bool> {
        Ok(false)
    }
    /// # Safety
    /// Task is registered to this group; caller owns a supported engine scheduling phase.
    /// On success the engine takes responsibility for destructor + heap release.
    unsafe fn unregister(&self, group_id: u32, task_address: usize) -> Result<()>;
    /// Checked copying only: a bad address must return an error, never dereference blindly.
    fn read_invocation(&self, address: usize) -> Result<TaskInvocation>;
}

/// C++ task prefix, independently authored; all tail fields belong to this SDK.
#[repr(C)]
pub struct NativeTaskObject {
    vtable: *const NativeTaskVTable,
    allocator: usize,
    payload: *mut Payload,
}

/// Three slots proven by native dispatch and deletion call sites.
#[repr(C)]
pub struct NativeTaskVTable {
    pub allocator: unsafe extern "system" fn(*mut NativeTaskObject) -> *mut c_void,
    pub destructor: unsafe extern "system" fn(*mut NativeTaskObject, u32) -> *mut NativeTaskObject,
    pub execute: unsafe extern "system" fn(*mut NativeTaskObject, *const c_void),
}

impl NativeTaskObject {
    /// # Safety
    /// `object` is a live SDK task; caller guarantees no concurrent destruction.
    pub unsafe fn vtable(object: NonNull<Self>) -> &'static NativeTaskVTable {
        // SAFETY: Caller guarantees the live object, initialized with our static table.
        unsafe { &*(*object.as_ptr()).vtable }
    }
}

static VTABLE: NativeTaskVTable = NativeTaskVTable {
    allocator: allocator_slot,
    destructor: destructor_slot,
    execute: execute_slot,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskOwnership {
    Sdk,
    Engine,
    Destroying,
    Destroyed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRelease {
    NotRequested,
    /// Destructor flags zero: subsequent native free is the engine's responsibility.
    EngineResponsible,
    ReleasedBySdk,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationState {
    Unsubmitted,
    Pending,
    Inserted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistrationReceipt {
    pub group_id: u32,
    pub state: RegistrationState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnregisterOutcome {
    /// Callback stopped; no cancellation command submitted before registration confirmation.
    WaitingForRegistration,
    Submitted,
    InProgress,
    AlreadySubmitted,
    DestructorObserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestructorWait {
    Completed,
    TimedOut,
    WouldBlockCurrentCallback,
}

#[derive(Debug, Clone)]
pub struct NativeTaskStatus {
    pub ownership: TaskOwnership,
    pub group_id: Option<u32>,
    pub registration: RegistrationState,
    pub unregister_submitted: bool,
    pub destructor_completed: bool,
    pub memory_release: MemoryRelease,
    pub rejected_invocations: u64,
    pub abi_panicked: bool,
    pub last_error: Option<Error>,
    pub callback: CallbackStatus,
}

struct State {
    ownership: TaskOwnership,
    group_id: Option<u32>,
    registration: RegistrationState,
    unregister_state: u8, // 0 absent/rejected, 1 submitting, 2 accepted
    destructor_completed: bool,
    memory_release: MemoryRelease,
    rejected_invocations: u64,
    abi_panicked: bool,
    last_error: Option<Error>,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
}

struct Payload {
    callback: TaskCallback,
    engine: Arc<dyn TaskEngine>,
    shared: Arc<Shared>,
}

/// SDK-owned allocation before registration. Drop destroys captures and frees this heap.
/// It is intentionally not Send; prepare/register/drop must respect allocator affinity.
pub struct PreparedTask {
    object: Option<NonNull<NativeTaskObject>>,
    control: NativeTaskControl,
}

pub struct RegisteredTask {
    control: NativeTaskControl,
}

/// Independent control state survives native object destruction without borrowing it.
#[derive(Clone)]
pub struct NativeTaskControl {
    address: usize,
    engine: Arc<dyn TaskEngine>,
    shared: Arc<Shared>,
    callback: TaskCallbackControl,
}

impl PreparedTask {
    /// # Safety
    /// Engine allocator/heap/module and this DLL remain valid. Caller owns an
    /// allocator phase through registration or prepared-task destruction.
    pub unsafe fn new(engine: Arc<dyn TaskEngine>, callback: TaskCallback) -> Result<Self> {
        if engine.allocator_address() == 0 {
            return Err(Error::InvalidProfile("task allocator address is null"));
        }
        let layout = Layout::new::<NativeTaskObject>();
        // SAFETY: Forwarded allocator phase obligation; layout matches our concrete object.
        let allocation = unsafe { engine.allocate(layout)? };
        if !(allocation.as_ptr() as usize).is_multiple_of(layout.align()) {
            // Broken unsafe backend contract: do not write to or guess how to free this pointer.
            return Err(Error::InvalidProfile(
                "engine returned an unaligned allocation",
            ));
        }
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                ownership: TaskOwnership::Sdk,
                group_id: None,
                registration: RegistrationState::Unsubmitted,
                unregister_state: 0,
                destructor_completed: false,
                memory_release: MemoryRelease::NotRequested,
                rejected_invocations: 0,
                abi_panicked: false,
                last_error: None,
            }),
            changed: Condvar::new(),
        });
        let control = NativeTaskControl {
            address: allocation.as_ptr() as usize,
            engine: Arc::clone(&engine),
            shared: Arc::clone(&shared),
            callback: callback.control(),
        };
        let payload = Box::into_raw(Box::new(Payload {
            callback,
            engine,
            shared,
        }));
        let object = allocation.cast::<NativeTaskObject>();
        // SAFETY: Contract gives fresh aligned storage of exactly this layout; payload is owned.
        unsafe {
            object.as_ptr().write(NativeTaskObject {
                vtable: &VTABLE,
                allocator: control.engine.allocator_address(),
                payload,
            });
        }
        Ok(Self {
            object: Some(object),
            control,
        })
    }

    pub fn control(&self) -> NativeTaskControl {
        self.control.clone()
    }

    /// # Safety
    /// Caller owns a serialized, supported registration phase. The backend must
    /// keep execution/destruction from starting until this method records its receipt.
    pub unsafe fn register(mut self, phase: TaskPhase) -> Result<RegisteredTask> {
        let object = self.object.expect("prepared task owns its object");
        // SAFETY: Fresh unique task and caller's registration-phase guarantee.
        let receipt = match unsafe { self.control.engine.register(phase, object) } {
            Ok(receipt) => receipt,
            Err(error) => {
                self.control.shared.lock().last_error = Some(error.clone());
                return Err(error);
            }
        };
        // A zero group after success breaks the unsafe backend contract: ownership
        // is already transferred, so never reclaim this task speculatively.
        self.object = None;
        let group_id = receipt.group_id;
        {
            let mut state = self.control.shared.lock();
            state.ownership = TaskOwnership::Engine;
            state.group_id = (group_id != 0).then_some(group_id);
            state.registration = receipt.state;
            if group_id == 0 || receipt.state == RegistrationState::Unsubmitted {
                state.last_error = Some(Error::InvalidProfile("invalid registration receipt"));
            }
        }
        if group_id == 0 || receipt.state == RegistrationState::Unsubmitted {
            self.control.callback.cancel();
            return Err(Error::InvalidProfile(
                "backend returned an invalid registration receipt",
            ));
        }
        Ok(RegisteredTask {
            control: self.control.clone(),
        })
    }
}

impl Drop for PreparedTask {
    fn drop(&mut self) {
        if let Some(object) = self.object.take() {
            // SAFETY: Never transferred; unique allocation and original allocator remain valid.
            unsafe {
                destructor_slot(object.as_ptr(), 1);
            }
        }
    }
}

impl RegisteredTask {
    pub fn control(&self) -> NativeTaskControl {
        self.control.clone()
    }
}

impl Drop for RegisteredTask {
    fn drop(&mut self) {
        // Native calls require an explicit engine phase. Dropping the handle only
        // stops Rust code; native storage stays engine-owned until its destructor.
        self.control.callback.cancel();
    }
}

impl NativeTaskControl {
    pub fn stop_callback(&self) {
        self.callback.cancel();
    }

    pub fn status(&self) -> NativeTaskStatus {
        let state = self.shared.lock();
        NativeTaskStatus {
            ownership: state.ownership,
            group_id: state.group_id,
            registration: state.registration,
            unregister_submitted: state.unregister_state == 2,
            destructor_completed: state.destructor_completed,
            memory_release: state.memory_release,
            rejected_invocations: state.rejected_invocations,
            abi_panicked: state.abi_panicked,
            last_error: state.last_error.clone(),
            callback: self.callback.status(),
        }
    }

    /// # Safety
    /// Caller owns a supported native cancellation phase; engine/task system,
    /// heap and SDK DLL remain resident through queued destruction AND the
    /// native caller's return. Destructor observation does not permit DLL unload.
    /// No unrelated teardown may destroy this object concurrently with submission.
    pub unsafe fn unregister(&self) -> Result<UnregisterOutcome> {
        self.callback.cancel();
        let (group_id, pending) = {
            let mut state = self.shared.lock();
            if matches!(
                state.ownership,
                TaskOwnership::Destroying | TaskOwnership::Destroyed
            ) {
                return Ok(UnregisterOutcome::DestructorObserved);
            }
            if state.ownership != TaskOwnership::Engine {
                return Err(Error::NativeOperationRejected(
                    "task has not been registered",
                ));
            }
            match state.unregister_state {
                1 => return Ok(UnregisterOutcome::InProgress),
                2 => return Ok(UnregisterOutcome::AlreadySubmitted),
                _ => {}
            }
            let group_id = state
                .group_id
                .ok_or(Error::InvalidProfile("task has no native group id"))?;
            state.unregister_state = 1;
            (group_id, state.registration == RegistrationState::Pending)
        };
        if pending {
            // SAFETY: Caller serializes this observation with native teardown; address is retained.
            let inserted = unsafe { self.engine.registration_inserted(group_id, self.address) };
            let mut state = self.shared.lock();
            match inserted {
                Ok(true) => state.registration = RegistrationState::Inserted,
                Ok(false) if state.registration != RegistrationState::Inserted => {
                    state.unregister_state = 0;
                    return Ok(UnregisterOutcome::WaitingForRegistration);
                }
                Ok(false) => {}
                Err(error) => {
                    state.unregister_state = 0;
                    state.last_error = Some(error.clone());
                    return Err(error);
                }
            }
        }
        // SAFETY: Registered unique task and the caller's serialized cancellation guarantee.
        let result = unsafe { self.engine.unregister(group_id, self.address) };
        let mut state = self.shared.lock();
        state.unregister_state = if result.is_ok() { 2 } else { 0 };
        if let Err(error) = result {
            state.last_error = Some(error.clone());
            self.shared.changed.notify_all();
            return Err(error);
        }
        self.shared.changed.notify_all();
        Ok(UnregisterOutcome::Submitted)
    }

    /// Acknowledges SDK payload destruction, NOT subsequent native heap free or
    /// return of every foreign call frame. Never use this as DLL unload permission.
    pub fn wait_for_destructor(&self, timeout: Duration) -> DestructorWait {
        let started = Instant::now();
        loop {
            let state = self.shared.lock();
            if state.destructor_completed {
                return DestructorWait::Completed;
            }
            drop(state);
            if self.callback.wait_stopped(Duration::ZERO) == WaitOutcome::WouldBlockCurrentCallback
            {
                return DestructorWait::WouldBlockCurrentCallback;
            }
            let state = self.shared.lock();
            if state.destructor_completed {
                return DestructorWait::Completed;
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return DestructorWait::TimedOut;
            }
            let (guard, _) = self
                .shared
                .changed
                .wait_timeout(state, remaining.min(Duration::from_secs(60)))
                .unwrap_or_else(|error| error.into_inner());
            drop(guard);
        }
    }
}

fn contained<T>(fallback: T, operation: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(value) => value,
        Err(payload) => {
            std::mem::forget(payload);
            fallback
        }
    }
}

unsafe extern "system" fn allocator_slot(object: *mut NativeTaskObject) -> *mut c_void {
    if object.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: Native contract guarantees this SDK object is live through the getter.
    unsafe { (*object).allocator as *mut c_void }
}

unsafe extern "system" fn execute_slot(object: *mut NativeTaskObject, argument: *const c_void) {
    if object.is_null() {
        return;
    }
    // SAFETY: Native engine guarantees live payload and no overlapping destructor.
    let payload = unsafe { (*object).payload.as_ref() };
    let Some(payload) = payload else {
        return;
    };
    let panicked = contained(true, || {
        let invocation = payload
            .engine
            .read_invocation(argument as usize)
            .and_then(|invocation| {
                if payload.shared.lock().group_id != Some(invocation.group_id) {
                    return Err(Error::InvalidProfile(
                        "native callback group differs from the registered task",
                    ));
                }
                Ok(invocation)
            });
        match invocation {
            Ok(invocation) => {
                payload.shared.lock().registration = RegistrationState::Inserted;
                payload.callback.invoke(invocation);
            }
            Err(error) => {
                let mut state = payload.shared.lock();
                state.rejected_invocations = state.rejected_invocations.saturating_add(1);
                state.last_error = Some(error);
                drop(state);
                payload.callback.control().cancel();
            }
        }
        false
    });
    if panicked {
        payload.shared.lock().abi_panicked = true;
        payload.callback.control().cancel();
    }
}

unsafe extern "system" fn destructor_slot(
    object: *mut NativeTaskObject,
    flags: u32,
) -> *mut NativeTaskObject {
    if object.is_null() {
        return object;
    }
    contained(object, || {
        // SAFETY: Engine finishes all executions first; SDK task is exclusively destructed once.
        let raw = unsafe { ptr::replace(ptr::addr_of_mut!((*object).payload), ptr::null_mut()) };
        if raw.is_null() {
            return object;
        }
        // SAFETY: Payload was created by Box::into_raw and is reclaimed exactly once here.
        let payload = unsafe { Box::from_raw(raw) };
        let engine = Arc::clone(&payload.engine);
        let shared = Arc::clone(&payload.shared);
        shared.lock().ownership = TaskOwnership::Destroying;
        // All capture destruction is outside the lifecycle mutex and inside unwind containment.
        let panicked = contained(true, || {
            drop(payload);
            false
        });
        {
            let mut state = shared.lock();
            state.abi_panicked |= panicked;
            state.memory_release = if flags & 1 == 0 {
                MemoryRelease::EngineResponsible
            } else {
                MemoryRelease::NotRequested
            };
            shared.changed.notify_all();
        }
        if flags & 1 != 0 {
            // SAFETY: Destruction is finished and this exact engine allocation is uniquely owned.
            let result = unsafe {
                engine.deallocate(
                    NonNull::new_unchecked(object.cast()),
                    Layout::new::<NativeTaskObject>(),
                )
            };
            let mut state = shared.lock();
            match result {
                Ok(()) => state.memory_release = MemoryRelease::ReleasedBySdk,
                Err(error) => {
                    state.memory_release = MemoryRelease::Failed;
                    state.last_error = Some(error);
                }
            }
            shared.changed.notify_all();
        }
        {
            let mut state = shared.lock();
            state.ownership = TaskOwnership::Destroyed;
            state.destructor_completed = true;
            shared.changed.notify_all();
        }
        object
    })
}
