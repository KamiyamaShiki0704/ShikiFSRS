//! The first seven signed words of the exact native resistance module.
//! Individual status names, thresholds and native damage behavior are caller policy.
use crate::{
    character_modules::{CharacterModule, CharacterModuleObservation},
    character_sets::CharacterIdentity,
    objects::GameObjectCatalog,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const RESISTANCE_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x43dcf0,
        length: 0x65,
    },
    CodeRange {
        rva: 0x43dab0,
        length: 0x4d,
    },
];
const SELECTED_WORDS_OFFSET: u32 = 0x10;
const SELECTED_WORD_COUNT: u32 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterResistanceObservation {
    pub module: CharacterModuleObservation,
    /// Signed native words in source order. No clamping or float reinterpretation.
    pub words: [i32; SELECTED_WORD_COUNT as usize],
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy exactly 28 selected bytes twice and recheck the typed reciprocal module owner.
    /// Other five arrays, pointers, thresholds and padding stay unread. Full native lifetime,
    /// exclusive phase and epoch remain caller obligations; no engine function is called.
    pub fn character_resistance(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterResistanceObservation>> {
        let Some(module) = self.character_module(target, CharacterModule::Resistance)? else {
            return Ok(None);
        };
        let read = || -> Result<[i32; SELECTED_WORD_COUNT as usize]> {
            let address = module
                .address
                .checked_add(u64::from(SELECTED_WORDS_OFFSET))
                .ok_or(Error::AddressOverflow)?;
            let mut bytes = [0; SELECTED_WORD_COUNT as usize * 4];
            self.module.memory().read_exact(address, &mut bytes)?;
            Ok(std::array::from_fn(|i| {
                i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
            }))
        };
        let words = read()?;
        if read()? != words
            || self.character_module(target, CharacterModule::Resistance)? != Some(module)
        {
            return Err(Error::InvalidProfile(
                "resistance words or typed ownership changed",
            ));
        }
        Ok(Some(CharacterResistanceObservation { module, words }))
    }
}
