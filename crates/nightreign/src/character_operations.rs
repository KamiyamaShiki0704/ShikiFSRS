//! Experimental native limit operations. No raw direct writes substitute for native methods.
use crate::{NightreignFile, character_state::CharacterAttributes, reader::NightreignReader};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceLimit {
    Health,
    Focus,
    Stamina,
}
impl ResourceLimit {
    pub(crate) fn entry(self) -> CodeRange {
        match self {
            Self::Health => CodeRange {
                rva: 0x427d40,
                length: 0x4f,
            },
            Self::Focus => CodeRange {
                rva: 0x427db0,
                length: 0x39,
            },
            Self::Stamina => CodeRange {
                rva: 0x427e00,
                length: 0x3a,
            },
        }
    }
    fn validate(self, value: i32) -> Result<()> {
        if (self == Self::Health && !(1..=0x7ffff).contains(&value))
            || (self != Self::Health && value < 0)
        {
            return Err(Error::InvalidProfile(
                "NR effective resource limit is out of supported range",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitChange {
    pub resource: ResourceLimit,
    pub before: CharacterAttributes,
    pub after: CharacterAttributes,
}

/// Invocation seam for a real in-process backend and owned-memory fixture backends.
/// # Safety
/// AddressSpace must correspond to the invoked engine. The entry has the verified Windows
/// AMD64 `void(this, i32)` limit ABI; module/heap/SDK DLL residency, mutation phase, no code
/// patching, stable module ownership and serialized operations cover the entire call/return.
/// Invocation must not unwind. An error can follow an applied mutation; never infer rollback.
pub unsafe trait CharacterOperationBackend: AddressSpace {
    /// # Safety
    /// The exact code entry and live owned module were checked; caller supplies phase/lifetime.
    unsafe fn invoke_limit(&self, entry: u64, module: u64, value: i32) -> Result<()>;
}

pub struct CharacterOperations<'r, 'f, B> {
    reader: NightreignReader<'r, 'f, B>,
}
impl<'f> NightreignFile<'f> {
    /// Byte checks only: this never invokes native code or mutates an object.
    pub fn bind_character_operations<'r, B: CharacterOperationBackend>(
        &'r self,
        backend: &'r B,
        base: u64,
    ) -> Result<CharacterOperations<'r, 'f, B>> {
        Ok(CharacterOperations {
            reader: self.bind_reader(backend, base)?,
        })
    }
}
impl<B: CharacterOperationBackend> CharacterOperations<'_, '_, B> {
    /// # Safety
    /// Caller owns the native mutation phase and prevents concurrent teardown, module/address
    /// reuse and patching. DLL/native engine residency extends through every foreign frame.
    /// In-game side effects remain unaccepted; an error after invocation is not a rollback.
    /// Supported limits: health 1..=524287; focus and stamina are nonnegative i32 values.
    pub unsafe fn set_effective_limit(
        &self,
        character: u64,
        resource: ResourceLimit,
        value: i32,
    ) -> Result<LimitChange> {
        resource.validate(value)?;
        let before = self
            .reader
            .character_attributes(character)?
            .ok_or(Error::NativeOperationRejected("NR data module is absent"))?;
        let entry = self.reader.module.code_address(resource.entry())?;
        // SAFETY: Exact module/type/owner/code checked; host phase/residency obligations forwarded.
        unsafe {
            self.reader
                .module
                .memory()
                .invoke_limit(entry, before.module.module_address, value)?;
        }
        let after =
            self.reader
                .character_attributes(character)?
                .ok_or(Error::NativeOperationRejected(
                    "NR data module disappeared after native operation",
                ))?;
        if before.module != after.module {
            return Err(Error::InvalidProfile(
                "NR module replaced during native operation",
            ));
        }
        Ok(LimitChange {
            resource,
            before,
            after,
        })
    }
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CurrentCharacterBackend;
#[cfg(windows)]
impl AddressSpace for CurrentCharacterBackend {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        shikifsrs_runtime::memory::CurrentProcess.read_exact(address, output)
    }
}
#[cfg(windows)]
// SAFETY: Backend invokes only the checked original NR limit ABI in this process;
// unsafe operation callers provide scheduling, object lifetime and DLL residency.
unsafe impl CharacterOperationBackend for CurrentCharacterBackend {
    unsafe fn invoke_limit(&self, entry: u64, module: u64, value: i32) -> Result<()> {
        type SetLimit = unsafe extern "system" fn(*mut std::ffi::c_void, i32);
        // SAFETY: Checked original AMD64 entry and supported this/value ABI.
        let function: SetLimit = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Caller supplies a live exact NR data module and exclusive mutation phase.
        unsafe {
            function(module as *mut std::ffi::c_void, value);
        }
        Ok(())
    }
}
