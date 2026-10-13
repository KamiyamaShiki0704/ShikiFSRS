//! Bounded trigger consumption and admission; neither acquires engine lifetime guards.
use crate::{
    TaskPhase,
    characters::CharacterEffectsObservation,
    effects::{EffectEngine, EffectId},
    summon_lifecycle::SummonCreationWait,
};
use shikifsrs_runtime::{Error, Result};
use std::rc::Rc;

/// Complete, fresh primary-player effect observation under the host's exclusive phase.
/// The native adapter copies the whole bounded chain and rechecks the primary identity.
pub trait SummonTriggerEffectBackend: EffectEngine {
    fn primary_effects(&self) -> Result<Option<CharacterEffectsObservation>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerEffectRemoval {
    pub configuration_index: usize,
    pub effect: EffectId,
    /// None means dispatch was entered but did not return, including a host panic.
    pub returned: Option<Result<bool>>,
    pub postflight: Option<Result<()>>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummonTriggerEffectsReceipt {
    pub source: Option<CharacterEffectsObservation>,
    /// Configuration order, including aliases. Feed admission only when completed is true.
    pub triggered: Vec<usize>,
    pub attempts: Vec<TriggerEffectRemoval>,
    pub completed: bool,
    pub failure: Option<Error>,
}

/// Consume matching trigger IDs before cooldown/capacity admission, as required by ER_Summon.
/// An incomplete batch retains its journal and blocks further calls. There is no retry,
/// native Drop cleanup, automatic rollback, or inference that a failed removal did no work.
pub struct SummonTriggerEffectConsumer {
    scope: (u64, u64),
    configurations: Vec<EffectId>,
    effect_limit: usize,
    inflight: Option<SummonTriggerEffectsReceipt>,
    _thread: Rc<()>,
}
impl SummonTriggerEffectConsumer {
    pub fn new(
        world_address: u64,
        epoch: u64,
        configurations: &[EffectId],
        configuration_limit: usize,
        effect_limit: usize,
    ) -> Result<Self> {
        if world_address == 0
            || !world_address.is_multiple_of(8)
            || epoch == 0
            || configurations.len() > configuration_limit
            || !(1..=65536).contains(&effect_limit)
        {
            return Err(Error::InvalidProfile(
                "invalid summon effect scope or bound",
            ));
        }
        let mut owned = Vec::new();
        owned.try_reserve_exact(configurations.len()).map_err(|_| {
            Error::InvalidProfile("cannot reserve bounded summon effect configurations")
        })?;
        owned.extend_from_slice(configurations);
        Ok(Self {
            scope: (world_address, epoch),
            configurations: owned,
            effect_limit,
            inflight: None,
            _thread: Rc::new(()),
        })
    }
    pub fn inflight(&self) -> Option<&SummonTriggerEffectsReceipt> {
        self.inflight.as_ref()
    }
    /// Preserve every matching configuration in order; remove one native node per configuration,
    /// including aliases of an ID. A later alias may return false after its last node was removed.
    /// Completed proves copied postflight agreement, not gameplay acceptance or ownership.
    /// Missing primary is an empty completed batch; missing effect storage is a backend error.
    /// # Safety
    /// Host independently owns PostPhysics mutation authority, a pinned primary/player/effect
    /// graph and original unlink callbacks/allocator/PARAM/module lifetime for the entire batch.
    /// No concurrent reader, target replacement, teardown, ABA or code change may overlap it.
    /// A phase value and repeated observations do not establish these guarantees. The host
    /// retains this consumer, including its incomplete journal, until uncertainty is resolved.
    pub unsafe fn consume(
        &mut self,
        backend: &impl SummonTriggerEffectBackend,
        phase: TaskPhase,
    ) -> Result<SummonTriggerEffectsReceipt> {
        if phase != TaskPhase::CHR_POST_PHYSICS || self.inflight.is_some() {
            return Err(Error::NativeOperationRejected(
                "summon effect phase or incomplete batch forbids consumption",
            ));
        }
        let source = backend.primary_effects()?;
        if let Some(source) = &source {
            self.validate_source(source)?;
        }
        let mut triggered = Vec::new();
        triggered
            .try_reserve_exact(self.configurations.len())
            .map_err(|_| Error::InvalidProfile("cannot reserve bounded summon effect matches"))?;
        if let Some(source) = &source {
            for (index, effect) in self.configurations.iter().enumerate() {
                if source
                    .entries
                    .iter()
                    .any(|node| node.native_id == effect.value())
                {
                    triggered.push(index);
                }
            }
        }
        let mut attempts = Vec::new();
        attempts
            .try_reserve_exact(triggered.len())
            .map_err(|_| Error::InvalidProfile("cannot reserve bounded summon effect journal"))?;
        self.inflight = Some(SummonTriggerEffectsReceipt {
            source: source.clone(),
            triggered,
            attempts,
            completed: false,
            failure: None,
        });
        let mut expected = source;
        let count = self
            .inflight
            .as_ref()
            .expect("published batch")
            .triggered
            .len();
        for ordinal in 0..count {
            let index = self.inflight.as_ref().expect("published batch").triggered[ordinal];
            let effect = self.configurations[index];
            if let Err(error) = self.agreement(backend, &expected) {
                return Ok(self.fail(error));
            }
            let current = expected.as_mut().expect("matches require a primary");
            // Publish the attempted operation before calling host/native code, including panic.
            self.inflight
                .as_mut()
                .expect("published batch")
                .attempts
                .push(TriggerEffectRemoval {
                    configuration_index: index,
                    effect,
                    returned: None,
                    postflight: None,
                });
            // SAFETY: Caller supplies the same live primary, exclusive phase and unlink graph.
            let returned = unsafe { backend.remove(current.character, effect) };
            self.inflight
                .as_mut()
                .expect("published batch")
                .attempts
                .last_mut()
                .expect("published attempt")
                .returned = Some(returned.clone());
            let removed = match returned {
                Ok(removed) => removed,
                Err(error) => return Ok(self.fail(error)),
            };
            let matching = current
                .entries
                .iter()
                .position(|node| node.native_id == effect.value());
            let postflight = if removed == matching.is_some() {
                if let Some(matching) = matching {
                    current.entries.remove(matching);
                }
                self.agreement(backend, &expected)
            } else {
                Err(Error::NativeOperationRejected(
                    "effect removal result disagrees with complete chain",
                ))
            };
            self.inflight
                .as_mut()
                .expect("published batch")
                .attempts
                .last_mut()
                .expect("published attempt")
                .postflight = Some(postflight.clone());
            if let Err(error) = postflight {
                return Ok(self.fail(error));
            }
        }
        let mut receipt = self.inflight.take().expect("published batch");
        receipt.completed = true;
        Ok(receipt)
    }
    fn validate_source(&self, source: &CharacterEffectsObservation) -> Result<()> {
        let identity = source.character;
        if (identity.world_address, identity.epoch) != self.scope
            || identity.address == 0
            || !identity.address.is_multiple_of(8)
            || source.container_address == 0
            || !source.container_address.is_multiple_of(8)
            || source.entries.len() > self.effect_limit
        {
            return Err(Error::NativeOperationRejected(
                "foreign or oversized summon effect source",
            ));
        }
        let mut addresses = std::collections::BTreeSet::new();
        for node in &source.entries {
            if node.address == 0
                || !node.address.is_multiple_of(8)
                || !addresses.insert(node.address)
            {
                return Err(Error::InvalidProfile("invalid summon effect chain node"));
            }
        }
        Ok(())
    }
    fn agreement(
        &self,
        backend: &impl SummonTriggerEffectBackend,
        expected: &Option<CharacterEffectsObservation>,
    ) -> Result<()> {
        let actual = backend.primary_effects()?;
        if let Some(actual) = &actual {
            self.validate_source(actual)?;
        }
        if &actual != expected {
            return Err(Error::NativeOperationRejected(
                "primary or complete summon effect chain changed",
            ));
        }
        Ok(())
    }
    fn fail(&mut self, error: Error) -> SummonTriggerEffectsReceipt {
        let journal = self.inflight.as_mut().expect("published batch");
        journal.failure = Some(error);
        journal.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummonTriggerPolicy {
    pub configuration_count: usize,
    /// Explicit caller bound, including disabled configurations.
    pub configuration_limit: usize,
    pub max_active: usize,
    pub same_cooldown_ms: u64,
    pub any_cooldown_ms: u64,
    pub binding_timeout_ms: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummonTriggerSnapshot {
    pub world_address: u64,
    pub epoch: u64,
    pub active_units: usize,
    pub pending_bindings: usize,
    /// Host's known retirement queue blocks new work; false is not a drain proof.
    pub retirement_pending: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerDenial {
    WrongPhase,
    RetirementPending,
    OperationPending,
    Capacity,
    AnyCooldown { remaining_ms: u64 },
    SameCooldown { remaining_ms: u64 },
    Quarantined,
}
pub enum SummonTriggerDecision {
    Rejected(TriggerDenial),
    Accepted(SummonTriggerTicket),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerAttemptStage {
    Reserved,
    Attempted,
    Uncertain,
    Quarantined,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingSummonTrigger {
    pub configuration_index: usize,
    pub requested_at: u64,
    pub sequence: u64,
    pub stage: TriggerAttemptStage,
    pub binding_wait: SummonCreationWait,
}
/// No Clone or native Drop. Losing this ticket retains the coordinator's reservation.
pub struct SummonTriggerTicket {
    owner: Rc<()>,
    sequence: u64,
    configuration_index: usize,
    requested_at: u64,
}
impl SummonTriggerTicket {
    pub fn configuration_index(&self) -> usize {
        self.configuration_index
    }
    pub fn requested_at(&self) -> u64 {
        self.requested_at
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummonTriggerOutcome {
    /// Host reports actual operation/postflight completion; not SDK proof of native guards.
    Completed,
    /// Host independently confirms no write or native dispatch occurred. Cooldown stays consumed.
    NotAttempted,
    /// Some work may have happened. Keep the reservation; timeout never clears it.
    Uncertain,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummonTriggerCompletion {
    pub configuration_index: usize,
    pub requested_at: u64,
    pub outcome: SummonTriggerOutcome,
}
/// One shared operation lane across configurations. Registered-unit records and their native
/// ownership remain with the host/SummonLifecycle, including hidden or quarantined units.
pub struct SummonTriggerCoordinator {
    owner: Rc<()>,
    world_address: u64,
    epoch: u64,
    policy: SummonTriggerPolicy,
    last_by_configuration: Vec<Option<u64>>,
    last_any: Option<u64>,
    last_time: u64,
    sequence: u64,
    pending: Option<PendingSummonTrigger>,
    quarantined: bool,
}
impl SummonTriggerCoordinator {
    pub fn new(
        world_address: u64,
        epoch: u64,
        policy: SummonTriggerPolicy,
        now: u64,
    ) -> Result<Self> {
        if world_address == 0
            || !world_address.is_multiple_of(8)
            || epoch == 0
            || policy.configuration_count > policy.configuration_limit
        {
            return Err(Error::InvalidProfile(
                "invalid summon trigger scope or configuration bound",
            ));
        }
        let mut last_by_configuration = Vec::new();
        last_by_configuration
            .try_reserve_exact(policy.configuration_count)
            .map_err(|_| Error::InvalidProfile("cannot reserve bounded summon trigger slots"))?;
        last_by_configuration.resize(policy.configuration_count, None);
        Ok(Self {
            owner: Rc::new(()),
            world_address,
            epoch,
            policy,
            last_by_configuration,
            last_any: None,
            last_time: now,
            sequence: 0,
            pending: None,
            quarantined: false,
        })
    }
    pub fn pending(&self) -> Option<PendingSummonTrigger> {
        self.pending
    }
    pub fn last_accepted(&self, configuration_index: usize) -> Result<Option<u64>> {
        self.last_by_configuration
            .get(configuration_index)
            .copied()
            .ok_or(Error::InvalidProfile(
                "unknown summon trigger configuration",
            ))
    }
    /// Host first consumes/observes semantic trigger effects, then presents configurations
    /// in its chosen order. Accepted triggers consume both cooldown clocks, including when
    /// the eventual reuse/spawn does no work. Counts include active and pending bindings;
    /// hidden units do not count, but their allocation records must still be retained.
    pub fn claim_trigger(
        &mut self,
        configuration_index: usize,
        phase: TaskPhase,
        now: u64,
        snapshot: SummonTriggerSnapshot,
    ) -> Result<SummonTriggerDecision> {
        self.check_time(now)?;
        if snapshot.world_address != self.world_address
            || snapshot.epoch != self.epoch
            || configuration_index >= self.last_by_configuration.len()
        {
            return Err(Error::NativeOperationRejected(
                "foreign summon trigger snapshot/configuration",
            ));
        }
        let count = snapshot
            .active_units
            .checked_add(snapshot.pending_bindings)
            .ok_or(Error::AddressOverflow)?;
        let denial = if self.quarantined {
            Some(TriggerDenial::Quarantined)
        } else if phase != TaskPhase::CHR_POST_PHYSICS {
            Some(TriggerDenial::WrongPhase)
        } else if snapshot.retirement_pending {
            Some(TriggerDenial::RetirementPending)
        } else if self.pending.is_some() {
            Some(TriggerDenial::OperationPending)
        } else if count >= self.policy.max_active {
            Some(TriggerDenial::Capacity)
        } else if let Some(remaining_ms) =
            remaining(self.last_any, self.policy.any_cooldown_ms, now)
        {
            Some(TriggerDenial::AnyCooldown { remaining_ms })
        } else {
            remaining(
                self.last_by_configuration[configuration_index],
                self.policy.same_cooldown_ms,
                now,
            )
            .map(|remaining_ms| TriggerDenial::SameCooldown { remaining_ms })
        };
        if let Some(denial) = denial {
            self.last_time = now;
            return Ok(SummonTriggerDecision::Rejected(denial));
        }
        let sequence = self.sequence.checked_add(1).ok_or(Error::AddressOverflow)?;
        let binding_wait = SummonCreationWait::new(now, self.policy.binding_timeout_ms)?;
        self.pending = Some(PendingSummonTrigger {
            configuration_index,
            requested_at: now,
            sequence,
            stage: TriggerAttemptStage::Reserved,
            binding_wait,
        });
        self.last_any = Some(now);
        self.last_by_configuration[configuration_index] = Some(now);
        self.last_time = now;
        self.sequence = sequence;
        Ok(SummonTriggerDecision::Accepted(SummonTriggerTicket {
            owner: self.owner.clone(),
            sequence,
            configuration_index,
            requested_at: now,
        }))
    }
    /// Mark immediately before host work; repeated marking, lost tickets and panic cannot
    /// reserve a second operation. This does not establish an engine phase or authorize a call.
    pub fn mark_attempted(&mut self, ticket: &SummonTriggerTicket, now: u64) -> Result<()> {
        self.check_time(now)?;
        self.check_ticket(ticket)?;
        let pending = self.pending.as_mut().expect("checked ticket");
        if pending.stage != TriggerAttemptStage::Reserved {
            return Err(Error::NativeOperationRejected(
                "summon trigger already attempted",
            ));
        }
        pending.stage = TriggerAttemptStage::Attempted;
        self.last_time = now;
        Ok(())
    }
    pub fn complete(
        &mut self,
        ticket: SummonTriggerTicket,
        now: u64,
        outcome: SummonTriggerOutcome,
    ) -> Result<SummonTriggerCompletion> {
        self.check_time(now)?;
        self.check_ticket(&ticket)?;
        let pending = self.pending.expect("checked ticket");
        if pending.stage != TriggerAttemptStage::Reserved
            && pending.stage != TriggerAttemptStage::Attempted
            || outcome != SummonTriggerOutcome::NotAttempted
                && pending.stage != TriggerAttemptStage::Attempted
        {
            return Err(Error::NativeOperationRejected(
                "summon trigger completion stage invalid",
            ));
        }
        if outcome == SummonTriggerOutcome::Uncertain {
            self.pending.as_mut().expect("checked ticket").stage = TriggerAttemptStage::Uncertain;
        } else {
            self.pending = None;
        }
        self.last_time = now;
        Ok(SummonTriggerCompletion {
            configuration_index: ticket.configuration_index,
            requested_at: ticket.requested_at,
            outcome,
        })
    }
    /// Expiration is diagnostic scheduling data only; no cancel/rollback/release occurs.
    pub fn pending_timed_out(&mut self, now: u64) -> Result<bool> {
        self.check_time(now)?;
        let expired = self
            .pending
            .map(|p| p.binding_wait.expired(now))
            .transpose()?
            .unwrap_or(false);
        self.last_time = now;
        Ok(expired)
    }
    /// Keep any outstanding request copy and invalidate its ticket after world/owner loss.
    pub fn quarantine(&mut self) {
        self.quarantined = true;
        if let Some(pending) = &mut self.pending {
            pending.stage = TriggerAttemptStage::Quarantined;
        }
    }
    fn check_time(&self, now: u64) -> Result<()> {
        if now < self.last_time {
            Err(Error::InvalidProfile(
                "summon trigger clock moved backwards",
            ))
        } else {
            Ok(())
        }
    }
    fn check_ticket(&self, ticket: &SummonTriggerTicket) -> Result<()> {
        if !Rc::ptr_eq(&self.owner, &ticket.owner)
            || self.pending.is_none_or(|p| {
                p.sequence != ticket.sequence
                    || p.configuration_index != ticket.configuration_index
                    || p.requested_at != ticket.requested_at
            })
        {
            return Err(Error::NativeOperationRejected(
                "foreign or superseded summon trigger ticket",
            ));
        }
        Ok(())
    }
}
fn remaining(last: Option<u64>, cooldown: u64, now: u64) -> Option<u64> {
    last.and_then(|last| cooldown.checked_sub(now - last))
        .filter(|remaining| *remaining != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        character_sets::CharacterIdentity, characters::EffectNodeObservation,
        effects::EffectApplication,
    };
    use std::cell::{Cell, RefCell};

    struct Effects {
        source: RefCell<Option<CharacterEffectsObservation>>,
        calls: RefCell<Vec<i32>>,
        fail_on: Cell<Option<usize>>,
        change_on: Cell<Option<usize>>,
        panic: Cell<bool>,
    }
    impl Effects {
        fn new(ids: &[i32]) -> Self {
            Self {
                source: RefCell::new(Some(CharacterEffectsObservation {
                    character: CharacterIdentity {
                        address: 0x2000,
                        world_address: 0x1000,
                        native_identifier: 0x1234000010000001,
                        epoch: 4,
                    },
                    container_address: 0x3000,
                    entries: ids
                        .iter()
                        .enumerate()
                        .map(|(index, &native_id)| EffectNodeObservation {
                            address: 0x4000 + index as u64 * 0x80,
                            native_id,
                        })
                        .collect(),
                })),
                calls: RefCell::new(Vec::new()),
                fail_on: Cell::new(None),
                change_on: Cell::new(None),
                panic: Cell::new(false),
            }
        }
    }
    impl SummonTriggerEffectBackend for Effects {
        fn primary_effects(&self) -> Result<Option<CharacterEffectsObservation>> {
            Ok(self.source.borrow().clone())
        }
    }
    // SAFETY: This original mock only changes owned Rust collections, never native storage.
    unsafe impl EffectEngine for Effects {
        unsafe fn apply(
            &self,
            _: CharacterIdentity,
            _: EffectId,
            _: bool,
        ) -> Result<EffectApplication> {
            panic!("trigger consumer must never apply effects")
        }
        unsafe fn remove(&self, target: CharacterIdentity, effect: EffectId) -> Result<bool> {
            self.calls.borrow_mut().push(effect.value());
            assert!(!self.panic.get(), "simulated dispatch panic");
            let mut source = self.source.borrow_mut();
            let source = source.as_mut().unwrap();
            assert_eq!(source.character, target);
            let index = source
                .entries
                .iter()
                .position(|node| node.native_id == effect.value());
            if let Some(index) = index {
                source.entries.remove(index);
            }
            if self.change_on.get() == Some(self.calls.borrow().len()) {
                source.character.native_identifier ^= 1_u64 << 32;
            }
            if self.fail_on.get() == Some(self.calls.borrow().len()) {
                return Err(Error::NativeOperationRejected("mock failure after unlink"));
            }
            Ok(index.is_some())
        }
    }
    fn effect_consumer(ids: &[i32]) -> SummonTriggerEffectConsumer {
        let ids: Vec<_> = ids.iter().map(|&id| EffectId::new(id).unwrap()).collect();
        SummonTriggerEffectConsumer::new(0x1000, 4, &ids, 16, 32).unwrap()
    }
    fn consume(
        consumer: &mut SummonTriggerEffectConsumer,
        backend: &Effects,
    ) -> Result<SummonTriggerEffectsReceipt> {
        // SAFETY: Mock graph and synchronous dispatch use only owned Rust storage.
        unsafe { consumer.consume(backend, TaskPhase::CHR_POST_PHYSICS) }
    }
    #[test]
    fn effects_preserve_configuration_order_aliases_and_complete_chain() {
        let backend = Effects::new(&[-1, 20, 10, 10, 99]);
        let mut consumer = effect_consumer(&[10, 30, 20, 10, 10]);
        let receipt = consume(&mut consumer, &backend).unwrap();
        assert!(receipt.completed);
        assert_eq!(receipt.triggered, [0, 2, 3, 4]);
        assert_eq!(*backend.calls.borrow(), [10, 20, 10, 10]);
        assert_eq!(
            receipt
                .attempts
                .iter()
                .map(|a| a.returned.clone().unwrap().unwrap())
                .collect::<Vec<_>>(),
            [true, true, true, false]
        );
        assert!(
            receipt
                .attempts
                .iter()
                .all(|a| a.postflight == Some(Ok(())))
        );
        assert!(consumer.inflight().is_none());
        assert_eq!(
            backend
                .source
                .borrow()
                .as_ref()
                .unwrap()
                .entries
                .iter()
                .map(|e| e.native_id)
                .collect::<Vec<_>>(),
            [-1, 99]
        );
        let second = consume(&mut consumer, &backend).unwrap();
        assert!(second.completed && second.triggered.is_empty());
        assert_eq!(backend.calls.borrow().len(), 4);
    }
    #[test]
    fn effects_keep_partial_failure_and_never_retry_or_dispatch_following_config() {
        let backend = Effects::new(&[10, 20, 30]);
        backend.fail_on.set(Some(2));
        let mut consumer = effect_consumer(&[10, 20, 30]);
        let receipt = consume(&mut consumer, &backend).unwrap();
        assert!(!receipt.completed && receipt.failure.is_some());
        assert_eq!(receipt.attempts.len(), 2);
        assert_eq!(receipt.attempts[0].postflight, Some(Ok(())));
        assert!(receipt.attempts[1].returned.as_ref().unwrap().is_err());
        assert_eq!(receipt.attempts[1].postflight, None);
        assert_eq!(consumer.inflight(), Some(&receipt));
        assert!(consume(&mut consumer, &backend).is_err());
        drop(consumer);
        assert_eq!(*backend.calls.borrow(), [10, 20]);
        assert_eq!(
            backend.source.borrow().as_ref().unwrap().entries[0].native_id,
            30
        );
    }
    #[test]
    fn effects_full_identity_change_after_unlink_retains_actual_attempt() {
        let backend = Effects::new(&[10, 20]);
        backend.change_on.set(Some(1));
        let mut consumer = effect_consumer(&[10, 20]);
        let receipt = consume(&mut consumer, &backend).unwrap();
        assert!(!receipt.completed);
        assert_eq!(receipt.attempts[0].returned, Some(Ok(true)));
        assert!(receipt.attempts[0].postflight.as_ref().unwrap().is_err());
        assert_eq!(consumer.inflight(), Some(&receipt));
        assert!(consume(&mut consumer, &backend).is_err());
        assert_eq!(*backend.calls.borrow(), [10]);
    }
    #[test]
    fn effects_record_attempt_before_panic_and_block_further_dispatch() {
        let backend = Effects::new(&[10]);
        backend.panic.set(true);
        let mut consumer = effect_consumer(&[10]);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            consume(&mut consumer, &backend)
        }));
        assert!(panic.is_err());
        let journal = consumer.inflight().unwrap();
        assert_eq!(journal.attempts.len(), 1);
        assert_eq!(journal.attempts[0].returned, None);
        assert!(consume(&mut consumer, &backend).is_err());
        assert_eq!(backend.calls.borrow().len(), 1);
    }
    #[test]
    fn effects_reject_wrong_phase_scope_bounds_nodes_and_keep_missing_player_empty() {
        let backend = Effects::new(&[10]);
        let mut consumer = effect_consumer(&[10]);
        // SAFETY: Wrong-phase rejection executes no backend/native work; mock storage only.
        assert!(unsafe { consumer.consume(&backend, TaskPhase::FRAME_BEGIN) }.is_err());
        assert!(backend.calls.borrow().is_empty());
        let initial = backend.source.borrow().clone();
        for mutation in 0..5 {
            *backend.source.borrow_mut() = initial.clone();
            {
                let mut source = backend.source.borrow_mut();
                let source = source.as_mut().unwrap();
                match mutation {
                    0 => source.character.world_address += 8,
                    1 => source.character.epoch += 1,
                    2 => source.container_address += 1,
                    3 => source.entries[0].address = 0,
                    _ => source.entries.push(source.entries[0]),
                }
            }
            assert!(consume(&mut consumer, &backend).is_err());
            assert!(consumer.inflight().is_none() && backend.calls.borrow().is_empty());
        }
        *backend.source.borrow_mut() = initial;
        let mut too_small =
            SummonTriggerEffectConsumer::new(0x1000, 4, &[EffectId::new(10).unwrap()], 1, 1)
                .unwrap();
        backend
            .source
            .borrow_mut()
            .as_mut()
            .unwrap()
            .entries
            .push(EffectNodeObservation {
                address: 0x5000,
                native_id: 20,
            });
        assert!(consume(&mut too_small, &backend).is_err());
        *backend.source.borrow_mut() = None;
        let receipt = consume(&mut consumer, &backend).unwrap();
        assert!(receipt.completed && receipt.source.is_none() && receipt.triggered.is_empty());
        assert!(consumer.inflight().is_none() && backend.calls.borrow().is_empty());
        let id = EffectId::new(10).unwrap();
        assert!(SummonTriggerEffectConsumer::new(0, 4, &[id], 1, 1).is_err());
        assert!(SummonTriggerEffectConsumer::new(0x1000, 4, &[id], 0, 1).is_err());
        assert!(SummonTriggerEffectConsumer::new(0x1000, 0, &[id], 1, 1).is_err());
        assert!(SummonTriggerEffectConsumer::new(0x1000, 4, &[id], 1, 0).is_err());
    }

    fn policy() -> SummonTriggerPolicy {
        SummonTriggerPolicy {
            configuration_count: 2,
            configuration_limit: 2,
            max_active: 3,
            same_cooldown_ms: 100,
            any_cooldown_ms: 20,
            binding_timeout_ms: 50,
        }
    }
    fn snapshot() -> SummonTriggerSnapshot {
        SummonTriggerSnapshot {
            world_address: 0x1000,
            epoch: 4,
            active_units: 0,
            pending_bindings: 0,
            retirement_pending: false,
        }
    }
    fn coordinator() -> SummonTriggerCoordinator {
        SummonTriggerCoordinator::new(0x1000, 4, policy(), 0).unwrap()
    }
    fn accept(c: &mut SummonTriggerCoordinator, index: usize, now: u64) -> SummonTriggerTicket {
        match c
            .claim_trigger(index, TaskPhase::CHR_POST_PHYSICS, now, snapshot())
            .unwrap()
        {
            SummonTriggerDecision::Accepted(ticket) => ticket,
            SummonTriggerDecision::Rejected(reason) => panic!("unexpected denial: {reason:?}"),
        }
    }
    fn denial(decision: SummonTriggerDecision) -> TriggerDenial {
        match decision {
            SummonTriggerDecision::Rejected(reason) => reason,
            SummonTriggerDecision::Accepted(_) => panic!("expected denial"),
        }
    }
    #[test]
    fn capacity_includes_pending_and_admission_requires_phase_and_no_retirement() {
        let mut c = coordinator();
        let mut state = snapshot();
        state.active_units = 1;
        state.pending_bindings = 2;
        assert_eq!(
            denial(
                c.claim_trigger(0, TaskPhase::CHR_POST_PHYSICS, 1, state)
                    .unwrap()
            ),
            TriggerDenial::Capacity
        );
        assert_eq!(
            denial(
                c.claim_trigger(0, TaskPhase::FRAME_END, 2, snapshot())
                    .unwrap()
            ),
            TriggerDenial::WrongPhase
        );
        state = snapshot();
        state.retirement_pending = true;
        assert_eq!(
            denial(
                c.claim_trigger(0, TaskPhase::CHR_POST_PHYSICS, 3, state)
                    .unwrap()
            ),
            TriggerDenial::RetirementPending
        );
        assert_eq!(c.last_accepted(0).unwrap(), None);
        assert_eq!(c.last_any, None);
        assert!(c.pending().is_none());
        let mut disabled = policy();
        disabled.max_active = 0;
        let mut disabled = SummonTriggerCoordinator::new(0x1000, 4, disabled, 0).unwrap();
        assert_eq!(
            denial(
                disabled
                    .claim_trigger(0, TaskPhase::CHR_POST_PHYSICS, 0, snapshot())
                    .unwrap()
            ),
            TriggerDenial::Capacity
        );
        let ticket = accept(&mut c, 0, 3);
        assert_eq!(ticket.requested_at(), 3);
    }
    #[test]
    fn clocks_are_consumed_by_accepted_no_work_and_allow_exact_boundaries() {
        let mut c = coordinator();
        let ticket = accept(&mut c, 0, 100);
        c.complete(ticket, 100, SummonTriggerOutcome::NotAttempted)
            .unwrap();
        assert_eq!(
            denial(
                c.claim_trigger(1, TaskPhase::CHR_POST_PHYSICS, 119, snapshot())
                    .unwrap()
            ),
            TriggerDenial::AnyCooldown { remaining_ms: 1 }
        );
        let ticket = accept(&mut c, 1, 120);
        c.complete(ticket, 120, SummonTriggerOutcome::NotAttempted)
            .unwrap();
        assert_eq!(
            denial(
                c.claim_trigger(0, TaskPhase::CHR_POST_PHYSICS, 140, snapshot())
                    .unwrap()
            ),
            TriggerDenial::SameCooldown { remaining_ms: 60 }
        );
        let ticket = accept(&mut c, 0, 200);
        assert_eq!(ticket.configuration_index(), 0);
        c.complete(ticket, 200, SummonTriggerOutcome::NotAttempted)
            .unwrap();
        assert_eq!(
            denial(
                c.claim_trigger(1, TaskPhase::CHR_POST_PHYSICS, 200, snapshot())
                    .unwrap()
            ),
            TriggerDenial::AnyCooldown { remaining_ms: 20 }
        );
        assert_eq!(c.last_accepted(1).unwrap(), Some(120));
    }
    #[test]
    fn one_lane_survives_lost_ticket_and_timeout_is_never_cancellation() {
        let mut c = coordinator();
        {
            let ticket = accept(&mut c, 0, 10);
            c.mark_attempted(&ticket, 10).unwrap();
            assert!(c.mark_attempted(&ticket, 10).is_err());
        }
        assert!(!c.pending_timed_out(59).unwrap());
        assert!(c.pending_timed_out(60).unwrap());
        assert_eq!(
            denial(
                c.claim_trigger(1, TaskPhase::CHR_POST_PHYSICS, 10_000, snapshot())
                    .unwrap()
            ),
            TriggerDenial::OperationPending
        );
        let pending = c.pending().unwrap();
        assert_eq!(pending.stage, TriggerAttemptStage::Attempted);
        assert_eq!(pending.requested_at, 10);
        assert_eq!(c.last_accepted(1).unwrap(), None);
    }
    #[test]
    fn uncertain_completion_retains_copy_and_quarantine_invalidates_tickets() {
        let mut c = coordinator();
        let ticket = accept(&mut c, 0, 10);
        c.mark_attempted(&ticket, 11).unwrap();
        let result = c
            .complete(ticket, 12, SummonTriggerOutcome::Uncertain)
            .unwrap();
        assert_eq!(result.requested_at, 10);
        assert_eq!(c.pending().unwrap().stage, TriggerAttemptStage::Uncertain);
        c.quarantine();
        assert_eq!(c.pending().unwrap().stage, TriggerAttemptStage::Quarantined);
        assert_eq!(
            denial(
                c.claim_trigger(0, TaskPhase::CHR_POST_PHYSICS, 100, snapshot())
                    .unwrap()
            ),
            TriggerDenial::Quarantined
        );
        let mut other = coordinator();
        let ticket = accept(&mut other, 1, 0);
        other.quarantine();
        assert!(other.mark_attempted(&ticket, 1).is_err());
        assert!(
            other
                .complete(ticket, 1, SummonTriggerOutcome::NotAttempted)
                .is_err()
        );
        assert!(other.pending().is_some());
    }
    #[test]
    fn same_scope_foreign_ticket_and_bad_inputs_do_not_change_reservation() {
        let mut c = coordinator();
        let ticket = accept(&mut c, 0, 10);
        let mut other = coordinator();
        let foreign = accept(&mut other, 0, 10);
        let before = c.pending();
        assert!(c.mark_attempted(&foreign, 10).is_err());
        assert!(
            c.complete(foreign, 10, SummonTriggerOutcome::NotAttempted)
                .is_err()
        );
        assert_eq!(c.pending(), before);
        assert!(c.mark_attempted(&ticket, 9).is_err());
        for state in [
            SummonTriggerSnapshot {
                world_address: 0x2000,
                ..snapshot()
            },
            SummonTriggerSnapshot {
                epoch: 5,
                ..snapshot()
            },
        ] {
            assert!(
                c.claim_trigger(0, TaskPhase::CHR_POST_PHYSICS, 11, state)
                    .is_err()
            );
        }
        assert!(
            c.claim_trigger(2, TaskPhase::CHR_POST_PHYSICS, 11, snapshot())
                .is_err()
        );
        assert_eq!(c.pending(), before);
        assert_eq!(c.last_time, 10);
        c.mark_attempted(&ticket, 11).unwrap();
        c.complete(ticket, 12, SummonTriggerOutcome::Completed)
            .unwrap();
        assert!(other.pending().is_some());
    }
    #[test]
    fn delayed_completion_preserves_request_time_and_does_not_restart_lifetime() {
        use crate::{
            character_sets::{CharacterIdentity, CharacterRegistrationObservation},
            summon_lifecycle::{
                SummonLifecycle, SummonLifetime, SummonPolicy, SummonRemovalMode, SummonStage,
            },
        };
        let mut c = coordinator();
        let ticket = accept(&mut c, 0, 100);
        c.mark_attempted(&ticket, 101).unwrap();
        assert!(c.pending_timed_out(150).unwrap());
        let completed = c
            .complete(ticket, 10_100, SummonTriggerOutcome::Completed)
            .unwrap();
        let registration = CharacterRegistrationObservation {
            character: CharacterIdentity {
                address: 0x2000,
                world_address: 0x1000,
                native_identifier: 9,
                epoch: 4,
            },
            set_address: 0x3000,
            entry_address: 0x4000,
            ordinal: 0,
        };
        let unit = SummonLifecycle::new(
            registration,
            SummonPolicy {
                lifetime: SummonLifetime::default(),
                removal: SummonRemovalMode::Hide,
                reuse_hidden: true,
                destroy_delay_ms: 50,
            },
            completed.requested_at,
            10_100,
        )
        .unwrap();
        assert_eq!(unit.stage(), SummonStage::CleanupPending);
        assert_eq!(unit.expires_at(), Some(10_100));
        assert!(c.pending().is_none());
    }
    #[test]
    fn overflow_rejects_before_publishing_cooldowns_or_reservations() {
        let mut c = coordinator();
        c.sequence = u64::MAX;
        assert!(
            c.claim_trigger(0, TaskPhase::CHR_POST_PHYSICS, 1, snapshot())
                .is_err()
        );
        assert!(c.pending().is_none());
        assert_eq!(c.last_any, None);
        assert_eq!(c.last_time, 0);
        c.sequence = 0;
        let state = SummonTriggerSnapshot {
            active_units: usize::MAX,
            pending_bindings: 1,
            ..snapshot()
        };
        assert!(
            c.claim_trigger(0, TaskPhase::CHR_POST_PHYSICS, 1, state)
                .is_err()
        );
        assert!(
            c.claim_trigger(0, TaskPhase::CHR_POST_PHYSICS, u64::MAX, snapshot())
                .is_err()
        );
        assert_eq!(c.last_accepted(0).unwrap(), None);
        assert_eq!(c.last_time, 0);
        assert!(SummonTriggerCoordinator::new(0, 4, policy(), 0).is_err());
        assert!(SummonTriggerCoordinator::new(0x1000, 0, policy(), 0).is_err());
        assert!(
            SummonTriggerCoordinator::new(
                0x1000,
                4,
                SummonTriggerPolicy {
                    configuration_limit: 1,
                    ..policy()
                },
                0
            )
            .is_err()
        );
    }
}
