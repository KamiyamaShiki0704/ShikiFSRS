use super::*;
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Default)]
struct Engine {
    allocations: Mutex<BTreeSet<usize>>,
    inserted: AtomicBool,
    queued: AtomicBool,
    reject: AtomicBool,
    frees: AtomicUsize,
    cancels: AtomicUsize,
    last: AtomicUsize,
    key: AtomicUsize,
}
// SAFETY: Own Rust allocator/layout, single-test-thread native scheduling, explicit fake
// insertion, no call before receipt, and explicit drain/destructor with matching deallocation.
unsafe impl TaskEngine for Engine {
    fn type_info_address(&self) -> usize {
        0x9990
    }
    unsafe fn allocate(&self, layout: Layout) -> Result<NonNull<u8>> {
        // SAFETY: Valid Layout; returned allocation is owned until our explicit destructor/free.
        let memory = NonNull::new(unsafe { std::alloc::alloc(layout) })
            .ok_or(Error::NativeAllocationFailed)?;
        self.allocations
            .lock()
            .unwrap()
            .insert(memory.as_ptr() as usize);
        Ok(memory)
    }
    unsafe fn deallocate(&self, allocation: NonNull<u8>, layout: Layout) -> Result<()> {
        assert!(
            self.allocations
                .lock()
                .unwrap()
                .remove(&(allocation.as_ptr() as usize))
        );
        self.frees.fetch_add(1, Ordering::SeqCst);
        // SAFETY: Tracked allocation from this allocator with exact layout, freed once.
        unsafe {
            std::alloc::dealloc(allocation.as_ptr(), layout);
        }
        Ok(())
    }
    unsafe fn register(
        &self,
        phase: TaskPhase,
        object: NonNull<NativeTaskObject>,
    ) -> Result<RegistrationReceipt> {
        if self.reject.load(Ordering::SeqCst) {
            return Err(Error::NativeOperationRejected("fixture rejected"));
        }
        assert_eq!(phase, TaskPhase::SOUND_STEP);
        self.last.store(object.as_ptr() as usize, Ordering::SeqCst);
        let pending = self.queued.load(Ordering::SeqCst);
        self.inserted.store(!pending, Ordering::SeqCst);
        RegistrationReceipt::new(0xab00005e, pending)
    }
    unsafe fn inserted(&self, id: u32, address: usize) -> Result<bool> {
        assert_eq!(id, 0xab00005e);
        assert_eq!(address, self.last.load(Ordering::SeqCst));
        Ok(self.inserted.load(Ordering::SeqCst))
    }
    unsafe fn unregister(&self, id: u32, address: usize) -> Result<()> {
        assert_eq!(id, 0xab00005e);
        assert_eq!(address, self.last.load(Ordering::SeqCst));
        self.cancels.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn invocation(&self, _address: usize) -> Result<TaskInvocation> {
        Ok(TaskInvocation {
            group_id: self.key.load(Ordering::SeqCst) as u32,
            scalar_08: 1.0,
            word_14: 7,
        })
    }
}
impl Engine {
    fn fire(&self) {
        let object =
            NonNull::new(self.last.load(Ordering::SeqCst) as *mut NativeTaskObject).unwrap();
        // SAFETY: Fixture owns a live SDK task and guarantees no concurrent destruction.
        let table = unsafe { NativeTaskObject::vtable(object) };
        // SAFETY: Same initialized object; fake invocation reader ignores this opaque address.
        unsafe {
            (table.execute)(object.as_ptr(), std::ptr::null());
        }
    }
    fn destroy(&self, flags: u32) {
        let object =
            NonNull::new(self.last.load(Ordering::SeqCst) as *mut NativeTaskObject).unwrap();
        // SAFETY: Tests explicitly drain synchronous fixture callbacks and destroy once.
        let table = unsafe { NativeTaskObject::vtable(object) };
        // SAFETY: Unique destructor; no foreign execution frame remains.
        unsafe {
            (table.destructor)(object.as_ptr(), flags);
        }
        if flags == 0 {
            // SAFETY: Native flags-zero path requires engine-side free after destructor return.
            unsafe {
                self.deallocate(object.cast(), Layout::new::<NativeTaskObject>())
                    .unwrap();
            }
        }
    }
}
struct Capture(Arc<AtomicUsize>);
impl Drop for Capture {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn callback(drops: &Arc<AtomicUsize>, calls: &Arc<AtomicUsize>) -> TaskCallback {
    let capture = Capture(Arc::clone(drops));
    let calls = Arc::clone(calls);
    TaskCallback::new(move |_| {
        let _ = &capture;
        calls.fetch_add(1, Ordering::SeqCst);
    })
}

#[test]
fn rejection_disposes_prepared_payload_and_releases_exact_heap_once() {
    let engine = Arc::new(Engine::default());
    engine.reject.store(true, Ordering::SeqCst);
    let drops = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut owner = TaskOwner::new(engine.clone());
    // SAFETY: Fixture allocator and synchronous serialized registration satisfy host contract.
    assert!(unsafe { owner.register(TaskPhase::SOUND_STEP, callback(&drops, &calls)) }.is_err());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
    assert_eq!(owner.remaining(), 0);
    assert!(engine.allocations.lock().unwrap().is_empty());
}

#[test]
fn pending_cancellation_waits_for_insertion_and_destruction_acknowledgement() {
    let engine = Arc::new(Engine::default());
    engine.queued.store(true, Ordering::SeqCst);
    let drops = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut owner = TaskOwner::new(engine.clone());
    // SAFETY: Own fixture heap and scheduling are serialized until the receipt is recorded.
    let control = unsafe {
        owner
            .register(TaskPhase::SOUND_STEP, callback(&drops, &calls))
            .unwrap()
    };
    assert_eq!(control.status().ownership, Ownership::EnginePending);
    // SAFETY: Fixture remains live and has no concurrent execution or teardown.
    assert_eq!(
        // SAFETY: Fixture remains live with no concurrent execution or teardown.
        unsafe { owner.close_step() },
        [Ok(CancelOutcome::WaitingForInsertion)]
    );
    assert_eq!(engine.cancels.load(Ordering::SeqCst), 0);
    assert_eq!(engine.frees.load(Ordering::SeqCst), 0);
    assert_eq!(drops.load(Ordering::SeqCst), 1); // callback stopped; native object still retained
    engine.inserted.store(true, Ordering::SeqCst);
    // SAFETY: Same explicit serialized fixture phase, now insertion is confirmed.
    assert_eq!(
        // SAFETY: Serialized fixture phase, now insertion is confirmed.
        unsafe { owner.close_step() },
        [Ok(CancelOutcome::Submitted)]
    );
    assert_eq!(owner.remaining(), 1);
    assert_eq!(control.status().ownership, Ownership::EngineInserted);
    // SAFETY: Repeat does not release an accepted cancellation before its destructor.
    assert_eq!(
        // SAFETY: Retain native object until the fixture's explicit destructor.
        unsafe { owner.close_step() },
        [Ok(CancelOutcome::AlreadySubmitted)]
    );
    engine.destroy(0);
    assert_eq!(control.status().ownership, Ownership::Destroyed);
    assert_eq!(
        control.status().heap_release,
        HeapRelease::EngineResponsible
    );
    assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
    // SAFETY: Destroyed control no longer consults native insertion or memory.
    assert_eq!(
        // SAFETY: Destroyed control no longer consults native memory.
        unsafe { owner.close_step() },
        [Ok(CancelOutcome::Destroyed)]
    );
    assert_eq!(owner.remaining(), 0);
}

#[test]
fn callback_group_guard_and_drop_stop_do_not_release_engine_storage() {
    let engine = Arc::new(Engine::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let mut owner = TaskOwner::new(engine.clone());
    // SAFETY: Fixture host contracts cover submission and eventual single destruction.
    let control = unsafe {
        owner
            .register(TaskPhase::SOUND_STEP, callback(&drops, &calls))
            .unwrap()
    };
    engine.key.store(7, Ordering::SeqCst);
    engine.fire();
    assert_eq!(control.status().rejected_invocations, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    engine.key.store(0xab00005e, Ordering::SeqCst);
    engine.fire();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(owner);
    engine.fire();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(engine.frees.load(Ordering::SeqCst), 0);
    assert_eq!(engine.cancels.load(Ordering::SeqCst), 0);
    engine.destroy(1);
    assert_eq!(control.status().heap_release, HeapRelease::Released);
}

#[test]
fn callback_panic_is_contained_and_task_destruction_remains_observable() {
    let engine = Arc::new(Engine::default());
    engine.key.store(0xab00005e, Ordering::SeqCst);
    let mut owner = TaskOwner::new(engine.clone());
    // SAFETY: Own fixture scheduling; closure panic is contained by the shared callback module.
    let control = unsafe {
        owner
            .register(
                TaskPhase::SOUND_STEP,
                TaskCallback::new(|_| panic!("fixture panic")),
            )
            .unwrap()
    };
    engine.fire();
    assert!(control.status().callback.panicked);
    engine.destroy(1);
    assert_eq!(control.status().ownership, Ownership::Destroyed);
    assert_eq!(engine.frees.load(Ordering::SeqCst), 1);
}

#[test]
fn phase_and_receipt_bounds_are_version_specific() {
    assert!(TaskPhase::new(156).is_err());
    assert_eq!(TaskPhase::SOUND_STEP.index(), 94);
    assert!(RegistrationReceipt::new(0, true).is_err());
}
