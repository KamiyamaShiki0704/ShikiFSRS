use super::*;
use std::{cell::RefCell, collections::VecDeque};

#[derive(Default)]
struct Calls {
    loads: usize,
    unloads: Vec<u32>,
}
struct Engine {
    loads: VecDeque<Result<BankLoad>>,
    unloads: VecDeque<Result<BankUnload>>,
    calls: Rc<RefCell<Calls>>,
}
impl Engine {
    fn new(
        loads: impl IntoIterator<Item = Result<BankLoad>>,
        unloads: impl IntoIterator<Item = Result<BankUnload>>,
    ) -> Self {
        Self {
            loads: loads.into_iter().collect(),
            unloads: unloads.into_iter().collect(),
            calls: Rc::new(RefCell::new(Calls::default())),
        }
    }
}
// SAFETY: Original in-memory state machine; no foreign memory or side effects on error.
// Calls consume exactly the scripted obligation, retain no input, and Drop performs no cleanup.
unsafe impl SoundBankEngine for Engine {
    unsafe fn load_memory_copy(&mut self, _: &[u8]) -> Result<BankLoad> {
        self.calls.borrow_mut().loads += 1;
        self.loads.pop_front().expect("planned load")
    }
    unsafe fn unload(&mut self, id: u32) -> Result<BankUnload> {
        self.calls.borrow_mut().unloads.push(id);
        self.unloads.pop_front().expect("planned unload")
    }
}

#[test]
fn borrowed_bank_never_unloads_and_owned_alias_waits_for_last_demand() {
    // SAFETY: All engines in these tests are independent synchronous simulations.
    unsafe {
        let engine = Engine::new(
            [
                Ok(BankLoad::AlreadyPresent(20)),
                Ok(BankLoad::Acquired(10)),
                Ok(BankLoad::AlreadyPresent(10)),
            ],
            [Ok(BankUnload::Released)],
        );
        let calls = engine.calls.clone();
        let mut owner = SoundBankOwner::new(engine, 3).unwrap();
        let borrowed = owner.acquire(&[1]).unwrap();
        let owned = owner.acquire(&[2]).unwrap();
        let alias = owner.acquire(&[3]).unwrap();
        assert_eq!(owner.release(borrowed).unwrap(), BankRelease::Borrowed);
        assert_eq!(owner.release(owned).unwrap(), BankRelease::Shared);
        assert!(calls.borrow().unloads.is_empty());
        assert_eq!(owner.observations()[0].owned_references, 1);
        assert_eq!(owner.release(alias).unwrap(), BankRelease::Released);
        assert_eq!(owner.release(alias).unwrap(), BankRelease::AlreadyReleased);
        assert_eq!(calls.borrow().unloads, [10]);
        assert!(owner.observations().is_empty());
    }
}

#[test]
fn partial_release_keeps_only_remaining_obligations_and_retries_without_reload() {
    // SAFETY: Original simulation; error means no release occurred.
    unsafe {
        let engine = Engine::new(
            [Ok(BankLoad::Acquired(10)), Ok(BankLoad::Acquired(10))],
            [
                Ok(BankUnload::Released),
                Ok(BankUnload::RetryLater),
                Err(Error::NativeOperationRejected("temporary")),
                Ok(BankUnload::Released),
            ],
        );
        let calls = engine.calls.clone();
        let mut owner = SoundBankOwner::new(engine, 2).unwrap();
        let first = owner.acquire(&[1]).unwrap();
        let last = owner.acquire(&[1]).unwrap();
        assert_eq!(owner.release(first).unwrap(), BankRelease::Shared);
        assert_eq!(owner.release(last).unwrap(), BankRelease::RetryLater);
        assert_eq!(owner.observations()[0].owned_references, 1);
        assert!(owner.release(last).is_err());
        assert_eq!(owner.observations()[0].owned_references, 1);
        assert_eq!(owner.release(last).unwrap(), BankRelease::Released);
        assert_eq!(calls.borrow().loads, 2);
        assert_eq!(calls.borrow().unloads, [10, 10, 10, 10]);
    }
}

#[test]
fn uncertain_native_release_never_retries_before_explicit_original_reference_reconciliation() {
    // SAFETY: Simulation supplies independent proof of each uncertain call's consumed status.
    unsafe {
        for consumed in [false, true] {
            let engine = Engine::new(
                [Ok(BankLoad::Acquired(10))],
                [Ok(BankUnload::Uncertain), Ok(BankUnload::Released)],
            );
            let calls = engine.calls.clone();
            let mut owner = SoundBankOwner::new(engine, 2).unwrap();
            let request = owner.acquire(&[1]).unwrap();
            assert_eq!(owner.release(request).unwrap(), BankRelease::Uncertain);
            assert_eq!(owner.release(request).unwrap(), BankRelease::Uncertain);
            assert!(owner.acquire(&[2]).is_err());
            assert_eq!(calls.borrow().unloads, [10]);
            assert!(owner.reconcile_unload(11, consumed).is_err());
            owner.reconcile_unload(10, consumed).unwrap();
            assert!(owner.reconcile_unload(10, consumed).is_err());
            assert_eq!(owner.release(request).unwrap(), BankRelease::Released);
            assert_eq!(calls.borrow().unloads.len(), if consumed { 1 } else { 2 });
        }
    }
}

#[test]
fn close_blocks_new_acquisitions_and_continues_other_banks_after_failure() {
    // SAFETY: All operations execute against original simulated storage.
    unsafe {
        let engine = Engine::new(
            [
                Ok(BankLoad::Acquired(10)),
                Ok(BankLoad::Acquired(20)),
                Ok(BankLoad::AlreadyPresent(30)),
            ],
            [
                Err(Error::NativeOperationRejected("temporary")),
                Ok(BankUnload::Released),
                Ok(BankUnload::Released),
            ],
        );
        let calls = engine.calls.clone();
        let mut owner = SoundBankOwner::new(engine, 3).unwrap();
        let first = owner.acquire(&[1]).unwrap();
        owner.acquire(&[2]).unwrap();
        owner.acquire(&[3]).unwrap();
        let progress = owner.close_step();
        assert!(owner.is_closing());
        assert_eq!(progress.remaining_requests, 1);
        assert_eq!(progress.remaining_references, 1);
        assert_eq!(progress.failures[0].0, first);
        assert_eq!(calls.borrow().unloads, [10, 20]);
        assert!(owner.acquire(&[1]).is_err());
        let progress = owner.close_step();
        assert_eq!(progress.remaining_requests, 0);
        assert_eq!(progress.remaining_references, 0);
        assert!(progress.failures.is_empty());
        assert!(owner.close_step().failures.is_empty());
        assert_eq!(calls.borrow().unloads, [10, 20, 10]);
    }
}

#[test]
fn bank_requests_are_bound_to_owner_and_invalid_inputs_do_not_call_engine() {
    // SAFETY: No game state; both engines are independent simulations.
    unsafe {
        for limit in [0, 1025] {
            assert!(SoundBankOwner::new(Engine::new([], []), limit).is_err());
        }
        let mut first =
            SoundBankOwner::new(Engine::new([Ok(BankLoad::Acquired(10))], []), 1).unwrap();
        let engine = Engine::new([Ok(BankLoad::Acquired(20))], []);
        let calls = engine.calls.clone();
        let mut other = SoundBankOwner::new(engine, 1).unwrap();
        let foreign = first.acquire(&[1]).unwrap();
        assert!(other.release(foreign).is_err());
        assert!(other.acquire(&[]).is_err());
        assert_eq!(calls.borrow().loads, 0);
        other.acquire(&[1]).unwrap();
        assert!(other.acquire(&[2]).is_err());
        assert_eq!(calls.borrow().loads, 1);
        other.next_request = u64::MAX;
        other.requests.clear(); // Isolate sequence exhaustion before the next engine call.
        assert!(other.acquire(&[1]).is_err());
        assert_eq!(calls.borrow().loads, 1);
    }
}

#[test]
fn load_rejection_preserves_accounting_and_drop_never_unloads_native_banks() {
    // SAFETY: Original simulation reports no acquisition on rejection.
    unsafe {
        let engine = Engine::new(
            [
                Err(Error::NativeOperationRejected("load rejected")),
                Ok(BankLoad::Acquired(10)),
            ],
            [],
        );
        let calls = engine.calls.clone();
        let mut owner = SoundBankOwner::new(engine, 1).unwrap();
        assert!(owner.acquire(&[1]).is_err());
        assert!(owner.observations().is_empty());
        owner.acquire(&[1]).unwrap();
        drop(owner);
        assert_eq!(calls.borrow().loads, 2);
        assert!(calls.borrow().unloads.is_empty());
    }
}

fn unknown_load(result: i32, output: u32) -> BankLoad {
    BankLoad::Uncertain(BankLoadUncertainty {
        native_result: result,
        bank_id_output: output,
        postflight: Err(Error::InvalidProfile("post-call code changed")),
    })
}

#[test]
fn unknown_load_retains_attempt_and_blocks_all_native_retry_and_release() {
    // SAFETY: Original scripted engine and independent reconciliation proof.
    unsafe {
        let engine = Engine::new(
            [Ok(BankLoad::Acquired(10)), Ok(unknown_load(-5, u32::MAX))],
            [Ok(BankUnload::Released)],
        );
        let calls = engine.calls.clone();
        let mut owner = SoundBankOwner::new(engine, 3).unwrap();
        let known = owner.acquire(&[1]).unwrap();
        let unknown = owner.acquire(&[2]).unwrap();
        assert!(owner.acquire(&[3]).is_err());
        assert_eq!(owner.release(known).unwrap(), BankRelease::Uncertain);
        assert_eq!(owner.release(unknown).unwrap(), BankRelease::Uncertain);
        let progress = owner.close_step();
        assert_eq!(progress.remaining_requests, 2);
        assert_eq!(progress.remaining_references, 1);
        assert_eq!(progress.uncertain_loads, owner.uncertain_loads());
        assert_eq!(progress.uncertain_loads[0].0, unknown);
        assert_eq!(progress.uncertain_loads[0].1.native_result, -5);
        assert_eq!(progress.uncertain_loads[0].1.bank_id_output, u32::MAX);
        assert!(progress.uncertain_loads[0].1.postflight.is_err());
        assert!(progress.uncertain_banks.is_empty());
        assert!(calls.borrow().unloads.is_empty());
        owner
            .reconcile_load(unknown, BankLoadResolution::NoReference)
            .unwrap();
        assert_eq!(
            owner.release(unknown).unwrap(),
            BankRelease::AlreadyReleased
        );
        assert_eq!(owner.close_step().remaining_requests, 0);
        assert_eq!(calls.borrow().loads, 2);
        assert_eq!(calls.borrow().unloads, [10]);
    }
}

#[test]
fn unknown_load_reconciliation_tracks_owned_and_borrowed_aliases_without_reload() {
    // SAFETY: Simulation specifies the original load's ownership, not current presence.
    unsafe {
        for acquired in [false, true] {
            let engine = Engine::new(
                [Ok(BankLoad::Acquired(10)), Ok(unknown_load(69, 0))],
                [Ok(BankUnload::Released), Ok(BankUnload::Released)],
            );
            let calls = engine.calls.clone();
            let mut owner = SoundBankOwner::new(engine, 2).unwrap();
            let first = owner.acquire(&[1]).unwrap();
            let unknown = owner.acquire(&[2]).unwrap();
            let outcome = if acquired {
                BankLoadResolution::Acquired(10)
            } else {
                BankLoadResolution::AlreadyPresent(10)
            };
            owner.reconcile_load(unknown, outcome).unwrap();
            assert_eq!(owner.release(first).unwrap(), BankRelease::Shared);
            assert_eq!(
                owner.observations()[0].owned_references,
                if acquired { 2 } else { 1 }
            );
            assert_eq!(owner.release(unknown).unwrap(), BankRelease::Released);
            assert_eq!(calls.borrow().unloads.len(), if acquired { 2 } else { 1 });
            assert_eq!(calls.borrow().loads, 2);
            assert!(owner.reconcile_load(unknown, outcome).is_err());
        }
    }
}

#[test]
fn unknown_load_reconciliation_rejects_foreign_or_nonpending_ticket_and_keeps_close_gate() {
    // SAFETY: Independent simulation with no native calls during reconciliation.
    unsafe {
        let mut owner = SoundBankOwner::new(
            Engine::new([Ok(unknown_load(1, 99))], [Ok(BankUnload::Released)]),
            1,
        )
        .unwrap();
        let mut other = SoundBankOwner::new(Engine::new([], []), 1).unwrap();
        let request = owner.acquire(&[1]).unwrap();
        assert!(
            other
                .reconcile_load(request, BankLoadResolution::Acquired(99))
                .is_err()
        );
        assert!(
            owner
                .reconcile_load(
                    BankRequest {
                        sequence: request.sequence + 1,
                        ..request
                    },
                    BankLoadResolution::Acquired(99)
                )
                .is_err()
        );
        assert_eq!(owner.uncertain_loads().len(), 1);
        assert_eq!(owner.close_step().remaining_requests, 1);
        owner
            .reconcile_load(request, BankLoadResolution::Acquired(99))
            .unwrap();
        assert!(owner.acquire(&[1]).is_err());
        let progress = owner.close_step();
        assert_eq!(progress.remaining_requests, 0);
        assert!(progress.uncertain_loads.is_empty());
    }
}

#[test]
fn uncertain_load_drop_preserves_no_cleanup_side_effect_or_false_known_bank() {
    // SAFETY: Original simulation; unknown output zero does not identify an owned bank.
    unsafe {
        let engine = Engine::new([Ok(unknown_load(i32::MIN, 0))], []);
        let calls = engine.calls.clone();
        let mut owner = SoundBankOwner::new(engine, 1).unwrap();
        owner.acquire(&[1]).unwrap();
        assert!(owner.observations().is_empty());
        assert_eq!(owner.uncertain_loads()[0].1.bank_id_output, 0);
        drop(owner);
        assert_eq!(calls.borrow().loads, 1);
        assert!(calls.borrow().unloads.is_empty());
    }
}
