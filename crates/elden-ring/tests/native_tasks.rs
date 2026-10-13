//! Original synthetic engine: no game process, game allocation or game calls.
use shikifsrs_elden_ring::{
    TaskCallback, TaskInvocation, TaskPhase,
    native_task::{
        DestructorWait, MemoryRelease, NativeTaskControl, NativeTaskObject, NativeTaskVTable,
        PreparedTask, RegistrationReceipt, RegistrationState, TaskEngine, TaskOwnership,
        UnregisterOutcome,
    },
};
use shikifsrs_runtime::{Error, Result};
use std::{
    alloc::{Layout, alloc, dealloc},
    collections::BTreeMap,
    ffi::c_void,
    ptr::NonNull,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

const GROUP: u32 = 0x9000006d;
const ARGUMENT: usize = 0x12340000;
const INVOCATION: TaskInvocation = TaskInvocation {
    time_value: 0.125,
    group_id: GROUP,
    scheduler_index: 5,
};

#[derive(Default)]
struct Engine {
    allocations: Mutex<BTreeMap<usize, Layout>>,
    registered: Mutex<BTreeMap<usize, u32>>,
    pending: Mutex<Vec<usize>>,
    pending_registration: Mutex<Vec<usize>>,
    queue_registration: AtomicBool,
    frame_begin: AtomicBool,
    draw_parameters: AtomicBool,
    bank_tasks: AtomicBool,
    bank_phases: Mutex<BTreeMap<usize, TaskPhase>>,
    reject_bank_conditional: AtomicBool,
    frees: AtomicUsize,
    unregister_calls: AtomicUsize,
    reject_registration: AtomicBool,
    reject_unregistration: AtomicBool,
    queued: AtomicBool,
    fail_allocation: AtomicBool,
    panic_reader: AtomicBool,
    fail_free: AtomicBool,
}

impl Engine {
    fn address(&self) -> usize {
        *self.registered.lock().unwrap().keys().next().unwrap()
    }

    fn invoke(&self, address: usize, argument: usize) {
        let pointer = NonNull::new(address as *mut NativeTaskObject).unwrap();
        assert!(self.registered.lock().unwrap().contains_key(&address));
        // SAFETY: Synthetic engine retains this task, runs serially and cannot destroy during call.
        unsafe {
            (NativeTaskObject::vtable(pointer).execute)(
                pointer.as_ptr(),
                argument as *const c_void,
            );
        }
    }

    fn destroy(&self, address: usize, flags: u32) {
        assert!(self.registered.lock().unwrap().remove(&address).is_some());
        let pointer = NonNull::new(address as *mut NativeTaskObject).unwrap();
        // SAFETY: Synthetic engine is serialized; retained allocation belongs to this SDK vtable.
        unsafe {
            (NativeTaskObject::vtable(pointer).destructor)(pointer.as_ptr(), flags);
        }
        if flags & 1 == 0 {
            // SAFETY: The scalar destructor did not free memory; unique synthetic allocation remains.
            unsafe {
                self.deallocate(pointer.cast(), Layout::new::<NativeTaskObject>())
                    .unwrap();
            }
        }
    }

    fn drain(&self) {
        let pending = std::mem::take(&mut *self.pending.lock().unwrap());
        for address in pending {
            self.destroy(address, 0);
        }
    }

    fn insert_pending(&self) {
        for address in std::mem::take(&mut *self.pending_registration.lock().unwrap()) {
            assert!(
                self.registered
                    .lock()
                    .unwrap()
                    .insert(address, GROUP)
                    .is_none()
            );
        }
    }
}

// SAFETY: Original test engine tracks exact layouts, directly acknowledges registration,
// retains accepted cancellations until drain, and serializes execution before destruction.
unsafe impl TaskEngine for Engine {
    fn allocator_address(&self) -> usize {
        0x11111100
    }
    unsafe fn allocate(&self, layout: Layout) -> Result<NonNull<u8>> {
        if self.fail_allocation.load(Ordering::SeqCst) {
            return Err(Error::NativeAllocationFailed);
        }
        // SAFETY: Valid Rust Layout; used only as the original synthetic heap, never in the game.
        let pointer =
            NonNull::new(unsafe { alloc(layout) }).ok_or(Error::NativeAllocationFailed)?;
        assert!(
            self.allocations
                .lock()
                .unwrap()
                .insert(pointer.as_ptr() as usize, layout)
                .is_none()
        );
        Ok(pointer)
    }
    unsafe fn deallocate(&self, pointer: NonNull<u8>, layout: Layout) -> Result<()> {
        if self.fail_free.load(Ordering::SeqCst) {
            return Err(Error::NativeOperationRejected(
                "synthetic heap free rejection",
            ));
        }
        assert_eq!(
            self.allocations
                .lock()
                .unwrap()
                .remove(&(pointer.as_ptr() as usize)),
            Some(layout)
        );
        // SAFETY: Tracked unique allocation from the synthetic Rust heap and exact layout.
        unsafe {
            dealloc(pointer.as_ptr(), layout);
        }
        self.frees.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    unsafe fn register(
        &self,
        phase: TaskPhase,
        task: NonNull<NativeTaskObject>,
    ) -> Result<RegistrationReceipt> {
        if self.bank_tasks.load(Ordering::SeqCst) {
            assert!(phase == TaskPhase::SOUND_STEP || phase == TaskPhase::FRAME_BEGIN);
            if phase == TaskPhase::FRAME_BEGIN
                && self.reject_bank_conditional.load(Ordering::SeqCst)
            {
                return Err(Error::NativeOperationRejected(
                    "bank conditional registration rejected",
                ));
            }
            self.bank_phases
                .lock()
                .unwrap()
                .insert(task.as_ptr() as usize, phase);
        } else {
            assert_eq!(
                phase,
                if self.draw_parameters.load(Ordering::SeqCst) {
                    TaskPhase::DRAW_PARAMETER_UPDATE
                } else if self.frame_begin.load(Ordering::SeqCst) {
                    TaskPhase::FRAME_BEGIN
                } else {
                    TaskPhase::SOUND_STEP
                }
            );
        }
        if self.reject_registration.load(Ordering::SeqCst) {
            return Err(Error::NativeOperationRejected(
                "synthetic registration rejection",
            ));
        }
        let address = task.as_ptr() as usize;
        assert!(self.allocations.lock().unwrap().contains_key(&address));
        if self.queue_registration.load(Ordering::SeqCst) {
            self.pending_registration.lock().unwrap().push(address);
            return Ok(RegistrationReceipt {
                group_id: GROUP,
                state: RegistrationState::Pending,
            });
        }
        assert!(
            self.registered
                .lock()
                .unwrap()
                .insert(address, GROUP)
                .is_none()
        );
        Ok(RegistrationReceipt {
            group_id: GROUP,
            state: RegistrationState::Inserted,
        })
    }
    unsafe fn registration_inserted(&self, group_id: u32, address: usize) -> Result<bool> {
        Ok(self.registered.lock().unwrap().get(&address) == Some(&group_id))
    }
    unsafe fn unregister(&self, group_id: u32, address: usize) -> Result<()> {
        self.unregister_calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(group_id, GROUP);
        assert_eq!(self.registered.lock().unwrap().get(&address), Some(&GROUP));
        if self.reject_unregistration.load(Ordering::SeqCst) {
            return Err(Error::NativeOperationRejected(
                "synthetic unregister rejection",
            ));
        }
        if self.queued.load(Ordering::SeqCst) {
            self.pending.lock().unwrap().push(address);
        } else {
            self.destroy(address, 0);
        }
        Ok(())
    }
    fn read_invocation(&self, address: usize) -> Result<TaskInvocation> {
        assert!(
            !self.panic_reader.load(Ordering::SeqCst),
            "synthetic reader panic"
        );
        if address == ARGUMENT + 8 {
            return Ok(TaskInvocation {
                group_id: GROUP + 1,
                ..INVOCATION
            });
        }
        if address != ARGUMENT {
            return Err(Error::OutOfBounds {
                address: address as u64,
                length: 24,
            });
        }
        Ok(INVOCATION)
    }
}

mod gparam_pipeline {
    use super::*;
    use shikifsrs_elden_ring::{
        gparam::{CommonEventId, NativeGparamCall},
        gparam_task::{
            GparamPreloadConfig, GparamStage, GparamSubmissionState, GparamTask, GparamTaskBackend,
        },
        objects::{ObjectKind, ObjectObservation},
        task_owner::NativeTaskOwner,
    };
    #[derive(Default)]
    struct State {
        now: u64,
        clock_samples: usize,
        calls: Vec<GparamStage>,
        fail_prime: bool,
        null_request: bool,
    }
    struct Backend(Arc<Mutex<State>>);
    // SAFETY: Original state/log backend; errors are before a synthetic submission,
    // no game calls or native addresses are dereferenced.
    unsafe impl GparamTaskBackend for Backend {
        fn elapsed_ms(&self) -> Result<u64> {
            let mut state = self.0.lock().unwrap();
            state.clock_samples += 1;
            Ok(state.now)
        }
        unsafe fn request(&mut self, id: CommonEventId) -> Result<NativeGparamCall<Option<u64>>> {
            self.0.lock().unwrap().calls.push(GparamStage::FileRequest);
            Ok(NativeGparamCall {
                manager: ObjectObservation {
                    address: 0x1000,
                    kind: ObjectKind::FileResourceManager,
                },
                id,
                value: (!self.0.lock().unwrap().null_request).then_some(0x2000),
                postflight: Ok(()),
            })
        }
        unsafe fn prime(&mut self, id: CommonEventId) -> Result<NativeGparamCall<()>> {
            let mut state = self.0.lock().unwrap();
            if std::mem::take(&mut state.fail_prime) {
                return Err(Error::NativeOperationRejected("prime preflight"));
            }
            state.calls.push(GparamStage::Prime);
            Ok(NativeGparamCall {
                manager: ObjectObservation {
                    address: 0x3000,
                    kind: ObjectKind::WorldSceneDrawParameters,
                },
                id,
                value: (),
                postflight: Ok(()),
            })
        }
    }
    #[test]
    fn draw_parameter_native_task_delay_partial_request_retry_then_explicit_owner_close() {
        let engine = Arc::new(Engine::default());
        engine.draw_parameters.store(true, Ordering::SeqCst);
        let state = Arc::new(Mutex::new(State {
            fail_prime: true,
            ..Default::default()
        }));
        let config = GparamPreloadConfig {
            start_delay_ms: 5,
            retry_every_frames: 2,
            ..Default::default()
        };
        // SAFETY: Original logged submissions and synthetic serialized native task engine.
        let task = unsafe { GparamTask::new(Backend(Arc::clone(&state)), config, 0) }.unwrap();
        assert_eq!(state.lock().unwrap().clock_samples, 0);
        let mut owner = NativeTaskOwner::new(engine.clone());
        // SAFETY: Original mock registration and owned callback/allocator residency.
        let (_, control) = unsafe { task.register(&mut owner) }.unwrap();
        let address = engine.address();
        engine.invoke(address, ARGUMENT);
        assert_eq!(control.last_report().unwrap().frame, 0);
        assert!(state.lock().unwrap().calls.is_empty());
        state.lock().unwrap().now = 5;
        engine.invoke(address, ARGUMENT);
        let report = control.last_report().unwrap();
        assert_eq!(report.invocation, INVOCATION);
        assert_eq!(report.frame, 1);
        assert!(!report.settled);
        assert!(!report.blocked);
        assert!(report.error.is_none());
        assert_eq!(report.submissions[0].state, GparamSubmissionState::Pending);
        assert_eq!(
            report.submissions[0].file_request.as_ref().unwrap().value,
            Some(0x2000)
        );
        engine.invoke(address, ARGUMENT);
        assert!(!control.last_report().unwrap().ran);
        engine.invoke(address, ARGUMENT);
        let report = control.last_report().unwrap();
        assert!(report.settled);
        assert!(!report.blocked);
        assert_eq!(
            report.submissions[0].state,
            GparamSubmissionState::Submitted
        );
        assert_eq!(report.submissions[0].attempts, 2);
        let clock_samples = state.lock().unwrap().clock_samples;
        engine.invoke(address, ARGUMENT);
        assert_eq!(state.lock().unwrap().clock_samples, clock_samples);
        control.request_stop();
        engine.invoke(address, ARGUMENT);
        assert!(control.last_report().unwrap().closing);
        assert_eq!(
            state.lock().unwrap().calls,
            vec![GparamStage::FileRequest, GparamStage::Prime]
        );
        // SAFETY: Mock engine is quiescent and frees only its owned test allocation.
        assert_eq!(unsafe { owner.close_step() }.remaining_tasks, 0);
        assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
        assert!(engine.allocations.lock().unwrap().is_empty());
        assert!(
            control.last_report().unwrap().submissions[0]
                .file_request
                .is_some()
        );
    }
    #[test]
    fn draw_parameter_stop_retains_prior_request_and_cancels_only_unsubmitted_work() {
        let engine = Arc::new(Engine::default());
        engine.draw_parameters.store(true, Ordering::SeqCst);
        let state = Arc::new(Mutex::new(State {
            fail_prime: true,
            ..Default::default()
        }));
        let config = GparamPreloadConfig {
            start_delay_ms: 0,
            retry_every_frames: 1,
            ..Default::default()
        };
        // SAFETY: Original mock resources/task allocation; no actual game submission.
        let task = unsafe { GparamTask::new(Backend(Arc::clone(&state)), config, 0) }.unwrap();
        let mut owner = NativeTaskOwner::new(engine.clone());
        // SAFETY: Serialized synthetic registration under retained mock heap/callback lifetime.
        let (_, control) = unsafe { task.register(&mut owner) }.unwrap();
        let address = engine.address();
        engine.invoke(address, ARGUMENT);
        control.request_stop();
        let samples = state.lock().unwrap().clock_samples;
        engine.invoke(address, ARGUMENT);
        let report = control.last_report().unwrap();
        assert!(report.settled);
        assert!(report.closing);
        assert_eq!(
            report.submissions[0].state,
            GparamSubmissionState::Cancelled
        );
        assert!(report.submissions[0].file_request.is_some());
        assert!(report.submissions[0].prime.is_none());
        assert_eq!(state.lock().unwrap().calls, vec![GparamStage::FileRequest]);
        assert_eq!(state.lock().unwrap().clock_samples, samples);
        // SAFETY: Native task teardown applies solely to mock allocation, never resource receipt.
        assert_eq!(unsafe { owner.close_step() }.remaining_tasks, 0);
        assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
    }
    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn resident_gparam_keeps_direct_and_queued_callbacks_and_partial_receipts_without_public_controls()
     {
        use shikifsrs_elden_ring::gparam_resident::{ResidentGparamSlot, ResidentGparamState};
        static DIRECT: ResidentGparamSlot = ResidentGparamSlot::new();
        static QUEUED: ResidentGparamSlot = ResidentGparamSlot::new();
        for (slot, queued) in [(&DIRECT, false), (&QUEUED, true)] {
            let engine = Arc::new(Engine::default());
            engine.draw_parameters.store(true, Ordering::SeqCst);
            engine.queue_registration.store(queued, Ordering::SeqCst);
            let state = Arc::new(Mutex::new(State {
                fail_prime: true,
                ..Default::default()
            }));
            // SAFETY: Original resource log and owned synthetic task allocator; no game calls.
            let task = unsafe {
                GparamTask::new(
                    Backend(state.clone()),
                    GparamPreloadConfig {
                        start_delay_ms: 5,
                        retry_every_frames: 2,
                        ..Default::default()
                    },
                    0,
                )
            }
            .unwrap();
            // SAFETY: Static mock host and serialized registration. Only test executable is pinned.
            let installed = unsafe { slot.install_task(engine.clone(), task) }.unwrap();
            assert_ne!(installed.module_handle, 0);
            let control = installed.registration.unwrap();
            assert_eq!(slot.state(), ResidentGparamState::Registered);
            assert_eq!(
                control.task_status().registration,
                if queued {
                    RegistrationState::Pending
                } else {
                    RegistrationState::Inserted
                }
            );
            assert_eq!(state.lock().unwrap().clock_samples, 0);
            drop(control);
            if queued {
                engine.insert_pending();
            }
            let address = engine.address();
            engine.invoke(address, ARGUMENT);
            assert!(state.lock().unwrap().calls.is_empty());
            state.lock().unwrap().now = 5;
            engine.invoke(address, ARGUMENT);
            let control = slot.control().unwrap();
            let partial = control.last_report().unwrap();
            assert_eq!(partial.submissions[0].attempts, 1);
            assert!(partial.submissions[0].file_request.is_some());
            assert!(partial.submissions[0].prime.is_none());
            assert!(!control.task_status().callback.cancelled);
            drop(control);
            engine.invoke(address, ARGUMENT);
            engine.invoke(address, ARGUMENT);
            let control = slot.control().unwrap();
            let report = control.last_report().unwrap();
            assert!(report.settled && !report.blocked);
            assert_eq!(
                report.submissions[0].state,
                GparamSubmissionState::Submitted
            );
            assert_eq!(report.submissions[0].attempts, 2);
            assert_eq!(
                state.lock().unwrap().calls,
                [GparamStage::FileRequest, GparamStage::Prime]
            );
            control.request_stop();
            drop(control);
            let samples = state.lock().unwrap().clock_samples;
            engine.invoke(address, ARGUMENT);
            assert_eq!(state.lock().unwrap().clock_samples, samples);
            assert!(slot.control().unwrap().last_report().unwrap().closing);
            assert_eq!(engine.unregister_calls.load(Ordering::SeqCst), 0);
            // SAFETY: The synthetic engine alone retires its allocation after the callback is idle.
            engine.destroy(address, 0);
            assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
            assert!(slot.control().unwrap().task_status().destructor_completed);
        }
    }
    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn resident_gparam_retains_null_attempt_and_latches_pinned_registration_failure() {
        use shikifsrs_elden_ring::gparam_resident::{ResidentGparamSlot, ResidentGparamState};
        static NULL: ResidentGparamSlot = ResidentGparamSlot::new();
        static FAILURE: ResidentGparamSlot = ResidentGparamSlot::new();
        // SAFETY: File identity fails before any base-zero native read, allocation or pin.
        let invalid = unsafe {
            FAILURE.install_current(
                Arc::from(b"not a game executable".as_slice()),
                0,
                GparamPreloadConfig::default(),
                None,
            )
        };
        assert!(matches!(invalid, Err(Error::UnsupportedImage { .. })));
        assert_eq!(FAILURE.state(), ResidentGparamState::Vacant);
        let failed = Arc::new(Engine::default());
        failed.fail_allocation.store(true, Ordering::SeqCst);
        // SAFETY: Original mock resource backend; constructed task performs no submission.
        let task = unsafe {
            GparamTask::new(
                Backend(Arc::new(Mutex::new(State::default()))),
                GparamPreloadConfig::default(),
                0,
            )
        }
        .unwrap();
        // SAFETY: Original failing allocator, static slot; Windows pin targets only test executable.
        let receipt = unsafe { FAILURE.install_task(failed.clone(), task) }.unwrap();
        assert_ne!(receipt.module_handle, 0);
        assert!(matches!(
            receipt.registration,
            Err(Error::NativeAllocationFailed)
        ));
        assert_eq!(FAILURE.state(), ResidentGparamState::RegistrationFailed);
        assert!(FAILURE.control().is_none());
        failed.fail_allocation.store(false, Ordering::SeqCst);
        // SAFETY: Same synthetic pre-call-only backend.
        let task = unsafe {
            GparamTask::new(
                Backend(Arc::new(Mutex::new(State::default()))),
                GparamPreloadConfig::default(),
                0,
            )
        }
        .unwrap();
        // SAFETY: Repeat is rejected before allocator entry.
        assert!(unsafe { FAILURE.install_task(failed.clone(), task) }.is_err());
        assert!(failed.allocations.lock().unwrap().is_empty());

        let engine = Arc::new(Engine::default());
        engine.draw_parameters.store(true, Ordering::SeqCst);
        let state = Arc::new(Mutex::new(State {
            null_request: true,
            ..Default::default()
        }));
        // SAFETY: Original synthetic null-return backend, no native resource or game memory.
        let task = unsafe {
            GparamTask::new(
                Backend(state.clone()),
                GparamPreloadConfig {
                    start_delay_ms: 0,
                    retry_every_frames: 1,
                    ..Default::default()
                },
                0,
            )
        }
        .unwrap();
        // SAFETY: Same serialized synthetic allocator/registration with test-executable pin.
        let installed = unsafe { NULL.install_task(engine.clone(), task) }.unwrap();
        drop(installed.registration.unwrap());
        let address = engine.address();
        engine.invoke(address, ARGUMENT);
        let report = NULL.control().unwrap().last_report().unwrap();
        assert!(report.blocked);
        assert_eq!(
            report.submissions[0].state,
            GparamSubmissionState::Uncertain(GparamStage::FileRequest)
        );
        assert_eq!(
            report.submissions[0].file_request.as_ref().unwrap().value,
            None
        );
        let samples = state.lock().unwrap().clock_samples;
        for _ in 0..3 {
            engine.invoke(address, ARGUMENT);
        }
        NULL.control().unwrap().request_stop();
        engine.invoke(address, ARGUMENT);
        assert_eq!(state.lock().unwrap().clock_samples, samples);
        assert_eq!(state.lock().unwrap().calls, [GparamStage::FileRequest]);
        assert!(NULL.control().unwrap().last_report().unwrap().blocked);
        assert_eq!(engine.unregister_calls.load(Ordering::SeqCst), 0);
        // SAFETY: Mock task is no longer executing; only the synthetic allocation is released.
        engine.destroy(address, 0);
        assert!(engine.allocations.lock().unwrap().is_empty());
    }
}

struct Capture(Arc<AtomicUsize>);
impl Drop for Capture {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn prepare(engine: &Arc<Engine>, callback: TaskCallback) -> PreparedTask {
    // SAFETY: Synthetic engine/heap are live, serialized, and retained through each test.
    unsafe { PreparedTask::new(engine.clone(), callback).unwrap() }
}

fn register(task: PreparedTask) -> shikifsrs_elden_ring::native_task::RegisteredTask {
    // SAFETY: Test engine inserts directly; no competing scheduling or teardown.
    unsafe { task.register(TaskPhase::SOUND_STEP).unwrap() }
}

#[test]
fn abi_layout_and_actual_virtual_calls_preserve_allocator_and_arguments() {
    assert_eq!(std::mem::offset_of!(NativeTaskVTable, allocator), 0);
    assert_eq!(std::mem::offset_of!(NativeTaskVTable, destructor), 8);
    assert_eq!(std::mem::offset_of!(NativeTaskVTable, execute), 16);
    assert_eq!(std::mem::size_of::<NativeTaskObject>(), 24);
    let engine = Arc::new(Engine::default());
    let values = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&values);
    let registered = register(prepare(
        &engine,
        TaskCallback::new(move |input| captured.lock().unwrap().push(input)),
    ));
    let address = engine.address();
    let pointer = NonNull::new(address as *mut NativeTaskObject).unwrap();
    // SAFETY: Live original test-engine allocation; no execution/destruction races.
    assert_eq!(
        // SAFETY: Live tracked test-engine allocation, no concurrent destruction.
        unsafe { (NativeTaskObject::vtable(pointer).allocator)(pointer.as_ptr()) } as usize,
        engine.allocator_address()
    );
    engine.invoke(address, ARGUMENT);
    assert_eq!(*values.lock().unwrap(), vec![INVOCATION]);
    assert_eq!(registered.control().status().group_id, Some(GROUP));
    engine.destroy(address, 0);
    assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
}

#[test]
fn unregistered_owner_drops_captures_and_exact_heap_allocation_once() {
    let engine = Arc::new(Engine::default());
    let drops = Arc::new(AtomicUsize::new(0));
    let capture = Capture(Arc::clone(&drops));
    let prepared = prepare(
        &engine,
        TaskCallback::new(move |_| {
            let _keep = &capture;
        }),
    );
    let control = prepared.control();
    assert_eq!(control.status().ownership, TaskOwnership::Sdk);
    drop(prepared);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
    assert_eq!(
        control.status().memory_release,
        MemoryRelease::ReleasedBySdk
    );
    assert_eq!(
        control.wait_for_destructor(Duration::ZERO),
        DestructorWait::Completed
    );
}

#[test]
fn rejected_registration_never_transfers_and_reclaims_the_allocation() {
    let engine = Arc::new(Engine::default());
    engine.reject_registration.store(true, Ordering::SeqCst);
    let prepared = prepare(&engine, TaskCallback::new(|_| {}));
    let control = prepared.control();
    // SAFETY: Test backend rejects without retaining anything.
    assert!(unsafe { prepared.register(TaskPhase::SOUND_STEP) }.is_err());
    assert_eq!(control.status().ownership, TaskOwnership::Destroyed);
    assert_eq!(
        control.status().memory_release,
        MemoryRelease::ReleasedBySdk
    );
    assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
    assert!(engine.registered.lock().unwrap().is_empty());
}

#[test]
fn allocation_failure_drops_rust_captures_without_register_or_free() {
    let engine = Arc::new(Engine::default());
    engine.fail_allocation.store(true, Ordering::SeqCst);
    let drops = Arc::new(AtomicUsize::new(0));
    let capture = Capture(Arc::clone(&drops));
    // SAFETY: Live original backend reports allocation failure without transferring anything.
    assert!(
        // SAFETY: Original test backend reports allocation failure without transferring ownership.
        unsafe {
            PreparedTask::new(
                engine.clone(),
                TaskCallback::new(move |_| {
                    let _keep = &capture;
                }),
            )
        }
        .is_err()
    );
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(engine.frees.load(Ordering::SeqCst), 0);
}

#[test]
fn queued_unregister_is_not_destructor_or_memory_release_acknowledgement() {
    let engine = Arc::new(Engine::default());
    engine.queued.store(true, Ordering::SeqCst);
    let registered = register(prepare(&engine, TaskCallback::new(|_| {})));
    let control = registered.control();
    // SAFETY: Synthetic engine supports queued cancellation and remains resident until drain.
    assert_eq!(
        // SAFETY: Serialized synthetic engine; pending/completed states never resubmit a freed task.
        unsafe { control.unregister().unwrap() },
        UnregisterOutcome::Submitted
    );
    assert!(control.status().unregister_submitted);
    assert!(!control.status().destructor_completed);
    assert_eq!(
        control.wait_for_destructor(Duration::ZERO),
        DestructorWait::TimedOut
    );
    assert_eq!(engine.frees.load(Ordering::SeqCst), 0);
    // SAFETY: Pending cancellation is deduplicated without reusing the native pointer.
    assert_eq!(
        // SAFETY: Serialized synthetic engine; pending/completed states never resubmit a freed task.
        unsafe { control.unregister().unwrap() },
        UnregisterOutcome::AlreadySubmitted
    );
    assert_eq!(engine.unregister_calls.load(Ordering::SeqCst), 1);
    engine.drain();
    assert_eq!(
        control.wait_for_destructor(Duration::ZERO),
        DestructorWait::Completed
    );
    assert_eq!(
        control.status().memory_release,
        MemoryRelease::EngineResponsible
    );
    assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
    // SAFETY: Completed state short-circuits without passing a freed pointer to the engine.
    assert_eq!(
        // SAFETY: Serialized synthetic engine; pending/completed states never resubmit a freed task.
        unsafe { control.unregister().unwrap() },
        UnregisterOutcome::DestructorObserved
    );
    assert_eq!(engine.unregister_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn failed_unregister_retains_engine_allocation_and_can_be_retried() {
    let engine = Arc::new(Engine::default());
    engine.reject_unregistration.store(true, Ordering::SeqCst);
    let registered = register(prepare(&engine, TaskCallback::new(|_| {})));
    let control = registered.control();
    // SAFETY: Serialized test-engine cancellation; rejection retains the object.
    assert!(unsafe { control.unregister() }.is_err());
    assert!(!control.status().unregister_submitted);
    assert_eq!(control.status().ownership, TaskOwnership::Engine);
    assert_eq!(engine.frees.load(Ordering::SeqCst), 0);
    engine.reject_unregistration.store(false, Ordering::SeqCst);
    // SAFETY: Same live retained object; no accepted cancellation exists.
    assert_eq!(
        // SAFETY: Serialized synthetic engine; pending/completed states never resubmit a freed task.
        unsafe { control.unregister().unwrap() },
        UnregisterOutcome::Submitted
    );
    assert_eq!(engine.unregister_calls.load(Ordering::SeqCst), 2);
    assert!(control.status().destructor_completed);
}

#[test]
fn dropping_registered_handle_stops_rust_code_without_freeing_native_object() {
    let engine = Arc::new(Engine::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let captured = Arc::clone(&calls);
    let registered = register(prepare(
        &engine,
        TaskCallback::new(move |_| {
            captured.fetch_add(1, Ordering::SeqCst);
        }),
    ));
    let control = registered.control();
    let address = engine.address();
    drop(registered);
    assert_eq!(control.status().ownership, TaskOwnership::Engine);
    assert!(control.status().callback.resources_released);
    assert_eq!(engine.frees.load(Ordering::SeqCst), 0);
    engine.invoke(address, ARGUMENT);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // SAFETY: Explicit cancellation owns the synthetic scheduling phase.
    unsafe {
        control.unregister().unwrap();
    }
    assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
}

#[test]
fn callback_can_request_queued_unregister_and_self_wait_is_rejected() {
    let engine = Arc::new(Engine::default());
    engine.queued.store(true, Ordering::SeqCst);
    let slot: Arc<Mutex<Option<NativeTaskControl>>> = Arc::new(Mutex::new(None));
    let captured = Arc::clone(&slot);
    let registered = register(prepare(
        &engine,
        TaskCallback::new(move |_| {
            let control = captured.lock().unwrap().as_ref().unwrap().clone();
            // SAFETY: Original synthetic engine queues while executing, and destroys only on drain.
            assert_eq!(
                // SAFETY: Serialized synthetic engine; pending/completed states never resubmit a freed task.
                unsafe { control.unregister().unwrap() },
                UnregisterOutcome::Submitted
            );
            assert_eq!(
                control.wait_for_destructor(Duration::from_secs(1)),
                DestructorWait::WouldBlockCurrentCallback
            );
        }),
    ));
    let control = registered.control();
    *slot.lock().unwrap() = Some(control.clone());
    engine.invoke(engine.address(), ARGUMENT);
    assert!(!control.status().destructor_completed);
    engine.drain();
    assert!(control.status().destructor_completed);
}

#[test]
fn invalid_invocation_is_rejected_and_future_user_code_is_stopped() {
    let engine = Arc::new(Engine::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let captured = Arc::clone(&calls);
    let registered = register(prepare(
        &engine,
        TaskCallback::new(move |_| {
            captured.fetch_add(1, Ordering::SeqCst);
        }),
    ));
    engine.invoke(engine.address(), 0);
    engine.invoke(engine.address(), ARGUMENT);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(registered.control().status().rejected_invocations, 1);
    assert!(registered.control().status().last_error.is_some());
    engine.destroy(engine.address(), 0);
}

#[test]
fn reader_and_user_panics_cannot_unwind_through_the_execute_virtual_slot() {
    let engine = Arc::new(Engine::default());
    engine.panic_reader.store(true, Ordering::SeqCst);
    let registered = register(prepare(&engine, TaskCallback::new(|_| {})));
    engine.invoke(engine.address(), ARGUMENT);
    assert!(registered.control().status().abi_panicked);
    engine.destroy(engine.address(), 0);
    engine.panic_reader.store(false, Ordering::SeqCst);
    let registered = register(prepare(
        &engine,
        TaskCallback::new(|_| panic!("synthetic user callback")),
    ));
    engine.invoke(engine.address(), ARGUMENT);
    assert!(registered.control().status().callback.panicked);
    engine.destroy(engine.address(), 0);
}

#[test]
fn deleting_destructor_flag_frees_itself_and_captures_are_dropped_once() {
    let engine = Arc::new(Engine::default());
    let drops = Arc::new(AtomicUsize::new(0));
    let capture = Capture(Arc::clone(&drops));
    let registered = register(prepare(
        &engine,
        TaskCallback::new(move |_| {
            let _keep = &capture;
        }),
    ));
    let control = registered.control();
    engine.destroy(engine.address(), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
    assert_eq!(
        control.status().memory_release,
        MemoryRelease::ReleasedBySdk
    );
    drop(registered);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn failed_sdk_heap_free_is_reported_without_claiming_release_or_freeing_again() {
    let engine = Arc::new(Engine::default());
    engine.fail_free.store(true, Ordering::SeqCst);
    let prepared = prepare(&engine, TaskCallback::new(|_| {}));
    let control = prepared.control();
    drop(prepared);
    assert_eq!(control.status().memory_release, MemoryRelease::Failed);
    assert!(control.status().destructor_completed);
    assert!(control.status().last_error.is_some());
    assert_eq!(engine.frees.load(Ordering::SeqCst), 0);
    let (address, layout) = {
        let allocations = engine.allocations.lock().unwrap();
        let (address, layout) = allocations.iter().next().unwrap();
        (*address, *layout)
    };
    engine.fail_free.store(false, Ordering::SeqCst);
    // SAFETY: Test cleanup uniquely owns the retained, already-destructed synthetic allocation.
    unsafe {
        engine
            .deallocate(NonNull::new(address as *mut u8).unwrap(), layout)
            .unwrap();
    }
    assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
}

#[test]
fn valid_argument_with_the_wrong_full_group_id_never_reaches_user_code() {
    let engine = Arc::new(Engine::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let captured = Arc::clone(&calls);
    let registered = register(prepare(
        &engine,
        TaskCallback::new(move |_| {
            captured.fetch_add(1, Ordering::SeqCst);
        }),
    ));
    engine.invoke(engine.address(), ARGUMENT + 8);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(registered.control().status().rejected_invocations, 1);
    assert!(registered.control().status().callback.resources_released);
    engine.destroy(engine.address(), 0);
}

#[test]
fn pending_registration_never_submits_cancellation_before_insertion() {
    let engine = Arc::new(Engine::default());
    engine.queue_registration.store(true, Ordering::SeqCst);
    engine.queued.store(true, Ordering::SeqCst);
    let calls = Arc::new(AtomicUsize::new(0));
    let captured = Arc::clone(&calls);
    let registered = register(prepare(
        &engine,
        TaskCallback::new(move |_| {
            captured.fetch_add(1, Ordering::SeqCst);
        }),
    ));
    let control = registered.control();
    assert_eq!(control.status().registration, RegistrationState::Pending);
    // SAFETY: Original serialized engine; pending registration is retained until insertion.
    assert_eq!(
        // SAFETY: Original serialized engine; pending queue retains the task.
        unsafe { control.unregister().unwrap() },
        UnregisterOutcome::WaitingForRegistration
    );
    assert_eq!(engine.unregister_calls.load(Ordering::SeqCst), 0);
    engine.drain(); // Post-dispatch cancellation queue may be drained before the group queue.
    assert_eq!(engine.frees.load(Ordering::SeqCst), 0);
    assert_eq!(
        control.wait_for_destructor(Duration::ZERO),
        DestructorWait::TimedOut
    );
    engine.insert_pending();
    let address = engine.address();
    engine.invoke(address, ARGUMENT);
    assert_eq!(calls.load(Ordering::SeqCst), 0); // Early cancellation already stopped Rust code.
    assert_eq!(control.status().registration, RegistrationState::Inserted);
    // SAFETY: Insertion is confirmed; original engine remains serialized and retains storage.
    assert_eq!(
        // SAFETY: Confirmed insertion in the original serialized test engine.
        unsafe { control.unregister().unwrap() },
        UnregisterOutcome::Submitted
    );
    engine.drain();
    assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
    assert_eq!(
        control.wait_for_destructor(Duration::ZERO),
        DestructorWait::Completed
    );
}

#[test]
fn registry_confirmation_allows_cancelling_a_pending_task_without_executing_it() {
    let engine = Arc::new(Engine::default());
    engine.queue_registration.store(true, Ordering::SeqCst);
    let registered = register(prepare(
        &engine,
        TaskCallback::new(|_| panic!("must never run")),
    ));
    engine.insert_pending();
    assert_eq!(
        registered.control().status().registration,
        RegistrationState::Pending
    );
    // SAFETY: Original registry confirms this task's insertion in the serialized engine.
    assert_eq!(
        // SAFETY: Synthetic registry confirms insertion before cancellation.
        unsafe { registered.control().unregister().unwrap() },
        UnregisterOutcome::Submitted
    );
    assert_eq!(
        registered.control().status().registration,
        RegistrationState::Inserted
    );
    assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
}

#[test]
fn owner_closes_registration_gate_retries_and_retains_tasks_until_destructors() {
    use shikifsrs_elden_ring::task_owner::{NativeTaskOwner, TaskOwnerState};
    let engine = Arc::new(Engine::default());
    engine.queued.store(true, Ordering::SeqCst);
    let mut owner = NativeTaskOwner::new(engine.clone());
    // SAFETY: Original test heap and host phase, serialized through receipt recording.
    let first = unsafe {
        owner
            .register(TaskPhase::SOUND_STEP, TaskCallback::new(|_| {}))
            .unwrap()
    };
    // SAFETY: Same original synthetic host phase and allocator contract.
    let second = unsafe {
        owner
            .register(TaskPhase::SOUND_STEP, TaskCallback::new(|_| {}))
            .unwrap()
    };
    let control = owner.control(first).unwrap();
    engine.reject_unregistration.store(true, Ordering::SeqCst);
    // SAFETY: Original serialized host phase; no unrelated teardown.
    let rejected = unsafe { owner.close_step() };
    assert_eq!(rejected.failures.len(), 2);
    assert_eq!(rejected.state, TaskOwnerState::Closing);
    assert_eq!(rejected.remaining_tasks, 2);
    assert!(owner.control(second).unwrap().status().callback.cancelled);
    // SAFETY: Phase remains valid, but the owner must reject the now-closed gate.
    assert!(unsafe { owner.register(TaskPhase::SOUND_STEP, TaskCallback::new(|_| {})) }.is_err());
    assert_eq!(engine.allocations.lock().unwrap().len(), 2);
    engine.reject_unregistration.store(false, Ordering::SeqCst);
    // SAFETY: Explicit retry in original serialized cancellation phase.
    let accepted = unsafe { owner.close_step() };
    assert_eq!(accepted.submitted, 2);
    assert_eq!(accepted.remaining_tasks, 2);
    // SAFETY: Same host phase; accepted submissions must not duplicate native commands.
    assert_eq!(unsafe { owner.close_step() }.pending_destruction, 2);
    assert_eq!(engine.pending.lock().unwrap().len(), 2);
    engine.drain();
    assert_eq!(owner.reap_destroyed(), 2);
    assert_eq!(owner.state(), TaskOwnerState::PayloadsDestroyed);
    assert!(owner.statuses().is_empty());
    assert_eq!(
        control.status().memory_release,
        MemoryRelease::EngineResponsible
    );
    assert_eq!(engine.frees.load(Ordering::SeqCst), 2);
}

#[test]
fn owner_close_retries_pending_insertion_and_drop_only_stops_rust_callbacks() {
    use shikifsrs_elden_ring::task_owner::{NativeTaskOwner, TaskOwnerState};
    let engine = Arc::new(Engine::default());
    engine.queue_registration.store(true, Ordering::SeqCst);
    let mut owner = NativeTaskOwner::new(engine.clone());
    // SAFETY: Original synthetic serialized registration phase, no competing dispatch.
    let id = unsafe {
        owner
            .register(TaskPhase::SOUND_STEP, TaskCallback::new(|_| {}))
            .unwrap()
    };
    let control = owner.control(id).unwrap();
    // SAFETY: Native pointer remains retained in the original synthetic queue.
    let pending = unsafe { owner.close_step() };
    assert_eq!(pending.waiting_registration, 1);
    assert_eq!(pending.state, TaskOwnerState::Closing);
    assert_eq!(engine.unregister_calls.load(Ordering::SeqCst), 0);
    drop(owner);
    assert!(control.status().callback.cancelled);
    assert_eq!(engine.frees.load(Ordering::SeqCst), 0);
    engine.insert_pending();
    // SAFETY: Surviving control confirms registry insertion before serialized cancellation.
    unsafe {
        control.unregister().unwrap();
    }
    assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
}

#[test]
fn owner_keeps_failed_heap_release_visible_after_payload_destruction() {
    use shikifsrs_elden_ring::task_owner::{NativeTaskOwner, TaskOwnerState};
    let engine = Arc::new(Engine::default());
    let mut owner = NativeTaskOwner::new(engine.clone());
    // SAFETY: Original serialized synthetic allocator and registration phase.
    unsafe {
        owner
            .register(TaskPhase::SOUND_STEP, TaskCallback::new(|_| {}))
            .unwrap();
    }
    let address = engine.address();
    engine.fail_free.store(true, Ordering::SeqCst);
    engine.destroy(address, 1);
    assert_eq!(owner.reap_destroyed(), 0);
    // SAFETY: Serialized original host phase; the failed release must remain in the ledger.
    let result = unsafe { owner.close_step() };
    assert_eq!(result.state, TaskOwnerState::Closing);
    assert_eq!(result.remaining_tasks, 1);
    assert_eq!(result.failures.len(), 1);
    assert_eq!(engine.unregister_calls.load(Ordering::SeqCst), 0);
    engine.fail_free.store(false, Ordering::SeqCst);
    // SAFETY: Explicit test cleanup owns the retained allocation, whose payload is destroyed.
    unsafe {
        engine
            .deallocate(
                NonNull::new(address as *mut u8).unwrap(),
                Layout::new::<NativeTaskObject>(),
            )
            .unwrap();
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn unknown_file_is_rejected_before_the_current_process_binding_can_read_or_call() {
    use shikifsrs_elden_ring::current_tasks::CurrentTaskEngine;
    // SAFETY: Deliberately invalid bytes fail the complete file gate before accessing base zero.
    let result = unsafe { CurrentTaskEngine::bind(Arc::from(b"unknown file".as_slice()), 0) };
    assert!(matches!(result, Err(Error::UnsupportedImage { .. })));
}

// Full pipeline uses this file's original native task allocator/vtable/registration engine.
mod input_pipeline {
    use super::*;
    use shikifsrs_elden_ring::{
        character_sets::CharacterIdentity,
        effects::{EffectApplication, EffectEngine, EffectId, InputEffect},
        input_effect_task::{InputEffectTask, InputEffectTaskBackend},
        task_owner::NativeTaskOwner,
    };
    use shikifsrs_runtime::input::{
        ForegroundWindow, GamepadState, Input, InputSource, VirtualKey,
    };
    #[derive(Default)]
    struct State {
        down: bool,
        focused: bool,
        target: Option<CharacterIdentity>,
        calls: Vec<(bool, i32)>,
        effect_targets: Vec<CharacterIdentity>,
        fail_remove: bool,
        target_error: bool,
        polls: usize,
        keys: Option<std::collections::BTreeSet<u8>>,
        pad: Option<GamepadState>,
        #[cfg(all(windows, target_arch = "x86_64"))]
        epochs:
            Option<shikifsrs_elden_ring::current_input_effect_task::CurrentInputEffectEpochControl>,
    }
    struct Backend(Arc<Mutex<State>>);
    impl InputSource for Backend {
        fn foreground(&self) -> Result<Option<ForegroundWindow>> {
            let mut state = self.0.lock().unwrap();
            state.polls += 1;
            Ok(Some(ForegroundWindow {
                handle: 1,
                process_id: if state.focused { 42 } else { 99 },
            }))
        }
        fn key_state(&self, key: VirtualKey) -> Result<i16> {
            let state = self.0.lock().unwrap();
            Ok(
                if state
                    .keys
                    .as_ref()
                    .map_or(state.down, |keys| keys.contains(&key.code()))
                {
                    i16::MIN
                } else {
                    0
                },
            )
        }
        fn gamepad(&self, index: u8) -> Result<Option<GamepadState>> {
            assert_eq!(index, 0);
            Ok(self.0.lock().unwrap().pad)
        }
    }
    // SAFETY: Original fake only logs arguments, no target dereference/native effects.
    unsafe impl EffectEngine for Backend {
        unsafe fn apply(
            &self,
            target: CharacterIdentity,
            id: EffectId,
            force: bool,
        ) -> Result<EffectApplication> {
            assert!(force);
            let mut state = self.0.lock().unwrap();
            state.calls.push((true, id.value()));
            state.effect_targets.push(target);
            Ok(EffectApplication::Submitted)
        }
        unsafe fn remove(&self, target: CharacterIdentity, id: EffectId) -> Result<bool> {
            let mut state = self.0.lock().unwrap();
            state.calls.push((false, id.value()));
            state.effect_targets.push(target);
            if std::mem::take(&mut state.fail_remove) {
                Err(Error::NativeOperationRejected("remove"))
            } else {
                Ok(false)
            }
        }
    }
    impl InputEffectTaskBackend for Backend {
        fn target(&mut self) -> Result<Option<CharacterIdentity>> {
            let state = self.0.lock().unwrap();
            if state.target_error {
                Err(Error::InvalidProfile("target"))
            } else {
                let target = state.target;
                #[cfg(all(windows, target_arch = "x86_64"))]
                let target = target.map(|mut target| {
                    if let Some(control) = &state.epochs {
                        target.epoch = control.requested_epoch();
                    }
                    target
                });
                Ok(target)
            }
        }
    }
    fn target() -> CharacterIdentity {
        CharacterIdentity {
            address: 0x1000,
            world_address: 0x2000,
            native_identifier: 0xfedcba9876543210,
            epoch: 7,
        }
    }
    fn setup() -> (
        Arc<Engine>,
        NativeTaskOwner,
        Arc<Mutex<State>>,
        InputEffectTask,
    ) {
        let engine = Arc::new(Engine::default());
        engine.frame_begin.store(true, Ordering::SeqCst);
        let state = Arc::new(Mutex::new(State {
            focused: true,
            down: true,
            target: Some(target()),
            ..State::default()
        }));
        // SAFETY: Owned original mock backend, synthetic native task engine, no game calls.
        let pipeline = unsafe {
            InputEffectTask::new(
                Backend(Arc::clone(&state)),
                42,
                [InputEffect {
                    input: Input::Key(VirtualKey::new(0x41).unwrap()),
                    effect: EffectId::new(899000).unwrap(),
                }],
            )
        }
        .unwrap();
        let owner = NativeTaskOwner::new(engine.clone());
        (engine, owner, state, pipeline)
    }
    #[test]
    fn frame_begin_native_callback_polling_effects_cleanup_retry_then_owner_cancellation() {
        let (engine, mut owner, state, pipeline) = setup();
        // SAFETY: Serialized original mock registration and owned callback lifetimes.
        let (_, control) = unsafe { pipeline.register(&mut owner) }.unwrap();
        let address = engine.address();
        engine.invoke(address, ARGUMENT);
        engine.invoke(address, ARGUMENT);
        assert_eq!(
            state.lock().unwrap().calls,
            vec![(true, 899000), (true, 899000)]
        );
        assert_eq!(control.last_report().unwrap().invocation, INVOCATION);
        state.lock().unwrap().focused = false;
        engine.invoke(address, ARGUMENT);
        assert_eq!(state.lock().unwrap().calls.last(), Some(&(false, 899000)));
        state.lock().unwrap().focused = true;
        engine.invoke(address, ARGUMENT);
        state.lock().unwrap().fail_remove = true;
        let polls = state.lock().unwrap().polls;
        control.request_cleanup();
        engine.invoke(address, ARGUMENT);
        assert!(!control.last_report().unwrap().cleanup_complete());
        engine.invoke(address, ARGUMENT);
        assert!(control.last_report().unwrap().cleanup_complete());
        assert_eq!(state.lock().unwrap().polls, polls); // cleanup does not sample user input.
        // SAFETY: Cleanup complete for original fake, serialized original engine shutdown.
        let closed = unsafe { owner.close_step() };
        assert_eq!(closed.remaining_tasks, 0);
        assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
        assert!(engine.allocations.lock().unwrap().is_empty());
    }
    #[test]
    fn lost_full_identity_and_target_errors_remain_unresolved_after_later_empty_frames() {
        let (engine, mut owner, state, pipeline) = setup();
        // SAFETY: Original owned mock engine/backend, serialized receipt before execution.
        let (_, control) = unsafe { pipeline.register(&mut owner) }.unwrap();
        let address = engine.address();
        engine.invoke(address, ARGUMENT);
        state.lock().unwrap().target_error = true;
        engine.invoke(address, ARGUMENT);
        let report = control.last_report().unwrap();
        assert!(report.target_error.is_some());
        assert_eq!(report.previous_target, Some(target()));
        assert_eq!(report.effects.abandoned.len(), 1);
        assert!(report.cleanup_uncertain);
        assert_eq!(state.lock().unwrap().calls.len(), 1);
        state.lock().unwrap().target_error = false;
        state.lock().unwrap().target = None;
        control.request_cleanup();
        engine.invoke(address, ARGUMENT);
        assert!(control.last_report().unwrap().cleanup_uncertain);
        assert!(!control.last_report().unwrap().cleanup_complete());
        // SAFETY: Fake effects own no resources; explicitly stop/release synthetic task only.
        unsafe { owner.close_step() };
        assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn all_87_consumer_requirements_cross_native_frame_begin_and_cleanup() {
        use shikifsrs_elden_ring::keyboard_effects::keyboard_ex_bindings;
        let engine = Arc::new(Engine::default());
        engine.frame_begin.store(true, Ordering::SeqCst);
        let state = Arc::new(Mutex::new(State {
            focused: true,
            target: Some(target()),
            keys: Some(Default::default()),
            ..State::default()
        }));
        let bindings = keyboard_ex_bindings().unwrap();
        let requirements: Vec<(i32, &str, u16)> = include_str!("fixtures/keyboard-ex-bindings.csv")
            .lines()
            .filter(|s| !s.starts_with('#'))
            .map(|row| {
                let x: Vec<_> = row.split(',').collect();
                (x[0].parse().unwrap(), x[1], x[2].parse().unwrap())
            })
            .collect();
        assert_eq!(requirements.len(), 87);
        assert_eq!(bindings.len(), 87);
        // SAFETY: Full original synthetic native engine and logged fake effects only.
        let pipeline =
            unsafe { InputEffectTask::new(Backend(state.clone()), 42, bindings) }.unwrap();
        let mut owner = NativeTaskOwner::new(engine.clone());
        // SAFETY: Serialized original task registry, no game code/storage or user input.
        let (_, control) = unsafe { pipeline.register(&mut owner) }.unwrap();
        let address = engine.address();
        for (effect, kind, value) in &requirements {
            {
                let mut s = state.lock().unwrap();
                s.calls.clear();
                s.keys = Some(Default::default());
                s.pad = None;
                match *kind {
                    "key" => {
                        s.keys.as_mut().unwrap().insert(*value as u8);
                    }
                    "button" => {
                        s.pad = Some(GamepadState {
                            buttons: *value,
                            ..GamepadState::default()
                        })
                    }
                    "left" => {
                        s.pad = Some(GamepadState {
                            left_trigger: 31,
                            right_trigger: 30,
                            ..GamepadState::default()
                        })
                    }
                    "right" => {
                        s.pad = Some(GamepadState {
                            left_trigger: 30,
                            right_trigger: 31,
                            ..GamepadState::default()
                        })
                    }
                    _ => panic!("invalid requirement"),
                }
            }
            engine.invoke(address, ARGUMENT);
            engine.invoke(address, ARGUMENT);
            assert_eq!(
                state.lock().unwrap().calls,
                vec![(true, *effect), (true, *effect)],
                "{kind}:{value}"
            );
            {
                let mut s = state.lock().unwrap();
                s.keys.as_mut().unwrap().clear();
                s.pad = Some(GamepadState {
                    left_trigger: 30,
                    right_trigger: 30,
                    ..GamepadState::default()
                });
            }
            engine.invoke(address, ARGUMENT);
            engine.invoke(address, ARGUMENT);
            assert_eq!(
                state.lock().unwrap().calls,
                vec![(true, *effect), (true, *effect), (false, *effect)],
                "single release {kind}:{value}"
            );
        }
        {
            let mut s = state.lock().unwrap();
            s.calls.clear();
            s.keys = Some(
                requirements
                    .iter()
                    .filter(|x| x.1 == "key")
                    .map(|x| x.2 as u8)
                    .collect(),
            );
            s.pad = Some(GamepadState {
                buttons: 0xf3ff,
                left_trigger: 255,
                right_trigger: 255,
                ..GamepadState::default()
            });
        }
        engine.invoke(address, ARGUMENT);
        assert_eq!(state.lock().unwrap().calls.len(), 87);
        state.lock().unwrap().focused = false;
        engine.invoke(address, ARGUMENT);
        let calls = state.lock().unwrap().calls.clone();
        assert_eq!(calls.len(), 174);
        assert!(calls[..87].iter().all(|x| x.0));
        assert!(calls[87..].iter().all(|x| !x.0));
        let required: std::collections::BTreeSet<_> = requirements.iter().map(|x| x.0).collect();
        assert_eq!(
            calls[..87]
                .iter()
                .map(|x| x.1)
                .collect::<std::collections::BTreeSet<_>>(),
            required
        );
        assert_eq!(
            calls[87..]
                .iter()
                .map(|x| x.1)
                .collect::<std::collections::BTreeSet<_>>(),
            required
        );
        engine.invoke(address, ARGUMENT);
        assert_eq!(state.lock().unwrap().calls.len(), 174);
        control.request_cleanup();
        engine.invoke(address, ARGUMENT);
        assert!(control.last_report().unwrap().cleanup_complete());
        // SAFETY: All synthetic effects cleared before native owner shutdown.
        assert_eq!(unsafe { owner.close_step() }.remaining_tasks, 0);
        assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
    }
    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn known_epoch_transition_at_same_address_never_removes_old_target_and_keeps_unproved_cleanup_visible()
     {
        use shikifsrs_elden_ring::current_input_effect_task::CurrentInputEffectEpochControl;
        let epochs = CurrentInputEffectEpochControl::new(7).unwrap();
        let (engine, mut owner, state, pipeline) = setup();
        state.lock().unwrap().epochs = Some(epochs.clone());
        // SAFETY: Original serialized task allocator/registry, fake input and logged effects only.
        let (_, control) = unsafe { pipeline.register(&mut owner) }.unwrap();
        let address = engine.address();
        engine.invoke(address, ARGUMENT);
        assert_eq!(state.lock().unwrap().effect_targets, [target()]);
        state.lock().unwrap().down = false;
        // SAFETY: Known original fixture generation change between serialized calls, no game graph.
        let changed = unsafe { epochs.request_epoch(8) }.unwrap();
        assert!(changed);
        engine.invoke(address, ARGUMENT);
        let report = control.last_report().unwrap();
        assert_eq!(report.previous_target, Some(target()));
        let fresh = CharacterIdentity {
            epoch: 8,
            ..target()
        };
        assert_eq!(report.target, Some(fresh));
        assert_eq!(report.effects.abandoned, [EffectId::new(899000).unwrap()]);
        assert!(report.cleanup_uncertain);
        assert_eq!(state.lock().unwrap().effect_targets, [target()]);
        state.lock().unwrap().down = true;
        engine.invoke(address, ARGUMENT);
        control.request_cleanup();
        engine.invoke(address, ARGUMENT);
        let report = control.last_report().unwrap();
        assert_eq!(
            state.lock().unwrap().effect_targets,
            [target(), fresh, fresh]
        );
        assert_eq!(
            state.lock().unwrap().calls,
            [(true, 899000), (true, 899000), (false, 899000)]
        );
        assert!(report.effects.cleanup_pending.is_empty());
        assert!(report.cleanup_uncertain && !report.cleanup_complete());
        // SAFETY: Logged fake effects need no game cleanup; original mock engine retains no effect
        // references. This does not treat the uncertain pipeline report as game-retirement permission.
        let closed = unsafe { owner.close_step() };
        assert_eq!(closed.remaining_tasks, 0);
        assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    mod resident_input_tests {
        // Reuse the original task heap and fake input/effect backend, without a second test framework.
        use super::*;
        use shikifsrs_elden_ring::input_effect_resident::{
            ResidentInputEffectSlot, ResidentInputEffectState,
        };
        use shikifsrs_elden_ring::input_effect_task::InputEffectTask;

        fn pipeline(state: Arc<Mutex<State>>) -> InputEffectTask {
            use shikifsrs_elden_ring::effects::{EffectId, InputEffect};
            use shikifsrs_runtime::input::{Input, VirtualKey};
            // SAFETY: Original fake input and logged effects only; original synthetic task engine.
            unsafe {
                InputEffectTask::new(
                    Backend(state),
                    42,
                    [InputEffect {
                        input: Input::Key(VirtualKey::new(0x41).unwrap()),
                        effect: EffectId::new(700).unwrap(),
                    }],
                )
            }
            .unwrap()
        }
        #[test]
        fn resident_input_retains_direct_and_pending_tasks_after_controls_drop_and_cleanup_does_not_unregister()
         {
            static DIRECT: ResidentInputEffectSlot = ResidentInputEffectSlot::new();
            static PENDING: ResidentInputEffectSlot = ResidentInputEffectSlot::new();
            for (slot, pending) in [(&DIRECT, false), (&PENDING, true)] {
                let engine = Arc::new(Engine::default());
                engine.frame_begin.store(true, Ordering::SeqCst);
                engine.queue_registration.store(pending, Ordering::SeqCst);
                let state = Arc::new(Mutex::new(State {
                    down: true,
                    focused: true,
                    target: Some(target()),
                    ..State::default()
                }));
                // SAFETY: Original serialized allocator/registry and logged effects. The Windows pin
                // targets this short-lived test executable, never a game DLL or external game process.
                let receipt =
                    unsafe { slot.install_task(engine.clone(), pipeline(state.clone())) }.unwrap();
                assert_ne!(receipt.module_handle, 0);
                let control = receipt.registration.unwrap();
                assert_eq!(slot.state(), ResidentInputEffectState::Registered);
                assert_eq!(
                    control.task_status().registration,
                    if pending {
                        RegistrationState::Pending
                    } else {
                        RegistrationState::Inserted
                    }
                );
                assert_eq!(state.lock().unwrap().polls, 0);
                // SAFETY: Same synthetic phase; duplicate rejection must precede allocation/registration.
                assert!(
                    // SAFETY: The original engine remains serialized; duplicate refusal precedes native allocation.
                    unsafe { slot.install_task(engine.clone(), pipeline(state.clone())) }.is_err()
                );
                assert_eq!(engine.allocations.lock().unwrap().len(), 1);
                drop(control);
                if pending {
                    engine.insert_pending();
                }
                let address = engine.address();
                engine.invoke(address, ARGUMENT);
                assert_eq!(state.lock().unwrap().calls, [(true, 700)]);
                let control = slot.control().unwrap();
                assert_eq!(
                    control.task_status().registration,
                    RegistrationState::Inserted
                );
                assert!(!control.task_status().callback.cancelled);
                state.lock().unwrap().fail_remove = true;
                let polls = state.lock().unwrap().polls;
                control.request_cleanup();
                engine.invoke(address, ARGUMENT);
                assert!(!control.last_report().unwrap().cleanup_complete());
                engine.invoke(address, ARGUMENT);
                assert!(control.last_report().unwrap().cleanup_complete());
                assert_eq!(state.lock().unwrap().polls, polls);
                assert_eq!(
                    state.lock().unwrap().calls,
                    [(true, 700), (false, 700), (false, 700)]
                );
                assert_eq!(engine.unregister_calls.load(Ordering::SeqCst), 0);
                drop(control);
                engine.invoke(address, ARGUMENT);
                assert_eq!(state.lock().unwrap().calls.len(), 3);
                engine.destroy(address, 0);
                assert!(slot.control().unwrap().task_status().destructor_completed);
                assert!(engine.allocations.lock().unwrap().is_empty());
                assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
            }
        }
        #[test]
        fn resident_input_unknown_file_releases_preparation_claim_but_actual_pin_and_failed_allocation_latch()
         {
            static SLOT: ResidentInputEffectSlot = ResidentInputEffectSlot::new();
            // SAFETY: Unsupported bytes fail file identity before any read at base zero or native call.
            let unsupported = unsafe {
                SLOT.install_current_keyboard(
                    Arc::from(b"unsupported executable".as_slice()),
                    0,
                    7,
                    1024,
                    42,
                    None,
                )
            };
            assert!(matches!(unsupported, Err(Error::UnsupportedImage { .. })));
            assert_eq!(SLOT.state(), ResidentInputEffectState::Vacant);
            let engine = Arc::new(Engine::default());
            engine.frame_begin.store(true, Ordering::SeqCst);
            engine.fail_allocation.store(true, Ordering::SeqCst);
            let state = Arc::new(Mutex::new(State::default()));
            // SAFETY: Original synthetic allocation failure, no game data/input. Pin only test executable.
            let receipt =
                unsafe { SLOT.install_task(engine.clone(), pipeline(state.clone())) }.unwrap();
            assert_ne!(receipt.module_handle, 0);
            assert!(matches!(
                receipt.registration,
                Err(Error::NativeAllocationFailed)
            ));
            assert_eq!(SLOT.state(), ResidentInputEffectState::RegistrationFailed);
            assert!(SLOT.control().is_none());
            assert_eq!(state.lock().unwrap().polls, 0);
            assert!(engine.allocations.lock().unwrap().is_empty());
            engine.fail_allocation.store(false, Ordering::SeqCst);
            // SAFETY: Explicit repeat must be refused before the original fake allocator is reached.
            assert!(unsafe { SLOT.install_task(engine.clone(), pipeline(state)) }.is_err());
            assert!(engine.allocations.lock().unwrap().is_empty());
            assert!(engine.registered.lock().unwrap().is_empty());
            assert_eq!(engine.unregister_calls.load(Ordering::SeqCst), 0);
        }
    }
}

mod bank_host_tests {
    use super::*;
    use shikifsrs_elden_ring::{
        sound_bank_schedule::{BankAssetSource, SoundBankScheduleConfig},
        sound_bank_tasks::{SoundBankTaskInputs, SoundBankTaskState, SoundBankTasks},
        sound_banks::{BankLoad, BankLoadUncertainty, BankUnload, SoundBankEngine},
        task_owner::NativeTaskOwner,
    };
    use std::{collections::BTreeSet, rc::Rc};
    #[derive(Default)]
    struct State {
        now: u64,
        targets: Option<BTreeSet<u32>>,
        logs: Vec<String>,
        factories: usize,
        uncertain: bool,
        retry_unload: bool,
        dropped: usize,
    }
    struct Audio {
        state: Arc<Mutex<State>>,
        _thread: Rc<()>,
    }
    // SAFETY: Original Rust-only log/return engine, no foreign calls; each load
    // records exactly one mock reference or an explicit retained uncertainty.
    unsafe impl SoundBankEngine for Audio {
        unsafe fn load_memory_copy(&mut self, _bytes: &[u8]) -> Result<BankLoad> {
            let mut s = self.state.lock().unwrap();
            s.logs.push("load".into());
            Ok(if s.uncertain {
                BankLoad::Uncertain(BankLoadUncertainty {
                    native_result: 31,
                    bank_id_output: 77,
                    postflight: Ok(()),
                })
            } else {
                BankLoad::Acquired(77)
            })
        }
        unsafe fn unload(&mut self, id: u32) -> Result<BankUnload> {
            let mut s = self.state.lock().unwrap();
            s.logs.push(format!("unload:{id}"));
            Ok(if std::mem::take(&mut s.retry_unload) {
                BankUnload::RetryLater
            } else {
                BankUnload::Released
            })
        }
    }
    impl Drop for Audio {
        fn drop(&mut self) {
            self.state.lock().unwrap().dropped += 1;
        }
    }
    struct Assets(Arc<Mutex<State>>);
    impl BankAssetSource for Assets {
        fn read_bank(&mut self, id: u32) -> Result<Vec<u8>> {
            self.0.lock().unwrap().logs.push(format!("read:{id}"));
            Ok(vec![1])
        }
    }
    fn config() -> SoundBankScheduleConfig {
        SoundBankScheduleConfig {
            static_ids: vec![1],
            conditional_enabled: true,
            poll_every_frames: 1,
            start_delay_ms: 5,
            retry_every_frames: 1,
            ..Default::default()
        }
    }
    fn create(state: Arc<Mutex<State>>) -> SoundBankTasks {
        // SAFETY: Synthetic callbacks run on a stable thread; wrong-thread test
        // verifies rejection before any session read. Mock native graph only.
        unsafe {
            SoundBankTasks::new(
                move || {
                    state.lock().unwrap().factories += 1;
                    let clock = state.clone();
                    let observer = state.clone();
                    Ok(SoundBankTaskInputs {
                        engine: Audio {
                            state: state.clone(),
                            _thread: Rc::new(()),
                        },
                        source: Assets(state),
                        observe: move || {
                            let mut s = observer.lock().unwrap();
                            s.logs.push("observe".into());
                            Ok(s.targets.clone())
                        },
                        clock: move || {
                            let mut s = clock.lock().unwrap();
                            s.logs.push("clock".into());
                            Ok(s.now)
                        },
                    })
                },
                config(),
                0,
                16,
            )
        }
        .unwrap()
    }
    fn phase(engine: &Engine, wanted: TaskPhase) -> usize {
        *engine
            .bank_phases
            .lock()
            .unwrap()
            .iter()
            .find(|(_, p)| **p == wanted)
            .unwrap()
            .0
    }
    #[cfg(all(windows, target_arch = "x86_64"))]
    mod resident {
        use super::*;
        use shikifsrs_elden_ring::{
            current_sound_conditions::SoundConditionReadConfig,
            sound_bank_resident::{
                ResidentSoundBankConfig, ResidentSoundBankSlot, ResidentSoundBankState,
            },
        };
        #[test]
        fn resident_pair_preserves_shared_owner_after_all_controls_drop_and_closes_on_original_thread()
         {
            static DIRECT: ResidentSoundBankSlot = ResidentSoundBankSlot::new();
            static QUEUED: ResidentSoundBankSlot = ResidentSoundBankSlot::new();
            for (slot, queued) in [(&DIRECT, false), (&QUEUED, true)] {
                let engine = Arc::new(Engine::default());
                engine.bank_tasks.store(true, Ordering::SeqCst);
                engine.queue_registration.store(queued, Ordering::SeqCst);
                let state = Arc::new(Mutex::new(State {
                    targets: Some(BTreeSet::from([2])),
                    ..Default::default()
                }));
                // SAFETY: Original synthetic task/audio engines; Windows pin only this test executable.
                let receipt =
                    unsafe { slot.install_tasks(engine.clone(), create(state.clone())) }.unwrap();
                assert_ne!(receipt.module_handle, 0);
                let control = receipt.registration.unwrap();
                assert!(control.registered_all());
                assert_eq!(slot.state(), ResidentSoundBankState::Registered);
                assert_eq!(
                    control.startup_status().unwrap().registration,
                    if queued {
                        RegistrationState::Pending
                    } else {
                        RegistrationState::Inserted
                    }
                );
                assert_eq!(state.lock().unwrap().factories, 0);
                if queued {
                    engine.insert_pending();
                }
                drop(control);
                let begin = phase(&engine, TaskPhase::FRAME_BEGIN);
                let sound = phase(&engine, TaskPhase::SOUND_STEP);
                engine.invoke(begin, ARGUMENT);
                engine.invoke(sound, ARGUMENT);
                assert_eq!(state.lock().unwrap().factories, 1);
                assert_eq!(
                    state
                        .lock()
                        .unwrap()
                        .logs
                        .iter()
                        .filter(|x| *x == "load")
                        .count(),
                    1
                );
                state.lock().unwrap().now = 5;
                engine.invoke(sound, ARGUMENT);
                assert_eq!(
                    state
                        .lock()
                        .unwrap()
                        .logs
                        .iter()
                        .filter(|x| *x == "load")
                        .count(),
                    2
                );
                let retained = slot.control().unwrap();
                assert_eq!(
                    retained
                        .last_report()
                        .unwrap()
                        .schedule
                        .unwrap()
                        .observations[0]
                        .owned_references,
                    2
                );
                let calls = state.lock().unwrap().logs.len();
                let other = engine.clone();
                std::thread::spawn(move || other.invoke(begin, ARGUMENT))
                    .join()
                    .unwrap();
                assert!(retained.last_report().unwrap().error.is_some());
                assert_eq!(state.lock().unwrap().logs.len(), calls);
                retained.request_stop();
                drop(retained);
                state.lock().unwrap().retry_unload = true;
                engine.invoke(sound, ARGUMENT);
                assert_eq!(
                    slot.control().unwrap().last_report().unwrap().state,
                    SoundBankTaskState::Closing
                );
                engine.invoke(begin, ARGUMENT);
                assert_eq!(
                    slot.control().unwrap().last_report().unwrap().state,
                    SoundBankTaskState::Closed
                );
                assert_eq!(state.lock().unwrap().dropped, 1);
                let calls = state.lock().unwrap().logs.len();
                engine.invoke(sound, ARGUMENT);
                engine.invoke(begin, ARGUMENT);
                assert_eq!(state.lock().unwrap().logs.len(), calls);
                assert_eq!(engine.unregister_calls.load(Ordering::SeqCst), 0);
                // Synthetic engine retires its own native-shaped allocations; host does not unregister.
                engine.destroy(sound, 0);
                engine.destroy(begin, 0);
                assert!(engine.allocations.lock().unwrap().is_empty());
                assert!(
                    slot.control()
                        .unwrap()
                        .startup_status()
                        .unwrap()
                        .destructor_completed
                );
                assert!(
                    slot.control()
                        .unwrap()
                        .conditional_status()
                        .unwrap()
                        .unwrap()
                        .destructor_completed
                );
            }
        }
        #[test]
        fn resident_pair_retains_first_after_second_failure_and_never_reopens_failed_claim() {
            static PARTIAL: ResidentSoundBankSlot = ResidentSoundBankSlot::new();
            static FIRST_FAILED: ResidentSoundBankSlot = ResidentSoundBankSlot::new();
            for (slot, first_failed) in [(&PARTIAL, false), (&FIRST_FAILED, true)] {
                let engine = Arc::new(Engine::default());
                engine.bank_tasks.store(true, Ordering::SeqCst);
                engine.fail_allocation.store(first_failed, Ordering::SeqCst);
                engine
                    .reject_bank_conditional
                    .store(!first_failed, Ordering::SeqCst);
                let state = Arc::new(Mutex::new(State::default()));
                // SAFETY: Only original mock allocation/registration failures; no game/native audio.
                let receipt =
                    unsafe { slot.install_tasks(engine.clone(), create(state.clone())) }.unwrap();
                assert_ne!(receipt.module_handle, 0);
                let control = receipt.registration.unwrap();
                assert!(!control.registered_all());
                assert_eq!(slot.state(), ResidentSoundBankState::RegistrationFailed);
                assert_eq!(control.startup_status().is_err(), first_failed);
                if first_failed {
                    assert!(control.conditional_status().is_none());
                } else {
                    assert!(control.conditional_status().unwrap().is_err());
                    drop(control);
                    let sound = phase(&engine, TaskPhase::SOUND_STEP);
                    engine.invoke(sound, ARGUMENT);
                    assert_eq!(
                        slot.control().unwrap().last_report().unwrap().state,
                        SoundBankTaskState::Closed
                    );
                    assert_eq!(state.lock().unwrap().factories, 0);
                    engine.destroy(sound, 0);
                }
                assert_eq!(state.lock().unwrap().factories, 0);
                assert!(state.lock().unwrap().logs.is_empty());
                assert!(engine.allocations.lock().unwrap().is_empty());
                engine.fail_allocation.store(false, Ordering::SeqCst);
                engine
                    .reject_bank_conditional
                    .store(false, Ordering::SeqCst);
                // SAFETY: Explicit repeat must reject before allocation, including after original callback retires.
                let repeated = unsafe { slot.install_tasks(engine.clone(), create(state.clone())) };
                assert!(repeated.is_err());
                assert!(engine.allocations.lock().unwrap().is_empty());
                assert_eq!(engine.unregister_calls.load(Ordering::SeqCst), 0);
            }
        }
        #[test]
        fn resident_current_unknown_file_and_invalid_config_never_pin_claim_or_read_assets() {
            static SLOT: ResidentSoundBankSlot = ResidentSoundBankSlot::new();
            let state = Arc::new(Mutex::new(State::default()));
            for invalid_epoch in [false, true] {
                let config = ResidentSoundBankConfig {
                    epoch: if invalid_epoch { 0 } else { 9 },
                    schedule: config(),
                    conditions: SoundConditionReadConfig::default(),
                    reference_limit: 16,
                };
                // SAFETY: Unknown bytes/zero epoch reject before any base-zero read or native/game operation.
                let result = unsafe {
                    SLOT.install_current(
                        Arc::from(b"unknown executable".as_slice()),
                        0,
                        config,
                        Assets(state.clone()),
                        None,
                    )
                };
                assert!(if invalid_epoch {
                    matches!(result, Err(Error::InvalidProfile(_)))
                } else {
                    matches!(result, Err(Error::UnsupportedImage { .. }))
                });
                assert_eq!(SLOT.state(), ResidentSoundBankState::Vacant);
                assert!(SLOT.control().is_none());
                assert!(state.lock().unwrap().logs.is_empty());
                assert_eq!(state.lock().unwrap().factories, 0);
            }
        }
    }
    #[test]
    fn two_phases_preserve_non_send_owner_delay_aliases_thread_rejection_and_ordered_close() {
        let engine = Arc::new(Engine::default());
        engine.bank_tasks.store(true, Ordering::SeqCst);
        let state = Arc::new(Mutex::new(State {
            targets: Some(BTreeSet::from([2])),
            ..Default::default()
        }));
        let task = create(state.clone());
        let control = task.control();
        assert_eq!(state.lock().unwrap().factories, 0);
        let mut owner = NativeTaskOwner::new(engine.clone());
        // SAFETY: Synthetic allocator and serialized engine-owned task registration.
        let registration = unsafe { task.register(&mut owner) };
        assert!(registration.startup.is_ok());
        assert!(registration.conditional.unwrap().is_ok());
        assert_eq!(state.lock().unwrap().factories, 0);
        let begin = phase(&engine, TaskPhase::FRAME_BEGIN);
        let sound = phase(&engine, TaskPhase::SOUND_STEP);
        engine.invoke(begin, ARGUMENT); // Conditional demand is independent of startup delay.
        assert_eq!(state.lock().unwrap().factories, 1);
        engine.invoke(sound, ARGUMENT);
        assert_eq!(
            control
                .last_report()
                .unwrap()
                .schedule
                .unwrap()
                .observations[0]
                .owned_references,
            1
        );
        state.lock().unwrap().now = 5;
        engine.invoke(sound, ARGUMENT);
        let bank = control
            .last_report()
            .unwrap()
            .schedule
            .unwrap()
            .observations[0];
        assert_eq!((bank.requests, bank.owned_references), (2, 2));
        let calls = state.lock().unwrap().logs.len();
        let other = engine.clone();
        std::thread::spawn(move || other.invoke(begin, ARGUMENT))
            .join()
            .unwrap();
        assert!(control.last_report().unwrap().error.is_some());
        assert_eq!(state.lock().unwrap().logs.len(), calls);
        state.lock().unwrap().targets = None;
        engine.invoke(begin, ARGUMENT);
        assert_eq!(
            control
                .last_report()
                .unwrap()
                .schedule
                .unwrap()
                .observations[0]
                .requests,
            2
        );
        control.request_stop();
        state.lock().unwrap().retry_unload = true;
        engine.invoke(sound, ARGUMENT);
        assert_eq!(
            control.last_report().unwrap().state,
            SoundBankTaskState::Closing
        );
        assert!(
            control
                .last_report()
                .unwrap()
                .close
                .unwrap()
                .remaining_requests
                > 0
        );
        engine.invoke(begin, ARGUMENT);
        assert_eq!(
            control.last_report().unwrap().state,
            SoundBankTaskState::Closed
        );
        assert_eq!(state.lock().unwrap().dropped, 1);
        let calls = state.lock().unwrap().logs.len();
        engine.invoke(sound, ARGUMENT);
        assert_eq!(state.lock().unwrap().logs.len(), calls);
        // SAFETY: Bank close complete, then callbacks cease and mock heap is destroyed.
        assert_eq!(unsafe { owner.close_step() }.remaining_tasks, 0);
        assert!(engine.allocations.lock().unwrap().is_empty());
        assert_eq!(
            state
                .lock()
                .unwrap()
                .logs
                .iter()
                .filter(|x| *x == "load")
                .count(),
            2
        );
    }
    #[test]
    fn partial_registration_and_stop_before_first_frame_never_create_bank_owner() {
        for rejected in [false, true] {
            let engine = Arc::new(Engine::default());
            engine.bank_tasks.store(true, Ordering::SeqCst);
            engine
                .reject_bank_conditional
                .store(rejected, Ordering::SeqCst);
            let state = Arc::new(Mutex::new(State::default()));
            let task = create(state.clone());
            let control = task.control();
            let mut owner = NativeTaskOwner::new(engine.clone());
            // SAFETY: Original mock task allocation, including one pre-invocation rejection.
            let registration = unsafe { task.register(&mut owner) };
            assert!(registration.startup.is_ok());
            assert_eq!(registration.conditional.unwrap().is_err(), rejected);
            control.request_stop();
            engine.invoke(phase(&engine, TaskPhase::SOUND_STEP), ARGUMENT);
            assert_eq!(
                control.last_report().unwrap().state,
                SoundBankTaskState::Closed
            );
            assert_eq!(state.lock().unwrap().factories, 0);
            assert!(state.lock().unwrap().logs.is_empty());
            // SAFETY: Factory never ran; only native mock callback allocation needs teardown.
            assert_eq!(unsafe { owner.close_step() }.remaining_tasks, 0);
            assert!(engine.allocations.lock().unwrap().is_empty());
        }
    }
    #[test]
    fn uncertain_native_submission_retains_owner_after_callback_destruction_without_retry_or_unload()
     {
        let engine = Arc::new(Engine::default());
        engine.bank_tasks.store(true, Ordering::SeqCst);
        let state = Arc::new(Mutex::new(State {
            now: 5,
            uncertain: true,
            targets: Some(BTreeSet::from([2])),
            ..Default::default()
        }));
        let task = create(state.clone());
        let control = task.control();
        let mut owner = NativeTaskOwner::new(engine.clone());
        // SAFETY: Serialized mock task engine and explicitly classified uncertain bank outcome.
        let registration = unsafe { task.register(&mut owner) };
        assert!(registration.startup.is_ok());
        let sound = phase(&engine, TaskPhase::SOUND_STEP);
        let begin = phase(&engine, TaskPhase::FRAME_BEGIN);
        engine.invoke(sound, ARGUMENT);
        engine.invoke(begin, ARGUMENT);
        engine.invoke(sound, ARGUMENT);
        assert!(control.last_report().unwrap().schedule.unwrap().blocked);
        control.request_stop();
        engine.invoke(sound, ARGUMENT);
        let report = control.last_report().unwrap();
        assert_eq!(report.state, SoundBankTaskState::Closing);
        assert_eq!(report.close.unwrap().uncertain_loads.len(), 1);
        let calls = state.lock().unwrap().logs.len();
        // SAFETY: Mock-only teardown deliberately exercises unresolved retained payload.
        assert_eq!(unsafe { owner.close_step() }.remaining_tasks, 0);
        assert_eq!(state.lock().unwrap().logs.len(), calls);
        assert_eq!(state.lock().unwrap().dropped, 0);
        assert_eq!(
            state
                .lock()
                .unwrap()
                .logs
                .iter()
                .filter(|x| *x == "load")
                .count(),
            1
        );
        assert_eq!(
            state
                .lock()
                .unwrap()
                .logs
                .iter()
                .filter(|x| x.starts_with("unload"))
                .count(),
            0
        );
        assert_eq!(
            control
                .last_report()
                .unwrap()
                .close
                .unwrap()
                .remaining_requests,
            1
        );
    }
}

#[cfg(windows)]
#[test]
fn consumer_resident_group_keeps_partial_receipts_callbacks_and_terminal_claim() {
    use shikifsrs_elden_ring::task_resident::{ResidentCallbackGroupSlot, ResidentTaskState};
    static SLOT: ResidentCallbackGroupSlot<2> = ResidentCallbackGroupSlot::new();
    let engine = Arc::new(Engine::default());
    engine.bank_tasks.store(true, Ordering::SeqCst);
    engine.reject_bank_conditional.store(true, Ordering::SeqCst);
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let callbacks = [
        TaskCallback::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
        TaskCallback::new(|_| panic!("unregistered callback")),
    ];
    // SAFETY: Original serial synthetic engine, static retained slot, no native resources.
    let installed = unsafe {
        SLOT.install(
            engine.clone(),
            [TaskPhase::SOUND_STEP, TaskPhase::FRAME_BEGIN],
            callbacks,
        )
    }
    .unwrap();
    assert_ne!(installed.module_handle, 0);
    let control = installed.registration.unwrap();
    assert!(!control.registered_all());
    assert!(control.outcomes()[0].as_ref().unwrap().is_ok());
    assert!(control.outcomes()[1].as_ref().unwrap().is_err());
    let address = engine.address();
    drop(control);
    engine.invoke(address, ARGUMENT);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(SLOT.state(), ResidentTaskState::RegistrationFailed);
    // SAFETY: The same synthetic phase; failed process-resident claims must remain terminal.
    assert!(
        // SAFETY: Same serial synthetic engine; terminal claim rejects before allocation.
        unsafe {
            SLOT.install(
                engine.clone(),
                [TaskPhase::SOUND_STEP, TaskPhase::FRAME_BEGIN],
                [TaskCallback::new(|_| {}), TaskCallback::new(|_| {})],
            )
        }
        .is_err()
    );
    SLOT.control().unwrap().stop_callbacks();
    engine.invoke(address, ARGUMENT);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(engine.unregister_calls.load(Ordering::SeqCst), 0);
    engine.destroy(address, 1);
    assert!(engine.allocations.lock().unwrap().is_empty());
    assert!(
        SLOT.control().unwrap().outcomes()[0]
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .status()
            .destructor_completed
    );
}
#[cfg(windows)]
#[test]
fn consumer_resident_group_rejects_invalid_count_before_pin_and_keeps_unattempted_distinct() {
    use shikifsrs_elden_ring::task_resident::{ResidentCallbackGroupSlot, ResidentTaskState};
    static EMPTY: ResidentCallbackGroupSlot<0> = ResidentCallbackGroupSlot::new();
    static FIRST_FAIL: ResidentCallbackGroupSlot<2> = ResidentCallbackGroupSlot::new();
    let engine = Arc::new(Engine::default());
    // SAFETY: Empty synthetic setup; invalid group cannot allocate or pin.
    assert!(unsafe { EMPTY.install(engine.clone(), [], []) }.is_err());
    assert_eq!(EMPTY.state(), ResidentTaskState::Vacant);
    engine.fail_allocation.store(true, Ordering::SeqCst);
    // SAFETY: Serial original engine fails before retaining a task; static slot owns the receipt.
    let result = unsafe {
        FIRST_FAIL.install(
            engine.clone(),
            [TaskPhase::SOUND_STEP, TaskPhase::FRAME_BEGIN],
            [TaskCallback::new(|_| {}), TaskCallback::new(|_| {})],
        )
    }
    .unwrap()
    .registration
    .unwrap();
    assert!(result.outcomes()[0].as_ref().unwrap().is_err());
    assert!(result.outcomes()[1].is_none());
    assert_eq!(FIRST_FAIL.state(), ResidentTaskState::RegistrationFailed);
    assert!(engine.allocations.lock().unwrap().is_empty());
}
