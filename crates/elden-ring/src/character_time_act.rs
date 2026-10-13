//! A selected entry of the native ten-record TimeAct submission history.
//! This ring is neither the currently playing animation nor a completion predicate.
use crate::{
    character_modules::{CharacterModule, CharacterModuleObservation},
    character_sets::CharacterIdentity,
    objects::GameObjectCatalog,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const TIME_ACT_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x430860,
        length: 0x61,
    },
    CodeRange {
        rva: 0x430d50,
        length: 0x14a,
    },
    CodeRange {
        rva: 0x43dbf0,
        length: 0x4d,
    },
    CodeRange {
        rva: 0x430eb0,
        length: 0x16e,
    },
];
const HISTORY_OFFSET: u32 = 0x20;
const HISTORY_STRIDE: u32 = 0x10;
const NEXT_INDEX_OFFSET: u32 = 0xc0;
const HISTORY_COUNT: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterTimeActRecord {
    pub module: CharacterModuleObservation,
    pub index: u8,
    /// Signed identifier argument recorded by native 430eb0, including -1 sentinels.
    pub identifier: i32,
    /// Three subsequent argument words, preserved without inventing time/speed semantics.
    pub argument_words: [u32; 3],
    /// Raw cursor updated modulo ten by 430eb0. Not a generation or elapsed-frame counter.
    pub next_index_raw: i32,
}
fn selected_index(index: u8) -> Result<()> {
    if u32::from(index) >= HISTORY_COUNT {
        return Err(Error::InvalidProfile(
            "TimeAct history index outside native ring",
        ));
    }
    Ok(())
}
fn copy_record(
    memory: &impl AddressSpace,
    source: CharacterModuleObservation,
    index: u8,
) -> Result<CharacterTimeActRecord> {
    let at = |offset: u32| {
        source
            .address
            .checked_add(u64::from(offset))
            .ok_or(Error::AddressOverflow)
    };
    let mut bytes = [0; 16];
    memory.read_exact(
        at(HISTORY_OFFSET + u32::from(index) * HISTORY_STRIDE)?,
        &mut bytes,
    )?;
    let words: [u32; 4] =
        std::array::from_fn(|i| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()));
    Ok(CharacterTimeActRecord {
        module: source,
        index,
        identifier: words[0] as i32,
        argument_words: [words[1], words[2], words[3]],
        next_index_raw: memory.read_u32(at(NEXT_INDEX_OFFSET)?)? as i32,
    })
}
fn stable_copy(
    memory: &impl AddressSpace,
    source: CharacterModuleObservation,
    index: u8,
    recheck: impl FnOnce() -> Result<Option<CharacterModuleObservation>>,
) -> Result<CharacterTimeActRecord> {
    selected_index(index)?;
    let copied = copy_record(memory, source, index)?;
    if copy_record(memory, source, index)? != copied || recheck()? != Some(source) {
        return Err(Error::InvalidProfile(
            "TimeAct selected record, cursor or owner changed",
        ));
    }
    Ok(copied)
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy only one 16-byte record and the cursor, then recheck their typed reciprocal owner.
    /// Resource links and other records are not followed. Native lifetime, exclusion and ABA
    /// remain caller obligations; repeated values do not acquire a lease or prove playback.
    pub fn character_time_act_record(
        &self,
        target: CharacterIdentity,
        index: u8,
    ) -> Result<Option<CharacterTimeActRecord>> {
        selected_index(index)?;
        let Some(source) = self.character_module(target, CharacterModule::TimeAct)? else {
            return Ok(None);
        };
        stable_copy(self.module.memory(), source, index, || {
            self.character_module(target, CharacterModule::TimeAct)
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
                address: 0x2000,
                world_address: 0x3000,
                native_identifier: 8,
                epoch: 1,
            },
            module: CharacterModule::TimeAct,
            container_address: 0x4000,
            address: 0x1000,
        }
    }
    fn storage() -> [u8; 0xc4] {
        let mut bytes = [0; 0xc4];
        // Unreadable resource pointers are deliberately not followed by the reader.
        bytes[0x10..0x20].fill(0xff);
        for index in 0..10 {
            for (lane, word) in [u32::MAX - index, 0x7fc01234, 0x80000000, 0x3f800000]
                .into_iter()
                .enumerate()
            {
                let begin = 0x20 + index as usize * 16 + lane * 4;
                bytes[begin..begin + 4].copy_from_slice(&word.to_le_bytes());
            }
        }
        bytes[0xc0..0xc4].copy_from_slice(&9_i32.to_le_bytes());
        bytes
    }
    #[test]
    fn all_ten_selected_records_preserve_signed_ids_raw_arguments_and_ring_cursor() {
        let bytes = storage();
        let memory = SliceMemory::new(0x1000, &bytes).unwrap();
        for index in 0..10 {
            let v = stable_copy(&memory, source(), index, || Ok(Some(source()))).unwrap();
            assert_eq!(v.identifier, -1 - i32::from(index));
            assert_eq!(v.argument_words, [0x7fc01234, 0x80000000, 0x3f800000]);
            assert_eq!(v.next_index_raw, 9);
        }
        assert!(stable_copy(&memory, source(), 10, || unreachable!()).is_err());
    }
    #[test]
    fn changed_record_or_owner_rejects_mixed_history() {
        struct Changing {
            bytes: [u8; 0xc4],
            reads: Cell<u32>,
        }
        impl AddressSpace for Changing {
            fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
                SliceMemory::new(0x1000, &self.bytes)?.read_exact(address, output)?;
                let reads = self.reads.get();
                self.reads.set(reads + 1);
                if reads >= 2 && address == 0x1040 {
                    output[0] ^= 1;
                }
                Ok(())
            }
        }
        let memory = Changing {
            bytes: storage(),
            reads: Cell::new(0),
        };
        assert!(stable_copy(&memory, source(), 2, || Ok(Some(source()))).is_err());
        let bytes = storage();
        let memory = SliceMemory::new(0x1000, &bytes).unwrap();
        assert!(stable_copy(&memory, source(), 2, || Ok(None)).is_err());
        let mut foreign = source();
        foreign.character.epoch += 1;
        assert!(stable_copy(&memory, source(), 2, || Ok(Some(foreign))).is_err());
    }
    #[test]
    fn truncated_cursor_or_record_fails_before_owner_recheck() {
        let bytes = storage();
        for end in [0x4f, 0xc3] {
            let memory = SliceMemory::new(0x1000, &bytes[..end]).unwrap();
            assert!(stable_copy(&memory, source(), 2, || unreachable!()).is_err());
        }
    }
}
