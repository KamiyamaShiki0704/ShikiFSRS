//! Selected WW storage facts reused from the independently reconstructed typed readers.
//! No complete C++ structure sizes, field references, or layouts from upstream libraries.
//! Descriptors alone grant no memory access, type, phase, lifetime, or mutation authority.
//! Callers first verify the exact WW file/loaded evidence and retain every native graph.
//! Middle collections keep storage names; gameplay-role aliases are consumer policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageField {
    pub offset: u32,
    pub width: u32,
}
impl StorageField {
    pub const fn end(self) -> u32 {
        self.offset + self.width
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeStorageField {
    CharacterEntry,
    CharacterIdentifier,
    CharacterController,
    CharacterModules,
    CharacterEffects,
    CharacterModel,
    CharacterNumber,
    CharacterRawKind,
    CharacterNpcRequest,
    CharacterStoredEvent,
    ControllerOwner,
    ControllerScaleX,
    ControllerScaleY,
    ControllerScaleZ,
    ControllerStorage,
    ControllerRagdoll,
    PhysicsOwner,
    PhysicsCharacterHeight,
    PhysicsCharacterRadius,
    PhysicsMapHeight,
    PhysicsMapRadius,
    PhysicsWeight,
    SetCapacity,
    SetEntries,
    WorldPlayers,
    WorldEmbedded10f38,
    WorldEmbedded10f90,
    WorldCreation,
    WorldOpenField,
    WorldIndexedSets,
    WorldPrimaryPlayer,
    ModulesPhysics,
}
impl NativeStorageField {
    pub const fn location(self) -> StorageField {
        match self {
            Self::CharacterEntry => StorageField {
                offset: crate::character_sets::CHARACTER_ENTRY_LINK_OFFSET,
                width: 8,
            },
            Self::CharacterIdentifier => StorageField {
                offset: crate::character_sets::CHARACTER_IDENTIFIER_OFFSET,
                width: 8,
            },
            Self::CharacterController => StorageField {
                offset: crate::character_controller::CONTROLLER_OFFSET,
                width: 8,
            },
            Self::CharacterModules => StorageField {
                offset: crate::character_modules::MODULE_CONTAINER_OFFSET,
                width: 8,
            },
            Self::CharacterEffects => StorageField {
                offset: crate::characters::EFFECT_CONTAINER_OFFSET,
                width: 8,
            },
            Self::CharacterModel => StorageField {
                offset: crate::character_visuals::MODEL_OFFSET,
                width: 8,
            },
            Self::CharacterNumber => StorageField {
                offset: crate::character_parameters::RAW_CHARACTER_NUMBER_OFFSET,
                width: 4,
            },
            Self::CharacterRawKind => StorageField {
                offset: crate::character_parameters::RAW_KIND_OFFSET,
                width: 4,
            },
            Self::CharacterNpcRequest => StorageField {
                offset: crate::character_parameters::STORED_NPC_REQUEST_ID_OFFSET,
                width: 4,
            },
            Self::CharacterStoredEvent => StorageField {
                offset: crate::character_events::STORED_EVENT_ID_OFFSET,
                width: 4,
            },
            Self::ControllerOwner => StorageField {
                offset: crate::character_controller::CONTROLLER_OWNER_OFFSET,
                width: 8,
            },
            Self::ControllerScaleX => StorageField {
                offset: crate::character_controller::AXIS_SCALE_OFFSET,
                width: 4,
            },
            Self::ControllerScaleY => StorageField {
                offset: crate::character_controller::AXIS_SCALE_OFFSET + 4,
                width: 4,
            },
            Self::ControllerScaleZ => StorageField {
                offset: crate::character_controller::AXIS_SCALE_OFFSET + 8,
                width: 4,
            },
            Self::ControllerStorage => StorageField {
                offset: crate::controller_storage::STORAGE_LINK_OFFSET,
                width: 8,
            },
            Self::ControllerRagdoll => StorageField {
                offset: crate::character_ragdoll::RAGDOLL_LINK_OFFSET,
                width: 8,
            },
            Self::PhysicsOwner => StorageField {
                offset: crate::character_modules::MODULE_OWNER_OFFSET,
                width: 8,
            },
            Self::PhysicsCharacterHeight => StorageField {
                offset: crate::character_physics::CHARACTER_COLLISION_OFFSET,
                width: 4,
            },
            Self::PhysicsCharacterRadius => StorageField {
                offset: crate::character_physics::CHARACTER_COLLISION_OFFSET + 4,
                width: 4,
            },
            Self::PhysicsMapHeight => StorageField {
                offset: crate::character_physics::MAP_COLLISION_OFFSET,
                width: 4,
            },
            Self::PhysicsMapRadius => StorageField {
                offset: crate::character_physics::MAP_COLLISION_OFFSET + 4,
                width: 4,
            },
            Self::PhysicsWeight => StorageField {
                offset: crate::character_physics::WEIGHT_OFFSET,
                width: 4,
            },
            Self::SetCapacity => StorageField {
                offset: crate::character_sets::SET_COUNT_OFFSET,
                width: 4,
            },
            Self::SetEntries => StorageField {
                offset: crate::character_sets::SET_ENTRIES_OFFSET,
                width: 8,
            },
            Self::WorldPlayers => StorageField {
                offset: crate::character_sets::WORLD_PLAYER_SET_OFFSET,
                width: 32,
            },
            Self::WorldEmbedded10f38 => StorageField {
                offset: crate::character_sets::WORLD_SECOND_SET_OFFSET,
                width: 32,
            },
            Self::WorldEmbedded10f90 => StorageField {
                offset: crate::character_sets::WORLD_THIRD_SET_OFFSET,
                width: 32,
            },
            Self::WorldCreation => StorageField {
                offset: crate::character_sets::WORLD_CREATION_SET_OFFSET,
                width: 32,
            },
            Self::WorldOpenField => StorageField {
                offset: crate::character_sets::WORLD_OPEN_FIELD_SET_OFFSET,
                width: 32,
            },
            Self::WorldIndexedSets => StorageField {
                offset: crate::character_sets::WORLD_INDEXED_SETS_OFFSET,
                width: 1576,
            },
            Self::WorldPrimaryPlayer => StorageField {
                offset: crate::objects::PRIMARY_PLAYER_OFFSET,
                width: 8,
            },
            Self::ModulesPhysics => StorageField {
                offset: crate::character_modules::PHYSICS_INDEX * 8,
                width: 8,
            },
        }
    }
}
// These are the smallest extents covering selected descriptor fields, never C++ sizeof.
pub const CHARACTER_SELECTED_EXTENT: usize =
    NativeStorageField::CharacterStoredEvent.location().end() as usize;
pub const CONTROLLER_SELECTED_EXTENT: usize =
    NativeStorageField::ControllerScaleZ.location().end() as usize;
pub const PHYSICS_SELECTED_EXTENT: usize =
    NativeStorageField::PhysicsMapRadius.location().end() as usize;
pub const MODULES_SELECTED_EXTENT: usize =
    NativeStorageField::ModulesPhysics.location().end() as usize;
pub const WORLD_SELECTED_EXTENT: usize =
    NativeStorageField::WorldPrimaryPlayer.location().end() as usize;
pub const PLAYER_NPC_SELECTED_EXTENT: usize =
    crate::character_parameters::PLAYER_NPC_HANDLE_OFFSET as usize + 8;
pub const SET_SELECTED_EXTENT: usize = NativeStorageField::SetEntries.location().end() as usize;
pub const SET_ENTRY_STRIDE: usize = crate::character_sets::SET_ENTRY_STRIDE as usize;
pub const WORLD_INDEXED_SET_COUNT: usize = crate::character_sets::WORLD_INDEXED_SET_COUNT as usize;
pub const WORLD_CHARACTERS_GLOBAL: u32 = crate::objects::WORLD_CHARACTERS_GLOBAL;
