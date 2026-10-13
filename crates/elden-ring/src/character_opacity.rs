//! Independently proved raw opacity factors, not the complete rendered visibility predicate.
use crate::{
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
pub(crate) const OPACITY_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3e6e30,
        length: 0x99f,
    },
    CodeRange {
        rva: 0x3efb30,
        length: 0x69,
    },
    CodeRange {
        rva: 0x3efba0,
        length: 0x6f,
    },
    CodeRange {
        rva: 0x656980,
        length: 0x87,
    },
    CodeRange {
        rva: 0x656a10,
        length: 0x7f,
    },
    CodeRange {
        rva: 0x3edd60,
        length: 0x139,
    },
    CodeRange {
        rva: 0x3ea090,
        length: 0x10f,
    },
    CodeRange {
        rva: 0x3faa80,
        length: 0x443,
    },
    CodeRange {
        rva: 0x3edea0,
        length: 0x133,
    },
    CodeRange {
        rva: 0x3f59c0,
        length: 0x2dc,
    },
];
pub(crate) const BASE_OPACITY_OFFSET: u32 = 0x24c;
pub(crate) const OPACITY_FACTOR0_OFFSET: u32 = 0x238;
pub(crate) const OPACITY_FACTOR1_OFFSET: u32 = 0x240;
pub(crate) const OPACITY_FACTOR2_OFFSET: u32 = 0x248;
pub(crate) const OPACITY_WORD23C_OFFSET: u32 = 0x23c;
pub(crate) const OPACITY_CONTROL244_OFFSET: u32 = 0x244;
pub(crate) const BASE_OPACITY_CONTROL250_OFFSET: u32 = 0x250;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterOpacityObservation {
    pub character: CharacterIdentity,
    pub kind: ObjectKind,
    /// [base24c, factor238, factor240, factor248], preserving all raw f32 bits.
    pub factor_bits: [u32; 4],
    /// [word23c, control244, control250], copied as raw words. These are not
    /// additional multiplicative alpha factors; negative control250 is native.
    pub control_bits: [u32; 3],
}
impl CharacterOpacityObservation {
    pub fn factors(self) -> [f32; 4] {
        self.factor_bits.map(f32::from_bits)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManualOpacityStore {
    pub offset: u32,
    pub before_bits: u32,
    pub after_bits: u32,
}
#[derive(Debug)]
pub struct ManualOpacityWrite {
    pub source: CharacterOpacityObservation,
    /// Explicit caller values: [control244, control250, factor240, base24c].
    pub requested_bits: [u32; 4],
    /// Actual sequential stores, including those preceding a failed postflight.
    pub stores: [Option<ManualOpacityStore>; 4],
    pub postflight: Result<CharacterOpacityObservation>,
}
pub(crate) struct ManualOpacityPlan {
    source: CharacterOpacityObservation,
    bits: [u32; 4],
    addresses: [usize; 4],
}
pub(crate) fn manual_opacity_plan(
    current: Option<CharacterOpacityObservation>,
    expected: CharacterOpacityObservation,
    bits: [u32; 4],
) -> Result<ManualOpacityPlan> {
    if current != Some(expected)
        || expected.character.epoch == 0
        || expected.character.address == 0
        || !expected.character.address.is_multiple_of(8)
        || expected.character.world_address == 0
        || !expected.character.world_address.is_multiple_of(8)
        || !matches!(
            expected.kind,
            ObjectKind::Character | ObjectKind::Enemy | ObjectKind::Player
        )
    {
        return Err(Error::NativeOperationRejected(
            "manual opacity source superseded",
        ));
    }
    for (index, word) in bits.into_iter().enumerate() {
        let value = f32::from_bits(word);
        if !value.is_finite() || (index >= 2 && value < 0.0) {
            return Err(Error::InvalidProfile("invalid manual opacity value"));
        }
    }
    let mut addresses = [0; 4];
    for (address, offset) in addresses.iter_mut().zip(manual_offsets()) {
        *address = usize::try_from(
            expected
                .character
                .address
                .checked_add(u64::from(offset))
                .ok_or(Error::AddressOverflow)?,
        )
        .map_err(|_| Error::AddressOverflow)?;
    }
    Ok(ManualOpacityPlan {
        source: expected,
        bits,
        addresses,
    })
}
fn manual_offsets() -> [u32; 4] {
    [
        OPACITY_CONTROL244_OFFSET,
        BASE_OPACITY_CONTROL250_OFFSET,
        OPACITY_FACTOR1_OFFSET,
        BASE_OPACITY_OFFSET,
    ]
}
fn manual_word(source: CharacterOpacityObservation, index: usize) -> u32 {
    match index {
        0 => source.control_bits[1],
        1 => source.control_bits[2],
        2 => source.factor_bits[2],
        _ => source.factor_bits[0],
    }
}
fn set_manual_word(source: &mut CharacterOpacityObservation, index: usize, bits: u32) {
    match index {
        0 => source.control_bits[1] = bits,
        1 => source.control_bits[2] = bits,
        2 => source.factor_bits[2] = bits,
        _ => source.factor_bits[0] = bits,
    }
}
pub(crate) fn commit_manual_opacity(
    plan: ManualOpacityPlan,
    mut store: impl FnMut(usize, u32),
    mut observe: impl FnMut() -> Result<CharacterOpacityObservation>,
) -> ManualOpacityWrite {
    let mut result = ManualOpacityWrite {
        source: plan.source,
        requested_bits: plan.bits,
        stores: [None; 4],
        postflight: Ok(plan.source),
    };
    let mut expected = plan.source;
    for (index, offset) in manual_offsets().into_iter().enumerate() {
        let before_bits = manual_word(expected, index);
        if before_bits == plan.bits[index] {
            continue;
        }
        match observe() {
            Ok(current) if current == expected => {}
            Ok(_) => {
                result.postflight =
                    Err(Error::InvalidProfile("manual opacity changed before store"));
                return result;
            }
            Err(error) => {
                result.postflight = Err(error);
                return result;
            }
        }
        result.stores[index] = Some(ManualOpacityStore {
            offset,
            before_bits,
            after_bits: plan.bits[index],
        });
        store(plan.addresses[index], plan.bits[index]);
        set_manual_word(&mut expected, index, plan.bits[index]);
        result.postflight = observe().and_then(|current| {
            if current == expected {
                Ok(current)
            } else {
                Err(Error::InvalidProfile("manual opacity changed after store"))
            }
        });
        if result.postflight.is_err() {
            return result;
        }
    }
    result
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copies four proved factors and three separate control words, rechecking full identity.
    /// The native common getter conditionally multiplies these; PlayerIns additionally calls
    /// virtual4b8. Debug switches, virtual factors, rendering, visibility and readiness are not
    /// evaluated here. No native getter, resource graph, lock or engine pin is acquired.
    pub fn character_opacity(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterOpacityObservation>> {
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale character opacity identity"));
        }
        let character = self
            .character(target.address)?
            .ok_or(Error::InvalidProfile("opacity character disappeared"))?;
        let read = || -> Result<[u32; 4]> {
            let mut bits = [0; 4];
            for (out, offset) in bits.iter_mut().zip([
                BASE_OPACITY_OFFSET,
                OPACITY_FACTOR0_OFFSET,
                OPACITY_FACTOR1_OFFSET,
                OPACITY_FACTOR2_OFFSET,
            ]) {
                *out = self.module.memory().read_u32(
                    target
                        .address
                        .checked_add(u64::from(offset))
                        .ok_or(Error::AddressOverflow)?,
                )?;
            }
            Ok(bits)
        };
        let factor_bits = read()?;
        let read_controls = || -> Result<[u32; 3]> {
            let mut bits = [0; 3];
            for (out, offset) in bits.iter_mut().zip([
                OPACITY_WORD23C_OFFSET,
                OPACITY_CONTROL244_OFFSET,
                BASE_OPACITY_CONTROL250_OFFSET,
            ]) {
                *out = self.module.memory().read_u32(
                    target
                        .address
                        .checked_add(u64::from(offset))
                        .ok_or(Error::AddressOverflow)?,
                )?;
            }
            Ok(bits)
        };
        let control_bits = read_controls()?;
        if read()? != factor_bits
            || read_controls()? != control_bits
            || self.character(target.address)? != Some(character)
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile(
                "character opacity changed during observation",
            ));
        }
        Ok(Some(CharacterOpacityObservation {
            character: target,
            kind: character.kind,
            factor_bits,
            control_bits,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    #[repr(align(8))]
    struct Storage([u8; 0x260]);
    fn observation(address: u64) -> CharacterOpacityObservation {
        CharacterOpacityObservation {
            character: CharacterIdentity {
                address,
                world_address: 0x2000,
                native_identifier: 0x1234567810000003,
                epoch: 4,
            },
            kind: ObjectKind::Player,
            factor_bits: [0.8f32, 0.35, 0.7, 0.6].map(f32::to_bits),
            control_bits: [0x7fc12345, (-0.25f32).to_bits(), (-0.5f32).to_bits()],
        }
    }
    fn write_initial(storage: &mut Storage, source: CharacterOpacityObservation) {
        for (offset, bits) in [
            BASE_OPACITY_OFFSET,
            OPACITY_FACTOR0_OFFSET,
            OPACITY_FACTOR1_OFFSET,
            OPACITY_FACTOR2_OFFSET,
        ]
        .into_iter()
        .zip(source.factor_bits)
        .chain(
            [
                OPACITY_WORD23C_OFFSET,
                OPACITY_CONTROL244_OFFSET,
                BASE_OPACITY_CONTROL250_OFFSET,
            ]
            .into_iter()
            .zip(source.control_bits),
        ) {
            storage.0[offset as usize..offset as usize + 4].copy_from_slice(&bits.to_le_bytes());
        }
    }
    fn read_owned(
        storage: &Storage,
        mut source: CharacterOpacityObservation,
    ) -> CharacterOpacityObservation {
        let read = |offset: u32| {
            u32::from_le_bytes(
                storage.0[offset as usize..offset as usize + 4]
                    .try_into()
                    .unwrap(),
            )
        };
        source.factor_bits = [
            BASE_OPACITY_OFFSET,
            OPACITY_FACTOR0_OFFSET,
            OPACITY_FACTOR1_OFFSET,
            OPACITY_FACTOR2_OFFSET,
        ]
        .map(read);
        source.control_bits = [
            OPACITY_WORD23C_OFFSET,
            OPACITY_CONTROL244_OFFSET,
            BASE_OPACITY_CONTROL250_OFFSET,
        ]
        .map(read);
        source
    }
    #[test]
    fn manual_opacity_changes_only_four_controls_and_preserves_animation_and_opaque_neighbors() {
        let storage = RefCell::new(Storage([0x5a; 0x260]));
        let source = observation(storage.borrow_mut().0.as_mut_ptr() as u64);
        write_initial(&mut storage.borrow_mut(), source);
        let before = storage.borrow().0;
        let stores = RefCell::new(Vec::new());
        let bits = [0f32; 4].map(f32::to_bits);
        let plan = manual_opacity_plan(Some(source), source, bits).unwrap();
        let receipt = commit_manual_opacity(
            plan,
            |address, word| {
                stores
                    .borrow_mut()
                    .push(address as u64 - source.character.address);
                let mut owned = storage.borrow_mut();
                let offset = address as u64 - source.character.address;
                let pointer = owned.0[offset as usize..offset as usize + 4].as_mut_ptr();
                // SAFETY: Original aligned exclusive owned initialized array, no game allocations.
                unsafe { pointer.cast::<u32>().write(word) }
            },
            || Ok(read_owned(&storage.borrow(), source)),
        );
        assert!(receipt.postflight.is_ok());
        assert_eq!(*stores.borrow(), [0x244, 0x250, 0x240, 0x24c]);
        assert!(receipt.stores.iter().all(Option::is_some));
        let mut expected = before;
        for offset in manual_offsets() {
            expected[offset as usize..offset as usize + 4].fill(0);
        }
        assert_eq!(storage.borrow().0, expected);
        let parked = receipt.postflight.unwrap();
        assert_eq!(parked.control_bits[0], source.control_bits[0]);
        assert_eq!(parked.factor_bits[1], source.factor_bits[1]);
        assert_eq!(parked.factor_bits[3], source.factor_bits[3]);
        let noop = manual_opacity_plan(Some(parked), parked, bits).unwrap();
        assert!(
            commit_manual_opacity(noop, |_, _| unreachable!(), || unreachable!())
                .stores
                .iter()
                .all(Option::is_none)
        );
        let shown = [0f32, 0.0, 1.0, 1.0].map(f32::to_bits);
        let plan = manual_opacity_plan(Some(parked), parked, shown).unwrap();
        let current = Cell::new(parked);
        let result = commit_manual_opacity(
            plan,
            |address, bits| {
                let index = manual_offsets()
                    .iter()
                    .position(|offset| {
                        source.character.address + u64::from(*offset) == address as u64
                    })
                    .unwrap();
                let mut value = current.get();
                set_manual_word(&mut value, index, bits);
                current.set(value);
            },
            || Ok(current.get()),
        );
        assert!(result.postflight.is_ok());
        assert_eq!(result.stores.iter().flatten().count(), 2);
        assert_eq!(current.get().control_bits[0], source.control_bits[0]);
        assert_eq!(current.get().factor_bits[1], source.factor_bits[1]);
    }
    #[test]
    fn manual_opacity_rejects_bad_requests_and_stops_after_actual_partial_store() {
        let source = observation(0x3000);
        let bits = [0f32; 4].map(f32::to_bits);
        assert!(manual_opacity_plan(None, source, bits).is_err());
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for index in 0..4 {
                let mut values = bits;
                values[index] = bad.to_bits();
                assert!(manual_opacity_plan(Some(source), source, values).is_err());
            }
        }
        let mut signed = bits;
        signed[0] = (-1f32).to_bits();
        signed[1] = (-2f32).to_bits();
        assert!(manual_opacity_plan(Some(source), source, signed).is_ok());
        signed[2] = (-1f32).to_bits();
        assert!(manual_opacity_plan(Some(source), source, signed).is_err());
        for mode in 0..5 {
            let mut newer = source;
            match mode {
                0 => newer.character.native_identifier ^= 1 << 32,
                1 => newer.character.world_address += 8,
                2 => newer.character.epoch += 1,
                3 => newer.control_bits[0] ^= 1,
                _ => newer.kind = ObjectKind::Enemy,
            }
            assert!(manual_opacity_plan(Some(newer), source, bits).is_err());
        }
        for lost_read in [false, true] {
            let calls = Cell::new(0);
            let reads = Cell::new(0);
            let plan = manual_opacity_plan(Some(source), source, bits).unwrap();
            let receipt = commit_manual_opacity(
                plan,
                |address, value| {
                    assert_eq!(address, 0x3244);
                    assert_eq!(value, 0);
                    calls.set(calls.get() + 1);
                },
                || {
                    reads.set(reads.get() + 1);
                    if reads.get() == 1 {
                        return Ok(source);
                    }
                    if lost_read {
                        Err(Error::InvalidProfile("lost target"))
                    } else {
                        let mut newer = source;
                        newer.control_bits[1] = 0;
                        newer.character.native_identifier ^= 1 << 32;
                        Ok(newer)
                    }
                },
            );
            assert!(receipt.postflight.is_err());
            assert_eq!(calls.get(), 1);
            assert_eq!(receipt.stores.iter().flatten().count(), 1);
            assert_eq!(
                receipt.stores[0].unwrap().before_bits,
                source.control_bits[1]
            );
        }
    }
}
