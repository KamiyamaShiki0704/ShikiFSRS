//! Copied NR runtime PARAM index and row payloads, independently evidenced at 0xe6a6a0.
//! Reads the loader's sorted id/ordinal index; no assumed ids from raw row-record padding.
use crate::{
    reader::{NightreignReader, PARAMETER_CAP_TABLE, ReaderCore},
    roots::RootKind,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterRow {
    pub id: u32,
    pub ordinal: u32,
    pub address: u64,
    pub relative_offset: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterRowsSnapshot {
    pub cap_address: u64,
    pub resource_address: u64,
    pub header_address: u64,
    pub native_row_count: u16,
    pub format_byte: u8,
    pub flags_2e: u8,
    pub rows: Vec<ParameterRow>,
}
impl ParameterRowsSnapshot {
    pub fn row(&self, id: u32) -> Option<&ParameterRow> {
        self.rows
            .binary_search_by_key(&id, |row| row.id)
            .ok()
            .map(|index| &self.rows[index])
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterRowCopy {
    pub row: ParameterRow,
    pub bytes: Vec<u8>,
}

impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    pub fn parameter_rows(
        &self,
        root: u64,
        name: &str,
        variant: usize,
        limit: usize,
    ) -> Result<ParameterRowsSnapshot> {
        let caps = self.parameter_caps(root, name)?;
        let cap = *caps
            .addresses
            .get(variant)
            .ok_or(Error::InvalidProfile("NR parameter variant is unavailable"))?;
        let rows = self.core().parameter_rows(cap, limit)?;
        if self.parameter_caps(root, name)? != caps {
            return Err(Error::InvalidProfile("NR parameter slot changed"));
        }
        Ok(rows)
    }
    /// Copies exactly the requested length, not a guessed schema size. No writes or native calls.
    pub fn copy_parameter_row(
        &self,
        root: u64,
        name: &str,
        variant: usize,
        id: u32,
        length: usize,
        limit: usize,
    ) -> Result<Option<ParameterRowCopy>> {
        if length == 0 || length > 65536 {
            return Err(Error::InvalidProfile("NR row copy must be 1..=65536 bytes"));
        }
        let rows = self.parameter_rows(root, name, variant, limit)?;
        let Some(&row) = rows.row(id) else {
            return Ok(None);
        };
        if row
            .relative_offset
            .checked_add(length as u64)
            .ok_or(Error::AddressOverflow)?
            > 64 * 1024 * 1024
        {
            return Err(Error::InvalidProfile(
                "NR row copy exceeds relative span limit",
            ));
        }
        let mut bytes = vec![0; length];
        self.module.memory().read_exact(row.address, &mut bytes)?;
        if self.parameter_rows(root, name, variant, limit)? != rows {
            return Err(Error::InvalidProfile(
                "NR PARAM index changed during row copy",
            ));
        }
        Ok(Some(ParameterRowCopy { row, bytes }))
    }
    pub fn current_parameter_rows(
        &self,
        name: &str,
        variant: usize,
        limit: usize,
    ) -> Result<Option<ParameterRowsSnapshot>> {
        self.root(RootKind::Parameters)?
            .map(|root| self.parameter_rows(root, name, variant, limit))
            .transpose()
    }
}

fn at(base: u64, offset: u64) -> Result<u64> {
    base.checked_add(offset).ok_or(Error::AddressOverflow)
}
fn pointer(value: u64) -> Result<u64> {
    if value == 0 || !value.is_multiple_of(8) {
        return Err(Error::InvalidProfile(
            "NR PARAM pointer is null or unaligned",
        ));
    }
    Ok(value)
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn parameter_rows(&self, cap: u64, limit: usize) -> Result<ParameterRowsSnapshot> {
        if !(1..=65535).contains(&limit) {
            return Err(Error::InvalidProfile(
                "NR PARAM row limit must be 1..=65535",
            ));
        }
        self.check_type(cap, &[PARAMETER_CAP_TABLE])?;
        let resource = pointer(self.u64(cap, 0x80)?)?;
        let header = pointer(self.u64(resource, 0x80)?)?;
        let mut count = [0; 2];
        self.memory.read_exact(at(header, 0xa)?, &mut count)?;
        let row_count = u16::from_le_bytes(count);
        let format = self.memory.read_u8(at(header, 0x2d)?)?;
        let flags = self.memory.read_u8(at(header, 0x2e)?)?;
        let mode = format & 0x7f;
        let wide = flags & 2 != 0 && (mode == 4 || (mode == 5 && format & 0x80 != 0));
        let (table, stride, data_field, data_width) = match mode {
            2 => (0x30u64, 12u64, 4u64, 4usize),
            3..=5 if wide => (0x40, 24, 8, 8),
            3..=5 => (0x40, 12, 4, 4),
            _ => return Err(Error::InvalidProfile("unsupported NR PARAM row format")),
        };
        let prefix = header.checked_sub(0x10).ok_or(Error::AddressOverflow)?;
        let index_offset = self.memory.read_u32(prefix)? as i32;
        let index_count = self.memory.read_u32(at(prefix, 4)?)? as i32;
        if index_offset < 0
            || index_offset as u64 > 64 * 1024 * 1024
            || index_count < 0
            || index_count as usize > limit
            || row_count as usize > limit
        {
            return Err(Error::InvalidProfile(
                "invalid NR runtime PARAM index bounds",
            ));
        }
        let aligned = (index_offset as u64 + 15) & !15;
        let index = at(header, aligned)?;
        let record_end = table + row_count as u64 * stride;
        let mut rows = Vec::with_capacity(index_count as usize);
        let mut previous = None;
        for item in 0..index_count as u64 {
            let slot = at(index, item * 8)?;
            let id = self.memory.read_u32(slot)?;
            let ordinal = self.memory.read_u32(at(slot, 4)?)? as i32;
            if previous.is_some_and(|last| id <= last)
                || ordinal < 0
                || ordinal as u32 >= row_count as u32
            {
                return Err(Error::InvalidProfile("invalid NR PARAM id/ordinal index"));
            }
            previous = Some(id);
            let field = at(header, table + ordinal as u64 * stride + data_field)?;
            let offset = if data_width == 8 {
                self.memory.read_u64(field)?
            } else {
                self.memory.read_u32(field)? as u64
            };
            if offset < record_end || offset > 64 * 1024 * 1024 {
                return Err(Error::InvalidProfile(
                    "invalid NR PARAM row relative offset",
                ));
            }
            rows.push(ParameterRow {
                id,
                ordinal: ordinal as u32,
                address: at(header, offset)?,
                relative_offset: offset,
            });
        }
        if self.memory.read_u32(prefix)? as i32 != index_offset
            || self.memory.read_u32(at(prefix, 4)?)? as i32 != index_count
        {
            return Err(Error::InvalidProfile("NR PARAM index bounds changed"));
        }
        self.unchanged_u64(cap, 0x80, resource)?;
        self.unchanged_u64(resource, 0x80, header)?;
        let mut repeated_count = [0; 2];
        self.memory
            .read_exact(at(header, 0xa)?, &mut repeated_count)?;
        if repeated_count != count
            || self.memory.read_u8(at(header, 0x2d)?)? != format
            || self.memory.read_u8(at(header, 0x2e)?)? != flags
        {
            return Err(Error::InvalidProfile(
                "NR PARAM header changed during snapshot",
            ));
        }
        self.check_type(cap, &[PARAMETER_CAP_TABLE])?;
        Ok(ParameterRowsSnapshot {
            cap_address: cap,
            resource_address: resource,
            header_address: header,
            native_row_count: row_count,
            format_byte: format,
            flags_2e: flags,
            rows,
        })
    }
}
