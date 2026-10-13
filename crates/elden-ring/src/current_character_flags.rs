//! Thread-confined conditional raw status/team byte stores; no native calls or Drop restoration.
use crate::{
    WW_171, Ww171File,
    character_flags::{
        CharacterFlagByte, CharacterFlagWrite, CharacterFlagsObservation,
        CharacterUpdateGroupObservation, FLAG_EVIDENCE, commit_flag, flag_plan, flag_restore_plan,
        store_flag, update_group_postflight, update_group_preflight,
    },
    character_sets::CharacterIdentity,
};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};
pub struct CurrentCharacterFlags {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
#[derive(Debug)]
pub struct NativeUpdateGroupCall {
    pub source: CharacterUpdateGroupObservation,
    pub enabled: bool,
    pub group_id: u32,
    /// A native attempt occurred, including switch-zero no-op. Failed postflight retains it.
    pub postflight: Result<CharacterUpdateGroupObservation>,
}
type SetGroup = unsafe extern "system" fn(u64, u8, u32);
unsafe fn invoke_group(
    entry: SetGroup,
    source: CharacterUpdateGroupObservation,
    enabled: bool,
    group_id: u32,
    observe: impl FnOnce() -> Result<CharacterUpdateGroupObservation>,
) -> NativeUpdateGroupCall {
    // SAFETY: Caller supplies the independently proved RCX/DL/R8D ABI and full live exclusive graph.
    unsafe { entry(source.flags.character.address, u8::from(enabled), group_id) };
    NativeUpdateGroupCall {
        source,
        enabled,
        group_id,
        postflight: observe()
            .and_then(|after| update_group_postflight(source, enabled, group_id, after)),
    }
}
impl CurrentCharacterFlags {
    /// # Safety
    /// compare_mask's full graph, exclusive native phase and writable byte contracts hold.
    /// A request store neither dispatches death callbacks nor revives an actor; retain receipts.
    pub unsafe fn set_native_death_marked(
        &self,
        expected: CharacterFlagsObservation,
        marked: bool,
    ) -> Result<CharacterFlagWrite> {
        // SAFETY: Caller supplies the complete conditional single-byte store contract.
        unsafe {
            self.compare_mask(
                expected,
                CharacterFlagByte::Status1,
                0x80,
                u8::from(marked) << 7,
            )
        }
    }
    /// Suppress the independently observed character hit dispatch paths.
    /// # Safety
    /// bind/compare_mask's full graph, lifetime and exclusive native phase contract holds.
    /// This bit is not a physics collision, callback drain or retirement barrier.
    pub unsafe fn set_hit_dispatch_suppressed(
        &self,
        expected: CharacterFlagsObservation,
        suppressed: bool,
    ) -> Result<CharacterFlagWrite> {
        // SAFETY: Same exclusive pinned character byte contract as compare_mask.
        unsafe {
            self.compare_mask(
                expected,
                CharacterFlagByte::DebugLow,
                8,
                u8::from(suppressed) << 3,
            )
        }
    }
    /// # Safety
    /// Caller pins used character/entire native graph, writable raw status/team storage, correct nonzero
    /// lifetime epoch and exclusive permitted phase; excludes callbacks/aliasing/teardown/ABA,
    /// hooks and unload. Copies/code checks acquire none of those guarantees. A status-bit store
    /// does not unregister tasks, unload/free a character or establish visibility/activation.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero character flag epoch"));
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
    pub fn observe(&self, target: CharacterIdentity) -> Result<Option<CharacterFlagsObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .character_flags(target)
    }
    /// Set only the independently named per-character disable-update request. The engine's
    /// global all-character setting is not changed. Explicit restore uses the returned mask receipt.
    /// # Safety
    /// Bind/compare_mask's complete lifetime and exclusive mutation-phase obligations hold.
    /// Request storage is not a completion/barrier, hidden-reuse or safe-retirement guarantee.
    pub unsafe fn set_debug_update_disabled(
        &self,
        expected: CharacterFlagsObservation,
        disabled: bool,
    ) -> Result<CharacterFlagWrite> {
        // SAFETY: Same exclusive pinned per-character byte contract as compare_mask.
        unsafe {
            self.compare_mask(
                expected,
                CharacterFlagByte::DebugHigh,
                1,
                u8::from(disabled),
            )
        }
    }
    /// Change the native diagnostic's forced-update request, preserving every other copied bit.
    /// # Safety
    /// Bind/compare_mask's full pinned graph and exclusive legal mutation-phase contract applies.
    /// This request does not establish engine activation, resources ready, or completed work.
    pub unsafe fn set_forced_update_requested(
        &self,
        expected: CharacterFlagsObservation,
        requested: bool,
    ) -> Result<CharacterFlagWrite> {
        // SAFETY: Same single-byte lifetime and exclusive phase obligations as compare_mask.
        unsafe {
            self.compare_mask(
                expected,
                CharacterFlagByte::Status0,
                2,
                u8::from(requested) << 1,
            )
        }
    }
    /// Change the model-enable request consumed on the native character presentation update.
    /// Other backread/controller/group conditions remain effective; enabling does not revive
    /// or activate the character. Explicit restore uses the returned mask receipt.
    /// # Safety
    /// Bind/compare_mask's full pinned storage and exclusive legal phase obligations hold.
    /// Caller coordinates opacity, physics, updates and lifecycle separately. No native update
    /// is invoked, and a successful store does not prove a frame has consumed the request.
    pub unsafe fn set_model_enable_requested(
        &self,
        expected: CharacterFlagsObservation,
        requested: bool,
    ) -> Result<CharacterFlagWrite> {
        // SAFETY: Same single-byte lifetime and exclusive phase obligations as compare_mask.
        unsafe {
            self.compare_mask(
                expected,
                CharacterFlagByte::Status1,
                8,
                u8::from(requested) << 3,
            )
        }
    }
    pub fn observe_update_group(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterUpdateGroupObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .character_update_group(target)
    }
    /// Assign via the complete native entry, preserving its switch-zero no-op and group word.
    /// No automatic restore: raw flag restoration alone cannot restore the group assignment.
    /// # Safety
    /// Bind's full caller obligations hold, including initialized debug singleton, stable global
    /// switch, transitive assertion/code graph, writable group word and exclusive legal phase.
    /// This call does not prove visibility, readiness, task drain, engine ownership or safe free.
    pub unsafe fn assign_update_group(
        &self,
        expected: CharacterUpdateGroupObservation,
        enabled: bool,
        group_id: u32,
    ) -> Result<NativeUpdateGroupCall> {
        if expected.flags.character.epoch != self.epoch {
            return Err(Error::NativeOperationRejected("foreign update group epoch"));
        }
        let image = Ww171File::from_verified(self.file.borrow()?)?;
        let objects =
            image.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?;
        update_group_preflight(
            objects.character_update_group(expected.flags.character)?,
            expected,
        )?;
        let address = objects.module.code_address(FLAG_EVIDENCE[7])?;
        // SAFETY: Complete independently proved entry/ABI; caller holds all phase/lifetime/code obligations.
        let entry = unsafe { std::mem::transmute::<usize, SetGroup>(address as usize) };
        // SAFETY: Same exclusive live graph, debug gate and owned scalar arguments as this method's contract.
        Ok(unsafe {
            invoke_group(entry, expected, enabled, group_id, || {
                objects
                    .character_update_group(expected.flags.character)?
                    .ok_or(Error::InvalidProfile("update group unavailable after call"))
            })
        })
    }
    /// # Safety
    /// Bind's full exclusive phase/lifetime/writable graph obligations hold. Masked bits must
    /// be appropriate for this caller's use; no gameplay meaning/ownership is inferred.
    /// Postflight failure preserves the actual store; reconcile before further writes.
    pub unsafe fn compare_mask(
        &self,
        expected: CharacterFlagsObservation,
        field: CharacterFlagByte,
        mask: u8,
        bits: u8,
    ) -> Result<CharacterFlagWrite> {
        let plan = flag_plan(
            self.observe(expected.character)?,
            expected,
            field,
            mask,
            bits,
        )?;
        Ok(commit_flag(
            plan,
            |address, value| {
                // SAFETY: Caller supplies exclusive pinned writable byte and full native phase.
                unsafe { store_flag(address, value) };
            },
            || {
                self.observe(expected.character)?
                    .ok_or(Error::InvalidProfile(
                        "character unavailable after flag store",
                    ))
            },
        ))
    }
    /// Restore only bits still matching this receipt, preserving changed unrelated bits/fields.
    /// # Safety
    /// Same lifetime/phase/alias exclusion as compare_mask. Source character still belongs to
    /// this lifetime; postflight may have failed but the receipt accurately records its store.
    /// No automatic retries or Drop restoration occur. Failure leaves obligations with caller.
    pub unsafe fn restore_mask(&self, applied: &CharacterFlagWrite) -> Result<CharacterFlagWrite> {
        let plan = flag_restore_plan(self.observe(applied.source.character)?, applied)?;
        Ok(commit_flag(
            plan,
            |address, value| {
                // SAFETY: Caller supplies exact exclusive live byte extent in this engine phase.
                unsafe { store_flag(address, value) };
            },
            || {
                self.observe(applied.source.character)?
                    .ok_or(Error::InvalidProfile(
                        "character unavailable after flag restore",
                    ))
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::{ObjectKind, ObjectObservation};
    use std::cell::{Cell, UnsafeCell};
    #[repr(C, align(8))]
    struct Mock {
        bytes: UnsafeCell<[u8; 0x580]>,
        calls: Cell<u32>,
        gate: Cell<u8>,
    }
    fn mock(gate: u8) -> Mock {
        let mut bytes = [0x5a; 0x580];
        bytes[0x1c4] = 0xa4;
        bytes[0xb4..0xb8].copy_from_slice(&9u32.to_le_bytes());
        Mock {
            bytes: UnsafeCell::new(bytes),
            calls: Cell::new(0),
            gate: Cell::new(gate),
        }
    }
    fn observation(m: &Mock) -> CharacterUpdateGroupObservation {
        // SAFETY: Exclusive synchronous original mock; no alias survives a native-mock dispatch.
        let bytes = unsafe { &*m.bytes.get() };
        CharacterUpdateGroupObservation {
            flags: CharacterFlagsObservation {
                character: CharacterIdentity {
                    address: m as *const Mock as u64,
                    world_address: 0x2000,
                    native_identifier: 3,
                    epoch: 4,
                },
                bytes: [0x1c4, 0x1c5, 0x1c8, 0x538, 0x6c, 0x539].map(|o| bytes[o]),
            },
            group_id: u32::from_le_bytes(bytes[0xb4..0xb8].try_into().unwrap()),
            debug_world: ObjectObservation {
                address: 0x1000,
                kind: ObjectKind::DebugWorld,
            },
            debug_switch: m.gate.get(),
        }
    }
    unsafe extern "system" fn set_group(address: u64, enabled: u8, group: u32) {
        // SAFETY: Only the exclusive initialized original Mock is passed to this test ABI.
        let m = unsafe { &*(address as *const Mock) };
        assert!(enabled <= 1);
        m.calls.set(m.calls.get() + 1);
        if m.gate.get() != 0 {
            // SAFETY: Same original mock, no borrowed storage references across dispatch.
            let bytes = unsafe { &mut *m.bytes.get() };
            bytes[0x1c4] = (bytes[0x1c4] & !1) | enabled;
            bytes[0xb4..0xb8].copy_from_slice(&group.to_le_bytes());
        }
    }
    #[test]
    fn native_group_abi_honors_debug_gate_and_touches_only_group_and_low_flag() {
        for gate in [0, 1, 255] {
            for enabled in [false, true] {
                for group in [0, 5, u32::MAX] {
                    let m = mock(gate);
                    let source = observation(&m);
                    // SAFETY: Original mock copied before call; synchronous exclusive test storage.
                    let before = unsafe { *m.bytes.get() };
                    update_group_preflight(Some(source), source).unwrap();
                    // SAFETY: Original mock entry and graph, no engine calls.
                    let receipt = unsafe {
                        invoke_group(set_group, source, enabled, group, || Ok(observation(&m)))
                    };
                    assert!(receipt.postflight.is_ok());
                    assert_eq!(m.calls.get(), 1);
                    let mut expected = before;
                    if gate != 0 {
                        expected[0x1c4] = (expected[0x1c4] & !1) | u8::from(enabled);
                        expected[0xb4..0xb8].copy_from_slice(&group.to_le_bytes());
                    }
                    // SAFETY: Mock dispatch ended, exclusive copied read.
                    assert_eq!(unsafe { *m.bytes.get() }, expected);
                }
            }
        }
    }
    #[test]
    fn group_attempt_is_retained_on_postflight_failure_and_does_not_repeat_on_drop() {
        let m = mock(1);
        let source = observation(&m);
        {
            // SAFETY: Original synchronous mock only.
            let receipt = unsafe {
                invoke_group(set_group, source, true, 5, || {
                    Err(Error::InvalidProfile("lost graph"))
                })
            };
            assert!(receipt.postflight.is_err());
            assert_eq!(receipt.source, source);
            assert_eq!(receipt.group_id, 5);
        }
        assert_eq!(m.calls.get(), 1);
        assert_eq!(observation(&m).group_id, 5);
        assert!(update_group_preflight(Some(observation(&m)), source).is_err());
        assert!(update_group_preflight(None, source).is_err());
        for mode in 0..7 {
            let mut after = observation(&m);
            match mode {
                0 => after.flags.character.epoch += 1,
                1 => after.flags.bytes[1] ^= 1,
                2 => after.group_id += 1,
                3 => after.debug_switch = 0,
                4 => after.debug_world.address += 8,
                5 => after.debug_world.kind = ObjectKind::Player,
                _ => after.flags.bytes[0] ^= 2,
            }
            assert!(update_group_postflight(source, true, 5, after).is_err());
        }
    }
}
