//! Bounded, file-only task definition catalogs. Raw seed words are not scheduler contracts.
use crate::{Error, Result, pe::FileImage};
use std::collections::BTreeSet;

const MAX_DEFINITIONS: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDefinition {
    pub registration_index: usize,
    pub name: String,
    pub native_seed_words: [u32; 4],
}

/// Owned metadata copied from an immutable PE file, sorted by registration index.
/// This does not identify live task groups, callback IDs or legal engine phases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDefinitions {
    records: Vec<TaskDefinition>,
}

impl TaskDefinitions {
    /// The caller supplies an independently evidenced table and exact record count.
    /// Rejects writable/code sections, unbacked tails, malformed names and duplicate indices.
    pub fn read(image: &FileImage<'_>, table: u32, count: usize) -> Result<Self> {
        Ok(Self {
            records: definitions(image, table, count)?,
        })
    }

    pub fn records(&self) -> &[TaskDefinition] {
        &self.records
    }

    pub fn find(&self, name: &str) -> Option<&TaskDefinition> {
        self.records.iter().find(|record| record.name == name)
    }
}

fn definitions(image: &FileImage<'_>, table: u32, count: usize) -> Result<Vec<TaskDefinition>> {
    let size = count.checked_mul(32).ok_or(Error::AddressOverflow)?;
    if count == 0 || count > MAX_DEFINITIONS {
        return Err(Error::InvalidProfile("invalid task definition count"));
    }
    let section = image
        .section_at(table)
        .filter(|section| !section.is_writable() && !section.is_executable())
        .ok_or(Error::InvalidProfile(
            "task definitions are not readonly data",
        ))?;
    if table as u64 + size as u64 > section.rva as u64 + section.raw_size as u64 {
        return Err(Error::InvalidProfile(
            "task definitions cross their section",
        ));
    }
    let bytes = image.read_rva(table, size)?;
    let mut result = Vec::with_capacity(count);
    let mut names = BTreeSet::new();
    let mut indices = BTreeSet::new();
    for row in bytes.chunks_exact(32) {
        let index = u32::from_le_bytes(row[..4].try_into().expect("checked width")) as usize;
        if index >= count || !indices.insert(index) {
            return Err(Error::InvalidProfile(
                "task definition indices are duplicate or out of range",
            ));
        }
        let pointer = u64::from_le_bytes(row[8..16].try_into().expect("checked width"));
        let rva = pointer
            .checked_sub(image.preferred_base)
            .and_then(|offset| u32::try_from(offset).ok())
            .ok_or(Error::InvalidProfile(
                "task definition name is outside image",
            ))?;
        let name = label(image, rva)?;
        if !names.insert(name.clone()) {
            return Err(Error::InvalidProfile("duplicate task definition name"));
        }
        let mut native_seed_words = [0; 4];
        for (word, raw) in native_seed_words
            .iter_mut()
            .zip(row[16..32].chunks_exact(4))
        {
            *word = u32::from_le_bytes(raw.try_into().expect("checked width"));
        }
        result.push(TaskDefinition {
            registration_index: index,
            name,
            native_seed_words,
        });
    }
    result.sort_by_key(|row| row.registration_index);
    Ok(result)
}

fn label(image: &FileImage<'_>, start: u32) -> Result<String> {
    let section = image
        .section_at(start)
        .filter(|s| !s.is_executable() && !s.is_writable())
        .ok_or(Error::InvalidProfile("task name is not readonly data"))?;
    if !start.is_multiple_of(2) {
        return Err(Error::InvalidProfile("task name is not UTF-16 aligned"));
    }
    let end = section.rva as u64 + section.raw_size as u64;
    let mut words = Vec::new();
    for index in 0..256u32 {
        let at = start.checked_add(index * 2).ok_or(Error::AddressOverflow)?;
        if at as u64 + 2 > end {
            return Err(Error::InvalidProfile("task name crosses its section"));
        }
        let raw = image.read_rva(at, 2)?;
        let word = u16::from_le_bytes(raw.try_into().expect("checked width"));
        if word == 0 {
            if words.is_empty() {
                return Err(Error::InvalidProfile("empty task name"));
            }
            return String::from_utf16(&words)
                .map_err(|_| Error::InvalidProfile("invalid UTF-16 task name"));
        }
        words.push(word);
    }
    Err(Error::InvalidProfile("task name has no bounded terminator"))
}
