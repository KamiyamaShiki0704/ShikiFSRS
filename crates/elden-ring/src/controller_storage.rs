//! Copied 32-byte nonvirtual storage allocated by the character controller.
//! Consumer diagnostics called this a collision root; native word roles are unproven.
use crate::{
    character_controller::CharacterControllerObservation, character_sets::CharacterIdentity,
    objects::GameObjectCatalog,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const STORAGE_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3c4d60,
        length: 0x547,
    },
    CodeRange {
        rva: 0x3c48f0,
        length: 0x23,
    },
    CodeRange {
        rva: 0x3c52b0,
        length: 0x2e1,
    },
    CodeRange {
        rva: 0x3c4920,
        length: 0x8,
    },
    CodeRange {
        rva: 0x3c4940,
        length: 0x8,
    },
];
pub(crate) const STORAGE_LINK_OFFSET: u32 = 0x20;
const STORAGE_BYTES: u32 = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerStorageObservation {
    pub controller: CharacterControllerObservation,
    pub address: u64,
    /// Four uninterpreted native words, including all-ones sentinels. No dereferencing.
    pub words: [u64; 4],
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy the complete proven allocation, rechecking its controller link and identity.
    /// This allocation has no RTTI or reciprocal owner field. Link membership does not
    /// guarantee unique ownership, physics readiness, native lifetime or atomicity.
    pub fn controller_storage(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<ControllerStorageObservation>> {
        if target.epoch == 0 {
            return Err(Error::InvalidProfile("zero controller storage epoch"));
        }
        let Some(controller) = self.character_controller(target)? else {
            return Ok(None);
        };
        let memory = self.module.memory();
        let link = controller
            .address
            .checked_add(u64::from(STORAGE_LINK_OFFSET))
            .ok_or(Error::AddressOverflow)?;
        let address = memory.read_u64(link)?;
        if address != 0 && address % 8 != 0 {
            return Err(Error::InvalidProfile("unaligned controller storage"));
        }
        let copy = || -> Result<[u64; 4]> {
            address
                .checked_add(u64::from(STORAGE_BYTES))
                .ok_or(Error::AddressOverflow)?;
            let mut bytes = [0; STORAGE_BYTES as usize];
            memory.read_exact(address, &mut bytes)?;
            Ok(std::array::from_fn(|i| {
                u64::from_le_bytes(bytes[i * 8..i * 8 + 8].try_into().unwrap())
            }))
        };
        let words = if address == 0 { None } else { Some(copy()?) };
        if memory.read_u64(link)? != address
            || self.character_controller(target)? != Some(controller)
            || (words.is_some() && Some(copy()?) != words)
            || memory.read_u64(link)? != address
        {
            return Err(Error::InvalidProfile(
                "controller storage changed during observation",
            ));
        }
        Ok(words.map(|words| ControllerStorageObservation {
            controller,
            address,
            words,
        }))
    }
}
