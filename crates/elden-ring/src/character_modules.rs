//! Experimental module links copied from a fully identified character.
use crate::{
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const MODULE_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x43f0d0,
        length: 0x5e,
    },
    CodeRange {
        rva: 0x43f950,
        length: 0x99,
    },
    CodeRange {
        rva: 0x43dba0,
        length: 0x4b,
    },
    CodeRange {
        rva: 0x43d5b0,
        length: 0x4d,
    },
    CodeRange {
        rva: 0x43d8d0,
        length: 0x4d,
    },
    CodeRange {
        rva: 0x44b8f0,
        length: 0x3a,
    },
    CodeRange {
        rva: 0x44c3a0,
        length: 0x54,
    },
    CodeRange {
        rva: 0x43fb80,
        length: 0x1b0,
    },
    CodeRange {
        rva: 0x65f330,
        length: 0x2ce,
    },
    CodeRange {
        rva: 0xe62d70,
        length: 0x1c7,
    },
    CodeRange {
        rva: 0x3eed20,
        length: 0x8,
    },
    CodeRange {
        rva: 0x3fe100,
        length: 0x611,
    },
    CodeRange {
        rva: 0x43d880,
        length: 0x4d,
    },
    CodeRange {
        rva: 0x43db00,
        length: 0x4d,
    },
    CodeRange {
        rva: 0x43b460,
        length: 0x39,
    },
    CodeRange {
        rva: 0x45bda0,
        length: 0x4c2,
    },
    CodeRange {
        rva: 0x4739e0,
        length: 0x2d7,
    },
    CodeRange {
        rva: 0x43cf70,
        length: 0x31,
    },
    CodeRange {
        rva: 0x4c5930,
        length: 0x5,
    },
    CodeRange {
        rva: 0x4c5940,
        length: 0x73,
    },
    CodeRange {
        rva: 0x43d6a0,
        length: 0x4b,
    },
    CodeRange {
        rva: 0x43d6f0,
        length: 0x4d,
    },
    CodeRange {
        rva: 0x43da10,
        length: 0x4b,
    },
    CodeRange {
        rva: 0x43da60,
        length: 0x4d,
    },
    CodeRange {
        rva: 0x47ee50,
        length: 0x2f,
    },
    CodeRange {
        rva: 0x47f2b0,
        length: 0x2f,
    },
    CodeRange {
        rva: 0x487820,
        length: 0x2f,
    },
    CodeRange {
        rva: 0x487c20,
        length: 0x39,
    },
    CodeRange {
        rva: 0x486d90,
        length: 0x52,
    },
    CodeRange {
        rva: 0x47deb0,
        length: 0x40,
    },
    CodeRange {
        rva: 0x47dff0,
        length: 0x4,
    },
    CodeRange {
        rva: 0x47ef20,
        length: 0x32,
    },
    CodeRange {
        rva: 0x47f380,
        length: 0x1fc,
    },
    CodeRange {
        rva: 0x47e850,
        length: 0x221,
    },
    CodeRange {
        rva: 0x47eb30,
        length: 0xe8,
    },
    CodeRange {
        rva: 0x4873b0,
        length: 0x1a3,
    },
];
const SUPER_ARMOR_INDEX: u32 = 8;
const TOUGHNESS_INDEX: u32 = 9;
const EVENT_INDEX: u32 = 0xb;
const ACTION_REQUEST_INDEX: u32 = 0x10;
const TALK_INDEX: u32 = 0xa;
const DAMAGE_INDEX: u32 = 0x13;
pub(crate) const MODULE_CONTAINER_OFFSET: u32 = 0x190;
pub(crate) const MODULE_OWNER_OFFSET: u32 = 8;
pub(crate) const PHYSICS_INDEX: u32 = 0xd;
const RIDE_INDEX: u32 = 0x1d;
const MODEL_PARAMETER_MODIFIER_INDEX: u32 = 0x1a;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterModule {
    Resistance,
    TimeAct,
    SuperArmor,
    Toughness,
    ActionFlags,
    ActionRequests,
    /// Exact damage module identity only; no attack records or hook ABI are inferred.
    Damage,
    Event,
    Talk,
    Physics,
    Ride,
    ModelParameterModifier,
}

impl CharacterModule {
    fn profile(self, character_kind: ObjectKind) -> (u32, ObjectKind) {
        match self {
            Self::Resistance => (4, ObjectKind::CharacterResistance),
            Self::TimeAct => (3, ObjectKind::CharacterTimeAct),
            Self::SuperArmor => (
                SUPER_ARMOR_INDEX,
                match character_kind {
                    ObjectKind::Player => ObjectKind::PlayerSuperArmor,
                    ObjectKind::Enemy => ObjectKind::EnemySuperArmor,
                    _ => ObjectKind::CharacterSuperArmor,
                },
            ),
            Self::Toughness => (
                TOUGHNESS_INDEX,
                match character_kind {
                    ObjectKind::Player => ObjectKind::PlayerToughness,
                    ObjectKind::Enemy => ObjectKind::EnemyToughness,
                    _ => ObjectKind::CharacterToughness,
                },
            ),
            Self::ActionFlags => (1, ObjectKind::CharacterActionFlags),
            Self::ActionRequests => (ACTION_REQUEST_INDEX, ObjectKind::CharacterActionRequests),
            Self::Damage => (
                DAMAGE_INDEX,
                match character_kind {
                    ObjectKind::Player => ObjectKind::PlayerDamage,
                    ObjectKind::Enemy => ObjectKind::EnemyDamage,
                    _ => ObjectKind::CharacterDamage,
                },
            ),
            Self::Event => (EVENT_INDEX, ObjectKind::CharacterEvent),
            Self::Talk => (TALK_INDEX, ObjectKind::CharacterTalk),
            Self::Physics => (PHYSICS_INDEX, ObjectKind::CharacterPhysics),
            Self::Ride => (RIDE_INDEX, ObjectKind::CharacterRide),
            Self::ModelParameterModifier => (
                MODEL_PARAMETER_MODIFIER_INDEX,
                ObjectKind::CharacterModelParameterModifier,
            ),
        }
    }
}

/// Copied identity and link, never an engine reference guard or permission to mutate.
/// A replacement at the same addresses between reads can remain undetectable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterModuleObservation {
    pub character: CharacterIdentity,
    pub module: CharacterModule,
    pub container_address: u64,
    pub address: u64,
}

fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy the selected slot only. Missing world/character/container/module returns
    /// None; wrong type/owner, stale identity, unreadable links or visible changes fail.
    /// Container padding and unrelated module slots are not accessed.
    pub fn character_module(
        &self,
        target: CharacterIdentity,
        kind: CharacterModule,
    ) -> Result<Option<CharacterModuleObservation>> {
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale character module identity"));
        }
        let memory = self.module.memory();
        let character = self
            .character(target.address)?
            .ok_or(Error::InvalidProfile(
                "character disappeared before module observation",
            ))?;
        let (index, object_kind) = kind.profile(character.kind);
        let link = at(target.address, MODULE_CONTAINER_OFFSET)?;
        let container = memory.read_u64(link)?;
        if !container.is_multiple_of(8) {
            return Err(Error::InvalidProfile(
                "unaligned character module container",
            ));
        }
        let slot = at(container, index * 8)?;
        let address = if container == 0 {
            0
        } else {
            memory.read_u64(slot)?
        };
        let object = self.require(address, object_kind)?;
        let check_owner = || -> Result<()> {
            if address != 0 && memory.read_u64(at(address, MODULE_OWNER_OFFSET)?)? != target.address
            {
                return Err(Error::InvalidProfile(
                    "character module belongs to another owner",
                ));
            }
            Ok(())
        };
        check_owner()?;
        if memory.read_u64(link)? != container
            || (container != 0 && memory.read_u64(slot)? != address)
            || self.require(address, object_kind)? != object
            || self.character_identity(target.address, target.epoch)? != Some(target)
            || self.character(target.address)? != Some(character)
        {
            return Err(Error::InvalidProfile(
                "character module changed during observation",
            ));
        }
        check_owner()?;
        Ok(object.map(|_| CharacterModuleObservation {
            character: target,
            module: kind,
            container_address: container,
            address,
        }))
    }
}
