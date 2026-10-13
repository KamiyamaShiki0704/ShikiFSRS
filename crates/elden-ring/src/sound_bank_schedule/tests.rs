use super::*;
use crate::sound_banks::{BankLoad, BankLoadResolution, BankUnload};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
#[derive(Default)]
struct Calls {
    load_ids: Vec<u32>,
    unloads: Vec<u32>,
    asset_ids: Vec<u32>,
}
struct Engine {
    calls: Rc<RefCell<Calls>>,
    loads: VecDeque<Result<BankLoad>>,
    unloads: VecDeque<Result<BankUnload>>,
}
// SAFETY: Original scripted copies/obligations; no native calls/retained input or
// cleanup in Drop. Every Err leaves synthetic reference ownership unchanged.
unsafe impl SoundBankEngine for Engine {
    unsafe fn load_memory_copy(&mut self, bytes: &[u8]) -> Result<BankLoad> {
        self.calls
            .borrow_mut()
            .load_ids
            .push(u32::from_le_bytes(bytes.try_into().unwrap()));
        self.loads.pop_front().expect("planned load")
    }
    unsafe fn unload(&mut self, id: u32) -> Result<BankUnload> {
        self.calls.borrow_mut().unloads.push(id);
        self.unloads.pop_front().expect("planned unload")
    }
}
struct Source {
    calls: Rc<RefCell<Calls>>,
    failures: usize,
}
impl BankAssetSource for Source {
    fn read_bank(&mut self, id: u32) -> Result<Vec<u8>> {
        self.calls.borrow_mut().asset_ids.push(id);
        if self.failures != 0 {
            self.failures -= 1;
            return Err(Error::InvalidProfile("bank file missing"));
        }
        Ok(id.to_le_bytes().to_vec())
    }
}
fn setup(
    loads: impl IntoIterator<Item = Result<BankLoad>>,
    unloads: impl IntoIterator<Item = Result<BankUnload>>,
) -> (SoundBankOwner<Engine>, Source, Rc<RefCell<Calls>>) {
    let calls = Rc::new(RefCell::new(Calls::default()));
    let engine = Engine {
        calls: Rc::clone(&calls),
        loads: loads.into_iter().collect(),
        unloads: unloads.into_iter().collect(),
    };
    (
        SoundBankOwner::new(engine, 16).unwrap(),
        Source {
            calls: Rc::clone(&calls),
            failures: 0,
        },
        calls,
    )
}
fn config() -> SoundBankScheduleConfig {
    SoundBankScheduleConfig {
        start_delay_ms: 0,
        retry_every_frames: 1,
        poll_every_frames: 1,
        conditional_enabled: true,
        ..Default::default()
    }
}
#[test]
fn startup_delay_duplicates_attempt_budget_and_interval_one_are_independent_of_conditions() {
    // SAFETY: Self-authored log engine/source; only original Rust state is accessed.
    unsafe {
        let (mut owner, mut source, calls) = setup(
            [Ok(BankLoad::Acquired(10)), Ok(BankLoad::AlreadyPresent(10))],
            [Ok(BankUnload::Released)],
        );
        let mut cfg = config();
        cfg.static_ids = vec![1, 1, 2];
        cfg.start_delay_ms = 5;
        cfg.attempts_per_id = 2;
        let mut scheduler = SoundBankSchedule::new(cfg, 10, &owner).unwrap();
        scheduler
            .startup_frame(14, &mut owner, &mut source)
            .unwrap();
        assert!(calls.borrow().asset_ids.is_empty());
        source.failures = 1;
        let report = scheduler
            .startup_frame(15, &mut owner, &mut source)
            .unwrap();
        assert_eq!(
            report
                .startup
                .iter()
                .map(|x| x.attempts)
                .collect::<Vec<_>>(),
            vec![1, 1, 1]
        );
        assert_eq!(report.startup[0].state, StartupBankState::Pending);
        assert_eq!(calls.borrow().load_ids, vec![1, 2]);
        source.failures = 1;
        let report = scheduler
            .startup_frame(15, &mut owner, &mut source)
            .unwrap();
        assert_eq!(report.startup[0].state, StartupBankState::Exhausted);
        assert_eq!(
            report.startup.iter().map(|x| x.ordinal).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(calls.borrow().asset_ids, vec![1, 1, 2, 1]);
        scheduler
            .startup_frame(15, &mut owner, &mut source)
            .unwrap();
        assert_eq!(calls.borrow().asset_ids.len(), 4);
        let closed = scheduler.close_step(&mut owner).unwrap();
        assert_eq!(closed.remaining_requests, 0);
        assert_eq!(calls.borrow().unloads, vec![10]);
        assert!(
            scheduler
                .startup_frame(15, &mut owner, &mut source)
                .is_err()
        );
    }
}
#[test]
fn condition_polls_deduplicate_external_banks_and_keep_static_alias_until_close() {
    // SAFETY: Original scripted reference counts, no native lifetime or filesystem.
    unsafe {
        let (mut owner, mut source, calls) = setup(
            [
                Ok(BankLoad::Acquired(10)),
                Ok(BankLoad::AlreadyPresent(10)),
                Ok(BankLoad::AlreadyPresent(20)),
            ],
            [Ok(BankUnload::Released)],
        );
        let mut cfg = config();
        cfg.static_ids = vec![1];
        cfg.poll_every_frames = 2;
        cfg.start_delay_ms = 5000;
        let mut scheduler = SoundBankSchedule::new(cfg, 0, &owner).unwrap();
        // Conditional cadence starts before startup delay, matching its separate task.
        scheduler
            .conditional_frame(&mut owner, &mut source, || panic!("not a poll frame"))
            .unwrap();
        scheduler
            .startup_frame(5000, &mut owner, &mut source)
            .unwrap();
        let targets = BTreeSet::from([2, 2, 3]);
        scheduler
            .conditional_frame(&mut owner, &mut source, || Ok(Some(targets)))
            .unwrap();
        assert_eq!(calls.borrow().load_ids, vec![1, 2, 3]);
        for _ in 0..2 {
            scheduler
                .conditional_frame(&mut owner, &mut source, || Ok(Some(BTreeSet::from([2, 3]))))
                .unwrap();
        }
        assert_eq!(calls.borrow().load_ids.len(), 3);
        for _ in 0..2 {
            scheduler
                .conditional_frame(&mut owner, &mut source, || Ok(Some(BTreeSet::new())))
                .unwrap();
        }
        assert!(calls.borrow().unloads.is_empty());
        assert!(scheduler.report(&owner).unwrap().conditional.is_empty());
        assert_eq!(owner.observations()[0].owned_references, 1);
        scheduler.close_step(&mut owner).unwrap();
        assert_eq!(calls.borrow().unloads, vec![10]);
        drop(scheduler);
        drop(owner);
        assert_eq!(calls.borrow().unloads, vec![10]);
    }
}
#[test]
fn unavailable_observation_preflight_failure_and_retrylater_do_not_reload_or_erase_obligations() {
    // SAFETY: Only owned original buffers/state, all retries are script-proven no side effect.
    unsafe {
        let (mut owner, mut source, calls) = setup(
            [Ok(BankLoad::Acquired(10))],
            [
                Ok(BankUnload::RetryLater),
                Err(Error::InvalidProfile("unload preflight")),
                Ok(BankUnload::Released),
            ],
        );
        let mut scheduler = SoundBankSchedule::new(config(), 0, &owner).unwrap();
        source.failures = 1;
        let report = scheduler
            .conditional_frame(&mut owner, &mut source, || Ok(Some(BTreeSet::from([1]))))
            .unwrap();
        assert_eq!(report.failures.len(), 1);
        assert!(report.conditional.is_empty());
        scheduler
            .conditional_frame(&mut owner, &mut source, || Ok(Some(BTreeSet::from([1]))))
            .unwrap();
        let before = scheduler.report(&owner).unwrap().conditional;
        assert!(
            scheduler
                .conditional_frame(&mut owner, &mut source, || Err(Error::InvalidProfile(
                    "world changed"
                )))
                .is_err()
        );
        scheduler
            .conditional_frame(&mut owner, &mut source, || Ok(None))
            .unwrap();
        assert_eq!(scheduler.report(&owner).unwrap().conditional, before);
        assert!(calls.borrow().unloads.is_empty());
        for _ in 0..2 {
            let report = scheduler
                .conditional_frame(&mut owner, &mut source, || Ok(Some(BTreeSet::new())))
                .unwrap();
            assert_eq!(report.conditional, before);
        }
        assert_eq!(calls.borrow().load_ids, vec![1]);
        scheduler
            .conditional_frame(&mut owner, &mut source, || Ok(Some(BTreeSet::from([1]))))
            .unwrap();
        assert_eq!(calls.borrow().load_ids, vec![1]);
        scheduler
            .conditional_frame(&mut owner, &mut source, || Ok(Some(BTreeSet::new())))
            .unwrap();
        assert!(owner.observations().is_empty());
        assert_eq!(calls.borrow().unloads, vec![10, 10, 10]);
        let (mut owner, mut source, calls) =
            setup([Ok(BankLoad::Acquired(10))], [Ok(BankUnload::Released)]);
        let mut cfg = config();
        cfg.unload_when_inactive = false;
        let mut scheduler = SoundBankSchedule::new(cfg, 0, &owner).unwrap();
        scheduler
            .conditional_frame(&mut owner, &mut source, || Ok(Some(BTreeSet::from([1]))))
            .unwrap();
        scheduler
            .conditional_frame(&mut owner, &mut source, || Ok(Some(BTreeSet::new())))
            .unwrap();
        assert!(calls.borrow().unloads.is_empty());
        assert_eq!(scheduler.report(&owner).unwrap().conditional.len(), 1);
        scheduler.close_step(&mut owner).unwrap();
        assert_eq!(calls.borrow().unloads, vec![10]);
    }
}
#[test]
fn uncertain_load_and_unload_block_all_new_work_until_explicit_original_attempt_reconciliation() {
    // SAFETY: Resolution is known from the original script, never inferred from game presence.
    unsafe {
        let unknown = BankLoadUncertainty {
            native_result: 400,
            bank_id_output: 37,
            postflight: Err(Error::InvalidProfile("lost postflight")),
        };
        let (mut owner, mut source, calls) = setup(
            [
                Ok(BankLoad::Uncertain(unknown.clone())),
                Ok(BankLoad::Acquired(22)),
            ],
            [Ok(BankUnload::Released)],
        );
        let mut cfg = config();
        cfg.static_ids = vec![1, 2];
        let mut scheduler = SoundBankSchedule::new(cfg, 0, &owner).unwrap();
        let report = scheduler.startup_frame(0, &mut owner, &mut source).unwrap();
        assert!(report.blocked);
        assert_eq!(report.uncertain_loads[0].1, unknown);
        assert_eq!(report.startup[1].attempts, 0);
        assert_eq!(calls.borrow().load_ids, vec![1]);
        scheduler.startup_frame(1, &mut owner, &mut source).unwrap();
        scheduler
            .conditional_frame(&mut owner, &mut source, || {
                panic!("blocked does not observe")
            })
            .unwrap();
        assert_eq!(calls.borrow().asset_ids, vec![1]);
        owner
            .reconcile_load(report.uncertain_loads[0].0, BankLoadResolution::NoReference)
            .unwrap();
        scheduler.startup_frame(2, &mut owner, &mut source).unwrap();
        assert_eq!(calls.borrow().load_ids, vec![1, 2]);
        scheduler.close_step(&mut owner).unwrap();
        assert_eq!(calls.borrow().unloads, vec![22]);
        let (mut owner, mut source, calls) = setup(
            [Ok(BankLoad::Acquired(10)), Ok(BankLoad::Acquired(11))],
            [Ok(BankUnload::Uncertain), Ok(BankUnload::Released)],
        );
        let mut scheduler = SoundBankSchedule::new(config(), 0, &owner).unwrap();
        scheduler
            .conditional_frame(&mut owner, &mut source, || Ok(Some(BTreeSet::from([1]))))
            .unwrap();
        let report = scheduler
            .conditional_frame(&mut owner, &mut source, || Ok(Some(BTreeSet::from([2]))))
            .unwrap();
        assert!(report.blocked);
        assert_eq!(calls.borrow().load_ids, vec![1]);
        scheduler
            .conditional_frame(&mut owner, &mut source, || {
                panic!("unknown unload blocks snapshot")
            })
            .unwrap();
        owner.reconcile_unload(10, true).unwrap();
        scheduler
            .conditional_frame(&mut owner, &mut source, || Ok(Some(BTreeSet::from([2]))))
            .unwrap();
        assert_eq!(calls.borrow().load_ids, vec![1, 2]);
        scheduler.close_step(&mut owner).unwrap();
        assert_eq!(calls.borrow().unloads, vec![10, 11]);
    }
}
#[test]
fn bounds_owner_identity_disabled_inputs_and_clock_overflow_precede_native_work() {
    // SAFETY: Original fake engines have no planned calls; any accidental native dispatch fails.
    unsafe {
        let (mut owner, mut source, calls) = setup([], []);
        let (mut foreign, _, foreign_calls) = setup([], []);
        let mut cfg = config();
        cfg.static_ids = vec![1];
        let mut scheduler = SoundBankSchedule::new(cfg, 10, &owner).unwrap();
        assert!(scheduler.startup_frame(9, &mut owner, &mut source).is_err());
        assert!(
            scheduler
                .startup_frame(10, &mut foreign, &mut source)
                .is_err()
        );
        assert!(scheduler.report(&foreign).is_err());
        assert!(scheduler.close_step(&mut foreign).is_err());
        assert!(
            scheduler
                .conditional_frame(&mut owner, &mut source, || Ok(Some((0..1025).collect())))
                .is_err()
        );
        scheduler.startup_frame = u64::MAX;
        assert!(
            scheduler
                .startup_frame(10, &mut owner, &mut source)
                .is_err()
        );
        scheduler.conditional_frame = u64::MAX;
        assert!(
            scheduler
                .conditional_frame(&mut owner, &mut source, || panic!("overflow"))
                .is_err()
        );
        assert!(calls.borrow().load_ids.is_empty());
        assert!(calls.borrow().asset_ids.is_empty());
        assert!(foreign_calls.borrow().unloads.is_empty());
        for field in 0..4 {
            let mut cfg = config();
            match field {
                0 => cfg.static_ids.resize(257, 1),
                1 => cfg.attempts_per_id = 0,
                2 => cfg.retry_every_frames = 0,
                _ => cfg.poll_every_frames = 0,
            };
            assert!(SoundBankSchedule::new(cfg, 0, &owner).is_err());
        }
        let mut cfg = config();
        cfg.start_delay_ms = 1;
        assert!(SoundBankSchedule::new(cfg, u64::MAX, &owner).is_err());
        for field in 0..3 {
            let mut cfg = config();
            cfg.static_ids = vec![1];
            match field {
                0 => cfg.enabled = false,
                1 => cfg.memory_copy_enabled = false,
                _ => cfg.conditional_enabled = false,
            };
            let mut scheduler = SoundBankSchedule::new(cfg, 0, &owner).unwrap();
            scheduler
                .conditional_frame(&mut owner, &mut source, || panic!("disabled"))
                .unwrap();
        }
        scheduler.close_step(&mut owner).unwrap();
        assert_eq!(
            scheduler.report(&owner).unwrap().startup[0].state,
            StartupBankState::Cancelled
        );
        assert!(SoundBankSchedule::new(config(), 0, &owner).is_err());
    }
}
