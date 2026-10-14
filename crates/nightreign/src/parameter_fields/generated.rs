// Pinned NR definition facts. Runtime schema acceptance remains experimental.
use super::{Field, FieldDefinition};
pub(crate) static DEFINITIONS: &[FieldDefinition] = &[
    FieldDefinition {
        table: "NpcParam",
        native_name: "behaviorVariationId",
        param_type: "NPC_PARAM_ST",
        slot: 6,
        offset: 4,
        width: 4,
        bit: None,
        data_version: 9,
        packed_size: 816,
    },
    FieldDefinition {
        table: "NpcParam",
        native_name: "hitHeight",
        param_type: "NPC_PARAM_ST",
        slot: 6,
        offset: 20,
        width: 4,
        bit: None,
        data_version: 9,
        packed_size: 816,
    },
    FieldDefinition {
        table: "NpcParam",
        native_name: "hitRadius",
        param_type: "NPC_PARAM_ST",
        slot: 6,
        offset: 24,
        width: 4,
        bit: None,
        data_version: 9,
        packed_size: 816,
    },
    FieldDefinition {
        table: "NpcParam",
        native_name: "weight",
        param_type: "NPC_PARAM_ST",
        slot: 6,
        offset: 28,
        width: 4,
        bit: None,
        data_version: 9,
        packed_size: 816,
    },
    FieldDefinition {
        table: "NpcParam",
        native_name: "hp",
        param_type: "NPC_PARAM_ST",
        slot: 6,
        offset: 36,
        width: 4,
        bit: None,
        data_version: 9,
        packed_size: 816,
    },
    FieldDefinition {
        table: "NpcParam",
        native_name: "mp",
        param_type: "NPC_PARAM_ST",
        slot: 6,
        offset: 40,
        width: 4,
        bit: None,
        data_version: 9,
        packed_size: 816,
    },
    FieldDefinition {
        table: "NpcParam",
        native_name: "stamina",
        param_type: "NPC_PARAM_ST",
        slot: 6,
        offset: 244,
        width: 2,
        bit: None,
        data_version: 9,
        packed_size: 816,
    },
    FieldDefinition {
        table: "NpcParam",
        native_name: "teamType",
        param_type: "NPC_PARAM_ST",
        slot: 6,
        offset: 307,
        width: 1,
        bit: None,
        data_version: 9,
        packed_size: 816,
    },
    FieldDefinition {
        table: "NpcParam",
        native_name: "chrHitHeight",
        param_type: "NPC_PARAM_ST",
        slot: 6,
        offset: 340,
        width: 4,
        bit: None,
        data_version: 9,
        packed_size: 816,
    },
    FieldDefinition {
        table: "NpcParam",
        native_name: "chrHitRadius",
        param_type: "NPC_PARAM_ST",
        slot: 6,
        offset: 344,
        width: 4,
        bit: None,
        data_version: 9,
        packed_size: 816,
    },
    FieldDefinition {
        table: "NpcParam",
        native_name: "isGhost",
        param_type: "NPC_PARAM_ST",
        slot: 6,
        offset: 329,
        width: 1,
        bit: Some(4),
        data_version: 9,
        packed_size: 816,
    },
    FieldDefinition {
        table: "EquipParamWeapon",
        native_name: "behaviorVariationId",
        param_type: "EQUIP_PARAM_WEAPON_ST",
        slot: 0,
        offset: 4,
        width: 4,
        bit: None,
        data_version: 6,
        packed_size: 680,
    },
    FieldDefinition {
        table: "EquipParamWeapon",
        native_name: "weight",
        param_type: "EQUIP_PARAM_WEAPON_ST",
        slot: 0,
        offset: 16,
        width: 4,
        bit: None,
        data_version: 6,
        packed_size: 680,
    },
    FieldDefinition {
        table: "EquipParamWeapon",
        native_name: "weaponWeightRate",
        param_type: "EQUIP_PARAM_WEAPON_ST",
        slot: 0,
        offset: 20,
        width: 4,
        bit: None,
        data_version: 6,
        packed_size: 680,
    },
    FieldDefinition {
        table: "EquipParamWeapon",
        native_name: "attackBasePhysics",
        param_type: "EQUIP_PARAM_WEAPON_ST",
        slot: 0,
        offset: 200,
        width: 2,
        bit: None,
        data_version: 6,
        packed_size: 680,
    },
    FieldDefinition {
        table: "EquipParamWeapon",
        native_name: "attackBaseMagic",
        param_type: "EQUIP_PARAM_WEAPON_ST",
        slot: 0,
        offset: 202,
        width: 2,
        bit: None,
        data_version: 6,
        packed_size: 680,
    },
    FieldDefinition {
        table: "EquipParamWeapon",
        native_name: "attackBaseFire",
        param_type: "EQUIP_PARAM_WEAPON_ST",
        slot: 0,
        offset: 204,
        width: 2,
        bit: None,
        data_version: 6,
        packed_size: 680,
    },
    FieldDefinition {
        table: "EquipParamWeapon",
        native_name: "attackBaseThunder",
        param_type: "EQUIP_PARAM_WEAPON_ST",
        slot: 0,
        offset: 206,
        width: 2,
        bit: None,
        data_version: 6,
        packed_size: 680,
    },
    FieldDefinition {
        table: "EquipParamWeapon",
        native_name: "attackBaseStamina",
        param_type: "EQUIP_PARAM_WEAPON_ST",
        slot: 0,
        offset: 208,
        width: 2,
        bit: None,
        data_version: 6,
        packed_size: 680,
    },
    FieldDefinition {
        table: "EquipParamWeapon",
        native_name: "staminaGuardDef",
        param_type: "EQUIP_PARAM_WEAPON_ST",
        slot: 0,
        offset: 216,
        width: 2,
        bit: None,
        data_version: 6,
        packed_size: 680,
    },
    FieldDefinition {
        table: "EquipParamWeapon",
        native_name: "rightHandEquipable",
        param_type: "EQUIP_PARAM_WEAPON_ST",
        slot: 0,
        offset: 261,
        width: 1,
        bit: Some(0),
        data_version: 6,
        packed_size: 680,
    },
    FieldDefinition {
        table: "EquipParamWeapon",
        native_name: "enableGuard",
        param_type: "EQUIP_PARAM_WEAPON_ST",
        slot: 0,
        offset: 261,
        width: 1,
        bit: Some(5),
        data_version: 6,
        packed_size: 680,
    },
    FieldDefinition {
        table: "LockCamParam",
        native_name: "camDistTarget",
        param_type: "LOCK_CAM_PARAM_ST",
        slot: 32,
        offset: 0,
        width: 4,
        bit: None,
        data_version: 1,
        packed_size: 128,
    },
    FieldDefinition {
        table: "LockCamParam",
        native_name: "chrOrgOffset_Y",
        param_type: "LOCK_CAM_PARAM_ST",
        slot: 32,
        offset: 12,
        width: 4,
        bit: None,
        data_version: 1,
        packed_size: 128,
    },
    FieldDefinition {
        table: "LockCamParam",
        native_name: "chrLockRangeMaxRadius",
        param_type: "LOCK_CAM_PARAM_ST",
        slot: 32,
        offset: 16,
        width: 4,
        bit: None,
        data_version: 1,
        packed_size: 128,
    },
    FieldDefinition {
        table: "LockCamParam",
        native_name: "camFovY",
        param_type: "LOCK_CAM_PARAM_ST",
        slot: 32,
        offset: 20,
        width: 4,
        bit: None,
        data_version: 1,
        packed_size: 128,
    },
    FieldDefinition {
        table: "LockCamParam",
        native_name: "chrLockRangeMaxRadius_forD",
        param_type: "LOCK_CAM_PARAM_ST",
        slot: 32,
        offset: 24,
        width: 4,
        bit: None,
        data_version: 1,
        packed_size: 128,
    },
    FieldDefinition {
        table: "LockCamParam",
        native_name: "chrLockRangeMaxRadius_forPD",
        param_type: "LOCK_CAM_PARAM_ST",
        slot: 32,
        offset: 28,
        width: 4,
        bit: None,
        data_version: 1,
        packed_size: 128,
    },
    FieldDefinition {
        table: "AtkParam_Npc",
        native_name: "hit0_Radius",
        param_type: "ATK_PARAM_ST",
        slot: 7,
        offset: 0,
        width: 4,
        bit: None,
        data_version: 5,
        packed_size: 464,
    },
    FieldDefinition {
        table: "AtkParam_Npc",
        native_name: "hitStopTime",
        param_type: "ATK_PARAM_ST",
        slot: 7,
        offset: 20,
        width: 4,
        bit: None,
        data_version: 5,
        packed_size: 464,
    },
    FieldDefinition {
        table: "AtkParam_Npc",
        native_name: "spEffectId0",
        param_type: "ATK_PARAM_ST",
        slot: 7,
        offset: 24,
        width: 4,
        bit: None,
        data_version: 5,
        packed_size: 464,
    },
    FieldDefinition {
        table: "AtkParam_Npc",
        native_name: "atkDir_forSfx",
        param_type: "ATK_PARAM_ST",
        slot: 7,
        offset: 128,
        width: 1,
        bit: None,
        data_version: 5,
        packed_size: 464,
    },
    FieldDefinition {
        table: "AtkParam_Npc",
        native_name: "disableStaminaAttack",
        param_type: "ATK_PARAM_ST",
        slot: 7,
        offset: 126,
        width: 1,
        bit: Some(1),
        data_version: 5,
        packed_size: 464,
    },
    FieldDefinition {
        table: "AtkParam_Pc",
        native_name: "hit0_Radius",
        param_type: "ATK_PARAM_ST",
        slot: 8,
        offset: 0,
        width: 4,
        bit: None,
        data_version: 5,
        packed_size: 464,
    },
    FieldDefinition {
        table: "AtkParam_Pc",
        native_name: "hitStopTime",
        param_type: "ATK_PARAM_ST",
        slot: 8,
        offset: 20,
        width: 4,
        bit: None,
        data_version: 5,
        packed_size: 464,
    },
    FieldDefinition {
        table: "AtkParam_Pc",
        native_name: "spEffectId0",
        param_type: "ATK_PARAM_ST",
        slot: 8,
        offset: 24,
        width: 4,
        bit: None,
        data_version: 5,
        packed_size: 464,
    },
    FieldDefinition {
        table: "AtkParam_Pc",
        native_name: "atkDir_forSfx",
        param_type: "ATK_PARAM_ST",
        slot: 8,
        offset: 128,
        width: 1,
        bit: None,
        data_version: 5,
        packed_size: 464,
    },
    FieldDefinition {
        table: "AtkParam_Pc",
        native_name: "disableStaminaAttack",
        param_type: "ATK_PARAM_ST",
        slot: 8,
        offset: 126,
        width: 1,
        bit: Some(1),
        data_version: 5,
        packed_size: 464,
    },
    FieldDefinition {
        table: "CharaInitParam",
        native_name: "soul",
        param_type: "CHARACTER_INIT_PARAM",
        slot: 24,
        offset: 0,
        width: 4,
        bit: None,
        data_version: 3,
        packed_size: 320,
    },
    FieldDefinition {
        table: "CharaInitParam",
        native_name: "equip_Wep_Right_1",
        param_type: "CHARACTER_INIT_PARAM",
        slot: 24,
        offset: 4,
        width: 4,
        bit: None,
        data_version: 3,
        packed_size: 320,
    },
    FieldDefinition {
        table: "CharaInitParam",
        native_name: "equip_Helm",
        param_type: "CHARACTER_INIT_PARAM",
        slot: 24,
        offset: 28,
        width: 4,
        bit: None,
        data_version: 3,
        packed_size: 320,
    },
];
pub mod npc {
    use super::{DEFINITIONS, Field};
    pub const BEHAVIOR_VARIATION_ID: Field<i32> = Field::new(&DEFINITIONS[0]);
    pub const HIT_HEIGHT: Field<f32> = Field::new(&DEFINITIONS[1]);
    pub const HIT_RADIUS: Field<f32> = Field::new(&DEFINITIONS[2]);
    pub const WEIGHT: Field<u32> = Field::new(&DEFINITIONS[3]);
    pub const HP: Field<u32> = Field::new(&DEFINITIONS[4]);
    pub const MP: Field<u32> = Field::new(&DEFINITIONS[5]);
    pub const STAMINA: Field<u16> = Field::new(&DEFINITIONS[6]);
    pub const TEAM_TYPE: Field<u8> = Field::new(&DEFINITIONS[7]);
    pub const CHR_HIT_HEIGHT: Field<f32> = Field::new(&DEFINITIONS[8]);
    pub const CHR_HIT_RADIUS: Field<f32> = Field::new(&DEFINITIONS[9]);
    pub const IS_GHOST: Field<bool> = Field::new(&DEFINITIONS[10]);
}
pub mod weapon {
    use super::{DEFINITIONS, Field};
    pub const BEHAVIOR_VARIATION_ID: Field<i32> = Field::new(&DEFINITIONS[11]);
    pub const WEIGHT: Field<f32> = Field::new(&DEFINITIONS[12]);
    pub const WEAPON_WEIGHT_RATE: Field<f32> = Field::new(&DEFINITIONS[13]);
    pub const ATTACK_BASE_PHYSICS: Field<u16> = Field::new(&DEFINITIONS[14]);
    pub const ATTACK_BASE_MAGIC: Field<u16> = Field::new(&DEFINITIONS[15]);
    pub const ATTACK_BASE_FIRE: Field<u16> = Field::new(&DEFINITIONS[16]);
    pub const ATTACK_BASE_THUNDER: Field<u16> = Field::new(&DEFINITIONS[17]);
    pub const ATTACK_BASE_STAMINA: Field<u16> = Field::new(&DEFINITIONS[18]);
    pub const STAMINA_GUARD_DEF: Field<i16> = Field::new(&DEFINITIONS[19]);
    pub const RIGHT_HAND_EQUIPABLE: Field<bool> = Field::new(&DEFINITIONS[20]);
    pub const ENABLE_GUARD: Field<bool> = Field::new(&DEFINITIONS[21]);
}
pub mod camera {
    use super::{DEFINITIONS, Field};
    pub const CAM_DIST_TARGET: Field<f32> = Field::new(&DEFINITIONS[22]);
    pub const CHR_ORG_OFFSET_Y: Field<f32> = Field::new(&DEFINITIONS[23]);
    pub const CHR_LOCK_RANGE_MAX_RADIUS: Field<f32> = Field::new(&DEFINITIONS[24]);
    pub const CAM_FOV_Y: Field<f32> = Field::new(&DEFINITIONS[25]);
    pub const CHR_LOCK_RANGE_MAX_RADIUS_FOR_D: Field<f32> = Field::new(&DEFINITIONS[26]);
    pub const CHR_LOCK_RANGE_MAX_RADIUS_FOR_PD: Field<f32> = Field::new(&DEFINITIONS[27]);
}
pub mod npc_attack {
    use super::{DEFINITIONS, Field};
    pub const HIT0_RADIUS: Field<f32> = Field::new(&DEFINITIONS[28]);
    pub const HIT_STOP_TIME: Field<f32> = Field::new(&DEFINITIONS[29]);
    pub const SP_EFFECT_ID0: Field<i32> = Field::new(&DEFINITIONS[30]);
    pub const ATK_DIR_FOR_SFX: Field<i8> = Field::new(&DEFINITIONS[31]);
    pub const DISABLE_STAMINA_ATTACK: Field<bool> = Field::new(&DEFINITIONS[32]);
}
pub mod player_attack {
    use super::{DEFINITIONS, Field};
    pub const HIT0_RADIUS: Field<f32> = Field::new(&DEFINITIONS[33]);
    pub const HIT_STOP_TIME: Field<f32> = Field::new(&DEFINITIONS[34]);
    pub const SP_EFFECT_ID0: Field<i32> = Field::new(&DEFINITIONS[35]);
    pub const ATK_DIR_FOR_SFX: Field<i8> = Field::new(&DEFINITIONS[36]);
    pub const DISABLE_STAMINA_ATTACK: Field<bool> = Field::new(&DEFINITIONS[37]);
}
pub mod initial {
    use super::{DEFINITIONS, Field};
    pub const SOUL: Field<i32> = Field::new(&DEFINITIONS[38]);
    pub const EQUIP_WEP_RIGHT_1: Field<i32> = Field::new(&DEFINITIONS[39]);
    pub const EQUIP_HELM: Field<i32> = Field::new(&DEFINITIONS[40]);
}
