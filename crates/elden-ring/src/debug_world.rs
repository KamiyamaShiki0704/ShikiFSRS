//! Copied debug-world input and character override links; no engine ownership.
use crate::{
    character_control_channels::ManipulatorObservation,
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind, ObjectObservation},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

// Selected complete blocks prove field production and consumers, not a callable constructor.
pub(crate) const DEBUG_WORLD_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x512d60,
        length: 0x5,
    },
    CodeRange {
        rva: 0x5444aa0,
        length: 0x4f,
    },
    CodeRange {
        rva: 0x5769fa7,
        length: 0x34,
    },
    CodeRange {
        rva: 0x5918825,
        length: 0x44,
    },
    CodeRange {
        rva: 0x530bd30,
        length: 0x43,
    },
    CodeRange {
        rva: 0x4e42267,
        length: 0x6b,
    },
    CodeRange {
        rva: 0x5cbd551,
        length: 0x42,
    },
    CodeRange {
        rva: 0x55accd5,
        length: 0x37,
    },
    CodeRange {
        rva: 0x5cde8ae,
        length: 0x77,
    },
    CodeRange {
        rva: 0x5660ac0,
        length: 0x68,
    },
    CodeRange {
        rva: 0x514010,
        length: 0x5,
    },
    CodeRange {
        rva: 0x5af943,
        length: 0x10,
    },
    CodeRange {
        rva: 0x505b1b,
        length: 0x42,
    },
    CodeRange {
        rva: 0x508dc0,
        length: 0x57,
    },
];
const DEBUG_WORLD_GLOBAL: u32 = 0x3d6a208;
const INPUT_OFFSET: u32 = 0xa8;
pub(crate) const CHARACTER_OVERRIDE_OFFSET: u32 = 0xb8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugWorldLinksObservation {
    pub object: ObjectObservation,
    pub epoch: u64,
    /// Runtime-typed input link; does not imply exclusive ownership or readiness.
    pub input: Option<ManipulatorObservation>,
    /// 508dc0 queries this before falling back to world's primary player.
    /// Not claimed to affect only cameras or to identify the game's local player.
    pub character_override: Option<CharacterIdentity>,
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    pub fn debug_world_links(&self, epoch: u64) -> Result<Option<DebugWorldLinksObservation>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero debug world epoch"));
        }
        let Some(object) = self.singleton(DEBUG_WORLD_GLOBAL, ObjectKind::DebugWorld)? else {
            return Ok(None);
        };
        let at = |offset| {
            object
                .address
                .checked_add(u64::from(offset))
                .ok_or(Error::AddressOverflow)
        };
        let input_address = self.module.memory().read_u64(at(INPUT_OFFSET)?)?;
        let character_address = self
            .module
            .memory()
            .read_u64(at(CHARACTER_OVERRIDE_OFFSET)?)?;
        let input = self.manipulator(input_address, epoch)?;
        let character_override = if character_address == 0 {
            None
        } else {
            Some(self.character_identity(character_address, epoch)?.ok_or(
                Error::InvalidProfile("debug override character has no world identity"),
            )?)
        };
        if self.module.memory().read_u64(at(INPUT_OFFSET)?)? != input_address
            || self
                .module
                .memory()
                .read_u64(at(CHARACTER_OVERRIDE_OFFSET)?)?
                != character_address
            || self.singleton(DEBUG_WORLD_GLOBAL, ObjectKind::DebugWorld)? != Some(object)
            || self.manipulator(input_address, epoch)? != input
            || (character_address != 0
                && self.character_identity(character_address, epoch)? != character_override)
        {
            return Err(Error::InvalidProfile(
                "debug world links changed during observation",
            ));
        }
        Ok(Some(DebugWorldLinksObservation {
            object,
            epoch,
            input,
            character_override,
        }))
    }
}
