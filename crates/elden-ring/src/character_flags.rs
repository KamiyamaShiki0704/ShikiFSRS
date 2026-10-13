//! Proved raw status/debug bytes and stored team. No visible/alive/loaded predicate is inferred from one bit.
use crate::{
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind, ObjectObservation},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
pub(crate) const FLAG_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3e6e30,
        length: 0x99f,
    },
    CodeRange {
        rva: 0x3f3930,
        length: 0x76,
    },
    CodeRange {
        rva: 0x3f3a60,
        length: 0x4a,
    },
    CodeRange {
        rva: 0x2d4a70,
        length: 0x7,
    },
    CodeRange {
        rva: 0x4d0770,
        length: 0x8,
    },
    CodeRange {
        rva: 0xd42c40,
        length: 0x76,
    },
    CodeRange {
        rva: 0x3f77b0,
        length: 0x151,
    },
    CodeRange {
        rva: 0x3ee7b0,
        length: 0x7a,
    },
    CodeRange {
        rva: 0x5f5b90,
        length: 0xae,
    },
    CodeRange {
        rva: 0x5ee3e0,
        length: 0x62,
    },
    CodeRange {
        rva: 0x3fdd50,
        length: 0x248,
    },
    CodeRange {
        rva: 0x578b00,
        length: 0xb8,
    },
    CodeRange {
        rva: 0x578be0,
        length: 0xb2,
    },
    CodeRange {
        rva: 0x578ca0,
        length: 0x77,
    },
    CodeRange {
        rva: 0x5e2170,
        length: 0x56,
    },
    CodeRange {
        rva: 0x5e2240,
        length: 0x56,
    },
    CodeRange {
        rva: 0x5f1bd0,
        length: 0x31,
    },
    CodeRange {
        rva: 0x5de430,
        length: 0x135,
    },
    CodeRange {
        rva: 0x5dc480,
        length: 0x135,
    },
    CodeRange {
        rva: 0x5f1c00,
        length: 0xc,
    },
    CodeRange {
        rva: 0x3f2ae0,
        length: 0xd,
    },
    CodeRange {
        rva: 0x3e8240,
        length: 0x629,
    },
    CodeRange {
        rva: 0x3ed710,
        length: 0x2fa,
    },
    // Character presentation request -> exact CSChrModelIns virtual18 -> model enable bit.
    CodeRange {
        rva: 0x3ff550,
        length: 0x33,
    },
    CodeRange {
        rva: 0x9f26d0,
        length: 0x1b,
    },
    CodeRange {
        rva: 0x44ae70,
        length: 0x65b,
    },
    CodeRange {
        rva: 0x6defb0,
        length: 0x18b,
    },
    CodeRange {
        rva: 0x3ee730,
        length: 0x75,
    },
    CodeRange {
        rva: 0x3f9140,
        length: 0xf6,
    },
    CodeRange {
        rva: 0x3f84c0,
        length: 0x108,
    },
    CodeRange {
        rva: 0x3fbe70,
        length: 0x623,
    },
];
const TEAM_OFFSET: u32 = 0x6c;
const STATUS0_OFFSET: u32 = 0x1c4;
const GROUP_RESULT_MASK: u32 = 8;
const FRUSTUM_RESULT_MASK: u32 = 0x10;
const STATUS1_OFFSET: u32 = 0x1c5;
const STATUS2_OFFSET: u32 = 0x1c8;
const DEBUG_LOW_OFFSET: u32 = 0x538;
const DEBUG_HIGH_OFFSET: u32 = 0x539;
const GROUP_ID_OFFSET: u32 = 0xb4;
const GROUP_SWITCH_GLOBAL: u32 = 0x3b1bffe;
const GROUP_DEBUG_WORLD_GLOBAL: u32 = 0x3d6a208;

/// Native update-frequency group assignment and its debug gate, not rendered visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterUpdateGroupObservation {
    pub flags: CharacterFlagsObservation,
    pub group_id: u32,
    pub debug_world: ObjectObservation,
    pub debug_switch: u8,
}
pub(crate) fn update_group_preflight(
    current: Option<CharacterUpdateGroupObservation>,
    expected: CharacterUpdateGroupObservation,
) -> Result<()> {
    if current != Some(expected)
        || expected.flags.character.epoch == 0
        || expected.flags.character.address == 0
        || !expected.flags.character.address.is_multiple_of(8)
        || expected.debug_world.address == 0
        || !expected.debug_world.address.is_multiple_of(8)
        || expected.debug_world.kind != ObjectKind::DebugWorld
    {
        return Err(Error::NativeOperationRejected(
            "update group source superseded",
        ));
    }
    Ok(())
}
pub(crate) fn update_group_postflight(
    source: CharacterUpdateGroupObservation,
    enabled: bool,
    group_id: u32,
    after: CharacterUpdateGroupObservation,
) -> Result<CharacterUpdateGroupObservation> {
    let mut expected = source;
    if source.debug_switch != 0 {
        expected.flags.bytes[0] = (expected.flags.bytes[0] & !1) | u8::from(enabled);
        expected.group_id = group_id;
    }
    if after != expected {
        return Err(Error::InvalidProfile(
            "update group native postflight differs",
        ));
    }
    Ok(after)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterFlagByte {
    Status0,
    Status1,
    Status2,
    DebugLow,
    /// Stored native team byte, not the complete effective hostility relation.
    StoredTeam,
    /// Includes the independently named disable-update request. Other bits remain opaque.
    DebugHigh,
}
impl CharacterFlagByte {
    pub(crate) fn index(self) -> usize {
        match self {
            Self::Status0 => 0,
            Self::Status1 => 1,
            Self::Status2 => 2,
            Self::DebugLow => 3,
            Self::StoredTeam => 4,
            Self::DebugHigh => 5,
        }
    }
    pub(crate) fn offset(self) -> u64 {
        u64::from(
            [
                STATUS0_OFFSET,
                STATUS1_OFFSET,
                STATUS2_OFFSET,
                DEBUG_LOW_OFFSET,
                TEAM_OFFSET,
                DEBUG_HIGH_OFFSET,
            ][self.index()],
        )
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterFlagsObservation {
    pub character: CharacterIdentity,
    pub bytes: [u8; 6],
}
impl CharacterFlagsObservation {
    /// Result stored from the native group-mask query, not a completed render or visibility lease.
    pub fn group_mask_result(self) -> bool {
        u32::from(self.value(CharacterFlagByte::Status0)) & GROUP_RESULT_MASK != 0
    }
    /// Stored native frustum query result. This does not establish visible pixels or readiness.
    pub fn frustum_result(self) -> bool {
        u32::from(self.value(CharacterFlagByte::Status0)) & FRUSTUM_RESULT_MASK != 0
    }
    /// Mark set by native 3ee730/3f9140, consumed and sometimes cleared by 3f84c0.
    /// This is distinct from input request bit1, HP, EcTestChrDead and completed death.
    pub fn native_death_marked(self) -> bool {
        self.value(CharacterFlagByte::Status1) & 0x80 != 0
    }
    /// The character debug request bypasses the two observed native hit dispatch paths.
    /// It is not physics collision disablement or proof of universal invulnerability.
    pub fn hit_dispatch_suppression_requested(self) -> bool {
        self.value(CharacterFlagByte::DebugLow) & 8 != 0
    }
    pub fn value(self, field: CharacterFlagByte) -> u8 {
        self.bytes[field.index()]
    }
    /// Request consumed by native 3f3930/3ff550 before CSChrModelIns virtual18.
    /// Other backread/controller/group conditions may still suppress the model.
    /// This is not rendered visibility, activation, death or resource readiness.
    pub fn model_enable_requested(self) -> bool {
        self.value(CharacterFlagByte::Status1) & 8 != 0
    }
    /// RTTI-linked SetDisableAllChrUpdate's callback writes this exact request bit.
    /// This is not an update-completion, render/physics suppression or task-drain predicate.
    pub fn debug_update_disabled_requested(self) -> bool {
        self.value(CharacterFlagByte::DebugHigh) & 1 != 0
    }
    /// Native diagnostic reason is "update-frequency setting". Not a render-group predicate.
    pub fn update_frequency_override_requested(self) -> bool {
        self.value(CharacterFlagByte::Status0) & 1 != 0
    }
    /// Native diagnostic reason is "forced update"; no completed-update or activation proof.
    pub fn forced_update_requested(self) -> bool {
        self.value(CharacterFlagByte::Status0) & 2 != 0
    }
    /// Exact EcTestChrBackReadReady predicate. Cleared before teardown finishes;
    /// set before all initializer callbacks return. No resource, ownership or drain guard.
    pub fn native_backread_ready(self) -> bool {
        self.value(CharacterFlagByte::Status2) & 0x10 != 0
    }
}
#[derive(Debug)]
pub struct CharacterFlagWrite {
    pub source: CharacterFlagsObservation,
    pub field: CharacterFlagByte,
    pub mask: u8,
    pub after_value: u8,
    pub changed: bool,
    /// The store already occurred when this fails. Never discard the source/mask obligation.
    pub postflight: Result<CharacterFlagsObservation>,
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// The complete native 3ee7b0 entry requires the debug singleton even when the switch is
    /// zero. Copies are rechecked, but acquire no pin/lock and do not evaluate native render predicates.
    pub fn character_update_group(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterUpdateGroupObservation>> {
        let Some(flags) = self.character_flags(target)? else {
            return Ok(None);
        };
        let Some(debug_world) = self.singleton(GROUP_DEBUG_WORLD_GLOBAL, ObjectKind::DebugWorld)?
        else {
            return Ok(None);
        };
        let group_address = target
            .address
            .checked_add(u64::from(GROUP_ID_OFFSET))
            .ok_or(Error::AddressOverflow)?;
        let switch_address = self
            .module
            .base()
            .checked_add(u64::from(GROUP_SWITCH_GLOBAL))
            .ok_or(Error::AddressOverflow)?;
        let group_id = self.module.memory().read_u32(group_address)?;
        let debug_switch = self.module.memory().read_u8(switch_address)?;
        if self.module.memory().read_u32(group_address)? != group_id
            || self.module.memory().read_u8(switch_address)? != debug_switch
            || self.singleton(GROUP_DEBUG_WORLD_GLOBAL, ObjectKind::DebugWorld)?
                != Some(debug_world)
            || self.character_flags(target)? != Some(flags)
        {
            return Err(Error::InvalidProfile(
                "update group graph changed during observation",
            ));
        }
        Ok(Some(CharacterUpdateGroupObservation {
            flags,
            group_id,
            debug_world,
            debug_switch,
        }))
    }
    /// Copies six independently initialized bytes, with full identity and payload rechecks.
    /// Missing character differs from a valid all-zero copy. No engine lock or pin is acquired.
    pub fn character_flags(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterFlagsObservation>> {
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale character flags identity"));
        }
        let read = || -> Result<[u8; 6]> {
            let mut bytes = [0; 6];
            for field in [
                CharacterFlagByte::Status0,
                CharacterFlagByte::Status1,
                CharacterFlagByte::Status2,
                CharacterFlagByte::DebugLow,
                CharacterFlagByte::StoredTeam,
                CharacterFlagByte::DebugHigh,
            ] {
                bytes[field.index()] = self.module.memory().read_u8(
                    target
                        .address
                        .checked_add(field.offset())
                        .ok_or(Error::AddressOverflow)?,
                )?;
            }
            Ok(bytes)
        };
        let bytes = read()?;
        if read()? != bytes
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile(
                "character flag storage changed during observation",
            ));
        }
        Ok(Some(CharacterFlagsObservation {
            character: target,
            bytes,
        }))
    }
}
pub(crate) struct FlagPlan {
    source: CharacterFlagsObservation,
    after: CharacterFlagsObservation,
    field: CharacterFlagByte,
    mask: u8,
    address: usize,
}
pub(crate) fn flag_plan(
    current: Option<CharacterFlagsObservation>,
    expected: CharacterFlagsObservation,
    field: CharacterFlagByte,
    mask: u8,
    bits: u8,
) -> Result<FlagPlan> {
    if mask == 0 || bits & !mask != 0 {
        return Err(Error::InvalidProfile("invalid character flag mask/value"));
    }
    if expected.character.epoch == 0
        || expected.character.address == 0
        || !expected.character.address.is_multiple_of(8)
        || current != Some(expected)
    {
        return Err(Error::NativeOperationRejected(
            "character flag source superseded",
        ));
    }
    let address = expected
        .character
        .address
        .checked_add(field.offset())
        .ok_or(Error::AddressOverflow)?;
    let mut after = expected;
    after.bytes[field.index()] = (after.bytes[field.index()] & !mask) | bits;
    Ok(FlagPlan {
        source: expected,
        after,
        field,
        mask,
        address: usize::try_from(address).map_err(|_| Error::AddressOverflow)?,
    })
}
pub(crate) fn flag_restore_plan(
    current: Option<CharacterFlagsObservation>,
    applied: &CharacterFlagWrite,
) -> Result<FlagPlan> {
    let current = current.ok_or(Error::NativeOperationRejected(
        "character flag restore unavailable",
    ))?;
    if current.character != applied.source.character
        || current.value(applied.field) & applied.mask != applied.after_value & applied.mask
    {
        return Err(Error::NativeOperationRejected(
            "character flag owned bits superseded",
        ));
    }
    flag_plan(
        Some(current),
        current,
        applied.field,
        applied.mask,
        applied.source.value(applied.field) & applied.mask,
    )
}
pub(crate) fn commit_flag(
    plan: FlagPlan,
    write: impl FnOnce(usize, u8),
    observe: impl FnOnce() -> Result<CharacterFlagsObservation>,
) -> CharacterFlagWrite {
    let after_value = plan.after.value(plan.field);
    let changed = plan.source.value(plan.field) != after_value;
    let postflight = if changed {
        write(plan.address, after_value);
        observe().and_then(|value| {
            if value != plan.after {
                Err(Error::InvalidProfile("character flag postflight changed"))
            } else {
                Ok(value)
            }
        })
    } else {
        Ok(plan.source)
    };
    CharacterFlagWrite {
        source: plan.source,
        field: plan.field,
        mask: plan.mask,
        after_value,
        changed,
        postflight,
    }
}
pub(crate) unsafe fn store_flag(address: usize, value: u8) {
    // SAFETY: Caller owns exclusive live initialized writable one-byte destination.
    unsafe {
        (address as *mut u8).write(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(address: u64) -> CharacterFlagsObservation {
        CharacterFlagsObservation {
            character: CharacterIdentity {
                address,
                world_address: 0x2000,
                native_identifier: 0xfedcba9876543210,
                epoch: 3,
            },
            bytes: [0xa5; 6],
        }
    }
    #[test]
    fn death_mark_is_separate_from_aim_and_model_and_restores_only_its_bit() {
        #[repr(align(8))]
        struct Storage([u8; 0x580]);
        let mut storage = Storage([0x5a; 0x580]);
        let address = storage.0.as_mut_ptr() as u64;
        for raw in 0u8..=255 {
            for marked in [false, true] {
                let mut before = source(address);
                before.bytes[CharacterFlagByte::Status1.index()] = raw;
                storage.0[0x1c5] = raw;
                let original = storage.0;
                assert_eq!(before.native_death_marked(), raw & 0x80 != 0);
                let plan = flag_plan(
                    Some(before),
                    before,
                    CharacterFlagByte::Status1,
                    0x80,
                    u8::from(marked) << 7,
                )
                .unwrap();
                let after = plan.after;
                let receipt = commit_flag(
                    plan,
                    |destination, value| {
                        // SAFETY: Exclusive synthetic storage, never a game process.
                        unsafe { store_flag(destination, value) };
                    },
                    || Ok(after),
                );
                let mut expected = original;
                expected[0x1c5] = (raw & 0x7f) | (u8::from(marked) << 7);
                assert_eq!(storage.0, expected);
                assert_eq!(
                    after.precision_aim_requested(),
                    before.precision_aim_requested()
                );
                assert_eq!(
                    after.model_enable_requested(),
                    before.model_enable_requested()
                );
                assert_eq!(
                    flag_restore_plan(Some(after), &receipt).unwrap().after,
                    before
                );
                let mut foreign = after;
                foreign.bytes[CharacterFlagByte::Status1.index()] ^= 0x80;
                assert!(flag_restore_plan(Some(foreign), &receipt).is_err());
            }
        }
    }
    #[test]
    fn hit_dispatch_request_preserves_each_unrelated_byte_and_owned_restore() {
        #[repr(align(8))]
        struct Storage([u8; 0x580]);
        let mut storage = Storage([0x5a; 0x580]);
        let address = storage.0.as_mut_ptr() as u64;
        let field = CharacterFlagByte::DebugLow;
        for raw in 0..=255_u8 {
            for requested in [false, true] {
                let mut before = source(address);
                before.bytes[field.index()] = raw;
                storage.0[0x538] = raw;
                let original = storage.0;
                assert_eq!(before.hit_dispatch_suppression_requested(), raw & 8 != 0);
                let plan =
                    flag_plan(Some(before), before, field, 8, u8::from(requested) << 3).unwrap();
                let after = plan.after;
                let receipt = commit_flag(
                    plan,
                    |destination, value| {
                        // SAFETY: Exclusive initialized original storage; no native game calls.
                        unsafe { store_flag(destination, value) };
                    },
                    || Err(Error::InvalidProfile("postflight failed")),
                );
                let mut expected = original;
                expected[0x538] = (raw & !8) | (u8::from(requested) << 3);
                assert_eq!(storage.0, expected);
                assert_eq!(after.hit_dispatch_suppression_requested(), requested);
                if receipt.changed {
                    assert!(receipt.postflight.is_err());
                    assert_eq!(
                        flag_restore_plan(Some(after), &receipt).unwrap().after,
                        before
                    );
                } else {
                    assert_eq!(receipt.postflight, Ok(before));
                }
                let mut foreign = after;
                foreign.bytes[field.index()] ^= 8;
                assert!(flag_restore_plan(Some(foreign), &receipt).is_err());
            }
        }
    }
    #[test]
    fn model_request_preserves_lifecycle_bits_and_records_failed_store() {
        #[repr(align(8))]
        struct Storage([u8; 0x580]);
        let mut storage = Storage([0x5a; 0x580]);
        let address = storage.0.as_mut_ptr() as u64;
        for raw in 0..=255_u8 {
            for requested in [false, true] {
                let mut before = source(address);
                before.bytes[CharacterFlagByte::Status1.index()] = raw;
                storage.0[0x1c5] = raw;
                let original = storage.0;
                assert_eq!(before.model_enable_requested(), raw & 8 != 0);
                let plan = flag_plan(
                    Some(before),
                    before,
                    CharacterFlagByte::Status1,
                    8,
                    u8::from(requested) << 3,
                )
                .unwrap();
                let after = plan.after;
                let receipt = commit_flag(
                    plan,
                    |destination, value| {
                        // SAFETY: Exclusive initialized original test storage, no game access.
                        unsafe { store_flag(destination, value) };
                    },
                    || Err(Error::InvalidProfile("original postflight failure")),
                );
                let mut expected = original;
                expected[0x1c5] = (raw & !8) | (u8::from(requested) << 3);
                assert_eq!(storage.0, expected);
                assert_eq!(after.model_enable_requested(), requested);
                assert_eq!(receipt.mask, 8);
                if receipt.changed {
                    assert!(receipt.postflight.is_err());
                    assert_eq!(
                        flag_restore_plan(Some(after), &receipt).unwrap().after,
                        before
                    );
                    let mut foreign = after;
                    foreign.bytes[CharacterFlagByte::Status1.index()] ^= 8;
                    assert!(flag_restore_plan(Some(foreign), &receipt).is_err());
                } else {
                    assert_eq!(receipt.postflight, Ok(before));
                }
            }
        }
    }
    #[test]
    fn debug_update_request_preserves_other_bits_and_failed_store_receipt() {
        #[repr(align(8))]
        struct Storage([u8; 0x580]);
        let mut storage = Storage([0x5a; 0x580]);
        let address = storage.0.as_mut_ptr() as u64;
        let field = CharacterFlagByte::DebugHigh;
        for raw in 0..=255_u8 {
            for disabled in [false, true] {
                let mut before = source(address);
                before.bytes[field.index()] = raw;
                assert_eq!(before.debug_update_disabled_requested(), raw & 1 != 0);
                storage.0[0x539] = raw;
                let original = storage.0;
                let plan = flag_plan(Some(before), before, field, 1, u8::from(disabled)).unwrap();
                let after = plan.after;
                let receipt = commit_flag(
                    plan,
                    |destination, value| {
                        // SAFETY: Exclusive initialized original storage, no native/game access.
                        unsafe { store_flag(destination, value) };
                    },
                    || Err(Error::InvalidProfile("mock failed post-copy")),
                );
                let mut expected = original;
                expected[0x539] = (raw & !1) | u8::from(disabled);
                assert_eq!(storage.0, expected);
                assert_eq!(after.debug_update_disabled_requested(), disabled);
                if receipt.changed {
                    assert!(receipt.postflight.is_err());
                    assert_eq!(
                        flag_restore_plan(Some(after), &receipt).unwrap().after,
                        before
                    );
                } else {
                    assert_eq!(receipt.postflight, Ok(before));
                }
                let mut superseded = after;
                superseded.character.native_identifier ^= 1_u64 << 32;
                assert!(flag_restore_plan(Some(superseded), &receipt).is_err());
            }
        }
    }
    #[test]
    fn masked_stores_and_restore_touch_one_byte_and_preserve_foreign_unowned_bits() {
        #[repr(align(16))]
        struct Storage([u8; 0x580]);
        let mut storage = Storage([0x5a; 0x580]);
        let address = storage.0.as_mut_ptr() as u64;
        for (field, offset) in [
            (CharacterFlagByte::Status0, 0x1c4),
            (CharacterFlagByte::Status1, 0x1c5),
            (CharacterFlagByte::Status2, 0x1c8),
            (CharacterFlagByte::DebugLow, 0x538),
            (CharacterFlagByte::StoredTeam, 0x6c),
            (CharacterFlagByte::DebugHigh, 0x539),
        ] {
            let before = source(address);
            for offset in [0x1c4, 0x1c5, 0x1c8, 0x538, 0x6c, 0x539] {
                storage.0[offset] = 0xa5;
            }
            let original = storage.0;
            let plan = flag_plan(Some(before), before, field, 0x38, 0x38).unwrap();
            let after = plan.after;
            let receipt = commit_flag(
                plan,
                |addr, value| {
                    // SAFETY: Original exclusive live owned byte array, no game access.
                    unsafe { store_flag(addr, value) };
                },
                || Ok(after),
            );
            assert!(receipt.changed);
            assert_eq!(*receipt.postflight.as_ref().unwrap(), after);
            let mut expected = original;
            expected[offset] = 0xbd;
            assert_eq!(storage.0, expected);
            // Foreign code changed one unowned bit; explicit restore must preserve that bit.
            let mut current = after;
            current.bytes[field.index()] ^= 0x40;
            storage.0[offset] = current.value(field);
            let plan = flag_restore_plan(Some(current), &receipt).unwrap();
            let restored = plan.after;
            let receipt = commit_flag(
                plan,
                |addr, value| {
                    // SAFETY: Same original owned writable array and exclusive test phase.
                    unsafe { store_flag(addr, value) };
                },
                || Ok(restored),
            );
            assert!(receipt.postflight.is_ok());
            expected[offset] = 0xe5;
            assert_eq!(storage.0, expected);
        }
    }
    #[test]
    fn flag_source_and_restore_reject_superseded_owned_bits_and_invalid_masks() {
        let before = source(0x1000);
        let f = CharacterFlagByte::DebugLow;
        for (mask, bits) in [(0, 0), (8, 16)] {
            assert!(flag_plan(Some(before), before, f, mask, bits).is_err());
        }
        assert!(flag_plan(None, before, f, 8, 8).is_err());
        for mode in 0..7 {
            let mut foreign = before;
            match mode {
                0 => foreign.character.address += 8,
                1 => foreign.character.world_address += 8,
                2 => foreign.character.native_identifier += 1,
                3 => foreign.character.epoch += 1,
                4 => foreign.bytes[0] ^= 1,
                5 => foreign.bytes[3] ^= 1,
                _ => foreign.bytes[4] ^= 1,
            }
            assert!(flag_plan(Some(foreign), before, f, 8, 8).is_err());
        }
        for address in [0, 1, u64::MAX - 7] {
            let bad = source(address);
            assert!(flag_plan(Some(bad), bad, f, 8, 8).is_err());
        }
        let plan = flag_plan(Some(before), before, f, 8, 8).unwrap();
        let after = plan.after;
        let calls = std::cell::Cell::new(0);
        let receipt = commit_flag(
            plan,
            |_, _| calls.set(calls.get() + 1),
            || Err(Error::InvalidProfile("lost copy")),
        );
        assert!(receipt.changed);
        assert!(receipt.postflight.is_err());
        assert_eq!(calls.get(), 1);
        assert!(flag_restore_plan(None, &receipt).is_err());
        let mut foreign = after;
        foreign.bytes[3] ^= 8;
        assert!(flag_restore_plan(Some(foreign), &receipt).is_err());
        foreign = after;
        foreign.character.epoch += 1;
        assert!(flag_restore_plan(Some(foreign), &receipt).is_err());
        assert!(flag_restore_plan(Some(after), &receipt).is_ok()); // Postflight loss never erased the store obligation.
        let noop = flag_plan(Some(before), before, f, 8, before.value(f) & 8).unwrap();
        assert!(!commit_flag(noop, |_, _| unreachable!(), || unreachable!()).changed);
    }
}
