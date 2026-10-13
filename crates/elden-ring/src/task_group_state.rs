//! Copied scheduler enable bytes keyed by assigned full group ID, not registration index.
use crate::{GroupObservation, TaskCatalog, TaskPhase};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const GROUP_STATE_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0xeb37e0,
        length: 0x23f,
    },
    CodeRange {
        rva: 0x268f5a0,
        length: 0x6e,
    },
    CodeRange {
        rva: 0x26a38b0,
        length: 0x8a,
    },
    CodeRange {
        rva: 0x26a3990,
        length: 0x15d,
    },
    CodeRange {
        rva: 0x26a3c90,
        length: 0x27,
    },
    CodeRange {
        rva: 0x26a3cc0,
        length: 0x3e,
    },
    CodeRange {
        rva: 0x26a3d10,
        length: 0x75,
    },
    CodeRange {
        rva: 0x26a3dd0,
        length: 0x70,
    },
];
const MANAGER_GLOBAL: u32 = 0x458fe88;
const MANAGER_TABLE: u32 = 0x2c04b00;
const SCHEDULER_TABLE: u32 = 0x329caf0;
const SCHEDULER_OFFSET: u32 = 8;
const RECORD_START: u32 = 0x18;
const RECORD_END: u32 = 0x20;
const RECORD_SIZE: u32 = 0x88;
const ENABLE_OFFSET: u32 = 0x84;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseGroupStateObservation {
    pub group: GroupObservation,
    pub manager_address: u64,
    pub scheduler_address: u64,
    pub records_start: u64,
    pub records_end: u64,
    pub record_address: u64,
    pub native_group_id: u32,
    /// Exact byte written by the native ID-based enable setter. Unknown nonzero values
    /// remain raw. This does not acknowledge execution, worker drain or a safe phase.
    pub native_enabled: u8,
}
impl PhaseGroupStateObservation {
    pub fn enabled(&self) -> bool {
        self.native_enabled != 0
    }
}
fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}
pub(crate) fn validate_record_limit(limit: usize) -> Result<()> {
    if !(1..=65_536).contains(&limit) {
        return Err(Error::InvalidProfile("invalid scheduler record limit"));
    }
    Ok(())
}
impl<M: AddressSpace> TaskCatalog<'_, '_, M> {
    /// Copies the first matching record's enable byte under the native assigned full ID.
    /// Missing manager, phase, assigned ID, scheduler or record remains None. No names,
    /// padding, capacity, callbacks or game functions are read/called. Two consumed-prefix
    /// copies catch visible changes, without pinning storage or guaranteeing atomicity.
    pub fn phase_group_state(
        &self,
        phase: TaskPhase,
        record_limit: usize,
    ) -> Result<Option<PhaseGroupStateObservation>> {
        validate_record_limit(record_limit)?;
        let first = self.copy_group_state(phase, record_limit)?;
        if first != self.copy_group_state(phase, record_limit)? {
            return Err(Error::InvalidProfile(
                "scheduler phase state changed during observation",
            ));
        }
        Ok(first.map(|x| x.0))
    }
    fn copy_group_state(
        &self,
        phase: TaskPhase,
        limit: usize,
    ) -> Result<Option<(PhaseGroupStateObservation, Vec<u32>)>> {
        let Some(group) = self.observe_phase(phase)? else {
            return Ok(None);
        };
        let Some(id) = group.native_group_id() else {
            return Ok(None);
        };
        let Some(manager) = self.module.global_pointer(MANAGER_GLOBAL)? else {
            return Ok(None);
        };
        self.check_vtable(manager, MANAGER_TABLE)?;
        let memory = self.module.memory();
        let scheduler = memory.read_u64(at(manager, SCHEDULER_OFFSET)?)?;
        if scheduler == 0 {
            return Ok(None);
        }
        if !scheduler.is_multiple_of(8) {
            return Err(Error::InvalidProfile("unaligned scheduler"));
        }
        self.check_vtable(scheduler, SCHEDULER_TABLE)?;
        let start = memory.read_u64(at(scheduler, RECORD_START)?)?;
        let end = memory.read_u64(at(scheduler, RECORD_END)?)?;
        let extent = end
            .checked_sub(start)
            .ok_or(Error::InvalidProfile("reversed scheduler records"))?;
        if !extent.is_multiple_of(u64::from(RECORD_SIZE))
            || !start.is_multiple_of(8)
            || (start == 0 && end != 0)
            || extent / u64::from(RECORD_SIZE) > limit as u64
        {
            return Err(Error::InvalidProfile("invalid bounded scheduler records"));
        }
        let mut ids = Vec::new();
        let mut found = None;
        for index in 0..extent / u64::from(RECORD_SIZE) {
            let record = start
                .checked_add(index * u64::from(RECORD_SIZE))
                .ok_or(Error::AddressOverflow)?;
            let key = memory.read_u32(record)?;
            ids.push(key);
            if key == id {
                let mut byte = [0];
                memory.read_exact(at(record, ENABLE_OFFSET)?, &mut byte)?;
                found = Some(PhaseGroupStateObservation {
                    group,
                    manager_address: manager,
                    scheduler_address: scheduler,
                    records_start: start,
                    records_end: end,
                    record_address: record,
                    native_group_id: id,
                    native_enabled: byte[0],
                });
                break;
            }
        }
        if self.observe_phase(phase)? != Some(group)
            || self.module.global_pointer(MANAGER_GLOBAL)? != Some(manager)
            || memory.read_u64(at(manager, SCHEDULER_OFFSET)?)? != scheduler
            || memory.read_u64(at(scheduler, RECORD_START)?)? != start
            || memory.read_u64(at(scheduler, RECORD_END)?)? != end
        {
            return Err(Error::InvalidProfile("scheduler phase graph changed"));
        }
        self.check_vtable(manager, MANAGER_TABLE)?;
        self.check_vtable(scheduler, SCHEDULER_TABLE)?;
        Ok(found.map(|x| (x, ids)))
    }
}
