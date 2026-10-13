//! Native transition/policy words. Raw request bits are separate from activation completion.
use crate::{character_sets::CharacterIdentity, objects::GameObjectCatalog};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
use std::{marker::PhantomData, rc::Rc, sync::Arc};
pub(crate) const STATE_WORD_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3e6e30,
        length: 0x99f,
    },
    CodeRange {
        rva: 0x3f26a0,
        length: 0x439,
    },
    CodeRange {
        rva: 0x3f3800,
        length: 0x65,
    },
    CodeRange {
        rva: 0x3f73b0,
        length: 0x5c,
    },
];
const TRANSITION_STEP_OFFSET: u32 = 0x1c0;
const POLICY_WORD_OFFSET: u32 = 0x1cc;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterStateWordsObservation {
    pub character: CharacterIdentity,
    /// Raw native opacity/presentation transition value; constructor sets1, native paths use1..6.
    pub transition_step: i32,
    /// Constructor preserves unspecified bits, clears0/1/2/4, sets3. Native3f3800 can return bit0.
    pub policy_bits: u32,
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    pub fn character_state_words(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterStateWordsObservation>> {
        if self.character_identity(target.address, target.epoch)? != Some(target) {
            return Ok(None);
        }
        let read = || -> Result<CharacterStateWordsObservation> {
            Ok(CharacterStateWordsObservation {
                character: target,
                transition_step: self.module.memory().read_u32(
                    target
                        .address
                        .checked_add(u64::from(TRANSITION_STEP_OFFSET))
                        .ok_or(Error::AddressOverflow)?,
                )? as i32,
                policy_bits: self.module.memory().read_u32(
                    target
                        .address
                        .checked_add(u64::from(POLICY_WORD_OFFSET))
                        .ok_or(Error::AddressOverflow)?,
                )?,
            })
        };
        let copied = read()?;
        if read()? != copied
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile("character state words changed"));
        }
        Ok(Some(copied))
    }
}
#[derive(Debug)]
pub struct CharacterStateWordWrite {
    pub source: CharacterStateWordsObservation,
    pub mask: u32,
    pub after_bits: u32,
    pub changed: bool,
    pub postflight: Result<CharacterStateWordsObservation>,
}
fn commit_policy(
    expected: CharacterStateWordsObservation,
    mask: u32,
    bits: u32,
    mut observe: impl FnMut() -> Result<Option<CharacterStateWordsObservation>>,
    store: impl FnOnce(usize, u32),
) -> Result<CharacterStateWordWrite> {
    if mask == 0
        || mask & !0x19 != 0
        || bits & !mask != 0
        || expected.character.epoch == 0
        || expected.character.address == 0
        || !expected.character.address.is_multiple_of(8)
    {
        return Err(Error::InvalidProfile(
            "invalid selected character policy bits",
        ));
    }
    let address = expected
        .character
        .address
        .checked_add(u64::from(POLICY_WORD_OFFSET))
        .ok_or(Error::AddressOverflow)?;
    address.checked_add(3).ok_or(Error::AddressOverflow)?;
    let address = usize::try_from(address).map_err(|_| Error::AddressOverflow)?;
    for _ in 0..2 {
        if observe()? != Some(expected) {
            return Err(Error::NativeOperationRejected(
                "character policy source changed",
            ));
        }
    }
    let after = CharacterStateWordsObservation {
        policy_bits: (expected.policy_bits & !mask) | bits,
        ..expected
    };
    let changed = after.policy_bits != expected.policy_bits;
    let postflight = if changed {
        store(address, after.policy_bits);
        observe().and_then(|current| {
            if current == Some(after) {
                Ok(after)
            } else {
                Err(Error::NativeOperationRejected(
                    "character policy postflight changed",
                ))
            }
        })
    } else {
        Ok(after)
    };
    Ok(CharacterStateWordWrite {
        source: expected,
        mask,
        after_bits: after.policy_bits,
        changed,
        postflight,
    })
}
pub struct CurrentCharacterStateWords {
    file: shikifsrs_runtime::profile::OwnedVerifiedFile,
    patches: Option<Arc<shikifsrs_runtime::code_patches::OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentCharacterStateWords {
    /// # Safety
    /// Caller supplies initialized stable full character/world/module graph, lifetime epoch,
    /// exclusive permitted phase, no competing transitions, teardown, unload or ABA.
    pub unsafe fn bind(bytes: Arc<[u8]>, base: u64, epoch: u64) -> Result<Self> {
        // SAFETY: Original binding obligations; no replacement manifest is supplied.
        unsafe { Self::bind_with_patches(bytes, base, epoch, None) }
    }
    /// Retain the host's exact owned replacements through every observation and store.
    /// # Safety
    /// bind's full graph/phase obligations and the manifest owner's ABI/lifetime/quiescence
    /// obligations apply. A replacement manifest establishes no engine ownership or lease.
    pub unsafe fn bind_with_patches(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<shikifsrs_runtime::code_patches::OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero character state epoch"));
        }
        let file = shikifsrs_runtime::profile::OwnedVerifiedFile::verify(bytes, crate::WW_171)?;
        crate::Ww171File::from_verified(file.borrow()?)?.bind_objects_with_patches(
            &shikifsrs_runtime::memory::CurrentProcess,
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
    ) -> Result<Option<CharacterStateWordsObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        crate::Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(
                &shikifsrs_runtime::memory::CurrentProcess,
                self.base,
                self.patches.as_deref(),
            )?
            .character_state_words(target)
    }
    /// Only native-observed policy bits0/3/4 may be selected. The transition step stays read-only.
    /// # Safety
    /// Bind's full graph/phase contract holds plus live writable initialized u32 extent.
    /// Caller provides selected-bit numerical/business meaning and engine exclusion; the store
    /// does not invoke native callbacks, prove activation or grant quiescence. Retain receipt
    /// and reconcile a failed postflight before further edits. No retry or Drop writes.
    pub unsafe fn compare_policy_mask(
        &self,
        expected: CharacterStateWordsObservation,
        mask: u32,
        bits: u32,
    ) -> Result<CharacterStateWordWrite> {
        if expected.character.epoch != self.epoch {
            return Err(Error::NativeOperationRejected(
                "foreign character state epoch",
            ));
        }
        commit_policy(
            expected,
            mask,
            bits,
            || self.observe(expected.character),
            |address, bits| {
                // SAFETY: Caller supplies exclusive initialized writable selected u32 extent.
                unsafe { (address as *mut u32).write_volatile(bits) };
            },
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_preserves_transition_neighbors_and_failed_store_is_retained() {
        #[repr(align(8))]
        struct Storage([u32; 128]);
        let mut storage = Storage([0xa5a5a5a5; 128]);
        let target = CharacterIdentity {
            address: storage.0.as_mut_ptr() as u64,
            world_address: 8,
            native_identifier: 16,
            epoch: 1,
        };
        let before = CharacterStateWordsObservation {
            character: target,
            transition_step: 6,
            policy_bits: storage.0[0x1cc / 4],
        };
        let original = storage.0;
        let mut reads = 0;
        let receipt = commit_policy(
            before,
            1,
            0,
            || {
                reads += 1;
                if reads <= 2 {
                    Ok(Some(before))
                } else {
                    Err(Error::InvalidProfile("postflight fixture"))
                }
            },
            |address, bits| {
                // SAFETY: Exact aligned owned synthetic storage, no game call.
                unsafe { (address as *mut u32).write(bits) };
            },
        )
        .unwrap();
        let mut expected = original;
        expected[0x1cc / 4] &= !1;
        assert_eq!(storage.0, expected);
        assert!(receipt.changed && receipt.postflight.is_err());
        assert_eq!(receipt.source.transition_step, 6);
        assert_eq!(receipt.after_bits, before.policy_bits & !1);
    }
    #[test]
    fn policy_rejects_unproved_bits_and_second_preflight_identity_change_without_store() {
        let before = CharacterStateWordsObservation {
            character: CharacterIdentity {
                address: 8,
                world_address: 16,
                native_identifier: 32,
                epoch: 1,
            },
            transition_step: 1,
            policy_bits: 8,
        };
        for (mask, bits) in [(0, 0), (2, 2), (0x20, 0), (1, 2)] {
            assert!(
                commit_policy(
                    before,
                    mask,
                    bits,
                    || Ok(Some(before)),
                    |_, _| panic!("no actual store")
                )
                .is_err()
            );
        }
        for field in 0..6 {
            let mut foreign = before;
            match field {
                0 => foreign.character.epoch += 1,
                1 => foreign.character.native_identifier += 1,
                2 => foreign.character.world_address += 8,
                3 => foreign.character.address += 8,
                4 => foreign.transition_step += 1,
                _ => foreign.policy_bits ^= 1,
            }
            let mut read = 0;
            assert!(
                commit_policy(
                    before,
                    1,
                    1,
                    || {
                        read += 1;
                        Ok(Some(if read == 1 { before } else { foreign }))
                    },
                    |_, _| panic!("no actual store")
                )
                .is_err()
            );
        }
    }
}
