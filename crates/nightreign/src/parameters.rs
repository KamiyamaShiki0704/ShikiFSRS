//! Immutable names/slots checked against the exact NR executable's file table.
use shikifsrs_runtime::{Error, Result, pe::FileImage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterDefinition {
    pub slot: u16,
    pub name: &'static str,
}

pub(crate) fn read(image: &FileImage<'_>) -> Result<Vec<ParameterDefinition>> {
    let mut definitions = Vec::with_capacity(253);
    for (index, line) in include_str!("../data/parameter-slots.csv")
        .lines()
        .enumerate()
    {
        let (slot, name) = line
            .split_once(',')
            .ok_or(Error::InvalidProfile("invalid NR parameter catalog"))?;
        let slot: u16 = slot
            .parse()
            .map_err(|_| Error::InvalidProfile("invalid NR parameter slot"))?;
        if slot >= 253
            || index >= 253
            || name.is_empty()
            || definitions.iter().any(|definition: &ParameterDefinition| {
                definition.slot == slot || definition.name == name
            })
        {
            return Err(Error::InvalidProfile("invalid NR parameter catalog order"));
        }
        let row = image.read_rva(0x39b3c40 + index as u32 * 16, 16)?;
        let pointer = u64::from_le_bytes(row[..8].try_into().expect("checked width"));
        let name_rva = pointer
            .checked_sub(image.preferred_base)
            .and_then(|rva| u32::try_from(rva).ok())
            .ok_or(Error::InvalidProfile("NR parameter name is outside image"))?;
        let section = image
            .section_at(name_rva)
            .filter(|section| !section.is_executable() && !section.is_writable())
            .ok_or(Error::InvalidProfile(
                "NR parameter name is not readonly data",
            ))?;
        let expected: Vec<u8> = name
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        if name_rva as u64 + expected.len() as u64 > section.rva as u64 + section.raw_size as u64
            || !name_rva.is_multiple_of(2)
            || image.read_rva(name_rva, expected.len())? != expected
            || u32::from_le_bytes(row[8..12].try_into().expect("checked width")) != slot as u32
            || row[12..] != [0; 4]
        {
            return Err(Error::InvalidProfile(
                "NR parameter name/slot differs from file",
            ));
        }
        definitions.push(ParameterDefinition { slot, name });
    }
    if definitions.len() != 253
        || image.read_rva(0x39b3c40 + 253 * 16, 16)?
            != [0, 0, 0, 0, 0, 0, 0, 0, 253, 0, 0, 0, 0, 0, 0, 0]
    {
        return Err(Error::InvalidProfile(
            "NR parameter catalog terminator differs from file",
        ));
    }
    // Physical name-table order differs from the repository slot order at 69..72.
    definitions.sort_by_key(|definition| definition.slot);
    Ok(definitions)
}
