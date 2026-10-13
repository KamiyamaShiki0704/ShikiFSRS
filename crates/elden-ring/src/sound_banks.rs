//! Explicit sound-bank reference accounting. Native binding remains separate.
use shikifsrs_runtime::{Error, Result};
use std::{
    collections::BTreeMap,
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BankLoad {
    /// One new unload obligation; the engine has copied the input before returning.
    Acquired(u32),
    /// No unload obligation acquired. Presence is not a keep-alive guarantee.
    AlreadyPresent(u32),
    /// A load was attempted; raw output is not automatically a valid identifier.
    /// This must be retained and reconciled before any subsequent native call.
    Uncertain(BankLoadUncertainty),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BankLoadUncertainty {
    pub native_result: i32,
    pub bank_id_output: u32,
    pub postflight: Result<()>,
}

/// Independently established outcome of exactly one previously attempted load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BankLoadResolution {
    NoReference,
    Acquired(u32),
    AlreadyPresent(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BankUnload {
    Released,
    /// Engine explicitly retains the same obligation; a later permitted-phase retry is valid.
    RetryLater,
    /// A native call occurred, but its effect on ownership cannot be classified.
    Uncertain,
}

/// Synchronous memory-copy engine; never an in-place buffer lease.
/// # Safety
/// Successful acquisition represents exactly one distinct unload obligation.
/// AlreadyPresent creates none. Input is neither retained nor accessed after load returns,
/// including errors. Errors leave ownership unchanged; an ambiguous post-call failure
/// must return Uncertain instead (for both load and unload). Drop must not perform native cleanup. Released consumes exactly one obligation, RetryLater
/// consumes none. Implementations must not panic after an unrecorded side effect.
pub unsafe trait SoundBankEngine {
    /// # Safety
    /// Caller holds the engine's initialized loading phase, synchronization and residency.
    unsafe fn load_memory_copy(&mut self, bytes: &[u8]) -> Result<BankLoad>;
    /// # Safety
    /// Caller holds the permitted unloading phase and this engine's live original obligation.
    unsafe fn unload(&mut self, bank_id: u32) -> Result<BankUnload>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BankRequest {
    owner: u64,
    sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BankObservation {
    pub bank_id: u32,
    pub requests: usize,
    pub owned_references: usize,
    /// This owner acquired a reference during this tracking interval, possibly already released.
    pub ever_acquired: bool,
    pub release_uncertain: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BankRelease {
    AlreadyReleased,
    /// Other tracked requests still demand this bank; no native unload was called.
    Shared,
    /// No reference was acquired by this owner; no native unload was called.
    Borrowed,
    Released,
    RetryLater,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BankCloseProgress {
    pub remaining_requests: usize,
    pub remaining_references: usize,
    pub uncertain_banks: Vec<u32>,
    /// Unclassified load attempts, distinct from known owned reference counts.
    pub uncertain_loads: Vec<(BankRequest, BankLoadUncertainty)>,
    pub failures: Vec<(BankRequest, Error)>,
}

/// Thread-confined owner. The engine is retained while requests remain.
/// Close is explicit; Drop calls no native function and is not cleanup completion.
/// Copied bank IDs and AlreadyPresent are not engine storage leases or audio-ready receipts.
pub struct SoundBankOwner<E: SoundBankEngine> {
    engine: E,
    owner_id: u64,
    requests: BTreeMap<BankRequest, u32>,
    uncertain_loads: BTreeMap<BankRequest, BankLoadUncertainty>,
    banks: BTreeMap<u32, BankObservation>,
    next_request: u64,
    limit: usize,
    closing: bool,
    _thread: PhantomData<Rc<()>>,
}

impl<E: SoundBankEngine> SoundBankOwner<E> {
    pub fn new(engine: E, limit: usize) -> Result<Self> {
        if !(1..=1024).contains(&limit) {
            return Err(Error::InvalidProfile("invalid sound-bank request limit"));
        }
        let owner_id = NEXT_OWNER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| Error::AddressOverflow)?;
        Ok(Self {
            engine,
            owner_id,
            requests: BTreeMap::new(),
            uncertain_loads: BTreeMap::new(),
            banks: BTreeMap::new(),
            next_request: 1,
            limit,
            closing: false,
            _thread: PhantomData,
        })
    }

    pub fn observations(&self) -> Vec<BankObservation> {
        self.banks.values().copied().collect()
    }

    pub fn uncertain_loads(&self) -> Vec<(BankRequest, BankLoadUncertainty)> {
        self.uncertain_loads
            .iter()
            .map(|(request, receipt)| (*request, receipt.clone()))
            .collect()
    }

    pub fn is_closing(&self) -> bool {
        self.closing
    }

    pub fn is_uncertain(&self) -> bool {
        !self.uncertain_loads.is_empty() || self.banks.values().any(|bank| bank.release_uncertain)
    }
    pub(crate) fn identity(&self) -> u64 {
        self.owner_id
    }
    pub(crate) fn contains_request(&self, request: BankRequest) -> bool {
        self.requests.contains_key(&request) || self.uncertain_loads.contains_key(&request)
    }

    /// Inspect retained engine receipts. Mutating/calling the engine still needs its contract.
    pub fn engine(&self) -> &E {
        &self.engine
    }

    /// Acquire one demand. Aliases returning the same native ID share tracked ownership;
    /// releasing a borrowed alias never discards this owner's remaining unload obligation.
    /// # Safety
    /// Caller supplies the engine's loading phase, synchronization, and native residency.
    pub unsafe fn acquire(&mut self, bytes: &[u8]) -> Result<BankRequest> {
        if self.closing
            || self.requests.len() + self.uncertain_loads.len() == self.limit
            || !self.uncertain_loads.is_empty()
            || self.banks.values().any(|bank| bank.release_uncertain)
        {
            return Err(Error::NativeOperationRejected(
                "sound-bank owner is closing, full or uncertain",
            ));
        }
        if bytes.is_empty() || bytes.len() > u32::MAX as usize {
            return Err(Error::InvalidProfile("invalid memory-copy bank length"));
        }
        let next = self
            .next_request
            .checked_add(1)
            .ok_or(Error::AddressOverflow)?;
        // SAFETY: Forward the caller's phase/residency contract; trait forbids retained input.
        let loaded = unsafe { self.engine.load_memory_copy(bytes)? };
        let request = BankRequest {
            owner: self.owner_id,
            sequence: self.next_request,
        };
        self.next_request = next;
        let (bank_id, owned) = match loaded {
            BankLoad::Acquired(id) => (id, 1),
            BankLoad::AlreadyPresent(id) => (id, 0),
            BankLoad::Uncertain(receipt) => {
                self.uncertain_loads.insert(request, receipt);
                return Ok(request);
            }
        };
        self.track(request, bank_id, owned);
        Ok(request)
    }

    fn track(&mut self, request: BankRequest, bank_id: u32, owned: usize) {
        let bank = self.banks.entry(bank_id).or_insert(BankObservation {
            bank_id,
            requests: 0,
            owned_references: 0,
            ever_acquired: false,
            release_uncertain: false,
        });
        bank.requests += 1;
        bank.owned_references += owned;
        bank.ever_acquired |= owned != 0;
        self.requests.insert(request, bank_id);
    }

    /// Establish the original attempt's ownership, preserving aliases and pending close.
    /// # Safety
    /// Caller proves this exact invocation's outcome and valid ID, including whether it
    /// acquired one reference. Presence or the output word alone cannot establish that.
    /// The engine and original bank identity remain resident; no ABA or external unload.
    pub unsafe fn reconcile_load(
        &mut self,
        request: BankRequest,
        resolution: BankLoadResolution,
    ) -> Result<()> {
        if request.owner != self.owner_id || !self.uncertain_loads.contains_key(&request) {
            return Err(Error::NativeOperationRejected(
                "no matching uncertain sound-bank load",
            ));
        }
        match resolution {
            BankLoadResolution::NoReference => {}
            BankLoadResolution::Acquired(id) => self.track(request, id, 1),
            BankLoadResolution::AlreadyPresent(id) => self.track(request, id, 0),
        }
        self.uncertain_loads.remove(&request);
        Ok(())
    }

    /// Release one demand; only the last demand submits the owner's unload obligations.
    /// Errors and RetryLater retain that demand for a later explicit retry. Uncertain
    /// blocks further native calls until the original obligation is independently reconciled.
    /// # Safety
    /// Caller holds permitted unloading phase, synchronization and original native residency.
    pub unsafe fn release(&mut self, request: BankRequest) -> Result<BankRelease> {
        if request.owner != self.owner_id {
            return Err(Error::InvalidProfile(
                "sound-bank request belongs to another owner",
            ));
        }
        // An unknown load may alias any known bank and change its unload obligations.
        // Block native releases without treating its untrusted output as a bank ID.
        if !self.uncertain_loads.is_empty()
            && (self.uncertain_loads.contains_key(&request) || self.requests.contains_key(&request))
        {
            return Ok(BankRelease::Uncertain);
        }
        let Some(&id) = self.requests.get(&request) else {
            return Ok(BankRelease::AlreadyReleased);
        };
        let bank = self.banks.get_mut(&id).expect("tracked request has bank");
        if bank.release_uncertain {
            return Ok(BankRelease::Uncertain);
        }
        if bank.requests > 1 {
            bank.requests -= 1;
            self.requests.remove(&request);
            return Ok(BankRelease::Shared);
        }
        let borrowed = !bank.ever_acquired;
        while bank.owned_references > 0 {
            // SAFETY: Owner retains original obligations; caller supplies the native phase.
            match unsafe { self.engine.unload(id)? } {
                BankUnload::Released => bank.owned_references -= 1,
                BankUnload::RetryLater => return Ok(BankRelease::RetryLater),
                BankUnload::Uncertain => {
                    bank.release_uncertain = true;
                    return Ok(BankRelease::Uncertain);
                }
            }
        }
        self.banks.remove(&id);
        self.requests.remove(&request);
        Ok(if borrowed {
            BankRelease::Borrowed
        } else {
            BankRelease::Released
        })
    }

    /// Reconcile exactly the uncertain unload, not a new bank with the same ID.
    /// # Safety
    /// Caller independently proves whether that call consumed one original obligation.
    /// Current bank presence alone cannot establish this fact or justify a retry.
    pub unsafe fn reconcile_unload(&mut self, bank_id: u32, consumed: bool) -> Result<()> {
        let bank = self
            .banks
            .get_mut(&bank_id)
            .filter(|bank| bank.release_uncertain)
            .ok_or(Error::NativeOperationRejected(
                "no uncertain sound-bank unload",
            ))?;
        if consumed {
            bank.owned_references -= 1;
        }
        bank.release_uncertain = false;
        Ok(())
    }

    /// Close the acquisition gate before attempting all current demands. One failure
    /// does not prevent cleanup of other banks; later steps retry only retained work.
    /// # Safety
    /// Caller holds permitted unloading phase, synchronization and native residency.
    pub unsafe fn close_step(&mut self) -> BankCloseProgress {
        self.closing = true;
        let requests: Vec<_> = self.requests.keys().copied().collect();
        let mut failures = Vec::new();
        for request in requests {
            // SAFETY: Same caller-owned unload phase for all retained requests.
            if let Err(error) = unsafe { self.release(request) } {
                failures.push((request, error));
            }
        }
        BankCloseProgress {
            remaining_requests: self.requests.len() + self.uncertain_loads.len(),
            remaining_references: self.banks.values().map(|bank| bank.owned_references).sum(),
            uncertain_banks: self
                .banks
                .values()
                .filter(|bank| bank.release_uncertain)
                .map(|bank| bank.bank_id)
                .collect(),
            uncertain_loads: self.uncertain_loads(),
            failures,
        }
    }
}

#[cfg(test)]
mod tests;
