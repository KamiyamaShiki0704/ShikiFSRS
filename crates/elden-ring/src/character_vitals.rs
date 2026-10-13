//! Raw HP, MP and stamina, with explicit conditional current-word plans; no death-state inference.
use crate::{
    CharacterDataObservation, character_sets::CharacterIdentity, objects::GameObjectCatalog,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const VITALS_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0xd42d50,
        length: 0x1a0,
    },
    CodeRange {
        rva: 0x438fb0,
        length: 0x1af,
    },
    CodeRange {
        rva: 0x438870,
        length: 0x7a,
    },
    CodeRange {
        rva: 0x437450,
        length: 0x234,
    },
    CodeRange {
        rva: 0x438900,
        length: 0xc5,
    },
    CodeRange {
        rva: 0x4389f0,
        length: 0xd8,
    },
    CodeRange {
        rva: 0x439160,
        length: 0x14f,
    },
];

const HP_OFFSET: u32 = 0x138;
const MP_OFFSET: u32 = 0x148;
const STAMINA_OFFSET: u32 = 0x154;

/// Native signed words copied verbatim. Current stamina can legitimately be negative.
/// No clamping, ratio, alive/dead decision or gameplay readiness is implied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentMaximum {
    pub current: i32,
    pub maximum: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterVitalsObservation {
    pub character: CharacterIdentity,
    pub data: CharacterDataObservation,
    pub hp: CurrentMaximum,
    /// The NPC definition calls this MP; Elden Ring's UI uses FP.
    pub mp: CurrentMaximum,
    pub stamina: CurrentMaximum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterVital {
    Hp,
    Mp,
    Stamina,
}
impl CharacterVital {
    fn offset(self) -> u64 {
        u64::from(match self {
            Self::Hp => HP_OFFSET,
            Self::Mp => MP_OFFSET,
            Self::Stamina => STAMINA_OFFSET,
        })
    }
    pub fn pair(self, copy: CharacterVitalsObservation) -> CurrentMaximum {
        match self {
            Self::Hp => copy.hp,
            Self::Mp => copy.mp,
            Self::Stamina => copy.stamina,
        }
    }
    fn replace(self, copy: &mut CharacterVitalsObservation, value: i32) {
        match self {
            Self::Hp => copy.hp.current = value,
            Self::Mp => copy.mp.current = value,
            Self::Stamina => copy.stamina.current = value,
        }
    }
}
#[derive(Debug)]
pub struct CharacterVitalWrite {
    pub source: CharacterVitalsObservation,
    pub field: CharacterVital,
    pub after_current: i32,
    pub changed: bool,
    /// An executed store remains the caller's obligation when the full copy check fails.
    pub postflight: Result<CharacterVitalsObservation>,
}
pub(crate) struct VitalPlan {
    source: CharacterVitalsObservation,
    after: CharacterVitalsObservation,
    field: CharacterVital,
    address: usize,
}
pub(crate) fn vital_plan(
    current: Option<CharacterVitalsObservation>,
    expected: CharacterVitalsObservation,
    field: CharacterVital,
    value: i32,
) -> Result<VitalPlan> {
    if expected.character.epoch == 0
        || expected.character.address == 0
        || !expected.character.address.is_multiple_of(8)
        || expected.data.owner_address != expected.character.address
        || expected.data.address == 0
        || !expected.data.address.is_multiple_of(8)
        || expected.data.container_address == 0
        || !expected.data.container_address.is_multiple_of(8)
        || current != Some(expected)
    {
        return Err(Error::NativeOperationRejected(
            "vital current-word source superseded",
        ));
    }
    let address = expected
        .data
        .address
        .checked_add(field.offset())
        .ok_or(Error::AddressOverflow)?;
    address.checked_add(4).ok_or(Error::AddressOverflow)?;
    let mut after = expected;
    field.replace(&mut after, value);
    Ok(VitalPlan {
        source: expected,
        after,
        field,
        address: usize::try_from(address).map_err(|_| Error::AddressOverflow)?,
    })
}
pub(crate) fn vital_restore_plan(
    current: Option<CharacterVitalsObservation>,
    applied: &CharacterVitalWrite,
) -> Result<VitalPlan> {
    let current = current.ok_or(Error::NativeOperationRejected("vital restore unavailable"))?;
    if current.character != applied.source.character
        || current.data != applied.source.data
        || applied.field.pair(current).current != applied.after_current
    {
        return Err(Error::NativeOperationRejected(
            "vital owned current-word superseded",
        ));
    }
    vital_plan(
        Some(current),
        current,
        applied.field,
        applied.field.pair(applied.source).current,
    )
}
pub(crate) fn commit_vital(
    plan: VitalPlan,
    write: impl FnOnce(usize, i32),
    observe: impl FnOnce() -> Result<CharacterVitalsObservation>,
) -> CharacterVitalWrite {
    let after_current = plan.field.pair(plan.after).current;
    let changed = plan.field.pair(plan.source).current != after_current;
    let postflight = if changed {
        write(plan.address, after_current);
        observe().and_then(|value| {
            if value == plan.after {
                Ok(value)
            } else {
                Err(Error::InvalidProfile("vital store postflight changed"))
            }
        })
    } else {
        Ok(plan.source)
    };
    CharacterVitalWrite {
        source: plan.source,
        field: plan.field,
        after_current,
        changed,
        postflight,
    }
}
pub(crate) unsafe fn store_vital(address: usize, value: i32) {
    // SAFETY: Caller provides exact initialized live aligned writable four-byte storage, no aliases.
    unsafe { (address as *mut i32).write(value) };
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy three current/maximum pairs from the independently identified data module.
    /// Missing character/world/module returns None; a stale identity or detectable change fails.
    /// Rechecks do not pin objects, prevent ABA, or provide an atomic engine snapshot.
    pub fn character_vitals(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterVitalsObservation>> {
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale character vitals identity"));
        }
        let Some(data) = self.character_data(target.address)? else {
            return Ok(None);
        };
        let read = || -> Result<[[u8; 8]; 3]> {
            let mut pairs = [[0; 8]; 3];
            for (offset, pair) in [HP_OFFSET, MP_OFFSET, STAMINA_OFFSET]
                .into_iter()
                .zip(&mut pairs)
            {
                let address = data
                    .address
                    .checked_add(u64::from(offset))
                    .ok_or(Error::AddressOverflow)?;
                self.module.memory().read_exact(address, pair)?;
            }
            Ok(pairs)
        };
        let pairs = read()?;
        if read()? != pairs
            || self.character_data(target.address)? != Some(data)
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile(
                "character vitals changed during observation",
            ));
        }
        let decode = |pair: [u8; 8]| CurrentMaximum {
            current: i32::from_le_bytes(pair[..4].try_into().expect("four bytes")),
            maximum: i32::from_le_bytes(pair[4..].try_into().expect("four bytes")),
        };
        Ok(Some(CharacterVitalsObservation {
            character: target,
            data,
            hp: decode(pairs[0]),
            mp: decode(pairs[1]),
            stamina: decode(pairs[2]),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(address: u64) -> CharacterVitalsObservation {
        CharacterVitalsObservation {
            character: CharacterIdentity {
                address: 0x1000,
                world_address: 0x2000,
                native_identifier: 0x123456789abcdef0,
                epoch: 3,
            },
            data: CharacterDataObservation {
                address,
                owner_address: 0x1000,
                container_address: 0x3000,
            },
            hp: CurrentMaximum {
                current: 0,
                maximum: 900,
            },
            mp: CurrentMaximum {
                current: 55,
                maximum: 99,
            },
            stamina: CurrentMaximum {
                current: -50,
                maximum: 120,
            },
        }
    }
    #[repr(C, align(8))]
    struct Storage([u32; 100]);
    #[test]
    fn owned_vital_stores_touch_exact_current_word_and_restore_preserves_engine_neighbors() {
        let mut storage = Storage([0xa5a5a5a5; 100]);
        for (field, offset) in [
            (CharacterVital::Hp, 0x138),
            (CharacterVital::Mp, 0x148),
            (CharacterVital::Stamina, 0x154),
        ] {
            storage.0.fill(0xa5a5a5a5);
            let before = source(storage.0.as_mut_ptr() as u64);
            for (f, at) in [
                (CharacterVital::Hp, 0x138),
                (CharacterVital::Mp, 0x148),
                (CharacterVital::Stamina, 0x154),
            ] {
                storage.0[at / 4] = f.pair(before).current as u32;
                storage.0[(at + 4) / 4] = f.pair(before).maximum as u32;
            }
            let initial = storage.0;
            let plan = vital_plan(Some(before), before, field, i32::MAX).unwrap();
            let after = plan.after;
            let receipt = commit_vital(
                plan,
                |address, value| {
                    // SAFETY: Original exclusive aligned owned array, all bytes remain initialized.
                    unsafe { store_vital(address, value) };
                },
                || Ok(after),
            );
            assert!(receipt.changed);
            assert_eq!(*receipt.postflight.as_ref().unwrap(), after);
            let mut expected = initial;
            expected[offset / 4] = i32::MAX as u32;
            assert_eq!(storage.0, expected);
            // Engine changes maximum and another current field; restoring the owned word
            // must preserve those newer values instead of copying the original whole struct.
            let mut current = after;
            current.hp.maximum = 777;
            current.mp.maximum = 888;
            current.stamina.maximum = 999;
            let other = if field == CharacterVital::Mp {
                CharacterVital::Stamina
            } else {
                CharacterVital::Mp
            };
            other.replace(&mut current, -123);
            for (f, at) in [
                (CharacterVital::Hp, 0x138),
                (CharacterVital::Mp, 0x148),
                (CharacterVital::Stamina, 0x154),
            ] {
                storage.0[at / 4] = f.pair(current).current as u32;
                storage.0[(at + 4) / 4] = f.pair(current).maximum as u32;
                expected[at / 4] = f.pair(current).current as u32;
                expected[(at + 4) / 4] = f.pair(current).maximum as u32;
            }
            let plan = vital_restore_plan(Some(current), &receipt).unwrap();
            let restored = plan.after;
            let restored = commit_vital(
                plan,
                |address, value| {
                    // SAFETY: Same original exclusive current-word array, never game memory.
                    unsafe { store_vital(address, value) };
                },
                || Ok(restored),
            );
            assert!(restored.postflight.is_ok());
            expected[offset / 4] = field.pair(before).current as u32;
            assert_eq!(storage.0, expected);
        }
    }
    #[test]
    fn vital_plans_reject_full_source_changes_and_keep_failed_write_obligations() {
        let before = source(0x4000);
        let field = CharacterVital::Hp;
        assert!(vital_plan(None, before, field, 900).is_err());
        for mode in 0..13 {
            let mut other = before;
            match mode {
                0 => other.character.address += 8,
                1 => other.character.world_address += 8,
                2 => other.character.native_identifier += 1,
                3 => other.character.epoch += 1,
                4 => other.data.address += 8,
                5 => other.data.owner_address += 8,
                6 => other.data.container_address += 8,
                7 => other.hp.current += 1,
                8 => other.hp.maximum += 1,
                9 => other.mp.current += 1,
                10 => other.mp.maximum += 1,
                11 => other.stamina.current += 1,
                _ => other.stamina.maximum += 1,
            }
            assert!(vital_plan(Some(other), before, field, 900).is_err());
        }
        for address in [0, 1, u64::MAX - 7] {
            let bad = source(address);
            assert!(vital_plan(Some(bad), bad, field, 900).is_err());
        }
        let plan = vital_plan(Some(before), before, field, 900).unwrap();
        let after = plan.after;
        let count = std::cell::Cell::new(0);
        let call = commit_vital(
            plan,
            |_, _| count.set(count.get() + 1),
            || Err(Error::InvalidProfile("copy lost")),
        );
        assert!(call.changed && call.postflight.is_err());
        assert_eq!(count.get(), 1);
        assert!(vital_restore_plan(Some(after), &call).is_ok());
        let mut superseded = after;
        superseded.hp.current = 899;
        assert!(vital_restore_plan(Some(superseded), &call).is_err());
        superseded = after;
        superseded.data.container_address += 8;
        assert!(vital_restore_plan(Some(superseded), &call).is_err());
        assert!(vital_restore_plan(None, &call).is_err());
        let noop = vital_plan(Some(before), before, field, 0).unwrap();
        assert!(!commit_vital(noop, |_, _| unreachable!(), || unreachable!()).changed);
    }
}
