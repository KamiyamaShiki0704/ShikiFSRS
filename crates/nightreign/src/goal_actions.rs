//! Experimental copies of the control object embedded by the original Goal context ctor.
//! No action execution, queue publication or animation-ID mapping is implied.
use crate::reader::{NightreignReader, ReaderCore};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
pub(crate) const GOAL_FUNC_TABLE: u32 = 0x2c025f0;
pub(crate) const CONTROL_TABLE: u32 = 0x2bf9cf0;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalActionState {
    pub context_address: u64,
    pub controller_address: u64,
    /// Copied context first word. Parent type and discovery are not reconstructed.
    pub raw_parent_address: u64,
    /// The original setter installs this link and clears the raw flag; no dereference here.
    pub raw_controller_link: u64,
    pub raw_word_10: u32,
    pub raw_word_14: i32,
    /// Original virtual getter at 0x28acf0 returns these 32 bits.
    pub raw_id_bits: u32,
    /// Original virtual getter at 0x28acd0 preserves the entire byte.
    pub raw_flag: u8,
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// Takes the containing context, not its CSGoalFunc interface at +0x18.
    /// Requires the exact embedded interface/self-link and controller primary tables.
    /// Stable memory/lifetimes are the provider's contract; no native references are returned.
    pub fn goal_action_state(&self, context: u64) -> Result<GoalActionState> {
        self.core().goal_action_state(context)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn goal_action_state(&self, context: u64) -> Result<GoalActionState> {
        if context == 0 || !context.is_multiple_of(8) {
            return Err(Error::InvalidProfile("invalid NR goal context address"));
        }
        let interface = context.checked_add(0x18).ok_or(Error::AddressOverflow)?;
        let controller = context.checked_add(0x300).ok_or(Error::AddressOverflow)?;
        self.check_type(interface, &[GOAL_FUNC_TABLE])?;
        if self.u64(context, 0x20)? != context {
            return Err(Error::InvalidProfile("NR goal interface self-link differs"));
        }
        self.check_type(controller, &[CONTROL_TABLE])?;
        let parent = self.u64(context, 0)?;
        let link = self.u64(controller, 8)?;
        let address = controller.checked_add(0x10).ok_or(Error::AddressOverflow)?;
        let mut raw = [0; 13];
        self.memory.read_exact(address, &mut raw)?;
        let mut repeated = [0; 13];
        self.memory.read_exact(address, &mut repeated)?;
        if raw != repeated {
            return Err(Error::InvalidProfile(
                "NR goal action control changed during copy",
            ));
        }
        self.unchanged_u64(context, 0, parent)?;
        self.unchanged_u64(context, 0x20, context)?;
        self.unchanged_u64(controller, 8, link)?;
        self.check_type(interface, &[GOAL_FUNC_TABLE])?;
        self.check_type(controller, &[CONTROL_TABLE])?;
        Ok(GoalActionState {
            context_address: context,
            controller_address: controller,
            raw_parent_address: parent,
            raw_controller_link: link,
            raw_word_10: u32::from_le_bytes(raw[..4].try_into().expect("word")),
            raw_word_14: i32::from_le_bytes(raw[4..8].try_into().expect("word")),
            raw_id_bits: u32::from_le_bytes(raw[8..12].try_into().expect("word")),
            raw_flag: raw[12],
        })
    }
}
