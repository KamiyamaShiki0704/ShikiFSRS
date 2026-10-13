//! Read-only repository slots and native PARAM rows. Named gameplay schemas are separate.
use crate::Ww171File;
use crate::objects::{GameObjectCatalog, ObjectKind};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, pe::FileImage, profile::CodeRange};
use std::collections::BTreeSet;

pub(crate) const PARAMETER_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0xd4ea00,
        length: 0x2f,
    },
    CodeRange {
        rva: 0xd4ea30,
        length: 0x63,
    },
    CodeRange {
        rva: 0xd40190,
        length: 0x168,
    },
    CodeRange {
        rva: 0xd464e0,
        length: 0x1b6,
    },
];

pub const PARAMETER_SLOT_COUNT: u16 = 194;
const PARAMETER_DEFINITIONS: u32 = 0x3b405f0;
const NAMED_PARAMETER_COUNT: usize = 193;
const MAX_VARIANTS: u32 = 8;
const SLOT_START: u64 = 0x80;
const SLOT_STRIDE: u64 = 72;
const MAX_RELATIVE_SPAN: u64 = 64 * 1024 * 1024;
const MAX_ROW_COPY: usize = 65_536;

/// Native table slot, sealed to the independently proven repository array extent.
/// This number is not a stable name across other game versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterSlot(u16);

impl ParameterSlot {
    pub fn new(index: u16) -> Result<Self> {
        if index >= PARAMETER_SLOT_COUNT {
            return Err(Error::InvalidProfile("parameter slot is out of range"));
        }
        Ok(Self(index))
    }

    pub fn index(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterDefinition {
    pub slot: ParameterSlot,
    pub name: String,
}

impl Ww171File<'_> {
    /// Read the verified executable file's name-to-slot table; no process reads.
    /// Slot 193 is the native unknown-name result and has no name in this table.
    pub fn parameter_definitions(&self) -> Result<Vec<ParameterDefinition>> {
        definitions(
            self.file().image(),
            PARAMETER_DEFINITIONS,
            NAMED_PARAMETER_COUNT,
        )
    }

    pub fn parameter_slot(&self, name: &str) -> Result<Option<ParameterSlot>> {
        Ok(self
            .parameter_definitions()?
            .into_iter()
            .find(|item| item.name == name)
            .map(|item| item.slot))
    }
}

pub(crate) fn definitions(
    image: &FileImage<'_>,
    table: u32,
    count: usize,
) -> Result<Vec<ParameterDefinition>> {
    if !(1..=NAMED_PARAMETER_COUNT).contains(&count) {
        return Err(Error::InvalidProfile("invalid parameter definition count"));
    }
    let extent = (count as u32 + 1) * 16;
    let section = image
        .section_at(table)
        .filter(|s| !s.is_executable())
        .ok_or(Error::InvalidProfile(
            "parameter definitions are not file data",
        ))?;
    if table as u64 + extent as u64 > section.rva as u64 + section.raw_size as u64 {
        return Err(Error::InvalidProfile(
            "parameter definitions cross file-backed data",
        ));
    }
    let records = image.read_rva(table, extent as usize)?;
    let mut names = BTreeSet::new();
    let mut slots = BTreeSet::new();
    let mut result = Vec::with_capacity(count);
    for record in records[..count * 16].chunks_exact(16) {
        let pointer = u64::from_le_bytes(record[..8].try_into().expect("record width"));
        let index = u32::from_le_bytes(record[8..12].try_into().expect("record width"));
        if index >= NAMED_PARAMETER_COUNT as u32 || !slots.insert(index) {
            return Err(Error::InvalidProfile(
                "invalid or duplicate named parameter slot",
            ));
        }
        let rva = pointer
            .checked_sub(image.preferred_base)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or(Error::InvalidProfile(
                "parameter name is outside the file image",
            ))?;
        if !rva.is_multiple_of(2) {
            return Err(Error::InvalidProfile("unaligned parameter name"));
        }
        let section = image
            .section_at(rva)
            .filter(|s| !s.is_executable() && !s.is_writable())
            .ok_or(Error::InvalidProfile(
                "parameter name is not readonly file data",
            ))?;
        let mut units = Vec::new();
        for i in 0..256u32 {
            let at = rva.checked_add(i * 2).ok_or(Error::AddressOverflow)?;
            if at as u64 + 2 > section.rva as u64 + section.raw_size as u64 {
                return Err(Error::InvalidProfile("parameter name crosses file data"));
            }
            let unit = u16::from_le_bytes(image.read_rva(at, 2)?.try_into().expect("unit width"));
            if unit == 0 {
                break;
            }
            units.push(unit);
        }
        if units.len() == 256 {
            return Err(Error::InvalidProfile("unterminated parameter name"));
        }
        let name = String::from_utf16(&units)
            .map_err(|_| Error::InvalidProfile("invalid parameter name encoding"))?;
        if name.is_empty() || !names.insert(name.clone()) {
            return Err(Error::InvalidProfile("empty or duplicate parameter name"));
        }
        result.push(ParameterDefinition {
            slot: ParameterSlot(index as u16),
            name,
        });
    }
    if records[count * 16..count * 16 + 8] != [0; 8] {
        return Err(Error::InvalidProfile(
            "parameter name table has no expected sentinel",
        ));
    }
    Ok(result)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterRowLayout {
    Short32,
    Extended32,
    Extended64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterRow {
    pub id: u32,
    pub ordinal: u16,
    pub address: u64,
}

/// Copied metadata only. No row lifetime, allocation size, or field schema is implied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterRows {
    pub resource_address: u64,
    pub header_address: u64,
    pub native_index_address: u64,
    pub layout: ParameterRowLayout,
    /// Sorted by native row id, retaining each physical ordinal.
    pub rows: Vec<ParameterRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterRowCopy {
    pub row: ParameterRow,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Link {
    repository: u64,
    slot: u64,
    variants: u32,
    resource: u64,
    argument: u64,
    header: u64,
}

fn at(address: u64, offset: u64) -> Result<u64> {
    address.checked_add(offset).ok_or(Error::AddressOverflow)
}

fn aligned(pointer: u64) -> Result<u64> {
    if pointer == 0 || !pointer.is_multiple_of(8) {
        return Err(Error::InvalidProfile("parameter link is null or unaligned"));
    }
    Ok(pointer)
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    fn parameter_link(&self, slot: ParameterSlot, variant: u32) -> Result<Option<Link>> {
        if variant >= MAX_VARIANTS {
            return Err(Error::InvalidProfile("parameter variant is out of range"));
        }
        let Some(repository) = self.parameter_repository()? else {
            return Ok(None);
        };
        let address = at(repository.address, SLOT_START + slot.0 as u64 * SLOT_STRIDE)?;
        let memory = self.module.memory();
        let variants = memory.read_u32(address)?;
        if variants > MAX_VARIANTS {
            return Err(Error::InvalidProfile(
                "invalid native parameter variant count",
            ));
        }
        let pointer = at(address, 8 + variant as u64 * 8)?;
        let resource = if variant < variants {
            memory.read_u64(pointer)?
        } else {
            0
        };
        let result = if resource == 0 {
            None
        } else {
            aligned(resource)?;
            self.require(resource, ObjectKind::ParameterResource)?;
            let argument = aligned(memory.read_u64(at(resource, 0x80)?)?)?;
            let header = aligned(memory.read_u64(at(argument, 0x80)?)?)?;
            Some(Link {
                repository: repository.address,
                slot: address,
                variants,
                resource,
                argument,
                header,
            })
        };
        if memory.read_u32(address)? != variants
            || (variant < variants && memory.read_u64(pointer)? != resource)
            || self.parameter_repository()? != Some(repository)
        {
            return Err(Error::InvalidProfile(
                "parameter repository changed during observation",
            ));
        }
        Ok(result)
    }

    fn recheck_link(&self, slot: ParameterSlot, variant: u32, expected: Link) -> Result<()> {
        if self.parameter_link(slot, variant)? != Some(expected) {
            return Err(Error::InvalidProfile(
                "parameter links changed during observation",
            ));
        }
        Ok(())
    }

    /// Observe at most `limit` rows; zero and limits above 65535 are rejected.
    /// Unknown layouts, unsorted/duplicate ids and inconsistent row indices are rejected.
    pub fn parameter_rows(
        &self,
        slot: ParameterSlot,
        variant: u32,
        limit: usize,
    ) -> Result<Option<ParameterRows>> {
        validate_limit(limit)?;
        let Some(link) = self.parameter_link(slot, variant)? else {
            return Ok(None);
        };
        let (layout, index_address, rows) = read_rows(self.module.memory(), link.header, limit)?;
        self.recheck_link(slot, variant, link)?;
        Ok(Some(ParameterRows {
            resource_address: link.resource,
            header_address: link.header,
            native_index_address: index_address,
            layout,
            rows,
        }))
    }

    /// Copy a caller-requested prefix, up to 64 KiB, without native calls or writes.
    /// Length does not establish a field schema or the last row's allocation extent.
    /// If another row follows in memory, its boundary may not be crossed.
    pub fn copy_parameter_row(
        &self,
        slot: ParameterSlot,
        variant: u32,
        id: u32,
        length: usize,
        limit: usize,
    ) -> Result<Option<ParameterRowCopy>> {
        if id > i32::MAX as u32 || !(1..=MAX_ROW_COPY).contains(&length) {
            return Err(Error::InvalidProfile("invalid row id or prefix length"));
        }
        let Some(snapshot) = self.parameter_rows(slot, variant, limit)? else {
            return Ok(None);
        };
        let Ok(index) = snapshot.rows.binary_search_by_key(&id, |row| row.id) else {
            return Ok(None);
        };
        let row = snapshot.rows[index];
        let end = at(row.address, length as u64)?;
        if end - snapshot.header_address > MAX_RELATIVE_SPAN
            || (row.address
                < at(
                    snapshot.native_index_address,
                    snapshot.rows.len() as u64 * 8,
                )?
                && end > snapshot.native_index_address)
            || snapshot
                .rows
                .iter()
                .any(|other| other.address > row.address && other.address < end)
        {
            return Err(Error::InvalidProfile(
                "row prefix crosses the next row or read bound",
            ));
        }
        let mut bytes = vec![0; length];
        self.module.memory().read_exact(row.address, &mut bytes)?;
        if self.parameter_rows(slot, variant, limit)? != Some(snapshot) {
            return Err(Error::InvalidProfile(
                "parameter rows changed while copying data",
            ));
        }
        Ok(Some(ParameterRowCopy { row, bytes }))
    }
}

fn validate_limit(limit: usize) -> Result<()> {
    if !(1..=u16::MAX as usize).contains(&limit) {
        return Err(Error::InvalidProfile("invalid parameter observation limit"));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    count: u16,
    version: u8,
    flags: u8,
    index_offset: u32,
    index_count: u32,
}

fn header<M: AddressSpace>(memory: &M, address: u64) -> Result<Header> {
    let mut count = [0; 2];
    memory.read_exact(at(address, 0xa)?, &mut count)?;
    Ok(Header {
        count: u16::from_le_bytes(count),
        version: memory.read_u8(at(address, 0x2d)?)?,
        flags: memory.read_u8(at(address, 0x2e)?)?,
        index_offset: memory.read_u32(address.checked_sub(0x10).ok_or(Error::AddressOverflow)?)?,
        index_count: memory.read_u32(address.checked_sub(0xc).ok_or(Error::AddressOverflow)?)?,
    })
}

fn layout(header: Header) -> Result<(ParameterRowLayout, u64, u64, u64)> {
    let mode = header.version & 0x7f;
    let wide = header.flags & 2 != 0 && (mode == 4 || (mode == 5 && header.version >= 0x80));
    match mode {
        2 => Ok((ParameterRowLayout::Short32, 0x30, 12, 4)),
        3..=5 if !wide => Ok((ParameterRowLayout::Extended32, 0x40, 12, 4)),
        4..=5 => Ok((ParameterRowLayout::Extended64, 0x40, 24, 8)),
        _ => Err(Error::InvalidProfile("unsupported native PARAM layout")),
    }
}

fn read_rows<M: AddressSpace>(
    memory: &M,
    address: u64,
    limit: usize,
) -> Result<(ParameterRowLayout, u64, Vec<ParameterRow>)> {
    validate_limit(limit)?;
    let original = header(memory, address)?;
    let (layout, start, stride, data_offset) = layout(original)?;
    let count = original.count as usize;
    if count > limit || original.index_count != original.count as u32 {
        return Err(Error::InvalidProfile(
            "parameter count exceeds limit or differs from index",
        ));
    }
    let descriptor_end = start + count as u64 * stride;
    // The game's add/signed conversion must not wrap or produce a negative displacement.
    let index_relative = (original.index_offset as u64 + 15) & !15;
    if count != 0
        && (original.index_offset > i32::MAX as u32 - 15
            || index_relative < descriptor_end
            || index_relative + count as u64 * 8 > MAX_RELATIVE_SPAN)
    {
        return Err(Error::InvalidProfile(
            "invalid native parameter index extent",
        ));
    }
    let index_address = at(address, index_relative)?;
    let mut index_bytes = vec![0; count * 8];
    memory.read_exact(index_address, &mut index_bytes)?;
    let mut rows = Vec::with_capacity(count);
    let mut ordinals = BTreeSet::new();
    let mut offsets = BTreeSet::new();
    for record in index_bytes.chunks_exact(8) {
        let id = u32::from_le_bytes(record[..4].try_into().expect("record width"));
        let ordinal = u32::from_le_bytes(record[4..].try_into().expect("record width"));
        if id > i32::MAX as u32
            || rows.last().is_some_and(|row: &ParameterRow| row.id >= id)
            || ordinal >= original.count as u32
            || !ordinals.insert(ordinal)
        {
            return Err(Error::InvalidProfile(
                "invalid, duplicate or unsorted parameter index",
            ));
        }
        let descriptor = at(address, start + ordinal as u64 * stride)?;
        if memory.read_u32(descriptor)? != id {
            return Err(Error::InvalidProfile(
                "parameter index disagrees with physical row id",
            ));
        }
        let offset = if layout == ParameterRowLayout::Extended64 {
            memory.read_u64(at(descriptor, data_offset)?)?
        } else {
            memory.read_u32(at(descriptor, data_offset)?)? as u64
        };
        if offset < descriptor_end
            || offset >= MAX_RELATIVE_SPAN
            || !offsets.insert(offset)
            || (offset < index_relative + count as u64 * 8 && offset >= index_relative)
        {
            return Err(Error::InvalidProfile(
                "invalid, duplicate or overlapping parameter row offset",
            ));
        }
        rows.push(ParameterRow {
            id,
            ordinal: ordinal as u16,
            address: at(address, offset)?,
        });
    }
    // Repeat every copied metadata field. This detects change, but cannot prevent ABA.
    let mut recheck = vec![0; index_bytes.len()];
    memory.read_exact(index_address, &mut recheck)?;
    if recheck != index_bytes || header(memory, address)? != original {
        return Err(Error::InvalidProfile(
            "parameter header/index changed during observation",
        ));
    }
    for row in &rows {
        let descriptor = at(address, start + row.ordinal as u64 * stride)?;
        let offset = if layout == ParameterRowLayout::Extended64 {
            memory.read_u64(at(descriptor, data_offset)?)?
        } else {
            memory.read_u32(at(descriptor, data_offset)?)? as u64
        };
        if memory.read_u32(descriptor)? != row.id || at(address, offset)? != row.address {
            return Err(Error::InvalidProfile(
                "parameter descriptor changed during observation",
            ));
        }
    }
    Ok((layout, index_address, rows))
}
