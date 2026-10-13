//! Checked, copied observations of independently identified game objects.
use crate::Ww171File;
use shikifsrs_runtime::{
    Error, Result,
    code_patches::OwnedCodePatches,
    memory::AddressSpace,
    profile::{CodeRange, VerifiedModule},
};
static OBJECT_FILE_METADATA: crate::file_metadata::FileMetadataGate =
    crate::file_metadata::FileMetadataGate::new(crate::WW_171);

const OBJECT_TYPES: &[(ObjectKind, &str, u32)] = &[
    (
        ObjectKind::CharacterResistance,
        ".?AVCSChrResistModule@CS@@",
        0x2a391d0,
    ),
    (
        ObjectKind::CharacterTimeAct,
        ".?AVCSChrTimeActModule@CS@@",
        0x2a37670,
    ),
    (
        ObjectKind::CharacterSuperArmor,
        ".?AVCSChrSuperArmorModule@CS@@",
        0x2a3dc58,
    ),
    (
        ObjectKind::EnemySuperArmor,
        ".?AVCSEnemySuperArmorModule@CS@@",
        0x2a3de00,
    ),
    (
        ObjectKind::PlayerSuperArmor,
        ".?AVCSPlayerSuperArmorModule@CS@@",
        0x2a3df98,
    ),
    (
        ObjectKind::CharacterToughness,
        ".?AVCSChrToughnessModule@CS@@",
        0x2a3ebc0,
    ),
    (
        ObjectKind::EnemyToughness,
        ".?AVCSEnemyToughnessModule@CS@@",
        0x2a3ed70,
    ),
    (
        ObjectKind::PlayerToughness,
        ".?AVCSPlayerToughnessModule@CS@@",
        0x2a3ef30,
    ),
    (
        ObjectKind::CharacterActionFlags,
        ".?AVCSChrActionFlagModule@CS@@",
        0x2a32438,
    ),
    (ObjectKind::CharacterCamera, ".?AVChrCam@CS@@", 0x2a2aa08),
    (
        ObjectKind::CharacterFollowCamera,
        ".?AVChrExFollowCam@CS@@",
        0x2a2aca0,
    ),
    (
        ObjectKind::CharacterAimCamera,
        ".?AVChrAimCam@CS@@",
        0x2a2a7f0,
    ),
    (
        ObjectKind::DistanceViewCamera,
        ".?AVCSDistViewCam@CS@@",
        0x2a2bad0,
    ),
    (
        ObjectKind::CharacterActionRequests,
        ".?AVCSChrActionRequestModule@CS@@",
        0x2a32780,
    ),
    (
        ObjectKind::CharacterTalk,
        ".?AVCSChrTalkModule@CS@@",
        0x2a39350,
    ),
    (ObjectKind::CharacterResources, ".?AVChrRes@CS@@", 0x2a400c8),
    (ObjectKind::FrontendView, ".?AVFrontEndView@CS@@", 0x2aed8e0),
    (
        ObjectKind::SummonMessage,
        ".?AVSummonMsgData@CS@@",
        0x2ad0488,
    ),
    (
        ObjectKind::SummonMessageQueue,
        ".?AVSummonMsgQueue@CS@@",
        0x2ad0618,
    ),
    (
        ObjectKind::SummonMessageList,
        ".?AV?$CSFixedList@VSummonMsgData@CS@@$03@CS@@",
        0x2ad0608,
    ),
    (
        ObjectKind::BulletTargetingOwner,
        ".?AVCSBulletTargetingSystemOwner@CS@@",
        0x2a2a560,
    ),
    (ObjectKind::Bullet, ".?AVCSBulletIns@CS@@", 0x2a28ec0),
    (
        ObjectKind::BulletWaitState,
        ".?AVCSBulletWaitState@CS@@",
        0x2a2a4b0,
    ),
    (
        ObjectKind::BulletFlyState,
        ".?AVCSBulletFlyState@CS@@",
        0x2a28530,
    ),
    (
        ObjectKind::BulletExplosionState,
        ".?AVCSBulletExplosionState@CS@@",
        0x2a28470,
    ),
    (
        ObjectKind::CharacterDamage,
        ".?AVCSChrDamageModule@CS@@",
        0x2a39a90,
    ),
    (
        ObjectKind::EnemyDamage,
        ".?AVCSEnemyDamageModule@CS@@",
        0x2a3a0d8,
    ),
    (
        ObjectKind::PlayerDamage,
        ".?AVCSPlayerDamageModule@CS@@",
        0x2a3a3e0,
    ),
    (
        ObjectKind::CharacterEvent,
        ".?AVCSChrEventModule@CS@@",
        0x2a38368,
    ),
    (
        ObjectKind::DebugCharacterCreator,
        ".?AVCSDebugChrCreator@CS@@",
        0x2a51220,
    ),
    (
        ObjectKind::DebugWorld,
        ".?AVWorldChrManDbgImp@CS@@",
        0x2a4ec48,
    ),
    (ObjectKind::MenuManager, ".?AVCSMenuManImp@CS@@", 0x2a9eed8),
    (ObjectKind::Frontend, ".?AVCSFeManImp@CS@@", 0x2aa0a08),
    (
        ObjectKind::LoadingHelper,
        ".?AVCSNowLoadingHelperImp@CS@@",
        0x29fc918,
    ),
    (
        ObjectKind::CharacterManipulator,
        ".?AVChrManipulator@CS@@",
        0x2a2c8e8,
    ),
    (
        ObjectKind::CommandManipulator,
        ".?AVComManipulator@CS@@",
        0x2a2cd70,
    ),
    (
        ObjectKind::FollowManipulator,
        ".?AVFollowManipulator@CS@@",
        0x2a2d400,
    ),
    (
        ObjectKind::NetworkAiManipulator,
        ".?AVNetAIManipulator@CS@@",
        0x2a2d858,
    ),
    (
        ObjectKind::NetworkManipulator,
        ".?AVNetworkManipulator@CS@@",
        0x2a2e1b8,
    ),
    (
        ObjectKind::PadManipulator,
        ".?AVPadManipulator@CS@@",
        0x2a2e788,
    ),
    (
        ObjectKind::ReplayManipulator,
        ".?AVReplayManipulator@CS@@",
        0x2a2eda0,
    ),
    (
        ObjectKind::RideManipulator,
        ".?AVRideManipulator@CS@@",
        0x2a2f118,
    ),
    (ObjectKind::CameraManager, ".?AVCSCameraImp@CS@@", 0x2a2aff0),
    (ObjectKind::Window, ".?AVCSWindowImp@CS@@", 0x2c011e8),
    (ObjectKind::GameManager, ".?AVGameMan@CS@@", 0x2a82110),
    (
        ObjectKind::HavokManager,
        ".?AVCSHavokManImp@CS@@",
        0x2b934e8,
    ),
    (ObjectKind::PhysicsWorld, ".?AVCSPhysWorld@CS@@", 0x2b96978),
    (
        ObjectKind::TeamNeutral,
        ".?AVCSTeamTypeNeutral@@",
        0x2a4f868,
    ),
    (ObjectKind::TeamFriend, ".?AVCSTeamTypeFriend@@", 0x2a4f878),
    (ObjectKind::TeamEnemy, ".?AVCSTeamTypeEnemy@@", 0x2a4f888),
    (ObjectKind::TeamRival, ".?AVCSTemaTypeRival@@", 0x2a4f898),
    (
        ObjectKind::TemporaryTeamSlot,
        ".?AVChrTemporaryTeamTypeSlot@CS@@",
        0x2a44410,
    ),
    (ObjectKind::HavokWorld, ".?AVhknpWorld@@", 0x2ef0dd8),
    (
        ObjectKind::PerspectiveCamera,
        ".?AVCSPersCam@CS@@",
        0x2aa0a40,
    ),
    (
        ObjectKind::CharacterRideNode,
        ".?AVCSRideNode@CS@@",
        0x2a3d458,
    ),
    (
        ObjectKind::FileResourceManager,
        ".?AVCSFileImp@CS@@",
        0x29d4800,
    ),
    (
        ObjectKind::WorldSceneDrawParameters,
        ".?AVCSWorldSceneDrawParamManagerImp@CS@@",
        0x2b583c0,
    ),
    (
        ObjectKind::GparamRepository,
        ".?AVCSGparamRepositoryImp@CS@@",
        0x2b73788,
    ),
    (
        ObjectKind::PlayerGameData,
        ".?AVPlayerGameData@CS@@",
        0x29e8fa8,
    ),
    (
        ObjectKind::VirtualMemoryFlags,
        ".?AVCSFD4VirtualMemoryFlag@CS@@",
        0x2a74660,
    ),
    (
        ObjectKind::CharacterModel,
        ".?AVCSChrModelIns@CS@@",
        0x2b35c68,
    ),
    (
        ObjectKind::PoseImporter,
        ".?AVCSFD4LocationHkaPoseImporter@CS@@",
        0x2b70360,
    ),
    (
        ObjectKind::SkeletonMapper,
        ".?AVCSFD4LocationAnimSkeletonToMdlObjIdxModifier@CS@@",
        0x2b6f1a0,
    ),
    (
        ObjectKind::AnimationSkeleton,
        ".?AVhkaSkeleton@@",
        0x2b4c750,
    ),
    (
        ObjectKind::CharacterController,
        ".?AVChrCtrl@CS@@",
        0x2a2c6a0,
    ),
    (
        ObjectKind::CharacterRagdoll,
        ".?AVCSRagdollIns@CS@@",
        0x2b97168,
    ),
    (ObjectKind::EnemyController, ".?AVEnemyCtrl@CS@@", 0x2a2d200),
    (
        ObjectKind::PlayerController,
        ".?AVPlayerCtrl@CS@@",
        0x2a2eb10,
    ),
    (
        ObjectKind::CharacterPhysics,
        ".?AVCSChrPhysicsModule@CS@@",
        0x2a3c890,
    ),
    (
        ObjectKind::CharacterRide,
        ".?AVCSChrRideModule@CS@@",
        0x2a3d098,
    ),
    (
        ObjectKind::CharacterModelParameterModifier,
        ".?AVCSChrModelParamModifierModule@CS@@",
        0x2a38608,
    ),
    (
        ObjectKind::CharacterTaskManager,
        ".?AVCSTaskImp@CS@@",
        0x2c04b00,
    ),
    (
        ObjectKind::CharacterTask,
        ".?AV?$CSEzVoidTask@VCSEzRabbitNoUpdateTask@CS@@VChrIns@2@@CS@@",
        0x2a31090,
    ),
    (
        ObjectKind::CharacterTaskProxy,
        ".?AVCSEzTaskProxy@CS@@",
        0x2c049f8,
    ),
    (ObjectKind::Character, ".?AVChrIns@CS@@", 0x2a310c8),
    (ObjectKind::Enemy, ".?AVEnemyIns@CS@@", 0x2a47090),
    (ObjectKind::Player, ".?AVPlayerIns@CS@@", 0x2a7fbb0),
    (ObjectKind::CharacterSet, ".?AVChrSet@CS@@", 0x2a41348),
    (
        ObjectKind::OpenFieldCharacterSet,
        ".?AVOpenFieldChrSet@CS@@",
        0x2a4b620,
    ),
    (
        ObjectKind::CharacterData,
        ".?AVCSChrDataModule@CS@@",
        0x2a380b8,
    ),
    (
        ObjectKind::EffectContainer,
        ".?AVSpecialEffect@CS@@",
        0x2a4d490,
    ),
    (
        ObjectKind::WorldCharacters,
        ".?AVWorldChrManImp@CS@@",
        0x2a4e980,
    ),
    (
        ObjectKind::SoloParameters,
        ".?AVSoloParamRepositoryImp@CS@@",
        0x2bb84c8,
    ),
    (
        ObjectKind::ParameterResource,
        ".?AVParamResCap@CS@@",
        0x2bb29f8,
    ),
];

pub(crate) const OBJECT_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3e6e30,
        length: 0x99f,
    },
    CodeRange {
        rva: 0x4cee70,
        length: 0x5f8,
    },
    CodeRange {
        rva: 0x650c90,
        length: 0x85f,
    },
    CodeRange {
        rva: 0x504c40,
        length: 0x1372,
    },
    CodeRange {
        rva: 0xd4dac0,
        length: 0x2d5,
    },
    CodeRange {
        rva: 0xd22d00,
        length: 0x90,
    },
    CodeRange {
        rva: 0xaeeb30,
        length: 0x667,
    },
    CodeRange {
        rva: 0xd21fd0,
        length: 0x16e,
    },
    CodeRange {
        rva: 0xe3c4f0,
        length: 0x3e,
    },
];

pub(crate) const WORLD_CHARACTERS_GLOBAL: u32 = 0x3d69ff8;
const SOLO_PARAMETERS_GLOBAL: u32 = 0x3d85f58;
pub(crate) const PRIMARY_PLAYER_OFFSET: u32 = 0x1e508;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    CharacterResistance,
    CharacterTimeAct,
    CharacterSuperArmor,
    EnemySuperArmor,
    PlayerSuperArmor,
    CharacterToughness,
    EnemyToughness,
    PlayerToughness,

    CharacterActionFlags,
    CharacterCamera,
    CharacterFollowCamera,
    CharacterAimCamera,
    DistanceViewCamera,
    CharacterActionRequests,
    CharacterTalk,
    CharacterResources,
    FrontendView,
    SummonMessage,
    SummonMessageQueue,
    SummonMessageList,
    BulletTargetingOwner,
    Bullet,
    BulletWaitState,
    BulletFlyState,
    BulletExplosionState,
    CharacterDamage,
    EnemyDamage,
    PlayerDamage,
    CharacterEvent,
    DebugCharacterCreator,
    DebugWorld,
    MenuManager,
    Frontend,
    LoadingHelper,
    CharacterManipulator,
    CommandManipulator,
    FollowManipulator,
    NetworkAiManipulator,
    NetworkManipulator,
    PadManipulator,
    ReplayManipulator,
    RideManipulator,

    CameraManager,
    Window,
    GameManager,
    HavokManager,
    PhysicsWorld,
    HavokWorld,
    TeamNeutral,
    TeamFriend,
    TeamEnemy,
    TeamRival,
    TemporaryTeamSlot,
    PerspectiveCamera,
    FileResourceManager,
    WorldSceneDrawParameters,
    GparamRepository,
    PlayerGameData,
    VirtualMemoryFlags,
    CharacterModel,
    PoseImporter,
    SkeletonMapper,
    AnimationSkeleton,
    CharacterController,
    CharacterRagdoll,
    EnemyController,
    PlayerController,
    CharacterPhysics,
    CharacterRide,
    CharacterRideNode,
    CharacterModelParameterModifier,
    CharacterTaskManager,
    CharacterTask,
    CharacterTaskProxy,
    Character,
    Enemy,
    Player,
    CharacterSet,
    OpenFieldCharacterSet,
    CharacterData,
    EffectContainer,
    WorldCharacters,
    SoloParameters,
    ParameterResource,
}

/// Copied address/type only; no object lifetime or local/remote-player classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectObservation {
    pub address: u64,
    pub kind: ObjectKind,
}

/// Proven initialized fields only, without assigning an unproven loaded/ready meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterResourceObservation {
    pub address: u64,
    pub native_storage_tag: u32,
    /// Raw +0x80 constructor argument. Not interpreted or dereferenced.
    pub native_constructor_argument: u64,
}

pub struct GameObjectCatalog<'b, 'a, M> {
    pub(crate) module: VerifiedModule<'b, 'a, M>,
}

impl<'a> Ww171File<'a> {
    /// Verify independent primary-vtable RTTI facts from the exact file profile.
    pub fn object_types(&self) -> Result<Vec<(ObjectKind, String)>> {
        self.verify_object_types()?;
        Ok(OBJECT_TYPES
            .iter()
            .map(|(kind, name, _)| (*kind, (*name).to_owned()))
            .collect())
    }
    fn verify_object_types(&self) -> Result<()> {
        OBJECT_FILE_METADATA.verify(self.file(), || {
            for (_, name, expected) in OBJECT_TYPES {
                let tables: Vec<_> = self
                    .file()
                    .image()
                    .vtables(name)?
                    .into_iter()
                    .filter(|v| v.object_offset == 0)
                    .collect();
                if tables.len() != 1 || tables[0].table_rva != *expected {
                    return Err(Error::InvalidProfile(
                        "character/parameter RTTI differs from the independent profile",
                    ));
                }
            }
            Ok(())
        })
    }

    pub fn bind_objects<'b, M: AddressSpace>(
        &'b self,
        memory: &'b M,
        base: u64,
    ) -> Result<GameObjectCatalog<'b, 'a, M>> {
        self.bind_objects_with_patches(memory, base, None)
    }

    /// Opt in to a file/base-bound owned hook manifest, leaving other checks strict.
    pub fn bind_objects_with_patches<'b, M: AddressSpace>(
        &'b self,
        memory: &'b M,
        base: u64,
        patches: Option<&'b OwnedCodePatches>,
    ) -> Result<GameObjectCatalog<'b, 'a, M>> {
        self.verify_object_types()?;
        let module = self
            .file()
            .bind_with_patches(memory, base, OBJECT_EVIDENCE, patches)?;
        for range in crate::bullets::BULLET_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::summon_hud::SUMMON_HUD_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::friendly_hud::FRIENDLY_HUD_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::bullet_spawn::SPAWN_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_control_channels::CONTROL_CHANNEL_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::hud_hook_sites::HUD_HOOK_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::menu_state::MENU_STATE_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_spatial::SPATIAL_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::cameras::CAMERA_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::camera_adjustments::CAMERA_ADJUSTMENT_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_time_act::TIME_ACT_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_aim::AIM_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_distance_words::DISTANCE_WORD_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_load_requests::LOAD_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_kind_records::KIND_RECORD_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_state_words::STATE_WORD_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_teams::TEAM_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::physics_queries::QUERY_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::parameters::PARAMETER_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::characters::CHARACTER_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_sets::SET_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_resistance::RESISTANCE_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::world_update_list::UPDATE_LIST_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::buddy_groups::BUDDY_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_vitals::VITALS_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_parameters::NPC_ASSOCIATION_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_equipment::EQUIPMENT_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::debug_world::DEBUG_WORLD_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_events::EVENT_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_flags::FLAG_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_opacity::OPACITY_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::debug_creator::CREATOR_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_animation::ANIMATION_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_actions::ACTION_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_modules::MODULE_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_handles::HANDLE_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_physics::PHYSICS_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::physics_suppression::SUPPRESSION_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_controller::CONTROLLER_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_ragdoll::RAGDOLL_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::controller_storage::STORAGE_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::controller_policy::POLICY_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_visuals::VISUAL_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_resources::RESOURCE_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::pose_buffers::POSE_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::skeleton_mapping::MAPPING_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::pose_transforms::TRANSFORM_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::event_flags::FLAG_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::torrent::TORRENT_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::game_settings::SETTINGS_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::npc_player_data::NPC_DATA_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::gparam::GPARAM_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::world_references::REFERENCE_EVIDENCE {
            module.code_address(*range)?;
        }
        for range in crate::character_tasks::CHARACTER_TASK_EVIDENCE {
            module.code_address(*range)?;
        }
        Ok(GameObjectCatalog { module })
    }
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    pub fn world(&self) -> Result<Option<ObjectObservation>> {
        self.singleton(WORLD_CHARACTERS_GLOBAL, ObjectKind::WorldCharacters)
    }

    pub fn parameter_repository(&self) -> Result<Option<ObjectObservation>> {
        self.singleton(SOLO_PARAMETERS_GLOBAL, ObjectKind::SoloParameters)
    }

    /// Engine's primary PlayerIns slot, not an enumeration or a network-role promise.
    pub fn primary_player(&self) -> Result<Option<ObjectObservation>> {
        let Some(world) = self.world()? else {
            return Ok(None);
        };
        let slot = world
            .address
            .checked_add(PRIMARY_PLAYER_OFFSET as u64)
            .ok_or(Error::AddressOverflow)?;
        let pointer = self.module.memory().read_u64(slot)?;
        let player = self.require(pointer, ObjectKind::Player)?;
        if self.module.memory().read_u64(slot)? != pointer || self.world()? != Some(world) {
            return Err(Error::InvalidProfile(
                "primary player changed during observation",
            ));
        }
        Ok(player)
    }

    pub(crate) fn singleton(
        &self,
        global: u32,
        kind: ObjectKind,
    ) -> Result<Option<ObjectObservation>> {
        let pointer = self.module.global_pointer(global)?;
        let object = self.require(pointer.unwrap_or(0), kind)?;
        if self.module.global_pointer(global)? != pointer {
            return Err(Error::InvalidProfile(
                "singleton changed during observation",
            ));
        }
        Ok(object)
    }
    /// Primary pointer only. Unknown subclasses are rejected rather than treated as PlayerIns.
    pub fn character(&self, address: u64) -> Result<Option<ObjectObservation>> {
        let Some(object) = self.identify(address)? else {
            return Ok(None);
        };
        if !matches!(
            object.kind,
            ObjectKind::Character | ObjectKind::Enemy | ObjectKind::Player
        ) {
            return Err(Error::InvalidProfile(
                "object is not a supported character type",
            ));
        }
        Ok(Some(object))
    }

    pub fn world_characters(&self, address: u64) -> Result<Option<ObjectObservation>> {
        self.require(address, ObjectKind::WorldCharacters)
    }

    pub fn solo_parameters(&self, address: u64) -> Result<Option<ObjectObservation>> {
        self.require(address, ObjectKind::SoloParameters)
    }

    pub fn parameter_resource(&self, address: u64) -> Result<Option<ParameterResourceObservation>> {
        if self
            .require(address, ObjectKind::ParameterResource)?
            .is_none()
        {
            return Ok(None);
        }
        let tag_address = address.checked_add(0x78).ok_or(Error::AddressOverflow)?;
        let argument_address = address.checked_add(0x80).ok_or(Error::AddressOverflow)?;
        let memory = self.module.memory();
        let native_storage_tag = memory.read_u32(tag_address)?;
        let native_constructor_argument = memory.read_u64(argument_address)?;
        if memory.read_u32(tag_address)? != native_storage_tag
            || memory.read_u64(argument_address)? != native_constructor_argument
        {
            return Err(Error::InvalidProfile(
                "parameter resource changed during observation",
            ));
        }
        self.require(address, ObjectKind::ParameterResource)?;
        Ok(Some(ParameterResourceObservation {
            address,
            native_storage_tag,
            native_constructor_argument,
        }))
    }

    pub(crate) fn require(
        &self,
        address: u64,
        expected: ObjectKind,
    ) -> Result<Option<ObjectObservation>> {
        let object = self.identify(address)?;
        if object.is_some_and(|object| object.kind != expected) {
            return Err(Error::InvalidProfile(
                "object kind differs from the requested interface",
            ));
        }
        Ok(object)
    }

    pub(crate) fn identify(&self, address: u64) -> Result<Option<ObjectObservation>> {
        if address == 0 {
            return Ok(None);
        }
        if !address.is_multiple_of(8) {
            return Err(Error::InvalidProfile("unaligned game object address"));
        }
        let table = self.module.memory().read_u64(address)?;
        let rva = table
            .checked_sub(self.module.base())
            .and_then(|v| u32::try_from(v).ok())
            .ok_or(Error::InvalidProfile(
                "object virtual table is outside the module",
            ))?;
        let kind = OBJECT_TYPES
            .iter()
            .find(|(_, _, candidate)| *candidate == rva)
            .map(|(kind, _, _)| *kind)
            .ok_or(Error::InvalidProfile(
                "unknown character/parameter primary virtual table",
            ))?;
        if self.module.memory().read_u64(address)? != table {
            return Err(Error::InvalidProfile(
                "object type changed during observation",
            ));
        }
        Ok(Some(ObjectObservation { address, kind }))
    }
}
