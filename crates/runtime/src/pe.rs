//! Checked AMD64 PE file views, independently implemented from Microsoft's PE specification.
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: [u8; 8],
    pub rva: u32,
    pub virtual_size: u32,
    pub raw_offset: u32,
    pub raw_size: u32,
    pub characteristics: u32,
}

impl Section {
    pub fn is_executable(&self) -> bool {
        self.characteristics & 0x2000_0000 != 0
    }

    pub fn is_writable(&self) -> bool {
        self.characteristics & 0x8000_0000 != 0
    }
}

#[derive(Debug)]
pub struct FileImage<'a> {
    bytes: &'a [u8],
    pub preferred_base: u64,
    pub image_size: u32,
    pub header_size: u32,
    pub sections: Vec<Section>,
}

fn span(bytes: &[u8], at: usize, size: usize) -> Result<&[u8]> {
    let end = at.checked_add(size).ok_or(Error::AddressOverflow)?;
    bytes.get(at..end).ok_or(Error::InvalidPe("truncated file"))
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        span(bytes, at, 2)?.try_into().expect("checked width"),
    ))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        span(bytes, at, 4)?.try_into().expect("checked width"),
    ))
}

impl<'a> FileImage<'a> {
    /// Exact entry from the AMD64 exception directory. A chained entry can be
    /// only one fragment; this is not a claim to reconstruct an entire function.
    pub fn runtime_function(&self, entry_rva: u32) -> Result<crate::profile::CodeRange> {
        let pe = u32_at(self.bytes, 0x3c)? as usize;
        let optional = pe.checked_add(24).ok_or(Error::AddressOverflow)?;
        let optional_size = u16_at(self.bytes, pe + 20)? as usize;
        if optional_size < 144 || u32_at(self.bytes, optional + 108)? < 4 {
            return Err(Error::InvalidPe("missing exception directory"));
        }
        let table = u32_at(self.bytes, optional + 136)?;
        let size = u32_at(self.bytes, optional + 140)? as usize;
        if table == 0 || size == 0 || !size.is_multiple_of(12) {
            return Err(Error::InvalidPe("invalid exception directory size"));
        }
        let bytes = self.read_rva(table, size)?;
        let mut previous = None;
        let mut found = None;
        for row in bytes.chunks_exact(12) {
            let begin = u32_at(row, 0)?;
            let end = u32_at(row, 4)?;
            if end <= begin || previous.is_some_and(|value| begin <= value) {
                return Err(Error::InvalidPe("unsorted or invalid runtime functions"));
            }
            let section = self
                .section_at(begin)
                .filter(|s| s.is_executable())
                .ok_or(Error::InvalidPe("runtime function is not executable"))?;
            if end as u64 > section.rva as u64 + section.raw_size as u64 {
                return Err(Error::InvalidPe(
                    "runtime function crosses file-backed code",
                ));
            }
            previous = Some(begin);
            if begin == entry_rva {
                found = Some(crate::profile::CodeRange {
                    rva: begin,
                    length: (end - begin) as usize,
                });
            }
        }
        found.ok_or(Error::InvalidProfile(
            "native target has no exact runtime-function entry",
        ))
    }

    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if span(bytes, 0, 2)? != b"MZ" {
            return Err(Error::InvalidPe("DOS signature"));
        }
        let pe = u32_at(bytes, 0x3c)? as usize;
        if pe < 0x40 || span(bytes, pe, 4)? != b"PE\0\0" {
            return Err(Error::InvalidPe("PE signature"));
        }
        let coff = pe.checked_add(4).ok_or(Error::AddressOverflow)?;
        span(bytes, coff, 20)?;
        let machine = u16_at(bytes, coff)?;
        if machine != 0x8664 {
            return Err(Error::UnsupportedMachine(machine));
        }
        let count = u16_at(bytes, coff + 2)? as usize;
        if !(1..=96).contains(&count) {
            return Err(Error::InvalidPe("section count"));
        }
        let optional_size = u16_at(bytes, coff + 16)? as usize;
        if optional_size < 112 {
            return Err(Error::InvalidPe("optional header is too small"));
        }
        let optional = coff + 20;
        let header = span(bytes, optional, optional_size)?;
        if u16_at(header, 0)? != 0x20b {
            return Err(Error::InvalidPe("PE32+ required"));
        }
        let preferred_base =
            u64::from_le_bytes(span(header, 24, 8)?.try_into().expect("checked width"));
        let image_size = u32_at(header, 56)?;
        let header_size = u32_at(header, 60)?;
        if header_size == 0 || header_size > image_size || header_size as usize > bytes.len() {
            return Err(Error::InvalidPe("image/header size"));
        }
        preferred_base
            .checked_add(image_size as u64)
            .ok_or(Error::AddressOverflow)?;
        let table = optional
            .checked_add(optional_size)
            .ok_or(Error::AddressOverflow)?;
        let table_size = count.checked_mul(40).ok_or(Error::AddressOverflow)?;
        span(bytes, table, table_size)?;
        if table
            .checked_add(table_size)
            .ok_or(Error::AddressOverflow)?
            > header_size as usize
        {
            return Err(Error::InvalidPe("section table outside headers"));
        }
        let mut sections = Vec::new();
        for i in 0..count {
            let raw = span(bytes, table + i * 40, 40)?;
            let section = Section {
                name: raw[..8].try_into().expect("checked width"),
                virtual_size: u32_at(raw, 8)?,
                rva: u32_at(raw, 12)?,
                raw_size: u32_at(raw, 16)?,
                raw_offset: u32_at(raw, 20)?,
                characteristics: u32_at(raw, 36)?,
            };
            let extent = section.virtual_size.max(section.raw_size);
            let end = section
                .rva
                .checked_add(extent)
                .ok_or(Error::AddressOverflow)?;
            if end > image_size || (extent != 0 && section.rva < header_size) {
                return Err(Error::InvalidPe("section outside image"));
            }
            if section.raw_size != 0 {
                if section.raw_offset < header_size {
                    return Err(Error::InvalidPe("section data overlaps headers"));
                }
                span(
                    bytes,
                    section.raw_offset as usize,
                    section.raw_size as usize,
                )?;
            }
            sections.push(section);
        }
        let mut ordered: Vec<&Section> = sections
            .iter()
            .filter(|s| s.virtual_size.max(s.raw_size) != 0)
            .collect();
        ordered.sort_by_key(|s| s.rva);
        if ordered.windows(2).any(|pair| {
            pair[0].rva as u64 + pair[0].virtual_size.max(pair[0].raw_size) as u64
                > pair[1].rva as u64
        }) {
            return Err(Error::InvalidPe("overlapping virtual sections"));
        }
        let mut raw_order: Vec<&Section> = sections.iter().filter(|s| s.raw_size != 0).collect();
        raw_order.sort_by_key(|s| s.raw_offset);
        if raw_order.windows(2).any(|pair| {
            pair[0].raw_offset as u64 + pair[0].raw_size as u64 > pair[1].raw_offset as u64
        }) {
            return Err(Error::InvalidPe("overlapping raw sections"));
        }
        Ok(Self {
            bytes,
            preferred_base,
            image_size,
            header_size,
            sections,
        })
    }

    /// Reads only file-backed bytes. Zero-fill tails are not silently invented.
    pub fn read_rva(&self, rva: u32, size: usize) -> Result<&'a [u8]> {
        if size == 0 {
            return Ok(&self.bytes[..0]);
        }
        let end = (rva as u64)
            .checked_add(size as u64)
            .ok_or(Error::AddressOverflow)?;
        if end <= self.header_size as u64 {
            return span(self.bytes, rva as usize, size);
        }
        for section in &self.sections {
            let offset = (rva as u64).checked_sub(section.rva as u64);
            if let Some(offset) = offset
                && offset
                    .checked_add(size as u64)
                    .is_some_and(|end| end <= section.raw_size as u64)
            {
                let file_offset = (section.raw_offset as u64)
                    .checked_add(offset)
                    .ok_or(Error::AddressOverflow)?;
                return span(
                    self.bytes,
                    usize::try_from(file_offset).map_err(|_| Error::AddressOverflow)?,
                    size,
                );
            }
        }
        Err(Error::OutOfBounds {
            address: rva as u64,
            length: size,
        })
    }

    pub fn section_at(&self, rva: u32) -> Option<&Section> {
        self.sections.iter().find(|section| {
            section.rva <= rva
                && (rva as u64)
                    < section.rva as u64 + section.virtual_size.max(section.raw_size) as u64
        })
    }
}
