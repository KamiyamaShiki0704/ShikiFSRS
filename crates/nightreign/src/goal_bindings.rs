//! Experimental builtin Goal definition bindings and copied common definition words.
use crate::{
    builtin_goals::BuiltinGoalDefinition,
    goal_actions::GoalActionState,
    reader::{NightreignReader, ReaderCore},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinGoalImplementation {
    pub address: u64,
    pub raw_word_fc: u32,
    pub raw_float_100_bits: u32,
    pub raw_float_104_bits: u32,
    pub raw_word_108: u16,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalBuiltinBinding {
    pub control: GoalActionState,
    pub definition: BuiltinGoalDefinition,
    pub implementation: Option<BuiltinGoalImplementation>,
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// Unknown/dynamic definition IDs return None. A known builtin with a null
    /// implementation produces Some with implementation=None. Bound objects must
    /// match both context ID and exact concrete type. No definition string storage,
    /// callback, allocation or native behavior is touched.
    pub fn goal_builtin_binding(&self, context: u64) -> Result<Option<GoalBuiltinBinding>> {
        self.core()
            .goal_builtin_binding(context, self.file.builtin_goal_definitions())
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn goal_builtin_binding(
        &self,
        context: u64,
        definitions: &[BuiltinGoalDefinition],
    ) -> Result<Option<GoalBuiltinBinding>> {
        let control = self.goal_action_state(context)?;
        let id = self.u32(context, 0xa0)?;
        let known = definitions
            .binary_search_by_key(&(id as i32), |row| row.id)
            .ok()
            .map(|index| definitions[index]);
        let result = if let Some(definition) = known {
            let implementation = self.u64(context, 0xa8)?;
            let copied = if implementation == 0 {
                None
            } else {
                self.check_type(implementation, &[definition.primary_vtable_rva])?;
                let address = implementation
                    .checked_add(0xf8)
                    .ok_or(Error::AddressOverflow)?;
                let mut words = [0; 18];
                self.memory.read_exact(address, &mut words)?;
                if u32::from_le_bytes(words[..4].try_into().expect("word")) != id {
                    return Err(Error::InvalidProfile(
                        "NR builtin Goal definition ID differs",
                    ));
                }
                let mut repeated = [0; 18];
                self.memory.read_exact(address, &mut repeated)?;
                if repeated != words {
                    return Err(Error::InvalidProfile(
                        "NR builtin Goal common words changed",
                    ));
                }
                self.check_type(implementation, &[definition.primary_vtable_rva])?;
                Some(BuiltinGoalImplementation {
                    address: implementation,
                    raw_word_fc: u32::from_le_bytes(words[4..8].try_into().expect("word")),
                    raw_float_100_bits: u32::from_le_bytes(words[8..12].try_into().expect("word")),
                    raw_float_104_bits: u32::from_le_bytes(words[12..16].try_into().expect("word")),
                    raw_word_108: u16::from_le_bytes(words[16..18].try_into().expect("word")),
                })
            };
            self.unchanged_u64(context, 0xa8, implementation)?;
            Some(GoalBuiltinBinding {
                control,
                definition,
                implementation: copied,
            })
        } else {
            None
        };
        if self.u32(context, 0xa0)? != id || self.goal_action_state(context)? != control {
            return Err(Error::InvalidProfile(
                "NR Goal definition binding changed during copy",
            ));
        }
        Ok(result)
    }
}
