//! Actual native global/area handle routing. Copied results grant no allocation ownership.
use crate::{
    WW_171, Ww171File,
    character_handles::{HANDLE_EVIDENCE, NativeCharacterHandle},
    character_sets::CharacterIdentity,
    objects::{ObjectKind, ObjectObservation},
};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterLookupSource {
    pub world: ObjectObservation,
    pub epoch: u64,
    pub handle: NativeCharacterHandle,
}
#[derive(Debug)]
pub struct NativeCharacterLookupCall {
    pub source: CharacterLookupSource,
    /// Actual RAX value survives a rejected identity or world postflight.
    pub returned_address: u64,
    /// Null is a successful native absent result, distinct from failed postflight.
    pub postflight: Result<Option<CharacterIdentity>>,
}
/// Confined to its binding thread. Neither a native lease nor a callback/destructor barrier.
pub struct CurrentCharacterLookup {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
fn preflight(
    current: Option<ObjectObservation>,
    expected: CharacterLookupSource,
    epoch: u64,
) -> Result<()> {
    if expected.epoch == 0
        || expected.epoch != epoch
        || expected.world.kind != ObjectKind::WorldCharacters
        || expected.world.address == 0
        || !expected.world.address.is_multiple_of(8)
        || current != Some(expected.world)
    {
        return Err(Error::NativeOperationRejected(
            "native character lookup world/epoch superseded",
        ));
    }
    Ok(())
}
fn postflight(
    source: CharacterLookupSource,
    returned: u64,
    value: Option<CharacterIdentity>,
) -> Result<Option<CharacterIdentity>> {
    match value {
        None if returned == 0 => Ok(None),
        Some(target)
            if returned != 0
                && target.address == returned
                && target.epoch == source.epoch
                && target.world_address == source.world.address
                && target.native_identifier == source.handle.0 =>
        {
            Ok(Some(target))
        }
        _ => Err(Error::NativeOperationRejected(
            "native character lookup returned identity mismatch",
        )),
    }
}
type Entry = unsafe extern "system" fn(u64, *const u64) -> u64;
unsafe fn invoke(
    entry: Entry,
    source: CharacterLookupSource,
    observe: impl FnOnce(u64) -> Result<Option<CharacterIdentity>>,
) -> NativeCharacterLookupCall {
    // SAFETY: Caller retains the full allowed native graph/phase. This copied input word lives
    // through exactly one complete RCX world / RDX pointer-to-full-handle / RAX character call.
    let returned_address = unsafe { entry(source.world.address, &source.handle.0) };
    NativeCharacterLookupCall {
        source,
        returned_address,
        postflight: observe(returned_address)
            .and_then(|value| postflight(source, returned_address, value)),
    }
}
impl CurrentCharacterLookup {
    /// # Safety
    /// Retain the exact live WorldChrMan and every global/area/block/set/character allocation,
    /// all transitive native calls/virtual dispatch/code and engine readers in an exclusive allowed
    /// lookup phase. Exclude unload, teardown, address reuse, callbacks and Rust aliases. Checks
    /// acquire no lock, ownership or lease. Native area routing may log/assert on invalid state.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero character lookup epoch"));
        }
        let file = OwnedVerifiedFile::verify(bytes, WW_171)?;
        Ww171File::from_verified(file.borrow()?)?.bind_objects_with_patches(
            &CurrentProcess,
            base,
            patches.as_deref(),
        )?;
        Ok(Self {
            file,
            patches,
            base,
            epoch,
            _thread: PhantomData,
        })
    }
    /// Copy the actual typed world; the opaque handle is not resolved or normalized here.
    pub fn observe(&self, handle: NativeCharacterHandle) -> Result<Option<CharacterLookupSource>> {
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .world()
            .map(|value| {
                value.map(|world| CharacterLookupSource {
                    world,
                    epoch: self.epoch,
                    handle,
                })
            })
    }
    /// # Safety
    /// Bind's full graph/phase contract holds through native routing and copied postflight.
    /// Publish an unresolved attempt before calling; retain returned address and failed postflight.
    /// A result does not authorize later native borrowing, mutation, deletion or automatic retry.
    pub unsafe fn lookup(
        &self,
        source: CharacterLookupSource,
    ) -> Result<NativeCharacterLookupCall> {
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let objects =
            file.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?;
        preflight(objects.world()?, source, self.epoch)?;
        let address = objects.module.code_address(HANDLE_EVIDENCE[0])?;
        // SAFETY: Independently decoded full508970 entry has RCX/RDX/RAX Windows x64 ABI.
        let entry = unsafe { std::mem::transmute::<usize, Entry>(address as usize) };
        // SAFETY: Caller supplies native full graph; no transitive game call is made during tests.
        Ok(unsafe {
            invoke(entry, source, |returned| {
                let value = if returned == 0 {
                    None
                } else {
                    objects.character_identity(returned, self.epoch)?
                };
                if objects.world()? != Some(source.world) {
                    return Err(Error::NativeOperationRejected(
                        "world changed after native character lookup",
                    ));
                }
                if let Some(target) = value
                    && objects.character_identity(target.address, self.epoch)? != Some(target)
                {
                    return Err(Error::NativeOperationRejected(
                        "lookup target changed after world postflight",
                    ));
                }
                Ok(value)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    struct Mock {
        calls: Cell<u32>,
        handle: Cell<u64>,
        returned: u64,
    }
    unsafe extern "system" fn entry(world: u64, handle: *const u64) -> u64 {
        // SAFETY: Only this test's original owned Mock and invoke's initialized full handle word.
        let mock = unsafe { &*(world as *const Mock) };
        mock.calls.set(mock.calls.get() + 1);
        // SAFETY: invoke owns this disjoint input word for the duration of the call.
        mock.handle.set(unsafe { *handle });
        mock.returned
    }
    fn source(mock: &Mock, handle: u64) -> CharacterLookupSource {
        CharacterLookupSource {
            world: ObjectObservation {
                address: std::ptr::from_ref(mock) as u64,
                kind: ObjectKind::WorldCharacters,
            },
            epoch: 7,
            handle: NativeCharacterHandle(handle),
        }
    }
    fn target(source: CharacterLookupSource, address: u64) -> CharacterIdentity {
        CharacterIdentity {
            address,
            world_address: source.world.address,
            native_identifier: source.handle.0,
            epoch: source.epoch,
        }
    }
    #[test]
    fn actual_lookup_dispatch_preserves_full_global_and_area_handles_and_null_return() {
        for (handle, returned) in [
            (0x0000000010020003, 0x620000),
            (0x3c2a240010020003, 0x630000),
            (0x12345678ffffffff, 0),
        ] {
            let mock = Mock {
                calls: Cell::new(0),
                handle: Cell::new(0),
                returned,
            };
            let source = source(&mock, handle);
            preflight(Some(source.world), source, 7).unwrap();
            // SAFETY: Owned mock only, no game/native process access.
            let receipt = unsafe {
                invoke(entry, source, |address| {
                    Ok((address != 0).then(|| target(source, address)))
                })
            };
            assert_eq!(mock.calls.get(), 1);
            assert_eq!(mock.handle.get(), handle);
            assert_eq!(receipt.returned_address, returned);
            assert!(receipt.postflight.is_ok());
        }
    }
    #[test]
    fn lookup_retains_one_actual_return_when_postflight_fails_and_never_retries() {
        for mode in 0..7 {
            let mock = Mock {
                calls: Cell::new(0),
                handle: Cell::new(0),
                returned: 0x620000,
            };
            let source = source(&mock, 0x3c2a240010020003);
            // SAFETY: One owned mock ABI invocation; only the copied validation result changes.
            let receipt = unsafe {
                invoke(entry, source, |_| {
                    if mode == 0 {
                        return Err(Error::InvalidProfile("actual postflight failed"));
                    }
                    if mode == 1 {
                        return Ok(None);
                    }
                    let mut v = target(source, mock.returned);
                    match mode {
                        2 => v.address += 8,
                        3 => v.world_address += 8,
                        4 => v.epoch += 1,
                        5 => v.native_identifier ^= 1u64 << 40,
                        6 => v.native_identifier ^= 1,
                        _ => unreachable!(),
                    }
                    Ok(Some(v))
                })
            };
            assert_eq!(mock.calls.get(), 1);
            assert_eq!(receipt.returned_address, 0x620000);
            assert!(receipt.postflight.is_err());
            assert_eq!(receipt.source, source);
        }
    }
    #[test]
    fn lookup_preflight_rejects_missing_foreign_unaligned_world_and_epoch_before_dispatch() {
        let mock = Mock {
            calls: Cell::new(0),
            handle: Cell::new(0),
            returned: 0,
        };
        let value = source(&mock, 1);
        assert!(preflight(None, value, 7).is_err());
        assert!(preflight(Some(value.world), value, 8).is_err());
        for mode in 0..4 {
            let mut changed = value;
            match mode {
                0 => changed.epoch = 0,
                1 => changed.world.address = 0,
                2 => changed.world.address += 1,
                3 => changed.world.kind = ObjectKind::Player,
                _ => unreachable!(),
            }
            assert!(preflight(Some(value.world), changed, 7).is_err());
        }
        assert_eq!(mock.calls.get(), 0);
    }
}
