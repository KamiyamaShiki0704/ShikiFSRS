//! Experimental copied summon announcement markers. Strings and list ownership stay native.
use crate::objects::{GameObjectCatalog, ObjectKind, ObjectObservation};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const SUMMON_HUD_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x844aa0,
        length: 0x47,
    },
    CodeRange {
        rva: 0x8449d0,
        length: 0x8f,
    },
    CodeRange {
        rva: 0x8447c0,
        length: 0x4b,
    },
    CodeRange {
        rva: 0x8448c0,
        length: 0xe3,
    },
    CodeRange {
        rva: 0x844f60,
        length: 0x18e,
    },
    CodeRange {
        rva: 0x844db0,
        length: 0x23,
    },
];
const FRONTEND_GLOBAL: u32 = 0x3d6f8f0;
const QUEUE_OFFSET: u32 = 0x62b0;
const CURRENT_OFFSET: u32 = 0x8;
const LIST_OFFSET: u32 = 0x58;
const DATA_OFFSET: u32 = 0x8;
const NODE_STRIDE: u32 = 0x68;
const HEAD_OFFSET: u32 = 0x1b8;
const TYPE_OFFSET: u32 = 0x8;
const FLAG_OFFSET: u32 = 0xa;
const TAIL_FLAG_OFFSET: u32 = 0x48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummonMessageMarkers {
    pub message: ObjectObservation,
    /// Signed native kind; negative is the native reset/admission sentinel.
    pub native_kind: i16,
    pub raw_flag: u8,
    pub raw_tail_flag: u8,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummonHudObservation {
    pub module_base: u64,
    /// Caller-known lifetime generation, not an allocation reference or lock.
    pub epoch: u64,
    pub frontend: ObjectObservation,
    pub queue: ObjectObservation,
    pub list: ObjectObservation,
    /// Current, four fixed data slots, then the embedded head data.
    pub messages: [SummonMessageMarkers; 6],
}
fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copies six embedded marker sets twice with exact types and singleton rechecks.
    /// Does not follow list links, read strings or call the native resource-owning reset.
    pub fn summon_hud(&self, epoch: u64) -> Result<Option<SummonHudObservation>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero summon HUD lifetime epoch"));
        }
        let Some(frontend) = self.singleton(FRONTEND_GLOBAL, ObjectKind::Frontend)? else {
            if self
                .singleton(FRONTEND_GLOBAL, ObjectKind::Frontend)?
                .is_some()
            {
                return Err(Error::InvalidProfile(
                    "frontend appeared during summon HUD copy",
                ));
            }
            return Ok(None);
        };
        let copy = || -> Result<SummonHudObservation> {
            let queue = self
                .require(
                    at(frontend.address, QUEUE_OFFSET)?,
                    ObjectKind::SummonMessageQueue,
                )?
                .ok_or(Error::InvalidProfile(
                    "missing embedded summon message queue",
                ))?;
            let list = self
                .require(
                    at(queue.address, LIST_OFFSET)?,
                    ObjectKind::SummonMessageList,
                )?
                .ok_or(Error::InvalidProfile(
                    "missing embedded summon message list",
                ))?;
            let addresses = [
                at(queue.address, CURRENT_OFFSET)?,
                at(list.address, DATA_OFFSET)?,
                at(list.address, DATA_OFFSET + NODE_STRIDE)?,
                at(list.address, DATA_OFFSET + 2 * NODE_STRIDE)?,
                at(list.address, DATA_OFFSET + 3 * NODE_STRIDE)?,
                at(list.address, HEAD_OFFSET)?,
            ];
            let mut markers = Vec::with_capacity(6);
            for address in addresses {
                let message = self
                    .require(address, ObjectKind::SummonMessage)?
                    .ok_or(Error::InvalidProfile("missing embedded summon message"))?;
                let memory = self.module.memory();
                let mut kind = [0; 2];
                memory.read_exact(at(address, TYPE_OFFSET)?, &mut kind)?;
                markers.push(SummonMessageMarkers {
                    message,
                    native_kind: i16::from_le_bytes(kind),
                    raw_flag: memory.read_u8(at(address, FLAG_OFFSET)?)?,
                    raw_tail_flag: memory.read_u8(at(address, TAIL_FLAG_OFFSET)?)?,
                });
            }
            Ok(SummonHudObservation {
                module_base: self.module.base(),
                epoch,
                frontend,
                queue,
                list,
                messages: markers
                    .try_into()
                    .map_err(|_| Error::InvalidProfile("summon HUD slot count"))?,
            })
        };
        let first = copy()?;
        if copy()? != first
            || self.singleton(FRONTEND_GLOBAL, ObjectKind::Frontend)? != Some(frontend)
        {
            return Err(Error::InvalidProfile(
                "summon HUD source changed during copy",
            ));
        }
        Ok(Some(first))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummonMarkerStore {
    pub address: usize,
    pub before: Vec<u8>,
    pub written: Vec<u8>,
}
#[derive(Debug)]
pub struct SummonHudWrite {
    pub source: SummonHudObservation,
    /// Actual stores, retained even if postflight fails. No retry or rollback.
    pub stores: Vec<SummonMarkerStore>,
    pub postflight: Result<SummonHudObservation>,
}
pub(crate) struct SuppressionPlan {
    source: SummonHudObservation,
    expected: SummonHudObservation,
    stores: Vec<SummonMarkerStore>,
}
pub(crate) fn suppression_plan(
    current: Option<SummonHudObservation>,
    source: SummonHudObservation,
) -> Result<SuppressionPlan> {
    if source.epoch == 0 || current != Some(source) {
        return Err(Error::InvalidProfile(
            "summon HUD suppression source superseded",
        ));
    }
    let mut expected = source;
    let mut stores = Vec::with_capacity(18);
    for (before, after) in source.messages.iter().zip(&mut expected.messages) {
        for (offset, old, new) in [
            (
                TYPE_OFFSET,
                before.native_kind.to_le_bytes().to_vec(),
                (-1i16).to_le_bytes().to_vec(),
            ),
            (FLAG_OFFSET, vec![before.raw_flag], vec![0]),
            (TAIL_FLAG_OFFSET, vec![before.raw_tail_flag], vec![0]),
        ] {
            if old == new {
                continue;
            }
            let address = usize::try_from(at(before.message.address, offset)?)
                .map_err(|_| Error::AddressOverflow)?;
            address
                .checked_add(new.len())
                .ok_or(Error::AddressOverflow)?;
            if address == 0 || (new.len() == 2 && !address.is_multiple_of(2)) {
                return Err(Error::InvalidProfile("invalid summon HUD store extent"));
            }
            stores.push(SummonMarkerStore {
                address,
                before: old,
                written: new,
            });
        }
        after.native_kind = -1;
        after.raw_flag = 0;
        after.raw_tail_flag = 0;
    }
    Ok(SuppressionPlan {
        source,
        expected,
        stores,
    })
}
pub(crate) fn commit_suppression(
    plan: SuppressionPlan,
    mut store: impl FnMut(usize, &[u8]),
    observe: impl FnOnce() -> Result<Option<SummonHudObservation>>,
) -> SummonHudWrite {
    for item in &plan.stores {
        store(item.address, &item.written);
    }
    let postflight = observe().and_then(|value| {
        if value == Some(plan.expected) {
            Ok(plan.expected)
        } else {
            Err(Error::InvalidProfile(
                "summon HUD changed after marker stores",
            ))
        }
    });
    SummonHudWrite {
        source: plan.source,
        stores: plan.stores,
        postflight,
    }
}
