//! Explicit native event requests. Caller supplies the whole exclusive engine graph and phase.
use crate::{
    WW_171, Ww171File,
    character_animation::{
        ANIMATION_EVIDENCE, AnimationOperation, CharacterAnimationObservation, NativeAnimationCall,
        check_postflight, check_preflight,
    },
    character_sets::CharacterIdentity,
};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

pub struct CurrentCharacterAnimation {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentCharacterAnimation {
    /// # Safety
    /// Caller pins the character/event module, native input/manipulator, all engine readers,
    /// writable request/current fields and complete transitive call graph, provides an exclusive
    /// permitted phase and a nonzero lifetime epoch, and excludes callbacks/aliases/teardown,
    /// reuse, unowned code changes and unload. File/type/code checks acquire none of these.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero native animation epoch"));
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
    pub fn observe(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterAnimationObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .character_animation(target)
    }
    /// # Safety
    /// Bind's full exclusive writable graph/phase contract holds through native dispatch and
    /// postflight. Request(-1) is an explicit clear, not a no-operation sentinel. Calling with an
    /// unchanged ID can still mutate native priority/input. ClearMatchedRequest is a separate
    /// native attempt; use its successful updated snapshot before requesting again. No combined
    /// transaction, automatic retry/restore, animation start/completion or resource-ready claim.
    pub unsafe fn execute(
        &self,
        expected: CharacterAnimationObservation,
        operation: AnimationOperation,
    ) -> Result<NativeAnimationCall> {
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        check_preflight(
            objects.character_animation(expected.module.character)?,
            expected,
            self.epoch,
        )?;
        let index = match operation {
            AnimationOperation::Request(_) => 2,
            AnimationOperation::ClearMatchedRequest => 3,
        };
        let entry = objects.module.code_address(ANIMATION_EVIDENCE[index])?;
        let native = match operation {
            AnimationOperation::Request(_) => {
                // SAFETY: Complete RCX event/RDX pointer-to-signed-ID/void ABI, full caller graph.
                Entry::Request(unsafe { std::mem::transmute::<usize, Request>(entry as usize) })
            }
            AnimationOperation::ClearMatchedRequest => {
                // SAFETY: Complete single-RCX event/void ABI, same full caller graph.
                Entry::Clear(unsafe { std::mem::transmute::<usize, Clear>(entry as usize) })
            }
        };
        // SAFETY: Caller guarantees phase, graph and native residency; helper owns request word.
        Ok(unsafe {
            invoke(native, expected, operation, || {
                objects
                    .character_animation(expected.module.character)?
                    .ok_or(Error::InvalidProfile(
                        "event module unavailable after native animation",
                    ))
            })
        })
    }
}
type Request = unsafe extern "system" fn(u64, *const i32);
type Clear = unsafe extern "system" fn(u64);
enum Entry {
    Request(Request),
    Clear(Clear),
}
unsafe fn invoke(
    entry: Entry,
    source: CharacterAnimationObservation,
    operation: AnimationOperation,
    observe: impl FnOnce() -> Result<CharacterAnimationObservation>,
) -> NativeAnimationCall {
    match (entry, operation) {
        (Entry::Request(function), AnimationOperation::Request(id)) => {
            // SAFETY: Caller supplies exact pinned event graph; id is live disjoint owned word.
            unsafe { function(source.module.address, &id) };
        }
        (Entry::Clear(function), AnimationOperation::ClearMatchedRequest) => {
            // SAFETY: Caller supplies exact complete single-RCX reset ABI and pinned graph.
            unsafe { function(source.module.address) };
        }
        _ => unreachable!("entry chosen from the operation before dispatch"),
    }
    NativeAnimationCall {
        source,
        operation,
        postflight: observe().and_then(|after| check_postflight(source, operation, after)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character_modules::{CharacterModule, CharacterModuleObservation};
    use std::cell::Cell;
    #[repr(C, align(8))]
    struct Mock {
        calls: Cell<u32>,
        latched: Cell<i32>,
        requested: Cell<i32>,
        mutate_latch: bool,
    }
    unsafe extern "system" fn request(address: u64, id: *const i32) {
        // SAFETY: Only original owned Mock and helper-owned initialized request word in tests.
        let (mock, id) = unsafe { (&*(address as *const Mock), *id) };
        mock.calls.set(mock.calls.get() + 1);
        mock.requested.set(id);
        if id != -1 && mock.mutate_latch {
            mock.latched.set(id);
        }
    }
    unsafe extern "system" fn clear(address: u64) {
        // SAFETY: Original owned Mock only; no game call or memory.
        let mock = unsafe { &*(address as *const Mock) };
        mock.calls.set(mock.calls.get() + 1);
        if mock.latched.get() == mock.requested.get() {
            mock.latched.set(-1);
        }
    }
    fn observation(mock: &Mock) -> CharacterAnimationObservation {
        CharacterAnimationObservation {
            module: CharacterModuleObservation {
                character: CharacterIdentity {
                    address: 0x1000,
                    world_address: 0x2000,
                    native_identifier: 0x123456789abcdef0,
                    epoch: 3,
                },
                module: CharacterModule::Event,
                container_address: 0x3000,
                address: mock as *const Mock as u64,
            },
            latched_id: mock.latched.get(),
            requested_id: mock.requested.get(),
        }
    }
    #[test]
    fn original_native_abi_preserves_signed_ids_and_explicit_reset_then_request() {
        for mutate_latch in [false, true] {
            let mock = Mock {
                calls: Cell::new(0),
                latched: Cell::new(20012),
                requested: Cell::new(20012),
                mutate_latch,
            };
            let source = observation(&mock);
            // SAFETY: Self-authored original function/Mock; exact reset and observation contracts.
            let call = unsafe {
                invoke(
                    Entry::Clear(clear),
                    source,
                    AnimationOperation::ClearMatchedRequest,
                    || Ok(observation(&mock)),
                )
            };
            let next = call.postflight.unwrap();
            assert_eq!((next.latched_id, next.requested_id), (-1, 20012));
            for id in [20012, -1, i32::MIN, i32::MAX, 0] {
                let source = observation(&mock);
                // SAFETY: Original ABI function with helper-owned signed request; no native game.
                let call = unsafe {
                    invoke(
                        Entry::Request(request),
                        source,
                        AnimationOperation::Request(id),
                        || Ok(observation(&mock)),
                    )
                };
                assert_eq!(call.operation, AnimationOperation::Request(id));
                assert_eq!(call.postflight.unwrap().requested_id, id);
            }
            assert_eq!(mock.calls.get(), 6);
        }
    }
    #[test]
    fn native_attempt_survives_failed_postflight_and_foreign_source_is_rejected() {
        let mock = Mock {
            calls: Cell::new(0),
            latched: Cell::new(7),
            requested: Cell::new(9),
            mutate_latch: false,
        };
        let source = observation(&mock);
        check_preflight(Some(source), source, 3).unwrap();
        assert!(check_preflight(None, source, 3).is_err());
        for mode in 0..9 {
            let mut foreign = source;
            match mode {
                0 => foreign.module.character.address += 8,
                1 => foreign.module.character.world_address += 8,
                2 => foreign.module.character.native_identifier += 1,
                3 => foreign.module.character.epoch += 1,
                4 => foreign.module.container_address += 8,
                5 => foreign.module.address += 8,
                6 => foreign.latched_id += 1,
                7 => foreign.requested_id += 1,
                _ => foreign.module.module = CharacterModule::Physics,
            }
            assert!(check_preflight(Some(foreign), source, 3).is_err());
            assert!(check_postflight(source, AnimationOperation::Request(9), foreign).is_err());
        }
        for epoch in [0, 4] {
            assert!(check_preflight(Some(source), source, epoch).is_err());
        }
        let mut overflow = source;
        overflow.module.address = u64::MAX - 7;
        assert!(check_preflight(Some(overflow), overflow, 3).is_err());
        // SAFETY: Original mock; failure after this call must preserve its attempt and source.
        let call = unsafe {
            invoke(
                Entry::Request(request),
                source,
                AnimationOperation::Request(23),
                || Err(Error::InvalidProfile("copy unavailable")),
            )
        };
        assert_eq!(mock.calls.get(), 1);
        assert_eq!(mock.requested.get(), 23);
        assert_eq!(call.source, source);
        assert!(call.postflight.is_err());
        let after = observation(&mock);
        // SAFETY: Original mock no-match reset; explicit attempt still occurs exactly once.
        let call = unsafe {
            invoke(
                Entry::Clear(clear),
                after,
                AnimationOperation::ClearMatchedRequest,
                || Ok(observation(&mock)),
            )
        };
        assert_eq!(*call.postflight.as_ref().unwrap(), after);
        assert_eq!(mock.calls.get(), 2);
        {
            let _retained = Box::new(call);
        }
        assert_eq!(mock.calls.get(), 2); // No native operation when a receipt leaves scope.
    }
}
