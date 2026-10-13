use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
use std::collections::BTreeSet;

/// Copied native addresses, never borrowed task objects or an ownership acknowledgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskRegistration {
    pub task_address: u64,
    pub callbacker_address: Option<u64>,
    pub group_id: u32,
}

const MAX_OBSERVATION_TASKS: usize = 65_536;
const STRIDE: u64 = 32;

pub(crate) fn read<M: AddressSpace>(
    memory: &M,
    group: u64,
    group_id: u32,
    record_vtable: u64,
    limit: usize,
) -> Result<Vec<TaskRegistration>> {
    if !(1..=MAX_OBSERVATION_TASKS).contains(&limit) {
        return Err(Error::InvalidProfile("invalid task observation limit"));
    }
    let at = |offset| group.checked_add(offset).ok_or(Error::AddressOverflow);
    let header = || -> Result<[u64; 3]> {
        Ok([
            memory.read_u64(at(0x10)?)?,
            memory.read_u64(at(0x18)?)?,
            memory.read_u64(at(0x20)?)?,
        ])
    };
    let [begin, end, capacity] = header()?;
    if begin > end || end > capacity || (begin == 0 && capacity != 0) {
        return Err(Error::InvalidProfile("invalid task vector pointers"));
    }
    if !begin.is_multiple_of(8)
        || !(end - begin).is_multiple_of(STRIDE)
        || !(capacity - begin).is_multiple_of(STRIDE)
    {
        return Err(Error::InvalidProfile("misaligned task vector"));
    }
    let count = usize::try_from((end - begin) / STRIDE).map_err(|_| Error::AddressOverflow)?;
    if count > limit {
        return Err(Error::InvalidProfile(
            "task vector exceeds observation limit",
        ));
    }
    let mut tasks = BTreeSet::new();
    let mut records = Vec::with_capacity(count);
    for index in 0..count {
        let pointer = begin
            .checked_add(index as u64 * STRIDE)
            .ok_or(Error::AddressOverflow)?;
        if memory.read_u64(pointer)? != record_vtable {
            return Err(Error::InvalidProfile("wrong task registration record type"));
        }
        let field = |offset| pointer.checked_add(offset).ok_or(Error::AddressOverflow);
        let task_address = memory.read_u64(field(8)?)?;
        let callbacker = memory.read_u64(field(16)?)?;
        let record_group_id = memory.read_u32(field(24)?)?;
        if task_address == 0 || !tasks.insert(task_address) || record_group_id != group_id {
            return Err(Error::InvalidProfile("invalid task registration identity"));
        }
        // +28..+32 is padding; no native path proves it initialized.
        if memory.read_u64(pointer)? != record_vtable {
            return Err(Error::InvalidProfile(
                "task record changed during observation",
            ));
        }
        records.push(TaskRegistration {
            task_address,
            callbacker_address: (callbacker != 0).then_some(callbacker),
            group_id: record_group_id,
        });
    }
    if header()? != [begin, end, capacity] {
        return Err(Error::InvalidProfile(
            "task vector changed during observation",
        ));
    }
    Ok(records)
}
