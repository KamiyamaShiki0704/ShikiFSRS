//! Experimental copied current camera and window storage. No rendering or writes.
use crate::objects::{GameObjectCatalog, ObjectKind, ObjectObservation};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const CAMERA_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3bad30,
        length: 0x115,
    },
    CodeRange {
        rva: 0xb10200,
        length: 0x1a0,
    },
    CodeRange {
        rva: 0xb12d50,
        length: 0x4b,
    },
    // Full render preparation, beyond the first partial .pdata fragment.
    CodeRange {
        rva: 0xb13730,
        length: 0x148,
    },
    CodeRange {
        rva: 0x3bc580,
        length: 0x61,
    },
    CodeRange {
        rva: 0xdecf20,
        length: 0x13dd,
    },
    CodeRange {
        rva: 0xe8f5f0,
        length: 0x113,
    },
    CodeRange {
        rva: 0x2670a00,
        length: 0x3d,
    },
    CodeRange {
        rva: 0xe8fbe0,
        length: 0xfc8,
    },
    CodeRange {
        rva: 0x26707b0,
        length: 0x5,
    },
    CodeRange {
        rva: 0xe206a0,
        length: 0x107,
    },
    CodeRange {
        rva: 0x676cf0,
        length: 0x1aef,
    },
    CodeRange {
        rva: 0x67a6d0,
        length: 0x259,
    },
    CodeRange {
        rva: 0x680420,
        length: 0x5eb,
    },
    CodeRange {
        rva: 0x679a40,
        length: 0xd,
    },
    CodeRange {
        rva: 0x679a50,
        length: 0xd,
    },
    CodeRange {
        rva: 0x679a60,
        length: 0xd,
    },
    CodeRange {
        rva: 0x679a70,
        length: 0xd,
    },
    CodeRange {
        rva: 0x67a9c0,
        length: 0xc,
    },
    CodeRange {
        rva: 0x67a9d0,
        length: 0xc,
    },
    CodeRange {
        rva: 0x67b5d0,
        length: 0x12,
    },
    CodeRange {
        rva: 0x67b5f0,
        length: 0x16,
    },
    CodeRange {
        rva: 0x67b610,
        length: 0x1a,
    },
    CodeRange {
        rva: 0x427800,
        length: 0x9d,
    },
    CodeRange {
        rva: 0x3b65b0,
        length: 0x5c1,
    },
    CodeRange {
        rva: 0x67a070,
        length: 0xb,
    },
    CodeRange {
        rva: 0x67a080,
        length: 0xb,
    },
    CodeRange {
        rva: 0x67a090,
        length: 0xb,
    },
    CodeRange {
        rva: 0x67a0a0,
        length: 0xb,
    },
    CodeRange {
        rva: 0x67a0b0,
        length: 0xb,
    },
    CodeRange {
        rva: 0x67a0c0,
        length: 0xb,
    },
    CodeRange {
        rva: 0x67ab70,
        length: 0xf,
    },
    CodeRange {
        rva: 0x6799e0,
        length: 0x57,
    },
    CodeRange {
        rva: 0x67b590,
        length: 0x3c,
    },
    CodeRange {
        rva: 0x3b6b80,
        length: 0x2a6,
    },
    CodeRange {
        rva: 0x427730,
        length: 0xc8,
    },
    CodeRange {
        rva: 0x3b8440,
        length: 0x223,
    },
];
const CAMERA_GLOBAL: u32 = 0x3d669c8;
const FIRST_CAMERA_OFFSET: u32 = 8;
const CAMERA_FLAGS_OFFSET: u32 = 0x28;
const MATRIX_OFFSET: u32 = 0x10;
const FRUSTUM_OFFSET: u32 = 0x50;
const WINDOW_GLOBAL: u32 = 0x458f910;
const WINDOW_HANDLE_OFFSET: u32 = 8;
const WINDOW_SIZE_OFFSET: u32 = 0x28;
const GAME_MANAGER_GLOBAL: u32 = 0x3d6d988;
const CAMERA_OVERRIDE_OFFSET: u32 = 0x58;
const CAMERA_OVERRIDE_FLAGS_OFFSET: u32 = 0x6c;
const CAMERA_PARAM_IDS_OFFSET: u32 = 0x40;
const CAMERA_FORCE_WORDS_OFFSET: u32 = 0x70;
const CAMERA_FORCE_FLAGS_OFFSET: u32 = 0x80;

/// Copied diagnostic state. Parameter IDs stay signed; method/flags remain raw,
/// without inventing force-mode enum values or coercing bytes to Rust bools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameCameraStateObservation {
    pub module_base: u64,
    pub epoch: u64,
    pub manager: ObjectObservation,
    /// Native storage order +40/+44/+48/+4c/+50/+54; no PARAM rows dereferenced.
    pub parameter_ids: [i32; 6],
    /// Native +70/+74, kept as raw bits including the maximum-float sentinel.
    pub force_angle_bits: [u32; 2],
    pub force_method_raw: u32,
    pub force_progress_bits: u32,
    /// Native +80/+81/+82. Independent getter outputs first/axis0/axis1 flags.
    pub force_flag_bytes: [u8; 3],
}

impl GameCameraStateObservation {
    /// Exact native predicate used by both the force-state getter and query.
    /// A request is not proof of applied camera orientation or current interpolation.
    pub fn force_requested(&self) -> bool {
        self.force_method_raw != 0
    }
}

/// Five raw float words and two bytes used by the native camera command.
/// Native order: +58/+5c/+60/+64/+68, then +6c/+6d. No Rust bool conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraOverridePayload {
    pub scalar_bits: [u32; 5],
    pub flag_bytes: [u8; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraOverrideObservation {
    pub module_base: u64,
    pub epoch: u64,
    pub manager: ObjectObservation,
    pub payload: CameraOverridePayload,
}

/// Actual single-field store. Byte entries never overwrite the neighbor/padding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraOverrideStore {
    pub address: u64,
    pub width: u8,
    pub before: u32,
    pub after: u32,
}

#[derive(Debug)]
pub struct CameraOverrideWrite {
    pub source: CameraOverrideObservation,
    pub replacement: CameraOverridePayload,
    pub stores: [Option<CameraOverrideStore>; 7],
    pub postflight: Result<CameraOverrideObservation>,
}

fn commit_camera_overrides(
    expected: CameraOverrideObservation,
    replacement: CameraOverridePayload,
    mut observe: impl FnMut() -> Result<Option<CameraOverrideObservation>>,
    mut store: impl FnMut(CameraOverrideStore),
) -> Result<CameraOverrideWrite> {
    if expected.epoch == 0
        || expected.manager.kind != ObjectKind::GameManager
        || expected.manager.address == 0
        || !expected.manager.address.is_multiple_of(8)
    {
        return Err(Error::InvalidProfile("invalid camera override source"));
    }
    let mut plan = [None; 7];
    for (index, step) in plan.iter_mut().enumerate() {
        let (offset, width, before, after) = if index < 5 {
            (
                CAMERA_OVERRIDE_OFFSET + index as u32 * 4,
                4,
                expected.payload.scalar_bits[index],
                replacement.scalar_bits[index],
            )
        } else {
            (
                CAMERA_OVERRIDE_FLAGS_OFFSET + (index - 5) as u32,
                1,
                u32::from(expected.payload.flag_bytes[index - 5]),
                u32::from(replacement.flag_bytes[index - 5]),
            )
        };
        let address = at(expected.manager.address, offset)?;
        address
            .checked_add(u64::from(width) - 1)
            .ok_or(Error::AddressOverflow)?;
        usize::try_from(address).map_err(|_| Error::AddressOverflow)?;
        if before != after {
            *step = Some(CameraOverrideStore {
                address,
                width,
                before,
                after,
            });
        }
    }
    let validate = |observed: Option<CameraOverrideObservation>, progress| {
        if observed == Some(progress) {
            Ok(progress)
        } else {
            Err(Error::InvalidProfile("camera override storage changed"))
        }
    };
    validate(observe()?, expected)?;
    let mut progress = expected;
    let mut receipt = CameraOverrideWrite {
        source: expected,
        replacement,
        stores: [None; 7],
        postflight: Ok(expected),
    };
    for (index, step) in plan.into_iter().enumerate() {
        let Some(step) = step else {
            continue;
        };
        if let Err(error) = observe().and_then(|observed| validate(observed, progress)) {
            receipt.postflight = Err(error);
            return Ok(receipt);
        }
        store(step);
        receipt.stores[index] = Some(step);
        if index < 5 {
            progress.payload.scalar_bits[index] = replacement.scalar_bits[index];
        } else {
            progress.payload.flag_bytes[index - 5] = replacement.flag_bytes[index - 5];
        }
        receipt.postflight = observe().and_then(|observed| validate(observed, progress));
        if receipt.postflight.is_err() {
            return Ok(receipt);
        }
    }
    receipt.postflight = Ok(progress);
    Ok(receipt)
}

/// Thread-confined explicit stores. No automatic restoration, retry or Drop writes.
pub struct CurrentCameraOverrides {
    file: shikifsrs_runtime::profile::OwnedVerifiedFile,
    patches: Option<std::sync::Arc<shikifsrs_runtime::code_patches::OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl CurrentCameraOverrides {
    /// # Safety
    /// Caller identifies this ER process and holds stable module/singleton allocation,
    /// initialized readable fields, and an exclusive permitted engine phase. No update,
    /// replacement, teardown, patch/unload, address reuse or conflicting aliases overlap.
    /// The optional owned hook manifest retains its full ABI/lifetime obligations.
    pub unsafe fn bind(
        bytes: std::sync::Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<std::sync::Arc<shikifsrs_runtime::code_patches::OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero camera override epoch"));
        }
        let file = shikifsrs_runtime::profile::OwnedVerifiedFile::verify(bytes, crate::WW_171)?;
        crate::Ww171File::from_verified(file.borrow()?)?.bind_objects_with_patches(
            &shikifsrs_runtime::memory::CurrentProcess,
            base,
            patches.as_deref(),
        )?;
        Ok(Self {
            file,
            patches,
            base,
            epoch,
            _thread: std::marker::PhantomData,
        })
    }

    pub fn observe(&self) -> Result<Option<CameraOverrideObservation>> {
        crate::Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(
                &shikifsrs_runtime::memory::CurrentProcess,
                self.base,
                self.patches.as_deref(),
            )?
            .game_camera_overrides(self.epoch)
    }

    /// Conditional seven-field replacement, also used with a fresh snapshot and an
    /// explicit saved payload for restoration. Negative/raw baseline bits are preserved.
    /// # Safety
    /// Bind obligations hold plus each selected word/byte has live writable extent.
    /// Caller proves payload numerical/flag semantics and excludes concurrent native
    /// resets, timer updates and ABA throughout all stores. This is not atomic.
    /// A failed postflight retains every actual store; reconcile before further work.
    pub unsafe fn compare_replace(
        &self,
        expected: CameraOverrideObservation,
        replacement: CameraOverridePayload,
    ) -> Result<CameraOverrideWrite> {
        if expected.epoch != self.epoch || expected.module_base != self.base {
            return Err(Error::InvalidProfile("foreign camera override session"));
        }
        commit_camera_overrides(
            expected,
            replacement,
            || self.observe(),
            |step| {
                // SAFETY: Caller supplies exclusive pinned writable u32/u8 destinations;
                // plan validates aligned word addresses and pointer-width bounds beforehand.
                unsafe {
                    if step.width == 4 {
                        (step.address as *mut u32).write_volatile(step.after);
                    } else {
                        (step.address as *mut u8).write_volatile(step.after as u8);
                    }
                }
            },
        )
    }

    /// # Safety
    /// Caller revokes earlier observations and independently supplies the bind contract
    /// for the new scene/lifetime. Advancing an epoch does not acquire an engine lock.
    pub unsafe fn advance_epoch(&mut self, epoch: u64) -> Result<()> {
        if epoch <= self.epoch {
            return Err(Error::InvalidProfile("camera epoch must advance"));
        }
        self.epoch = epoch;
        Ok(())
    }
}

/// Native window storage, separate from actual client/render-target dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowObservation {
    pub window: ObjectObservation,
    /// Opaque HWND value only; zero remains observable. No OS call or handle lease.
    pub handle: u64,
    pub configured_width: i32,
    pub configured_height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelExtent {
    pub width: u32,
    pub height: u32,
}

impl WindowObservation {
    /// Validate the stored positive pair. Does not establish a current client rect,
    /// backbuffer, viewport, DPI convention, or readiness of the HWND.
    pub fn configured_extent(&self) -> Result<PixelExtent> {
        if self.configured_width <= 0 || self.configured_height <= 0 {
            return Err(Error::InvalidProfile("invalid stored window dimensions"));
        }
        Ok(PixelExtent {
            width: self.configured_width as u32,
            height: self.configured_height as u32,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CamerasObservation {
    pub manager: ObjectObservation,
    pub perspectives: [Option<ObjectObservation>; 4],
    /// Raw bits; constructor clears six low bits without proving gameplay meanings.
    pub native_flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerspectiveCameraObservation {
    pub camera: ObjectObservation,
    pub matrix_bits: [[u32; 4]; 4],
    /// FOV, aspect, near and far in native order. Nonfinite bits remain observable.
    pub frustum_bits: [u32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerspectiveFrustum {
    pub fov_radians: f32,
    pub aspect_ratio: f32,
    pub near_clip: f32,
    pub far_clip: f32,
}

impl PerspectiveCameraObservation {
    /// Validate only a usable mathematical frustum. Does not prove camera axes,
    /// current render target, projection convention, or gameplay camera selection.
    pub fn frustum(&self) -> Result<PerspectiveFrustum> {
        let [fov, aspect, near, far] = self.frustum_bits.map(f32::from_bits);
        if [fov, aspect, near, far]
            .into_iter()
            .any(|value| !value.is_finite())
            || fov <= 0.0
            || fov >= std::f32::consts::PI
            || aspect <= 0.0
            || near <= 0.0
            || far <= near
        {
            return Err(Error::InvalidProfile("invalid perspective frustum"));
        }
        Ok(PerspectiveFrustum {
            fov_radians: fov,
            aspect_ratio: aspect,
            near_clip: near,
            far_clip: far,
        })
    }
}

fn at(base: u64, offset: u32) -> Result<u64> {
    base.checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Six independent parameter-ID getters and the native force-state getter.
    /// Rechecks reject visible replacement/field changes, not concurrent ABA.
    pub fn game_camera_state(&self, epoch: u64) -> Result<Option<GameCameraStateObservation>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero camera state epoch"));
        }
        let Some(manager) = self.singleton(GAME_MANAGER_GLOBAL, ObjectKind::GameManager)? else {
            return Ok(None);
        };
        let read = || -> Result<GameCameraStateObservation> {
            let memory = self.module.memory();
            let mut parameter_ids = [0; 6];
            for (i, id) in parameter_ids.iter_mut().enumerate() {
                *id = memory
                    .read_u32(at(manager.address, CAMERA_PARAM_IDS_OFFSET + i as u32 * 4)?)?
                    as i32;
            }
            let mut force_words = [0; 4];
            for (i, word) in force_words.iter_mut().enumerate() {
                *word = memory.read_u32(at(
                    manager.address,
                    CAMERA_FORCE_WORDS_OFFSET + i as u32 * 4,
                )?)?;
            }
            let mut force_flag_bytes = [0; 3];
            for (i, byte) in force_flag_bytes.iter_mut().enumerate() {
                *byte =
                    memory.read_u8(at(manager.address, CAMERA_FORCE_FLAGS_OFFSET + i as u32)?)?;
            }
            Ok(GameCameraStateObservation {
                module_base: self.module.base(),
                epoch,
                manager,
                parameter_ids,
                force_angle_bits: [force_words[0], force_words[1]],
                force_method_raw: force_words[2],
                force_progress_bits: force_words[3],
                force_flag_bytes,
            })
        };
        let result = read()?;
        if read()? != result
            || self.singleton(GAME_MANAGER_GLOBAL, ObjectKind::GameManager)? != Some(manager)
        {
            return Err(Error::InvalidProfile(
                "game camera state changed during observation",
            ));
        }
        Ok(Some(result))
    }

    /// Copy only the independently proved native camera command storage. Native
    /// periodic resets/timers remain observable; this does not pin a GameMan or camera.
    pub fn game_camera_overrides(&self, epoch: u64) -> Result<Option<CameraOverrideObservation>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero camera observation epoch"));
        }
        let Some(manager) = self.singleton(GAME_MANAGER_GLOBAL, ObjectKind::GameManager)? else {
            return Ok(None);
        };
        let read = || -> Result<CameraOverrideObservation> {
            let mut payload = CameraOverridePayload {
                scalar_bits: [0; 5],
                flag_bytes: [0; 2],
            };
            for (i, word) in payload.scalar_bits.iter_mut().enumerate() {
                *word = self
                    .module
                    .memory()
                    .read_u32(at(manager.address, CAMERA_OVERRIDE_OFFSET + i as u32 * 4)?)?;
            }
            for (i, byte) in payload.flag_bytes.iter_mut().enumerate() {
                *byte = self.module.memory().read_u8(at(
                    manager.address,
                    CAMERA_OVERRIDE_FLAGS_OFFSET + i as u32,
                )?)?;
            }
            Ok(CameraOverrideObservation {
                module_base: self.module.base(),
                epoch,
                manager,
                payload,
            })
        };
        let result = read()?;
        if read()? != result
            || self.singleton(GAME_MANAGER_GLOBAL, ObjectKind::GameManager)? != Some(manager)
        {
            return Err(Error::InvalidProfile(
                "camera override changed during observation",
            ));
        }
        Ok(Some(result))
    }

    /// Read the independently published singleton, HWND and stored size pair.
    /// The runtime-class descriptor and desktop position are not read as sizes.
    /// Copies detect visible changes, not lifetime/ABA or an atomic frame snapshot.
    pub fn window(&self) -> Result<Option<WindowObservation>> {
        let Some(window) = self.singleton(WINDOW_GLOBAL, ObjectKind::Window)? else {
            return Ok(None);
        };
        let read = || -> Result<WindowObservation> {
            let memory = self.module.memory();
            Ok(WindowObservation {
                window,
                handle: memory.read_u64(at(window.address, WINDOW_HANDLE_OFFSET)?)?,
                configured_width: memory.read_u32(at(window.address, WINDOW_SIZE_OFFSET)?)? as i32,
                configured_height: memory.read_u32(at(window.address, WINDOW_SIZE_OFFSET + 4)?)?
                    as i32,
            })
        };
        let result = read()?;
        if read()? != result || self.singleton(WINDOW_GLOBAL, ObjectKind::Window)? != Some(window) {
            return Err(Error::InvalidProfile(
                "window storage changed during observation",
            ));
        }
        Ok(Some(result))
    }

    /// Exact singleton and four constructor-linked perspective cameras. A null
    /// camera stays None, not a guessed fallback. No heap or camera lifetime lease.
    pub fn cameras(&self) -> Result<Option<CamerasObservation>> {
        let Some(manager) = self.singleton(CAMERA_GLOBAL, ObjectKind::CameraManager)? else {
            return Ok(None);
        };
        let read = || -> Result<CamerasObservation> {
            let mut perspectives = [None; 4];
            for (i, camera) in perspectives.iter_mut().enumerate() {
                let address = self
                    .module
                    .memory()
                    .read_u64(at(manager.address, FIRST_CAMERA_OFFSET + i as u32 * 8)?)?;
                if !address.is_multiple_of(16) {
                    return Err(Error::InvalidProfile("unaligned perspective camera"));
                }
                *camera = self.require(address, ObjectKind::PerspectiveCamera)?;
            }
            Ok(CamerasObservation {
                manager,
                perspectives,
                native_flags: self
                    .module
                    .memory()
                    .read_u32(at(manager.address, CAMERA_FLAGS_OFFSET)?)?,
            })
        };
        let result = read()?;
        if read()? != result
            || self.singleton(CAMERA_GLOBAL, ObjectKind::CameraManager)? != Some(manager)
        {
            return Err(Error::InvalidProfile(
                "camera singleton links changed during observation",
            ));
        }
        Ok(Some(result))
    }

    /// Copy only 64 matrix bytes and 16 frustum bytes against the exact primary
    /// vtable. Nonfinite payload remains raw for diagnostics; frustum() is explicit.
    /// Rechecks detect visible changes, not same-type ABA or an atomic scene snapshot.
    pub fn perspective_camera(&self, address: u64) -> Result<Option<PerspectiveCameraObservation>> {
        if !address.is_multiple_of(16) {
            return Err(Error::InvalidProfile("unaligned perspective camera"));
        }
        let Some(camera) = self.identify(address)? else {
            return Ok(None);
        };
        if !matches!(
            camera.kind,
            ObjectKind::PerspectiveCamera
                | ObjectKind::CharacterCamera
                | ObjectKind::CharacterFollowCamera
                | ObjectKind::CharacterAimCamera
                | ObjectKind::DistanceViewCamera
        ) {
            return Err(Error::InvalidProfile(
                "object has no proved perspective camera base",
            ));
        }
        let read = || -> Result<PerspectiveCameraObservation> {
            let mut matrix_bits = [[0; 4]; 4];
            let mut frustum_bits = [0; 4];
            for (i, value) in matrix_bits.iter_mut().flatten().enumerate() {
                *value = self
                    .module
                    .memory()
                    .read_u32(at(address, MATRIX_OFFSET + i as u32 * 4)?)?;
            }
            for (i, value) in frustum_bits.iter_mut().enumerate() {
                *value = self
                    .module
                    .memory()
                    .read_u32(at(address, FRUSTUM_OFFSET + i as u32 * 4)?)?;
            }
            Ok(PerspectiveCameraObservation {
                camera,
                matrix_bits,
                frustum_bits,
            })
        };
        let result = read()?;
        if read()? != result || self.require(address, camera.kind)? != Some(camera) {
            return Err(Error::InvalidProfile(
                "perspective camera changed during observation",
            ));
        }
        Ok(Some(result))
    }
}

#[cfg(test)]
mod override_tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn copy_owned(
        storage: &[u32; 28],
        source: CameraOverrideObservation,
    ) -> CameraOverrideObservation {
        let bytes = storage[27].to_le_bytes();
        CameraOverrideObservation {
            payload: CameraOverridePayload {
                scalar_bits: storage[22..27].try_into().unwrap(),
                flag_bytes: [bytes[0], bytes[1]],
            },
            ..source
        }
    }

    fn fixture() -> (RefCell<Box<[u32; 28]>>, CameraOverrideObservation) {
        let mut words = Box::new([0x91929394; 28]);
        words[22..27].copy_from_slice(&[0xbf800000, 0, 0x7fc12345, 0x80000000, 0x3f800000]);
        words[27] = 0xbbaa01fe;
        let source = CameraOverrideObservation {
            module_base: 0x140000000,
            epoch: 4,
            manager: ObjectObservation {
                address: words.as_ptr() as u64,
                kind: ObjectKind::GameManager,
            },
            payload: CameraOverridePayload {
                scalar_bits: [0; 5],
                flag_bytes: [0; 2],
            },
        };
        let source = copy_owned(&words, source);
        (RefCell::new(words), source)
    }

    fn store_owned(words: &RefCell<Box<[u32; 28]>>, step: CameraOverrideStore) {
        let mut storage = words.borrow_mut();
        let start = storage.as_mut_ptr() as u64;
        assert!(
            step.address >= start + 0x58 && step.address + u64::from(step.width) <= start + 0x6e
        );
        // SAFETY: The boxed original test storage is exclusively mutably borrowed,
        // validated in-bounds; word steps are aligned, byte steps use u8 alignment.
        unsafe {
            if step.width == 4 {
                (step.address as *mut u32).write_volatile(step.after);
            } else {
                (step.address as *mut u8).write_volatile(step.after as u8);
            }
        }
    }

    #[test]
    fn seven_fields_replace_and_explicitly_restore_raw_baseline_preserving_neighbors() {
        let (words, source) = fixture();
        let replacement = CameraOverridePayload {
            scalar_bits: [0x3f800000; 5],
            flag_bytes: [1, 0],
        };
        let receipt = commit_camera_overrides(
            source,
            replacement,
            || Ok(Some(copy_owned(&words.borrow(), source))),
            |step| store_owned(&words, step),
        )
        .unwrap();
        let current = receipt.postflight.unwrap();
        assert_eq!(current.payload, replacement);
        assert_eq!(receipt.stores.iter().flatten().count(), 6); // One float was already 1.
        assert_eq!(words.borrow()[27] & 0xffff0000, 0xbbaa0000);
        assert!(words.borrow()[..22].iter().all(|word| *word == 0x91929394));
        let no_op = commit_camera_overrides(
            current,
            replacement,
            || Ok(Some(copy_owned(&words.borrow(), source))),
            |_| panic!("no-op wrote"),
        )
        .unwrap();
        assert!(no_op.stores.iter().all(Option::is_none));
        let restored = commit_camera_overrides(
            current,
            source.payload,
            || Ok(Some(copy_owned(&words.borrow(), source))),
            |step| store_owned(&words, step),
        )
        .unwrap();
        assert_eq!(restored.postflight.unwrap(), source);
        assert_eq!(words.borrow()[27], 0xbbaa01fe);
    }

    #[test]
    fn first_post_failure_retains_actual_store_and_preflight_conflicts_do_not_write() {
        let (words, source) = fixture();
        let replacement = CameraOverridePayload {
            scalar_bits: [0x3f800000; 5],
            flag_bytes: [1, 0],
        };
        let count = Cell::new(0);
        let receipt = commit_camera_overrides(
            source,
            replacement,
            || Ok(Some(copy_owned(&words.borrow(), source))),
            |step| {
                store_owned(&words, step);
                count.set(count.get() + 1);
                words.borrow_mut()[26] = 0x40000000; // Independent native update breaks full payload postflight.
            },
        )
        .unwrap();
        assert!(receipt.postflight.is_err());
        assert_eq!(count.get(), 1);
        assert_eq!(receipt.stores.iter().flatten().count(), 1);
        assert_eq!(words.borrow()[22], replacement.scalar_bits[0]);
        assert_eq!(words.borrow()[23..26], source.payload.scalar_bits[1..4]);
        assert_eq!(words.borrow()[27], 0xbbaa01fe);
        assert!(
            commit_camera_overrides(
                source,
                replacement,
                || Ok(Some(copy_owned(&words.borrow(), source))),
                |_| panic!("conflict wrote")
            )
            .is_err()
        );
        for invalid in [
            CameraOverrideObservation { epoch: 0, ..source },
            CameraOverrideObservation {
                manager: ObjectObservation {
                    address: u64::MAX - 7,
                    ..source.manager
                },
                ..source
            },
        ] {
            assert!(
                commit_camera_overrides(
                    invalid,
                    replacement,
                    || Ok(Some(invalid)),
                    |_| panic!("invalid wrote")
                )
                .is_err()
            );
        }
    }
}
