//! Native loading request words and their latched diagnostic states; no readiness or ownership lease.
use crate::{character_sets::CharacterIdentity, objects::GameObjectCatalog};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
use std::{marker::PhantomData, rc::Rc, sync::Arc};
pub(crate) const LOAD_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3e6e30,
        length: 0x99f,
    },
    // Complete contiguous dispatch prefix ending at its computed jump; switch paths excluded.
    CodeRange {
        rva: 0x3f8770,
        length: 0x2df,
    },
    CodeRange {
        rva: 0x3fd510,
        length: 0x2dd,
    },
];
const REQUEST_OFFSET: u32 = 0x1b0;
const LATCHED_OFFSET: u32 = 0x1b4;
const PRIOR_OFFSET: u32 = 0x1b8;
const WRITABLE_MASK: u32 = 0x81;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterLoadRequestObservation {
    pub character: CharacterIdentity,
    pub request_bits: u32,
    pub latched_bits: u32,
    pub prior_bits: u32,
}
impl CharacterLoadRequestObservation {
    /// Labels trace the engine's diagnostic reasons for cleared latched bits. Applying them
    /// to the request word reports caller selection intent, never completed loading or visibility.
    pub fn draw_group_requested(self) -> bool {
        self.request_bits & 1 != 0
    }
    pub fn backread_disabled_requested(self) -> bool {
        self.request_bits & 0x10 == 0
    }
    pub fn extinction_death_requested(self) -> bool {
        self.request_bits & 0x80 == 0
    }
    pub fn near_player_requested(self) -> bool {
        self.request_bits & 0x100 != 0
    }
    pub fn host_inactive_requested(self) -> bool {
        self.request_bits & 0x400 == 0
    }
}
fn at(base: u64, offset: u32) -> Result<u64> {
    base.checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    pub fn character_load_requests(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterLoadRequestObservation>> {
        if target.epoch == 0 {
            return Err(Error::InvalidProfile("zero character loading epoch"));
        }
        if self.character_identity(target.address, target.epoch)? != Some(target) {
            return Ok(None);
        }
        let read = || -> Result<CharacterLoadRequestObservation> {
            Ok(CharacterLoadRequestObservation {
                character: target,
                request_bits: self
                    .module
                    .memory()
                    .read_u32(at(target.address, REQUEST_OFFSET)?)?,
                latched_bits: self
                    .module
                    .memory()
                    .read_u32(at(target.address, LATCHED_OFFSET)?)?,
                prior_bits: self
                    .module
                    .memory()
                    .read_u32(at(target.address, PRIOR_OFFSET)?)?,
            })
        };
        let value = read()?;
        if read()? != value
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile("character loading requests changed"));
        }
        Ok(Some(value))
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterLoadRequestWrite {
    pub source: CharacterLoadRequestObservation,
    pub mask: u32,
    pub replacement_bits: u32,
    pub destination: u64,
    pub stored: bool,
    pub postflight: Result<CharacterLoadRequestObservation>,
}
fn commit_request(
    source: CharacterLoadRequestObservation,
    mask: u32,
    bits: u32,
    mut observe: impl FnMut() -> Result<Option<CharacterLoadRequestObservation>>,
    store: impl FnOnce(u64, u32),
) -> Result<CharacterLoadRequestWrite> {
    if mask == 0
        || mask & !WRITABLE_MASK != 0
        || bits & !mask != 0
        || source.character.epoch == 0
        || source.character.address == 0
        || !source.character.address.is_multiple_of(8)
    {
        return Err(Error::InvalidProfile(
            "invalid selected character loading request",
        ));
    }
    let destination = at(source.character.address, REQUEST_OFFSET)?;
    destination.checked_add(3).ok_or(Error::AddressOverflow)?;
    for _ in 0..2 {
        if observe()? != Some(source) {
            return Err(Error::NativeOperationRejected(
                "character loading source changed",
            ));
        }
    }
    let after = CharacterLoadRequestObservation {
        request_bits: (source.request_bits & !mask) | bits,
        ..source
    };
    let stored = after.request_bits != source.request_bits;
    if stored {
        store(destination, after.request_bits);
    }
    let postflight = observe().and_then(|actual| {
        if actual == Some(after) {
            Ok(after)
        } else {
            Err(Error::NativeOperationRejected(
                "character loading postflight changed",
            ))
        }
    });
    Ok(CharacterLoadRequestWrite {
        source,
        mask,
        replacement_bits: after.request_bits,
        destination,
        stored,
        postflight,
    })
}
pub struct CurrentCharacterLoadRequests {
    file: shikifsrs_runtime::profile::OwnedVerifiedFile,
    patches: Option<Arc<shikifsrs_runtime::code_patches::OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentCharacterLoadRequests {
    /// # Safety
    /// Retain initialized full character/world graph in its exact exclusive permitted phase,
    /// including epochs, no allocation reuse/ABA, aliases, competing load transitions or unload.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<shikifsrs_runtime::code_patches::OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero character loading epoch"));
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
    ) -> Result<Option<CharacterLoadRequestObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        crate::Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(
                &shikifsrs_runtime::memory::CurrentProcess,
                self.base,
                self.patches.as_deref(),
            )?
            .character_load_requests(target)
    }
    /// # Safety
    /// Bind's graph/phase obligations hold plus exclusive initialized writable four-byte request
    /// extent. Only bits0/7 are selected; preserve all others. Latched/prior words remain read-only.
    /// Keep actual receipt/pending intent on interruption or failed postflight; no retry/Drop writes.
    /// This store reports storage only, never native processing, visibility, readiness or ownership.
    pub unsafe fn compare_mask(
        &self,
        source: CharacterLoadRequestObservation,
        mask: u32,
        bits: u32,
    ) -> Result<CharacterLoadRequestWrite> {
        if source.character.epoch != self.epoch {
            return Err(Error::NativeOperationRejected(
                "foreign character loading epoch",
            ));
        }
        commit_request(
            source,
            mask,
            bits,
            || self.observe(source.character),
            |address, bits| {
                // SAFETY: Caller owns the original initialized writable extent under exclusive phase.
                unsafe { (address as *mut u32).write_volatile(bits) };
            },
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn source(bits: u32) -> CharacterLoadRequestObservation {
        CharacterLoadRequestObservation {
            character: CharacterIdentity {
                address: 0x1000,
                world_address: 0x2000,
                native_identifier: 0xfedcba9876543210,
                epoch: 1,
            },
            request_bits: bits,
            latched_bits: 0x12345678,
            prior_bits: 0xabcdef01,
        }
    }
    #[test]
    fn selected_words_preserve_all_unselected_bits_and_latched_history_and_actual_postflight_failure()
     {
        for raw in [0, u32::MAX, 0xa5a5a5a5, 0x5a5a5a5a] {
            for mask in [1, 0x80, 0x81] {
                for bits in [0, mask] {
                    let before = source(raw);
                    let after = CharacterLoadRequestObservation {
                        request_bits: (raw & !mask) | bits,
                        ..before
                    };
                    let mut reads = 0;
                    let mut stores = 0;
                    let receipt = commit_request(
                        before,
                        mask,
                        bits,
                        || {
                            reads += 1;
                            if reads <= 2 {
                                Ok(Some(before))
                            } else {
                                Err(Error::InvalidProfile("synthetic postflight failure"))
                            }
                        },
                        |address, actual| {
                            stores += 1;
                            assert_eq!(address, 0x11b0);
                            assert_eq!(actual, after.request_bits);
                        },
                    )
                    .unwrap();
                    assert_eq!(stores, usize::from(raw != after.request_bits));
                    assert_eq!(receipt.stored, raw != after.request_bits);
                    assert_eq!(receipt.source, before);
                    assert_eq!(receipt.replacement_bits, after.request_bits);
                    assert!(receipt.postflight.is_err());
                    assert_eq!(reads, 3);
                }
            }
        }
    }
    #[test]
    fn invalid_masks_stale_full_id_and_changed_requests_never_enter_store() {
        let before = source(0);
        for (mask, bits) in [(0, 0), (2, 2), (0x100, 0), (1, 0x80)] {
            assert!(
                commit_request(
                    before,
                    mask,
                    bits,
                    || Ok(Some(before)),
                    |_, _| panic!("must not store")
                )
                .is_err()
            );
        }
        for field in 0..7 {
            let mut changed = before;
            match field {
                0 => changed.character.address += 8,
                1 => changed.character.world_address += 8,
                2 => changed.character.native_identifier ^= 1 << 48,
                3 => changed.character.epoch += 1,
                4 => changed.request_bits ^= 1,
                5 => changed.latched_bits ^= 1,
                _ => changed.prior_bits ^= 1,
            }
            let mut reads = 0;
            assert!(
                commit_request(
                    before,
                    1,
                    1,
                    || {
                        reads += 1;
                        Ok(Some(if reads == 1 { before } else { changed }))
                    },
                    |_, _| panic!("must not store")
                )
                .is_err()
            );
        }
        for bit in 0..11 {
            let v = source(1 << bit);
            assert_eq!(v.draw_group_requested(), bit == 0);
            assert_eq!(v.backread_disabled_requested(), bit != 4);
            assert_eq!(v.extinction_death_requested(), bit != 7);
            assert_eq!(v.near_player_requested(), bit == 8);
            assert_eq!(v.host_inactive_requested(), bit != 10);
        }
    }
}
