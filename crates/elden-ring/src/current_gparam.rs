//! Experimental resource submissions. Return values do not prove load or render readiness.
use crate::{
    WW_171, Ww171File,
    gparam::{CommonEventId, FILE_REQUEST_SLOT, GPARAM_EVIDENCE},
    objects::{GameObjectCatalog, ObjectObservation},
};
use shikifsrs_runtime::{
    Error, Result,
    code_patches::OwnedCodePatches,
    memory::{AddressSpace, CurrentProcess},
    profile::OwnedVerifiedFile,
};
use std::{ffi::c_void, marker::PhantomData, rc::Rc, sync::Arc};

type RequestEntry = unsafe extern "system" fn(*mut c_void, *const u16, usize) -> *mut c_void;
type PrimeEntry = unsafe extern "system" fn(*mut c_void, u16, f32);

pub use crate::gparam::NativeGparamCall;

pub struct CurrentGparam {
    file: Arc<OwnedVerifiedFile>,
    base: u64,
    patches: Option<Arc<OwnedCodePatches>>,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentGparam {
    /// File/module/type/code verification only; no resource request, prime or allocation.
    /// # Safety
    /// base is this process's exact current ER module; caller retains module/SDK, all used
    /// initialized managers/repository/resource graphs, native heaps/allocators and callbacks.
    /// Native file allocation and request machinery must be valid for the entire call.
    /// Caller holds a permitted exclusive resource/DrawParam mutation phase, excludes all
    /// concurrent destruction, producer writes, object reuse/ABA, alias/unload and patching.
    /// Installed owned hooks require the manifest's transitive contracts. This Adapter does
    /// not acquire native reference guards, cancel requests or manage returned resource lifetime.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        let file = Arc::new(OwnedVerifiedFile::verify(bytes, WW_171)?);
        Self::from_verified(file, base, patches)
    }

    // Immutable file proof may cross threads; native sessions remain thread-confined.
    pub(crate) fn from_verified(
        file: Arc<OwnedVerifiedFile>,
        base: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        Ww171File::from_verified(file.borrow()?)?.bind_objects_with_patches(
            &CurrentProcess,
            base,
            patches.as_deref(),
        )?;
        Ok(Self {
            file,
            base,
            patches,
            _thread: PhantomData,
        })
    }

    /// Request the NUL-terminated common-event path with native constructor context0.
    /// # Safety
    /// bind's complete allocation/phase/lifetime contract holds. Native may allocate/queue
    /// even for null output. Nonnull address is a borrowed opaque result, never SDK ownership,
    /// readiness, a Rust reference or permission to destroy/free. Outer Err precedes invocation.
    pub unsafe fn request_common_event(
        &self,
        id: CommonEventId,
    ) -> Result<NativeGparamCall<Option<u64>>> {
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let objects =
            view.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?;
        let manager = objects
            .gparam_file_manager()?
            .ok_or(Error::NativeOperationRejected(
                "Gparam file manager is absent",
            ))?;
        // Exact leaf has no pdata; compare its independently proved readonly slot and
        // complete9-byte proxy rather than require a nonexistent exception-directory entry.
        let proxy = objects.module.code_address(GPARAM_EVIDENCE[11])?;
        let slot = self
            .base
            .checked_add(u64::from(FILE_REQUEST_SLOT))
            .ok_or(Error::AddressOverflow)?;
        if objects.module.memory().read_u64(slot)? != proxy {
            return Err(Error::ImageMismatch {
                rva: FILE_REQUEST_SLOT,
            });
        }
        let backend = request_backend(&objects, manager)?;
        let at = objects.module.code_address(GPARAM_EVIDENCE[1])?;
        // SAFETY: Complete independent request entry proves rcx manager,rdx NUL UTF16,r8 context,
        // RAX opaque virtual-request result. Caller guarantees initialized native allocation graph.
        let entry: RequestEntry = unsafe { std::mem::transmute(at as usize) };
        // SAFETY: Locally owned UTF16 remains initialized/alive through the synchronous call;
        // native copies its descriptor before submitting. Caller retains all native dependencies.
        let value = unsafe { invoke_request(entry, manager.address, id) };
        let postflight = objects
            .gparam_file_manager()
            .and_then(|now| same_manager(now, manager))
            .and_then(|()| {
                if request_backend(&objects, manager)? != backend {
                    return Err(Error::NativeOperationRejected(
                        "Gparam file backend changed after call",
                    ));
                }
                Ok(())
            });
        Ok(NativeGparamCall {
            manager,
            id,
            value,
            postflight,
        })
    }

    /// Native prime passes a finite float unchanged; consumer uses0.0.
    /// # Safety
    /// bind's DrawParam/repository/resource mutation and complete transitive lifetime contract
    /// holds. This may change resource pending IDs/flags and issue render-side work, even with
    /// void output. No ready/loaded acknowledgement, cancellation or automatic retry follows.
    pub unsafe fn prime_common_event(
        &self,
        id: CommonEventId,
        time: f32,
    ) -> Result<NativeGparamCall<()>> {
        validate_time(time)?;
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let objects =
            view.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?;
        let manager = objects
            .gparam_draw_manager()?
            .ok_or(Error::NativeOperationRejected(
                "DrawParam manager is absent",
            ))?;
        let repository = objects
            .gparam_repository()?
            .ok_or(Error::NativeOperationRejected(
                "Gparam repository is absent",
            ))?;
        let at = objects.module.code_address(GPARAM_EVIDENCE[0])?;
        // SAFETY: Complete native entry proves rcx manager,DX ID,xmm2 float,ignored return;
        // caller supplies exclusive live transitive DrawParam/resource/heap graph.
        let entry: PrimeEntry = unsafe { std::mem::transmute(at as usize) };
        // SAFETY: Caller contract and full preflight hold through the native call.
        unsafe { invoke_prime(entry, manager.address, id, time) };
        let postflight = objects
            .gparam_draw_manager()
            .and_then(|now| same_manager(now, manager))
            .and_then(|()| {
                objects
                    .gparam_repository()
                    .and_then(|now| same_manager(now, repository))
            });
        Ok(NativeGparamCall {
            manager,
            id,
            value: (),
            postflight,
        })
    }
}
fn request_backend<M: AddressSpace>(
    objects: &GameObjectCatalog<'_, '_, M>,
    manager: ObjectObservation,
) -> Result<u64> {
    let at = manager
        .address
        .checked_add(8)
        .ok_or(Error::AddressOverflow)?;
    let pointer = objects.module.memory().read_u64(at)?;
    if pointer == 0 || !pointer.is_multiple_of(8) {
        return Err(Error::NativeOperationRejected(
            "Gparam file backend is absent or unaligned",
        ));
    }
    if objects.module.memory().read_u64(at)? != pointer
        || objects.gparam_file_manager()? != Some(manager)
    {
        return Err(Error::NativeOperationRejected(
            "Gparam file backend changed",
        ));
    }
    Ok(pointer)
}
fn validate_time(time: f32) -> Result<()> {
    if !time.is_finite() {
        return Err(Error::InvalidProfile("nonfinite Gparam prime time"));
    }
    Ok(())
}
fn same_manager(now: Option<ObjectObservation>, expected: ObjectObservation) -> Result<()> {
    if now != Some(expected) {
        return Err(Error::NativeOperationRejected(
            "Gparam manager changed after call",
        ));
    }
    Ok(())
}
unsafe fn invoke_request(entry: RequestEntry, manager: u64, id: CommonEventId) -> Option<u64> {
    let path = id.wide_path();
    // SAFETY: Caller supplies native entry/manager lifetime/phase; owned path is NUL terminated.
    let result = unsafe { entry(manager as *mut c_void, path.as_ptr(), 0) } as u64;
    (result != 0).then_some(result)
}
unsafe fn invoke_prime(entry: PrimeEntry, manager: u64, id: CommonEventId, time: f32) {
    // SAFETY: Caller supplies native entry/manager transitive lifetime/phase/finite time.
    unsafe { entry(manager as *mut c_void, id.value(), time) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::ObjectKind;
    use std::cell::Cell;
    struct Mock {
        id: u16,
        time: u32,
        result: u64,
        calls: Cell<usize>,
    }
    unsafe extern "system" fn request(
        manager: *mut c_void,
        path: *const u16,
        context: usize,
    ) -> *mut c_void {
        // SAFETY: Test passes live owned Mock and a live initialized UTF16 allocation.
        let mock = unsafe { &*(manager as *const Mock) };
        assert_eq!(context, 0);
        let expected = CommonEventId::new(mock.id as u32).unwrap().wide_path();
        for (n, word) in expected.iter().enumerate() {
            // SAFETY: Test helper generated the same bounded NUL-terminated owned path.
            assert_eq!(unsafe { path.add(n).read() }, *word);
        }
        mock.calls.set(mock.calls.get() + 1);
        mock.result as *mut c_void
    }
    unsafe extern "system" fn prime(manager: *mut c_void, id: u16, time: f32) {
        // SAFETY: Original Rust entry receives a live test Mock; no game code executed.
        let mock = unsafe { &*(manager as *const Mock) };
        assert_eq!(id, mock.id);
        assert_eq!(time.to_bits(), mock.time);
        mock.calls.set(mock.calls.get() + 1);
    }
    #[test]
    fn request_abi_keeps_path_alive_passes_zero_context_and_retains_null_attempt() {
        for id in [0, 5, 65535] {
            for result in [0, 0x12345678] {
                let mock = Mock {
                    id,
                    time: 0,
                    result,
                    calls: Cell::new(0),
                };
                // SAFETY: Original Rust ABI entry and self-authored live Mock/storage only.
                let actual = unsafe {
                    invoke_request(
                        request,
                        &mock as *const Mock as u64,
                        CommonEventId::new(id as u32).unwrap(),
                    )
                };
                assert_eq!(actual, (result != 0).then_some(result));
                assert_eq!(mock.calls.get(), 1);
            }
        }
    }
    #[test]
    fn prime_abi_passes_low16_id_and_original_finite_float_bits() {
        for time in [0.0_f32, -0.0, 1.5, -1.5] {
            for id in [0, 5, 65535] {
                let mock = Mock {
                    id,
                    time: time.to_bits(),
                    result: 0,
                    calls: Cell::new(0),
                };
                validate_time(time).unwrap();
                // SAFETY: Original entry receives live self-authored Mock; no native game entry.
                unsafe {
                    invoke_prime(
                        prime,
                        &mock as *const Mock as u64,
                        CommonEventId::new(id as u32).unwrap(),
                        time,
                    )
                };
                assert_eq!(mock.calls.get(), 1);
            }
        }
        for time in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(validate_time(time).is_err());
        }
    }
    #[test]
    fn gparam_receipt_retains_original_call_result_on_manager_postflight_failure() {
        let manager = ObjectObservation {
            address: 0x1000,
            kind: ObjectKind::FileResourceManager,
        };
        let call = NativeGparamCall {
            manager,
            id: CommonEventId::new(5).unwrap(),
            value: Some(0x2000u64),
            postflight: same_manager(None, manager),
        };
        assert_eq!(call.value, Some(0x2000));
        assert!(call.postflight.is_err());
        assert!(
            same_manager(
                Some(ObjectObservation {
                    address: 0x1008,
                    ..manager
                }),
                manager
            )
            .is_err()
        );
        assert!(
            same_manager(
                Some(ObjectObservation {
                    kind: ObjectKind::GparamRepository,
                    ..manager
                }),
                manager
            )
            .is_err()
        );
        assert_eq!(same_manager(Some(manager), manager), Ok(()));
    }
}
