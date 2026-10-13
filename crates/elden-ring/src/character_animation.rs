//! Raw event-module animation requests, distinct from a playing/finished animation.
use crate::{
    character_modules::{CharacterModule, CharacterModuleObservation},
    character_sets::CharacterIdentity,
    objects::GameObjectCatalog,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const ANIMATION_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x43a8e0,
        length: 0xd4,
    },
    CodeRange {
        rva: 0x43d740,
        length: 0x4b,
    },
    CodeRange {
        rva: 0x43af20,
        length: 0x5d,
    },
    CodeRange {
        rva: 0x43ac20,
        length: 0x4a,
    },
    CodeRange {
        rva: 0x43d250,
        length: 0x5,
    },
    CodeRange {
        rva: 0x264a50,
        length: 0xa,
    },
];
const LATCHED_ID_OFFSET: u32 = 0x18;
const REQUESTED_ID_OFFSET: u32 = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterAnimationObservation {
    pub module: CharacterModuleObservation,
    pub latched_id: i32,
    pub requested_id: i32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationOperation {
    /// Exact signed native ID. -1 explicitly clears the request; callers that mean "no
    /// animation" should skip this operation, preserving another system's pending request.
    Request(i32),
    /// Native reset changes latched18 to -1 only if it equals requested20. Request20 stays.
    ClearMatchedRequest,
}
#[derive(Debug)]
pub struct NativeAnimationCall {
    pub source: CharacterAnimationObservation,
    pub operation: AnimationOperation,
    /// The native call occurred even when this fails. It is never retried or undone by Drop.
    pub postflight: Result<CharacterAnimationObservation>,
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy exactly two signed words, preserving -1 and unknown IDs. No playing/completion
    /// predicate, engine pin, lock or resource readiness follows from these request fields.
    pub fn character_animation(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterAnimationObservation>> {
        let Some(module) = self.character_module(target, CharacterModule::Event)? else {
            return Ok(None);
        };
        let read = || -> Result<[i32; 2]> {
            let word = |offset: u32| {
                self.module
                    .memory()
                    .read_u32(
                        module
                            .address
                            .checked_add(u64::from(offset))
                            .ok_or(Error::AddressOverflow)?,
                    )
                    .map(|bits| bits as i32)
            };
            Ok([word(LATCHED_ID_OFFSET)?, word(REQUESTED_ID_OFFSET)?])
        };
        let words = read()?;
        if read()? != words
            || self.character_module(target, CharacterModule::Event)? != Some(module)
        {
            return Err(Error::InvalidProfile(
                "animation request changed during observation",
            ));
        }
        Ok(Some(CharacterAnimationObservation {
            module,
            latched_id: words[0],
            requested_id: words[1],
        }))
    }
}
pub(crate) fn check_preflight(
    current: Option<CharacterAnimationObservation>,
    expected: CharacterAnimationObservation,
    epoch: u64,
) -> Result<()> {
    if epoch == 0
        || expected.module.character.epoch != epoch
        || expected.module.module != CharacterModule::Event
        || expected.module.address == 0
        || !expected.module.address.is_multiple_of(8)
        || expected.module.container_address == 0
        || !expected.module.container_address.is_multiple_of(8)
    {
        return Err(Error::InvalidProfile("invalid native animation source"));
    }
    expected
        .module
        .address
        .checked_add(u64::from(REQUESTED_ID_OFFSET) + 4)
        .ok_or(Error::AddressOverflow)?;
    if current != Some(expected) {
        return Err(Error::NativeOperationRejected(
            "native animation source superseded",
        ));
    }
    Ok(())
}
pub(crate) fn check_postflight(
    source: CharacterAnimationObservation,
    operation: AnimationOperation,
    after: CharacterAnimationObservation,
) -> Result<CharacterAnimationObservation> {
    let valid = after.module == source.module
        && match operation {
            AnimationOperation::Request(id) => {
                after.requested_id == id
                    && (after.latched_id == source.latched_id || id != -1 && after.latched_id == id)
            }
            AnimationOperation::ClearMatchedRequest => {
                after.requested_id == source.requested_id
                    && after.latched_id
                        == if source.latched_id == source.requested_id {
                            -1
                        } else {
                            source.latched_id
                        }
            }
        };
    if !valid {
        return Err(Error::InvalidProfile("native animation postflight changed"));
    }
    Ok(after)
}
