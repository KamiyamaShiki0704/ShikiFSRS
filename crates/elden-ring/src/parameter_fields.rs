//! Copied scalar fields from independently pinned ER definitions. Experimental, not live accepted.
use crate::{objects::GameObjectCatalog, parameters::ParameterSlot};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterField {
    WeaponSpAttackCategory,
    EffectDuration,
    EffectCategory,
    EffectStateInfo,
    NpcTeamType,
    NpcBehaviorVariation,
    NpcLockCamera,
    NpcHp,
    NpcMp,
    NpcStamina,
    NpcHitHeight,
    NpcHitRadius,
    NpcWeight,
    NpcCharacterHitHeight,
    NpcCharacterHitRadius,
    CameraDistance,
    CameraHeight,
    CameraLockRadius,
    CameraLockRadiusDistance,
    CameraLockRadiusPlayerDistance,

    BuddyTriggerEffect,
    BuddyNpc,
    BuddyThink,
    BuddyRiddenNpc,
    BuddyRiddenThink,
    EffectHpDelta,
    EffectPoisonAttack,
    EffectDiseaseAttack,
    EffectBloodAttack,
    EffectCurseAttack,
    EffectFreezeAttack,
    EffectSleepAttack,
    EffectMadnessAttack,
    EffectSuperArmorDelta,
    EffectSaDelta,
    BuddyStoneEliminateTarget,
    BuddyStoneSummonedFlag,
    BuddyStoneActivateRegion,
    NpcAttackSfxDirection,
    PlayerAttackSfxDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarType {
    U8,
    I8,
    U16,
    U32,
    I32,
    F32,
}
impl ScalarType {
    pub fn width(self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldEvidence {
    /// Pinned independent definition and packed layout only, without a live schema check.
    Definition,
    /// Definition offset/type additionally matches the owned current EXE's native use.
    DefinitionAndNative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldDefinition {
    pub field: ParameterField,
    pub table: &'static str,
    pub native_name: &'static str,
    pub slot: u16,
    pub offset: usize,
    pub scalar: ScalarType,
    pub evidence: FieldEvidence,
}
impl FieldDefinition {
    const fn new(
        field: ParameterField,
        table: &'static str,
        native_name: &'static str,
        slot: u16,
        offset: usize,
        scalar: ScalarType,
        evidence: FieldEvidence,
    ) -> Self {
        Self {
            field,
            table,
            native_name,
            slot,
            offset,
            scalar,
            evidence,
        }
    }
}
const FIELD_DEFINITIONS: &[FieldDefinition] = &[
    FieldDefinition::new(
        ParameterField::CameraLockRadius,
        "LockCamParam",
        "chrLockRangeMaxRadius",
        31,
        0x10,
        ScalarType::F32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::CameraLockRadiusDistance,
        "LockCamParam",
        "chrLockRangeMaxRadius_forD",
        31,
        0x18,
        ScalarType::F32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::CameraLockRadiusPlayerDistance,
        "LockCamParam",
        "chrLockRangeMaxRadius_forPD",
        31,
        0x1c,
        ScalarType::F32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::NpcTeamType,
        "NpcParam",
        "teamType",
        6,
        0x133,
        ScalarType::U8,
        FieldEvidence::DefinitionAndNative,
    ),
    FieldDefinition::new(
        ParameterField::NpcHitHeight,
        "NpcParam",
        "hitHeight",
        6,
        0x14,
        ScalarType::F32,
        FieldEvidence::DefinitionAndNative,
    ),
    FieldDefinition::new(
        ParameterField::NpcHitRadius,
        "NpcParam",
        "hitRadius",
        6,
        0x18,
        ScalarType::F32,
        FieldEvidence::DefinitionAndNative,
    ),
    FieldDefinition::new(
        ParameterField::NpcWeight,
        "NpcParam",
        "weight",
        6,
        0x1c,
        ScalarType::U32,
        FieldEvidence::DefinitionAndNative,
    ),
    FieldDefinition::new(
        ParameterField::NpcCharacterHitHeight,
        "NpcParam",
        "chrHitHeight",
        6,
        0x154,
        ScalarType::F32,
        FieldEvidence::DefinitionAndNative,
    ),
    FieldDefinition::new(
        ParameterField::NpcCharacterHitRadius,
        "NpcParam",
        "chrHitRadius",
        6,
        0x158,
        ScalarType::F32,
        FieldEvidence::DefinitionAndNative,
    ),
    FieldDefinition::new(
        ParameterField::WeaponSpAttackCategory,
        "EquipParamWeapon",
        "spAtkcategory",
        0,
        0xee,
        ScalarType::U16,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::EffectDuration,
        "SpEffectParam",
        "effectEndurance",
        15,
        0x8,
        ScalarType::F32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::EffectCategory,
        "SpEffectParam",
        "spCategory",
        15,
        0x13e,
        ScalarType::U16,
        FieldEvidence::DefinitionAndNative,
    ),
    FieldDefinition::new(
        ParameterField::EffectStateInfo,
        "SpEffectParam",
        "stateInfo",
        15,
        0x156,
        ScalarType::U16,
        FieldEvidence::DefinitionAndNative,
    ),
    FieldDefinition::new(
        ParameterField::NpcBehaviorVariation,
        "NpcParam",
        "behaviorVariationId",
        6,
        0x4,
        ScalarType::I32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::NpcLockCamera,
        "NpcParam",
        "lockCameraParamId",
        6,
        0x1e4,
        ScalarType::I32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::NpcHp,
        "NpcParam",
        "hp",
        6,
        0x24,
        ScalarType::U32,
        FieldEvidence::DefinitionAndNative,
    ),
    FieldDefinition::new(
        ParameterField::NpcMp,
        "NpcParam",
        "mp",
        6,
        0x28,
        ScalarType::U32,
        FieldEvidence::DefinitionAndNative,
    ),
    FieldDefinition::new(
        ParameterField::NpcStamina,
        "NpcParam",
        "stamina",
        6,
        0xf4,
        ScalarType::U16,
        FieldEvidence::DefinitionAndNative,
    ),
    FieldDefinition::new(
        ParameterField::CameraDistance,
        "LockCamParam",
        "camDistTarget",
        31,
        0x0,
        ScalarType::F32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::CameraHeight,
        "LockCamParam",
        "chrOrgOffset_Y",
        31,
        0xc,
        ScalarType::F32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::BuddyTriggerEffect,
        "BuddyParam",
        "triggerSpEffectId",
        128,
        0x4,
        ScalarType::I32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::BuddyNpc,
        "BuddyParam",
        "npcParamId",
        128,
        0x8,
        ScalarType::I32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::BuddyThink,
        "BuddyParam",
        "npcThinkParamId",
        128,
        0xc,
        ScalarType::I32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::BuddyRiddenNpc,
        "BuddyParam",
        "npcParamId_ridden",
        128,
        0x10,
        ScalarType::I32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::BuddyRiddenThink,
        "BuddyParam",
        "npcThinkParamId_ridden",
        128,
        0x14,
        ScalarType::I32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::EffectHpDelta,
        "SpEffectParam",
        "changeHpPoint",
        15,
        0xa0,
        ScalarType::I32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::EffectPoisonAttack,
        "SpEffectParam",
        "poizonAttackPower",
        15,
        0xcc,
        ScalarType::I32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::EffectDiseaseAttack,
        "SpEffectParam",
        "diseaseAttackPower",
        15,
        0xd0,
        ScalarType::I32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::EffectBloodAttack,
        "SpEffectParam",
        "bloodAttackPower",
        15,
        0xd4,
        ScalarType::I32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::EffectCurseAttack,
        "SpEffectParam",
        "curseAttackPower",
        15,
        0xd8,
        ScalarType::I32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::EffectFreezeAttack,
        "SpEffectParam",
        "freezeAttackPower",
        15,
        0x1a8,
        ScalarType::I32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::EffectSleepAttack,
        "SpEffectParam",
        "sleepAttackPower",
        15,
        0x338,
        ScalarType::I32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::EffectMadnessAttack,
        "SpEffectParam",
        "madnessAttackPower",
        15,
        0x33c,
        ScalarType::I32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::EffectSuperArmorDelta,
        "SpEffectParam",
        "changeSuperArmorPoint",
        15,
        0x354,
        ScalarType::F32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::EffectSaDelta,
        "SpEffectParam",
        "changeSaPoint",
        15,
        0x358,
        ScalarType::F32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::BuddyStoneEliminateTarget,
        "BuddyStoneParam",
        "eliminateTargetEntityId",
        134,
        0x8,
        ScalarType::U32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::BuddyStoneSummonedFlag,
        "BuddyStoneParam",
        "summonedEventFlagId",
        134,
        0xc,
        ScalarType::U32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::BuddyStoneActivateRegion,
        "BuddyStoneParam",
        "overwriteActivateRegionEntityId",
        134,
        0x20,
        ScalarType::U32,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::NpcAttackSfxDirection,
        "AtkParam_Npc",
        "atkDir_forSfx",
        7,
        0x80,
        ScalarType::I8,
        FieldEvidence::Definition,
    ),
    FieldDefinition::new(
        ParameterField::PlayerAttackSfxDirection,
        "AtkParam_Pc",
        "atkDir_forSfx",
        8,
        0x80,
        ScalarType::I8,
        FieldEvidence::Definition,
    ),
];

impl ParameterField {
    pub fn definition(self) -> &'static FieldDefinition {
        FIELD_DEFINITIONS
            .iter()
            .find(|field| field.field == self)
            .expect("all public fields have definitions")
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScalarValue {
    U8(u8),
    I8(i8),
    U16(u16),
    U32(u32),
    I32(i32),
    F32(f32),
}
impl ScalarValue {
    pub fn scalar_type(self) -> ScalarType {
        match self {
            Self::U8(_) => ScalarType::U8,
            Self::I8(_) => ScalarType::I8,
            Self::U16(_) => ScalarType::U16,
            Self::U32(_) => ScalarType::U32,
            Self::I32(_) => ScalarType::I32,
            Self::F32(_) => ScalarType::F32,
        }
    }

    pub fn bits(self) -> Result<u32> {
        Ok(match self {
            Self::U8(value) => u32::from(value),
            Self::I8(value) => u32::from(value as u8),
            Self::U16(value) => u32::from(value),
            Self::U32(value) => value,
            Self::I32(value) => value as u32,
            Self::F32(value) if value.is_finite() => value.to_bits(),
            Self::F32(_) => return Err(Error::InvalidProfile("parameter scalar is not finite")),
        })
    }

    pub(crate) fn decode(scalar: ScalarType, bytes: &[u8]) -> Result<Self> {
        if bytes.len() != scalar.width() {
            return Err(Error::InvalidProfile("parameter scalar width mismatch"));
        }
        Ok(match scalar {
            ScalarType::U8 => Self::U8(bytes[0]),
            ScalarType::I8 => Self::I8(bytes[0] as i8),
            ScalarType::U16 => {
                Self::U16(u16::from_le_bytes(bytes.try_into().expect("checked width")))
            }
            ScalarType::U32 => {
                Self::U32(u32::from_le_bytes(bytes.try_into().expect("checked width")))
            }
            ScalarType::I32 => {
                Self::I32(i32::from_le_bytes(bytes.try_into().expect("checked width")))
            }
            ScalarType::F32 => {
                let value = f32::from_le_bytes(bytes.try_into().expect("checked width"));
                if !value.is_finite() {
                    return Err(Error::InvalidProfile("parameter scalar is not finite"));
                }
                Self::F32(value)
            }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParameterFieldObservation {
    pub field: ParameterField,
    pub row_id: u32,
    pub row_address: u64,
    pub value: ScalarValue,
    pub evidence: FieldEvidence,
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Explicit experimental read. Definitions describe a pinned source, not the live PARAM
    /// data version, row allocation extent, atomic snapshot or gameplay acceptance.
    pub fn experimental_parameter_field(
        &self,
        field: ParameterField,
        variant: u32,
        id: u32,
        limit: usize,
    ) -> Result<Option<ParameterFieldObservation>> {
        let definition = field.definition();
        let Some(copy) = self.copy_parameter_row(
            ParameterSlot::new(definition.slot)?,
            variant,
            id,
            definition.offset + definition.scalar.width(),
            limit,
        )?
        else {
            return Ok(None);
        };
        let value = ScalarValue::decode(definition.scalar, &copy.bytes[definition.offset..])?;
        Ok(Some(ParameterFieldObservation {
            field,
            row_id: id,
            row_address: copy.row.address,
            value,
            evidence: definition.evidence,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_attack_direction_keeps_negative_values_and_exact_byte_extent() {
        for byte in 0..=255 {
            let value = ScalarValue::decode(ScalarType::I8, &[byte]).unwrap();
            assert_eq!(value, ScalarValue::I8(byte as i8));
            assert_eq!(value.bits().unwrap(), u32::from(byte));
            assert_eq!(value.scalar_type(), ScalarType::I8);
        }
        for field in [
            ParameterField::NpcAttackSfxDirection,
            ParameterField::PlayerAttackSfxDirection,
        ] {
            let definition = field.definition();
            assert_eq!(definition.scalar, ScalarType::I8);
            assert_eq!(definition.offset, 0x80);
        }
        for bytes in [&[][..], &[0xff, 0][..], &[0xff, 0, 0, 0][..]] {
            assert!(ScalarValue::decode(ScalarType::I8, bytes).is_err());
        }
    }
    #[test]
    fn byte_scalar_preserves_all_bits_and_refuses_incorrect_extent() {
        assert_eq!(ScalarType::U8.width(), 1);
        for byte in 0..=255 {
            let value = ScalarValue::decode(ScalarType::U8, &[byte]).unwrap();
            assert_eq!(value, ScalarValue::U8(byte));
            assert_eq!(value.bits().unwrap(), u32::from(byte));
            assert_eq!(value.scalar_type(), ScalarType::U8);
        }
        for bytes in [&[][..], &[1, 0][..], &[1, 0, 0, 0][..]] {
            assert!(ScalarValue::decode(ScalarType::U8, bytes).is_err());
        }
    }
}
