//! Copied debug creation inputs and result storage. No request attribution or ownership.
use crate::{
    character_flags::CharacterFlagsObservation,
    character_sets::CharacterRegistrationObservation,
    objects::{GameObjectCatalog, ObjectKind, ObjectObservation},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const CREATOR_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x505a51,
        length: 0x74,
    },
    CodeRange {
        rva: 0x52b5e0,
        length: 0x4b1,
    },
    CodeRange {
        rva: 0x403690,
        length: 0x522,
    },
    CodeRange {
        rva: 0x4cee70,
        length: 0x5f8,
    },
    CodeRange {
        rva: 0xd43990,
        length: 0x178,
    },
    CodeRange {
        rva: 0xd42d50,
        length: 0x1a0,
    },
    CodeRange {
        rva: 0x96140,
        length: 0x55,
    },
    CodeRange {
        rva: 0x52b540,
        length: 0x5,
    },
    CodeRange {
        rva: 0x54a9bd0,
        length: 0x68,
    },
    CodeRange {
        rva: 0x52b5b0,
        length: 0x5,
    },
    CodeRange {
        rva: 0x55308a3,
        length: 0x2c,
    },
    CodeRange {
        rva: 0xd35f70,
        length: 0x168,
    },
    CodeRange {
        rva: 0x3e6e30,
        length: 0x99f,
    },
    CodeRange {
        rva: 0x1602d0,
        length: 0x137,
    },
    CodeRange {
        rva: 0x52baa0,
        length: 0x5,
    },
    CodeRange {
        rva: 0xd9e23,
        length: 0xd,
    },
    CodeRange {
        rva: 0x4c6ef68,
        length: 0x8c,
    },
    CodeRange {
        rva: 0x4db509e,
        length: 0x37,
    },
    CodeRange {
        rva: 0xe46a85,
        length: 0x11,
    },
    // Full permitted path for next-step selection when current descriptor is known0..2.
    CodeRange {
        rva: 0x52bbc0,
        length: 0x5,
    },
    CodeRange {
        rva: 0x5074863,
        length: 0x66,
    },
    CodeRange {
        rva: 0x52bcb6,
        length: 0x5,
    },
    CodeRange {
        rva: 0xebaa3,
        length: 0xd,
    },
    CodeRange {
        rva: 0x52bcbe,
        length: 0xb,
    },
    CodeRange {
        rva: 0xdb4a25,
        length: 0x17,
    },
    CodeRange {
        rva: 0x52a9f0,
        length: 0x5,
    },
    CodeRange {
        rva: 0xe2423,
        length: 0xd,
    },
    CodeRange {
        rva: 0x52ae80,
        length: 0x5,
    },
    CodeRange {
        rva: 0x5bd26d5,
        length: 0x2c,
    },
    CodeRange {
        rva: 0x52ae90,
        length: 0x5,
    },
    CodeRange {
        rva: 0x580b540,
        length: 0x8b,
    },
    CodeRange {
        rva: 0x52aed0,
        length: 0x5,
    },
    CodeRange {
        rva: 0x52af0c,
        length: 0x5,
    },
    CodeRange {
        rva: 0x585946,
        length: 0xa,
    },
    CodeRange {
        rva: 0x52af12,
        length: 0x9,
    },
    CodeRange {
        rva: 0xf1193,
        length: 0xd,
    },
    CodeRange {
        rva: 0x3b4bad,
        length: 0xb,
    },
    CodeRange {
        rva: 0x52af24,
        length: 0xb,
    },
    CodeRange {
        rva: 0x2658f10,
        length: 0x37,
    },
    CodeRange {
        rva: 0x26592a7,
        length: 0x1b,
    },
    CodeRange {
        rva: 0x52b500,
        length: 0x5,
    },
    CodeRange {
        rva: 0x14ca0ba,
        length: 0x10,
    },
    CodeRange {
        rva: 0x5bd1ad0,
        length: 0x73,
    },
    CodeRange {
        rva: 0x52af4d,
        length: 0x5,
    },
    CodeRange {
        rva: 0x4c5b7bd,
        length: 0x3a,
    },
    CodeRange {
        rva: 0x52af72,
        length: 0x5,
    },
    CodeRange {
        rva: 0x51f46b8,
        length: 0x26,
    },
    CodeRange {
        rva: 0x52af83,
        length: 0xa,
    },
    CodeRange {
        rva: 0x4ef0bb5,
        length: 0x37,
    },
    CodeRange {
        rva: 0x52afc3,
        length: 0x5,
    },
    CodeRange {
        rva: 0x5cd89a3,
        length: 0x82,
    },
    CodeRange {
        rva: 0x59c871a,
        length: 0x39,
    },
    CodeRange {
        rva: 0x5bd9fb2,
        length: 0x22,
    },
    CodeRange {
        rva: 0x52b0ad,
        length: 0x2,
    },
    CodeRange {
        rva: 0x52b12a,
        length: 0x5,
    },
    CodeRange {
        rva: 0x9b9a7,
        length: 0x9,
    },
    CodeRange {
        rva: 0x52b0b1,
        length: 0x5,
    },
    CodeRange {
        rva: 0xea483,
        length: 0xd,
    },
    CodeRange {
        rva: 0x52b0b9,
        length: 0xb,
    },
    CodeRange {
        rva: 0x511768c,
        length: 0x3d,
    },
    CodeRange {
        rva: 0x24fd3b0,
        length: 0x21,
    },
    CodeRange {
        rva: 0x52b5c0,
        length: 0xb,
    },
    CodeRange {
        rva: 0x26d1b07,
        length: 0x1f,
    },
    CodeRange {
        rva: 0x52bab0,
        length: 0x5,
    },
    CodeRange {
        rva: 0x4fce19d,
        length: 0x3e,
    },
    CodeRange {
        rva: 0x52bba6,
        length: 0x5,
    },
    CodeRange {
        rva: 0xedaf3,
        length: 0xd,
    },
    CodeRange {
        rva: 0x52bbae,
        length: 0xb,
    },
    CodeRange {
        rva: 0x5846237,
        length: 0x17,
    },
];
const STEP_CURRENT_OFFSET: u32 = 0x40;
const STEP_NEXT_OFFSET: u32 = 0x44;
const ADVANCE_BYTE_OFFSET: u32 = 0x48;
const WORLD_CREATOR_OFFSET: u32 = 0x1e648;
pub(crate) const INPUT_OFFSET: u32 = 0xb0;
pub(crate) const NAME_OBJECT_OFFSET: u32 = 0x50;
const NAME_BUFFER_OFFSET: u32 = 0x98;
const NAME_LENGTH_OFFSET: u32 = 0xa8;
const NAME_CAPACITY_OFFSET: u32 = 0xb0;
const NATIVE_ARGUMENT_OFFSET: u32 = 0xc0;
pub(crate) const KIND_BYTES_OFFSET: u32 = 0xc8;
pub(crate) const CHARA_INIT_ID_OFFSET: u32 = 0xcc;
const INPUT_MODE_OFFSET: u32 = 0xd0;
const COUNT_OFFSET: u32 = 0x19c;
pub(crate) const ANIMATION_OFFSET: u32 = 0x1a4;
pub(crate) const PHASE_BYTE_OFFSET: u32 = 0x1a8;
pub(crate) const RESULT_OFFSET: u32 = 0x1b0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugCreationInput {
    pub position_bits: [u32; 4],
    /// Passed to the physics initializer as angles; no convention inferred here.
    pub rotation_bits: [u32; 4],
    pub auxiliary20_bits: [u32; 4],
    pub auxiliary30_bits: [u32; 4],
    /// Ordinary enemy branch routes these to native PARAM slots 6 and 9.
    pub npc_param_id: i32,
    pub npc_think_param_id: i32,
    /// Ordinary branch forwards input48 through initializer54 to ChrIns+1e8.
    pub event_entity_id: u32,
    /// Factory input4c, preserved without assigning an unproved meaning.
    pub auxiliary_id4c: i32,
    /// Exact UTF-16 code units from the string consumed by the factory.
    /// May contain embedded NUL or invalid surrogate pairs; not silently repaired.
    pub name_units: Vec<u16>,
    pub name_buffer_address: u64,
    pub name_capacity: u64,
    /// Opaque native argument, not dereferenced or named as an owned object.
    pub native_argument: u64,
    pub kind_bytes: [u8; 2],
    /// Player-creation branch queries PARAM slot23 using inputCC.
    pub chara_init_param_id: i32,
    pub mode: u32,
}

/// Native descriptor order is Idle0/Load1/LoadWait2. Other states remain raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugCreatorStep {
    Idle,
    Load,
    LoadWait,
    Other(i32),
}
impl DebugCreatorStep {
    pub fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::Idle,
            1 => Self::Load,
            2 => Self::LoadWait,
            _ => Self::Other(raw),
        }
    }
}
/// Copied scheduling storage, not an engine barrier or proof of resource readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugCreatorSchedule {
    pub current_step: i32,
    pub next_step: i32,
    /// Virtual slot30 sets one; preserve raw byte and do not classify it as load-complete.
    pub advance_byte: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugCreatorObservation {
    pub world: ObjectObservation,
    pub object: ObjectObservation,
    pub epoch: u64,
    pub input: DebugCreationInput,
    pub schedule: DebugCreatorSchedule,
    pub count: i32,
    pub animation_id: i32,
    /// Idle handler sets one; Load/LoadWait handlers clear it. Not a submit switch.
    pub phase_byte: u8,
    /// Unattributed address only: may be stale or belong to an earlier/foreign request.
    /// It is not dereferenced, pinned, or returned as a character identity.
    pub last_created_address: u64,
}

/// Scheduler storage copied independently of the request. No dependency/queue traversal,
/// readiness inference, native ownership or synchronization is provided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugCreatorDispatchObservation {
    pub creator: DebugCreatorObservation,
    pub descriptor_address: u64,
    /// Three callback/name pointer pairs, copied without dereferencing names.
    pub descriptors: [u64; 6],
    pub dependency_count: u64,
    pub waiting_queue: u64,
    pub special_mode: u8,
    /// Actual predicate, selector and advancement callbacks (virtual18/20/30).
    pub scheduler_callbacks: [u64; 3],
}

/// Fresh exact creation-set membership plus the native event readiness storage.
/// Does not attribute this member to an SDK attempt or acquire an engine lifetime guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugCreationBackreadObservation {
    pub registration: CharacterRegistrationObservation,
    pub flags: CharacterFlagsObservation,
}
impl DebugCreationBackreadObservation {
    pub fn native_ready(self) -> bool {
        self.flags.native_backread_ready()
    }
}

fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy only the fixed native descriptor table and bounded scheduler header, twice.
    /// A foreign/uninitialized descriptor pointer is rejected before dereferencing it.
    pub fn debug_creator_dispatch(
        &self,
        epoch: u64,
    ) -> Result<Option<DebugCreatorDispatchObservation>> {
        let Some(creator) = self.debug_creator(epoch)? else {
            return Ok(None);
        };
        let copy = || -> Result<DebugCreatorDispatchObservation> {
            let memory = self.module.memory();
            let descriptor_address = memory.read_u64(at(creator.object.address, 0x8)?)?;
            let expected = self
                .module
                .base()
                .checked_add(0x3d6a468)
                .ok_or(Error::AddressOverflow)?;
            if descriptor_address != expected {
                return Err(Error::InvalidProfile("unknown creator descriptor table"));
            }
            let mut descriptors = [0; 6];
            for (index, value) in descriptors.iter_mut().enumerate() {
                *value = memory.read_u64(at(descriptor_address, index as u32 * 8)?)?;
            }
            let mut scheduler_callbacks = [0; 3];
            let table = memory.read_u64(creator.object.address)?;
            for (index, offset) in [0x18, 0x20, 0x30].into_iter().enumerate() {
                scheduler_callbacks[index] = memory.read_u64(at(table, offset)?)?;
            }
            Ok(DebugCreatorDispatchObservation {
                creator: creator.clone(),
                descriptor_address,
                descriptors,
                dependency_count: memory.read_u64(at(creator.object.address, 0x28)?)?,
                waiting_queue: memory.read_u64(at(creator.object.address, 0x58)?)?,
                special_mode: memory.read_u8(at(creator.object.address, 0x61)?)?,
                scheduler_callbacks,
            })
        };
        let first = copy()?;
        if copy()? != first || self.debug_creator(epoch)? != Some(creator) {
            return Err(Error::InvalidProfile("creator dispatch storage changed"));
        }
        Ok(Some(first))
    }
    /// Copy readiness only for a supplied registration still belonging to the world's
    /// exact embedded creation set. False is a valid native observation, not a missing
    /// object. Neither value establishes complete resources, initialization or teardown.
    pub fn creation_backread(
        &self,
        expected: CharacterRegistrationObservation,
        limit: usize,
    ) -> Result<Option<DebugCreationBackreadObservation>> {
        if expected.character.epoch == 0 {
            return Err(Error::InvalidProfile("zero creation backread epoch"));
        }
        let Some(before) = self.world_creation_set(limit)? else {
            return Ok(None);
        };
        if expected.set_address != before.set.address
            || expected.character.world_address != before.world_address
        {
            return Err(Error::InvalidProfile(
                "foreign creation backread registration",
            ));
        }
        let Some(registration) =
            self.character_registration(expected.character, expected.set_address, limit)?
        else {
            return Ok(None);
        };
        if registration != expected {
            return Err(Error::InvalidProfile(
                "creation backread registration superseded",
            ));
        }
        let Some(flags) = self.character_flags(expected.character)? else {
            return Ok(None);
        };
        if self.world_creation_set(limit)? != Some(before)
            || self.character_registration(expected.character, expected.set_address, limit)?
                != Some(expected)
            || self.character_flags(expected.character)? != Some(flags)
        {
            return Err(Error::InvalidProfile("creation backread graph changed"));
        }
        Ok(Some(DebugCreationBackreadObservation {
            registration,
            flags,
        }))
    }

    /// Copy only factory-consumed fields and recheck payload, storage and world links.
    /// Does not prevent ABA, take an engine lock, or prove the creator is idle.
    pub fn debug_creator(&self, epoch: u64) -> Result<Option<DebugCreatorObservation>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero debug creator epoch"));
        }
        let Some(world) = self.world()? else {
            return Ok(None);
        };
        let slot = at(world.address, WORLD_CREATOR_OFFSET)?;
        let pointer = self.module.memory().read_u64(slot)?;
        let object = self.require(pointer, ObjectKind::DebugCharacterCreator)?;
        let result = object
            .map(|object| self.copy_creator(world, object, epoch))
            .transpose()?;
        let confirmation = object
            .map(|object| self.copy_creator(world, object, epoch))
            .transpose()?;
        if confirmation != result
            || self.module.memory().read_u64(slot)? != pointer
            || self.world()? != Some(world)
            || self.require(pointer, ObjectKind::DebugCharacterCreator)? != object
        {
            return Err(Error::InvalidProfile(
                "debug creation inputs changed during observation",
            ));
        }
        Ok(result)
    }

    fn copy_creator(
        &self,
        world: ObjectObservation,
        object: ObjectObservation,
        epoch: u64,
    ) -> Result<DebugCreatorObservation> {
        let memory = self.module.memory();
        let input = at(object.address, INPUT_OFFSET)?;
        let mut vectors = [[0; 4]; 4];
        for (index, vector) in vectors.iter_mut().enumerate() {
            let mut bytes = [0; 16];
            memory.read_exact(at(input, index as u32 * 16)?, &mut bytes)?;
            *vector = std::array::from_fn(|i| {
                u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
            });
        }
        let mut ids = [0; 4];
        for (index, id) in ids.iter_mut().enumerate() {
            *id = memory.read_u32(at(input, 0x40 + index as u32 * 4)?)? as i32;
        }
        // Factory 403690 selects inline storage at capacity<=7, external otherwise.
        let length = memory.read_u64(at(input, NAME_LENGTH_OFFSET)?)?;
        let capacity = memory.read_u64(at(input, NAME_CAPACITY_OFFSET)?)?;
        if length > 31 || length > capacity || !(capacity == 7 || (8..=31).contains(&capacity)) {
            return Err(Error::InvalidProfile(
                "debug creator name metadata is outside bounded storage",
            ));
        }
        let buffer = at(input, NAME_BUFFER_OFFSET)?;
        let buffer = if capacity > 7 {
            memory.read_u64(buffer)?
        } else {
            buffer
        };
        if buffer == 0 || !buffer.is_multiple_of(2) {
            return Err(Error::InvalidProfile("invalid debug creator name address"));
        }
        let mut bytes = vec![0; (length as usize + 1) * 2];
        buffer
            .checked_add(bytes.len() as u64 - 1)
            .ok_or(Error::AddressOverflow)?;
        memory.read_exact(buffer, &mut bytes)?;
        let mut units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect();
        if units.pop() != Some(0) {
            return Err(Error::InvalidProfile(
                "debug creator name is not terminated at native length",
            ));
        }
        let mut kind_bytes = [0; 2];
        memory.read_exact(at(input, KIND_BYTES_OFFSET)?, &mut kind_bytes)?;
        Ok(DebugCreatorObservation {
            world,
            object,
            epoch,
            input: DebugCreationInput {
                position_bits: vectors[0],
                rotation_bits: vectors[1],
                auxiliary20_bits: vectors[2],
                auxiliary30_bits: vectors[3],
                npc_param_id: ids[0],
                npc_think_param_id: ids[1],
                event_entity_id: ids[2] as u32,
                auxiliary_id4c: ids[3],
                name_units: units,
                name_buffer_address: buffer,
                name_capacity: capacity,
                native_argument: memory.read_u64(at(input, NATIVE_ARGUMENT_OFFSET)?)?,
                kind_bytes,
                chara_init_param_id: memory.read_u32(at(input, CHARA_INIT_ID_OFFSET)?)? as i32,
                mode: memory.read_u32(at(input, INPUT_MODE_OFFSET)?)?,
            },
            schedule: DebugCreatorSchedule {
                current_step: memory.read_u32(at(object.address, STEP_CURRENT_OFFSET)?)? as i32,
                next_step: memory.read_u32(at(object.address, STEP_NEXT_OFFSET)?)? as i32,
                advance_byte: memory.read_u8(at(object.address, ADVANCE_BYTE_OFFSET)?)?,
            },
            count: memory.read_u32(at(object.address, COUNT_OFFSET)?)? as i32,
            animation_id: memory.read_u32(at(object.address, ANIMATION_OFFSET)?)? as i32,
            phase_byte: memory.read_u8(at(object.address, PHASE_BYTE_OFFSET)?)?,
            last_created_address: memory.read_u64(at(object.address, RESULT_OFFSET)?)?,
        })
    }
}
