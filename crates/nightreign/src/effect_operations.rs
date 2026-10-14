//! Experimental original SpecialEffect removal. The engine unlinks and frees its nodes.
use crate::{
    NightreignFile,
    character_effects::SpecialEffectsSnapshot,
    reader::{NightreignReader, ReaderCore},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

const REMOVE_ID: CodeRange = CodeRange {
    rva: 0x504e80,
    length: 52,
};

/// Original Windows AMD64 `u8(SpecialEffect*, i32)` invocation seam.
/// # Safety
/// Memory and calls must refer to the same engine. Execute the supplied original entry,
/// without patches or unwinding; real native removal owns unlinking, heap selection/free,
/// summary updates, peer flag changes and owner callbacks. No SDK deallocation is allowed.
/// Host callers supply serialized mutation phase, engine/PARAM/heap/module residency and
/// stable object identity through all foreign callbacks. Errors need not imply no mutation.
pub unsafe trait EffectRemovalBackend: AddressSpace {
    /// # Safety
    /// Caller supplies the exact checked code and live, exclusively accessible effect owner.
    unsafe fn invoke_remove_id(&self, entry: u64, effects: u64, id: i32) -> Result<bool>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectRemovalReceipt {
    pub requested_id: i32,
    /// Copied values only. Node addresses in this snapshot may have been freed by the engine.
    pub before: SpecialEffectsSnapshot,
    /// True means the native search matched its first node; it does not mean every copy is gone.
    pub native_result: Result<bool>,
    /// Always attempted after invocation, even when the backend reports an error.
    /// An error here cannot roll back native unlinking/freeing or callbacks.
    pub after: Result<SpecialEffectsSnapshot>,
}
pub struct EffectRemoval<'r, 'f, B> {
    reader: NightreignReader<'r, 'f, B>,
}
impl<'f> NightreignFile<'f> {
    /// Checks original bytes only. Binding never invokes removal or allocates engine storage.
    pub fn bind_effect_removal<'r, B: EffectRemovalBackend>(
        &'r self,
        backend: &'r B,
        base: u64,
    ) -> Result<EffectRemoval<'r, 'f, B>> {
        Ok(EffectRemoval {
            reader: self.bind_reader(backend, base)?,
        })
    }
}
impl<B: EffectRemovalBackend> EffectRemoval<'_, '_, B> {
    /// Removes the first native ID match, including a query-ineligible node.
    /// # Safety
    /// Host provides an exclusive engine mutation phase and live native list, PARAM rows,
    /// heap registry and callback dependencies. Prevent concurrent teardown/ABA or patches;
    /// engine and SDK DLL lifetime covers all callbacks and return frames. Side effects and
    /// scheduling have not been accepted in-game. Never retry based on an error receipt.
    pub unsafe fn remove_id(
        &self,
        character: u64,
        id: i32,
        limit: usize,
    ) -> Result<EffectRemovalReceipt> {
        if id <= 0 {
            return Err(Error::InvalidProfile(
                "NR effect removal requires a positive ID",
            ));
        }
        let before = self
            .reader
            .special_effects(character, limit)?
            .ok_or(Error::NativeOperationRejected("NR effect object is absent"))?;
        let callback_possible = self.reader.core().check_effect_removal(&before, id)?;
        if callback_possible {
            self.check_owner_callback(character)?;
        }
        let entry = self.reader.module.code_address(REMOVE_ID)?;
        if self.reader.special_effects(character, limit)? != Some(before.clone()) {
            return Err(Error::InvalidProfile(
                "NR effect graph changed before removal",
            ));
        }
        // SAFETY: Checked original ABI, owner, complete bounded list and PARAM read dependencies;
        // host provides phase, real heap/PARAM ownership and callback residency obligations.
        let native_result = unsafe {
            self.reader
                .module
                .memory()
                .invoke_remove_id(entry, before.effect_address, id)
        };
        let after = self
            .reader
            .special_effects(character, limit)
            .and_then(|snapshot| {
                let snapshot = snapshot.ok_or(Error::InvalidProfile(
                    "NR effect object disappeared after removal",
                ))?;
                if snapshot.effect_address != before.effect_address
                    || snapshot.owner_address != character
                {
                    return Err(Error::InvalidProfile(
                        "NR effect owner replaced after removal",
                    ));
                }
                self.reader.core().check_effect_removal(&snapshot, id)?;
                Ok(snapshot)
            });
        Ok(EffectRemovalReceipt {
            requested_id: id,
            before,
            native_result,
            after,
        })
    }
    fn check_owner_callback(&self, character: u64) -> Result<()> {
        use crate::reader::{CHARACTER_TABLE, ENEMY_TABLE, PLAYER_TABLE};
        let table = self
            .reader
            .core()
            .check_type(character, &[CHARACTER_TABLE, ENEMY_TABLE, PLAYER_TABLE])?;
        // These original leaf entries have no exact unwind record. Their selected direct
        // control-flow spans are independently verified at bind; the generic runtime
        // virtual-slot validator retains its exact-unwind requirement.
        let entry = match table {
            PLAYER_TABLE => CodeRange {
                rva: 0x6684b0,
                length: 17,
            },
            _ => CodeRange {
                rva: 0x3c7de0,
                length: 14,
            },
        };
        let slot_rva = table.checked_add(0x510).ok_or(Error::AddressOverflow)?;
        let image = self.reader.file.file().image();
        let expected = u64::from_le_bytes(image.read_rva(slot_rva, 8)?.try_into().expect("slot"));
        if expected != image.preferred_base + entry.rva as u64 {
            return Err(Error::InvalidProfile(
                "NR effect owner callback differs from evidenced file",
            ));
        }
        let actual = self.reader.module.code_address(entry)?;
        let slot = self
            .reader
            .module
            .base()
            .checked_add(slot_rva as u64)
            .ok_or(Error::AddressOverflow)?;
        if self.reader.module.memory().read_u64(slot)? != actual {
            return Err(Error::ImageMismatch { rva: slot_rva });
        }
        Ok(())
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    /// Readability and graph consistency are checks, not proofs of native allocation ownership.
    pub(crate) fn check_effect_removal(
        &self,
        snapshot: &SpecialEffectsSnapshot,
        id: i32,
    ) -> Result<bool> {
        snapshot.check_owner_links()?;
        let mut callback_possible = false;
        let mut first_match = false;
        for node in &snapshot.nodes {
            if node.parameter_address != 0 {
                if !node.parameter_address.is_multiple_of(4) {
                    return Err(Error::InvalidProfile("unaligned NR effect PARAM"));
                }
                // Native removal examines these fields even on excluded nodes and scans peers.
                let mut category = [0; 2];
                let mut state = [0; 2];
                let mut callback = [0; 2];
                for (offset, output) in [
                    (0x13e, &mut category),
                    (0x156, &mut state),
                    (0x31c, &mut callback),
                ] {
                    self.memory.read_exact(
                        node.parameter_address
                            .checked_add(offset)
                            .ok_or(Error::AddressOverflow)?,
                        output,
                    )?;
                }
                if node.id == id && !first_match {
                    callback_possible = i16::from_le_bytes(callback) > 0;
                }
            }
            if node.id == id {
                first_match = true;
            }
        }
        Ok(callback_possible)
    }
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CurrentEffectRemovalBackend;
#[cfg(windows)]
impl AddressSpace for CurrentEffectRemovalBackend {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        shikifsrs_runtime::memory::CurrentProcess.read_exact(address, output)
    }
}
#[cfg(windows)]
// SAFETY: Calls only the original checked removal ABI; unsafe callers supply all lifetimes.
unsafe impl EffectRemovalBackend for CurrentEffectRemovalBackend {
    unsafe fn invoke_remove_id(&self, entry: u64, effects: u64, id: i32) -> Result<bool> {
        type Remove = unsafe extern "system" fn(*mut std::ffi::c_void, i32) -> u8;
        // SAFETY: Entry has the independently evidenced original AMD64 ABI.
        let function: Remove = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Caller provides exclusive live native effect owner and callback dependencies.
        Ok(unsafe { function(effects as *mut std::ffi::c_void, id) } != 0)
    }
}
