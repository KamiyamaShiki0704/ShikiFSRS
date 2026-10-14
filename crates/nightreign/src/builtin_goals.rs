//! File-only builtin Goal definitions from the exact current native factory.
//! Directory entries do not imply implementation/validation of native Goal behavior.
use shikifsrs_runtime::{Error, Result, pe::FileImage};
pub const BUILTIN_GOAL_COUNT: usize = 39;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinGoalDefinition {
    /// Goal definition kind, not a controller request/animation ID.
    pub id: i32,
    pub name: &'static str,
    pub class_name: &'static str,
    pub allocation_size: usize,
    /// File metadata only; no callable constructor ABI is exposed.
    pub constructor_rva: u32,
    pub primary_vtable_rva: u32,
    pub name_rva: u32,
    pub factory_constructor_call_rva: u32,
}
fn hex(text: &str) -> Result<u32> {
    u32::from_str_radix(text, 16).map_err(|_| Error::InvalidProfile("invalid NR builtin Goal RVA"))
}
pub(crate) fn catalog(text: &'static str) -> Result<Vec<BuiltinGoalDefinition>> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let fields = line.split(',').collect::<Vec<_>>();
        if fields.len() != 8 {
            return Err(Error::InvalidProfile("invalid NR builtin Goal row"));
        }
        let row = BuiltinGoalDefinition {
            id: fields[0]
                .parse()
                .map_err(|_| Error::InvalidProfile("invalid NR builtin Goal ID"))?,
            name: fields[1],
            allocation_size: fields[2]
                .parse()
                .map_err(|_| Error::InvalidProfile("invalid NR builtin Goal size"))?,
            constructor_rva: hex(fields[3])?,
            primary_vtable_rva: hex(fields[4])?,
            class_name: fields[5],
            name_rva: hex(fields[6])?,
            factory_constructor_call_rva: hex(fields[7])?,
        };
        if row.id < 0
            || row.name.is_empty()
            || row.name.len() > 128
            || !row.class_name.starts_with(".?AV")
            || row.allocation_size < 0x110
            || row.allocation_size > 4096
            || !row.primary_vtable_rva.is_multiple_of(8)
            || !row.name_rva.is_multiple_of(2)
            || rows
                .last()
                .is_some_and(|prior: &BuiltinGoalDefinition| prior.id >= row.id)
        {
            return Err(Error::InvalidProfile(
                "invalid NR builtin Goal catalog order/shape",
            ));
        }
        rows.push(row);
    }
    if rows.len() != BUILTIN_GOAL_COUNT {
        return Err(Error::InvalidProfile(
            "NR builtin Goal catalog count differs",
        ));
    }
    Ok(rows)
}
pub(crate) fn read(image: &FileImage<'_>) -> Result<Vec<BuiltinGoalDefinition>> {
    let rows = catalog(include_str!("../data/builtin-goals.csv"))?;
    for row in &rows {
        let expected = row
            .name
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        if !image
            .section_at(row.name_rva)
            .is_some_and(|s| !s.is_executable() && !s.is_writable())
            || image.read_rva(row.name_rva, expected.len())? != expected
            || !image.vtables(row.class_name)?.iter().any(|v| {
                v.table_rva == row.primary_vtable_rva
                    && v.object_offset == 0
                    && !v.entries.is_empty()
            })
            || !image
                .section_at(row.constructor_rva)
                .is_some_and(|s| s.is_executable())
            || !image
                .section_at(row.factory_constructor_call_rva)
                .is_some_and(|s| s.is_executable())
        {
            return Err(Error::InvalidProfile(
                "NR builtin Goal metadata differs from file",
            ));
        }
        let call = image.read_rva(row.factory_constructor_call_rva, 5)?;
        let displacement = i32::from_le_bytes(call[1..].try_into().expect("call width"));
        if call[0] != 0xe8
            || row.factory_constructor_call_rva as i64 + 5 + displacement as i64
                != row.constructor_rva as i64
        {
            return Err(Error::InvalidProfile(
                "NR builtin Goal constructor call differs",
            ));
        }
    }
    Ok(rows)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_duplicate_and_reordered_catalogs_are_rejected() {
        assert!(catalog("").is_err());
        assert!(catalog("2034,ExecAction").is_err());
        let source = include_str!("../data/builtin-goals.csv");
        assert!(
            catalog(Box::leak(
                source.replace("2,Stay,", "0,Stay,").into_boxed_str()
            ))
            .is_err()
        );
        assert!(
            catalog(Box::leak(
                source
                    .replace("2034,ExecAction,296,", "2034,ExecAction,1,")
                    .into_boxed_str()
            ))
            .is_err()
        );
        let mut reordered = source.lines().collect::<Vec<_>>();
        reordered.swap(0, 1);
        assert!(catalog(Box::leak(reordered.join("\n").into_boxed_str())).is_err());
    }
}
