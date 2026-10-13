//! Independently proven controller ragdoll link and reciprocal character owner.
use crate::{
    character_controller::CharacterControllerObservation,
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind, ObjectObservation},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const RAGDOLL_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3c4d60,
        length: 0x547,
    },
    CodeRange {
        rva: 0x3cb980,
        length: 0x288,
    },
    CodeRange {
        rva: 0xc7a580,
        length: 0xbd,
    },
    CodeRange {
        rva: 0xc78180,
        length: 0x51a,
    },
    CodeRange {
        rva: 0xc68500,
        length: 0x29,
    },
];
pub(crate) const RAGDOLL_LINK_OFFSET: u32 = 0x28;
const RAGDOLL_OWNER_OFFSET: u32 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterRagdollObservation {
    pub controller: CharacterControllerObservation,
    pub ragdoll: ObjectObservation,
}

fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy the exact ragdoll link, rejecting visible controller, class or owner changes.
    /// Missing world/controller/ragdoll returns None. This does not pin native objects,
    /// prove physics readiness, interpret Havok internals or authorize shape writes.
    pub fn character_ragdoll(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterRagdollObservation>> {
        if target.epoch == 0 {
            return Err(Error::InvalidProfile("zero ragdoll observation epoch"));
        }
        let Some(controller) = self.character_controller(target)? else {
            return Ok(None);
        };
        let memory = self.module.memory();
        let link = at(controller.address, RAGDOLL_LINK_OFFSET)?;
        let address = memory.read_u64(link)?;
        let ragdoll = self.require(address, ObjectKind::CharacterRagdoll)?;
        let check_owner = || -> Result<()> {
            if address != 0
                && memory.read_u64(at(address, RAGDOLL_OWNER_OFFSET)?)? != target.address
            {
                return Err(Error::InvalidProfile(
                    "ragdoll belongs to another character",
                ));
            }
            Ok(())
        };
        check_owner()?;
        if memory.read_u64(link)? != address
            || self.require(address, ObjectKind::CharacterRagdoll)? != ragdoll
            || self.character_controller(target)? != Some(controller)
        {
            return Err(Error::InvalidProfile(
                "ragdoll graph changed during observation",
            ));
        }
        check_owner()?;
        Ok(ragdoll.map(|ragdoll| CharacterRagdollObservation {
            controller,
            ragdoll,
        }))
    }
}
