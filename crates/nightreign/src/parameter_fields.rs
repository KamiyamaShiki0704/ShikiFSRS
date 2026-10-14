//! Pinned independent NR definitions. Typed reads remain experimental: executable
//! identity and a named repository slot do not establish a live regulation schema.
use crate::reader::NightreignReader;
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
use std::marker::PhantomData;

mod generated;
pub use generated::{
    behavior, camera, effect, initial, npc, npc_attack, player_attack, player_behavior, think,
    weapon,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldDefinition {
    pub table: &'static str,
    pub native_name: &'static str,
    pub param_type: &'static str,
    pub slot: u16,
    pub offset: usize,
    pub width: usize,
    pub bit: Option<u8>,
    pub data_version: u16,
    pub packed_size: usize,
}
pub const DEFINITION_REVISION: &str = "ff7245e524329bc3eab00036723d2bd53384cedf";
pub fn definitions() -> &'static [FieldDefinition] {
    generated::DEFINITIONS
}

#[derive(Debug, PartialEq, Eq)]
pub struct Field<T> {
    definition: &'static FieldDefinition,
    _type: PhantomData<fn() -> T>,
}
impl<T> Copy for Field<T> {}
impl<T> Clone for Field<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Field<T> {
    const fn new(definition: &'static FieldDefinition) -> Self {
        Self {
            definition,
            _type: PhantomData,
        }
    }
    pub fn definition(self) -> &'static FieldDefinition {
        self.definition
    }
}
mod sealed {
    pub trait Sealed {}
}
pub trait Scalar: sealed::Sealed + Sized {
    fn decode(bytes: &[u8], bit: Option<u8>) -> Result<Self>;
}
macro_rules! scalar {
    ($($ty:ty),*)=>{$(
        impl sealed::Sealed for $ty {}
        impl Scalar for $ty {
            fn decode(bytes:&[u8],bit:Option<u8>)->Result<Self> {
                if bit.is_some() { return Err(Error::InvalidProfile("bit field used with a numeric scalar")); }
                Ok(<$ty>::from_le_bytes(bytes.try_into().map_err(|_|Error::InvalidProfile("truncated NR scalar field"))?))
            }
        }
    )*};
}
scalar!(u8, i8, u16, i16, u32, i32, f32);
impl sealed::Sealed for bool {}
impl Scalar for bool {
    fn decode(bytes: &[u8], bit: Option<u8>) -> Result<Self> {
        let bit = bit
            .filter(|bit| *bit < 8)
            .ok_or(Error::InvalidProfile("invalid NR boolean bit field"))?;
        if bytes.len() != 1 {
            return Err(Error::InvalidProfile("truncated NR bit field"));
        }
        Ok(bytes[0] & (1 << bit) != 0)
    }
}
impl<T: Scalar> Field<T> {
    /// Decodes an owned little-endian row copy. Table identity remains the caller's
    /// responsibility for standalone bytes; reader methods select the pinned table.
    pub fn read_row_bytes(self, bytes: &[u8]) -> Result<T> {
        let field = self.definition;
        let end = field
            .offset
            .checked_add(field.width)
            .ok_or(Error::AddressOverflow)?;
        let value = bytes
            .get(field.offset..end)
            .ok_or(Error::InvalidProfile("NR row does not contain this field"))?;
        T::decode(value, field.bit)
    }
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// Reads only a pinned scalar/bit span from a checked NR row. No casts to native
    /// schema structs, writes, or live-schema acceptance are implied.
    pub fn parameter_field<T: Scalar>(
        &self,
        root: u64,
        variant: usize,
        id: u32,
        field: Field<T>,
        limit: usize,
    ) -> Result<Option<T>> {
        self.parameter_field_with_policy(
            root,
            variant,
            id,
            field,
            limit,
            crate::parameter_schema::SchemaPolicy::MetadataAndAvailableStride,
        )
    }
    /// Applies an explicit spacing-evidence policy before copying the field.
    pub fn parameter_field_with_policy<T: Scalar>(
        &self,
        root: u64,
        variant: usize,
        id: u32,
        field: Field<T>,
        limit: usize,
        policy: crate::parameter_schema::SchemaPolicy,
    ) -> Result<Option<T>> {
        let definition = field.definition();
        if self
            .file
            .parameter_definitions()
            .iter()
            .find(|candidate| candidate.name == definition.table)
            .map(|candidate| candidate.slot)
            != Some(definition.slot)
        {
            return Err(Error::InvalidProfile(
                "NR field table/slot differs from file",
            ));
        }
        let rows = self.parameter_rows(root, definition.table, variant, limit)?;
        let schema = self.core().parameter_schema(&rows)?;
        schema.check_definition_with_policy(definition, policy)?;
        let Some(row) = rows.row(id) else {
            return Ok(None);
        };
        let start = row
            .address
            .checked_add(definition.offset as u64)
            .ok_or(Error::AddressOverflow)?;
        let end = row
            .relative_offset
            .checked_add((definition.offset + definition.width) as u64)
            .ok_or(Error::AddressOverflow)?;
        if end > 64 * 1024 * 1024 {
            return Err(Error::InvalidProfile(
                "NR field exceeds relative span limit",
            ));
        }
        if rows
            .rows
            .iter()
            .filter(|other| other.relative_offset > row.relative_offset)
            .any(|other| end > other.relative_offset)
        {
            return Err(Error::InvalidProfile(
                "NR field overlaps the next indexed row",
            ));
        }
        let mut bytes = [0; 4];
        self.module
            .memory()
            .read_exact(start, &mut bytes[..definition.width])?;
        let value = T::decode(&bytes[..definition.width], definition.bit)?;
        if self.parameter_rows(root, definition.table, variant, limit)? != rows {
            return Err(Error::InvalidProfile(
                "NR row changed during typed field read",
            ));
        }
        if self.core().parameter_schema(&rows)? != schema {
            return Err(Error::InvalidProfile(
                "NR PARAM schema changed during typed field read",
            ));
        }
        Ok(Some(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn behavior_and_effect_links_preserve_signed_ids_and_independent_target_bits() {
        let mut row = [0; 32];
        row[9] = 2;
        row[12..16].copy_from_slice(&(-1i32).to_le_bytes());
        row[20..24].copy_from_slice(&33i32.to_le_bytes());
        assert_eq!(behavior::REF_TYPE.read_row_bytes(&row).unwrap(), 2);
        assert_eq!(behavior::REF_ID.read_row_bytes(&row).unwrap(), -1);
        assert_eq!(player_behavior::STAMINA.read_row_bytes(&row).unwrap(), 33);
        assert_eq!(behavior::REF_ID.definition().slot, 12);
        assert_eq!(player_behavior::REF_ID.definition().slot, 13);
        let mut effect_row = [0; 976];
        effect_row[292..296].copy_from_slice(&(-1i32).to_le_bytes());
        effect_row[351] = 5;
        assert_eq!(
            effect::REPLACE_SP_EFFECT_ID
                .read_row_bytes(&effect_row)
                .unwrap(),
            -1
        );
        assert!(
            effect::EFFECT_TARGET_SELF
                .read_row_bytes(&effect_row)
                .unwrap()
        );
        assert!(
            !effect::EFFECT_TARGET_FRIEND
                .read_row_bytes(&effect_row)
                .unwrap()
        );
        assert!(
            effect::EFFECT_TARGET_ENEMY
                .read_row_bytes(&effect_row)
                .unwrap()
        );
        assert_eq!(effect::STATE_INFO.definition().offset, 0x156);
    }
    #[test]
    fn pinned_nr_scalar_types_preserve_signed_values_and_float_bits() {
        let mut npc_row = vec![0; 816];
        npc_row[36..40].copy_from_slice(&1234u32.to_le_bytes());
        npc_row[4..8].copy_from_slice(&(-17i32).to_le_bytes());
        npc_row[20..24].copy_from_slice(&1.75f32.to_le_bytes());
        npc_row[244..246].copy_from_slice(&600u16.to_le_bytes());
        assert_eq!(npc::HP.read_row_bytes(&npc_row).unwrap(), 1234);
        assert_eq!(
            npc::BEHAVIOR_VARIATION_ID.read_row_bytes(&npc_row).unwrap(),
            -17
        );
        assert_eq!(npc::HIT_HEIGHT.read_row_bytes(&npc_row).unwrap(), 1.75);
        assert_eq!(npc::STAMINA.read_row_bytes(&npc_row).unwrap(), 600);
        let mut row = vec![0; 464];
        row[128] = (-7i8) as u8;
        assert_eq!(
            npc_attack::ATK_DIR_FOR_SFX.read_row_bytes(&row).unwrap(),
            -7
        );
    }
    #[test]
    fn nr_boolean_fields_select_only_the_pinned_bit() {
        let mut row = vec![0; 680];
        row[261] = 0b00100001;
        assert!(weapon::RIGHT_HAND_EQUIPABLE.read_row_bytes(&row).unwrap());
        assert!(weapon::ENABLE_GUARD.read_row_bytes(&row).unwrap());
        row[261] = 0b11011110;
        assert!(!weapon::RIGHT_HAND_EQUIPABLE.read_row_bytes(&row).unwrap());
        assert!(!weapon::ENABLE_GUARD.read_row_bytes(&row).unwrap());
    }
    #[test]
    fn short_rows_fail_and_descriptor_slots_remain_nr_specific() {
        assert!(npc::HP.read_row_bytes(&[0; 39]).is_err());
        assert!(weapon::ENABLE_GUARD.read_row_bytes(&[0; 261]).is_err());
        assert_eq!(npc_attack::ATK_DIR_FOR_SFX.definition().slot, 7);
        assert_eq!(player_attack::ATK_DIR_FOR_SFX.definition().slot, 8);
        assert_eq!(definitions().len(), 98);
        assert_eq!(npc::WEIGHT.definition().width, 4);
    }
    #[test]
    fn added_effect_and_resistance_fields_decode_pinned_signed_and_unsigned_values() {
        let mut effect_row = vec![0; 976];
        effect_row[160..164].copy_from_slice(&(-100i32).to_le_bytes());
        effect_row[318..320].copy_from_slice(&(-3i16).to_le_bytes());
        effect_row[8..12].copy_from_slice(&5.5f32.to_le_bytes());
        assert_eq!(
            effect::CHANGE_HP_POINT.read_row_bytes(&effect_row).unwrap(),
            -100
        );
        assert_eq!(effect::SP_CATEGORY.read_row_bytes(&effect_row).unwrap(), -3);
        assert_eq!(
            effect::EFFECT_ENDURANCE
                .read_row_bytes(&effect_row)
                .unwrap(),
            5.5
        );
        let mut npc_row = vec![0; 816];
        npc_row[480..482].copy_from_slice(&500u16.to_le_bytes());
        assert_eq!(npc::RESIST_FREEZE.read_row_bytes(&npc_row).unwrap(), 500);
    }
}
