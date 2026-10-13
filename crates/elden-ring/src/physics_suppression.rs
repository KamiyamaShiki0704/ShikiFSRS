//! Copied physics policy bytes. Effective suppression is recomputed by the engine.
use crate::{
    character_modules::{CharacterModule, CharacterModuleObservation},
    character_sets::CharacterIdentity,
    objects::GameObjectCatalog,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const SUPPRESSION_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x45bda0,
        length: 0x4c2,
    },
    CodeRange {
        rva: 0x467130,
        length: 0x14c,
    },
    CodeRange {
        rva: 0x461410,
        length: 0x1e0,
    },
    CodeRange {
        rva: 0x466660,
        length: 0x1b9,
    },
];
const POLICY_BYTES_OFFSET: u32 = 0x1d0;
const REQUEST_OFFSET: u32 = 0x1d3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsSuppressionObservation {
    pub module: CharacterModuleObservation,
    /// Raw policy bytes 1d0..1d3; no Boolean normalization or gameplay guarantee.
    pub bytes: [u8; 4],
}
impl PhysicsSuppressionObservation {
    pub fn effective_byte(self) -> u8 {
        self.bytes[2]
    }
    /// Nonzero forces effective_byte to 1 when the native policy next runs.
    /// Zero allows other native predicates to decide; it does not enable gravity.
    pub fn request_byte(self) -> u8 {
        self.bytes[3]
    }
}
#[derive(Debug)]
pub struct PhysicsSuppressionWrite {
    pub source: PhysicsSuppressionObservation,
    pub after_request: u8,
    pub changed: bool,
    /// A failed check does not undo or erase a completed one-byte store.
    pub postflight: Result<PhysicsSuppressionObservation>,
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Typed module/owner/full character identity and all four bytes are rechecked.
    /// This copy does not acquire an engine phase, lock, pin or ABA protection.
    pub fn physics_suppression(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<PhysicsSuppressionObservation>> {
        let Some(module) = self.character_module(target, CharacterModule::Physics)? else {
            return Ok(None);
        };
        let address = module
            .address
            .checked_add(u64::from(POLICY_BYTES_OFFSET))
            .ok_or(Error::AddressOverflow)?;
        let read = || -> Result<[u8; 4]> {
            let mut bytes = [0; 4];
            self.module.memory().read_exact(address, &mut bytes)?;
            Ok(bytes)
        };
        let bytes = read()?;
        if read()? != bytes
            || self.character_module(target, CharacterModule::Physics)? != Some(module)
        {
            return Err(Error::InvalidProfile(
                "physics suppression changed during observation",
            ));
        }
        Ok(Some(PhysicsSuppressionObservation { module, bytes }))
    }
}
pub(crate) struct SuppressionPlan {
    source: PhysicsSuppressionObservation,
    after: PhysicsSuppressionObservation,
    address: usize,
}
pub(crate) fn suppression_plan(
    current: Option<PhysicsSuppressionObservation>,
    expected: PhysicsSuppressionObservation,
    value: u8,
) -> Result<SuppressionPlan> {
    let m = expected.module;
    if current != Some(expected)
        || m.module != CharacterModule::Physics
        || m.character.epoch == 0
        || m.character.address == 0
        || m.container_address == 0
        || m.address == 0
        || !m.address.is_multiple_of(8)
        || !m.container_address.is_multiple_of(8)
        || !m.character.address.is_multiple_of(8)
    {
        return Err(Error::NativeOperationRejected(
            "physics suppression source superseded",
        ));
    }
    let address = m
        .address
        .checked_add(u64::from(REQUEST_OFFSET))
        .ok_or(Error::AddressOverflow)?;
    let mut after = expected;
    after.bytes[3] = value;
    Ok(SuppressionPlan {
        source: expected,
        after,
        address: usize::try_from(address).map_err(|_| Error::AddressOverflow)?,
    })
}
pub(crate) fn suppression_restore_plan(
    current: Option<PhysicsSuppressionObservation>,
    applied: &PhysicsSuppressionWrite,
) -> Result<SuppressionPlan> {
    let current = current.ok_or(Error::NativeOperationRejected(
        "physics suppression unavailable",
    ))?;
    if current.module != applied.source.module || current.request_byte() != applied.after_request {
        return Err(Error::NativeOperationRejected(
            "physics suppression request superseded",
        ));
    }
    // Native policy may have changed the other three bytes. Preserve their current values.
    suppression_plan(Some(current), current, applied.source.request_byte())
}
pub(crate) fn commit_suppression(
    plan: SuppressionPlan,
    write: impl FnOnce(usize, u8),
    observe: impl FnOnce() -> Result<PhysicsSuppressionObservation>,
) -> PhysicsSuppressionWrite {
    let after_request = plan.after.request_byte();
    let changed = plan.source.request_byte() != after_request;
    let postflight = if changed {
        write(plan.address, after_request);
        observe().and_then(|observed| {
            if observed == plan.after {
                Ok(observed)
            } else {
                Err(Error::InvalidProfile(
                    "physics suppression postflight changed",
                ))
            }
        })
    } else {
        Ok(plan.source)
    };
    PhysicsSuppressionWrite {
        source: plan.source,
        after_request,
        changed,
        postflight,
    }
}
pub(crate) unsafe fn store_request(address: usize, value: u8) {
    // SAFETY: Caller independently supplies the exact exclusive live writable byte extent.
    unsafe {
        (address as *mut u8).write(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(address: u64) -> PhysicsSuppressionObservation {
        PhysicsSuppressionObservation {
            module: CharacterModuleObservation {
                character: CharacterIdentity {
                    address: 0x1000,
                    world_address: 0x2000,
                    native_identifier: 0xfedcba9876543210,
                    epoch: 3,
                },
                module: CharacterModule::Physics,
                container_address: 0x3000,
                address,
            },
            bytes: [0x81, 0x42, 0, 0],
        }
    }
    #[test]
    fn request_store_and_restore_preserve_adjacent_policy_changes() {
        #[repr(align(16))]
        struct Storage([u8; 0x200]);
        let mut storage = Storage([0xa5; 0x200]);
        let before = source(storage.0.as_mut_ptr() as u64);
        storage.0[0x1d0..0x1d4].copy_from_slice(&before.bytes);
        let mut expected = storage.0;
        let plan = suppression_plan(Some(before), before, 255).unwrap();
        let after = plan.after;
        let applied = commit_suppression(
            plan,
            |address, value| {
                // SAFETY: Original owned exclusive byte array, not game memory.
                unsafe { store_request(address, value) };
            },
            || Ok(after),
        );
        expected[0x1d3] = 255;
        assert_eq!(storage.0, expected);
        assert!(applied.changed && applied.postflight.is_ok());
        let mut current = after;
        current.bytes[..3].copy_from_slice(&[0, 255, 1]);
        storage.0[0x1d0..0x1d3].copy_from_slice(&current.bytes[..3]);
        expected[0x1d0..0x1d3].copy_from_slice(&current.bytes[..3]);
        let plan = suppression_restore_plan(Some(current), &applied).unwrap();
        let restored = plan.after;
        let receipt = commit_suppression(
            plan,
            |address, value| {
                // SAFETY: Same live original array, independently held exclusive test phase.
                unsafe { store_request(address, value) };
            },
            || Ok(restored),
        );
        expected[0x1d3] = 0;
        assert_eq!(storage.0, expected);
        assert!(receipt.postflight.is_ok());
        assert_eq!(restored.effective_byte(), 1); // Clearing request did not clear engine output.
    }
    #[test]
    fn conflicts_failure_and_noop_keep_request_obligations_explicit() {
        let before = source(0x4000);
        assert!(suppression_plan(None, before, 1).is_err());
        for mode in 0..11 {
            let mut changed = before;
            match mode {
                0 => changed.module.character.address += 8,
                1 => changed.module.character.world_address += 8,
                2 => changed.module.character.native_identifier ^= 1,
                3 => changed.module.character.epoch += 1,
                4 => changed.module.address += 8,
                5 => changed.module.container_address += 8,
                6 => changed.module.module = CharacterModule::Ride,
                i => changed.bytes[i - 7] ^= 1,
            }
            assert!(suppression_plan(Some(changed), before, 1).is_err());
        }
        for address in [0, 1, u64::MAX - 7] {
            let bad = source(address);
            assert!(suppression_plan(Some(bad), bad, 1).is_err());
        }
        let plan = suppression_plan(Some(before), before, 1).unwrap();
        let after = plan.after;
        let calls = std::cell::Cell::new(0);
        let applied = commit_suppression(
            plan,
            |_, _| calls.set(calls.get() + 1),
            || Err(Error::InvalidProfile("copy lost")),
        );
        assert!(applied.changed && applied.postflight.is_err());
        assert_eq!(calls.get(), 1);
        assert!(suppression_restore_plan(None, &applied).is_err());
        let mut foreign = after;
        foreign.bytes[3] = 2;
        assert!(suppression_restore_plan(Some(foreign), &applied).is_err());
        foreign = after;
        foreign.module.address += 8;
        assert!(suppression_restore_plan(Some(foreign), &applied).is_err());
        assert!(suppression_restore_plan(Some(after), &applied).is_ok());
        let noop = suppression_plan(Some(before), before, 0).unwrap();
        assert!(!commit_suppression(noop, |_, _| unreachable!(), || unreachable!()).changed);
    }
}
