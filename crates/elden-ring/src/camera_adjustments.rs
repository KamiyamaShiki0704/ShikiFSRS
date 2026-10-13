//! Selected perspective fields with typed owner links and explicit store receipts.
//! These copies and epochs do not supply engine exclusion, camera lifetime or ABA protection.
use crate::{
    cameras::PerspectiveCameraObservation,
    objects::{GameObjectCatalog, ObjectKind, ObjectObservation},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const CAMERA_ADJUSTMENT_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0xaf3be0,
        length: 0xf32,
    },
    CodeRange {
        rva: 0x3b0680,
        length: 0x194,
    },
    CodeRange {
        rva: 0x3b3730,
        length: 0x72c,
    },
    CodeRange {
        rva: 0x3aeab0,
        length: 0x15a,
    },
    CodeRange {
        rva: 0x3bcc20,
        length: 0x113,
    },
];
const WORLD_CAMERA_OFFSET: u32 = 0xf8;
const FOLLOW_CAMERA_OFFSET: u32 = 0x60;
const AIM_CAMERA_OFFSET: u32 = 0x68;
const DISTANCE_CAMERA_OFFSET: u32 = 0x70;

/// Only the four native slots and the four independently constructed character cameras.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraSlot {
    Perspective0,
    Perspective1,
    Perspective2,
    Perspective3,
    Character,
    Follow,
    Aim,
    Distance,
}
impl CameraSlot {
    fn perspective_index(self) -> Option<usize> {
        match self {
            Self::Perspective0 => Some(0),
            Self::Perspective1 => Some(1),
            Self::Perspective2 => Some(2),
            Self::Perspective3 => Some(3),
            _ => None,
        }
    }
}

/// The owner is the exact WorldChrManImp. Null subcamera slots remain absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterCamerasObservation {
    pub world: ObjectObservation,
    /// Character, follow, aim and distance, in this fixed order.
    pub cameras: [Option<ObjectObservation>; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraScalar {
    FieldOfView,
    PositionX,
    PositionY,
    PositionZ,
}
impl CameraScalar {
    fn offset(self) -> u64 {
        match self {
            Self::FieldOfView => 0x50,
            Self::PositionX => 0x40,
            Self::PositionY => 0x44,
            Self::PositionZ => 0x48,
        }
    }
    fn bits(self, copied: PerspectiveCameraObservation) -> u32 {
        match self {
            Self::FieldOfView => copied.frustum_bits[0],
            Self::PositionX => copied.matrix_bits[3][0],
            Self::PositionY => copied.matrix_bits[3][1],
            Self::PositionZ => copied.matrix_bits[3][2],
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraScalarKey {
    pub epoch: u64,
    pub slot: CameraSlot,
    pub field: CameraScalar,
}

/// Selected scalar plus the complete known owner path. No other matrix/frustum fields are writable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraScalarObservation {
    pub module_base: u64,
    pub key: CameraScalarKey,
    pub owner: ObjectObservation,
    /// Equal to camera for a top-level slot; character base for a subcamera.
    pub parent: ObjectObservation,
    pub camera: ObjectObservation,
    pub bits: u32,
}
impl CameraScalarObservation {
    pub fn value(self) -> f32 {
        f32::from_bits(self.bits)
    }
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Rechecks every copied link and primary type. The native constructor allocates
    /// all subcameras through the same perspective base; runtime scene ownership is external.
    pub fn character_cameras(&self) -> Result<Option<CharacterCamerasObservation>> {
        let Some(world) = self.world()? else {
            return Ok(None);
        };
        let memory = self.module.memory();
        let link = world
            .address
            .checked_add(u64::from(WORLD_CAMERA_OFFSET))
            .ok_or(Error::AddressOverflow)?;
        let read = || -> Result<CharacterCamerasObservation> {
            let mut cameras = [None; 4];
            let base = memory.read_u64(link)?;
            if !base.is_multiple_of(16) {
                return Err(Error::InvalidProfile("unaligned character camera"));
            }
            cameras[0] = self.require(base, ObjectKind::CharacterCamera)?;
            if base != 0 {
                for (index, offset, kind) in [
                    (1, FOLLOW_CAMERA_OFFSET, ObjectKind::CharacterFollowCamera),
                    (2, AIM_CAMERA_OFFSET, ObjectKind::CharacterAimCamera),
                    (3, DISTANCE_CAMERA_OFFSET, ObjectKind::DistanceViewCamera),
                ] {
                    let address = memory.read_u64(
                        base.checked_add(u64::from(offset))
                            .ok_or(Error::AddressOverflow)?,
                    )?;
                    if !address.is_multiple_of(16) {
                        return Err(Error::InvalidProfile("unaligned character subcamera"));
                    }
                    cameras[index] = self.require(address, kind)?;
                }
            }
            Ok(CharacterCamerasObservation { world, cameras })
        };
        let copied = read()?;
        if read()? != copied || self.world()? != Some(world) {
            return Err(Error::InvalidProfile("character camera owner path changed"));
        }
        Ok(Some(copied))
    }

    pub fn camera_scalar(&self, key: CameraScalarKey) -> Result<Option<CameraScalarObservation>> {
        if key.epoch == 0 {
            return Err(Error::InvalidProfile("zero camera scalar epoch"));
        }
        let resolve =
            || -> Result<Option<(ObjectObservation, ObjectObservation, ObjectObservation)>> {
                if let Some(index) = key.slot.perspective_index() {
                    let Some(cameras) = self.cameras()? else {
                        return Ok(None);
                    };
                    Ok(cameras.perspectives[index].map(|camera| (cameras.manager, camera, camera)))
                } else {
                    let Some(cameras) = self.character_cameras()? else {
                        return Ok(None);
                    };
                    let index = match key.slot {
                        CameraSlot::Character => 0,
                        CameraSlot::Follow => 1,
                        CameraSlot::Aim => 2,
                        CameraSlot::Distance => 3,
                        _ => unreachable!(),
                    };
                    Ok(cameras.cameras[0]
                        .zip(cameras.cameras[index])
                        .map(|(parent, camera)| (cameras.world, parent, camera)))
                }
            };
        let Some((owner, parent, camera)) = resolve()? else {
            return Ok(None);
        };
        let Some(copied) = self.perspective_camera(camera.address)? else {
            return Ok(None);
        };
        if copied.camera != camera || resolve()? != Some((owner, parent, camera)) {
            return Err(Error::InvalidProfile("camera scalar owner changed"));
        }
        Ok(Some(CameraScalarObservation {
            module_base: self.module.base(),
            key,
            owner,
            parent,
            camera,
            bits: key.field.bits(copied),
        }))
    }
}

/// An actual volatile store obligation exists even when postflight fails.
#[derive(Debug)]
pub struct CameraScalarWrite {
    pub source: CameraScalarObservation,
    pub replacement_bits: u32,
    pub address: u64,
    pub postflight: Result<CameraScalarObservation>,
}

fn commit_camera_scalar(
    expected: CameraScalarObservation,
    bits: u32,
    mut observe: impl FnMut() -> Result<Option<CameraScalarObservation>>,
    store: impl FnOnce(u64, u32),
) -> Result<CameraScalarWrite> {
    if expected.key.epoch == 0
        || expected.camera.address == 0
        || !expected.camera.address.is_multiple_of(16)
        || expected.owner.address == 0
        || expected.parent.address == 0
        || !f32::from_bits(bits).is_finite()
    {
        return Err(Error::InvalidProfile("invalid selected camera store"));
    }
    let address = expected
        .camera
        .address
        .checked_add(expected.key.field.offset())
        .filter(|value| *value <= usize::MAX as u64 - 3)
        .ok_or(Error::AddressOverflow)?;
    if observe()? != Some(expected) || observe()? != Some(expected) {
        return Err(Error::InvalidProfile("selected camera store conflict"));
    }
    store(address, bits);
    let replacement = CameraScalarObservation { bits, ..expected };
    let postflight = observe().and_then(|actual| {
        if actual == Some(replacement) {
            Ok(replacement)
        } else {
            Err(Error::InvalidProfile("selected camera postflight differs"))
        }
    });
    Ok(CameraScalarWrite {
        source: expected,
        replacement_bits: bits,
        address,
        postflight,
    })
}

/// Thread-confined writer; has no Drop stores, inferred phase or automatic retry.
pub struct CurrentCameraScalars {
    file: shikifsrs_runtime::profile::OwnedVerifiedFile,
    patches: Option<std::sync::Arc<shikifsrs_runtime::code_patches::OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl CurrentCameraScalars {
    /// # Safety
    /// Caller pins this module, exact camera manager/world/subcamera/full graph, excludes
    /// reset, replacement, native update, teardown, ABA, callback aliases and unload, and
    /// holds an exclusive permitted engine phase. Epochs/copies establish none of these.
    pub unsafe fn bind(
        bytes: std::sync::Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<std::sync::Arc<shikifsrs_runtime::code_patches::OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero camera scalar session"));
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
    pub fn observe(&self, key: CameraScalarKey) -> Result<Option<CameraScalarObservation>> {
        if key.epoch != self.epoch {
            return Err(Error::InvalidProfile("foreign camera scalar epoch"));
        }
        crate::Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(
                &shikifsrs_runtime::memory::CurrentProcess,
                self.base,
                self.patches.as_deref(),
            )?
            .camera_scalar(key)
    }
    /// Only one independently proved float word is stored. Caller establishes FOV/translation
    /// semantics; finite signed/zero baseline bits can be restored exactly.
    /// # Safety
    /// Bind obligations hold plus live writable selected u32 extent. Caller retains this
    /// actual receipt and reconciles failed postflight before any subsequent store.
    pub unsafe fn compare_replace(
        &self,
        expected: CameraScalarObservation,
        bits: u32,
    ) -> Result<CameraScalarWrite> {
        if expected.key.epoch != self.epoch || expected.module_base != self.base {
            return Err(Error::InvalidProfile("foreign camera scalar observation"));
        }
        commit_camera_scalar(
            expected,
            bits,
            || self.observe(expected.key),
            |address, bits| {
                // SAFETY: Caller supplies the full exclusive pinned writable word contract.
                unsafe {
                    (address as *mut u32).write_volatile(bits);
                }
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    fn source() -> CameraScalarObservation {
        let camera = ObjectObservation {
            address: 0x1000,
            kind: ObjectKind::CharacterAimCamera,
        };
        CameraScalarObservation {
            module_base: 0x140000000,
            key: CameraScalarKey {
                epoch: 9,
                slot: CameraSlot::Aim,
                field: CameraScalar::FieldOfView,
            },
            owner: ObjectObservation {
                address: 0x2000,
                kind: ObjectKind::WorldCharacters,
            },
            parent: ObjectObservation {
                address: 0x3000,
                kind: ObjectKind::CharacterCamera,
            },
            camera,
            bits: 0.8f32.to_bits(),
        }
    }
    #[test]
    fn each_selected_word_has_the_proved_address_and_preserves_other_storage() {
        for (field, offset) in [
            (CameraScalar::FieldOfView, 0x50),
            (CameraScalar::PositionX, 0x40),
            (CameraScalar::PositionY, 0x44),
            (CameraScalar::PositionZ, 0x48),
        ] {
            let initial = CameraScalarObservation {
                key: CameraScalarKey {
                    field,
                    ..source().key
                },
                ..source()
            };
            let current = RefCell::new(initial);
            let stores = Cell::new(0);
            let receipt = commit_camera_scalar(
                initial,
                (-0.0f32).to_bits(),
                || Ok(Some(*current.borrow())),
                |address, bits| {
                    assert_eq!(address, 0x1000 + offset);
                    stores.set(stores.get() + 1);
                    current.borrow_mut().bits = bits;
                },
            )
            .unwrap();
            assert_eq!(stores.get(), 1);
            assert_eq!(receipt.postflight.unwrap().bits, (-0.0f32).to_bits());
            assert_eq!(receipt.source, initial);
        }
    }
    #[test]
    fn replacement_owner_slot_epoch_or_second_preflight_conflict_never_stores() {
        let mut variants = [source(); 5];
        variants[0].owner.address += 8;
        variants[1].parent.address += 16;
        variants[2].camera.address += 16;
        variants[3].key.epoch += 1;
        variants[4].key.slot = CameraSlot::Follow;
        for foreign in variants {
            let reads = Cell::new(0);
            let stores = Cell::new(0);
            assert!(
                commit_camera_scalar(
                    source(),
                    1.0f32.to_bits(),
                    || {
                        reads.set(reads.get() + 1);
                        Ok(Some(if reads.get() == 1 { source() } else { foreign }))
                    },
                    |_, _| stores.set(1)
                )
                .is_err()
            );
            assert_eq!(stores.get(), 0);
        }
        assert!(
            commit_camera_scalar(
                source(),
                f32::NAN.to_bits(),
                || unreachable!(),
                |_, _| unreachable!()
            )
            .is_err()
        );
    }
    #[test]
    fn failed_postflight_returns_actual_receipt_with_no_retry_or_drop_write() {
        let reads = Cell::new(0);
        let stores = Cell::new(0);
        let receipt = commit_camera_scalar(
            source(),
            1.0f32.to_bits(),
            || {
                reads.set(reads.get() + 1);
                if reads.get() > 2 {
                    Err(Error::InvalidProfile("synthetic postflight loss"))
                } else {
                    Ok(Some(source()))
                }
            },
            |_, _| stores.set(stores.get() + 1),
        )
        .unwrap();
        assert_eq!(receipt.address, 0x1050);
        assert!(receipt.postflight.is_err());
        drop(receipt);
        assert_eq!(stores.get(), 1);
    }
}
