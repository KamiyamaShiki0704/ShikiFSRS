//! Experimental copies of the native dynamic/script Goal fallback definition.
//! Names are native bytes, not proof that a script or callback exists or can run.
use crate::{
    builtin_goals::BuiltinGoalDefinition,
    goal_actions::GoalActionState,
    reader::{NightreignReader, ReaderCore},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};

pub(crate) const SCRIPT_GOAL_TABLE: u32 = 0x2c038e8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptGoalDefinition {
    pub address: u64,
    pub id: i32,
    /// Converted native name followed by Activate, Update, Terminate, Interupt names.
    /// The native suffix spells Interupt with one r; it is preserved verbatim.
    /// Byte encoding is deliberately unspecified. Empty names are preserved.
    pub names: [Vec<u8>; 5],
    pub raw_common_words: [u8; 18],
    /// Cached native callback/dispatch flags. No readiness guarantee is implied.
    pub raw_dispatch_flags: u32,
    pub raw_word_134: u32,
    pub raw_initialized_byte: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptGoalBinding {
    pub control: GoalActionState,
    pub id: i32,
    /// A null context binding is retained separately from an empty script name.
    pub implementation: Option<ScriptGoalDefinition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptGoalStage {
    Activate,
    Update,
    Terminate,
    Interrupt,
}
impl ScriptGoalStage {
    fn slot(self) -> (u32, u32, usize) {
        match self {
            Self::Activate => (0x10, 0x2da9b0, 1),
            Self::Update => (0x18, 0x2dae10, 2),
            Self::Terminate => (0x20, 0x2dad60, 3),
            Self::Interrupt => (0x28, 0x2dac70, 4),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptGoalDispatchPlan {
    pub binding: ScriptGoalBinding,
    pub stage: ScriptGoalStage,
    /// Checked original entry address, for diagnostics only. No callable ABI is exposed.
    pub original_entry_address: u64,
    pub callback_name: Vec<u8>,
}

impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// Copies fallback definitions only. Specific builtin IDs return None without
    /// traversing the implementation. Wrong concrete types or changing data fail.
    /// No Lua state, callback, allocator or registry is dereferenced or invoked.
    pub fn goal_script_binding(
        &self,
        context: u64,
        name_limit: usize,
    ) -> Result<Option<ScriptGoalBinding>> {
        self.core()
            .goal_script_binding(context, name_limit, self.file.builtin_goal_definitions())
    }

    /// Copies the selected stage's original entry and fallback callback name.
    /// Native cached flags may select registry/table dispatch instead of this name.
    /// A plan never establishes callback existence, VM readiness or valid scheduling.
    pub fn goal_script_dispatch_plan(
        &self,
        context: u64,
        stage: ScriptGoalStage,
        name_limit: usize,
    ) -> Result<Option<ScriptGoalDispatchPlan>> {
        let Some(binding) = self.goal_script_binding(context, name_limit)? else {
            return Ok(None);
        };
        let Some(implementation) = &binding.implementation else {
            return Ok(None);
        };
        let (slot, entry_rva, name_index) = stage.slot();
        let slot_rva = SCRIPT_GOAL_TABLE + slot;
        let image = self.file.file().image();
        let expected = u64::from_le_bytes(image.read_rva(slot_rva, 8)?.try_into().expect("slot"));
        if expected != image.preferred_base + entry_rva as u64 {
            return Err(Error::InvalidProfile(
                "NR script Goal stage differs from file",
            ));
        }
        let entry = self
            .module
            .code_address(shikifsrs_runtime::profile::CodeRange {
                rva: entry_rva,
                length: 5,
            })?;
        let slot_address = self
            .module
            .base()
            .checked_add(slot_rva as u64)
            .ok_or(Error::AddressOverflow)?;
        if self.module.memory().read_u64(slot_address)? != entry {
            return Err(Error::ImageMismatch { rva: slot_rva });
        }
        let callback_name = implementation.names[name_index].clone();
        if self.goal_script_binding(context, name_limit)?.as_ref() != Some(&binding) {
            return Err(Error::InvalidProfile(
                "NR script Goal dispatch binding changed",
            ));
        }
        Ok(Some(ScriptGoalDispatchPlan {
            binding,
            stage,
            original_entry_address: entry,
            callback_name,
        }))
    }
}

impl<M: AddressSpace> ReaderCore<'_, M> {
    fn script_name(&self, string: u64, limit: usize) -> Result<Vec<u8>> {
        let mut header = [0; 48];
        self.memory.read_exact(string, &mut header)?;
        let word = |offset: usize| {
            u64::from_le_bytes(header[offset..offset + 8].try_into().expect("word"))
        };
        let length = word(0x18);
        let capacity = word(0x20);
        if length > limit as u64 || length > capacity || capacity > 65535 {
            return Err(Error::InvalidProfile(
                "invalid NR script Goal string bounds",
            ));
        }
        let storage = if capacity < 16 {
            string.checked_add(8).ok_or(Error::AddressOverflow)?
        } else {
            let address = word(8);
            if address == 0 {
                return Err(Error::InvalidProfile("null NR script Goal string storage"));
            }
            address
        };
        let mut bytes = vec![0; length as usize + 1];
        self.memory.read_exact(storage, &mut bytes)?;
        if bytes.pop() != Some(0) {
            return Err(Error::InvalidProfile("unterminated NR script Goal name"));
        }
        let mut repeated_header = [0; 48];
        self.memory.read_exact(string, &mut repeated_header)?;
        if repeated_header != header {
            return Err(Error::InvalidProfile(
                "NR script Goal string header changed",
            ));
        }
        Ok(bytes)
    }

    fn script_definition(
        &self,
        address: u64,
        id: i32,
        limit: usize,
    ) -> Result<ScriptGoalDefinition> {
        self.check_type(address, &[SCRIPT_GOAL_TABLE])?;
        if self.u64(address, 0x128)? != address {
            return Err(Error::InvalidProfile(
                "NR script Goal self definition differs",
            ));
        }
        let mut common = [0; 18];
        self.memory.read_exact(
            address.checked_add(0xf8).ok_or(Error::AddressOverflow)?,
            &mut common,
        )?;
        if i32::from_le_bytes(common[..4].try_into().expect("ID")) != id {
            return Err(Error::InvalidProfile(
                "NR script Goal definition ID differs",
            ));
        }
        let mut names = [const { Vec::new() }; 5];
        for (name, offset) in names.iter_mut().zip([8, 0x38, 0x68, 0x98, 0xc8]) {
            *name = self.script_name(
                address.checked_add(offset).ok_or(Error::AddressOverflow)?,
                limit,
            )?;
        }
        let mut raw = [0; 9];
        self.memory.read_exact(
            address.checked_add(0x130).ok_or(Error::AddressOverflow)?,
            &mut raw,
        )?;
        Ok(ScriptGoalDefinition {
            address,
            id,
            names,
            raw_common_words: common,
            raw_dispatch_flags: u32::from_le_bytes(raw[..4].try_into().expect("flags")),
            raw_word_134: u32::from_le_bytes(raw[4..8].try_into().expect("word")),
            raw_initialized_byte: raw[8],
        })
    }

    pub(crate) fn goal_script_binding(
        &self,
        context: u64,
        limit: usize,
        builtins: &[BuiltinGoalDefinition],
    ) -> Result<Option<ScriptGoalBinding>> {
        if !(1..=4096).contains(&limit) {
            return Err(Error::InvalidProfile(
                "NR script Goal name limit must be 1..=4096",
            ));
        }
        let control = self.goal_action_state(context)?;
        let id = self.u32(context, 0xa0)? as i32;
        if builtins.binary_search_by_key(&id, |row| row.id).is_ok() {
            return Ok(None);
        }
        let address = self.u64(context, 0xa8)?;
        let implementation = if address == 0 {
            None
        } else {
            let first = self.script_definition(address, id, limit)?;
            if self.script_definition(address, id, limit)? != first {
                return Err(Error::InvalidProfile(
                    "NR script Goal definition changed during copy",
                ));
            }
            Some(first)
        };
        self.unchanged_u64(context, 0xa8, address)?;
        if self.u32(context, 0xa0)? as i32 != id || self.goal_action_state(context)? != control {
            return Err(Error::InvalidProfile(
                "NR script Goal binding changed during copy",
            ));
        }
        Ok(Some(ScriptGoalBinding {
            control,
            id,
            implementation,
        }))
    }
}
