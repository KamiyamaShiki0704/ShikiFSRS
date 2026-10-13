//! Embedded character task links, copied without invoking or releasing engine objects.
use crate::{
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const CHARACTER_TASK_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0xeb30a0,
        length: 0x4c,
    },
    CodeRange {
        rva: 0xeb3560,
        length: 0x50,
    },
    CodeRange {
        rva: 0xeb3750,
        length: 0x11,
    },
    CodeRange {
        rva: 0xeb3770,
        length: 0x4,
    },
    CodeRange {
        rva: 0x3ee2f0,
        length: 0x15,
    },
    CodeRange {
        rva: 0x3ee310,
        length: 0xb,
    },
];
const TASK_0_OFFSET: u32 = 0x418;
const TASK_1_OFFSET: u32 = 0x448;
const TASK_2_OFFSET: u32 = 0x478;
const TASK_3_OFFSET: u32 = 0x4a8;
const TASK_4_OFFSET: u32 = 0x4d8;
const TASK_5_OFFSET: u32 = 0x508;
const CALLBACK_0_RVA: u32 = 0x3f3ff0;
const CALLBACK_1_RVA: u32 = 0x3f8700;
const CALLBACK_2_RVA: u32 = 0x3f9390;
const CALLBACK_3_RVA: u32 = 0x3f9470;
const CALLBACK_4_RVA: u32 = 0x3f92b0;
const CALLBACK_5_RVA: u32 = 0x3f8fd0;

/// A registered proxy can remain queued after its capture link has been cleared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterTaskRegistration {
    pub address: u64,
    pub phase_index: u32,
    pub captures_character_task: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterTaskObservation {
    pub ordinal: u8,
    pub address: u64,
    pub callback_rva: u32,
    pub registration: Option<CharacterTaskRegistration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterTasksObservation {
    pub character: CharacterIdentity,
    pub tasks: [CharacterTaskObservation; 6],
}

fn at(base: u64, offset: u32) -> Result<u64> {
    base.checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Six constructor-proven embedded tasks, owner/callback and optional native proxy.
    /// Missing character/world returns None; contradictions and visible changes fail.
    /// No registration, cancellation, callback execution, phase safety or drain permission.
    pub fn character_tasks(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterTasksObservation>> {
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale character task identity"));
        }
        let tasks = self.character_task_snapshot(target)?;
        if self.character_task_snapshot(target)? != tasks
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile(
                "character tasks changed during observation",
            ));
        }
        Ok(Some(CharacterTasksObservation {
            character: target,
            tasks,
        }))
    }

    fn character_task_snapshot(
        &self,
        target: CharacterIdentity,
    ) -> Result<[CharacterTaskObservation; 6]> {
        let mut result = Vec::with_capacity(6);
        for (ordinal, (offset, callback_rva)) in [
            (TASK_0_OFFSET, CALLBACK_0_RVA),
            (TASK_1_OFFSET, CALLBACK_1_RVA),
            (TASK_2_OFFSET, CALLBACK_2_RVA),
            (TASK_3_OFFSET, CALLBACK_3_RVA),
            (TASK_4_OFFSET, CALLBACK_4_RVA),
            (TASK_5_OFFSET, CALLBACK_5_RVA),
        ]
        .into_iter()
        .enumerate()
        {
            let address = at(target.address, offset)?;
            if self.require(address, ObjectKind::CharacterTask)?.is_none() {
                return Err(Error::InvalidProfile(
                    "unexpected embedded character task type",
                ));
            }
            let memory = self.module.memory();
            let owner = memory.read_u64(at(address, 0x20)?)?;
            let callback = memory.read_u64(at(address, 0x28)?)?;
            if owner != target.address || callback != at(self.module.base(), callback_rva)? {
                return Err(Error::InvalidProfile(
                    "character task owner or callback disagrees",
                ));
            }
            let pointer = memory.read_u64(at(address, 0x10)?)?;
            let registration = if pointer == 0 {
                None
            } else {
                if self
                    .require(pointer, ObjectKind::CharacterTaskProxy)?
                    .is_none()
                {
                    return Err(Error::InvalidProfile(
                        "unexpected character task proxy type",
                    ));
                }
                let capture = memory.read_u64(at(pointer, 0x10)?)?;
                let phase_index = memory.read_u32(at(pointer, 0x18)?)?;
                if (capture != 0 && capture != address)
                    || phase_index >= crate::TASK_GROUP_COUNT as u32
                {
                    return Err(Error::InvalidProfile(
                        "character task proxy capture or phase disagrees",
                    ));
                }
                Some(CharacterTaskRegistration {
                    address: pointer,
                    phase_index,
                    captures_character_task: capture == address,
                })
            };
            result.push(CharacterTaskObservation {
                ordinal: ordinal as u8,
                address,
                callback_rva,
                registration,
            });
        }
        result
            .try_into()
            .map_err(|_| Error::InvalidProfile("invalid embedded task count"))
    }
}
