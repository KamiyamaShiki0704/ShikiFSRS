//! Explicit native spatial calls. Caller supplies the complete exclusive engine graph and phase.
use crate::{
    WW_171, Ww171File,
    character_sets::CharacterIdentity,
    character_spatial::{
        CHARACTER_TRANSLATION_SLOT, CharacterMotionObservation, CharacterPlacementObservation,
        CharacterSpatialObservation, CharacterTranslationObservation, NativeCharacterPlacementCall,
        NativeCharacterTranslationCall, NativeControllerSyncCall, NativePoseCall, NativePoseInput,
        NativeRotationCall, NativeTranslationCall, RotationOperation, SPATIAL_EVIDENCE,
        character_translation_entry, character_translation_plan, character_translation_postflight,
        controller_sync_plan, controller_sync_postflight, placement_plan, placement_postflight,
        pose_plan, pose_postflight, rotation_plan, rotation_postflight, translation_plan,
        translation_postflight,
    },
};
use shikifsrs_runtime::{
    Error, Result,
    code_patches::OwnedCodePatches,
    memory::{AddressSpace, CurrentProcess},
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

#[repr(C, align(16))]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NativeEulerBits {
    pub bits: [u32; 4],
}
impl NativeEulerBits {
    /// Does not normalize angles or assign a live gameplay yaw convention.
    pub fn finite_xyz(&self) -> Result<[f32; 3]> {
        let xyz = std::array::from_fn(|i| f32::from_bits(self.bits[i]));
        if xyz.into_iter().any(|v| !v.is_finite()) {
            return Err(Error::InvalidProfile("nonfinite native Euler result"));
        }
        Ok(xyz)
    }
}
#[derive(Debug)]
pub struct NativeEulerCall {
    pub source: CharacterSpatialObservation,
    pub value: NativeEulerBits,
    /// Raw return pointer; mismatching output retains the attempt but fails postflight.
    pub returned_output: bool,
    pub postflight: Result<()>,
}
#[repr(C, align(16))]
struct NativeSpatialInput {
    values: [f32; 4],
}
pub struct CurrentCharacterSpatial {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentCharacterSpatial {
    pub fn observe_position_request(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<crate::physics_position_requests::PhysicsPositionRequestObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .physics_position_request(target)
    }
    /// Publish primary position and the three bytes used by native 45fe70.
    /// Secondary position, owner state, rotations and controller policy remain unchanged.
    /// # Safety
    /// Caller independently pins the complete exclusive writable character/module/physics
    /// graph, all four selected extents, native readers and permitted update phase. Exclude
    /// native updates, callbacks, teardown, address reuse and aliases through postflight.
    /// This does not prove collision contact, movement completion or synchronized caches.
    pub unsafe fn publish_position_request(
        &self,
        source: crate::physics_position_requests::PhysicsPositionRequestObservation,
        position_bits: [u32; 4],
    ) -> Result<crate::physics_position_requests::PhysicsPositionRequestWrite> {
        use crate::physics_position_requests::{
            commit_position_request, position_request_plan, store_position_request,
        };
        let target = source.spatial.module.character;
        let planned = position_request_plan(
            self.observe_position_request(target)?,
            source,
            self.epoch,
            position_bits,
        )?;
        Ok(commit_position_request(
            source,
            planned,
            |address, position| {
                // SAFETY: Caller owns all selected writable extents, full graph and exclusive phase.
                unsafe { store_position_request(address, position) };
            },
            || {
                self.observe_position_request(target)?
                    .ok_or(Error::NativeOperationRejected(
                        "position publication actor absent",
                    ))
            },
        ))
    }
    /// Copy one linked perspective's raw matrix; no basis or gameplay convention is inferred.
    /// Bind's pinned camera-manager/camera graph and read-phase obligations also apply.
    pub fn observe_perspective(
        &self,
        index: usize,
    ) -> Result<Option<crate::cameras::PerspectiveCameraObservation>> {
        if index >= 4 {
            return Err(Error::InvalidProfile("perspective index out of range"));
        }
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let objects =
            file.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?;
        let Some(before) = objects.cameras()? else {
            return Ok(None);
        };
        let Some(camera) = before.perspectives[index] else {
            return Ok(None);
        };
        let copied = objects.perspective_camera(camera.address)?;
        if objects.cameras()? != Some(before) {
            return Err(Error::NativeOperationRejected("camera linkage changed"));
        }
        Ok(copied)
    }
    /// # Safety
    /// Caller identifies ER, pins physics/character/module and the complete getter/math graph,
    /// supplies its permitted read phase and stable initialized rotation/numerical globals,
    /// For translation the complete writable physics/cache/shape/native graph and numerical
    /// environment are pinned, including all transitive callees and their engine readers.
    /// The caller excludes callbacks that replace/update these values, teardown, unload, address reuse,
    /// Rust aliases and code patches. Every owned hook's transitive contract remains valid.
    /// Binding only verifies file/loaded evidence; it does not acquire these guarantees.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero native Euler epoch"));
        }
        let file = OwnedVerifiedFile::verify(bytes, WW_171)?;
        Ww171File::from_verified(file.borrow()?)?.bind_objects_with_patches(
            &CurrentProcess,
            base,
            patches.as_deref(),
        )?;
        Ok(Self {
            file,
            patches,
            base,
            epoch,
            _thread: PhantomData,
        })
    }
    pub fn observe(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterSpatialObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .character_spatial(target)
    }
    pub fn observe_motion(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterMotionObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .character_motion(target)
    }
    pub fn observe_placement(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterPlacementObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .character_placement(target)
    }
    pub fn observe_translation(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterTranslationObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .character_translation(target)
    }
    /// # Safety
    /// Bind's complete exclusive writable graph/phase/numerical/residency contract holds.
    /// Pin the exact character subclass and every linked native controller, model, position
    /// state, module, optional Enemy580/Player648/588/590/5c8 graph, transitive callees and
    /// initialized cache/shape readers. Caller proves all native arithmetic and synchronization
    /// conditions for fields beyond these copied records and excludes aliases/reuse/teardown.
    /// Dispatches the complete typed entry, including Enemy/Player additional paths; zero
    /// displacement still calls once. This is relative translation of existing native state,
    /// not a change of map registration or a proven cross-map relocation/ready barrier.
    /// Failed postflight retains the actual attempt; never retry with the old observation.
    pub unsafe fn translate_character(
        &self,
        expected: CharacterTranslationObservation,
        displacement: [f32; 3],
    ) -> Result<NativeCharacterTranslationCall> {
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let target = expected.placement.motion.spatial.module.character;
        let (delta, planned) = character_translation_plan(
            objects.character_translation(target)?,
            expected,
            self.epoch,
            displacement,
        )?;
        let table = objects.module.memory().read_u64(target.address)?;
        let range = character_translation_entry(expected.kind)?;
        let actual = objects
            .module
            .checked_virtual_function(table, CHARACTER_TRANSLATION_SLOT)?;
        if (actual.rva, actual.length) != (range.rva, range.length) {
            return Err(Error::InvalidProfile(
                "native character translation virtual entry changed",
            ));
        }
        let controller_table = objects
            .module
            .memory()
            .read_u64(expected.placement.motion.controls.controller.address)?;
        let actual = objects
            .module
            .checked_virtual_function(controller_table, 0x40)?;
        let expected_controller = SPATIAL_EVIDENCE[19];
        if (actual.rva, actual.length) != (expected_controller.rva, expected_controller.length) {
            return Err(Error::InvalidProfile(
                "native translation controller virtual entry changed",
            ));
        }
        let entry = objects.module.code_address(range)?;
        // SAFETY: Exact typed RCX character/RDX aligned four-float displacement/void ABI.
        let function: SpatialSetter = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Caller pins the complete writable graph and phase; helper owns aligned delta.
        Ok(unsafe {
            invoke_character_translation(function, expected, delta, planned, || {
                objects
                    .character_translation(target)?
                    .ok_or(Error::InvalidProfile(
                        "character unavailable after native translation",
                    ))
            })
        })
    }
    /// # Safety
    /// Bind's complete exclusive writable graph/phase/numerical/residency contract holds.
    /// Pin the character, exact controller-owner-container-physics chain and both native
    /// position/rotation tails, numerical globals and all transitive callees/readers.
    /// Caller independently establishes valid native position/angle conventions and excludes
    /// aliases, callbacks, address reuse, destruction and unload through postflight.
    /// Writes owner80/90/a0/350, physics70/80/50/60 and flags90/1de. OwnerD0 and physics150
    /// are not reset; this does not synchronize shapes, input, controller matrices or map
    /// registration. A successful copied result does not prove complete world relocation.
    pub unsafe fn place_character(
        &self,
        expected: CharacterPlacementObservation,
        input: NativePoseInput,
    ) -> Result<NativeCharacterPlacementCall> {
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let target = expected.motion.spatial.module.character;
        let angles = placement_plan(
            objects.character_placement(target)?,
            expected,
            self.epoch,
            input,
        )?;
        let entry = objects.module.code_address(SPATIAL_EVIDENCE[10])?;
        // SAFETY: Complete RCX character/RDX aligned position/R8 aligned angles/void ABI.
        let function: PoseSetter = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Caller pins the full graph/phase; helper owns both aligned initialized inputs.
        Ok(unsafe {
            invoke_placement(function, expected, input, angles, || {
                objects
                    .character_placement(target)?
                    .ok_or(Error::InvalidProfile(
                        "character unavailable after native placement",
                    ))
            })
        })
    }
    /// # Safety
    /// Bind's complete exclusive writable graph/phase/numerical/residency contract holds.
    /// Pin controller, character/module graph, ordinary input's exact virtual28 implementation,
    /// native getters/math, module slot1, controller28/180 optional graphs and all transitive
    /// callees/readers. Caller independently establishes valid native mode and numerical state.
    /// Native forwards raw_mode to ordinary input (not the preferred override), resets physics
    /// using native getters, updates slot1/controller/optional modules. Rotations can change;
    /// secondary position resets to primary. This does not prove complete owner/world relocation
    /// or a retirement/ready barrier, and is a separate attempt from translate/rotate/set_pose.
    pub unsafe fn synchronize_controller(
        &self,
        expected: CharacterMotionObservation,
        raw_mode: u8,
    ) -> Result<NativeControllerSyncCall> {
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        controller_sync_plan(
            objects.character_motion(expected.spatial.module.character)?,
            expected,
            self.epoch,
        )?;
        let entry = objects.module.code_address(SPATIAL_EVIDENCE[9])?;
        // SAFETY: Complete RCX controller/DL forwarded native byte/void ABI; full caller graph.
        let function: ControllerSync = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Caller pins complete native graph and phase through dispatch/postflight.
        Ok(unsafe {
            invoke_controller_sync(function, expected, raw_mode, || {
                objects
                    .character_motion(expected.spatial.module.character)?
                    .ok_or(Error::InvalidProfile(
                        "motion unavailable after native controller synchronization",
                    ))
            })
        })
    }
    /// # Safety
    /// Bind's full writable graph, exclusive permitted phase, numerical environment and code
    /// residency contract holds through postflight. All transitive positions/caches/shapes must
    /// be initialized and writable, and arithmetic/synchronization conditions valid independently
    /// of the copied fields. This is a relative translation, not a complete world relocation or
    /// rotation/controller update. Even zero displacement calls native code and updates flags.
    /// A failed postflight preserves the attempt; never retry it using the old snapshot.
    pub unsafe fn translate(
        &self,
        expected: CharacterSpatialObservation,
        displacement: [f32; 3],
    ) -> Result<NativeTranslationCall> {
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let (delta_bits, planned) = translation_plan(
            objects.character_spatial(expected.module.character)?,
            expected,
            self.epoch,
            displacement,
        )?;
        let entry = objects.module.code_address(SPATIAL_EVIDENCE[3])?;
        // SAFETY: Complete RCX physics/RDX pointer-to-four-floats/void ABI; full caller graph.
        let function: SpatialSetter = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Caller pins writable native graph and phase; helper owns aligned input.
        Ok(unsafe {
            invoke_translation(function, expected, delta_bits, planned, || {
                objects
                    .character_spatial(expected.module.character)?
                    .ok_or(Error::InvalidProfile(
                        "physics unavailable after native translation",
                    ))
            })
        })
    }
    /// # Safety
    /// Bind's full exclusive writable graph/numerical/residency contract holds. PrimaryBits
    /// requires a valid caller-established unit quaternion in native conventions. Native angles
    /// require independently valid inputs for the current native basis and constraint state.
    /// Pin every transitive numerical global, character, physics, optional constraint and shape
    /// object; code/type checks do not acquire these guarantees. Secondary rotation is native-
    /// derived and may differ from primary. Primary mode does not synchronize shape orientation;
    /// angle mode only performs its proved optional native path. Neither updates owner/world/
    /// controller coordinates or proves complete relocation. Even unchanged input is an attempt.
    pub unsafe fn rotate(
        &self,
        expected: CharacterSpatialObservation,
        operation: RotationOperation,
    ) -> Result<NativeRotationCall> {
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let input_bits = rotation_plan(
            objects.character_spatial(expected.module.character)?,
            expected,
            self.epoch,
            operation,
        )?;
        let index = match operation {
            RotationOperation::PrimaryBits(_) => 4,
            RotationOperation::NativeAngleBits(_) => 5,
        };
        let entry = objects.module.code_address(SPATIAL_EVIDENCE[index])?;
        // SAFETY: Complete RCX physics/RDX pointer-to-four-floats/void ABI; full caller contract.
        let function: SpatialSetter = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Caller pins complete writable graph/phase; helper owns aligned initialized input.
        Ok(unsafe {
            invoke_rotation(function, expected, operation, input_bits, || {
                objects
                    .character_spatial(expected.module.character)?
                    .ok_or(Error::InvalidProfile(
                        "physics unavailable after native rotation",
                    ))
            })
        })
    }
    /// # Safety
    /// Bind's full writable native graph, exclusive phase, numerical globals and residency hold.
    /// Caller supplies a valid four-lane native position and angles in its established native
    /// basis. This resets native70/80/150, rotation50/60 and flags90/1de/1b0. It does not directly
    /// synchronize native shapes, controller input or owner/world/chunk coordinates. Those
    /// obligations are independent; a successful copied postflight is not full relocation.
    pub unsafe fn set_pose(
        &self,
        expected: CharacterSpatialObservation,
        input: NativePoseInput,
    ) -> Result<NativePoseCall> {
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let angles = pose_plan(
            objects.character_spatial(expected.module.character)?,
            expected,
            self.epoch,
            input,
        )?;
        let entry = objects.module.code_address(SPATIAL_EVIDENCE[8])?;
        // SAFETY: Complete RCX physics/RDX position/R8 angles/void ABI; full caller contract.
        let function: PoseSetter = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Caller supplies pinned graph/phase; helper owns both aligned initialized vectors.
        Ok(unsafe {
            invoke_pose(function, expected, input, angles, || {
                objects
                    .character_spatial(expected.module.character)?
                    .ok_or(Error::InvalidProfile(
                        "physics unavailable after native pose",
                    ))
            })
        })
    }
    /// # Safety
    /// Bind's full phase/lifetime contract holds through the call and postflight.
    /// This is not a rotation update; the ordinary native getter transforms primary50 only.
    pub unsafe fn euler(&self, target: CharacterIdentity) -> Result<NativeEulerCall> {
        if target.epoch != self.epoch {
            return Err(Error::InvalidProfile("stale native Euler epoch"));
        }
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let source = objects
            .character_spatial(target)?
            .ok_or(Error::NativeOperationRejected(
                "missing physics for Euler getter",
            ))?;
        let entry = objects.module.code_address(SPATIAL_EVIDENCE[1])?;
        // SAFETY: Proved RCX physics/RDX aligned sixteen-byte output ABI; caller pins full graph.
        let function: EulerGetter = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Same caller contract; output is fresh aligned SDK-owned storage.
        Ok(unsafe {
            invoke(function, source, || {
                if objects.character_spatial(target)? != Some(source) {
                    return Err(Error::InvalidProfile("native Euler source changed"));
                }
                Ok(())
            })
        })
    }
}
type SpatialSetter = unsafe extern "system" fn(u64, *const NativeSpatialInput);
unsafe fn invoke_character_translation(
    function: SpatialSetter,
    source: CharacterTranslationObservation,
    delta_bits: [u32; 4],
    planned: CharacterTranslationObservation,
    observe: impl FnOnce() -> Result<CharacterTranslationObservation>,
) -> NativeCharacterTranslationCall {
    let delta = NativeSpatialInput {
        values: delta_bits.map(f32::from_bits),
    };
    // SAFETY: Complete typed entry and caller graph; fresh aligned displacement lives through call.
    unsafe {
        function(
            source.placement.motion.spatial.module.character.address,
            &delta,
        )
    };
    NativeCharacterTranslationCall {
        source,
        delta_bits,
        postflight: observe().and_then(|after| character_translation_postflight(planned, after)),
    }
}
unsafe fn invoke_translation(
    function: SpatialSetter,
    source: CharacterSpatialObservation,
    delta_bits: [u32; 4],
    planned: CharacterSpatialObservation,
    observe: impl FnOnce() -> Result<CharacterSpatialObservation>,
) -> NativeTranslationCall {
    let delta = NativeSpatialInput {
        values: delta_bits.map(f32::from_bits),
    };
    // SAFETY: Caller supplies complete native contract; four initialized aligned floats owned here.
    unsafe { function(source.module.address, &delta) };
    NativeTranslationCall {
        source,
        delta_bits,
        postflight: observe().and_then(|after| translation_postflight(planned, after)),
    }
}
unsafe fn invoke_rotation(
    function: SpatialSetter,
    source: CharacterSpatialObservation,
    operation: RotationOperation,
    input_bits: [u32; 4],
    observe: impl FnOnce() -> Result<CharacterSpatialObservation>,
) -> NativeRotationCall {
    let input = NativeSpatialInput {
        values: input_bits.map(f32::from_bits),
    };
    // SAFETY: Full caller contract holds; all sixteen aligned input bytes are helper-owned.
    unsafe { function(source.module.address, &input) };
    NativeRotationCall {
        source,
        operation,
        postflight: observe().and_then(|after| rotation_postflight(source, operation, after)),
    }
}

type PoseSetter =
    unsafe extern "system" fn(u64, *const NativeSpatialInput, *const NativeSpatialInput);
unsafe fn invoke_pose(
    function: PoseSetter,
    source: CharacterSpatialObservation,
    input: NativePoseInput,
    angle_bits: [u32; 4],
    observe: impl FnOnce() -> Result<CharacterSpatialObservation>,
) -> NativePoseCall {
    let position = NativeSpatialInput {
        values: input.position_bits.map(f32::from_bits),
    };
    let angles = NativeSpatialInput {
        values: angle_bits.map(f32::from_bits),
    };
    // SAFETY: Full native caller graph contract; both distinct aligned16 inputs owned here.
    unsafe { function(source.module.address, &position, &angles) };
    NativePoseCall {
        source,
        input,
        postflight: observe().and_then(|after| pose_postflight(source, input, after)),
    }
}

type ControllerSync = unsafe extern "system" fn(u64, u8);
unsafe fn invoke_placement(
    function: PoseSetter,
    source: CharacterPlacementObservation,
    input: NativePoseInput,
    angle_bits: [u32; 4],
    observe: impl FnOnce() -> Result<CharacterPlacementObservation>,
) -> NativeCharacterPlacementCall {
    let position = NativeSpatialInput {
        values: input.position_bits.map(f32::from_bits),
    };
    let angles = NativeSpatialInput {
        values: angle_bits.map(f32::from_bits),
    };
    // SAFETY: Complete caller graph; RCX is the character, not physics or controller.
    unsafe {
        function(
            source.motion.spatial.module.character.address,
            &position,
            &angles,
        )
    };
    NativeCharacterPlacementCall {
        source,
        input,
        postflight: observe().and_then(|after| placement_postflight(source, input, after)),
    }
}
unsafe fn invoke_controller_sync(
    function: ControllerSync,
    source: CharacterMotionObservation,
    raw_mode: u8,
    observe: impl FnOnce() -> Result<CharacterMotionObservation>,
) -> NativeControllerSyncCall {
    // SAFETY: Exact complete native ABI and caller-pinned writable transitive graph/phase.
    unsafe { function(source.controls.controller.address, raw_mode) };
    NativeControllerSyncCall {
        source,
        raw_mode,
        postflight: observe().and_then(|after| controller_sync_postflight(source, after)),
    }
}

type EulerGetter = unsafe extern "system" fn(u64, *mut NativeEulerBits) -> *mut NativeEulerBits;
unsafe fn invoke(
    function: EulerGetter,
    source: CharacterSpatialObservation,
    postflight: impl FnOnce() -> Result<()>,
) -> NativeEulerCall {
    let mut value = NativeEulerBits::default();
    let output = &mut value as *mut NativeEulerBits;
    // SAFETY: Caller supplies proved getter/source contract; output spans all sixteen bytes.
    let returned = unsafe { function(source.module.address, output) };
    let returned_output = returned == output;
    let postflight = postflight().and({
        if returned_output {
            Ok(())
        } else {
            Err(Error::InvalidProfile(
                "native Euler returned another output",
            ))
        }
    });
    NativeEulerCall {
        source,
        value,
        returned_output,
        postflight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character_modules::{CharacterModule, CharacterModuleObservation};
    struct Mock {
        bits: [u32; 4],
        wrong_return: bool,
    }
    unsafe extern "system" fn getter(
        source: u64,
        output: *mut NativeEulerBits,
    ) -> *mut NativeEulerBits {
        // SAFETY: Tests supply an owned Mock and live aligned output.
        let mock = unsafe { &*(source as *const Mock) };
        assert_eq!(output as usize % 16, 0);
        // SAFETY: SDK owns all sixteen writable bytes.
        unsafe { (*output).bits = mock.bits };
        if mock.wrong_return {
            std::ptr::null_mut()
        } else {
            output
        }
    }
    fn observation(mock: &Mock) -> CharacterSpatialObservation {
        CharacterSpatialObservation {
            module: CharacterModuleObservation {
                character: CharacterIdentity {
                    address: 0x1000,
                    world_address: 0x2000,
                    native_identifier: 0x123456789abcdef0,
                    epoch: 9,
                },
                module: CharacterModule::Physics,
                container_address: 0x3000,
                address: mock as *const Mock as u64,
            },
            primary_rotation_bits: [1, 2, 3, 4],
            secondary_rotation_bits: [5, 6, 7, 8],
            position_bits: [9, 10, 11, 12],
            secondary_position_bits: [13, 14, 15, 16],
        }
    }
    #[test]
    fn original_getter_abi_retains_aligned_output_raw_bits_and_full_source() {
        let mock = Mock {
            bits: [
                (-0f32).to_bits(),
                2f32.to_bits(),
                3f32.to_bits(),
                0x7fc01234,
            ],
            wrong_return: false,
        };
        let source = observation(&mock);
        // SAFETY: Self-authored entry, owned Mock, helper-owned output; no game entry.
        let call = unsafe { invoke(getter, source, || Ok(())) };
        assert_eq!(call.source, source);
        assert_eq!(call.value.bits, mock.bits);
        assert!(call.returned_output);
        assert!(call.postflight.is_ok());
        assert_eq!(
            call.value.finite_xyz().unwrap().map(f32::to_bits),
            mock.bits[..3]
        );
    }
    #[test]
    fn attempted_output_survives_postflight_failure_and_unexpected_return_pointer() {
        for wrong_return in [false, true] {
            let mock = Mock {
                bits: [0x7fc01234, 2, 3, 4],
                wrong_return,
            };
            let source = observation(&mock);
            // SAFETY: Self-authored mock only, with valid owned source/output storage.
            let call = unsafe {
                invoke(getter, source, || {
                    if wrong_return {
                        Ok(())
                    } else {
                        Err(Error::InvalidProfile("source changed"))
                    }
                })
            };
            assert_eq!(call.source, source);
            assert_eq!(call.value.bits, mock.bits);
            assert_eq!(call.returned_output, !wrong_return);
            assert!(call.postflight.is_err());
            assert!(call.value.finite_xyz().is_err());
        }
    }

    #[repr(C, align(16))]
    struct TranslationMock {
        calls: std::cell::Cell<u32>,
        positions: std::cell::Cell<[[u32; 4]; 2]>,
    }
    fn translated_observation(mock: &TranslationMock) -> CharacterSpatialObservation {
        CharacterSpatialObservation {
            module: CharacterModuleObservation {
                character: CharacterIdentity {
                    address: 0x1000,
                    world_address: 0x2000,
                    native_identifier: 0x123456789abcdef0,
                    epoch: 9,
                },
                module: CharacterModule::Physics,
                container_address: 0x3000,
                address: mock as *const TranslationMock as u64,
            },
            primary_rotation_bits: [1, 2, 3, 4],
            secondary_rotation_bits: [5, 6, 7, 8],
            position_bits: mock.positions.get()[0],
            secondary_position_bits: mock.positions.get()[1],
        }
    }
    unsafe extern "system" fn translator(address: u64, delta: *const NativeSpatialInput) {
        assert_eq!(delta as usize % 16, 0);
        // SAFETY: Tests supply original owned Mock and helper-owned four-float input only.
        let (mock, delta) = unsafe { (&*(address as *const TranslationMock), &*delta) };
        assert_eq!(delta.values[3].to_bits(), 0);
        mock.calls.set(mock.calls.get() + 1);
        mock.positions.set(mock.positions.get().map(|position| {
            std::array::from_fn(|i| (f32::from_bits(position[i]) + delta.values[i]).to_bits())
        }));
    }
    #[test]
    fn original_translation_abi_updates_both_vectors_once_even_for_zero_delta() {
        let mock = TranslationMock {
            calls: std::cell::Cell::new(0),
            positions: std::cell::Cell::new([
                [(-0f32).to_bits(); 4],
                [10.0, -20.0, 30.0, -0.0].map(f32::to_bits),
            ]),
        };
        for displacement in [[-0.0, 0.0, -0.0], [2.0, -3.0, 4.0], [0.0; 3]] {
            let source = translated_observation(&mock);
            let (delta, planned) = translation_plan(Some(source), source, 9, displacement).unwrap();
            let calls = mock.calls.get();
            // SAFETY: Original owned mock function, live cells and helper-owned aligned input.
            let receipt = unsafe {
                invoke_translation(translator, source, delta, planned, || {
                    Ok(translated_observation(&mock))
                })
            };
            assert_eq!(mock.calls.get(), calls + 1);
            assert_eq!(receipt.source, source);
            assert_eq!(receipt.delta_bits, delta);
            assert_eq!(receipt.postflight.unwrap(), planned);
            assert_eq!(planned.position_bits[3], 0); // -0 + explicit +0, not a raw lane copy.
            assert_eq!(planned.secondary_position_bits[3], 0);
        }
        let source = translated_observation(&mock);
        let (delta, planned) = translation_plan(Some(source), source, 9, [1.0; 3]).unwrap();
        // SAFETY: Original owned mock; the observer intentionally fails after a single attempt.
        let failed = unsafe {
            invoke_translation(translator, source, delta, planned, || {
                Err(Error::InvalidProfile("postflight unavailable"))
            })
        };
        assert_eq!(mock.calls.get(), 4);
        assert_eq!(failed.source, source);
        assert_eq!(failed.delta_bits, delta);
        assert!(failed.postflight.is_err());
        assert_eq!(translated_observation(&mock), planned);
    }
    #[test]
    fn translation_rejects_nonfinite_overflow_and_all_superseded_spatial_storage() {
        let mock = TranslationMock {
            calls: std::cell::Cell::new(0),
            positions: std::cell::Cell::new([[1.0, 2.0, 3.0, 1.0].map(f32::to_bits); 2]),
        };
        let source = translated_observation(&mock);
        assert!(translation_plan(None, source, 9, [0.0; 3]).is_err());
        for epoch in [0, 8] {
            assert!(translation_plan(Some(source), source, epoch, [0.0; 3]).is_err());
        }
        for field in 0..10 {
            let mut foreign = source;
            match field {
                0 => foreign.module.character.address += 16,
                1 => foreign.module.character.world_address += 16,
                2 => foreign.module.character.native_identifier ^= 1 << 63,
                3 => foreign.module.character.epoch += 1,
                4 => foreign.module.container_address += 8,
                5 => foreign.module.address += 16,
                6 => foreign.primary_rotation_bits[0] ^= 1,
                7 => foreign.secondary_rotation_bits[0] ^= 1,
                8 => foreign.position_bits[0] ^= 1,
                _ => foreign.secondary_position_bits[0] ^= 1,
            }
            assert!(translation_plan(Some(foreign), source, 9, [0.0; 3]).is_err());
            assert!(translation_postflight(source, foreign).is_err());
        }
        for field in 0..6 {
            let mut invalid = source;
            match field {
                0 => invalid.module.module = CharacterModule::Event,
                1 => invalid.module.address = 0,
                2 => invalid.module.address += 8,
                3 => invalid.module.container_address = 0,
                4 => invalid.module.container_address += 1,
                _ => invalid.module.address = u64::MAX - 15,
            }
            assert!(translation_plan(Some(invalid), invalid, 9, [0.0; 3]).is_err());
        }
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for i in 0..3 {
                let mut delta = [0.0; 3];
                delta[i] = bad;
                assert!(translation_plan(Some(source), source, 9, delta).is_err());
            }
            for i in 0..8 {
                let mut invalid = source;
                if i < 4 {
                    invalid.position_bits[i] = bad.to_bits();
                } else {
                    invalid.secondary_position_bits[i - 4] = bad.to_bits();
                }
                assert!(translation_plan(Some(invalid), invalid, 9, [0.0; 3]).is_err());
            }
        }
        for secondary in [false, true] {
            let mut invalid = source;
            if secondary {
                invalid.secondary_position_bits[0] = f32::MAX.to_bits();
            } else {
                invalid.position_bits[0] = f32::MAX.to_bits();
            }
            assert!(translation_plan(Some(invalid), invalid, 9, [f32::MAX, 0.0, 0.0]).is_err());
        }
        assert_eq!(mock.calls.get(), 0);
    }

    #[repr(C, align(16))]
    struct RotationMock {
        calls: std::cell::Cell<u32>,
        source: std::cell::Cell<CharacterSpatialObservation>,
        input: std::cell::Cell<[u32; 4]>,
        angle_mode: bool,
    }
    unsafe extern "system" fn rotation_setter(address: u64, input: *const NativeSpatialInput) {
        assert_eq!(input as usize % 16, 0);
        // SAFETY: Original owned mock and helper-owned four-float input, never game objects.
        let (mock, bits) = unsafe {
            (
                &*(address as *const RotationMock),
                (*input).values.map(f32::to_bits),
            )
        };
        mock.calls.set(mock.calls.get() + 1);
        mock.input.set(bits);
        let mut after = mock.source.get();
        after.primary_rotation_bits = if mock.angle_mode {
            [0.0, 1.0, 0.0, 0.0].map(f32::to_bits)
        } else {
            bits
        };
        // Original synthetic transformation deliberately differs from primary; no native math copy.
        after.secondary_rotation_bits = [0.0, 0.0, 1.0, 0.0].map(f32::to_bits);
        mock.source.set(after);
    }
    #[test]
    fn original_rotation_abi_keeps_native_derived_cache_and_single_attempt_failure() {
        let fixture = TranslationMock {
            calls: std::cell::Cell::new(0),
            positions: std::cell::Cell::new([[0.0, 1.0, 2.0, -0.0].map(f32::to_bits); 2]),
        };
        for angle_mode in [false, true] {
            let mock = RotationMock {
                calls: std::cell::Cell::new(0),
                source: std::cell::Cell::new(translated_observation(&fixture)),
                input: std::cell::Cell::new([0; 4]),
                angle_mode,
            };
            let mut source = mock.source.get();
            source.module.address = &mock as *const RotationMock as u64;
            let operation = if angle_mode {
                RotationOperation::NativeAngleBits([-0.0, 0.5, -1.0].map(f32::to_bits))
            } else {
                RotationOperation::PrimaryBits([-0.0, 0.0, 0.0, 1.0].map(f32::to_bits))
            };
            let input = rotation_plan(Some(source), source, 9, operation).unwrap();
            mock.source.set(source);
            // SAFETY: Self-authored entry with owned mock/input. No native game or live process read.
            let receipt = unsafe {
                invoke_rotation(rotation_setter, source, operation, input, || {
                    Ok(mock.source.get())
                })
            };
            assert_eq!(mock.calls.get(), 1);
            assert_eq!(mock.input.get(), input);
            assert_eq!(receipt.source, source);
            assert_eq!(receipt.operation, operation);
            let after = receipt.postflight.unwrap();
            assert_ne!(after.primary_rotation_bits, after.secondary_rotation_bits);
            assert_eq!(after.position_bits, source.position_bits);
            assert_eq!(
                after.secondary_position_bits,
                source.secondary_position_bits
            );
            // SAFETY: Same original owned mock; deliberate observation failure preserves the call.
            let receipt = unsafe {
                invoke_rotation(rotation_setter, after, operation, input, || {
                    Err(Error::InvalidProfile("rotation observer failed"))
                })
            };
            assert_eq!(mock.calls.get(), 2);
            assert_eq!(receipt.source, after);
            assert_eq!(receipt.operation, operation);
            assert!(receipt.postflight.is_err());
        }
    }
    #[test]
    fn rotation_plan_and_postflight_refuse_foreign_storage_nonfinite_and_zero_quaternion() {
        let fixture = TranslationMock {
            calls: std::cell::Cell::new(0),
            positions: std::cell::Cell::new([[0.0, 1.0, 2.0, 0.0].map(f32::to_bits); 2]),
        };
        let source = translated_observation(&fixture);
        let primary = RotationOperation::PrimaryBits([0.0, 0.0, 0.0, 1.0].map(f32::to_bits));
        let angles = RotationOperation::NativeAngleBits([0; 3]);
        for operation in [primary, angles] {
            assert!(rotation_plan(None, source, 9, operation).is_err());
            assert!(rotation_plan(Some(source), source, 8, operation).is_err());
            for field in 0..10 {
                let mut foreign = source;
                match field {
                    0 => foreign.module.character.address += 16,
                    1 => foreign.module.character.world_address += 16,
                    2 => foreign.module.character.native_identifier ^= 1 << 63,
                    3 => foreign.module.character.epoch += 1,
                    4 => foreign.module.container_address += 8,
                    5 => foreign.module.address += 16,
                    6 => foreign.primary_rotation_bits[0] ^= 1,
                    7 => foreign.secondary_rotation_bits[0] ^= 1,
                    8 => foreign.position_bits[0] ^= 1,
                    _ => foreign.secondary_position_bits[0] ^= 1,
                }
                assert!(rotation_plan(Some(foreign), source, 9, operation).is_err());
                if !(6..8).contains(&field) {
                    assert!(rotation_postflight(source, operation, foreign).is_err());
                }
            }
        }
        assert!(
            rotation_plan(
                Some(source),
                source,
                9,
                RotationOperation::PrimaryBits([(-0f32).to_bits(); 4])
            )
            .is_err()
        );
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for lane in 0..4 {
                let mut bits = [0.0, 0.0, 0.0, 1.0].map(f32::to_bits);
                bits[lane] = bad.to_bits();
                assert!(
                    rotation_plan(
                        Some(source),
                        source,
                        9,
                        RotationOperation::PrimaryBits(bits)
                    )
                    .is_err()
                );
                if lane < 3 {
                    assert!(
                        rotation_plan(
                            Some(source),
                            source,
                            9,
                            RotationOperation::NativeAngleBits(bits[..3].try_into().unwrap())
                        )
                        .is_err()
                    );
                }
                for secondary in [false, true] {
                    let mut after = source;
                    if secondary {
                        after.secondary_rotation_bits[lane] = bad.to_bits();
                    } else {
                        after.primary_rotation_bits[lane] = bad.to_bits();
                    }
                    assert!(rotation_postflight(source, angles, after).is_err());
                }
            }
        }
        for secondary in [false, true] {
            let mut after = source;
            if secondary {
                after.secondary_rotation_bits = [0; 4];
            } else {
                after.primary_rotation_bits = [0; 4];
            }
            assert!(rotation_postflight(source, angles, after).is_err());
        }
        assert!(rotation_postflight(source, primary, source).is_err()); // Exact primary input required.
        assert_eq!(fixture.calls.get(), 0);
    }

    unsafe extern "system" fn pose_setter(
        address: u64,
        position: *const NativeSpatialInput,
        angles: *const NativeSpatialInput,
    ) {
        assert_eq!(position as usize % 16, 0);
        assert_eq!(angles as usize % 16, 0);
        assert!((position as usize).abs_diff(angles as usize) >= 16);
        // SAFETY: Original owned mock and both distinct helper-owned initialized input vectors.
        let (mock, position) = unsafe {
            (
                &*(address as *const RotationMock),
                (*position).values.map(f32::to_bits),
            )
        };
        let mut after = mock.source.get();
        after.position_bits = position;
        after.secondary_position_bits = position;
        mock.source.set(after);
        // SAFETY: Same original mock contract; shared toy rotation callback counts one attempt.
        unsafe { rotation_setter(address, angles) };
    }
    #[test]
    fn original_pose_abi_uses_distinct_position_and_angle_inputs_and_preserves_attempt() {
        let fixture = TranslationMock {
            calls: std::cell::Cell::new(0),
            positions: std::cell::Cell::new([[0.0, 1.0, 2.0, 0.0].map(f32::to_bits); 2]),
        };
        let mock = RotationMock {
            calls: std::cell::Cell::new(0),
            source: std::cell::Cell::new(translated_observation(&fixture)),
            input: std::cell::Cell::new([0; 4]),
            angle_mode: true,
        };
        let mut source = mock.source.get();
        source.module.address = &mock as *const RotationMock as u64;
        mock.source.set(source);
        for position in [[10.0, -20.0, 30.0, -0.0], [-0.0; 4]] {
            let source = mock.source.get();
            let input = NativePoseInput {
                position_bits: position.map(f32::to_bits),
                angle_bits: [-0.0, 0.5, -1.0].map(f32::to_bits),
            };
            let angles = pose_plan(Some(source), source, 9, input).unwrap();
            let calls = mock.calls.get();
            // SAFETY: Original owned mock and helper-owned two-vector input; no game calls.
            let receipt = unsafe {
                invoke_pose(pose_setter, source, input, angles, || Ok(mock.source.get()))
            };
            assert_eq!(mock.calls.get(), calls + 1);
            assert_eq!(receipt.source, source);
            assert_eq!(receipt.input, input);
            assert_eq!(
                mock.input.get(),
                [
                    input.angle_bits[0],
                    input.angle_bits[1],
                    input.angle_bits[2],
                    0
                ]
            );
            let after = receipt.postflight.unwrap();
            assert_eq!(after.position_bits, input.position_bits);
            assert_eq!(after.secondary_position_bits, input.position_bits);
            assert_ne!(after.primary_rotation_bits, after.secondary_rotation_bits);
        }
        let source = mock.source.get();
        let input = NativePoseInput {
            position_bits: [1.0; 4].map(f32::to_bits),
            angle_bits: [0; 3],
        };
        let angles = pose_plan(Some(source), source, 9, input).unwrap();
        // SAFETY: Same original owned mock; post-observation deliberately fails after one call.
        let receipt = unsafe {
            invoke_pose(pose_setter, source, input, angles, || {
                Err(Error::InvalidProfile("pose observer failed"))
            })
        };
        assert_eq!(mock.calls.get(), 3);
        assert_eq!(receipt.source, source);
        assert_eq!(receipt.input, input);
        assert!(receipt.postflight.is_err());
        assert_eq!(mock.source.get().position_bits, input.position_bits);
    }
    #[test]
    fn pose_rejects_invalid_fourth_lane_angles_stale_source_and_partial_native_result() {
        let fixture = TranslationMock {
            calls: std::cell::Cell::new(0),
            positions: std::cell::Cell::new([[0.0, 1.0, 2.0, 0.0].map(f32::to_bits); 2]),
        };
        let source = translated_observation(&fixture);
        let input = NativePoseInput {
            position_bits: [5.0, 6.0, 7.0, -0.0].map(f32::to_bits),
            angle_bits: [0; 3],
        };
        assert!(pose_plan(None, source, 9, input).is_err());
        assert!(pose_plan(Some(source), source, 8, input).is_err());
        let mut foreign = source;
        foreign.module.container_address += 8;
        assert!(pose_plan(Some(foreign), source, 9, input).is_err());
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for lane in 0..4 {
                let mut invalid = input;
                invalid.position_bits[lane] = bad.to_bits();
                assert!(pose_plan(Some(source), source, 9, invalid).is_err());
                if lane < 3 {
                    let mut invalid = input;
                    invalid.angle_bits[lane] = bad.to_bits();
                    assert!(pose_plan(Some(source), source, 9, invalid).is_err());
                }
            }
        }
        let planned = CharacterSpatialObservation {
            position_bits: input.position_bits,
            secondary_position_bits: input.position_bits,
            ..source
        };
        assert_eq!(pose_postflight(source, input, planned).unwrap(), planned);
        for lane in 0..4 {
            for secondary in [false, true] {
                let mut partial = planned;
                if secondary {
                    partial.secondary_position_bits[lane] ^= 1;
                } else {
                    partial.position_bits[lane] ^= 1;
                }
                assert!(pose_postflight(source, input, partial).is_err());
            }
        }
        let mut foreign = planned;
        foreign.module.address += 16;
        assert!(pose_postflight(source, input, foreign).is_err());
        assert_eq!(fixture.calls.get(), 0);
    }

    #[repr(C, align(16))]
    struct SyncMock {
        calls: std::cell::Cell<u32>,
        mode: std::cell::Cell<u8>,
        source: std::cell::Cell<CharacterMotionObservation>,
    }
    fn motion_source() -> CharacterMotionObservation {
        let fixture = TranslationMock {
            calls: std::cell::Cell::new(0),
            positions: std::cell::Cell::new([
                [1.0, 2.0, 3.0, -0.0].map(f32::to_bits),
                [4.0, 5.0, 6.0, 1.0].map(f32::to_bits),
            ]),
        };
        let mut spatial = translated_observation(&fixture);
        spatial.module.address = 0x4000;
        spatial.primary_rotation_bits = [0.0, 0.0, 0.0, 1.0].map(f32::to_bits);
        CharacterMotionObservation {
            spatial,
            controls: crate::character_control_channels::CharacterControlChannels {
                controller: crate::character_controller::CharacterControllerObservation {
                    character: spatial.module.character,
                    address: 0x6000,
                    kind: crate::objects::ObjectKind::PlayerController,
                    axis_scale_bits: [1.0, 2.0, 3.0].map(f32::to_bits),
                },
                ordinary: Some(crate::character_control_channels::ManipulatorObservation {
                    object: crate::objects::ObjectObservation {
                        address: 0x7000,
                        kind: crate::objects::ObjectKind::PadManipulator,
                    },
                    epoch: 9,
                    character: Some(spatial.module.character),
                    native_character_address: spatial.module.character.address,
                }),
                override_input: None,
            },
        }
    }
    unsafe extern "system" fn controller_sync(address: u64, mode: u8) {
        // SAFETY: Original owned mock pointer; its copied observation addresses are not accessed.
        let mock = unsafe { &*(address as *const SyncMock) };
        mock.calls.set(mock.calls.get() + 1);
        mock.mode.set(mode);
        let mut after = mock.source.get();
        after.spatial.secondary_position_bits = after.spatial.position_bits;
        after.spatial.primary_rotation_bits = [0.0, 1.0, 0.0, 0.0].map(f32::to_bits);
        after.spatial.secondary_rotation_bits = [0.0, 0.0, 1.0, 0.0].map(f32::to_bits);
        mock.source.set(after);
    }
    #[test]
    fn original_controller_sync_abi_forwards_raw_byte_and_retains_each_native_attempt() {
        let mock = SyncMock {
            calls: std::cell::Cell::new(0),
            mode: std::cell::Cell::new(0),
            source: std::cell::Cell::new(motion_source()),
        };
        let mut source = mock.source.get();
        source.controls.controller.address = &mock as *const SyncMock as u64;
        mock.source.set(source);
        for mode in [0u8, 1, 128, 255] {
            let source = mock.source.get();
            controller_sync_plan(Some(source), source, 9).unwrap();
            let calls = mock.calls.get();
            // SAFETY: Original owned sync callback and mock; no game functions or engine memory.
            let receipt = unsafe {
                invoke_controller_sync(controller_sync, source, mode, || Ok(mock.source.get()))
            };
            assert_eq!(mock.calls.get(), calls + 1);
            assert_eq!(mock.mode.get(), mode);
            assert_eq!(receipt.source, source);
            assert_eq!(receipt.raw_mode, mode);
            let after = receipt.postflight.unwrap();
            assert_eq!(after.controls, source.controls);
            assert_eq!(after.spatial.position_bits, source.spatial.position_bits);
            assert_eq!(
                after.spatial.secondary_position_bits,
                source.spatial.position_bits
            );
        }
        let source = mock.source.get();
        // SAFETY: Original owned mock; observer fails after one native attempt, never retries.
        let failed = unsafe {
            invoke_controller_sync(controller_sync, source, 1, || {
                Err(Error::InvalidProfile("sync observer failed"))
            })
        };
        assert_eq!(mock.calls.get(), 5);
        assert_eq!(failed.source, source);
        assert!(failed.postflight.is_err());
    }
    #[test]
    fn controller_sync_requires_full_cross_graph_source_and_native_postflight() {
        let source = motion_source();
        assert!(controller_sync_plan(None, source, 9).is_err());
        assert!(controller_sync_plan(Some(source), source, 8).is_err());
        for field in 0..9 {
            let mut foreign = source;
            match field {
                0 => foreign.spatial.module.container_address += 8,
                1 => foreign.spatial.module.character.native_identifier ^= 1 << 63,
                2 => foreign.controls.controller.character.native_identifier ^= 1 << 63,
                3 => foreign.controls.controller.address += 8,
                4 => foreign.controls.controller.axis_scale_bits[0] ^= 1,
                5 => foreign.controls.controller.kind = crate::objects::ObjectKind::EnemyController,
                6 => foreign.controls.ordinary.as_mut().unwrap().object.address += 8,
                7 => {
                    foreign
                        .controls
                        .ordinary
                        .as_mut()
                        .unwrap()
                        .native_character_address += 16
                }
                _ => foreign.controls.override_input = foreign.controls.ordinary,
            }
            assert!(controller_sync_plan(Some(foreign), source, 9).is_err());
            assert!(controller_sync_postflight(source, foreign).is_err());
        }
        for field in 0..5 {
            let mut invalid = source;
            match field {
                0 => invalid.controls.controller.address = 0,
                1 => invalid.controls.controller.address += 1,
                2 => invalid.controls.controller.address = u64::MAX - 7,
                3 => invalid.controls.controller.kind = crate::objects::ObjectKind::Player,
                _ => invalid.controls.controller.character.epoch += 1,
            }
            assert!(controller_sync_plan(Some(invalid), invalid, 9).is_err());
        }
        for field in 0..8 {
            for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut invalid = source;
                if field < 4 {
                    invalid.spatial.position_bits[field] = bad.to_bits();
                } else {
                    invalid.spatial.primary_rotation_bits[field - 4] = bad.to_bits();
                }
                assert!(controller_sync_plan(Some(invalid), invalid, 9).is_err());
            }
        }
        let mut invalid = source;
        invalid.spatial.primary_rotation_bits = [(-0f32).to_bits(); 4];
        assert!(controller_sync_plan(Some(invalid), invalid, 9).is_err());
        let mut after = source;
        after.spatial.secondary_position_bits = source.spatial.position_bits;
        assert_eq!(controller_sync_postflight(source, after).unwrap(), after);
        for lane in 0..4 {
            let mut partial = after;
            partial.spatial.position_bits[lane] ^= 1;
            assert!(controller_sync_postflight(source, partial).is_err());
            let mut partial = after;
            partial.spatial.secondary_position_bits[lane] ^= 1;
            assert!(controller_sync_postflight(source, partial).is_err());
        }
        assert!(controller_sync_postflight(source, source).is_err()); // old secondary cache not reset.
    }
    #[repr(C, align(16))]
    struct PlacementMock {
        calls: std::cell::Cell<u32>,
        source: std::cell::Cell<CharacterPlacementObservation>,
    }
    fn placement_source() -> CharacterPlacementObservation {
        CharacterPlacementObservation {
            motion: motion_source(),
            position80_bits: [1, 2, 3, 4],
            position90_bits: [5, 6, 7, 8],
            anglea0_bits: [9, 10, 11, 12],
            positiond0_bits: [0x7fc01234, 0x80000000, 13, 14],
            scalar350_bits: (-0.0f32).to_bits(),
        }
    }
    fn placed(
        mut source: CharacterPlacementObservation,
        input: NativePoseInput,
    ) -> CharacterPlacementObservation {
        source.position80_bits = input.position_bits;
        source.position90_bits = input.position_bits;
        source.anglea0_bits = [
            input.angle_bits[0],
            input.angle_bits[1],
            input.angle_bits[2],
            0,
        ];
        source.scalar350_bits = 1f32.to_bits();
        source.motion.spatial.position_bits = input.position_bits;
        source.motion.spatial.secondary_position_bits = input.position_bits;
        source.motion.spatial.primary_rotation_bits = [0., 1., 0., 0.].map(f32::to_bits);
        source.motion.spatial.secondary_rotation_bits = [1., 0., 0., 0.].map(f32::to_bits);
        source
    }
    unsafe extern "system" fn placement_setter(
        address: u64,
        position: *const NativeSpatialInput,
        angles: *const NativeSpatialInput,
    ) {
        assert_ne!(position, angles);
        assert!((position as usize).is_multiple_of(16));
        assert!((angles as usize).is_multiple_of(16));
        // SAFETY: Owned mock and both distinct live initialized inputs supplied by the helper.
        let (mock, position, angles) =
            unsafe { (&*(address as *const PlacementMock), &*position, &*angles) };
        assert_eq!(angles.values[3].to_bits(), 0);
        mock.calls.set(mock.calls.get() + 1);
        mock.source.set(placed(
            mock.source.get(),
            NativePoseInput {
                position_bits: position.values.map(f32::to_bits),
                angle_bits: std::array::from_fn(|i| angles.values[i].to_bits()),
            },
        ));
    }
    #[test]
    fn original_character_placement_abi_targets_owner_and_preserves_each_attempt() {
        let mock = PlacementMock {
            calls: std::cell::Cell::new(0),
            source: std::cell::Cell::new(placement_source()),
        };
        let mut source = mock.source.get();
        source.motion.spatial.module.character.address = &mock as *const PlacementMock as u64;
        source.motion.controls.controller.character = source.motion.spatial.module.character;
        mock.source.set(source);
        let input = NativePoseInput {
            position_bits: [3., -4., 5., -0.].map(f32::to_bits),
            angle_bits: [0.1, -0.2, 0.3].map(f32::to_bits),
        };
        for _ in 0..2 {
            let source = mock.source.get();
            let angles = placement_plan(Some(source), source, 9, input).unwrap();
            // SAFETY: Original owned ABI mock, no engine memory or game dispatch.
            let receipt = unsafe {
                invoke_placement(placement_setter, source, input, angles, || {
                    Ok(mock.source.get())
                })
            };
            assert_eq!(receipt.source, source);
            assert_eq!(receipt.input, input);
            assert_eq!(receipt.postflight.unwrap(), placed(source, input));
        }
        let source = mock.source.get();
        // SAFETY: Same owned mock; failed observation must retain one completed call.
        let failed = unsafe {
            invoke_placement(placement_setter, source, input, [0; 4], || {
                Err(Error::InvalidProfile("placement observer failed"))
            })
        };
        assert_eq!(mock.calls.get(), 3);
        assert!(failed.postflight.is_err());
        assert_eq!(failed.source, source);
        drop(failed);
        assert_eq!(mock.calls.get(), 3);
    }
    #[test]
    fn placement_rejects_stale_owner_caches_bad_inputs_and_partial_native_results() {
        let source = placement_source();
        let input = NativePoseInput {
            position_bits: [3., -4., 5., 1.].map(f32::to_bits),
            angle_bits: [0.1, -0.2, 0.3].map(f32::to_bits),
        };
        assert!(placement_plan(None, source, 9, input).is_err());
        assert!(placement_plan(Some(source), source, 8, input).is_err());
        for field in 0..17 {
            let mut changed = source;
            match field {
                0..=3 => changed.position80_bits[field] ^= 1,
                4..=7 => changed.position90_bits[field - 4] ^= 1,
                8..=11 => changed.anglea0_bits[field - 8] ^= 1,
                12..=15 => changed.positiond0_bits[field - 12] ^= 1,
                _ => changed.scalar350_bits ^= 1,
            }
            assert!(placement_plan(Some(changed), source, 9, input).is_err());
        }
        for field in 0..7 {
            for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut invalid = input;
                if field < 4 {
                    invalid.position_bits[field] = bad.to_bits();
                } else {
                    invalid.angle_bits[field - 4] = bad.to_bits();
                }
                assert!(placement_plan(Some(source), source, 9, invalid).is_err());
            }
        }
        let after = placed(source, input);
        assert_eq!(placement_postflight(source, input, after).unwrap(), after);
        for field in 0..21 {
            let mut partial = after;
            match field {
                0..=3 => partial.position80_bits[field] ^= 1,
                4..=7 => partial.position90_bits[field - 4] ^= 1,
                8..=11 => partial.anglea0_bits[field - 8] ^= 1,
                12..=15 => partial.positiond0_bits[field - 12] ^= 1,
                16 => partial.scalar350_bits ^= 1,
                17 => partial.motion.controls.controller.address += 8,
                18 => partial.motion.spatial.position_bits[0] ^= 1,
                19 => partial.motion.spatial.secondary_position_bits[0] ^= 1,
                _ => partial.motion.spatial.module.container_address += 8,
            }
            assert!(placement_postflight(source, input, partial).is_err());
        }
        for address in [0, 1, u64::MAX - 15] {
            let mut invalid = source;
            invalid.motion.spatial.module.character.address = address;
            invalid.motion.controls.controller.character = invalid.motion.spatial.module.character;
            assert!(placement_plan(Some(invalid), invalid, 9, input).is_err());
        }
    }
    #[repr(C, align(16))]
    struct CharacterTranslationMock {
        calls: std::cell::Cell<u32>,
        source: std::cell::Cell<CharacterTranslationObservation>,
    }
    fn character_translation_source(
        kind: crate::objects::ObjectKind,
    ) -> CharacterTranslationObservation {
        let mut placement = placement_source();
        placement.motion.spatial.position_bits = [1., 2., 3., -0.].map(f32::to_bits);
        placement.motion.spatial.secondary_position_bits = [4., 5., 6., 1.].map(f32::to_bits);
        placement.position80_bits = [7., 8., 9., -0.].map(f32::to_bits);
        placement.position90_bits = [10., 11., 12., 1.].map(f32::to_bits);
        placement.positiond0_bits = [13., 14., 15., -0.].map(f32::to_bits);
        placement.motion.controls.controller.kind = match kind {
            crate::objects::ObjectKind::Character => {
                crate::objects::ObjectKind::CharacterController
            }
            crate::objects::ObjectKind::Enemy => crate::objects::ObjectKind::EnemyController,
            _ => crate::objects::ObjectKind::PlayerController,
        };
        CharacterTranslationObservation {
            placement,
            kind,
            position_state_address: 0x8000,
            position_state_bits: [16., 17., 18., 1.].map(f32::to_bits),
            controller_position100_bits: [19., 20., 21., -0.].map(f32::to_bits),
            controller_position260_bits: [22., 23., 24., 1.].map(f32::to_bits),
            scalar2e8_bits: 25f32.to_bits(),
        }
    }
    unsafe extern "system" fn character_translation_setter(
        address: u64,
        delta: *const NativeSpatialInput,
    ) {
        assert!((delta as usize).is_multiple_of(16));
        // SAFETY: Only original owned storage and helper's live aligned input are accessed.
        let (mock, delta) = unsafe { (&*(address as *const CharacterTranslationMock), &*delta) };
        assert_eq!(delta.values[3].to_bits(), 0);
        mock.calls.set(mock.calls.get() + 1);
        let mut value = mock.source.get();
        for field in [
            &mut value.placement.motion.spatial.position_bits,
            &mut value.placement.motion.spatial.secondary_position_bits,
            &mut value.placement.position80_bits,
            &mut value.placement.position90_bits,
            &mut value.placement.positiond0_bits,
            &mut value.position_state_bits,
            &mut value.controller_position100_bits,
            &mut value.controller_position260_bits,
        ] {
            for (lane, bits) in field.iter_mut().enumerate() {
                *bits = (f32::from_bits(*bits) + delta.values[lane]).to_bits();
            }
        }
        value.scalar2e8_bits = (f32::from_bits(value.scalar2e8_bits) + delta.values[1]).to_bits();
        mock.source.set(value);
    }
    #[test]
    fn typed_character_translation_original_abi_keeps_derived_entries_and_attempt_receipts() {
        for (kind, entry) in [
            (crate::objects::ObjectKind::Character, 0x3f7a20),
            (crate::objects::ObjectKind::Enemy, 0x4d0ea0),
            (crate::objects::ObjectKind::Player, 0x65a680),
        ] {
            assert_eq!(character_translation_entry(kind).unwrap().rva, entry);
            let mock = CharacterTranslationMock {
                calls: std::cell::Cell::new(0),
                source: std::cell::Cell::new(character_translation_source(kind)),
            };
            let mut source = mock.source.get();
            source.placement.motion.spatial.module.character.address =
                &mock as *const CharacterTranslationMock as u64;
            source.placement.motion.controls.controller.character =
                source.placement.motion.spatial.module.character;
            mock.source.set(source);
            for displacement in [[2., -3., 4.], [0.; 3]] {
                let source = mock.source.get();
                let (delta, planned) =
                    character_translation_plan(Some(source), source, 9, displacement).unwrap();
                // SAFETY: Original owned ABI mock; no engine code or engine memory.
                let receipt = unsafe {
                    invoke_character_translation(
                        character_translation_setter,
                        source,
                        delta,
                        planned,
                        || Ok(mock.source.get()),
                    )
                };
                assert_eq!(receipt.source, source);
                assert_eq!(receipt.delta_bits, delta);
                assert_eq!(receipt.postflight.unwrap(), planned);
                assert_eq!(
                    planned.placement.anglea0_bits,
                    source.placement.anglea0_bits
                );
            }
            let source = mock.source.get();
            let (delta, planned) =
                character_translation_plan(Some(source), source, 9, [1.; 3]).unwrap();
            // SAFETY: Same owned mock; failed observation retains one attempt, no automatic retry.
            let failed = unsafe {
                invoke_character_translation(
                    character_translation_setter,
                    source,
                    delta,
                    planned,
                    || Err(Error::InvalidProfile("translation observer failed")),
                )
            };
            assert!(failed.postflight.is_err());
            assert_eq!(failed.source, source);
            drop(failed);
            assert_eq!(mock.calls.get(), 3);
        }
    }
    #[test]
    fn character_translation_rejects_nonfinite_all_caches_overflow_and_superseded_graph() {
        let source = character_translation_source(crate::objects::ObjectKind::Player);
        assert!(character_translation_plan(None, source, 9, [0.; 3]).is_err());
        assert!(character_translation_plan(Some(source), source, 8, [0.; 3]).is_err());
        for field in 0..14 {
            let mut foreign = source;
            match field {
                0 => foreign.kind = crate::objects::ObjectKind::Enemy,
                1 => foreign.position_state_address += 16,
                2 => foreign.position_state_bits[0] ^= 1,
                3 => foreign.controller_position100_bits[0] ^= 1,
                4 => foreign.controller_position260_bits[0] ^= 1,
                5 => foreign.scalar2e8_bits ^= 1,
                6 => foreign.placement.positiond0_bits[0] ^= 1,
                7 => foreign.placement.motion.controls.controller.address += 8,
                8 => foreign.placement.motion.spatial.module.container_address += 8,
                9 => {
                    foreign
                        .placement
                        .motion
                        .spatial
                        .module
                        .character
                        .native_identifier ^= 1 << 63
                }
                10 => foreign.placement.position80_bits[0] ^= 1,
                11 => foreign.placement.position90_bits[0] ^= 1,
                12 => foreign.placement.anglea0_bits[0] ^= 1,
                _ => foreign.placement.scalar350_bits ^= 1,
            }
            assert!(character_translation_plan(Some(foreign), source, 9, [0.; 3]).is_err());
            let (_, planned) =
                character_translation_plan(Some(source), source, 9, [0.; 3]).unwrap();
            let (_, mut changed) =
                character_translation_plan(Some(source), source, 9, [0.; 3]).unwrap();
            match field {
                0 => changed.kind = foreign.kind,
                1 => changed.position_state_address = foreign.position_state_address,
                2 => changed.position_state_bits = foreign.position_state_bits,
                3 => changed.controller_position100_bits = foreign.controller_position100_bits,
                4 => changed.controller_position260_bits = foreign.controller_position260_bits,
                5 => changed.scalar2e8_bits = foreign.scalar2e8_bits,
                _ => changed.placement = foreign.placement,
            }
            assert!(character_translation_postflight(planned, changed).is_err());
        }
        for field in 0..25 {
            for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut invalid = source;
                match field {
                    0..=3 => invalid.placement.position80_bits[field] = bad.to_bits(),
                    4..=7 => invalid.placement.position90_bits[field - 4] = bad.to_bits(),
                    8..=11 => invalid.placement.positiond0_bits[field - 8] = bad.to_bits(),
                    12..=15 => invalid.position_state_bits[field - 12] = bad.to_bits(),
                    16..=19 => invalid.controller_position100_bits[field - 16] = bad.to_bits(),
                    20..=23 => invalid.controller_position260_bits[field - 20] = bad.to_bits(),
                    _ => invalid.scalar2e8_bits = bad.to_bits(),
                }
                assert!(character_translation_plan(Some(invalid), invalid, 9, [0.; 3]).is_err());
            }
        }
        let mut overflow = source;
        overflow.position_state_bits[0] = f32::MAX.to_bits();
        assert!(
            character_translation_plan(Some(overflow), overflow, 9, [f32::MAX, 0., 0.]).is_err()
        );
        for address in [0, 1, u64::MAX - 15] {
            let mut invalid = source;
            invalid.position_state_address = address;
            assert!(character_translation_plan(Some(invalid), invalid, 9, [0.; 3]).is_err());
        }
        assert!(character_translation_entry(crate::objects::ObjectKind::WorldCharacters).is_err());
        let mut mismatch = source;
        mismatch.placement.motion.controls.controller.kind =
            crate::objects::ObjectKind::EnemyController;
        assert!(character_translation_plan(Some(mismatch), mismatch, 9, [0.; 3]).is_err());
    }
}
