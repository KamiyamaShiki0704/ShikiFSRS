//! Serial, cancellable Rust callbacks for a future native task adapter.
//!
//! This module never registers or frees a native task. `resources_released` only
//! acknowledges destruction of the Rust closure and its captures. An engine
//! adapter must separately prove that no native invocation can reach its object.
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Condvar, Mutex, MutexGuard},
    thread::{self, ThreadId},
    time::{Duration, Instant},
};

type Handler<I> = Box<dyn FnMut(I) + Send + 'static>;

struct State<I> {
    handler: Option<Handler<I>>,
    executing: Option<ThreadId>,
    disposing: Option<ThreadId>,
    cancelled: bool,
    panicked: bool,
    owner_released: bool,
    completed_calls: u64,
}

impl<I> State<I> {
    fn stopped(&self) -> bool {
        self.cancelled || self.panicked || self.owner_released
    }

    fn resources_released(&self) -> bool {
        self.handler.is_none() && self.executing.is_none() && self.disposing.is_none()
    }
}

struct Shared<I> {
    state: Mutex<State<I>>,
    changed: Condvar,
}

impl<I> Shared<I> {
    fn lock(&self) -> MutexGuard<'_, State<I>> {
        // User code and capture destructors always run outside this mutex.
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn stop(&self, release_owner: bool) {
        let handler = {
            let mut state = self.lock();
            if release_owner {
                state.owner_released = true;
            } else {
                state.cancelled = true;
            }
            let handler = state.handler.take();
            if handler.is_some() {
                state.disposing = Some(thread::current().id());
            }
            self.changed.notify_all();
            handler
        };
        if let Some(handler) = handler {
            self.dispose(handler);
        }
    }

    fn dispose(&self, handler: Handler<I>) -> bool {
        let panicked = contain_unwind(|| drop(handler));
        let mut state = self.lock();
        state.panicked |= panicked;
        state.disposing = None;
        self.changed.notify_all();
        panicked
    }
}

// Dropping an arbitrary panic payload can panic again. Keep that payload out of
// the foreign-boundary return path. This leaks the exceptional payload only;
// Rust's panic hook still runs, and panic=abort is not catchable.
fn contain_unwind(action: impl FnOnce()) -> bool {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(()) => false,
        Err(payload) => {
            std::mem::forget(payload);
            true
        }
    }
}

/// Owns the Rust closure. Dropping the owner stops future invocations.
///
/// Wrap the owner in `Arc` if more than one thread can invoke it. Calls serialize
/// rather than dropping overlapping events. Same-thread recursion is rejected.
pub struct Callback<I> {
    shared: Arc<Shared<I>>,
}

/// Cloneable stop/status handle; it does not keep an abandoned closure alive.
pub struct CallbackControl<I> {
    shared: Arc<Shared<I>>,
}

impl<I> Clone for CallbackControl<I> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvocationOutcome {
    Executed,
    Stopped,
    Reentrant,
    /// The handler or its capture destructor unwound; future calls are stopped.
    Panicked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallbackStatus {
    pub cancelled: bool,
    pub panicked: bool,
    pub owner_released: bool,
    pub executing: bool,
    pub disposing: bool,
    /// Closure/captures released, not an acknowledgement from a native engine.
    pub resources_released: bool,
    /// Successfully returned handler calls, saturating at `u64::MAX`.
    pub completed_calls: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitOutcome {
    ResourcesReleased,
    TimedOut,
    /// Called by the active handler or capture destructor on the same thread.
    WouldBlockCurrentCallback,
}

impl<I> Callback<I> {
    pub fn new(handler: impl FnMut(I) + Send + 'static) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    handler: Some(Box::new(handler)),
                    executing: None,
                    disposing: None,
                    cancelled: false,
                    panicked: false,
                    owner_released: false,
                    completed_calls: 0,
                }),
                changed: Condvar::new(),
            }),
        }
    }

    pub fn control(&self) -> CallbackControl<I> {
        CallbackControl {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Invoke once, waiting for another thread's invocation to finish.
    ///
    /// Cancellation prevents calls not yet started; an executing handler runs
    /// to completion. Destruction of rejected inputs and unrelated caller panics
    /// are outside this handler boundary. Unwinding handler/destructor panics
    /// are contained; aborting panics cannot be caught.
    pub fn invoke(&self, input: I) -> InvocationOutcome {
        let current = thread::current().id();
        let mut handler = {
            let mut state = self.shared.lock();
            loop {
                if state.stopped() {
                    return InvocationOutcome::Stopped;
                }
                match state.executing {
                    Some(active) if active == current => return InvocationOutcome::Reentrant,
                    Some(_) => {
                        state = self
                            .shared
                            .changed
                            .wait(state)
                            .unwrap_or_else(|error| error.into_inner());
                    }
                    None => {
                        // Only invocation removes a running handler. Stops are
                        // permanent, so an unstopped idle state owns a handler.
                        let handler = state.handler.take().expect("idle callback has a handler");
                        state.executing = Some(current);
                        break handler;
                    }
                }
            }
        };
        let panicked = contain_unwind(|| handler(input));
        {
            let mut state = self.shared.lock();
            state.panicked |= panicked;
            if !panicked {
                state.completed_calls = state.completed_calls.saturating_add(1);
            }
            state.executing = None;
            if !state.stopped() {
                state.handler = Some(handler);
                self.shared.changed.notify_all();
                return InvocationOutcome::Executed;
            }
            state.disposing = Some(current);
            self.shared.changed.notify_all();
        }
        let drop_panicked = self.shared.dispose(handler);
        if panicked || drop_panicked {
            InvocationOutcome::Panicked
        } else {
            InvocationOutcome::Executed
        }
    }
}

impl<I> Drop for Callback<I> {
    fn drop(&mut self) {
        self.shared.stop(true);
    }
}

impl<I> CallbackControl<I> {
    /// Stop new calls. An active handler is not interrupted or waited for.
    /// Idle captures are destroyed here, so their destructors may take time.
    pub fn cancel(&self) {
        self.shared.stop(false);
    }

    /// Coherent snapshot of the Rust lifecycle under its mutex.
    pub fn status(&self) -> CallbackStatus {
        let state = self.shared.lock();
        CallbackStatus {
            cancelled: state.cancelled,
            panicked: state.panicked,
            owner_released: state.owner_released,
            executing: state.executing.is_some(),
            disposing: state.disposing.is_some(),
            resources_released: state.resources_released(),
            completed_calls: state.completed_calls,
        }
    }

    /// Wait for stopping and closure destruction; never waits on itself.
    /// Spurious wakeups recheck the predicate using the same mutex.
    pub fn wait_stopped(&self, timeout: Duration) -> WaitOutcome {
        let started = Instant::now();
        let current = thread::current().id();
        let mut state = self.shared.lock();
        loop {
            if state.stopped() && state.resources_released() {
                return WaitOutcome::ResourcesReleased;
            }
            if state.executing == Some(current) || state.disposing == Some(current) {
                return WaitOutcome::WouldBlockCurrentCallback;
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return WaitOutcome::TimedOut;
            }
            // Bound the OS wait even when a caller supplies Duration::MAX.
            let (next, _) = self
                .shared
                .changed
                .wait_timeout(state, remaining.min(Duration::from_secs(60)))
                .unwrap_or_else(|error| error.into_inner());
            state = next;
        }
    }
}
