//! Narrow copied position publication. Does not invoke the native setter or reset its caches.
use crate::{
    character_sets::CharacterIdentity,
    character_spatial::{CharacterSpatialObservation, POSITION_OFFSET},
    objects::GameObjectCatalog,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};

// Native 45fe70 stores word90=0101 and byte1de=1 after both position vectors.
// This interface deliberately changes only primary70 and these publication bytes.
const PUBLICATION_OFFSETS: [u32; 3] = [0x90, 0x91, 0x1de];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsPositionRequestObservation {
    pub spatial: CharacterSpatialObservation,
    pub publication_bytes: [u8; 3],
}
#[derive(Debug)]
pub struct PhysicsPositionRequestWrite {
    pub source: PhysicsPositionRequestObservation,
    pub position_bits: [u32; 4],
    /// The four selected stores occurred before postflight, including on failure.
    pub postflight: Result<PhysicsPositionRequestObservation>,
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    pub fn physics_position_request(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<PhysicsPositionRequestObservation>> {
        let Some(spatial) = self.character_spatial(target)? else {
            return Ok(None);
        };
        let read = || -> Result<[u8; 3]> {
            let mut bytes = [0; 3];
            for (index, offset) in PUBLICATION_OFFSETS.into_iter().enumerate() {
                let address = spatial
                    .module
                    .address
                    .checked_add(u64::from(offset))
                    .ok_or(Error::AddressOverflow)?;
                self.module
                    .memory()
                    .read_exact(address, &mut bytes[index..index + 1])?;
            }
            Ok(bytes)
        };
        let publication_bytes = read()?;
        if read()? != publication_bytes || self.character_spatial(target)? != Some(spatial) {
            return Err(Error::NativeOperationRejected(
                "physics position publication changed",
            ));
        }
        Ok(Some(PhysicsPositionRequestObservation {
            spatial,
            publication_bytes,
        }))
    }
}
pub(crate) fn position_request_plan(
    current: Option<PhysicsPositionRequestObservation>,
    source: PhysicsPositionRequestObservation,
    epoch: u64,
    position_bits: [u32; 4],
) -> Result<PhysicsPositionRequestObservation> {
    if epoch == 0 || source.spatial.module.character.epoch != epoch || current != Some(source) {
        return Err(Error::NativeOperationRejected(
            "superseded physics position publication",
        ));
    }
    if position_bits[..3]
        .iter()
        .any(|bits| !f32::from_bits(*bits).is_finite())
    {
        return Err(Error::InvalidProfile("nonfinite requested position"));
    }
    // Validate every selected extent before the first store.
    for (offset, width) in [(POSITION_OFFSET, 16), (0x90, 1), (0x91, 1), (0x1de, 1)] {
        source
            .spatial
            .module
            .address
            .checked_add(u64::from(offset) + width)
            .ok_or(Error::AddressOverflow)?;
    }
    let mut planned = source;
    planned.spatial.position_bits = position_bits;
    planned.publication_bytes = [1; 3];
    Ok(planned)
}
pub(crate) fn commit_position_request(
    source: PhysicsPositionRequestObservation,
    planned: PhysicsPositionRequestObservation,
    store: impl FnOnce(u64, [u32; 4]),
    observe: impl FnOnce() -> Result<PhysicsPositionRequestObservation>,
) -> PhysicsPositionRequestWrite {
    store(source.spatial.module.address, planned.spatial.position_bits);
    let postflight = observe().and_then(|after| {
        if after != planned {
            Err(Error::NativeOperationRejected(
                "physics position publication postflight changed",
            ))
        } else {
            Ok(after)
        }
    });
    PhysicsPositionRequestWrite {
        source,
        position_bits: planned.spatial.position_bits,
        postflight,
    }
}
/// # Safety
/// Caller owns the complete pinned exclusive writable physics graph and every selected extent.
pub(crate) unsafe fn store_position_request(address: u64, position: [u32; 4]) {
    // SAFETY: All extents were checked before dispatch; caller excludes native updates and aliases.
    unsafe {
        std::ptr::copy_nonoverlapping(
            position.as_ptr().cast::<u8>(),
            (address + u64::from(POSITION_OFFSET)) as *mut u8,
            16,
        );
    }
    for offset in PUBLICATION_OFFSETS {
        // SAFETY: Same independently pinned writable-storage contract, selected byte only.
        unsafe {
            std::ptr::write((address + u64::from(offset)) as *mut u8, 1);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::character_modules::{CharacterModule, CharacterModuleObservation};
    fn fixture(address: u64) -> PhysicsPositionRequestObservation {
        PhysicsPositionRequestObservation {
            spatial: CharacterSpatialObservation {
                module: CharacterModuleObservation {
                    character: CharacterIdentity {
                        address: 0x1000,
                        native_identifier: 7,
                        world_address: 0x1800,
                        epoch: 3,
                    },
                    container_address: 0x2000,
                    address,
                    module: CharacterModule::Physics,
                },
                primary_rotation_bits: [2; 4],
                secondary_rotation_bits: [3; 4],
                position_bits: [1.0_f32.to_bits(); 4],
                secondary_position_bits: [4; 4],
            },
            publication_bytes: [0, 7, 8],
        }
    }
    #[test]
    fn narrow_publication_preserves_secondary_rotation_and_other_storage_and_retains_failure() {
        let mut memory = [0xa5u8; 0x200];
        let source = fixture(memory.as_mut_ptr() as u64);
        let requested = [
            2.0_f32.to_bits(),
            (-0.0_f32).to_bits(),
            5.0_f32.to_bits(),
            0x7fc01234,
        ];
        let plan = position_request_plan(Some(source), source, 3, requested).unwrap();
        let before = memory;
        let receipt = commit_position_request(
            source,
            plan,
            |address, values| {
                // SAFETY: This test exclusively owns the complete byte buffer and all extents.
                unsafe { store_position_request(address, values) };
            },
            || {
                Err(Error::NativeOperationRejected(
                    "injected postflight failure",
                ))
            },
        );
        assert!(receipt.postflight.is_err());
        assert_eq!(receipt.position_bits, requested);
        assert_eq!(receipt.source, source);
        let copied_position: [u32; 4] = std::array::from_fn(|index| {
            u32::from_le_bytes(
                memory[0x70 + index * 4..0x74 + index * 4]
                    .try_into()
                    .unwrap(),
            )
        });
        assert_eq!(copied_position, requested);
        for index in 0..memory.len() {
            if (0x70..0x80).contains(&index) {
                continue;
            }
            assert_eq!(
                memory[index],
                if [0x90, 0x91, 0x1de].contains(&index) {
                    1
                } else {
                    before[index]
                }
            );
        }
        assert_eq!(
            plan.spatial.secondary_position_bits,
            source.spatial.secondary_position_bits
        );
        assert!(
            commit_position_request(source, plan, |_, _| {}, || Ok(plan))
                .postflight
                .is_ok()
        );
        let mut changed = plan;
        changed.spatial.secondary_position_bits[0] ^= 1;
        assert!(
            commit_position_request(source, plan, |_, _| {}, || Ok(changed))
                .postflight
                .is_err()
        );
    }
    #[test]
    fn superseded_identity_flags_payload_epoch_extent_and_nonfinite_xyz_are_rejected_before_store()
    {
        let source = fixture(0x3000);
        let requested = [0; 4];
        for index in 0..11 {
            let mut foreign = source;
            match index {
                0 => foreign.spatial.module.character.epoch += 1,
                1 => foreign.spatial.module.character.native_identifier += 1,
                2 => foreign.spatial.module.character.address += 1,
                3 => foreign.spatial.module.address += 1,
                4 => foreign.spatial.module.container_address += 1,
                5 => foreign.spatial.position_bits[0] ^= 1,
                6 => foreign.spatial.secondary_position_bits[0] ^= 1,
                7 => foreign.publication_bytes[0] ^= 1,
                8 => foreign.publication_bytes[1] ^= 1,
                9 => foreign.publication_bytes[2] ^= 1,
                _ => foreign.spatial.module.character.world_address += 1,
            }
            assert!(position_request_plan(Some(foreign), source, 3, requested).is_err());
        }
        for epoch in [0, 1, 4] {
            assert!(position_request_plan(Some(source), source, epoch, requested).is_err());
        }
        assert!(position_request_plan(None, source, 3, requested).is_err());
        for index in 0..3 {
            for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut values = requested;
                values[index] = invalid.to_bits();
                assert!(position_request_plan(Some(source), source, 3, values).is_err());
            }
        }
        let invalid = fixture(u64::MAX - 0x100);
        assert!(position_request_plan(Some(invalid), invalid, 3, requested).is_err());
    }
}
