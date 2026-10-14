//! Independently located direct singleton slots. No native singleton constructors are called.
use crate::reader::{NightreignReader, PARAMETER_TABLE, ReaderCore, TASK_TABLE, WORLD_TABLE};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, native_tree::U32PointerTree};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootKind {
    TaskGroups,
    World,
    Parameters,
    TaskSystem,
}

pub(crate) const SYSTEM_TABLE: u32 = 0x32e8478;
pub(crate) const TYPE_INFO_GLOBAL: u32 = 0x4706eb0;
pub(crate) const TYPE_INFO_TABLE: u32 = 0x32e7718;

pub(crate) fn validate(image: &shikifsrs_runtime::pe::FileImage<'_>) -> Result<()> {
    for (kind, instruction) in [
        (RootKind::TaskGroups, 0xfec599),
        (RootKind::World, 0x8251c9),
        (RootKind::Parameters, 0xe6c254),
        (RootKind::TaskSystem, 0xfec5e7),
    ] {
        let (rva, _) = kind.profile();
        let bytes = image.read_rva(instruction, 7)?;
        let displacement = i32::from_le_bytes(bytes[3..].try_into().expect("checked displacement"));
        if bytes[..3] != [0x48, 0x8b, 0x0d]
            || (instruction as i64 + 7 + displacement as i64) != rva as i64
        {
            return Err(Error::InvalidProfile(
                "NR direct root instruction differs from file",
            ));
        }
        let section = image
            .section_at(rva)
            .filter(|section| section.is_writable() && !section.is_executable())
            .ok_or(Error::InvalidProfile("NR root is not writable image data"))?;
        if !rva.is_multiple_of(8)
            || rva as u64 + 8
                > section.rva as u64 + section.virtual_size.max(section.raw_size) as u64
        {
            return Err(Error::InvalidProfile("NR root slot crosses section"));
        }
    }
    for (instruction, table) in [(0x109efb, TYPE_INFO_TABLE), (0x27d9ae9, SYSTEM_TABLE)] {
        let bytes = image.read_rva(instruction, 7)?;
        let displacement = i32::from_le_bytes(bytes[3..].try_into().expect("checked displacement"));
        if bytes[..3] != [0x48, 0x8d, 0x05]
            || instruction as i64 + 7 + displacement as i64 != table as i64
        {
            return Err(Error::InvalidProfile(
                "NR native metadata constructor differs from file",
            ));
        }
    }
    Ok(())
}

impl RootKind {
    pub(crate) fn profile(self) -> (u32, u32) {
        match self {
            Self::TaskGroups => (0x4434088, TASK_TABLE),
            Self::World => (0x3c0f0a8, WORLD_TABLE),
            Self::Parameters => (0x3c28880, PARAMETER_TABLE),
            Self::TaskSystem => (0x4707290, SYSTEM_TABLE),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskSystemSnapshot {
    pub address: u64,
    pub allocator: u64,
    pub scheduler: u64,
    pub registry: u64,
    pub dispatching: bool,
}

impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// Missing singletons return None. Unknown objects fail instead of following guessed chains.
    pub fn root(&self, kind: RootKind) -> Result<Option<u64>> {
        self.core().root(kind)
    }

    pub fn task_system(&self) -> Result<Option<TaskSystemSnapshot>> {
        self.core().task_system()
    }

    pub fn group_id(&self, index: usize) -> Result<Option<u32>> {
        self.core().group_id(index)
    }

    /// Copies only the bounded native registration list; this establishes no phase or lifetime.
    pub fn registration_inserted(
        &self,
        group_id: u32,
        task_address: u64,
        limit: usize,
    ) -> Result<bool> {
        self.core()
            .registration_inserted(group_id, task_address, limit)
    }
}

impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn root(&self, kind: RootKind) -> Result<Option<u64>> {
        let (global, table) = kind.profile();
        let slot = self
            .base
            .checked_add(global as u64)
            .ok_or(Error::AddressOverflow)?;
        let address = self.memory.read_u64(slot)?;
        if address == 0 {
            return Ok(None);
        }
        self.check_type(address, &[table])?;
        if self.memory.read_u64(slot)? != address {
            return Err(Error::InvalidProfile("NR singleton changed during lookup"));
        }
        Ok(Some(address))
    }

    pub(crate) fn task_system(&self) -> Result<Option<TaskSystemSnapshot>> {
        let Some(address) = self.root(RootKind::TaskSystem)? else {
            return Ok(None);
        };
        let allocator = self.u64(address, 8)?;
        let scheduler = self.u64(address, 0x40)?;
        let registry = self.u64(address, 0x48)?;
        for pointer in [allocator, scheduler, registry] {
            if pointer == 0 || !pointer.is_multiple_of(8) {
                return Err(Error::InvalidProfile("NR task system is incomplete"));
            }
        }
        let dispatch = self
            .memory
            .read_u8(address.checked_add(0x38).ok_or(Error::AddressOverflow)?)?;
        if dispatch > 1 {
            return Err(Error::InvalidProfile("invalid NR dispatch flag"));
        }
        for (offset, pointer) in [(8, allocator), (0x40, scheduler), (0x48, registry)] {
            self.unchanged_u64(address, offset, pointer)?;
        }
        if self
            .memory
            .read_u8(address.checked_add(0x38).ok_or(Error::AddressOverflow)?)?
            != dispatch
        {
            return Err(Error::InvalidProfile(
                "NR dispatch flag changed during snapshot",
            ));
        }
        if self.root(RootKind::TaskSystem)? != Some(address) {
            return Err(Error::InvalidProfile("NR task system changed"));
        }
        Ok(Some(TaskSystemSnapshot {
            address,
            allocator,
            scheduler,
            registry,
            dispatching: dispatch != 0,
        }))
    }

    pub(crate) fn group_id(&self, index: usize) -> Result<Option<u32>> {
        if index >= crate::TASK_DEFINITION_COUNT {
            return Err(Error::InvalidProfile("NR group index out of range"));
        }
        let Some(root) = self.root(RootKind::TaskGroups)? else {
            return Ok(None);
        };
        let offset = 8 + index as u64 * 8;
        let group = self.u64(root, offset)?;
        if group == 0 {
            return Ok(None);
        }
        self.check_type(
            group,
            &[crate::reader::GROUP_TABLE, crate::reader::TIMELINE_TABLE],
        )?;
        let id = self.u32(group, 0x54)?;
        self.unchanged_u64(root, offset, group)?;
        if self.root(RootKind::TaskGroups)? != Some(root) {
            return Err(Error::InvalidProfile("NR task root changed"));
        }
        Ok((id != 0).then_some(id))
    }

    pub(crate) fn registration_inserted(
        &self,
        group_id: u32,
        task_address: u64,
        limit: usize,
    ) -> Result<bool> {
        if group_id == 0
            || task_address == 0
            || !task_address.is_multiple_of(8)
            || !(1..=65536).contains(&limit)
        {
            return Err(Error::InvalidProfile("invalid NR registration query"));
        }
        let system = self.task_system()?.ok_or(Error::NativeOperationRejected(
            "NR task system is uninitialized",
        ))?;
        let tree = self.u64(system.registry, 0x18)?;
        let lookup = U32PointerTree::new(self.memory, tree, limit)?;
        let Some(group) = lookup.find(group_id)? else {
            return Ok(false);
        };
        if group == 0 || !group.is_multiple_of(8) {
            return Err(Error::InvalidProfile("invalid NR registry group"));
        }
        let begin = self.u64(group, 0x10)?;
        let end = self.u64(group, 0x18)?;
        let span = end
            .checked_sub(begin)
            .ok_or(Error::InvalidProfile("reversed NR registration range"))?;
        if !begin.is_multiple_of(8)
            || !span.is_multiple_of(32)
            || span / 32 > limit as u64
            || (span > 0 && begin == 0)
        {
            return Err(Error::InvalidProfile("invalid NR registration extent"));
        }
        let mut found = false;
        for offset in (0..span).step_by(32) {
            if self.u64(begin, offset + 8)? == task_address {
                found = true;
            }
        }
        self.unchanged_u64(group, 0x10, begin)?;
        self.unchanged_u64(group, 0x18, end)?;
        if lookup.find(group_id)? != Some(group) || self.task_system()? != Some(system) {
            return Err(Error::InvalidProfile("NR registration graph changed"));
        }
        Ok(found)
    }
}
