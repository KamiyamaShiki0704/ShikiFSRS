//! Experimental ownership adapter for the independently studied current Wwise path.
//! Protected entry dependencies and permitted caller phase remain unsafe obligations.
use crate::{
    current_sound_bank_api::{CurrentSoundBankApi, NativeSoundBankCall, RawBankLoad},
    sound_banks::{BankLoad, BankLoadUncertainty, BankUnload, SoundBankEngine},
};
use shikifsrs_runtime::{
    Error, Result,
    memory::{AddressSpace, CurrentProcess},
    profile::CodeRange,
};

pub(crate) const SOUND_OWNERSHIP_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x22b0ad0,
        length: 0x97,
    },
    CodeRange {
        rva: 0x22b7770,
        length: 0x68,
    },
    CodeRange {
        rva: 0x22b4f90,
        length: 0x5f,
    },
    CodeRange {
        rva: 0x22b1ee0,
        length: 0x6,
    },
    CodeRange {
        rva: 0x22b75f0,
        length: 0x11,
    },
    CodeRange {
        rva: 0x22b7ba0,
        length: 0xc4,
    },
    CodeRange {
        rva: 0x22b7b60,
        length: 0x2b,
    },
    CodeRange {
        rva: 0x22b8000,
        length: 0x6b,
    },
    CodeRange {
        rva: 0x22b1840,
        length: 0x2ec,
    },
    CodeRange {
        rva: 0x22b2200,
        length: 0x4c8,
    },
    CodeRange {
        rva: 0x22b26d0,
        length: 0x103,
    },
    CodeRange {
        rva: 0x22b2e40,
        length: 0x9c,
    },
    CodeRange {
        rva: 0x22b6e60,
        length: 0x10c,
    },
    CodeRange {
        rva: 0x22b3fe0,
        length: 0x96,
    },
    CodeRange {
        rva: 0x22af530,
        length: 0x15,
    },
    CodeRange {
        rva: 0x22b2090,
        length: 0x5,
    },
    CodeRange {
        rva: 0x22b2120,
        length: 0xe0,
    },
    CodeRange {
        rva: 0x22b7b90,
        length: 0x5,
    },
    CodeRange {
        rva: 0x22b20a0,
        length: 0x79,
    },
];
const BANK_MANAGER_GLOBAL: u32 = 0x48544e8;
const SIMPLE_MANAGER_TABLE: u32 = 0x314f510;
const THREADED_MANAGER_TABLE: u32 = 0x314f6b0;
const LOAD_SUCCESS: i32 = 1;
const LOAD_ALREADY_PRESENT: i32 = 69;
const UNLOAD_SUCCESS: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BankManagerObservation {
    address: u64,
    table: u64,
}

fn at(base: u64, offset: u32) -> Result<u64> {
    base.checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

fn manager<M: AddressSpace>(memory: &M, base: u64) -> Result<BankManagerObservation> {
    let slot = at(base, BANK_MANAGER_GLOBAL)?;
    let address = memory.read_u64(slot)?;
    if address == 0 || !address.is_multiple_of(8) {
        return Err(Error::InvalidProfile(
            "audio bank manager is absent or unaligned",
        ));
    }
    let table = memory.read_u64(address)?;
    let targets = if table == at(base, SIMPLE_MANAGER_TABLE)? {
        [0x22b4f90, 0x22b1ee0, 0x22b75f0, 0x22b2090]
    } else if table == at(base, THREADED_MANAGER_TABLE)? {
        [0x22b7ba0, 0x22b7b60, 0x22b8000, 0x22b7b90]
    } else {
        return Err(Error::InvalidProfile(
            "unknown audio bank manager virtual table",
        ));
    };
    for (offset, target) in [0x18, 0x20, 0x28, 0x40].into_iter().zip(targets) {
        if memory.read_u64(at(table, offset)?)? != at(base, target)? {
            return Err(Error::InvalidProfile("audio manager dispatch changed"));
        }
    }
    if memory.read_u64(slot)? != address || memory.read_u64(address)? != table {
        return Err(Error::InvalidProfile(
            "audio manager changed during observation",
        ));
    }
    Ok(BankManagerObservation { address, table })
}

fn classify_load(call: &NativeSoundBankCall<RawBankLoad>) -> BankLoad {
    if call.postflight.is_ok() {
        match call.value.result {
            LOAD_SUCCESS => return BankLoad::Acquired(call.value.bank_id_output),
            LOAD_ALREADY_PRESENT => return BankLoad::AlreadyPresent(call.value.bank_id_output),
            _ => {}
        }
    }
    BankLoad::Uncertain(BankLoadUncertainty {
        native_result: call.value.result,
        bank_id_output: call.value.bank_id_output,
        postflight: call.postflight.clone(),
    })
}

fn classify_unload(call: &NativeSoundBankCall<i32>) -> BankUnload {
    if call.value == UNLOAD_SUCCESS && call.postflight.is_ok() {
        BankUnload::Released
    } else {
        BankUnload::Uncertain
    }
}

/// Retains raw attempted-call receipts. No native Drop or automatic retry/classification
/// of unknown errors. SoundBankOwner retains unresolved load/unload obligations.
pub struct CurrentSoundBankEngine {
    api: CurrentSoundBankApi,
    manager: BankManagerObservation,
    last_load: Option<NativeSoundBankCall<RawBankLoad>>,
    last_unload: Option<NativeSoundBankCall<i32>>,
}

impl CurrentSoundBankEngine {
    /// Read-only construction; does not invoke IsInitialized, load or unload.
    /// # Safety
    /// All CurrentSoundBankApi contracts hold for the whole owner lifetime. The original
    /// manager, allocator, plugins, callbacks, synchronization and SDK/module remain resident.
    /// Caller supplies a phase in which the synchronous bank queue AND audio callback wait
    /// can complete; do not call from a worker/callback whose progress is required by that wait.
    /// No external clear/unload/destruction, bank/manager reuse or patch change may race.
    /// Bounded code/dispatch checks do not reconstruct the protected entry's complete graph.
    pub unsafe fn new(api: CurrentSoundBankApi) -> Result<Self> {
        api.verify_ranges(SOUND_OWNERSHIP_EVIDENCE)?;
        let observed = manager(&CurrentProcess, api.module_base())?;
        Ok(Self {
            api,
            manager: observed,
            last_load: None,
            last_unload: None,
        })
    }

    pub fn last_load(&self) -> Option<&NativeSoundBankCall<RawBankLoad>> {
        self.last_load.as_ref()
    }
    pub fn last_unload(&self) -> Option<&NativeSoundBankCall<i32>> {
        self.last_unload.as_ref()
    }

    fn verify(&self) -> Result<()> {
        self.api.verify_ranges(SOUND_OWNERSHIP_EVIDENCE)?;
        if manager(&CurrentProcess, self.api.module_base())? != self.manager {
            return Err(Error::InvalidProfile("original audio manager was replaced"));
        }
        Ok(())
    }
}

// SAFETY: The unsafe constructor and per-call caller contract retain the complete engine
// graph and valid synchronous memory-copy input. Pre-call errors do not submit; every
// attempted load/unload with unknown status/postflight becomes a retained uncertainty.
// Only independently proved 1/69 outcomes classify load; unload1 consumes the obligation.
unsafe impl SoundBankEngine for CurrentSoundBankEngine {
    unsafe fn load_memory_copy(&mut self, bytes: &[u8]) -> Result<BankLoad> {
        self.verify()?;
        // SAFETY: Trait caller and constructor retain valid bank input, graph and phase.
        let mut call = unsafe { self.api.load_memory_copy(bytes)? };
        if call.postflight.is_ok() {
            call.postflight = self.verify();
        }
        let result = classify_load(&call);
        self.last_load = Some(call);
        Ok(result)
    }

    unsafe fn unload(&mut self, bank_id: u32) -> Result<BankUnload> {
        self.verify()?;
        // SAFETY: Owner/trait caller supplies the original live obligation and unload phase.
        let mut call = unsafe { self.api.unload_copied_bank(bank_id)? };
        if call.postflight.is_ok() {
            call.postflight = self.verify();
        }
        let result = classify_unload(&call);
        self.last_unload = Some(call);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, collections::BTreeMap};

    #[test]
    fn only_proven_load_outcomes_classify_and_postflight_failure_retains_raw_output() {
        for code in [i32::MIN, -1, 0, 1, 2, 31, 52, 54, 69, 89, i32::MAX] {
            for id in [0, 1, u32::MAX] {
                for failed in [false, true] {
                    let call = NativeSoundBankCall {
                        value: RawBankLoad {
                            result: code,
                            bank_id_output: id,
                        },
                        postflight: if failed {
                            Err(Error::InvalidProfile("changed"))
                        } else {
                            Ok(())
                        },
                    };
                    let result = classify_load(&call);
                    if !failed && code == 1 {
                        assert_eq!(result, BankLoad::Acquired(id));
                    } else if !failed && code == 69 {
                        assert_eq!(result, BankLoad::AlreadyPresent(id));
                    } else {
                        assert_eq!(
                            result,
                            BankLoad::Uncertain(BankLoadUncertainty {
                                native_result: code,
                                bank_id_output: id,
                                postflight: call.postflight.clone(),
                            })
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn unload_failure_or_unknown_result_never_infers_retry_or_consumption() {
        for result in [i32::MIN, 0, 1, 2, 54, 69, i32::MAX] {
            for failed in [false, true] {
                let call = NativeSoundBankCall {
                    value: result,
                    postflight: if failed {
                        Err(Error::InvalidProfile("changed"))
                    } else {
                        Ok(())
                    },
                };
                assert_eq!(
                    classify_unload(&call),
                    if result == 1 && !failed {
                        BankUnload::Released
                    } else {
                        BankUnload::Uncertain
                    }
                );
            }
        }
    }

    struct Memory {
        words: BTreeMap<u64, u64>,
        watched: u64,
        reads: Cell<u32>,
    }
    impl AddressSpace for Memory {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            assert_eq!(output.len(), 8);
            let word = *self
                .words
                .get(&address)
                .ok_or(Error::OutOfBounds { address, length: 8 })?;
            if address == self.watched {
                let n = self.reads.get();
                self.reads.set(n + 1);
                if n > 0 {
                    output.fill(0);
                    return Ok(());
                }
            }
            output.copy_from_slice(&word.to_le_bytes());
            Ok(())
        }
    }

    #[test]
    fn manager_checks_both_exact_tables_and_selected_dispatch_without_unknown_payload_reads() {
        const BASE: u64 = 0x7ff000000000;
        for (table, targets) in [
            (
                SIMPLE_MANAGER_TABLE,
                [0x22b4f90, 0x22b1ee0, 0x22b75f0, 0x22b2090],
            ),
            (
                THREADED_MANAGER_TABLE,
                [0x22b7ba0, 0x22b7b60, 0x22b8000, 0x22b7b90],
            ),
        ] {
            let mut m = Memory {
                words: BTreeMap::from([
                    (BASE + u64::from(BANK_MANAGER_GLOBAL), 0x100000),
                    (0x100000, BASE + u64::from(table)),
                ]),
                watched: 0,
                reads: Cell::new(0),
            };
            for (offset, target) in [0x18, 0x20, 0x28, 0x40].into_iter().zip(targets) {
                m.words
                    .insert(BASE + u64::from(table) + offset, BASE + target);
            }
            assert_eq!(manager(&m, BASE).unwrap().address, 0x100000);
            for watched in [BASE + u64::from(BANK_MANAGER_GLOBAL), 0x100000] {
                m.watched = watched;
                m.reads.set(0);
                assert!(manager(&m, BASE).is_err());
            }
            m.watched = 0;
            m.words
                .insert(BASE + u64::from(table) + 0x20, BASE + 0x22b75f0);
            assert!(manager(&m, BASE).is_err());
            m.words.insert(0x100000, BASE + u64::from(table) + 8);
            assert!(manager(&m, BASE).is_err());
            m.words.insert(BASE + u64::from(BANK_MANAGER_GLOBAL), 0);
            assert!(manager(&m, BASE).is_err());
        }
    }
}
