//! Raw controller policy and its nonpolymorphic modifier. Bit meanings remain experimental.
use crate::{
    character_controller::CharacterControllerObservation,
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
pub(crate) const POLICY_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3c4d60,
        length: 0x547,
    },
    CodeRange {
        rva: 0x3c38c0,
        length: 0x2c,
    },
    CodeRange {
        rva: 0x3c3b20,
        length: 0x38,
    },
    CodeRange {
        rva: 0x3c3b70,
        length: 0x48,
    },
    CodeRange {
        rva: 0x3c8db0,
        length: 0x22c,
    },
    CodeRange {
        rva: 0x3c8fe0,
        length: 0x1b2,
    },
    CodeRange {
        rva: 0x578d70,
        length: 0xb8,
    },
    CodeRange {
        rva: 0x578e50,
        length: 0xb2,
    },
    CodeRange {
        rva: 0x578f10,
        length: 0x77,
    },
    CodeRange {
        rva: 0x5e1c10,
        length: 0x56,
    },
    CodeRange {
        rva: 0x5e1c70,
        length: 0x56,
    },
    CodeRange {
        rva: 0x5f09a0,
        length: 0x36,
    },
    CodeRange {
        rva: 0x5f09d0,
        length: 0xc,
    },
    CodeRange {
        rva: 0x3f4740,
        length: 0x12,
    },
    CodeRange {
        rva: 0x5dac80,
        length: 0x135,
    },
    CodeRange {
        rva: 0x5dd4e0,
        length: 0x135,
    },
    CodeRange {
        rva: 0x3f59c0,
        length: 0x2dc,
    },
    CodeRange {
        rva: 0x3f3bf0,
        length: 0x6a,
    },
    CodeRange {
        rva: 0x3f2ec0,
        length: 0x15,
    },
    CodeRange {
        rva: 0x49d3b0,
        length: 0x9ac,
    },
    CodeRange {
        rva: 0x7170b0,
        length: 0x1264,
    },
];
const CONTROLLER_LOW_OFFSET: u32 = 0xe8;
const MODIFIER_LINK_OFFSET: u32 = 0xc8;
const MODIFIER_DATA_OFFSET: u32 = 8;
const MODIFIER_ACTION_LOW_OFFSET: u32 = 0x10;
const MODIFIER_WORD_OFFSET: u32 = 0x14;
const MODIFIER_STATE_WORD_OFFSET: u32 = 0x1c;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerPolicyField {
    ControllerLow,
    ModifierWord10,
    ModifierWord14,
    ModifierWord1c,
}
impl ControllerPolicyField {
    pub(crate) fn width(self) -> usize {
        match self {
            Self::ControllerLow => 1,
            Self::ModifierWord10 | Self::ModifierWord14 | Self::ModifierWord1c => 4,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerPolicyObservation {
    pub controller: CharacterControllerObservation,
    /// Zero is a missing modifier; its first word is an owner pointer, never a vtable.
    pub modifier_address: u64,
    pub low_byte: u8,
    pub modifier_word10: Option<u32>,
    pub modifier_word14: Option<u32>,
    pub modifier_word1c: Option<u32>,
}
impl ControllerPolicyObservation {
    /// Request consumed by native 3f3bf0/49d3b0 to exclude lock target records.
    /// Other resource/event conditions also exclude records; this is not a target lease.
    pub fn lock_target_suppression_requested(self) -> Option<bool> {
        self.modifier_word10.map(|word| word & 4 != 0)
    }
    /// Raw mark used by native EcTestChrDead, not HP, animation, corpse or revival completion.
    pub fn event_dead_marked(self) -> Option<bool> {
        self.modifier_word1c.map(|word| word & 1 != 0)
    }
    pub fn value(self, field: ControllerPolicyField) -> Option<u32> {
        match field {
            ControllerPolicyField::ControllerLow => Some(u32::from(self.low_byte)),
            ControllerPolicyField::ModifierWord10 => self.modifier_word10,
            ControllerPolicyField::ModifierWord14 => self.modifier_word14,
            ControllerPolicyField::ModifierWord1c => self.modifier_word1c,
        }
    }
    fn same_storage(self, other: Self) -> bool {
        self.controller.character == other.controller.character
            && self.controller.address == other.controller.address
            && self.controller.kind == other.controller.kind
            && self.modifier_address == other.modifier_address
            && self.modifier_word10.is_some() == other.modifier_word10.is_some()
            && self.modifier_word14.is_some() == other.modifier_word14.is_some()
            && self.modifier_word1c.is_some() == other.modifier_word1c.is_some()
    }
}
#[derive(Debug)]
pub struct ControllerPolicyWrite {
    pub source: ControllerPolicyObservation,
    pub field: ControllerPolicyField,
    pub mask: u32,
    pub after_value: u32,
    pub changed: bool,
    pub postflight: Result<ControllerPolicyObservation>,
}
fn add(base: u64, offset: u32) -> Result<u64> {
    base.checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Rechecks full typed controller, reciprocal modifier owner and exact used payload.
    /// Missing controller is None; missing modifier preserves the available controller byte.
    /// No modifier RTTI, unrelated data, pin, native getter or engine lock is used.
    pub fn controller_policy(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<ControllerPolicyObservation>> {
        let Some(controller) = self.character_controller(target)? else {
            return Ok(None);
        };
        let memory = self.module.memory();
        let link = add(controller.address, MODIFIER_LINK_OFFSET)?;
        let modifier_address = memory.read_u64(link)?;
        if !modifier_address.is_multiple_of(8) {
            return Err(Error::InvalidProfile("unaligned controller modifier"));
        }
        let owner = || -> Result<()> {
            if modifier_address != 0 && memory.read_u64(modifier_address)? != controller.address {
                return Err(Error::InvalidProfile(
                    "controller modifier belongs to another owner",
                ));
            }
            Ok(())
        };
        owner()?;
        let read = || -> Result<(u8, [Option<u32>; 3])> {
            let low = memory.read_u8(add(controller.address, CONTROLLER_LOW_OFFSET)?)?;
            let action = if modifier_address == 0 {
                None
            } else {
                Some(memory.read_u32(add(
                    add(modifier_address, MODIFIER_DATA_OFFSET)?,
                    MODIFIER_ACTION_LOW_OFFSET,
                )?)?)
            };
            let word = if modifier_address == 0 {
                None
            } else {
                Some(memory.read_u32(add(
                    add(modifier_address, MODIFIER_DATA_OFFSET)?,
                    MODIFIER_WORD_OFFSET,
                )?)?)
            };
            let state = if modifier_address == 0 {
                None
            } else {
                Some(memory.read_u32(add(
                    add(modifier_address, MODIFIER_DATA_OFFSET)?,
                    MODIFIER_STATE_WORD_OFFSET,
                )?)?)
            };
            Ok((low, [action, word, state]))
        };
        let (low_byte, words) = read()?;
        let [modifier_word10, modifier_word14, modifier_word1c] = words;
        if read()? != (low_byte, words)
            || memory.read_u64(link)? != modifier_address
            || self.character_controller(target)? != Some(controller)
        {
            return Err(Error::InvalidProfile(
                "controller policy changed during observation",
            ));
        }
        owner()?;
        Ok(Some(ControllerPolicyObservation {
            controller,
            modifier_address,
            low_byte,
            modifier_word10,
            modifier_word14,
            modifier_word1c,
        }))
    }
}
pub(crate) struct PolicyPlan {
    source: ControllerPolicyObservation,
    after: ControllerPolicyObservation,
    field: ControllerPolicyField,
    mask: u32,
    address: usize,
}
pub(crate) fn policy_plan(
    current: Option<ControllerPolicyObservation>,
    expected: ControllerPolicyObservation,
    field: ControllerPolicyField,
    mask: u32,
    bits: u32,
) -> Result<PolicyPlan> {
    if mask == 0 || bits & !mask != 0 || (field.width() == 1 && mask > 255) {
        return Err(Error::InvalidProfile(
            "invalid controller policy mask/value",
        ));
    }
    let c = expected.controller;
    if current != Some(expected)
        || c.character.epoch == 0
        || c.address == 0
        || !c.address.is_multiple_of(8)
        || !matches!(
            c.kind,
            ObjectKind::CharacterController
                | ObjectKind::EnemyController
                | ObjectKind::PlayerController
        )
    {
        return Err(Error::NativeOperationRejected(
            "controller policy source superseded",
        ));
    }
    let before = expected.value(field).ok_or(Error::NativeOperationRejected(
        "controller modifier unavailable",
    ))?;
    let address = match field {
        ControllerPolicyField::ControllerLow => add(c.address, CONTROLLER_LOW_OFFSET)?,
        ControllerPolicyField::ModifierWord10
        | ControllerPolicyField::ModifierWord14
        | ControllerPolicyField::ModifierWord1c => {
            if expected.modifier_address == 0 || !expected.modifier_address.is_multiple_of(8) {
                return Err(Error::NativeOperationRejected(
                    "invalid controller modifier storage",
                ));
            }
            add(
                add(expected.modifier_address, MODIFIER_DATA_OFFSET)?,
                match field {
                    ControllerPolicyField::ModifierWord10 => MODIFIER_ACTION_LOW_OFFSET,
                    ControllerPolicyField::ModifierWord14 => MODIFIER_WORD_OFFSET,
                    _ => MODIFIER_STATE_WORD_OFFSET,
                },
            )?
        }
    };
    let value = (before & !mask) | bits;
    let mut after = expected;
    match field {
        ControllerPolicyField::ControllerLow => after.low_byte = value as u8,
        ControllerPolicyField::ModifierWord10 => after.modifier_word10 = Some(value),
        ControllerPolicyField::ModifierWord14 => after.modifier_word14 = Some(value),
        ControllerPolicyField::ModifierWord1c => after.modifier_word1c = Some(value),
    }
    Ok(PolicyPlan {
        source: expected,
        after,
        field,
        mask,
        address: usize::try_from(address).map_err(|_| Error::AddressOverflow)?,
    })
}
pub(crate) fn policy_restore_plan(
    current: Option<ControllerPolicyObservation>,
    applied: &ControllerPolicyWrite,
) -> Result<PolicyPlan> {
    let current = current.ok_or(Error::NativeOperationRejected(
        "controller policy unavailable",
    ))?;
    let value = current
        .value(applied.field)
        .ok_or(Error::NativeOperationRejected(
            "controller modifier unavailable",
        ))?;
    if !current.same_storage(applied.source)
        || value & applied.mask != applied.after_value & applied.mask
    {
        return Err(Error::NativeOperationRejected(
            "controller policy owned bits superseded",
        ));
    }
    policy_plan(
        Some(current),
        current,
        applied.field,
        applied.mask,
        applied
            .source
            .value(applied.field)
            .ok_or(Error::InvalidProfile("invalid policy receipt"))?
            & applied.mask,
    )
}
pub(crate) fn commit_policy(
    plan: PolicyPlan,
    write: impl FnOnce(usize, usize, u32),
    observe: impl FnOnce() -> Result<ControllerPolicyObservation>,
) -> ControllerPolicyWrite {
    let after_value = plan.after.value(plan.field).expect("checked field");
    let changed = plan.source.value(plan.field) != Some(after_value);
    let postflight = if changed {
        write(plan.address, plan.field.width(), after_value);
        observe().and_then(|observed| {
            if observed == plan.after {
                Ok(observed)
            } else {
                Err(Error::InvalidProfile(
                    "controller policy postflight changed",
                ))
            }
        })
    } else {
        Ok(plan.source)
    };
    ControllerPolicyWrite {
        source: plan.source,
        field: plan.field,
        mask: plan.mask,
        after_value,
        changed,
        postflight,
    }
}
pub(crate) unsafe fn store_policy(address: usize, width: usize, value: u32) {
    // SAFETY: Private checked plan chooses exact byte or aligned u32, caller independently pins
    // its live initialized writable extent and excludes native/other aliases in this phase.
    unsafe {
        match width {
            1 => (address as *mut u8).write(value as u8),
            4 => (address as *mut u32).write(value),
            _ => unreachable!("private policy width"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(address: u64) -> ControllerPolicyObservation {
        ControllerPolicyObservation {
            controller: CharacterControllerObservation {
                character: CharacterIdentity {
                    address: 0x1000,
                    world_address: 0x2000,
                    native_identifier: 0xfedcba9876543210,
                    epoch: 3,
                },
                address,
                kind: ObjectKind::EnemyController,
                axis_scale_bits: [0x3f800000; 3],
            },
            modifier_address: address + 0x400,
            low_byte: 0x60,
            modifier_word10: Some(0x98765430),
            modifier_word14: Some(0x80000400),
            modifier_word1c: Some(0x12345670),
        }
    }
    #[test]
    fn exact_policy_spans_and_mask_restore_preserve_foreign_fields_and_bits() {
        #[repr(align(16))]
        struct Storage([u8; 0x480]);
        let mut storage = Storage([0xa5; 0x480]);
        let before = source(storage.0.as_mut_ptr() as u64);
        for (field, offset, width, mask, bits) in [
            (ControllerPolicyField::ControllerLow, 0xe8, 1, 2, 2),
            (ControllerPolicyField::ModifierWord10, 0x418, 4, 4, 4),
            (ControllerPolicyField::ModifierWord14, 0x41c, 4, 4, 4),
            (ControllerPolicyField::ModifierWord1c, 0x424, 4, 1, 1),
        ] {
            storage.0.fill(0xa5);
            storage.0[0xe8] = before.low_byte;
            storage.0[0x418..0x41c].copy_from_slice(&before.modifier_word10.unwrap().to_le_bytes());
            storage.0[0x41c..0x420].copy_from_slice(&before.modifier_word14.unwrap().to_le_bytes());
            storage.0[0x424..0x428].copy_from_slice(&before.modifier_word1c.unwrap().to_le_bytes());
            let mut expected = storage.0;
            let plan = policy_plan(Some(before), before, field, mask, bits).unwrap();
            let after = plan.after;
            let applied = commit_policy(
                plan,
                |address, size, value| {
                    assert_eq!(size, width);
                    // SAFETY: Original exclusively owned array with exact aligned field extent.
                    unsafe { store_policy(address, size, value) };
                },
                || Ok(after),
            );
            expected[offset..offset + width]
                .copy_from_slice(&applied.after_value.to_le_bytes()[..width]);
            assert!(applied.changed && applied.postflight.is_ok());
            assert_eq!(storage.0, expected);
            let mut current = after;
            current.controller.axis_scale_bits[1] ^= 1;
            let current_value = applied.after_value ^ 0x80;
            match field {
                ControllerPolicyField::ControllerLow => {
                    current.low_byte = current_value as u8;
                    current.modifier_word14 = Some(0xabcdef01);
                }
                ControllerPolicyField::ModifierWord14 => {
                    current.modifier_word14 = Some(current_value);
                    current.low_byte = 0xff;
                }
                ControllerPolicyField::ModifierWord10 => {
                    current.modifier_word10 = Some(current_value);
                    current.modifier_word14 = Some(0x87654321);
                    current.low_byte = 0xff;
                }
                ControllerPolicyField::ModifierWord1c => {
                    current.modifier_word1c = Some(current_value);
                    current.modifier_word14 = Some(0x87654321);
                    current.low_byte = 0xfe;
                }
            }
            storage.0[offset..offset + width]
                .copy_from_slice(&current_value.to_le_bytes()[..width]);
            let plan = policy_restore_plan(Some(current), &applied).unwrap();
            let restored = plan.after;
            let receipt = commit_policy(
                plan,
                |address, size, value| {
                    // SAFETY: Same original live exclusive initialized writable storage.
                    unsafe { store_policy(address, size, value) };
                },
                || Ok(restored),
            );
            expected[offset..offset + width]
                .copy_from_slice(&receipt.after_value.to_le_bytes()[..width]);
            assert_eq!(storage.0, expected);
            assert_eq!(receipt.after_value, before.value(field).unwrap() ^ 0x80);
            assert_eq!(
                restored.controller.axis_scale_bits,
                current.controller.axis_scale_bits
            );
        }
    }
    #[test]
    fn policy_conflicts_missing_modifier_widths_failure_and_noop_are_explicit() {
        let before = source(0x4000);
        let field = ControllerPolicyField::ModifierWord14;
        for (f, mask, bits) in [
            (field, 0, 0),
            (field, 4, 8),
            (ControllerPolicyField::ControllerLow, 256, 256),
        ] {
            assert!(policy_plan(Some(before), before, f, mask, bits).is_err());
        }
        assert!(policy_plan(None, before, field, 4, 4).is_err());
        for mode in 0..12 {
            let mut foreign = before;
            match mode {
                0 => foreign.controller.character.address += 8,
                1 => foreign.controller.character.world_address += 8,
                2 => foreign.controller.character.native_identifier ^= 1,
                3 => foreign.controller.character.epoch += 1,
                4 => foreign.controller.address += 8,
                5 => foreign.controller.kind = ObjectKind::PlayerController,
                6 => foreign.modifier_address += 8,
                7 => foreign.controller.axis_scale_bits[0] ^= 1,
                8 => foreign.low_byte ^= 1,
                9 => foreign.modifier_word14 = Some(0),
                10 => foreign.modifier_word1c = Some(0),
                _ => foreign.modifier_word10 = Some(0),
            }
            assert!(policy_plan(Some(foreign), before, field, 4, 4).is_err());
        }
        let mut missing = before;
        missing.modifier_address = 0;
        missing.modifier_word10 = None;
        missing.modifier_word14 = None;
        missing.modifier_word1c = None;
        assert!(policy_plan(Some(missing), missing, field, 4, 4).is_err());
        assert!(
            policy_plan(
                Some(missing),
                missing,
                ControllerPolicyField::ControllerLow,
                2,
                2
            )
            .is_ok()
        );
        let plan = policy_plan(Some(before), before, field, 4, 4).unwrap();
        let after = plan.after;
        let calls = std::cell::Cell::new(0);
        let applied = commit_policy(
            plan,
            |_, _, _| calls.set(calls.get() + 1),
            || Err(Error::InvalidProfile("copy lost")),
        );
        assert!(applied.changed && applied.postflight.is_err());
        assert_eq!(calls.get(), 1);
        assert!(policy_restore_plan(Some(after), &applied).is_ok());
        let mut foreign = after;
        foreign.modifier_word14 = Some(applied.after_value ^ 4);
        assert!(policy_restore_plan(Some(foreign), &applied).is_err());
        foreign = after;
        foreign.modifier_address += 8;
        assert!(policy_restore_plan(Some(foreign), &applied).is_err());
        assert!(policy_restore_plan(None, &applied).is_err());
        let noop = policy_plan(Some(before), before, field, 4, 0).unwrap();
        assert!(!commit_policy(noop, |_, _, _| unreachable!(), || unreachable!()).changed);
    }
}
