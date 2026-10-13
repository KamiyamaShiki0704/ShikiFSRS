use shikifsrs_runtime::{Error, Result, pe::FileImage};

/// Registration indices independently recovered from this version's native definition table.
/// They are not the full IDs carried by callback arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskPhase(usize);

impl TaskPhase {
    pub const FRAME_BEGIN: Self = Self(0);
    pub const DRAW_PARAMETER_UPDATE: Self = Self(107);
    pub const SOUND_STEP: Self = Self(109);
    pub const CHR_POST_PHYSICS: Self = Self(113);
    pub const FRAME_END: Self = Self(168);

    pub fn registration_index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseDefinition {
    pub phase: TaskPhase,
    pub name: String,
    pub native_seed_words: [u32; 4],
}

pub(crate) fn definitions(
    image: &FileImage<'_>,
    table: u32,
    count: usize,
) -> Result<Vec<PhaseDefinition>> {
    if count == 0 || count > super::TASK_GROUP_COUNT {
        return Err(Error::InvalidProfile("invalid task definition count"));
    }
    Ok(
        shikifsrs_runtime::task_definitions::TaskDefinitions::read(image, table, count)?
            .records()
            .iter()
            .map(|record| PhaseDefinition {
                phase: TaskPhase(record.registration_index),
                name: record.name.clone(),
                native_seed_words: record.native_seed_words,
            })
            .collect(),
    )
}
