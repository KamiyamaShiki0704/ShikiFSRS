//! PARAM header compatibility checks, distinct from in-game schema acceptance.
use crate::{
    parameter_fields::FieldDefinition,
    parameter_rows::ParameterRowsSnapshot,
    reader::{NightreignReader, ReaderCore},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowLayout {
    UniformStride(usize),
    InsufficientRows,
    IncompleteOrdinalIndex,
    Irregular,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterSchemaSnapshot {
    pub param_type: String,
    pub data_version: u16,
    pub header_address: u64,
    pub format_byte: u8,
    pub row_count: u16,
    pub layout: RowLayout,
}
impl ParameterSchemaSnapshot {
    /// Matches observable metadata and spacing. A single-row/partial-index PARAM has
    /// no stride evidence; header matching never establishes in-game acceptance.
    pub fn check_definition(&self, definition: &FieldDefinition) -> Result<()> {
        if self.param_type != definition.param_type || self.data_version != definition.data_version
        {
            return Err(Error::InvalidProfile(
                "NR PARAM type/data version differs from pinned definition",
            ));
        }
        match self.layout {
            RowLayout::UniformStride(stride) if stride != definition.packed_size => Err(
                Error::InvalidProfile("NR PARAM stride differs from pinned row size"),
            ),
            RowLayout::Irregular => {
                Err(Error::InvalidProfile("NR PARAM rows have irregular layout"))
            }
            _ => Ok(()),
        }
    }
    pub fn has_row_stride_evidence(&self) -> bool {
        matches!(self.layout, RowLayout::UniformStride(_))
    }
}

impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    pub fn parameter_schema(
        &self,
        root: u64,
        name: &str,
        variant: usize,
        limit: usize,
    ) -> Result<ParameterSchemaSnapshot> {
        let rows = self.parameter_rows(root, name, variant, limit)?;
        let schema = self.core().parameter_schema(&rows)?;
        if self.parameter_rows(root, name, variant, limit)? != rows {
            return Err(Error::InvalidProfile(
                "NR PARAM changed during schema inspection",
            ));
        }
        Ok(schema)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn parameter_schema(
        &self,
        rows: &ParameterRowsSnapshot,
    ) -> Result<ParameterSchemaSnapshot> {
        let header = rows.header_address;
        let at = |offset: u64| header.checked_add(offset).ok_or(Error::AddressOverflow);
        if self.memory.read_u8(at(0x2c)?)? != 0 {
            return Err(Error::InvalidProfile(
                "NR PARAM schema requires little-endian header",
            ));
        }
        let mut version = [0; 2];
        self.memory.read_exact(at(8)?, &mut version)?;
        let data_version = u16::from_le_bytes(version);
        let format = self.memory.read_u8(at(0x2d)?)?;
        if format != rows.format_byte {
            return Err(Error::InvalidProfile(
                "NR PARAM format changed during schema inspection",
            ));
        }
        let (param_type, type_pointer) = if format & 0x80 == 0 {
            let mut raw = [0; 32];
            self.memory.read_exact(at(0xc)?, &mut raw)?;
            (decode_type(&raw)?, None)
        } else {
            let relative = self.u64(header, 0x10)?;
            if relative == 0 || relative > 64 * 1024 * 1024 - 128 {
                return Err(Error::InvalidProfile(
                    "NR PARAM type offset is outside relative span limit",
                ));
            }
            let start = at(relative)?;
            let mut bytes = Vec::new();
            for offset in 0..128 {
                let byte = self
                    .memory
                    .read_u8(start.checked_add(offset).ok_or(Error::AddressOverflow)?)?;
                bytes.push(byte);
                if byte == 0 {
                    break;
                }
            }
            if bytes.last() != Some(&0) {
                return Err(Error::InvalidProfile("unterminated NR PARAM type"));
            }
            (decode_type(&bytes)?, Some(relative))
        };
        let mut ordered = BTreeMap::new();
        for row in &rows.rows {
            if let Some(previous) = ordered.insert(row.ordinal, row.relative_offset)
                && previous != row.relative_offset
            {
                return Err(Error::InvalidProfile(
                    "NR PARAM aliases disagree on row offset",
                ));
            }
        }
        let layout = if rows.native_row_count < 2 {
            RowLayout::InsufficientRows
        } else if ordered.len() != rows.native_row_count as usize
            || ordered.keys().copied().ne(0..rows.native_row_count as u32)
        {
            RowLayout::IncompleteOrdinalIndex
        } else {
            let offsets: Vec<_> = ordered.values().copied().collect();
            let differences: Vec<_> = offsets
                .windows(2)
                .map(|pair| pair[1].checked_sub(pair[0]))
                .collect();
            match differences.first().copied().flatten() {
                Some(stride)
                    if stride > 0 && differences.iter().all(|delta| *delta == Some(stride)) =>
                {
                    RowLayout::UniformStride(
                        usize::try_from(stride).map_err(|_| Error::AddressOverflow)?,
                    )
                }
                _ => RowLayout::Irregular,
            }
        };
        let mut repeated = [0; 2];
        self.memory.read_exact(at(8)?, &mut repeated)?;
        if repeated != version
            || self.memory.read_u8(at(0x2c)?)? != 0
            || self.memory.read_u8(at(0x2d)?)? != format
        {
            return Err(Error::InvalidProfile("NR PARAM schema metadata changed"));
        }
        if let Some(relative) = type_pointer {
            self.unchanged_u64(header, 0x10, relative)?;
        }
        Ok(ParameterSchemaSnapshot {
            param_type,
            data_version,
            header_address: header,
            format_byte: format,
            row_count: rows.native_row_count,
            layout,
        })
    }
}
fn decode_type(bytes: &[u8]) -> Result<String> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let value = std::str::from_utf8(&bytes[..end])
        .map_err(|_| Error::InvalidProfile("NR PARAM type is not ASCII"))?
        .trim_end_matches(' ');
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(Error::InvalidProfile("invalid NR PARAM type name"));
    }
    Ok(value.to_owned())
}
