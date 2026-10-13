//! Independent ER access. Only WW 1.17.1 is accepted by the current profile.
//! Task observations are read-only. Experimental native tasks support direct/queued
//! registration and explicit unregistration under unsafe phase/lifetime obligations.
//! Native runtime interfaces remain experimental; game-runtime acceptance is pending.
use shikifsrs_runtime::{
    Error, Result,
    memory::AddressSpace,
    native_tree::U32PointerTree,
    profile::{CodeRange, ExecutableIdentity, VerifiedFile, VerifiedModule},
};

mod file_metadata;
mod phases;
pub use phases::{PhaseDefinition, TaskPhase};
mod registrations;
pub use registrations::TaskRegistration;
pub mod animation_capacity;
pub mod buddy_groups;
pub mod bullet_spawn;
pub mod bullets;
pub mod camera_adjustments;
pub mod cameras;
pub mod character_actions;
pub mod character_aim;
pub mod character_animation;
pub mod character_control_channels;
pub mod character_controller;
pub mod character_distance_words;
pub mod character_equipment;
pub mod character_events;
pub mod character_flags;
pub mod character_handles;
pub mod character_kind_records;
pub mod character_load_requests;
pub mod character_modules;
pub mod character_opacity;
pub mod character_parameters;
pub mod character_physics;
pub mod character_posture;
pub mod character_ragdoll;
pub mod character_resistance;
pub mod character_resources;
pub mod character_scalars;
pub mod character_scale;
pub mod character_sets;
pub mod character_spatial;
pub mod character_state_words;
pub mod character_task_clear;
pub mod character_tasks;
pub mod character_teams;
pub mod character_time_act;
pub mod character_units;
pub mod character_visuals;
pub mod character_vitals;
mod characters;
pub mod controller_policy;
pub mod controller_storage;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_buddy_control;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_bullets;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_character_lookup;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_character_reset;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_character_tasks;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_character_units;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_effects;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_frontend_hud;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_gparam;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_gparam_task;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_network;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_objects;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_skeleton_transforms;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_sound_bank_api;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_sound_banks;
pub mod current_sound_conditions;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_summon_hud;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_tasks;
pub mod debug_creation_fields;
pub mod debug_creator;
pub mod debug_world;
pub mod effects;
pub mod event_flags;
pub mod friendly_hud;
pub mod frontend_markers;
pub mod game_settings;
pub mod gparam;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod gparam_resident;
pub mod gparam_task;
pub mod hud_hook_sites;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod input_effect_resident;
pub mod input_effect_task;
pub mod keyboard_effects;
pub mod menu_state;
pub mod native_task;
pub mod network_sessions;
pub mod npc_player_data;
pub mod objects;
pub mod physics_position_requests;
pub mod physics_queries;
pub mod physics_suppression;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod sound_bank_resident;
pub mod sound_bank_schedule;
pub mod sound_bank_tasks;
pub mod sound_banks;
pub mod sound_conditions;
pub mod storage_facts;
pub mod summon_hud;
pub mod summon_lifecycle;
pub mod summon_triggers;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod task_resident;
pub mod world_references;
pub mod world_update_list;
pub use characters::CharacterDataObservation;
pub use characters::{CharacterEffectsObservation, EffectNodeObservation};
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod character_policy_session;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod control_link_session;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod creation_input_preparation;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_character_animation;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_character_events;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_character_flags;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_character_lifecycle;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_character_resources;
pub mod current_character_scalars;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_character_spatial;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_character_vitals;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_control_links;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_controller_policy;
#[cfg(windows)]
pub mod current_debug_creator;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_input_effect_task;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_parameters;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_physics_suppression;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_title_runtime;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod current_torrent;
pub mod parameter_fields;
pub mod parameter_overlays;
pub mod parameters;
pub mod pose_buffers;
pub mod pose_transforms;
pub mod skeleton_mapping;
pub mod task_group_state;
pub mod task_owner;
pub mod title_runtime;
pub mod torrent;

/// Owned Rust callback for copied task arguments; not a registered native task.
pub type TaskCallback = shikifsrs_runtime::callback::Callback<TaskInvocation>;
/// Controls Rust callback resources; does not acknowledge native unregistration.
pub type TaskCallbackControl = shikifsrs_runtime::callback::CallbackControl<TaskInvocation>;

pub const WW_171: ExecutableIdentity = ExecutableIdentity {
    name: "Elden Ring WW 1.17.1 / EXE 2.7.1.0",
    file_size: 87_042_128,
    sha256: [
        0x1a, 0x35, 0x47, 0x10, 0x13, 0x27, 0xf6, 0x5d, 0x0c, 0x76, 0xda, 0x2f, 0x91, 0x90, 0xac,
        0x0a, 0xa6, 0x68, 0x71, 0xea, 0x42, 0xba, 0xe2, 0xae, 0xcc, 0x61, 0xe1, 0x1a, 0x8b, 0x59,
        0x78, 0x91,
    ],
};

// Derived by direct PE/RTTI extraction and constructor/registration disassembly.
// This profile is fixed to the exact executable digest below.
const GROUP_GLOBAL: u32 = 0x458ff08;
const GROUP_MANAGER_VTABLE: u32 = 0x2c04cb0;
const GROUP_INSTANCE_VTABLE: u32 = 0x2c08cf8;
const TIME_VTABLE: u32 = 0x29cbe58;
const TASK_SYSTEM_GLOBAL: u32 = 0x4861280;
const REGISTRY_VTABLE: u32 = 0x329d370;
const PHASE_DEFINITIONS: u32 = 0x2c04d20;
const REGISTERED_GROUP_VTABLE: u32 = 0x329d348;
const REGISTRATION_RECORD_VTABLE: u32 = 0x329d350;
pub const TASK_GROUP_COUNT: usize = 169;
const TASK_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0xeb3e50,
        length: 0xc3,
    },
    CodeRange {
        rva: 0xeb42a0,
        length: 0x6c,
    },
    CodeRange {
        rva: 0xeb4600,
        length: 0x21,
    },
    CodeRange {
        rva: 0xeb47e0,
        length: 0x87,
    },
    CodeRange {
        rva: 0xeb4930,
        length: 0x18,
    },
    CodeRange {
        rva: 0x26d7d80,
        length: 0x79,
    },
    CodeRange {
        rva: 0x26d7e00,
        length: 0x390,
    },
    CodeRange {
        rva: 0x26d8980,
        length: 0x5d,
    },
    CodeRange {
        rva: 0x2657970,
        length: 0x135,
    },
    CodeRange {
        rva: 0x26a1d10,
        length: 0xd2,
    },
    CodeRange {
        rva: 0x172f60,
        length: 0x34,
    },
    CodeRange {
        rva: 0x26a1cc0,
        length: 0x49,
    },
    CodeRange {
        rva: 0x26a14f0,
        length: 0x2cc,
    },
    CodeRange {
        rva: 0x26a1900,
        length: 0x145,
    },
    CodeRange {
        rva: 0x26a1b30,
        length: 0x105,
    },
];

pub struct Ww171File<'a> {
    file: VerifiedFile<'a>,
}
static TASK_FILE_METADATA: file_metadata::FileMetadataGate =
    file_metadata::FileMetadataGate::new(WW_171);

impl<'a> Ww171File<'a> {
    pub fn verify(bytes: &'a [u8]) -> Result<Self> {
        let file = VerifiedFile::verify(bytes, WW_171)?;
        Self::from_verified(file)
    }

    /// Reuses an exact previously hashed file, retaining this game's metadata gates.
    pub fn from_verified(file: VerifiedFile<'a>) -> Result<Self> {
        TASK_FILE_METADATA.verify(&file, || {
            for (name, table) in [
                (".?AVCSTaskGroupImp@CS@@", GROUP_MANAGER_VTABLE),
                (".?AVCSTaskGroupIns@CS@@", GROUP_INSTANCE_VTABLE),
                (".?AVFD4Time@FD4@@", TIME_VTABLE),
            ] {
                let matches: Vec<_> = file
                    .image()
                    .vtables(name)?
                    .into_iter()
                    .filter(|v| v.object_offset == 0)
                    .collect();
                if matches.len() != 1 || matches[0].table_rva != table {
                    return Err(Error::InvalidProfile(
                        "task RTTI does not match independently derived profile",
                    ));
                }
            }
            let definitions =
                phases::definitions(file.image(), PHASE_DEFINITIONS, TASK_GROUP_COUNT)?;
            for (phase, name) in [
                (TaskPhase::FRAME_BEGIN, "FrameBegin"),
                (TaskPhase::SOUND_STEP, "SoundStep"),
                (TaskPhase::CHR_POST_PHYSICS, "ChrIns_PostPhysics"),
                (TaskPhase::FRAME_END, "FrameEnd"),
            ] {
                if definitions[phase.registration_index()].name != name {
                    return Err(Error::InvalidProfile(
                        "named task phase differs from native table",
                    ));
                }
            }
            Ok(())
        })?;
        Ok(Self { file })
    }

    pub fn file(&self) -> &VerifiedFile<'a> {
        &self.file
    }

    pub fn phase_definitions(&self) -> Result<Vec<PhaseDefinition>> {
        phases::definitions(self.file.image(), PHASE_DEFINITIONS, TASK_GROUP_COUNT)
    }

    /// File-only catalog shared with the cross-game SDK. This is not live phase ownership.
    pub fn task_definitions(&self) -> Result<shikifsrs_runtime::task_definitions::TaskDefinitions> {
        shikifsrs_runtime::task_definitions::TaskDefinitions::read(
            self.file.image(),
            PHASE_DEFINITIONS,
            TASK_GROUP_COUNT,
        )
    }

    pub fn phase(&self, name: &str) -> Result<TaskPhase> {
        self.phase_definitions()?
            .into_iter()
            .find(|d| d.name == name)
            .map(|d| d.phase)
            .ok_or(Error::InvalidProfile("requested task phase is absent"))
    }

    pub fn bind<'b, M: AddressSpace>(
        &'b self,
        memory: &'b M,
        base: u64,
    ) -> Result<TaskCatalog<'b, 'a, M>> {
        self.bind_with_patches(memory, base, None)
    }

    /// Explicit owned hook policy; unregistered code and virtual slots remain strict.
    pub fn bind_with_patches<'b, M: AddressSpace>(
        &'b self,
        memory: &'b M,
        base: u64,
        patches: Option<&'b shikifsrs_runtime::code_patches::OwnedCodePatches>,
    ) -> Result<TaskCatalog<'b, 'a, M>> {
        let module = self
            .file
            .bind_with_patches(memory, base, TASK_EVIDENCE, patches)?;
        for range in task_group_state::GROUP_STATE_EVIDENCE {
            module.code_address(*range)?;
        }
        Ok(TaskCatalog { module })
    }
}

/// A copied task-group observation, without borrowing game-owned Rust references.
/// Fields are deliberately raw where engine semantics have not yet been proven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupObservation {
    pub registration_index: usize,
    pub native_pointer: u64,
    pub native_words: [u32; 4],
}

impl GroupObservation {
    /// The native submit helper copies +0x54 into the full registered group ID.
    /// Zero denotes an ID that has not yet been assigned; this does not test activity.
    pub fn native_group_id(&self) -> Option<u32> {
        (self.native_words[3] != 0).then_some(self.native_words[3])
    }
}

/// Copied fields from the native callback argument. No padding bytes are read.
/// The meaning of the time value and scheduler index has not yet been validated in-game.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TaskInvocation {
    pub time_value: f32,
    pub group_id: u32,
    pub scheduler_index: u32,
}

pub struct TaskCatalog<'b, 'a, M> {
    module: VerifiedModule<'b, 'a, M>,
}

impl<M: AddressSpace> TaskCatalog<'_, '_, M> {
    pub fn observe_phase(&self, phase: TaskPhase) -> Result<Option<GroupObservation>> {
        self.observe(phase.registration_index())
    }

    /// Takes a bounded snapshot of a group's registration records.
    /// None means the group is absent; Some(empty) means the group has no records.
    /// Presence/absence never acknowledges cancellation completion or grants ownership.
    pub fn task_registrations(
        &self,
        group_id: u32,
        node_limit: usize,
        task_limit: usize,
    ) -> Result<Option<Vec<TaskRegistration>>> {
        let Some(group) = self.registered_group(group_id, node_limit)? else {
            return Ok(None);
        };
        self.check_vtable(group, REGISTERED_GROUP_VTABLE)?;
        let id_slot = group.checked_add(0x80).ok_or(Error::AddressOverflow)?;
        let memory = self.module.memory();
        if memory.read_u32(id_slot)? != group_id {
            return Err(Error::InvalidProfile(
                "registered group ID differs from tree key",
            ));
        }
        let record_vtable = self
            .module
            .base()
            .checked_add(REGISTRATION_RECORD_VTABLE as u64)
            .ok_or(Error::AddressOverflow)?;
        let result = registrations::read(memory, group, group_id, record_vtable, task_limit)?;
        self.check_vtable(group, REGISTERED_GROUP_VTABLE)?;
        if memory.read_u32(id_slot)? != group_id
            || self.registered_group(group_id, node_limit)? != Some(group)
        {
            return Err(Error::InvalidProfile(
                "registered group changed during observation",
            ));
        }
        Ok(Some(result))
    }

    /// Resolves the complete native group ID through the task system's registered-group tree.
    /// A missing system, registry, key or null stored pointer returns None.
    /// This copies a pointer; it does not borrow or establish the lifetime of the native group.
    pub fn registered_group(&self, group_id: u32, node_limit: usize) -> Result<Option<u64>> {
        // The native lookup explicitly excludes ID zero before reading its tree.
        if group_id == 0 {
            return Ok(None);
        }
        let Some(system) = self.module.global_pointer(TASK_SYSTEM_GLOBAL)? else {
            return Ok(None);
        };
        let registry_slot = system.checked_add(0x48).ok_or(Error::AddressOverflow)?;
        let memory = self.module.memory();
        let registry = memory.read_u64(registry_slot)?;
        if registry == 0 {
            return Ok(None);
        }
        self.check_vtable(registry, REGISTRY_VTABLE)?;
        let tree = registry.checked_add(0x10).ok_or(Error::AddressOverflow)?;
        let result = U32PointerTree::new(memory, tree, node_limit)?.find(group_id)?;
        self.check_vtable(registry, REGISTRY_VTABLE)?;
        if memory.read_u64(registry_slot)? != registry
            || self.module.global_pointer(TASK_SYSTEM_GLOBAL)? != Some(system)
        {
            return Err(Error::InvalidProfile(
                "task system registry changed during lookup",
            ));
        }
        Ok(result.filter(|pointer| *pointer != 0))
    }

    /// Copies an invocation argument using the same validated image and memory adapter.
    /// This does not register a callback or establish that the caller owns an engine phase.
    pub fn read_invocation(&self, address: u64) -> Result<TaskInvocation> {
        copy_invocation(
            self.module.memory(),
            address,
            self.module
                .base()
                .checked_add(TIME_VTABLE as u64)
                .ok_or(Error::AddressOverflow)?,
        )
    }

    /// None means the manager or requested slot is not initialized.
    /// Reading these fields does not prove that the group is active or safe for mutation.
    pub fn observe(&self, index: usize) -> Result<Option<GroupObservation>> {
        if index >= TASK_GROUP_COUNT {
            return Err(Error::InvalidProfile(
                "task group index exceeds validated table",
            ));
        }
        let Some(manager) = self.module.global_pointer(GROUP_GLOBAL)? else {
            return Ok(None);
        };
        let memory = self.module.memory();
        self.check_vtable(manager, GROUP_MANAGER_VTABLE)?;
        let slot = manager
            .checked_add(8 + index as u64 * 8)
            .ok_or(Error::AddressOverflow)?;
        let pointer = memory.read_u64(slot)?;
        if pointer == 0 {
            return Ok(None);
        }
        self.check_vtable(pointer, GROUP_INSTANCE_VTABLE)?;
        let mut native_words = [0; 4];
        for (index, word) in native_words.iter_mut().enumerate() {
            let at = pointer
                .checked_add(0x48 + index as u64 * 4)
                .ok_or(Error::AddressOverflow)?;
            *word = memory.read_u32(at)?;
        }
        // Detect changes visible at the end. This does not lock the engine or guarantee lifetime.
        if memory.read_u64(slot)? != pointer
            || self.module.global_pointer(GROUP_GLOBAL)? != Some(manager)
        {
            return Err(Error::InvalidProfile(
                "task group changed during observation",
            ));
        }
        self.check_vtable(pointer, GROUP_INSTANCE_VTABLE)?;
        self.check_vtable(manager, GROUP_MANAGER_VTABLE)?;
        Ok(Some(GroupObservation {
            registration_index: index,
            native_pointer: pointer,
            native_words,
        }))
    }

    fn check_vtable(&self, pointer: u64, rva: u32) -> Result<()> {
        let expected = self
            .module
            .base()
            .checked_add(rva as u64)
            .ok_or(Error::AddressOverflow)?;
        if self.module.memory().read_u64(pointer)? != expected {
            return Err(Error::InvalidProfile(
                "task object vtable differs from validated profile",
            ));
        }
        Ok(())
    }
}

fn copy_invocation(
    memory: &impl AddressSpace,
    address: u64,
    expected_table: u64,
) -> Result<TaskInvocation> {
    if address == 0 {
        return Err(Error::InvalidProfile("null task invocation"));
    }
    let check = || -> Result<()> {
        if memory.read_u64(address)? != expected_table {
            return Err(Error::InvalidProfile(
                "task object vtable differs from validated profile",
            ));
        }
        Ok(())
    };
    check()?;
    let at = |offset| address.checked_add(offset).ok_or(Error::AddressOverflow);
    let result = TaskInvocation {
        time_value: memory.read_f32(at(8)?)?,
        group_id: memory.read_u32(at(16)?)?,
        scheduler_index: memory.read_u32(at(20)?)?,
    };
    check()?;
    Ok(result)
}

#[cfg(test)]
mod tests;
