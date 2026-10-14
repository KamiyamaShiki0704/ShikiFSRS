//! Exact-version Nightreign metadata and experimental copied object views.
pub mod character_actions;
pub mod character_effects;
pub mod character_modules;
pub mod character_operations;
pub mod character_state;
pub mod character_think;
#[cfg(windows)]
pub mod current_tasks;
pub mod effect_addition;
pub mod effect_operations;
pub mod effect_tiers;
pub mod goal_actions;
pub mod goal_children;
pub mod goal_requests;
pub mod goal_roots;
pub mod native_tasks;
pub mod parameter_fields;
pub mod parameter_rows;
pub mod parameter_schema;
pub mod parameters;
pub mod reader;
pub mod roots;
pub mod think_actions;
pub mod think_targets;
use shikifsrs_runtime::{
    Error, Result,
    memory::AddressSpace,
    profile::{CodeRange, ExecutableIdentity, VerifiedFile},
    task_definitions::TaskDefinitions,
};

/// Executable file version, not the game's App or Regulation version.
pub const EXE_1330: ExecutableIdentity = ExecutableIdentity {
    name: "Nightreign / EXE 1.3.3.0",
    file_size: 118_835_320,
    sha256: [
        0x95, 0x45, 0xe8, 0x73, 0x29, 0x19, 0xbb, 0x6a, 0x63, 0x90, 0x4c, 0xb7, 0x32, 0x23, 0x64,
        0x1b, 0x3c, 0x4f, 0x9a, 0x67, 0x86, 0x9b, 0xe5, 0x72, 0x26, 0x12, 0xac, 0xca, 0x41, 0x3e,
        0x8b, 0x3a,
    ],
};

// The initializer's first loop consumes [0x2cdf8e0, 0x2ce0c60) in 32-byte rows.
// Dummy is included in that file table; this count does not authorize live registration.
const DEFINITION_TABLE: u32 = 0x2cdf8e0;
pub const TASK_DEFINITION_COUNT: usize = 156;

pub struct NightreignFile<'a> {
    file: VerifiedFile<'a>,
    definitions: TaskDefinitions,
    parameters: Vec<parameters::ParameterDefinition>,
}

impl<'a> NightreignFile<'a> {
    /// Accepts only the exact evidenced immutable file. Modified and unknown files fail.
    pub fn verify(bytes: &'a [u8]) -> Result<Self> {
        let file = VerifiedFile::verify(bytes, EXE_1330)?;
        Self::from_verified(file)
    }

    /// Reuses one checked immutable file without hashing it again.
    pub fn from_verified(file: VerifiedFile<'a>) -> Result<Self> {
        let identity = file.identity();
        if identity.file_size != EXE_1330.file_size || identity.sha256 != EXE_1330.sha256 {
            return Err(Error::UnsupportedImage {
                expected: EXE_1330.name,
                actual_sha256: identity.sha256,
            });
        }
        let definitions =
            TaskDefinitions::read(file.image(), DEFINITION_TABLE, TASK_DEFINITION_COUNT)?;
        for (name, index) in [
            ("FrameBegin", 0),
            ("DrawParamUpdate", 92),
            ("SoundStep", 94),
            ("ChrIns_PostPhysics", 98),
            ("FrameEnd", 154),
            ("Dummy", 155),
        ] {
            if definitions.find(name).map(|row| row.registration_index) != Some(index) {
                return Err(Error::InvalidProfile(
                    "Nightreign task definitions differ from the evidenced file",
                ));
            }
        }
        validate_object_metadata(file.image())?;
        roots::validate(file.image())?;
        validate_character_metadata(file.image())?;
        let parameters = parameters::read(file.image())?;
        Ok(Self {
            file,
            definitions,
            parameters,
        })
    }

    pub fn file(&self) -> &VerifiedFile<'a> {
        &self.file
    }

    pub fn task_definitions(&self) -> &TaskDefinitions {
        &self.definitions
    }

    pub fn parameter_definitions(&self) -> &[parameters::ParameterDefinition] {
        &self.parameters
    }

    /// Checks loaded headers, evidenced code spans and relocated primary vtable slots.
    /// The provider must supply stable memory for coherent object snapshots. This does
    /// not discover roots or authorize an engine phase, ownership transfer or calls.
    pub fn bind_reader<'r, M: AddressSpace>(
        &'r self,
        memory: &'r M,
        base: u64,
    ) -> Result<reader::NightreignReader<'r, 'a, M>> {
        let module = self.file.bind(memory, base, READ_CODE)?;
        for range in native_evidence()? {
            module.code_address(range)?;
        }
        for range in character_evidence()? {
            module.code_address(range)?;
        }
        for range in state_evidence()? {
            module.code_address(range)?;
        }
        for range in action_effect_evidence()? {
            module.code_address(range)?;
        }
        for range in effect_mutation_evidence()? {
            module.code_address(range)?;
        }
        for range in effect_addition_evidence()? {
            module.code_address(range)?;
        }
        for range in effect_tier_evidence()? {
            module.code_address(range)?;
        }
        for range in goal_action_evidence()? {
            module.code_address(range)?;
        }
        for range in goal_request_evidence()? {
            module.code_address(range)?;
        }
        for range in think_action_evidence()? {
            module.code_address(range)?;
        }
        for (_, table) in OBJECT_TYPES {
            let expected = self.file.image().read_rva(*table, 8)?;
            let pointer = u64::from_le_bytes(expected.try_into().expect("checked width"));
            let target = pointer
                .checked_sub(self.file.image().preferred_base)
                .and_then(|rva| base.checked_add(rva))
                .ok_or(Error::AddressOverflow)?;
            let slot = base
                .checked_add(*table as u64)
                .ok_or(Error::AddressOverflow)?;
            if memory.read_u64(slot)? != target {
                return Err(Error::ImageMismatch { rva: *table });
            }
        }
        Ok(reader::NightreignReader { module, file: self })
    }
}

pub(crate) fn state_evidence() -> Result<Vec<CodeRange>> {
    include_str!("../data/state-code.csv")
        .lines()
        .map(|line| {
            let mut fields = line.split(',');
            let rva = u32::from_str_radix(
                fields
                    .next()
                    .ok_or(Error::InvalidProfile("missing NR state RVA"))?,
                16,
            )
            .map_err(|_| Error::InvalidProfile("invalid NR state RVA"))?;
            let length = fields
                .next()
                .ok_or(Error::InvalidProfile("missing NR state extent"))?
                .parse::<usize>()
                .map_err(|_| Error::InvalidProfile("invalid NR state extent"))?;
            Ok(CodeRange { rva, length })
        })
        .collect()
}

pub(crate) fn action_effect_evidence() -> Result<Vec<CodeRange>> {
    include_str!("../data/action-effect-code.csv")
        .lines()
        .map(|line| {
            let mut fields = line.split(',');
            let rva = u32::from_str_radix(
                fields
                    .next()
                    .ok_or(Error::InvalidProfile("missing NR action/effect RVA"))?,
                16,
            )
            .map_err(|_| Error::InvalidProfile("invalid NR action/effect RVA"))?;
            let length = fields
                .next()
                .ok_or(Error::InvalidProfile("missing NR action/effect extent"))?
                .parse::<usize>()
                .map_err(|_| Error::InvalidProfile("invalid NR action/effect extent"))?;
            Ok(CodeRange { rva, length })
        })
        .collect()
}
pub(crate) fn effect_mutation_evidence() -> Result<Vec<CodeRange>> {
    include_str!("../data/effect-removal-code.csv")
        .lines()
        .map(|line| {
            let mut fields = line.split(',');
            let rva = u32::from_str_radix(
                fields
                    .next()
                    .ok_or(Error::InvalidProfile("missing NR effect mutation RVA"))?,
                16,
            )
            .map_err(|_| Error::InvalidProfile("invalid NR effect mutation RVA"))?;
            let length = fields
                .next()
                .ok_or(Error::InvalidProfile("missing NR effect mutation extent"))?
                .parse::<usize>()
                .map_err(|_| Error::InvalidProfile("invalid NR effect mutation extent"))?;
            Ok(CodeRange { rva, length })
        })
        .collect()
}
pub(crate) fn effect_addition_evidence() -> Result<Vec<CodeRange>> {
    include_str!("../data/effect-addition-code.csv")
        .lines()
        .map(|line| {
            let mut fields = line.split(',');
            let rva = u32::from_str_radix(
                fields
                    .next()
                    .ok_or(Error::InvalidProfile("missing NR addition RVA"))?,
                16,
            )
            .map_err(|_| Error::InvalidProfile("invalid NR addition RVA"))?;
            let length = fields
                .next()
                .ok_or(Error::InvalidProfile("missing NR addition extent"))?
                .parse::<usize>()
                .map_err(|_| Error::InvalidProfile("invalid NR addition extent"))?;
            Ok(CodeRange { rva, length })
        })
        .collect()
}
pub(crate) fn effect_tier_evidence() -> Result<Vec<CodeRange>> {
    include_str!("../data/effect-tier-code.csv")
        .lines()
        .map(|line| {
            let mut fields = line.split(',');
            let rva = u32::from_str_radix(
                fields
                    .next()
                    .ok_or(Error::InvalidProfile("missing NR tier RVA"))?,
                16,
            )
            .map_err(|_| Error::InvalidProfile("invalid NR tier RVA"))?;
            let length = fields
                .next()
                .ok_or(Error::InvalidProfile("missing NR tier extent"))?
                .parse::<usize>()
                .map_err(|_| Error::InvalidProfile("invalid NR tier extent"))?;
            Ok(CodeRange { rva, length })
        })
        .collect()
}
pub(crate) fn goal_action_evidence() -> Result<Vec<CodeRange>> {
    include_str!("../data/goal-action-code.csv")
        .lines()
        .map(|line| {
            let mut fields = line.split(',');
            let rva = u32::from_str_radix(
                fields
                    .next()
                    .ok_or(Error::InvalidProfile("missing NR goal RVA"))?,
                16,
            )
            .map_err(|_| Error::InvalidProfile("invalid NR goal RVA"))?;
            let length = fields
                .next()
                .ok_or(Error::InvalidProfile("missing NR goal extent"))?
                .parse::<usize>()
                .map_err(|_| Error::InvalidProfile("invalid NR goal extent"))?;
            Ok(CodeRange { rva, length })
        })
        .collect()
}
pub(crate) fn goal_request_evidence() -> Result<Vec<CodeRange>> {
    include_str!("../data/goal-request-code.csv")
        .lines()
        .map(|line| {
            let mut fields = line.split(',');
            let rva = u32::from_str_radix(
                fields
                    .next()
                    .ok_or(Error::InvalidProfile("missing NR Goal request RVA"))?,
                16,
            )
            .map_err(|_| Error::InvalidProfile("invalid NR Goal request RVA"))?;
            let length = fields
                .next()
                .ok_or(Error::InvalidProfile("missing NR Goal request extent"))?
                .parse::<usize>()
                .map_err(|_| Error::InvalidProfile("invalid NR Goal request extent"))?;
            Ok(CodeRange { rva, length })
        })
        .collect()
}
pub(crate) fn think_action_evidence() -> Result<Vec<CodeRange>> {
    include_str!("../data/think-action-code.csv")
        .lines()
        .map(|line| {
            let mut fields = line.split(',');
            let rva = u32::from_str_radix(
                fields
                    .next()
                    .ok_or(Error::InvalidProfile("missing NR Think RVA"))?,
                16,
            )
            .map_err(|_| Error::InvalidProfile("invalid NR Think RVA"))?;
            let length = fields
                .next()
                .ok_or(Error::InvalidProfile("missing NR Think extent"))?
                .parse::<usize>()
                .map_err(|_| Error::InvalidProfile("invalid NR Think extent"))?;
            Ok(CodeRange { rva, length })
        })
        .collect()
}

pub(crate) fn character_evidence() -> Result<Vec<CodeRange>> {
    include_str!("../data/character-code.csv")
        .lines()
        .map(|line| {
            let mut fields = line.split(',');
            let rva = u32::from_str_radix(
                fields
                    .next()
                    .ok_or(Error::InvalidProfile("missing NR character RVA"))?,
                16,
            )
            .map_err(|_| Error::InvalidProfile("invalid NR character RVA"))?;
            let length = fields
                .next()
                .ok_or(Error::InvalidProfile("missing NR character extent"))?
                .parse::<usize>()
                .map_err(|_| Error::InvalidProfile("invalid NR character extent"))?;
            Ok(CodeRange { rva, length })
        })
        .collect()
}
fn validate_character_metadata(image: &shikifsrs_runtime::pe::FileImage<'_>) -> Result<()> {
    for (name, table) in [
        (".?AVCSChrDataModule@CS@@", 0x2c10b88),
        (".?AVCSChrActionFlagModule@CS@@", 0x2c0c6d0),
        (".?AVCSChrBehaviorScriptModule@CS@@", 0x2c10068),
        (".?AVCSChrResistModule@CS@@", 0x2c113c8),
        (".?AVCSChrBehaviorModule@CS@@", 0x2c0fef0),
        (".?AVCSChrBehaviorSyncModule@CS@@", 0x2c10138),
        (".?AVCSChrPhysicsModule@CS@@", 0x2c13720),
        (".?AVCSChrActionRequestModule@CS@@", 0x2c0c808),
        (".?AVCSChrBehaviorDataModule@CS@@", 0x2c0f278),
        (".?AVSpecialEffect@CS@@", character_effects::EFFECT_TABLE),
        (".?AVCSGoalFunc@CS@@", goal_actions::GOAL_FUNC_TABLE),
        (".?AVEzActionExecCtrl@CS@@", goal_actions::CONTROL_TABLE),
        (".?AVGoalExecAction@CS@@", goal_requests::EXEC_GOAL_TABLE),
        (".?AVCSAiFunc@CS@@", think_actions::AI_FUNC_TABLE),
        (
            ".?AVComManipulator@CS@@",
            character_think::COM_MANIPULATOR_TABLE,
        ),
        (".?AVCSComThinkOwner@CS@@", character_think::COM_OWNER_TABLE),
        (
            ".?AVSelfTarget@CSAiThink@CS@@",
            think_targets::SELF_TARGET_TABLE,
        ),
    ] {
        if !image
            .vtables(name)?
            .iter()
            .any(|v| v.table_rva == table && v.object_offset == 0 && !v.entries.is_empty())
        {
            return Err(Error::InvalidProfile(
                "NR character module RTTI differs from file",
            ));
        }
    }
    for field in parameter_fields::definitions() {
        if field.width == 0
            || field.width > 4
            || field
                .offset
                .checked_add(field.width)
                .is_none_or(|end| end > field.packed_size)
            || field.bit.is_some_and(|bit| bit >= 8 || field.width != 1)
        {
            return Err(Error::InvalidProfile("invalid pinned NR field descriptor"));
        }
    }
    Ok(())
}

pub(crate) fn native_evidence() -> Result<Vec<CodeRange>> {
    include_str!("../data/native-code.csv")
        .lines()
        .map(|line| {
            let mut fields = line.split(',');
            let rva = u32::from_str_radix(
                fields
                    .next()
                    .ok_or(Error::InvalidProfile("missing NR native RVA"))?,
                16,
            )
            .map_err(|_| Error::InvalidProfile("invalid NR native RVA"))?;
            let length = fields
                .next()
                .ok_or(Error::InvalidProfile("missing NR native extent"))?
                .parse::<usize>()
                .map_err(|_| Error::InvalidProfile("invalid NR native extent"))?;
            Ok(CodeRange { rva, length })
        })
        .collect()
}

const OBJECT_TYPES: &[(&str, u32)] = &[
    (".?AVCSTaskGroupImp@CS@@", reader::TASK_TABLE),
    (".?AVCSTaskGroupIns@CS@@", reader::GROUP_TABLE),
    (".?AVCSTimeLineTaskGroupIns@CS@@", reader::TIMELINE_TABLE),
    (".?AVWorldChrManImp@CS@@", reader::WORLD_TABLE),
    (".?AVChrSet@CS@@", reader::PLAYER_POOL_TABLE),
    (".?AVPlayerIns@CS@@", reader::PLAYER_TABLE),
    (".?AVEnemyIns@CS@@", reader::ENEMY_TABLE),
    (".?AVChrIns@CS@@", reader::CHARACTER_TABLE),
    (".?AVSoloParamRepositoryImp@CS@@", reader::PARAMETER_TABLE),
    (".?AVParamResCap@CS@@", reader::PARAMETER_CAP_TABLE),
    (".?AVFD4Time@FD4@@", 0x2bda590),
];

// Selected code evidence spans, including leaf getters without .pdata entries.
// A span is not a claim that protected continuations or native ABI are reconstructed.
const READ_CODE: &[CodeRange] = &[
    CodeRange {
        rva: 0xfec930,
        length: 0x6c,
    }, // 156 task slots
    CodeRange {
        rva: 0xfed500,
        length: 0x18,
    }, // indexed slot transfer
    CodeRange {
        rva: 0xfecc80,
        length: 0x20,
    }, // checked indexed task lookup
    CodeRange {
        rva: 0xfece30,
        length: 0x1aa,
    }, // group name/seed fields
    CodeRange {
        rva: 0xfed160,
        length: 0xf8,
    }, // group seed fields
    CodeRange {
        rva: 0xfed8d0,
        length: 0xa7,
    }, // timeline seed copy
    CodeRange {
        rva: 0x514e9c,
        length: 0x68,
    }, // world player pool construction route
    CodeRange {
        rva: 0x5197e0,
        length: 0x84,
    }, // player pool/key lookup
    CodeRange {
        rva: 0x6df1d6b,
        length: 0x4f,
    }, // protected ChrSet constructor route
    CodeRange {
        rva: 0x4ac770,
        length: 0xf,
    }, // 16-byte player slot stride
    CodeRange {
        rva: 0x4ac780,
        length: 4,
    }, // player count
    CodeRange {
        rva: 0x66c6e0,
        length: 8,
    }, // player data pointer
    CodeRange {
        rva: 0x66c6f0,
        length: 0x28,
    }, // player lookup key
    CodeRange {
        rva: 0xe9e680,
        length: 0x2f,
    }, // parameter cap bounds/stride
    CodeRange {
        rva: 0xe9e910,
        length: 0x65,
    }, // cap insert/count update
    CodeRange {
        rva: 0xe9f040,
        length: 0x98,
    }, // parameter name lookup and fallback
];

fn validate_object_metadata(image: &shikifsrs_runtime::pe::FileImage<'_>) -> Result<()> {
    for (name, table) in OBJECT_TYPES {
        if !image.vtables(name)?.iter().any(|vtable| {
            vtable.table_rva == *table && vtable.object_offset == 0 && !vtable.entries.is_empty()
        }) {
            return Err(Error::InvalidProfile(
                "NR object RTTI differs from evidenced file",
            ));
        }
    }
    for range in READ_CODE {
        let section = image
            .section_at(range.rva)
            .filter(|section| section.is_executable())
            .ok_or(Error::InvalidProfile("NR read evidence is not executable"))?;
        if range.rva as u64 + range.length as u64 > section.rva as u64 + section.raw_size as u64 {
            return Err(Error::InvalidProfile("NR read evidence crosses section"));
        }
        image.read_rva(range.rva, range.length)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_files_are_rejected_before_metadata_access() {
        for bytes in [&[][..], b"MZ".as_slice(), &[0; 1024][..]] {
            assert!(matches!(
                NightreignFile::verify(bytes),
                Err(Error::UnsupportedImage { .. })
            ));
        }
    }
}
