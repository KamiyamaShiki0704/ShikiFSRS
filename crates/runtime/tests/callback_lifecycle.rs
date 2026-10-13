use shikifsrs_runtime::callback::{Callback, CallbackControl, InvocationOutcome, WaitOutcome};
use std::{
    sync::{
        Arc, Barrier, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

const DEADLINE: Duration = Duration::from_secs(5);

struct DropAction(Option<Box<dyn FnOnce() + Send>>);

impl Drop for DropAction {
    fn drop(&mut self) {
        self.0.take().unwrap()();
    }
}

fn counted_drop(count: &Arc<AtomicUsize>) -> DropAction {
    let count = Arc::clone(count);
    DropAction(Some(Box::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    })))
}

#[test]
fn concurrent_invocations_preserve_every_input_and_serialize_mutable_state() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&values);
    let mut sequence = 0;
    let callback = Arc::new(Callback::new(move |input: usize| {
        sequence += 1;
        captured.lock().unwrap().push((sequence, input));
        thread::yield_now();
    }));
    let control = callback.control();
    let barrier = Arc::new(Barrier::new(5));
    let workers: Vec<_> = (0..4)
        .map(|worker| {
            let callback = Arc::clone(&callback);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for input in worker * 100..(worker + 1) * 100 {
                    assert_eq!(callback.invoke(input), InvocationOutcome::Executed);
                }
            })
        })
        .collect();
    barrier.wait();
    for worker in workers {
        worker.join().unwrap();
    }
    let values = values.lock().unwrap();
    assert_eq!(values.len(), 400);
    assert!(values.iter().enumerate().all(|(i, pair)| pair.0 == i + 1));
    let mut inputs: Vec<_> = values.iter().map(|pair| pair.1).collect();
    inputs.sort_unstable();
    assert_eq!(inputs, (0..400).collect::<Vec<_>>());
    assert_eq!(control.status().completed_calls, 400);
    assert!(!control.status().resources_released);
}

#[test]
fn cancellation_during_execution_defers_capture_drop_and_rejects_new_calls() {
    let drops = Arc::new(AtomicUsize::new(0));
    let capture = counted_drop(&drops);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (finish_tx, finish_rx) = mpsc::channel();
    let callback = Arc::new(Callback::new(move |()| {
        let _keep = &capture;
        entered_tx.send(()).unwrap();
        finish_rx.recv_timeout(DEADLINE).unwrap();
    }));
    let control = callback.control();
    let invoking = Arc::clone(&callback);
    let worker = thread::spawn(move || invoking.invoke(()));
    entered_rx.recv_timeout(DEADLINE).unwrap();
    control.cancel();
    control.cancel();
    assert!(control.status().cancelled);
    assert!(control.status().executing);
    assert!(!control.status().resources_released);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(callback.invoke(()), InvocationOutcome::Stopped);
    assert_eq!(control.wait_stopped(Duration::ZERO), WaitOutcome::TimedOut);
    finish_tx.send(()).unwrap();
    assert_eq!(worker.join().unwrap(), InvocationOutcome::Executed);
    assert_eq!(
        control.wait_stopped(DEADLINE),
        WaitOutcome::ResourcesReleased
    );
    assert_eq!(control.status().completed_calls, 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    drop(callback);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn cancel_inside_handler_never_locks_user_code_or_waits_on_itself() {
    let slot: Arc<Mutex<Option<CallbackControl<()>>>> = Arc::new(Mutex::new(None));
    let captured = Arc::clone(&slot);
    let callback = Callback::new(move |()| {
        let control = captured.lock().unwrap().as_ref().unwrap().clone();
        control.cancel();
        assert!(control.status().executing);
        assert_eq!(
            control.wait_stopped(DEADLINE),
            WaitOutcome::WouldBlockCurrentCallback
        );
    });
    let control = callback.control();
    *slot.lock().unwrap() = Some(control.clone());
    assert_eq!(callback.invoke(()), InvocationOutcome::Executed);
    assert_eq!(
        control.wait_stopped(Duration::ZERO),
        WaitOutcome::ResourcesReleased
    );
    assert_eq!(callback.invoke(()), InvocationOutcome::Stopped);
}

#[test]
fn dropping_owner_releases_captures_even_if_they_capture_a_control() {
    let drops = Arc::new(AtomicUsize::new(0));
    let capture = counted_drop(&drops);
    let slot: Arc<Mutex<Option<CallbackControl<()>>>> = Arc::new(Mutex::new(None));
    let weak_slot = Arc::downgrade(&slot);
    let captured = Arc::clone(&slot);
    let callback = Callback::new(move |()| {
        let _keep = (&capture, &captured);
    });
    let control = callback.control();
    *slot.lock().unwrap() = Some(control.clone());
    drop(slot);
    assert!(weak_slot.upgrade().is_some());
    drop(callback);
    assert!(weak_slot.upgrade().is_none());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(control.status().owner_released);
    assert_eq!(
        control.wait_stopped(DEADLINE),
        WaitOutcome::ResourcesReleased
    );
}

#[test]
fn reentrant_invocation_is_rejected_without_dropping_outer_handler() {
    let nested = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&nested);
    let callback = Arc::new_cyclic(|weak: &Weak<Callback<()>>| {
        let weak = weak.clone();
        Callback::new(move |()| {
            let result = weak.upgrade().unwrap().invoke(());
            captured.lock().unwrap().push(result);
        })
    });
    assert_eq!(callback.invoke(()), InvocationOutcome::Executed);
    assert_eq!(callback.invoke(()), InvocationOutcome::Executed);
    assert_eq!(
        *nested.lock().unwrap(),
        vec![InvocationOutcome::Reentrant, InvocationOutcome::Reentrant]
    );
    assert_eq!(callback.control().status().completed_calls, 2);
}

#[test]
fn handler_panic_stops_future_calls_and_destroys_captures_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    let capture = counted_drop(&drops);
    let callback = Callback::new(move |()| {
        let _keep = &capture;
        panic!("synthetic callback panic");
    });
    let control = callback.control();
    assert_eq!(callback.invoke(()), InvocationOutcome::Panicked);
    assert!(control.status().panicked);
    assert_eq!(control.status().completed_calls, 0);
    assert_eq!(callback.invoke(()), InvocationOutcome::Stopped);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(
        control.wait_stopped(DEADLINE),
        WaitOutcome::ResourcesReleased
    );
}

#[test]
fn panic_payload_is_not_dropped_on_the_containment_return_path() {
    struct Payload(Arc<AtomicUsize>);
    impl Drop for Payload {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic!("synthetic panic payload destructor");
        }
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let captured = Arc::clone(&drops);
    let callback = Callback::new(move |()| {
        std::panic::panic_any(Payload(Arc::clone(&captured)));
    });
    assert_eq!(callback.invoke(()), InvocationOutcome::Panicked);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert!(callback.control().status().resources_released);
}

#[test]
fn idle_capture_destructor_can_inspect_cancel_and_detect_self_wait() {
    let slot: Arc<Mutex<Option<CallbackControl<()>>>> = Arc::new(Mutex::new(None));
    let captured = Arc::clone(&slot);
    let (result_tx, result_rx) = mpsc::channel();
    let capture = DropAction(Some(Box::new(move || {
        let control = captured.lock().unwrap().as_ref().unwrap().clone();
        control.cancel();
        assert!(control.status().disposing);
        assert!(!control.status().resources_released);
        result_tx.send(control.wait_stopped(DEADLINE)).unwrap();
    })));
    let callback = Callback::new(move |()| {
        let _keep = &capture;
    });
    let control = callback.control();
    *slot.lock().unwrap() = Some(control.clone());
    control.cancel();
    assert_eq!(
        result_rx.recv_timeout(DEADLINE).unwrap(),
        WaitOutcome::WouldBlockCurrentCallback
    );
    assert!(control.status().resources_released);
}

#[test]
fn idle_capture_panic_is_contained_and_release_still_completes() {
    let capture = DropAction(Some(Box::new(|| panic!("synthetic capture destructor"))));
    let callback = Callback::new(move |()| {
        let _keep = &capture;
    });
    let control = callback.control();
    control.cancel();
    assert!(control.status().panicked);
    assert!(control.status().cancelled);
    assert!(control.status().resources_released);
    assert_eq!(callback.invoke(()), InvocationOutcome::Stopped);
    assert_eq!(
        control.wait_stopped(DEADLINE),
        WaitOutcome::ResourcesReleased
    );
}

#[test]
fn waiting_control_wakes_only_after_capture_destruction_finishes() {
    let (dropping_tx, dropping_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let capture = DropAction(Some(Box::new(move || {
        dropping_tx.send(()).unwrap();
        release_rx.recv_timeout(DEADLINE).unwrap();
    })));
    let callback = Callback::new(move |()| {
        let _keep = &capture;
    });
    let control = callback.control();
    let cancelling = control.clone();
    let worker = thread::spawn(move || cancelling.cancel());
    dropping_rx.recv_timeout(DEADLINE).unwrap();
    assert!(control.status().disposing);
    assert_eq!(control.wait_stopped(Duration::ZERO), WaitOutcome::TimedOut);
    let waiter = control.clone();
    let (waiting_tx, waiting_rx) = mpsc::channel();
    let waiting = thread::spawn(move || {
        waiting_tx.send(()).unwrap();
        waiter.wait_stopped(DEADLINE)
    });
    waiting_rx.recv_timeout(DEADLINE).unwrap();
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(waiting.join().unwrap(), WaitOutcome::ResourcesReleased);
}

#[test]
fn idle_uncancelled_callback_times_out_without_discarding_it() {
    let callback = Callback::new(|_: ()| {});
    let control = callback.control();
    assert_eq!(control.wait_stopped(Duration::ZERO), WaitOutcome::TimedOut);
    assert_eq!(callback.invoke(()), InvocationOutcome::Executed);
    assert!(!control.status().cancelled);
}

#[test]
fn control_can_be_cloned_without_requiring_cloneable_inputs() {
    struct Input(usize);
    let value = Arc::new(AtomicUsize::new(0));
    let captured = Arc::clone(&value);
    let callback = Callback::new(move |input: Input| {
        captured.store(input.0, Ordering::SeqCst);
    });
    let control = callback.control().clone();
    assert_eq!(callback.invoke(Input(17)), InvocationOutcome::Executed);
    assert_eq!(value.load(Ordering::SeqCst), 17);
    control.cancel();
    assert_eq!(callback.invoke(Input(18)), InvocationOutcome::Stopped);
    assert_eq!(value.load(Ordering::SeqCst), 17);
}

#[test]
fn cancellation_releases_a_waiting_invoker_before_the_active_handler_finishes() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (finish_tx, finish_rx) = mpsc::channel();
    let callback = Arc::new(Callback::new(move |_: ()| {
        entered_tx.send(()).unwrap();
        finish_rx.recv_timeout(DEADLINE).unwrap();
    }));
    let active = Arc::clone(&callback);
    let worker = thread::spawn(move || active.invoke(()));
    entered_rx.recv_timeout(DEADLINE).unwrap();
    let pending = Arc::clone(&callback);
    let (starting_tx, starting_rx) = mpsc::channel();
    let (stopped_tx, stopped_rx) = mpsc::channel();
    let waiting = thread::spawn(move || {
        starting_tx.send(()).unwrap();
        stopped_tx.send(pending.invoke(())).unwrap();
    });
    starting_rx.recv_timeout(DEADLINE).unwrap();
    callback.control().cancel();
    assert_eq!(
        stopped_rx.recv_timeout(DEADLINE).unwrap(),
        InvocationOutcome::Stopped
    );
    assert!(callback.control().status().executing);
    assert_eq!(callback.control().status().completed_calls, 0);
    finish_tx.send(()).unwrap();
    assert_eq!(worker.join().unwrap(), InvocationOutcome::Executed);
    waiting.join().unwrap();
    assert_eq!(callback.control().status().completed_calls, 1);
}
