//! Selected native distance/update words, copied without evaluating native policy.
use crate::{character_sets::CharacterIdentity, objects::GameObjectCatalog};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
pub(crate) const DISTANCE_WORD_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3e6e30,
        length: 0x99f,
    },
    CodeRange {
        rva: 0x3fbe70,
        length: 0x623,
    },
    CodeRange {
        rva: 0x3f26a0,
        length: 0x439,
    },
];
const WORD3FC_OFFSET: u32 = 0x3fc;
const WORD400_OFFSET: u32 = 0x400;
const WORD408_OFFSET: u32 = 0x408;
const WORD40C_OFFSET: u32 = 0x40c;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterDistanceWordsObservation {
    pub character: CharacterIdentity,
    /// Raw f32 bits at 3fc,400,408,40c in that order, including unknown/nonfinite values.
    /// 3fbe70 writes position-distance caches and an adjusted update threshold; 3f26a0
    /// initializes408 through a native map query. These are neither fresh measurements
    /// nor a complete activation/render decision. Caller owns diagnostic interpretation.
    pub words: [u32; 4],
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy exactly four initialized words. No neighboring404/410 read, native function,
    /// epoch inference, lifetime lease or promise that the engine updated this frame.
    pub fn character_distance_words(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterDistanceWordsObservation>> {
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale character distance identity"));
        }
        let object = self
            .character(target.address)?
            .ok_or(Error::InvalidProfile("distance character disappeared"))?;
        let read = || -> Result<[u32; 4]> {
            let mut values = [0; 4];
            for (value, offset) in values.iter_mut().zip([
                WORD3FC_OFFSET,
                WORD400_OFFSET,
                WORD408_OFFSET,
                WORD40C_OFFSET,
            ]) {
                *value = self.module.memory().read_u32(
                    target
                        .address
                        .checked_add(u64::from(offset))
                        .ok_or(Error::AddressOverflow)?,
                )?;
            }
            Ok(values)
        };
        let words = read()?;
        if read()? != words
            || self.character(target.address)? != Some(object)
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile(
                "character distance words changed during observation",
            ));
        }
        Ok(Some(CharacterDistanceWordsObservation {
            character: target,
            words,
        }))
    }
}
