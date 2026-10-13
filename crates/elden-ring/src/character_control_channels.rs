//! Experimental copied input links. No native input dispatch or reference acquisition.
use crate::{
    character_controller::CharacterControllerObservation,
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind, ObjectObservation},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const CONTROL_CHANNEL_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3cd380,
        length: 0x7e,
    },
    CodeRange {
        rva: 0x3cde20,
        length: 0xaf,
    },
    CodeRange {
        rva: 0x3d1f90,
        length: 0x6e,
    },
    CodeRange {
        rva: 0x3d2f00,
        length: 0x124,
    },
    CodeRange {
        rva: 0x3d4a00,
        length: 0x1a7,
    },
    CodeRange {
        rva: 0x3d8670,
        length: 0x212,
    },
    CodeRange {
        rva: 0x3deaa0,
        length: 0x157,
    },
    CodeRange {
        rva: 0x3e0070,
        length: 0x44,
    },
    CodeRange {
        rva: 0x3c7260,
        length: 0x2e,
    },
    CodeRange {
        rva: 0x3c81e0,
        length: 0xa4,
    },
];
pub(crate) const ORDINARY_INPUT_OFFSET: u32 = 0x18;
pub(crate) const OVERRIDE_INPUT_OFFSET: u32 = 0x3b0;
pub(crate) const INPUT_CHARACTER_OFFSET: u32 = 0xa8;

const INPUT_KINDS: &[ObjectKind] = &[
    ObjectKind::CharacterManipulator,
    ObjectKind::CommandManipulator,
    ObjectKind::FollowManipulator,
    ObjectKind::NetworkAiManipulator,
    ObjectKind::NetworkManipulator,
    ObjectKind::PadManipulator,
    ObjectKind::ReplayManipulator,
    ObjectKind::RideManipulator,
];

/// Copied primary type and optional character identity. Neither object is pinned.
/// A null or different character link is preserved, not repaired or treated as input permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManipulatorObservation {
    pub object: ObjectObservation,
    pub epoch: u64,
    pub character: Option<CharacterIdentity>,
    pub native_character_address: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterControlChannels {
    pub controller: CharacterControllerObservation,
    pub ordinary: Option<ManipulatorObservation>,
    pub override_input: Option<ManipulatorObservation>,
}

impl CharacterControlChannels {
    /// The target selected by the independently proved 3c7260 dispatch path.
    /// Other engine paths, input readiness and correct gameplay control are not inferred.
    pub fn preferred_manipulator(&self) -> Option<ManipulatorObservation> {
        self.override_input.or(self.ordinary)
    }
}

fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    fn require_manipulator(&self, address: u64) -> Result<Option<ObjectObservation>> {
        let object = self.identify(address)?;
        if object.is_some_and(|value| !INPUT_KINDS.contains(&value.kind)) {
            return Err(Error::InvalidProfile("unsupported input primary type"));
        }
        Ok(object)
    }

    pub fn manipulator(&self, address: u64, epoch: u64) -> Result<Option<ManipulatorObservation>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero input observation epoch"));
        }
        if address == 0 {
            return Ok(None);
        }
        let object = self
            .require_manipulator(address)?
            .ok_or(Error::InvalidProfile("missing input object"))?;
        let memory = self.module.memory();
        let link = at(address, INPUT_CHARACTER_OFFSET)?;
        let owner = memory.read_u64(link)?;
        let character = self.character_identity(owner, epoch)?;
        if memory.read_u64(link)? != owner
            || self.require_manipulator(address)? != Some(object)
            || self.character_identity(owner, epoch)? != character
        {
            return Err(Error::InvalidProfile(
                "input object changed during observation",
            ));
        }
        Ok(Some(ManipulatorObservation {
            object,
            epoch,
            character,
            native_character_address: owner,
        }))
    }

    /// Copy ordinary/override channels and recheck the complete controller/character links.
    /// Missing controller returns None; missing channels remain independently None.
    /// Same-type ABA, callback exclusion and native lifetime require caller guarantees.
    pub fn character_control_channels(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterControlChannels>> {
        let Some(controller) = self.character_controller(target)? else {
            return Ok(None);
        };
        let memory = self.module.memory();
        let ordinary_link = at(controller.address, ORDINARY_INPUT_OFFSET)?;
        let override_link = at(controller.address, OVERRIDE_INPUT_OFFSET)?;
        let ordinary_address = memory.read_u64(ordinary_link)?;
        let override_address = memory.read_u64(override_link)?;
        let ordinary = self.manipulator(ordinary_address, target.epoch)?;
        let override_input = self.manipulator(override_address, target.epoch)?;
        if memory.read_u64(ordinary_link)? != ordinary_address
            || memory.read_u64(override_link)? != override_address
            || self.manipulator(ordinary_address, target.epoch)? != ordinary
            || self.manipulator(override_address, target.epoch)? != override_input
            || self.character_controller(target)? != Some(controller)
        {
            return Err(Error::InvalidProfile(
                "control channels changed during observation",
            ));
        }
        Ok(Some(CharacterControlChannels {
            controller,
            ordinary,
            override_input,
        }))
    }
}
