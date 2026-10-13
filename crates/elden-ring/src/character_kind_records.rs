//! Four independently consumed classification bytes in the native writable data table, without role-name guesses.
use crate::{
    character_parameters::CharacterSelectionObservation, character_sets::CharacterIdentity,
    objects::GameObjectCatalog,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const KIND_RECORD_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x4c8730,
        length: 0x21,
    },
    CodeRange {
        rva: 0x4c85f0,
        length: 0x21,
    },
    CodeRange {
        rva: 0x4c8620,
        length: 0x21,
    },
    CodeRange {
        rva: 0x4c86a0,
        length: 0x21,
    },
];
pub(crate) const KIND_RECORD_TABLE: u32 = 0x3b1bc00;
pub(crate) const KIND_RECORD_STRIDE: u32 = 20;
pub(crate) const KIND_RECORD_COUNT: u32 = 23;
pub(crate) const KIND_RECORD_FALLBACK: u32 = 0x3b1bdcc;
const SELECTED_OFFSETS: [u32; 4] = [1, 8, 9, 10];

/// A copied native record selection. These bytes are not engine
/// ownership, a network session, team allegiance or completed gameplay behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterKindRecordObservation {
    pub selection: CharacterSelectionObservation,
    pub record_rva: u32,
    /// Exact offsets 1/8/9/a; preserve unknown values rather than coercing to bool.
    pub selected_bytes: [u8; 4],
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy only four native-consumed bytes from the loaded writable table,
    /// repeating selection and selected-byte verification. File values are
    /// initializer evidence; stable runtime modifications remain actual data.
    /// Signed out-of-range kinds take the native default record, never indexing
    /// arbitrary memory. No native calls or allocations are performed.
    pub fn character_kind_record(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterKindRecordObservation>> {
        let Some(selection) = self.character_selection(target)? else {
            return Ok(None);
        };
        let row = selection.raw_kind as u32;
        let record_rva = if row < KIND_RECORD_COUNT {
            KIND_RECORD_TABLE + row * KIND_RECORD_STRIDE
        } else {
            KIND_RECORD_FALLBACK
        };
        let image = self.module.file().image();
        let section = image
            .section_at(KIND_RECORD_TABLE)
            .filter(|section| {
                section.is_writable()
                    && !section.is_executable()
                    && section.characteristics & 0x40000000 != 0
            })
            .ok_or(Error::InvalidProfile(
                "kind record table is not readable writable data",
            ))?;
        if u64::from(KIND_RECORD_FALLBACK) + u64::from(KIND_RECORD_STRIDE)
            > u64::from(section.rva) + u64::from(section.raw_size)
        {
            return Err(Error::InvalidProfile(
                "kind record table crosses its section",
            ));
        }
        let read = || -> Result<[u8; 4]> {
            let mut bytes = [0; 4];
            for (index, offset) in SELECTED_OFFSETS.into_iter().enumerate() {
                let rva = record_rva
                    .checked_add(offset)
                    .ok_or(Error::AddressOverflow)?;
                let address = self
                    .module
                    .base()
                    .checked_add(u64::from(rva))
                    .ok_or(Error::AddressOverflow)?;
                let actual = self.module.memory().read_u8(address)?;
                bytes[index] = actual;
            }
            Ok(bytes)
        };
        let selected_bytes = read()?;
        if read()? != selected_bytes
            || self.character_selection(target)? != Some(selection)
            || read()? != selected_bytes
        {
            return Err(Error::InvalidProfile("kind record selection changed"));
        }
        Ok(Some(CharacterKindRecordObservation {
            selection,
            record_rva,
            selected_bytes,
        }))
    }
}
