//! Experimental synchronous Wwise export calls. Raw receipts do not classify ownership.
//! The exported signatures are evidence; protected implementation paths remain opaque.
use crate::WW_171;
use shikifsrs_runtime::{
    Error, Result,
    code_patches::OwnedCodePatches,
    memory::CurrentProcess,
    profile::{CodeRange, OwnedVerifiedFile},
};
use std::{ffi::c_void, marker::PhantomData, rc::Rc, sync::Arc};

pub(crate) const SOUND_API_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x223be30,
        length: 0xf5,
    },
    CodeRange {
        rva: 0x2242ce0,
        length: 0xd3,
    },
    CodeRange {
        rva: 0x223b860,
        length: 0x5,
    },
    CodeRange {
        rva: 0x22b1bf0,
        length: 0x89,
    },
    CodeRange {
        rva: 0x1ddb395,
        length: 0x48,
    },
    CodeRange {
        rva: 0xc3f8ef,
        length: 0x48,
    },
    CodeRange {
        rva: 0x5402c6,
        length: 0x49,
    },
];

type LoadEntry = unsafe extern "system" fn(*const c_void, u32, *mut u32) -> i32;
type UnloadEntry = unsafe extern "system" fn(u32, *const c_void) -> i32;
// Copy AL rather than create a Rust bool from an unchecked native representation.
type InitializedEntry = unsafe extern "system" fn() -> u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawBankLoad {
    pub result: i32,
    /// Initialized to zero before invocation. Not a valid bank identifier for every result.
    pub bank_id_output: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSoundBankCall<T> {
    pub value: T,
    /// Failed verification retains the attempted call and its original output.
    /// It does not permit retry, infer rollback or classify an unload obligation.
    pub postflight: Result<()>,
}

/// Thread-confined raw export boundary, separate from SoundBankEngine ownership semantics.
/// There are no automatic retries, classification, unloads, native Drop or file I/O.
pub struct CurrentSoundBankApi {
    file: OwnedVerifiedFile,
    base: u64,
    patches: Option<Arc<OwnedCodePatches>>,
    _thread: PhantomData<Rc<()>>,
}

impl CurrentSoundBankApi {
    /// Exact file/base/code checks only, without calling IsInitialized or polling audio.
    /// # Safety
    /// Caller retains this exact process module and SDK, the initialized Wwise engine,
    /// complete protected-code dependencies, bank/stream/memory managers, their allocators
    /// and all callbacks. No Term/ClearBanks, destruction, alias/unload, patching or object
    /// reuse may race calls. Caller independently establishes a permitted audio phase and
    /// synchronization; an exported name and these bounded code windows do not prove them.
    /// Vendor synchronous memory-copy semantics, including no input access after return,
    /// must hold for this version. Protected bodies have not been fully reconstructed.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        let file = OwnedVerifiedFile::verify(bytes, WW_171)?;
        file.borrow()?.bind_with_patches(
            &CurrentProcess,
            base,
            SOUND_API_EVIDENCE,
            patches.as_deref(),
        )?;
        Ok(Self {
            file,
            base,
            patches,
            _thread: PhantomData,
        })
    }

    fn entry(&self, index: usize) -> Result<u64> {
        let file = self.file.borrow()?;
        file.bind_with_patches(
            &CurrentProcess,
            self.base,
            SOUND_API_EVIDENCE,
            self.patches.as_deref(),
        )?
        .code_address(SOUND_API_EVIDENCE[index])
    }

    fn postflight(&self) -> Result<()> {
        self.entry(0).map(|_| ())
    }

    pub(crate) fn verify_ranges(&self, ranges: &[CodeRange]) -> Result<()> {
        self.file.borrow()?.bind_with_patches(
            &CurrentProcess,
            self.base,
            ranges,
            self.patches.as_deref(),
        )?;
        Ok(())
    }

    pub(crate) fn module_base(&self) -> u64 {
        self.base
    }

    /// Copy the raw bool return byte. This is no lifetime or successful initialization lease.
    /// # Safety
    /// bind's residency/synchronization/complete dependency contract holds. No concurrent Term.
    pub unsafe fn initialization_status(&self) -> Result<NativeSoundBankCall<u8>> {
        let address = self.entry(2)?;
        // SAFETY: Exact decorated exported signature is bool(void); capture its low byte.
        // Caller supplies the complete initialized/resident vendor engine dependency contract.
        let entry: InitializedEntry = unsafe { std::mem::transmute(address as usize) };
        // SAFETY: Exact void-argument ABI, with the above caller obligations.
        let value = unsafe { entry() };
        Ok(NativeSoundBankCall {
            value,
            postflight: self.postflight(),
        })
    }

    /// Call the synchronous three-argument memory-copy overload, preserving result and output.
    /// # Safety
    /// bind's full contract holds; bytes contain a valid bank compatible with this engine and
    /// its already loaded initialization bank/plugins. Caller retains immutable initialized
    /// bytes through the entire blocking call, and independently records/classifies ownership
    /// even when postflight fails. A returned receipt is not permission to repeat the load.
    pub unsafe fn load_memory_copy(
        &self,
        bytes: &[u8],
    ) -> Result<NativeSoundBankCall<RawBankLoad>> {
        let length = bank_length(bytes.len())?;
        let address = self.entry(0)?;
        // SAFETY: Decorated export proves const void*, u32, u32& -> AKRESULT; fixed x64 ABI.
        // Caller supplies actual vendor semantics, permitted phase and live graph.
        let entry: LoadEntry = unsafe { std::mem::transmute(address as usize) };
        // SAFETY: Local output is initialized/aligned; byte borrow stays alive through the
        // synchronous call. No other fallible operation can discard this native output.
        let value = unsafe { invoke_load(entry, bytes, length) };
        Ok(NativeSoundBankCall {
            value,
            postflight: self.postflight(),
        })
    }

    /// Call the two-argument unload overload with a null memory pointer for copied banks.
    /// # Safety
    /// Caller proves an original still-live owned obligation for bank_id, excluding borrowed
    /// banks, uncertain previous unloads, bank replacement and concurrent ClearBanks/Term.
    /// bind's phase/dependency contract holds. Every receipt is an attempt; unknown return or
    /// postflight failure needs reconciliation, not automatic retry or guessed consumption.
    pub unsafe fn unload_copied_bank(&self, bank_id: u32) -> Result<NativeSoundBankCall<i32>> {
        let address = self.entry(1)?;
        // SAFETY: Exact decorated export proves u32,const void* -> AKRESULT; fixed x64 ABI.
        let entry: UnloadEntry = unsafe { std::mem::transmute(address as usize) };
        // SAFETY: Caller supplies the original live obligation and full vendor graph contract;
        // copied banks use null, not the address of a dropped input buffer.
        let value = unsafe { invoke_unload(entry, bank_id) };
        Ok(NativeSoundBankCall {
            value,
            postflight: self.postflight(),
        })
    }
}

fn bank_length(length: usize) -> Result<u32> {
    // Native header helper reads 16 bytes starting at+8 after its length>=0x1c gate.
    // This bound only prevents invalid input width/short buffers, not malformed bank content.
    if !(0x1c..=u32::MAX as usize).contains(&length) {
        return Err(Error::InvalidProfile(
            "memory-copy bank length must be 28..=u32::MAX",
        ));
    }
    Ok(length as u32)
}

unsafe fn invoke_load(entry: LoadEntry, bytes: &[u8], length: u32) -> RawBankLoad {
    let mut bank_id_output = 0;
    // SAFETY: Caller supplies checked length, exact signature and synchronous input lifetime.
    let result = unsafe { entry(bytes.as_ptr().cast(), length, &mut bank_id_output) };
    RawBankLoad {
        result,
        bank_id_output,
    }
}

unsafe fn invoke_unload(entry: UnloadEntry, bank_id: u32) -> i32 {
    // SAFETY: Caller supplies original owned obligation and exact synchronous signature.
    unsafe { entry(bank_id, std::ptr::null()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static SEEN: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());

    unsafe extern "system" fn load(bytes: *const c_void, length: u32, output: *mut u32) -> i32 {
        assert_eq!(length, 28);
        // SAFETY: Test passes its own initialized28-byte array and live u32 output.
        let input = unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), length as usize) };
        assert_eq!(input[27], 0xad);
        // SAFETY: Own initialized aligned output retained by invoke_load throughout this call.
        unsafe {
            *output = u32::MAX;
        }
        -1234
    }

    unsafe extern "system" fn untouched(_: *const c_void, _: u32, _: *mut u32) -> i32 {
        31
    }
    unsafe extern "system" fn unload(id: u32, input: *const c_void) -> i32 {
        assert!(input.is_null());
        SEEN.lock().unwrap().push((id, 0));
        69
    }

    #[test]
    fn original_rust_load_abi_keeps_input_and_all_raw_outputs_without_classification() {
        let bytes = [0xad; 28];
        assert_eq!(
            // SAFETY: Own original Rust ABI entries and initialized input/output, no game access.
            unsafe { invoke_load(load, &bytes, bank_length(bytes.len()).unwrap()) },
            RawBankLoad {
                result: -1234,
                bank_id_output: u32::MAX
            }
        );
        assert_eq!(
            // SAFETY: Same original owned ABI/storage; entry deliberately leaves output untouched.
            unsafe { invoke_load(untouched, &bytes, 28) },
            RawBankLoad {
                result: 31,
                bank_id_output: 0
            }
        );
    }

    #[test]
    fn original_rust_unload_abi_uses_null_for_copied_banks_and_preserves_unknown_result() {
        for id in [0, 1, u32::MAX] {
            // SAFETY: Original self-authored function accesses no native resources.
            assert_eq!(unsafe { invoke_unload(unload, id) }, 69);
        }
        assert_eq!(*SEEN.lock().unwrap(), [(0, 0), (1, 0), (u32::MAX, 0)]);
    }

    #[test]
    fn length_gate_rejects_short_or_truncated_width_without_allocating_large_buffers() {
        for n in [0, 1, 27, u32::MAX as usize + 1, usize::MAX] {
            assert!(bank_length(n).is_err());
        }
        for n in [28, 4096, u32::MAX as usize] {
            assert_eq!(bank_length(n).unwrap() as usize, n);
        }
        let receipt = NativeSoundBankCall {
            value: RawBankLoad {
                result: 1,
                bank_id_output: 123,
            },
            postflight: Err(Error::ImageMismatch { rva: 1 }),
        };
        assert_eq!(receipt.value.bank_id_output, 123);
        assert!(receipt.postflight.is_err());
    }

    #[test]
    fn initialization_abi_preserves_raw_byte_and_unknown_file_binding_rejects_before_process_reads()
    {
        unsafe extern "system" fn initialized() -> u8 {
            0xfe
        }
        let entry: InitializedEntry = initialized;
        // SAFETY: Original self-authored function with no native graph or process access.
        assert_eq!(unsafe { entry() }, 0xfe);
        // SAFETY: No live graph supplied; unknown file identity must reject before any native call.
        assert!(unsafe { CurrentSoundBankApi::bind(Arc::from([0; 64]), 0, None) }.is_err());
    }
}
