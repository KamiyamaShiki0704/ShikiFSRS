//! MSVC x64 RTTI discovery. Results identify metadata, not verified object fields.
use crate::{Error, Result, pe::FileImage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vtable {
    pub type_descriptor_rva: u32,
    pub locator_rva: u32,
    pub object_offset: u32,
    pub table_rva: u32,
    pub entries: Vec<u32>,
}

impl FileImage<'_> {
    /// Finds one complete decorated MSVC name, including its terminating zero.
    pub fn type_descriptor(&self, decorated_name: &str) -> Result<u32> {
        if decorated_name.is_empty()
            || decorated_name.as_bytes().contains(&0)
            || decorated_name.len() > 4096
        {
            return Err(Error::InvalidPe("invalid RTTI name query"));
        }
        let mut needle = decorated_name.as_bytes().to_vec();
        needle.push(0);
        let mut found = None;
        for section in self.sections.iter().filter(|s| !s.is_executable()) {
            if (section.raw_size as usize) < needle.len() + 16 {
                continue;
            }
            let raw = self.read_rva(section.rva, section.raw_size as usize)?;
            for (offset, value) in raw.windows(needle.len()).enumerate().skip(16) {
                if value == needle {
                    let candidate = section
                        .rva
                        .checked_add((offset - 16) as u32)
                        .ok_or(Error::AddressOverflow)?;
                    if found.replace(candidate).is_some() {
                        return Err(Error::AmbiguousType(decorated_name.to_owned()));
                    }
                }
            }
        }
        found.ok_or_else(|| Error::TypeNotFound(decorated_name.to_owned()))
    }

    /// Validates complete-object locators and executable function targets.
    ///
    /// Naming a class alone must never authorize casting a live pointer to a Rust struct.
    pub fn vtables(&self, decorated_name: &str) -> Result<Vec<Vtable>> {
        let descriptor = self.type_descriptor(decorated_name)?;
        let mut result = Vec::new();
        for section in self
            .sections
            .iter()
            .filter(|s| !s.is_writable() && !s.is_executable())
        {
            let raw = self.read_rva(section.rva, section.raw_size as usize)?;
            for (word_index, word) in raw.chunks_exact(4).enumerate() {
                if u32::from_le_bytes(word.try_into().expect("checked width")) != descriptor {
                    continue;
                }
                let position = word_index * 4;
                let Some(start) = position.checked_sub(12) else {
                    continue;
                };
                let Some(locator) = raw.get(start..start + 24) else {
                    continue;
                };
                let words: Vec<u32> = locator
                    .chunks_exact(4)
                    .map(|b| u32::from_le_bytes(b.try_into().expect("checked width")))
                    .collect();
                let locator_rva = section
                    .rva
                    .checked_add(start as u32)
                    .ok_or(Error::AddressOverflow)?;
                if words[0] != 1
                    || words[5] != locator_rva
                    || words[1] >= self.image_size
                    || self.read_rva(words[4], 4).is_err()
                {
                    continue;
                }
                let locator_pointer = self
                    .preferred_base
                    .checked_add(locator_rva as u64)
                    .ok_or(Error::AddressOverflow)?;
                for table_section in self
                    .sections
                    .iter()
                    .filter(|s| !s.is_writable() && !s.is_executable())
                {
                    let table_raw =
                        self.read_rva(table_section.rva, table_section.raw_size as usize)?;
                    for (index, pointer) in table_raw.chunks_exact(8).enumerate() {
                        if u64::from_le_bytes(pointer.try_into().expect("checked width"))
                            != locator_pointer
                        {
                            continue;
                        }
                        let table_start = (index + 1) * 8;
                        let mut entries = Vec::new();
                        for slot in 0..32 {
                            let at = table_start + slot * 8;
                            let Some(bytes) = table_raw.get(at..at + 8) else {
                                break;
                            };
                            let address =
                                u64::from_le_bytes(bytes.try_into().expect("checked width"));
                            let Some(relative) = address.checked_sub(self.preferred_base) else {
                                break;
                            };
                            let Ok(rva) = u32::try_from(relative) else {
                                break;
                            };
                            if !self
                                .section_at(rva)
                                .is_some_and(|section| section.is_executable())
                                || self.read_rva(rva, 1).is_err()
                            {
                                break;
                            }
                            entries.push(rva);
                        }
                        if !entries.is_empty() {
                            result.push(Vtable {
                                type_descriptor_rva: descriptor,
                                locator_rva,
                                object_offset: words[1],
                                table_rva: table_section
                                    .rva
                                    .checked_add(table_start as u32)
                                    .ok_or(Error::AddressOverflow)?,
                                entries,
                            });
                        }
                    }
                }
            }
        }
        result.sort_by_key(|table| table.table_rva);
        result.dedup_by_key(|table| table.table_rva);
        Ok(result)
    }
}
