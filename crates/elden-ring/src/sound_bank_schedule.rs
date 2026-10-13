//! Consumer startup and conditional demands share explicit native reference accounting.
//! This scheduler is not a bank-file resolver, audio-ready receipt or task-thread host.
use crate::sound_banks::{
    BankCloseProgress, BankLoadUncertainty, BankObservation, BankRelease, BankRequest,
    SoundBankEngine, SoundBankOwner,
};
use shikifsrs_runtime::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};

pub trait BankAssetSource {
    /// Owned synchronous bytes; error precedes any native bank call. Consumer owns
    /// search directories, filesystem policy and file-format validation.
    fn read_bank(&mut self, configured_id: u32) -> Result<Vec<u8>>;
}
#[derive(Debug, Clone)]
pub struct SoundBankScheduleConfig {
    pub enabled: bool,
    pub static_ids: Vec<u32>,
    pub start_delay_ms: u64,
    pub attempts_per_id: u32,
    pub retry_every_frames: u32,
    pub memory_copy_enabled: bool,
    pub conditional_enabled: bool,
    pub poll_every_frames: u32,
    pub unload_when_inactive: bool,
}
impl Default for SoundBankScheduleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            static_ids: Vec::new(),
            start_delay_ms: 5000,
            attempts_per_id: 120,
            retry_every_frames: 60,
            memory_copy_enabled: true,
            conditional_enabled: false,
            poll_every_frames: 6,
            unload_when_inactive: true,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupBankState {
    Pending,
    Disabled,
    Exhausted,
    Attempted(BankRequest),
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupBankSubmission {
    pub ordinal: usize,
    pub configured_id: u32,
    pub attempts: u32,
    /// Attempted can represent acquired, external or uncertain ownership. Never ready.
    pub state: StartupBankState,
    pub last_error: Option<Error>,
}
#[derive(Debug, Clone)]
pub struct SoundBankScheduleReport {
    pub startup: Vec<StartupBankSubmission>,
    pub conditional: BTreeMap<u32, BankRequest>,
    pub observations: Vec<BankObservation>,
    pub uncertain_loads: Vec<(BankRequest, BankLoadUncertainty)>,
    pub blocked: bool,
    pub closing: bool,
    /// Definitely pre-invocation errors; attempted uncertainty remains above.
    pub failures: Vec<(u32, Error)>,
}

pub struct SoundBankSchedule {
    config: SoundBankScheduleConfig,
    owner: u64,
    deadline_ms: u64,
    last_ms: u64,
    startup_frame: u64,
    next_startup_frame: u64,
    conditional_frame: u64,
    startup: Vec<StartupBankSubmission>,
    conditional: BTreeMap<u32, BankRequest>,
    closing: bool,
}
impl SoundBankSchedule {
    pub fn new<E: SoundBankEngine>(
        config: SoundBankScheduleConfig,
        started_ms: u64,
        owner: &SoundBankOwner<E>,
    ) -> Result<Self> {
        if config.static_ids.len() > 256
            || config.attempts_per_id == 0
            || config.retry_every_frames == 0
            || config.poll_every_frames == 0
            || owner.is_closing()
        {
            return Err(Error::InvalidProfile(
                "invalid sound schedule bounds or closing owner",
            ));
        }
        let deadline_ms = started_ms
            .checked_add(config.start_delay_ms)
            .ok_or(Error::AddressOverflow)?;
        let startup = config
            .static_ids
            .iter()
            .copied()
            .enumerate()
            .map(|(ordinal, configured_id)| StartupBankSubmission {
                ordinal,
                configured_id,
                attempts: 0,
                state: if config.enabled {
                    StartupBankState::Pending
                } else {
                    StartupBankState::Disabled
                },
                last_error: None,
            })
            .collect();
        Ok(Self {
            config,
            owner: owner.identity(),
            deadline_ms,
            last_ms: started_ms,
            startup_frame: 0,
            next_startup_frame: 1,
            conditional_frame: 0,
            startup,
            conditional: BTreeMap::new(),
            closing: false,
        })
    }
    fn check<E: SoundBankEngine>(&self, owner: &SoundBankOwner<E>) -> Result<()> {
        if owner.identity() != self.owner {
            return Err(Error::InvalidProfile("sound schedule has foreign owner"));
        }
        if self.closing || owner.is_closing() {
            return Err(Error::NativeOperationRejected("sound schedule is closing"));
        }
        Ok(())
    }
    pub fn report<E: SoundBankEngine>(
        &self,
        owner: &SoundBankOwner<E>,
    ) -> Result<SoundBankScheduleReport> {
        if owner.identity() != self.owner {
            return Err(Error::InvalidProfile("sound schedule has foreign owner"));
        }
        Ok(self.capture(owner, Vec::new()))
    }
    fn capture<E: SoundBankEngine>(
        &self,
        owner: &SoundBankOwner<E>,
        failures: Vec<(u32, Error)>,
    ) -> SoundBankScheduleReport {
        SoundBankScheduleReport {
            startup: self.startup.clone(),
            conditional: self.conditional.clone(),
            observations: owner.observations(),
            uncertain_loads: owner.uncertain_loads(),
            blocked: owner.is_uncertain(),
            closing: self.closing || owner.is_closing(),
            failures,
        }
    }
    /// Called at the consumer's startup SoundStep cadence, distinct from condition
    /// FrameBegin cadence. Interval1 works every frame; immutable acquired demands
    /// remain until explicit close. Errors before invocation consume the retry budget.
    /// # Safety
    /// Caller holds owner/engine's live permitted loading phase, synchronization and
    /// complete allocator/manager/module/bank residency. No native owner crosses threads.
    pub unsafe fn startup_frame<E: SoundBankEngine, S: BankAssetSource>(
        &mut self,
        now_ms: u64,
        owner: &mut SoundBankOwner<E>,
        source: &mut S,
    ) -> Result<SoundBankScheduleReport> {
        self.check(owner)?;
        if now_ms < self.last_ms {
            return Err(Error::InvalidProfile("sound startup clock moved backwards"));
        }
        self.last_ms = now_ms;
        if owner.is_uncertain()
            || now_ms < self.deadline_ms
            || !self
                .startup
                .iter()
                .any(|entry| entry.state == StartupBankState::Pending)
        {
            return Ok(self.capture(owner, Vec::new()));
        }
        let frame = self
            .startup_frame
            .checked_add(1)
            .ok_or(Error::AddressOverflow)?;
        if frame < self.next_startup_frame {
            self.startup_frame = frame;
            return Ok(self.capture(owner, Vec::new()));
        }
        let next = frame
            .checked_add(u64::from(self.config.retry_every_frames))
            .ok_or(Error::AddressOverflow)?;
        self.startup_frame = frame;
        self.next_startup_frame = next;
        let mut failures = Vec::new();
        for entry in &mut self.startup {
            if entry.state != StartupBankState::Pending {
                continue;
            }
            entry.attempts += 1;
            let result = if !self.config.memory_copy_enabled {
                Err(Error::NativeOperationRejected(
                    "memory-copy loading is disabled",
                ))
            } else {
                source.read_bank(entry.configured_id).and_then(|bytes| {
                    // SAFETY: This frame retains owner's synchronous-copy phase/residency contract.
                    unsafe { owner.acquire(&bytes) }
                })
            };
            match result {
                Ok(request) => {
                    entry.last_error = None;
                    entry.state = StartupBankState::Attempted(request);
                }
                Err(error) => {
                    entry.last_error = Some(error.clone());
                    failures.push((entry.configured_id, error));
                    if entry.attempts >= self.config.attempts_per_id {
                        entry.state = StartupBankState::Exhausted;
                    }
                }
            }
            if owner.is_uncertain() {
                break;
            }
        }
        Ok(self.capture(owner, failures))
    }
    /// Called on FrameBegin independently of startup delay. The producer runs only
    /// on each configured poll. Err or None means unavailable observation: retain
    /// all demands. Some(empty) is a verified no-demand observation. Loading and
    /// unloading never operate on an incomplete condition snapshot.
    /// # Safety
    /// Caller holds valid observation graph plus loading/unloading engine phases,
    /// original bank/owner identity and residency. Assets are synchronously copied.
    pub unsafe fn conditional_frame<E: SoundBankEngine, S: BankAssetSource>(
        &mut self,
        owner: &mut SoundBankOwner<E>,
        source: &mut S,
        observe: impl FnOnce() -> Result<Option<BTreeSet<u32>>>,
    ) -> Result<SoundBankScheduleReport> {
        self.check(owner)?;
        if !self.config.enabled
            || !self.config.conditional_enabled
            || !self.config.memory_copy_enabled
            || owner.is_uncertain()
        {
            return Ok(self.capture(owner, Vec::new()));
        }
        let frame = self
            .conditional_frame
            .checked_add(1)
            .ok_or(Error::AddressOverflow)?;
        self.conditional_frame = frame;
        if !frame.is_multiple_of(u64::from(self.config.poll_every_frames)) {
            return Ok(self.capture(owner, Vec::new()));
        }
        let Some(targets) = observe()? else {
            return Ok(self.capture(owner, Vec::new()));
        };
        if targets.len() > 1024 {
            return Err(Error::InvalidProfile("too many conditional bank targets"));
        }
        // An independently reconciled NoReference or external close may remove a
        // request; stale tickets must never suppress a new active demand indefinitely.
        self.conditional
            .retain(|_, request| owner.contains_request(*request));
        let mut failures = Vec::new();
        if self.config.unload_when_inactive {
            let inactive: Vec<_> = self
                .conditional
                .iter()
                .filter(|(id, _)| !targets.contains(id))
                .map(|(id, request)| (*id, *request))
                .collect();
            for (id, request) in inactive {
                // SAFETY: Caller retains this request's original unloading obligation and phase.
                match unsafe { owner.release(request) } {
                    Ok(BankRelease::RetryLater | BankRelease::Uncertain) => {}
                    Ok(_) => {
                        self.conditional.remove(&id);
                    }
                    Err(error) => failures.push((id, error)),
                }
                if owner.is_uncertain() {
                    return Ok(self.capture(owner, failures));
                }
            }
        }
        for id in targets {
            if self.conditional.contains_key(&id) {
                continue;
            }
            let result = source.read_bank(id).and_then(|bytes| {
                // SAFETY: Same live synchronous memory-copy loading phase, no retained byte lease.
                unsafe { owner.acquire(&bytes) }
            });
            match result {
                Ok(request) => {
                    self.conditional.insert(id, request);
                }
                Err(error) => failures.push((id, error)),
            }
            if owner.is_uncertain() {
                break;
            }
        }
        Ok(self.capture(owner, failures))
    }
    /// Close acquisition before explicitly releasing every startup/conditional
    /// demand. Failed/uncertain obligations remain in the original owner. No Drop cleanup.
    /// # Safety
    /// SoundBankOwner close-step phase/residency and original-reference contracts hold.
    pub unsafe fn close_step<E: SoundBankEngine>(
        &mut self,
        owner: &mut SoundBankOwner<E>,
    ) -> Result<BankCloseProgress> {
        if owner.identity() != self.owner {
            return Err(Error::InvalidProfile("sound schedule has foreign owner"));
        }
        self.closing = true;
        for entry in &mut self.startup {
            if entry.state == StartupBankState::Pending {
                entry.state = StartupBankState::Cancelled;
            }
        }
        // SAFETY: Caller explicitly retains all owner's original unload obligations and phase.
        let progress = unsafe { owner.close_step() };
        self.conditional
            .retain(|_, request| owner.contains_request(*request));
        Ok(progress)
    }
}
#[cfg(test)]
mod tests;
