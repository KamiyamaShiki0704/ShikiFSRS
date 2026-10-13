//! Native aim request and input-policy copies. Gameplay transition remains experimental.
use crate::{
    character_modules::{CharacterModule, CharacterModuleObservation},
    character_sets::CharacterIdentity,
    objects::GameObjectCatalog,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
pub(crate) const AIM_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x405d40,
        length: 0x1ea,
    },
    CodeRange {
        rva: 0x43d330,
        length: 0x4d,
    },
    CodeRange {
        rva: 0x3fe100,
        length: 0x611,
    },
    CodeRange {
        rva: 0x3db4e0,
        length: 0x165,
    },
    CodeRange {
        rva: 0x427b30,
        length: 0x12ac,
    },
    CodeRange {
        rva: 0x3f7730,
        length: 0x3f,
    },
    CodeRange {
        rva: 0x3f4a90,
        length: 0xc,
    },
    CodeRange {
        rva: 0x3f2da0,
        length: 0x6c,
    },
];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterAimInputObservation {
    pub module: CharacterModuleObservation,
    /// Constructor-cleared word40; only the selected scalar is read.
    pub input_modifier_bits: u64,
}
impl CharacterAimInputObservation {
    /// Exact low-byte suppression predicate in 3f2da0, consumed by frontend778200.
    /// Other registration/controller guards can suppress the gauge independently.
    pub fn floating_gauge_suppression_requested(self) -> bool {
        self.input_modifier_bits & (1 << 44) != 0
    }
    /// Native manual/held-input branch in 3db4e0 tests word40 bit18 before
    /// its input2 hold query and aim-request setter. This is input admission,
    /// not current camera mode or proof of animation/projectile completion.
    pub fn manual_aim_input_admitted(self) -> bool {
        self.input_modifier_bits & (1 << 18) != 0
    }
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    pub fn character_aim_input(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterAimInputObservation>> {
        let Some(module) = self.character_module(target, CharacterModule::ActionFlags)? else {
            return Ok(None);
        };
        let address = module
            .address
            .checked_add(0x40)
            .ok_or(Error::AddressOverflow)?;
        let input_modifier_bits = self.module.memory().read_u64(address)?;
        if self.module.memory().read_u64(address)? != input_modifier_bits
            || self.character_module(target, CharacterModule::ActionFlags)? != Some(module)
        {
            return Err(Error::InvalidProfile(
                "aim input module or selected word changed",
            ));
        }
        Ok(Some(CharacterAimInputObservation {
            module,
            input_modifier_bits,
        }))
    }
}

impl crate::character_flags::CharacterFlagsObservation {
    /// Exact bit consumed by native 3f4a90 and set by 3f7730. Camera transition is separate.
    pub fn precision_aim_requested(self) -> bool {
        self.value(crate::character_flags::CharacterFlagByte::Status1) & 2 != 0
    }
    /// Native camera case2 copies the aim-camera transform and sets this additional bit.
    /// This diagnostic bit is not an engine lease or a complete frame snapshot.
    pub fn precision_camera_state(self) -> bool {
        self.value(crate::character_flags::CharacterFlagByte::Status1) & 4 != 0
    }
}
impl crate::controller_policy::ControllerPolicyObservation {
    /// Native HKS command58 sets data1c bit13 then clears aim request through 3f7730.
    pub fn precision_aim_suppressed(self) -> Option<bool> {
        self.modifier_word1c.map(|word| word & 0x2000 != 0)
    }
}
impl crate::current_character_flags::CurrentCharacterFlags {
    /// Direct selected request-byte edit used by phase coordination; it does not call
    /// the native setter or implicitly honor its controller filter. Caller coordinates that separately.
    /// # Safety
    /// compare_mask's full graph/lifetime/exclusive permitted phase and writable byte
    /// contract holds. Retain actual receipt; reconcile failed postflight before later edits.
    pub unsafe fn set_precision_aim_requested(
        &self,
        expected: crate::character_flags::CharacterFlagsObservation,
        requested: bool,
    ) -> Result<crate::character_flags::CharacterFlagWrite> {
        // SAFETY: Caller supplies the complete conditional flag-store contract.
        unsafe {
            self.compare_mask(
                expected,
                crate::character_flags::CharacterFlagByte::Status1,
                2,
                u8::from(requested) << 1,
            )
        }
    }
}
impl crate::current_controller_policy::CurrentControllerPolicy {
    /// Changes only the independently observed HKS suppression request. It does not
    /// clear an existing character request itself or establish native script completion.
    /// # Safety
    /// compare_mask's full controller/modifier graph, lifetime, epoch, exclusive permitted
    /// phase and writable word contract holds. Retain and reconcile actual receipts.
    pub unsafe fn set_precision_aim_suppressed(
        &self,
        expected: crate::controller_policy::ControllerPolicyObservation,
        suppressed: bool,
    ) -> Result<crate::controller_policy::ControllerPolicyWrite> {
        // SAFETY: Caller supplies the complete conditional policy-store contract.
        unsafe {
            self.compare_mask(
                expected,
                crate::controller_policy::ControllerPolicyField::ModifierWord1c,
                0x2000,
                u32::from(suppressed) << 13,
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterInputModifierWrite {
    pub source: CharacterAimInputObservation,
    pub destination: u64,
    pub replacement_bits: u64,
    pub stored: bool,
    pub postflight: Result<CharacterAimInputObservation>,
}
fn commit_gauge(
    source: CharacterAimInputObservation,
    disabled: bool,
    mut observe: impl FnMut() -> Result<Option<CharacterAimInputObservation>>,
    store: impl FnOnce(u64, u64),
) -> Result<CharacterInputModifierWrite> {
    if source.module.module != CharacterModule::ActionFlags
        || source.module.character.epoch == 0
        || source.module.address == 0
        || !source.module.address.is_multiple_of(8)
    {
        return Err(Error::InvalidProfile(
            "invalid floating gauge module source",
        ));
    }
    let destination = source
        .module
        .address
        .checked_add(0x40)
        .ok_or(Error::AddressOverflow)?;
    destination.checked_add(7).ok_or(Error::AddressOverflow)?;
    for _ in 0..2 {
        if observe()? != Some(source) {
            return Err(Error::NativeOperationRejected(
                "floating gauge source changed",
            ));
        }
    }
    let mask = 1u64 << 44;
    let replacement_bits = (source.input_modifier_bits & !mask) | (u64::from(disabled) << 44);
    let after = CharacterAimInputObservation {
        input_modifier_bits: replacement_bits,
        ..source
    };
    let stored = replacement_bits != source.input_modifier_bits;
    if stored {
        store(destination, replacement_bits);
    }
    let postflight = observe().and_then(|actual| {
        if actual == Some(after) {
            Ok(after)
        } else {
            Err(Error::NativeOperationRejected(
                "floating gauge postflight changed",
            ))
        }
    });
    Ok(CharacterInputModifierWrite {
        source,
        destination,
        replacement_bits,
        stored,
        postflight,
    })
}
pub struct CurrentCharacterInputModifiers {
    file: shikifsrs_runtime::profile::OwnedVerifiedFile,
    patches: Option<std::sync::Arc<shikifsrs_runtime::code_patches::OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl CurrentCharacterInputModifiers {
    /// # Safety
    /// Retain full initialized exact character/container/module graph, current epoch and original
    /// allocation in the exclusive permitted phase; exclude teardown, aliases, updates and ABA.
    pub unsafe fn bind(
        bytes: std::sync::Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<std::sync::Arc<shikifsrs_runtime::code_patches::OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero input modifier epoch"));
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
    pub fn observe(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterAimInputObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        crate::Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(
                &shikifsrs_runtime::memory::CurrentProcess,
                self.base,
                self.patches.as_deref(),
            )?
            .character_aim_input(target)
    }
    /// # Safety
    /// Bind's full exclusive graph/phase/lifetime/epoch/ABA contract holds plus exclusive
    /// initialized writable eight-byte word40. Only bit44 changes, including when bit18 is set.
    /// Preserve pending intent and actual receipt on interruptions or failed postflight. No retry,
    /// rollback or Drop writes; storage does not prove actual gauge hiding or native ownership.
    pub unsafe fn set_floating_gauge_suppressed(
        &self,
        source: CharacterAimInputObservation,
        disabled: bool,
    ) -> Result<CharacterInputModifierWrite> {
        if source.module.character.epoch != self.epoch {
            return Err(Error::NativeOperationRejected(
                "foreign input modifier epoch",
            ));
        }
        commit_gauge(
            source,
            disabled,
            || self.observe(source.module.character),
            |address, bits| {
                // SAFETY: Caller retains exact initialized writable word under exclusive full graph contract.
                unsafe { (address as *mut u64).write_volatile(bits) };
            },
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn gauge_source(bits: u64) -> CharacterAimInputObservation {
        CharacterAimInputObservation {
            module: CharacterModuleObservation {
                character: CharacterIdentity {
                    address: 0x1000,
                    world_address: 0x2000,
                    native_identifier: 0xfedcba9876543210,
                    epoch: 7,
                },
                module: CharacterModule::ActionFlags,
                container_address: 0x3000,
                address: 0x4000,
            },
            input_modifier_bits: bits,
        }
    }
    #[test]
    fn gauge_store_keeps_manual_aim_and_every_neighbor_bit_actual_receipts_and_failed_postflight() {
        for raw in [0, u64::MAX, 1 << 18, 1 << 44, 0xa5a5a5a5a5a5a5a5] {
            for disabled in [false, true] {
                let source = gauge_source(raw);
                let expected = (raw & !(1 << 44)) | (u64::from(disabled) << 44);
                let mut reads = 0;
                let mut stores = 0;
                let receipt = commit_gauge(
                    source,
                    disabled,
                    || {
                        reads += 1;
                        if reads <= 2 {
                            Ok(Some(source))
                        } else {
                            Err(Error::InvalidProfile("synthetic postflight failure"))
                        }
                    },
                    |address, bits| {
                        stores += 1;
                        assert_eq!(address, 0x4040);
                        assert_eq!(bits, expected);
                    },
                )
                .unwrap();
                assert_eq!(receipt.source, source);
                assert_eq!(receipt.replacement_bits, expected);
                assert_eq!(stores, usize::from(raw != expected));
                assert_eq!(receipt.stored, raw != expected);
                assert!(receipt.postflight.is_err());
                assert_eq!(expected & !(1 << 44), raw & !(1 << 44));
                assert_eq!(
                    source.floating_gauge_suppression_requested(),
                    raw & (1 << 44) != 0
                );
                let after = CharacterAimInputObservation {
                    input_modifier_bits: expected,
                    ..source
                };
                let mut reads = 0;
                assert_eq!(
                    commit_gauge(
                        source,
                        disabled,
                        || {
                            reads += 1;
                            Ok(Some(if reads <= 2 { source } else { after }))
                        },
                        |_, _| {}
                    )
                    .unwrap()
                    .postflight,
                    Ok(after)
                );
            }
        }
    }
    #[test]
    fn gauge_source_changes_across_module_owner_identity_or_word_prevent_every_store() {
        let source = gauge_source(1 << 18);
        for field in 0..8 {
            let mut changed = source;
            match field {
                0 => changed.module.character.epoch += 1,
                1 => changed.module.character.native_identifier ^= 1 << 48,
                2 => changed.module.character.world_address += 8,
                3 => changed.module.character.address += 8,
                4 => changed.module.container_address += 8,
                5 => changed.module.address += 8,
                6 => changed.module.module = CharacterModule::ActionRequests,
                _ => changed.input_modifier_bits ^= 1 << 44,
            }
            let mut reads = 0;
            assert!(
                commit_gauge(
                    source,
                    true,
                    || {
                        reads += 1;
                        Ok(Some(if reads == 1 { source } else { changed }))
                    },
                    |_, _| panic!("must not store")
                )
                .is_err()
            );
        }
    }
    #[test]
    fn input_admission_request_and_camera_transition_are_independent_bits() {
        let target = CharacterIdentity {
            epoch: 1,
            world_address: 0x2000,
            address: 0x3000,
            native_identifier: 7,
        };
        let module = CharacterModuleObservation {
            character: target,
            module: CharacterModule::ActionFlags,
            container_address: 0x4000,
            address: 0x5000,
        };
        for bits in [0u64, 1, 1 << 18, u64::MAX, !(1 << 18)] {
            assert_eq!(
                CharacterAimInputObservation {
                    module,
                    input_modifier_bits: bits
                }
                .manual_aim_input_admitted(),
                bits & (1 << 18) != 0
            );
        }
        for value in 0u8..=255 {
            let flags = crate::character_flags::CharacterFlagsObservation {
                character: target,
                bytes: [0, value, 0, 0, 0, 0],
            };
            assert_eq!(flags.precision_aim_requested(), value & 2 != 0);
            assert_eq!(flags.precision_camera_state(), value & 4 != 0);
        }
    }
}
