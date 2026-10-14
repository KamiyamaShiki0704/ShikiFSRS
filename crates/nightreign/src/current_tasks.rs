//! Experimental Windows in-process NR backend. Binding never executes native code.
use crate::{
    EXE_1330, NightreignFile, native_evidence,
    native_tasks::{NativeTaskObject, RegistrationReceipt, TaskEngine, TaskInvocation, TaskPhase},
    reader::ReaderCore,
    roots,
};
use shikifsrs_runtime::{
    Error, Result,
    memory::{AddressSpace, CurrentProcess},
    profile::{CodeRange, OwnedVerifiedFile, VerifiedModule},
};
use std::{alloc::Layout, ffi::c_void, ptr::NonNull, sync::Arc};

pub struct CurrentTaskEngine {
    file: OwnedVerifiedFile,
    base: u64,
    heap: u64,
    heap_table: u64,
    type_info: u64,
    allocate_slot: CodeRange,
    free_slot: CodeRange,
    code: Vec<CodeRange>,
}
impl CurrentTaskEngine {
    /// # Safety
    /// Base identifies this process's NR module. Caller prevents code/vtable patches,
    /// unrelated teardown and address reuse, owns supported allocator/scheduling phases,
    /// and keeps module/heap/type metadata/SDK DLL resident through every foreign frame.
    /// Actual game behavior has not been accepted by in-game validation.
    pub unsafe fn bind(bytes: Arc<[u8]>, base: u64) -> Result<Arc<Self>> {
        // Exact immutable file identity precedes all current-process reads.
        let file = OwnedVerifiedFile::verify(bytes, EXE_1330)?;
        let view = NightreignFile::from_verified(file.borrow()?)?;
        let memory = CurrentProcess;
        let reader = view.bind_reader(&memory, base)?;
        let system = reader.task_system()?.ok_or(Error::NativeOperationRejected(
            "NR task system is uninitialized",
        ))?;
        let table = memory.read_u64(system.allocator)?;
        let allocate_slot = reader.module.checked_virtual_function(table, 0x50)?;
        let free_slot = reader.module.checked_virtual_function(table, 0x68)?;
        let type_info = reader
            .module
            .global_pointer(roots::TYPE_INFO_GLOBAL)?
            .ok_or(Error::NativeOperationRejected(
                "NR base task type is uninitialized",
            ))?;
        if memory.read_u64(type_info)?
            != base
                .checked_add(roots::TYPE_INFO_TABLE as u64)
                .ok_or(Error::AddressOverflow)?
        {
            return Err(Error::InvalidProfile(
                "unexpected NR base task type metadata",
            ));
        }
        Ok(Arc::new(Self {
            file,
            base,
            heap: system.allocator,
            heap_table: table,
            type_info,
            allocate_slot,
            free_slot,
            code: native_evidence()?,
        }))
    }

    fn checked<R>(
        &self,
        action: impl FnOnce(
            &VerifiedModule<'_, '_, CurrentProcess>,
            &ReaderCore<'_, CurrentProcess>,
            roots::TaskSystemSnapshot,
        ) -> Result<R>,
    ) -> Result<R> {
        let file = self.file.borrow()?;
        let memory = CurrentProcess;
        let module = file.bind(&memory, self.base, &self.code)?;
        let core = ReaderCore {
            memory: &memory,
            base: self.base,
        };
        let system = core.task_system()?.ok_or(Error::NativeOperationRejected(
            "NR task system is uninitialized",
        ))?;
        if system.allocator != self.heap
            || memory.read_u64(self.heap)? != self.heap_table
            || module.global_pointer(roots::TYPE_INFO_GLOBAL)? != Some(self.type_info)
            || memory.read_u64(self.type_info)?
                != self
                    .base
                    .checked_add(roots::TYPE_INFO_TABLE as u64)
                    .ok_or(Error::AddressOverflow)?
        {
            return Err(Error::InvalidProfile("NR task heap/type metadata changed"));
        }
        action(&module, &core, system)
    }
    fn entry(&self, module: &VerifiedModule<'_, '_, CurrentProcess>, rva: u32) -> Result<usize> {
        let range = self
            .code
            .iter()
            .find(|range| range.rva == rva)
            .ok_or(Error::InvalidProfile("NR entry has no code evidence"))?;
        Ok(module.code_address(*range)? as usize)
    }
    fn heap_slot(
        &self,
        module: &VerifiedModule<'_, '_, CurrentProcess>,
        offset: u32,
        expected: CodeRange,
    ) -> Result<usize> {
        let actual = module.checked_virtual_function(self.heap_table, offset)?;
        if actual.rva != expected.rva || actual.length != expected.length {
            return Err(Error::InvalidProfile("NR heap virtual entry changed"));
        }
        Ok(module.code_address(actual)? as usize)
    }
}

// SAFETY: Exact file/header/code/heap checks precede native operations. Unsafe host phase,
// serialization and residency obligations are explicit at binding and TaskEngine operations.
unsafe impl TaskEngine for CurrentTaskEngine {
    fn type_info_address(&self) -> usize {
        self.type_info as usize
    }
    unsafe fn allocate(&self, layout: Layout) -> Result<NonNull<u8>> {
        self.checked(|module, _, _| {
            self.heap_slot(module, 0x50, self.allocate_slot)?;
            type Allocate = unsafe extern "system" fn(usize, usize, *mut c_void) -> *mut u8;
            // SAFETY: NR thunk forwards size/alignment/heap to the proved heap +0x50 ABI.
            let allocate: Allocate = unsafe { std::mem::transmute(self.entry(module, 0x214e8d0)?) };
            // SAFETY: Caller owns allocator phase and heap residency.
            NonNull::new(unsafe {
                allocate(layout.size(), layout.align(), self.heap as *mut c_void)
            })
            .ok_or(Error::NativeAllocationFailed)
        })
    }
    unsafe fn deallocate(&self, allocation: NonNull<u8>, _layout: Layout) -> Result<()> {
        self.checked(|module, _, _| {
            type Owner = unsafe extern "system" fn(*mut u8) -> *mut c_void;
            // SAFETY: Same owner-query ABI used by NR native task deletion.
            let owner: Owner = unsafe { std::mem::transmute(self.entry(module, 0x215cdb0)?) };
            // SAFETY: Backend contract supplies a live allocation from this heap.
            if unsafe { owner(allocation.as_ptr()) } as u64 != self.heap {
                return Err(Error::InvalidProfile(
                    "NR allocation owner differs from task heap",
                ));
            }
            type Free = unsafe extern "system" fn(*mut c_void, *mut u8);
            // SAFETY: Native deletion calls heap +0x68 with this/unique allocation.
            let free: Free =
                unsafe { std::mem::transmute(self.heap_slot(module, 0x68, self.free_slot)?) };
            // SAFETY: Payload destroyed; same owning native heap; caller supplies free phase.
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
        self.checked(|module, core, system| {
            let id =
                core.group_id(phase.index() as usize)?
                    .ok_or(Error::NativeOperationRejected(
                        "NR task phase is uninitialized",
                    ))?;
            // Validate the selected native group even before an object is inserted.
            let tree = core.u64(system.registry, 0x18)?;
            if shikifsrs_runtime::native_tree::U32PointerTree::new(core.memory, tree, 65536)?
                .find(id)?
                .is_none()
            {
                return Err(Error::NativeOperationRejected(
                    "NR task phase is absent from registry",
                ));
            }
            if system.dispatching {
                let queue = core.u64(system.address, 0x20)?;
                if queue == 0 || core.u64(queue, 0x10)? == 0 {
                    return Err(Error::NativeOperationRejected(
                        "NR registration queue is unavailable",
                    ));
                }
            }
            let receipt = RegistrationReceipt::new(id, system.dispatching)?;
            type Register =
                unsafe extern "system" fn(*mut c_void, u32, *mut NativeTaskObject) -> u8;
            // SAFETY: NR wrapper ignores rcx, consumes edx phase index and r8 task.
            let register: Register = unsafe { std::mem::transmute(self.entry(module, 0xfec580)?) };
            // SAFETY: Host serializes scheduling until receipt recording, including queued mode.
            if unsafe { register(std::ptr::null_mut(), phase.index() as u32, task.as_ptr()) } == 0 {
                return Err(Error::NativeOperationRejected(
                    "NR task registration rejected",
                ));
            }
            Ok(receipt)
        })
    }
    unsafe fn inserted(&self, group_id: u32, address: usize) -> Result<bool> {
        self.checked(|_, core, _| core.registration_inserted(group_id, address as u64, 65536))
    }
    unsafe fn unregister(&self, group_id: u32, address: usize) -> Result<()> {
        self.checked(|module, core, system| {
            if !core.registration_inserted(group_id, address as u64, 65536)? {
                return Err(Error::NativeOperationRejected(
                    "NR task insertion is not confirmed",
                ));
            }
            type Unregister = unsafe extern "system" fn(*mut c_void, *mut NativeTaskObject) -> u8;
            // SAFETY: NR all-group cancellation sets native free-task flag; each SDK object
            // is submitted once into one group, validated before entering this call.
            let unregister: Unregister =
                unsafe { std::mem::transmute(self.entry(module, 0x27d8110)?) };
            // SAFETY: Host owns cancellation phase and preserves task/DLL/heap lifetime.
            if unsafe {
                unregister(
                    system.address as *mut c_void,
                    address as *mut NativeTaskObject,
                )
            } == 0
            {
                return Err(Error::NativeOperationRejected(
                    "NR task cancellation rejected",
                ));
            }
            Ok(())
        })
    }
    fn invocation(&self, address: usize) -> Result<TaskInvocation> {
        let memory = CurrentProcess;
        let address = address as u64;
        if address == 0
            || !address.is_multiple_of(8)
            || memory.read_u64(address)?
                != self
                    .base
                    .checked_add(0x2bda590)
                    .ok_or(Error::AddressOverflow)?
        {
            return Err(Error::InvalidProfile("unexpected NR task invocation type"));
        }
        Ok(TaskInvocation {
            scalar_08: memory.read_f32(address.checked_add(8).ok_or(Error::AddressOverflow)?)?,
            group_id: memory.read_u32(address.checked_add(0x10).ok_or(Error::AddressOverflow)?)?,
            word_14: memory.read_u32(address.checked_add(0x14).ok_or(Error::AddressOverflow)?)?,
        })
    }
}
