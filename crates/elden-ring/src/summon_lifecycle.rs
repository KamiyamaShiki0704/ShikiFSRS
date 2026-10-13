//! Offline lifecycle bookkeeping for a registered summon. Intents never call the game,
//! acquire ownership/guards, prove render state, or authorize native destruction/free.
use crate::{TaskPhase, character_sets::CharacterRegistrationObservation};
use shikifsrs_runtime::{Error, Result};
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummonLifetime {
    Infinite,
    Milliseconds(u64),
}
impl Default for SummonLifetime {
    fn default() -> Self {
        Self::Milliseconds(10_000)
    }
}
impl SummonLifetime {
    /// Consumer configuration contract: only -1 is infinite; zero/other negatives become 1ms.
    pub fn from_config_millis(value: i64) -> Self {
        if value == -1 {
            Self::Infinite
        } else {
            Self::Milliseconds(value.max(1) as u64)
        }
    }
    fn deadline(self, requested_at: u64) -> Result<Option<u64>> {
        match self {
            Self::Infinite => Ok(None),
            Self::Milliseconds(value) => requested_at
                .checked_add(value.max(1))
                .map(Some)
                .ok_or(Error::AddressOverflow),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummonRemovalMode {
    Hide,
    Destroy,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummonPolicy {
    pub lifetime: SummonLifetime,
    pub removal: SummonRemovalMode,
    pub reuse_hidden: bool,
    /// Earliest bookkeeping eligibility only. Elapsed time never proves quiescence.
    pub destroy_delay_ms: u64,
}
impl SummonPolicy {
    pub fn reuses_units(self) -> bool {
        self.removal == SummonRemovalMode::Hide && self.reuse_hidden
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummonStage {
    Active,
    CleanupPending,
    CleanupAttempted,
    HiddenReusable,
    HiddenRetained,
    ReactivationAttempted,
    RetirementDelay,
    RetirementAttempted,
    Retired,
    Uncertain,
    Quarantined,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummonRemovalReason {
    LifetimeExpired,
    VanishRequested,
    Dead,
    ExplicitCleanup,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummonActionKind {
    Hide { retain_for_reuse: bool },
    StageRetirement,
    Reactivate,
    Retire,
}
/// Non-cloneable bookkeeping ticket. Its Drop never invokes cleanup or resets an attempt.
/// It grants no engine lifetime, phase, reference drain, allocator or free authority.
pub struct SummonAction {
    owner: Rc<()>,
    sequence: u64,
    kind: SummonActionKind,
    claimed_at: u64,
    source: CharacterRegistrationObservation,
}
impl SummonAction {
    pub fn kind(&self) -> SummonActionKind {
        self.kind
    }
    pub fn source(&self) -> CharacterRegistrationObservation {
        self.source
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummonActionOutcome {
    /// Host records successful operation/postflight. This is not SDK proof of native guards.
    Confirmed,
    /// Host independently confirms no write/native call happened. A fresh intent may be claimed.
    NotAttempted,
    /// Some work may have occurred. Retain receipts/resources; no automatic retry or reuse.
    Uncertain,
}
/// One registered unit, retained even after hide, failure or quarantine. Non-cloneable and
/// thread confined; every actual native graph/lifetime obligation remains with the host.
pub struct SummonLifecycle {
    owner: Rc<()>,
    source: CharacterRegistrationObservation,
    policy: SummonPolicy,
    stage: SummonStage,
    requested_at: u64,
    expires_at: Option<u64>,
    retirement_at: Option<u64>,
    last_time: u64,
    reason: Option<SummonRemovalReason>,
    sequence: u64,
    pending: Option<(u64, SummonActionKind)>,
}
impl SummonLifecycle {
    /// Supply elapsed monotonic milliseconds from one clock origin, not wall-clock time.
    /// A late registered result is retained and immediately needs cleanup if its original
    /// lifetime expired. The copied registration is not dereferenced or treated as an owner.
    pub fn new(
        source: CharacterRegistrationObservation,
        policy: SummonPolicy,
        requested_at: u64,
        now: u64,
    ) -> Result<Self> {
        if source.character.epoch == 0
            || source.character.address == 0
            || source.character.world_address == 0
            || source.set_address == 0
            || source.entry_address == 0
            || now < requested_at
        {
            return Err(Error::InvalidProfile(
                "invalid summon registration or clock",
            ));
        }
        let expires_at = policy.lifetime.deadline(requested_at)?;
        let expired = expires_at.is_some_and(|deadline| now >= deadline);
        Ok(Self {
            owner: Rc::new(()),
            source,
            policy,
            stage: if expired {
                SummonStage::CleanupPending
            } else {
                SummonStage::Active
            },
            requested_at,
            expires_at,
            retirement_at: None,
            last_time: now,
            reason: expired.then_some(SummonRemovalReason::LifetimeExpired),
            sequence: 0,
            pending: None,
        })
    }
    pub fn source(&self) -> CharacterRegistrationObservation {
        self.source
    }
    pub fn stage(&self) -> SummonStage {
        self.stage
    }
    pub fn requested_at(&self) -> u64 {
        self.requested_at
    }
    pub fn expires_at(&self) -> Option<u64> {
        self.expires_at
    }
    pub fn retirement_at(&self) -> Option<u64> {
        self.retirement_at
    }
    pub fn removal_reason(&self) -> Option<SummonRemovalReason> {
        self.reason
    }
    /// Observe lifetime or an explicit semantic removal input. Does not invoke a backend.
    pub fn advance(&mut self, now: u64, removal: Option<SummonRemovalReason>) -> Result<()> {
        self.check_time(now)?;
        if self.stage == SummonStage::Active {
            let reason = removal.or_else(|| {
                self.expires_at
                    .filter(|&deadline| now >= deadline)
                    .map(|_| SummonRemovalReason::LifetimeExpired)
            });
            if let Some(reason) = reason {
                self.reason = Some(reason);
                self.stage = SummonStage::CleanupPending;
            }
        }
        self.last_time = now;
        Ok(())
    }
    /// Claim one explicit intent. Stage changes before any eventual backend operation,
    /// so panic, lost ticket, failed acknowledgement and Drop cannot generate a duplicate.
    /// Retire is eligible only in FrameEnd after cleanup plus delay; eligibility is not a
    /// destruction barrier. Actual native adapters still require all their unsafe contracts.
    pub fn claim_action(&mut self, phase: TaskPhase, now: u64) -> Result<Option<SummonAction>> {
        self.check_time(now)?;
        let action = match (self.stage, phase) {
            (SummonStage::CleanupPending, TaskPhase::CHR_POST_PHYSICS) => {
                Some(match self.policy.removal {
                    SummonRemovalMode::Hide => SummonActionKind::Hide {
                        retain_for_reuse: self.policy.reuses_units(),
                    },
                    SummonRemovalMode::Destroy => SummonActionKind::StageRetirement,
                })
            }
            (SummonStage::RetirementDelay, TaskPhase::FRAME_END)
                if self.retirement_at.is_some_and(|at| now >= at) =>
            {
                Some(SummonActionKind::Retire)
            }
            _ => None,
        };
        let Some(kind) = action else {
            self.last_time = now;
            return Ok(None);
        };
        self.claim(kind, now).map(Some)
    }
    /// Reuse is an explicit operation on a confirmed hidden unit. Never permitted after
    /// retirement started, failed cleanup, missing identity, quarantine or destroyed storage.
    pub fn claim_reactivation(&mut self, now: u64) -> Result<SummonAction> {
        self.check_time(now)?;
        if self.stage != SummonStage::HiddenReusable || !self.policy.reuses_units() {
            return Err(Error::NativeOperationRejected("summon is not reusable"));
        }
        self.policy.lifetime.deadline(now)?;
        self.claim(SummonActionKind::Reactivate, now)
    }
    /// Host reports an outcome for this exact claimed ticket. Confirmed is bookkeeping only;
    /// backend receipts/native ownership/quiescence must be independently established.
    /// A failed/partial operation stays uncertain; elapsed frames never erase that obligation.
    pub fn complete_action(
        &mut self,
        action: SummonAction,
        now: u64,
        outcome: SummonActionOutcome,
    ) -> Result<()> {
        self.check_time(now)?;
        if !Rc::ptr_eq(&self.owner, &action.owner)
            || self.pending != Some((action.sequence, action.kind))
            || action.source != self.source
        {
            return Err(Error::NativeOperationRejected(
                "foreign or superseded summon ticket",
            ));
        }
        let (stage, retirement_at, expires_at) = match outcome {
            SummonActionOutcome::Uncertain => {
                (SummonStage::Uncertain, self.retirement_at, self.expires_at)
            }
            SummonActionOutcome::NotAttempted => (
                match action.kind {
                    SummonActionKind::Hide { .. } | SummonActionKind::StageRetirement => {
                        SummonStage::CleanupPending
                    }
                    SummonActionKind::Reactivate => SummonStage::HiddenReusable,
                    SummonActionKind::Retire => SummonStage::RetirementDelay,
                },
                self.retirement_at,
                self.expires_at,
            ),
            SummonActionOutcome::Confirmed => match action.kind {
                SummonActionKind::Hide { retain_for_reuse } => (
                    if retain_for_reuse {
                        SummonStage::HiddenReusable
                    } else {
                        SummonStage::HiddenRetained
                    },
                    None,
                    self.expires_at,
                ),
                SummonActionKind::StageRetirement => (
                    SummonStage::RetirementDelay,
                    Some(
                        now.checked_add(self.policy.destroy_delay_ms)
                            .ok_or(Error::AddressOverflow)?,
                    ),
                    self.expires_at,
                ),
                SummonActionKind::Reactivate => (
                    SummonStage::Active,
                    None,
                    self.policy.lifetime.deadline(action.claimed_at)?,
                ),
                SummonActionKind::Retire => {
                    (SummonStage::Retired, self.retirement_at, self.expires_at)
                }
            },
        };
        self.stage = stage;
        self.retirement_at = retirement_at;
        self.expires_at = expires_at;
        if action.kind == SummonActionKind::Reactivate && outcome == SummonActionOutcome::Confirmed
        {
            self.requested_at = action.claimed_at;
            self.reason = None;
            // Slow acknowledgement may arrive after the renewed lifetime already expired.
            if expires_at.is_some_and(|at| now >= at) {
                self.stage = SummonStage::CleanupPending;
                self.reason = Some(SummonRemovalReason::LifetimeExpired);
            }
        }
        self.pending = None;
        self.last_time = now;
        Ok(())
    }
    /// World/target/owner loss retains the copied record but invalidates every outstanding
    /// ticket. Does not attempt native reads, cleanup or free on an inaccessible graph.
    pub fn quarantine(&mut self) {
        self.stage = SummonStage::Quarantined;
        self.pending = None;
    }
    fn check_time(&self, now: u64) -> Result<()> {
        if now < self.last_time {
            return Err(Error::InvalidProfile("summon clock moved backwards"));
        }
        Ok(())
    }
    fn claim(&mut self, kind: SummonActionKind, now: u64) -> Result<SummonAction> {
        let sequence = self.sequence.checked_add(1).ok_or(Error::AddressOverflow)?;
        if self.pending.is_some() {
            return Err(Error::NativeOperationRejected(
                "summon action already claimed",
            ));
        }
        self.sequence = sequence;
        self.pending = Some((sequence, kind));
        self.stage = match kind {
            SummonActionKind::Hide { .. } | SummonActionKind::StageRetirement => {
                SummonStage::CleanupAttempted
            }
            SummonActionKind::Reactivate => SummonStage::ReactivationAttempted,
            SummonActionKind::Retire => SummonStage::RetirementAttempted,
        };
        self.last_time = now;
        Ok(SummonAction {
            owner: self.owner.clone(),
            sequence,
            kind,
            claimed_at: now,
            source: self.source,
        })
    }
}

/// Deadline for a creation request whose registration has not been admitted. Timeout is a
/// scheduling outcome, never proof of cancellation, allocation absence or safe resource release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummonCreationWait {
    requested_at: u64,
    timeout_at: u64,
}
impl SummonCreationWait {
    pub fn new(requested_at: u64, timeout_ms: u64) -> Result<Self> {
        Ok(Self {
            requested_at,
            timeout_at: requested_at
                .checked_add(timeout_ms.max(1))
                .ok_or(Error::AddressOverflow)?,
        })
    }
    pub fn expired(self, now: u64) -> Result<bool> {
        if now < self.requested_at {
            return Err(Error::InvalidProfile("creation clock moved backwards"));
        }
        Ok(now >= self.timeout_at)
    }
}

#[cfg(test)]
mod tests;
