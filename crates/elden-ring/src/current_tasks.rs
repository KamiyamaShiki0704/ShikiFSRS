//! WW 1.17.1 native entry bindings, compiled but not game-runtime accepted.
use crate::{
    REGISTRY_VTABLE, TASK_SYSTEM_GLOBAL, TIME_VTABLE, TaskCatalog, TaskInvocation, TaskPhase,
    WW_171, Ww171File,
    native_task::{NativeTaskObject, RegistrationReceipt, RegistrationState, TaskEngine},
};
use shikifsrs_runtime::code_patches::OwnedCodePatches;
use shikifsrs_runtime::{
    Error, Result,
    memory::{AddressSpace, CurrentProcess},
    profile::{CodeRange, OwnedVerifiedFile, VerifiedModule},
};
use std::{alloc::Layout, ffi::c_void, ptr::NonNull, sync::Arc};

const SYSTEM_VTABLE: u32 = 0x3296e10;
pub(crate) const NATIVE_TASK_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x2658510,
        length: 0x52,
    },
    CodeRange {
        rva: 0x2657e50,
        length: 0x687,
    },
    CodeRange {
        rva: 0x2657880,
        length: 0xbe,
    },
    CodeRange {
        rva: 0x1ebbd40,
        length: 0x13,
    },
    CodeRange {
        rva: 0x1ec7c50,
        length: 0x4e,
    },
    CodeRange {
        rva: 0x1267a0,
        length: 0x49,
    },
    CodeRange {
        rva: 0x26585d0,
        length: 0x8,
    },
    CodeRange {
        rva: 0x26a1a50,
        length: 0xcb,
    },
    CodeRange {
        rva: 0x26a3e50,
        length: 0x5f,
    },
    CodeRange {
        rva: 0x26d74e0,
        length: 0x1c8,
    },
    CodeRange {
        rva: 0x26a3410,
        length: 0xc1,
    },
    CodeRange {
        rva: 0x26a34e0,
        length: 0xd9,
    },
];

/// In-process engine backend. Binding verifies bytes; it never calls the game.
/// Native operations still require explicit unsafe scheduling/lifetime guarantees.
pub struct CurrentTaskEngine {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    heap: usize,
    heap_table: u64,
    allocate_code: CodeRange,
    free_code: CodeRange,
}

impl CurrentTaskEngine {
    /// # Safety
    /// `base` is the current process's module, heap/module and this SDK DLL stay
    /// resident through every submitted task. No concurrent code/vtable patch or
    /// unrelated teardown may overlap operations. Actual game behavior remains
    /// unaccepted until human runtime validation; remote-process calls are unsupported.
    pub unsafe fn bind(bytes: Arc<[u8]>, base: u64) -> Result<Arc<Self>> {
        // SAFETY: Original bind obligations; strict unpatched policy.
        unsafe { Self::bind_with_patches(bytes, base, None) }
    }

    /// Bind under a declared owned hook policy.
    /// # Safety
    /// bind and the manifest owner's transitive ABI, semantics, lifetime and patch quiescence
    /// obligations hold through task destruction, engine free and every callback/return frame.
    pub unsafe fn bind_with_patches(
        bytes: Arc<[u8]>,
        base: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Arc<Self>> {
        // File identity precedes ALL current-process reads.
        let file = OwnedVerifiedFile::verify(bytes, WW_171)?;
        let view = Ww171File::from_verified(file.borrow()?)?;
        let memory = CurrentProcess;
        let catalog = view.bind_with_patches(&memory, base, patches.as_deref())?;
        for range in NATIVE_TASK_EVIDENCE {
            catalog.module.code_address(*range)?;
        }
        let system = checked_system(&catalog.module)?;
        let heap = memory.read_u64(at(system, 8)?)?;
        if heap == 0 || !heap.is_multiple_of(8) {
            return Err(Error::InvalidProfile(
                "task system allocator is missing or unaligned",
            ));
        }
        let table = memory.read_u64(heap)?;
        let allocate_code = catalog.module.checked_virtual_function(table, 0x50)?;
        let free_code = catalog.module.checked_virtual_function(table, 0x68)?;
        Ok(Arc::new(Self {
            file,
            patches,
            base,
            heap: heap as usize,
            heap_table: table,
            allocate_code,
            free_code,
        }))
    }

    fn checked<R>(
        &self,
        action: impl FnOnce(&TaskCatalog<'_, '_, CurrentProcess>, u64) -> Result<R>,
    ) -> Result<R> {
        let file = self.file.borrow()?;
        let memory = CurrentProcess;
        // The owned file was fully verified at binding; ordinary calls recheck loaded headers/code.
        let catalog = TaskCatalog {
            module: file.bind_with_patches(
                &memory,
                self.base,
                crate::TASK_EVIDENCE,
                self.patches.as_deref(),
            )?,
        };
        let system = checked_system(&catalog.module)?;
        if memory.read_u64(at(system, 8)?)? != self.heap as u64 {
            return Err(Error::InvalidProfile("task allocator identity changed"));
        }
        action(&catalog, system)
    }

    fn heap_slot(
        &self,
        module: &VerifiedModule<'_, '_, CurrentProcess>,
        offset: u32,
        expected: CodeRange,
    ) -> Result<u64> {
        let table = module.memory().read_u64(self.heap as u64)?;
        if table != self.heap_table {
            return Err(Error::InvalidProfile("allocator virtual table changed"));
        }
        let actual = module.checked_virtual_function(table, offset)?;
        if (actual.rva, actual.length) != (expected.rva, expected.length) {
            return Err(Error::InvalidProfile("allocator entry changed"));
        }
        module.code_address(actual)
    }

    fn code(&self, module: &VerifiedModule<'_, '_, CurrentProcess>, rva: u32) -> Result<usize> {
        let range = NATIVE_TASK_EVIDENCE
            .iter()
            .find(|range| range.rva == rva)
            .ok_or(Error::InvalidProfile("native entry has no evidence"))?;
        Ok(module.code_address(*range)? as usize)
    }
}

fn at(address: u64, offset: u64) -> Result<u64> {
    address.checked_add(offset).ok_or(Error::AddressOverflow)
}

pub(crate) fn checked_system<M: AddressSpace>(module: &VerifiedModule<'_, '_, M>) -> Result<u64> {
    let system =
        module
            .global_pointer(TASK_SYSTEM_GLOBAL)?
            .ok_or(Error::NativeOperationRejected(
                "task system is uninitialized",
            ))?;
    let memory = module.memory();
    if memory.read_u64(system)? != at(module.base(), SYSTEM_VTABLE as u64)? {
        return Err(Error::InvalidProfile("unexpected task system type"));
    }
    if memory.read_u64(at(system, 0x40)?)? == 0 {
        return Err(Error::NativeOperationRejected(
            "native scheduler is missing",
        ));
    }
    let registry = memory.read_u64(at(system, 0x48)?)?;
    if registry == 0 || memory.read_u64(registry)? != at(module.base(), REGISTRY_VTABLE as u64)? {
        return Err(Error::InvalidProfile(
            "native registry is missing or has the wrong type",
        ));
    }
    if memory.read_u8(at(system, 0x38)?)? > 1 {
        return Err(Error::InvalidProfile("invalid dispatch flag"));
    }
    Ok(system)
}

// SAFETY: Version/code/heap/virtual-slot checks precede every native operation;
// external scheduling/module lifetime obligations are explicit at bind and operation sites.
// Invocation copying uses checked process reads and the proven FD4Time field layout.
unsafe impl TaskEngine for CurrentTaskEngine {
    fn allocator_address(&self) -> usize {
        self.heap
    }

    unsafe fn allocate(&self, layout: Layout) -> Result<NonNull<u8>> {
        self.checked(|catalog, _| {
            self.heap_slot(&catalog.module, 0x50, self.allocate_code)?;
            let entry = self.code(&catalog.module, 0x1ebbd40)?;
            type Allocate = unsafe extern "system" fn(usize, usize, *mut c_void) -> *mut u8;
            // SAFETY: Exact independently proved thunk and Windows AMD64 pointer width.
            let allocate: Allocate = unsafe { std::mem::transmute(entry) };
            // SAFETY: Caller owns allocator phase; thunk forwards size/alignment/heap to +0x50.
            NonNull::new(unsafe {
                allocate(layout.size(), layout.align(), self.heap as *mut c_void)
            })
            .ok_or(Error::NativeAllocationFailed)
        })
    }

    unsafe fn deallocate(&self, allocation: NonNull<u8>, _layout: Layout) -> Result<()> {
        self.checked(|catalog, _| {
            type QueryHeap = unsafe extern "system" fn(*mut u8) -> *mut c_void;
            let entry = self.code(&catalog.module, 0x1ec7c50)?;
            // SAFETY: Exact allocator-owner lookup ABI from the native cancellation caller.
            let query: QueryHeap = unsafe { std::mem::transmute(entry) };
            // SAFETY: Caller guarantees the live allocation; heap ownership is checked before free.
            if unsafe { query(allocation.as_ptr()) } as usize != self.heap {
                return Err(Error::InvalidProfile(
                    "allocation owner differs from the task heap",
                ));
            }
            let address = self.heap_slot(&catalog.module, 0x68, self.free_code)?;
            type Free = unsafe extern "system" fn(*mut c_void, *mut u8);
            // SAFETY: Immutable file slot, exact loaded entry bytes, proven virtual free signature.
            let free: Free = unsafe { std::mem::transmute(address as usize) };
            // SAFETY: Same native heap owns this unique, already-destructed allocation.
            unsafe {
                free(self.heap as *mut c_void, allocation.as_ptr());
            }
            Ok(())
        })
    }

    unsafe fn register(
        &self,
        phase: TaskPhase,
        task: NonNull<NativeTaskObject>,
    ) -> Result<RegistrationReceipt> {
        self.checked(|catalog, system| {
            let pending = catalog.module.memory().read_u8(at(system, 0x38)?)? != 0;
            let observation =
                catalog
                    .observe_phase(phase)?
                    .ok_or(Error::NativeOperationRejected(
                        "task phase is uninitialized",
                    ))?;
            let id = observation
                .native_group_id()
                .ok_or(Error::NativeOperationRejected(
                    "phase group id is unassigned",
                ))?;
            if catalog.registered_group(id, 65_536)?.is_none() {
                return Err(Error::NativeOperationRejected(
                    "phase is absent from the native registry",
                ));
            }
            let range = crate::TASK_EVIDENCE
                .iter()
                .find(|range| range.rva == 0xeb3e50)
                .ok_or(Error::InvalidProfile(
                    "registration wrapper has no evidence",
                ))?;
            let address = catalog.module.code_address(*range)?;
            for range in NATIVE_TASK_EVIDENCE {
                catalog.module.code_address(*range)?;
            }
            type Register =
                unsafe extern "system" fn(*mut c_void, u32, *mut NativeTaskObject) -> u8;
            // SAFETY: Native wrapper consumes edx phase index/r8 task; rcx is unused.
            let register: Register = unsafe { std::mem::transmute(address as usize) };
            // SAFETY: Caller owns the native phase. Dispatch-active submission queues into +0x20;
            // this callback cannot finish its group before the receipt is recorded on return.
            let accepted =
                unsafe { register(ptr_null(), phase.registration_index() as u32, task.as_ptr()) };
            if accepted == 0 {
                return Err(Error::NativeOperationRejected(
                    "native task registration failed",
                ));
            }
            Ok(RegistrationReceipt {
                group_id: id,
                state: if pending {
                    RegistrationState::Pending
                } else {
                    RegistrationState::Inserted
                },
            })
        })
    }

    unsafe fn registration_inserted(&self, group_id: u32, task_address: usize) -> Result<bool> {
        self.checked(|catalog, _| {
            Ok(catalog
                .task_registrations(group_id, 65_536, 65_536)?
                .is_some_and(|records| {
                    records
                        .iter()
                        .any(|record| record.task_address == task_address as u64)
                }))
        })
    }

    unsafe fn unregister(&self, group_id: u32, task_address: usize) -> Result<()> {
        self.checked(|catalog, system| {
            let entry = self.code(&catalog.module, 0x2657880)?;
            type Unregister =
                unsafe extern "system" fn(*mut c_void, *mut NativeTaskObject, u32, u8) -> u8;
            // SAFETY: Independently proved system/task/full-group/free-task argument order.
            let unregister: Unregister = unsafe { std::mem::transmute(entry) };
            // SAFETY: Caller owns cancellation phase and task lifetime; free flag requests native deletion.
            let accepted = unsafe {
                unregister(
                    system as *mut c_void,
                    task_address as *mut NativeTaskObject,
                    group_id,
                    1,
                )
            };
            if accepted == 0 {
                return Err(Error::NativeOperationRejected(
                    "native task unregistration failed",
                ));
            }
            Ok(())
        })
    }

    fn read_invocation(&self, address: usize) -> Result<TaskInvocation> {
        crate::copy_invocation(
            &CurrentProcess,
            address as u64,
            at(self.base, TIME_VTABLE as u64)?,
        )
    }
}

fn ptr_null() -> *mut c_void {
    std::ptr::null_mut()
}
