//! Explicit character task clearing; queue completion and runtime acceptance remain unproved.
use crate::{
    TaskCatalog, WW_171, Ww171File,
    character_sets::CharacterIdentity,
    character_task_clear::CharacterTaskClearTicket,
    character_tasks::CharacterTaskRegistration,
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::code_patches::OwnedCodePatches;
use shikifsrs_runtime::{
    Error, Result,
    memory::{AddressSpace, CurrentProcess},
    profile::{CodeRange, OwnedVerifiedFile},
};
use std::{ffi::c_void, marker::PhantomData, rc::Rc, sync::Arc};

const CLEAR_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0xeb3530,
        length: 0x25,
    },
    CodeRange {
        rva: 0xeb36f0,
        length: 0x5d,
    },
    CodeRange {
        rva: 0xeb3cd0,
        length: 0x50,
    },
    CodeRange {
        rva: 0xeb4500,
        length: 0x5d,
    },
    CodeRange {
        rva: 0x2657950,
        length: 0xb,
    },
    CodeRange {
        rva: 0xeb37e0,
        length: 0x23f,
    },
    CodeRange {
        rva: 0xeb3a20,
        length: 0xfc,
    },
];
const CHARACTER_TASK_MANAGER_GLOBAL: u32 = 0x458fe88;
type ClearEntry = unsafe extern "system" fn(*mut c_void);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CharacterTaskClear {
    /// Slot was already null; no native call. Does not acknowledge any earlier cancellation.
    AlreadyClear,
    /// Native clear returned. Retain the ticket even when the post-call slot read fails.
    /// Cancellation may fail or be queued; slot_clear only reports the embedded slot.
    Requested {
        ticket: CharacterTaskClearTicket,
        slot_clear: Result<()>,
    },
}

/// Thread-limited session. Drop never calls native code and never frees character/proxy storage.
pub struct CurrentCharacterTasks {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}

impl CurrentCharacterTasks {
    /// # Safety
    /// base is this process's exact game module. Caller retains module/SDK, initialized task
    /// manager/system/heaps, live character and its proxies, and exclusively owns a permitted
    /// clear phase. No active callback, concurrent registration, teardown or reuse may overlap.
    /// These conditions are not acquired by the SDK; native behavior remains experimental.
    pub unsafe fn bind(bytes: Arc<[u8]>, base: u64, epoch: u64) -> Result<Self> {
        // SAFETY: Same caller obligations; no code replacements accepted by default.
        unsafe { Self::bind_with_patches(bytes, base, epoch, None) }
    }

    /// Bind with an explicitly owned code replacement manifest.
    /// # Safety
    /// All bind obligations apply, plus the manifest owner's ABI, semantic, lifetime and
    /// quiescence guarantees for every registered hook throughout all operations.
    pub unsafe fn bind_with_patches(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile(
                "character task epoch must be nonzero",
            ));
        }
        let file = OwnedVerifiedFile::verify(bytes, WW_171)?;
        let view = Ww171File::from_verified(file.borrow()?)?;
        let memory = CurrentProcess;
        let catalog = view.bind_objects_with_patches(&memory, base, patches.as_deref())?;
        for range in CLEAR_EVIDENCE {
            catalog.module.code_address(*range)?;
        }
        Ok(Self {
            file,
            patches,
            base,
            epoch,
            _thread: PhantomData,
        })
    }

    /// Explicitly clear one constructor ordinal. No destructor, deallocation or DLL unload.
    /// # Safety
    /// Caller fulfills the bind contract for this operation, including quiescence of callbacks
    /// that already copied the capture. Errors after the call may have partial native effects.
    /// Requested is not cancellation acceptance, completion, proxy-free or drain permission.
    pub unsafe fn clear(
        &self,
        target: CharacterIdentity,
        ordinal: u8,
    ) -> Result<CharacterTaskClear> {
        if ordinal >= 6 || target.epoch != self.epoch {
            return Err(Error::InvalidProfile(
                "invalid character task selection or epoch",
            ));
        }
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let catalog =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        for range in CLEAR_EVIDENCE
            .iter()
            .chain(crate::current_tasks::NATIVE_TASK_EVIDENCE)
        {
            catalog.module.code_address(*range)?;
        }
        let observation = catalog
            .character_tasks(target)?
            .ok_or(Error::NativeOperationRejected("character is absent"))?;
        let task = observation.tasks[ordinal as usize];
        let Some(registration) = task.registration else {
            return Ok(CharacterTaskClear::AlreadyClear);
        };
        let tasks = TaskCatalog {
            module: view.file().bind_with_patches(
                &memory,
                self.base,
                crate::TASK_EVIDENCE,
                self.patches.as_deref(),
            )?,
        };
        let (manager, group_id) = clear_preconditions(&catalog, &tasks, registration)?;
        // All preflight may have taken time: recheck complete target/task and manager before call.
        if catalog.character_tasks(target)? != Some(observation)
            || catalog
                .module
                .global_pointer(CHARACTER_TASK_MANAGER_GLOBAL)?
                != Some(manager)
        {
            return Err(Error::NativeOperationRejected(
                "character task preconditions changed",
            ));
        }
        catalog.require(manager, ObjectKind::CharacterTaskManager)?;
        let address = catalog.module.code_address(CLEAR_EVIDENCE[0])?;
        // SAFETY: Exact independently proved rcx=embedded task clear ABI; caller owns phase.
        let clear: ClearEntry = unsafe { std::mem::transmute(address as usize) };
        let ticket = CharacterTaskClearTicket {
            target,
            ordinal,
            phase_index: registration.phase_index,
            group_id,
            previous_proxy: registration.address,
        };
        // SAFETY: Preflight proves current links; caller guarantees quiescence/lifetime/exclusivity.
        unsafe { invoke_clear(clear, task.address) };
        // The old proxy can already be freed by native cancellation. Never read it after the call.
        Ok(clear_postflight(&memory, task.address, ticket))
    }
}

unsafe fn invoke_clear(entry: ClearEntry, task: u64) {
    // SAFETY: Caller supplies the exact native entry and exclusively live embedded task.
    unsafe { entry(task as *mut c_void) };
}

pub(crate) fn clear_postflight<M: AddressSpace>(
    memory: &M,
    task: u64,
    ticket: CharacterTaskClearTicket,
) -> CharacterTaskClear {
    let slot_clear = task
        .checked_add(0x10)
        .ok_or(Error::AddressOverflow)
        .and_then(|slot| memory.read_u64(slot))
        .and_then(|pointer| {
            if pointer == 0 {
                Ok(())
            } else {
                Err(Error::NativeOperationRejected(
                    "character task slot did not remain clear",
                ))
            }
        });
    CharacterTaskClear::Requested { ticket, slot_clear }
}

pub(crate) fn clear_preconditions<M: AddressSpace>(
    objects: &GameObjectCatalog<'_, '_, M>,
    tasks: &TaskCatalog<'_, '_, M>,
    registration: CharacterTaskRegistration,
) -> Result<(u64, u32)> {
    let manager = objects
        .module
        .global_pointer(CHARACTER_TASK_MANAGER_GLOBAL)?
        .ok_or(Error::NativeOperationRejected(
            "character task manager is absent",
        ))?;
    objects.require(manager, ObjectKind::CharacterTaskManager)?;
    crate::current_tasks::checked_system(&objects.module)?;
    let group =
        tasks
            .observe(registration.phase_index as usize)?
            .ok_or(Error::NativeOperationRejected(
                "character task group is absent",
            ))?;
    let group_id = group
        .native_group_id()
        .ok_or(Error::NativeOperationRejected(
            "character task group id is unassigned",
        ))?;
    if !tasks
        .task_registrations(group_id, 65536, 65536)?
        .is_some_and(|records| {
            records
                .iter()
                .any(|record| record.task_address == registration.address)
        })
    {
        // A pending registration cannot be cancelled before confirmed insertion.
        return Err(Error::NativeOperationRejected(
            "character task proxy is not confirmed inserted",
        ));
    }
    Ok((manager, group_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C)]
    struct Probe {
        padding: [u64; 2],
        slot: u64,
        calls: usize,
    }
    unsafe extern "system" fn clear_probe(pointer: *mut c_void) {
        // SAFETY: Tests pass one uniquely owned Probe, never game addresses/code.
        let probe = unsafe { &mut *pointer.cast::<Probe>() };
        probe.slot = 0;
        probe.calls += 1;
    }
    #[test]
    fn clear_abi_preserves_embedded_pointer_and_leaves_external_storage_owned() {
        let mut probe = Probe {
            padding: [1, 2],
            slot: 0x123400,
            calls: 0,
        };
        // SAFETY: Only a local Probe and independent synthetic entry are used.
        unsafe { invoke_clear(clear_probe, (&mut probe as *mut Probe) as u64) };
        assert_eq!((probe.slot, probe.calls, probe.padding), (0, 1, [1, 2]));
    }
    #[test]
    fn unknown_file_is_rejected_before_current_process_reads() {
        // SAFETY: Deliberately invalid file must reject before dereferencing impossible base.
        let result = unsafe { CurrentCharacterTasks::bind(Arc::from(vec![0; 128]), u64::MAX, 1) };
        assert!(matches!(result, Err(Error::UnsupportedImage { .. })));
    }
}
