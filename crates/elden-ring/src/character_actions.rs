//! Copied action-module words, derived from native initialization and frame update.
//! Raw masks and identifiers are diagnostic state, not an action dispatch/completion API.
use crate::{
    character_modules::{CharacterModule, CharacterModuleObservation},
    character_sets::CharacterIdentity,
    objects::GameObjectCatalog,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const ACTION_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x407270,
        length: 0x45,
    },
    CodeRange {
        rva: 0x407360,
        length: 0x7f,
    },
    CodeRange {
        rva: 0x407850,
        length: 0xce,
    },
    CodeRange {
        rva: 0x43d380,
        length: 0x4d,
    },
    CodeRange {
        rva: 0x408190,
        length: 0x285,
    },
    CodeRange {
        rva: 0x4079d0,
        length: 0x93,
    },
    CodeRange {
        rva: 0x407a70,
        length: 0x58,
    },
    CodeRange {
        rva: 0x3fe100,
        length: 0x611,
    },
    CodeRange {
        rva: 0x43cf70,
        length: 0x31,
    },
];

/// Consumer labels for signed words/flags remain experimental. The native update proves
/// the current/previous/edge-mask equations and the specific readback copies below.
/// Individual action-bit names, gameplay effects and TAE completion are not inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterActionWords {
    pub action_requests: u64,
    pub previous_action_requests: u64,
    pub new_action_presses: u64,
    pub new_action_releases: u64,
    pub buffered_actions: u64,
    pub pending_actions: u64,
    pub disabled_action_inputs: u64,
    pub possible_action_inputs: u64,
    pub possible_action_cancels: u64,
    pub readback_new_presses: u64,
    pub readback_buffered_actions: u64,
    pub readback_pending_actions: u64,
    pub readback_possible_inputs: u64,
    pub readback_possible_cancels: u64,
    /// Queue's signed selector at module+90; no animation playback inference.
    pub queue_selector: i32,
    /// Signed request word f4; native update copies nonnegative values into word130.
    pub requested_identifier: i32,
    pub readback_identifier: i32,
    /// Exact words fc/100; no meaning assigned to unnamed bits.
    pub movement_request_flags: u32,
    pub tae_cancel_flags: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterActionsObservation {
    pub module: CharacterModuleObservation,
    pub words: CharacterActionWords,
}

fn copy_words(memory: &impl AddressSpace, address: u64) -> Result<CharacterActionWords> {
    let at = |offset: u64| address.checked_add(offset).ok_or(Error::AddressOverflow);
    let qword = |offset| memory.read_u64(at(offset)?);
    let word = |offset| memory.read_u32(at(offset)?);
    Ok(CharacterActionWords {
        action_requests: qword(0x10)?,
        previous_action_requests: qword(0x18)?,
        new_action_presses: qword(0x20)?,
        new_action_releases: qword(0x28)?,
        buffered_actions: qword(0x30)?,
        pending_actions: qword(0x38)?,
        disabled_action_inputs: qword(0x40)?,
        possible_action_inputs: qword(0x98)?,
        possible_action_cancels: qword(0xa0)?,
        readback_new_presses: qword(0x108)?,
        readback_buffered_actions: qword(0x110)?,
        readback_pending_actions: qword(0x118)?,
        readback_possible_inputs: qword(0x120)?,
        readback_possible_cancels: qword(0x128)?,
        queue_selector: word(0x90)? as i32,
        requested_identifier: word(0xf4)? as i32,
        readback_identifier: word(0x130)? as i32,
        movement_request_flags: word(0xfc)?,
        tae_cancel_flags: word(0x100)?,
    })
}

fn stable_copy<M: AddressSpace>(
    memory: &M,
    source: CharacterModuleObservation,
    recheck: impl FnOnce() -> Result<Option<CharacterModuleObservation>>,
) -> Result<CharacterActionsObservation> {
    let words = copy_words(memory, source.address)?;
    if copy_words(memory, source.address)? != words || recheck()? != Some(source) {
        return Err(Error::InvalidProfile(
            "action words or module changed during copy",
        ));
    }
    Ok(CharacterActionsObservation {
        module: source,
        words,
    })
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy only selected scalar spans, then recheck all copied words and exact module owner.
    /// Queue vector allocations and unrequested fields are not read. No native call is made.
    pub fn character_actions(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterActionsObservation>> {
        let Some(module) = self.character_module(target, CharacterModule::ActionRequests)? else {
            return Ok(None);
        };
        stable_copy(self.module.memory(), module, || {
            self.character_module(target, CharacterModule::ActionRequests)
        })
        .map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shikifsrs_runtime::memory::SliceMemory;
    use std::cell::Cell;
    fn source() -> CharacterModuleObservation {
        CharacterModuleObservation {
            character: CharacterIdentity {
                epoch: 1,
                world_address: 0x2000,
                address: 0x3000,
                native_identifier: 7,
            },
            module: CharacterModule::ActionRequests,
            container_address: 0x4000,
            address: 0x1000,
        }
    }
    fn storage() -> [u8; 0x134] {
        let mut bytes = [0; 0x134];
        bytes[0x10..0x18].copy_from_slice(&(1u64 << 63 | 1).to_le_bytes());
        bytes[0x20..0x28].copy_from_slice(&1u64.to_le_bytes());
        bytes[0x108..0x110].copy_from_slice(&1u64.to_le_bytes());
        for offset in [0x90, 0xf4, 0x130] {
            bytes[offset..offset + 4].copy_from_slice(&(-1i32).to_le_bytes());
        }
        bytes
    }
    #[test]
    fn copied_diagnostic_state_preserves_high_bits_signed_sentinels_and_one_frame_edges() {
        let mut bytes = storage();
        let first = stable_copy(&SliceMemory::new(0x1000, &bytes).unwrap(), source(), || {
            Ok(Some(source()))
        })
        .unwrap();
        assert_eq!(first.words.action_requests, 1u64 << 63 | 1);
        assert_eq!(first.words.queue_selector, -1);
        assert_eq!(first.words.requested_identifier, -1);
        assert_eq!(first.words.readback_identifier, -1);
        assert_eq!(first.words.new_action_presses, 1);
        bytes[0x20..0x28].fill(0);
        bytes[0x108..0x110].fill(0);
        let next = stable_copy(&SliceMemory::new(0x1000, &bytes).unwrap(), source(), || {
            Ok(Some(source()))
        })
        .unwrap();
        assert_eq!(next.words.new_action_presses, 0);
        assert_eq!(next.words.readback_new_presses, 0);
        assert_eq!(next.words.action_requests, first.words.action_requests);
    }
    #[test]
    fn changes_during_copy_and_superseded_owner_do_not_produce_a_mixed_snapshot() {
        struct Changing {
            bytes: [u8; 0x134],
            reads: Cell<usize>,
        }
        impl AddressSpace for Changing {
            fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
                SliceMemory::new(0x1000, &self.bytes)?.read_exact(address, output)?;
                let count = self.reads.get();
                self.reads.set(count + 1);
                if count >= 19 && address == 0x1020 {
                    output[0] ^= 1;
                }
                Ok(())
            }
        }
        let memory = Changing {
            bytes: storage(),
            reads: Cell::new(0),
        };
        assert!(stable_copy(&memory, source(), || Ok(Some(source()))).is_err());
        let bytes = storage();
        let memory = SliceMemory::new(0x1000, &bytes).unwrap();
        assert!(stable_copy(&memory, source(), || Ok(None)).is_err());
        let mut foreign = source();
        foreign.character.native_identifier += 1;
        assert!(stable_copy(&memory, source(), || Ok(Some(foreign))).is_err());
        assert!(
            stable_copy(&memory, source(), || Err(Error::InvalidProfile(
                "lost owner"
            )))
            .is_err()
        );
    }
    #[test]
    fn unreadable_selected_span_is_an_error_without_following_queue_vectors() {
        let mut bytes = storage();
        // Queue allocation links need not be readable for these selected scalar observations.
        for offset in [0x58, 0x60, 0x78, 0x80] {
            bytes[offset..offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        }
        let memory = SliceMemory::new(0x1000, &bytes).unwrap();
        assert!(stable_copy(&memory, source(), || Ok(Some(source()))).is_ok());
        let memory = SliceMemory::new(0x1000, &bytes[..0x133]).unwrap();
        assert!(stable_copy(&memory, source(), || unreachable!()).is_err());
    }
}
