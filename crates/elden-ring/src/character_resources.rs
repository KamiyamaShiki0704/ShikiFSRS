//! Independent ChrRes and registered entry observations. No asset-pointer dereference or lease.
use crate::{
    character_sets::{CharacterIdentity, WorldCollectionSelection, WorldMemberStateObservation},
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const RESOURCE_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3e6e30,
        length: 0x99f,
    },
    CodeRange {
        rva: 0x48c870,
        length: 0x141,
    },
    CodeRange {
        rva: 0x95380,
        length: 0xfd,
    },
    CodeRange {
        rva: 0x3f8770,
        length: 0x7ec,
    },
    CodeRange {
        rva: 0x48d0d0,
        length: 0x243,
    },
    CodeRange {
        rva: 0x48e8a0,
        length: 0x2b,
    },
    CodeRange {
        rva: 0x48e170,
        length: 0x361,
    },
    CodeRange {
        rva: 0x48e710,
        length: 0x169,
    },
    CodeRange {
        rva: 0x48e100,
        length: 0x63,
    },
    CodeRange {
        rva: 0x48e050,
        length: 0xad,
    },
    CodeRange {
        rva: 0x48dfc0,
        length: 0x90,
    },
    CodeRange {
        rva: 0x48e880,
        length: 0x1b,
    },
    CodeRange {
        rva: 0x48e4e0,
        length: 0x224,
    },
    CodeRange {
        rva: 0x48e950,
        length: 0x8a,
    },
    CodeRange {
        rva: 0x48d8e0,
        length: 0x109,
    },
    CodeRange {
        rva: 0x48d3e0,
        length: 0x4,
    },
    CodeRange {
        rva: 0x3f4830,
        length: 0x9,
    },
    CodeRange {
        rva: 0x48d6b0,
        length: 0x32,
    },
];
const RESOURCE_LINK_OFFSET: u32 = 0x28;
const CURRENT_STEP_OFFSET: u32 = 0x40;
const NEXT_STEP_OFFSET: u32 = 0x44;
const ADVANCE_OFFSET: u32 = 0x48;
const REQUEST_FLAGS_OFFSET: u32 = 0x118;
const RESOURCE_POINTERS_OFFSET: u32 = 0x130;
const ENTRY_REQUEST_OFFSET: u32 = 9;
const BACKREAD_MARK_OFFSET: u32 = 0x1c8;

/// Exact entries established by the native descriptor initializer, rather than an inferred load enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterResourceStep {
    NoData,
    LoadInitialize,
    LoadWait,
    LoadFinish,
    HighTextureLoadWait,
    HighTextureLoadFinish,
    ModelRelease,
    LoadRelease,
    /// Preserve the -1 sentinel and currently undescribed step values.
    Other(i32),
}
impl CharacterResourceStep {
    pub fn from_raw(value: i32) -> Self {
        match value {
            0 => Self::NoData,
            1 => Self::LoadInitialize,
            2 => Self::LoadWait,
            3 => Self::LoadFinish,
            4 => Self::HighTextureLoadWait,
            5 => Self::HighTextureLoadFinish,
            6 => Self::ModelRelease,
            7 => Self::LoadRelease,
            other => Self::Other(other),
        }
    }
}
/// Values consumed by native 3f8770. A request is not completed backread/activation or teardown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CharacterBackreadRequest {
    Unload = 0,
    Resources = 2,
    Present = 4,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterResourcesObservation {
    pub member: WorldMemberStateObservation,
    pub resource_address: u64,
    pub current_step: i32,
    pub next_step: i32,
    pub advance: u8,
    /// +118..+11e: load, high-texture, acquired-high-texture, ancillary,
    /// variant, distance request, acquired-distance request. Raw bytes are retained.
    pub request_flags: [u8; 7],
    /// +130/+138/+140/+148 are opaque acquired references. No type/readiness claim or dereference.
    pub acquired_references: [u64; 4],
    pub backread_mark: u8,
}
impl CharacterResourcesObservation {
    pub fn step(self) -> CharacterResourceStep {
        CharacterResourceStep::from_raw(self.current_step)
    }
    /// Exact base ChrIns virtual4f8 storage predicate, not whole resource-graph readiness.
    pub fn native_resource_step_ready(self) -> bool {
        (3..6).contains(&self.current_step)
    }
    /// Combined copied marks for waiting after creation. They provide no native lifetime,
    /// callback drain or completed engine graph guarantee; caller supplies those independently.
    pub fn native_presentation_marks(self) -> bool {
        self.native_resource_step_ready()
            && self.member.native_state_bytes[0] == 4
            && self.backread_mark & 0x10 != 0
    }
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    pub fn character_resources(
        &self,
        target: CharacterIdentity,
        selection: WorldCollectionSelection,
        limit: usize,
    ) -> Result<Option<CharacterResourcesObservation>> {
        let Some(member) = self.world_member_state(target, selection, limit)? else {
            return Ok(None);
        };
        let link = at(target.address, RESOURCE_LINK_OFFSET)?;
        let resource_address = self.module.memory().read_u64(link)?;
        let Some(resource) = self.require(resource_address, ObjectKind::CharacterResources)? else {
            if self.module.memory().read_u64(link)? != 0
                || self.world_member_state(target, selection, limit)? != Some(member)
            {
                return Err(Error::InvalidProfile(
                    "absent character resource link changed",
                ));
            }
            return Ok(None);
        };
        let read = || -> Result<CharacterResourcesObservation> {
            let memory = self.module.memory();
            let mut request_flags = [0; 7];
            memory.read_exact(
                at(resource_address, REQUEST_FLAGS_OFFSET)?,
                &mut request_flags,
            )?;
            let mut acquired_references = [0; 4];
            for (index, pointer) in acquired_references.iter_mut().enumerate() {
                *pointer = memory.read_u64(at(
                    resource_address,
                    RESOURCE_POINTERS_OFFSET + index as u32 * 8,
                )?)?;
            }
            Ok(CharacterResourcesObservation {
                member,
                resource_address,
                current_step: memory.read_u32(at(resource_address, CURRENT_STEP_OFFSET)?)? as i32,
                next_step: memory.read_u32(at(resource_address, NEXT_STEP_OFFSET)?)? as i32,
                advance: memory.read_u8(at(resource_address, ADVANCE_OFFSET)?)?,
                request_flags,
                acquired_references,
                backread_mark: memory.read_u8(at(target.address, BACKREAD_MARK_OFFSET)?)?,
            })
        };
        let copy = read()?;
        if read()? != copy
            || self.module.memory().read_u64(link)? != resource_address
            || self.require(resource_address, ObjectKind::CharacterResources)? != Some(resource)
            || self.world_member_state(target, selection, limit)? != Some(member)
        {
            return Err(Error::InvalidProfile(
                "character resource graph changed during observation",
            ));
        }
        Ok(Some(copy))
    }
}
fn at(base: u64, offset: u32) -> Result<u64> {
    base.checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}
#[derive(Debug)]
pub struct CharacterBackreadWrite {
    pub source: WorldMemberStateObservation,
    pub request: CharacterBackreadRequest,
    pub changed: bool,
    /// Retains the actual store even when the postflight rejects subsequent changes.
    pub postflight: Result<WorldMemberStateObservation>,
}
pub(crate) struct RequestPlan {
    address: usize,
    source: WorldMemberStateObservation,
    after: WorldMemberStateObservation,
    request: CharacterBackreadRequest,
}
pub(crate) fn request_plan(
    current: Option<WorldMemberStateObservation>,
    source: WorldMemberStateObservation,
    request: CharacterBackreadRequest,
    primary: Option<CharacterIdentity>,
) -> Result<RequestPlan> {
    if source.registration.character.epoch == 0
        || current != Some(source)
        || primary.is_none_or(|value| {
            value.world_address != source.registration.character.world_address
                || value.epoch != source.registration.character.epoch
        })
        || primary == Some(source.registration.character)
        || source.native_state_bytes[0] > 5
        || !matches!(source.native_state_bytes[1], 0 | 2 | 4)
    {
        return Err(Error::NativeOperationRejected(
            "foreign or unadmitted backread request",
        ));
    }
    let address = usize::try_from(at(source.registration.entry_address, ENTRY_REQUEST_OFFSET)?)
        .map_err(|_| Error::AddressOverflow)?;
    let mut after = source;
    after.native_state_bytes[1] = request as u8;
    Ok(RequestPlan {
        address,
        source,
        after,
        request,
    })
}
pub(crate) fn commit_request(
    plan: RequestPlan,
    write: impl FnOnce(usize, u8),
    observe: impl FnOnce() -> Result<WorldMemberStateObservation>,
) -> CharacterBackreadWrite {
    let changed = plan.after != plan.source;
    let postflight = if changed {
        write(plan.address, plan.request as u8);
        observe().and_then(|after| {
            if after != plan.after {
                Err(Error::InvalidProfile("backread request postflight changed"))
            } else {
                Ok(after)
            }
        })
    } else {
        Ok(plan.source)
    };
    CharacterBackreadWrite {
        source: plan.source,
        request: plan.request,
        changed,
        postflight,
    }
}
