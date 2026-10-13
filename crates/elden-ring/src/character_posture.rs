//! Selected posture words and an explicit native maximum query. Experimental.
use crate::{
    WW_171, Ww171File,
    character_modules::{CharacterModule, CharacterModuleObservation},
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::{
    Error, Result,
    code_patches::OwnedCodePatches,
    memory::{AddressSpace, CurrentProcess},
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostureKind {
    SuperArmor,
    Toughness,
}
impl PostureKind {
    fn module(self) -> CharacterModule {
        match self {
            Self::SuperArmor => CharacterModule::SuperArmor,
            Self::Toughness => CharacterModule::Toughness,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterPostureObservation {
    pub module: CharacterModuleObservation,
    pub kind: PostureKind,
    pub object_kind: ObjectKind,
    pub current_bits: u32,
    pub previous_bits: u32,
    /// Toughness maximum; SuperArmor adjustment to a separately computed native base.
    /// It is never the SuperArmor maximum by itself.
    pub reference_bits: u32,
}

fn copy_words<M: AddressSpace>(memory: &M, address: u64) -> Result<[u32; 3]> {
    let mut words = [0; 3];
    for (index, word) in words.iter_mut().enumerate() {
        *word = memory.read_u32(
            address
                .checked_add(0x10 + index as u64 * 4)
                .ok_or(Error::AddressOverflow)?,
        )?;
    }
    Ok(words)
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Two copies of exactly three words plus reciprocal typed module and full identity checks.
    /// Raw float bits, including nonfinite values, are preserved; no queue or other slot is read.
    pub fn character_posture(
        &self,
        target: CharacterIdentity,
        kind: PostureKind,
    ) -> Result<Option<CharacterPostureObservation>> {
        let Some(module) = self.character_module(target, kind.module())? else {
            return Ok(None);
        };
        let object_kind = self
            .character(target.address)?
            .ok_or(Error::InvalidProfile("posture character disappeared"))?
            .kind;
        let words = copy_words(self.module.memory(), module.address)?;
        if copy_words(self.module.memory(), module.address)? != words
            || self.character_module(target, kind.module())? != Some(module)
            || self.character(target.address)?.map(|v| v.kind) != Some(object_kind)
        {
            return Err(Error::InvalidProfile(
                "posture words or typed ownership changed",
            ));
        }
        Ok(Some(CharacterPostureObservation {
            module,
            kind,
            object_kind,
            current_bits: words[0],
            previous_bits: words[1],
            reference_bits: words[2],
        }))
    }
}

#[derive(Debug)]
pub struct NativeSuperArmorMaximumCall {
    pub source: CharacterPostureObservation,
    pub native_base_bits: u32,
    /// Finite native base plus the freshly observed adjustment. No gameplay completion implied.
    pub maximum: Result<f32>,
    /// Actual call remains recorded even if the copied graph or words changed afterwards.
    pub postflight: Result<CharacterPostureObservation>,
}

fn maximum_entry(
    expected: CharacterPostureObservation,
    current: Option<CharacterPostureObservation>,
    epoch: u64,
) -> Result<u32> {
    if epoch == 0
        || expected.module.character.epoch != epoch
        || expected.kind != PostureKind::SuperArmor
        || expected.module.module != CharacterModule::SuperArmor
        || current != Some(expected)
    {
        return Err(Error::NativeOperationRejected(
            "super armor maximum source superseded",
        ));
    }
    match expected.object_kind {
        ObjectKind::Character => Ok(0x47dff0),
        ObjectKind::Enemy => Ok(0x47ef20),
        ObjectKind::Player => Ok(0x47f380),
        _ => Err(Error::InvalidProfile(
            "maximum query requires a typed character",
        )),
    }
}

fn commit_maximum(
    source: CharacterPostureObservation,
    call: impl FnOnce(u64) -> f32,
    observe: impl FnOnce() -> Result<CharacterPostureObservation>,
) -> NativeSuperArmorMaximumCall {
    let base = call(source.module.address);
    let value = base + f32::from_bits(source.reference_bits);
    let maximum = if base.is_finite() && value.is_finite() {
        Ok(value)
    } else {
        Err(Error::InvalidProfile(
            "native super armor maximum is nonfinite",
        ))
    };
    let postflight = observe().and_then(|after| {
        if after == source {
            Ok(after)
        } else {
            Err(Error::InvalidProfile(
                "super armor maximum graph or words changed after call",
            ))
        }
    });
    NativeSuperArmorMaximumCall {
        source,
        native_base_bits: base.to_bits(),
        maximum,
        postflight,
    }
}

pub struct CurrentCharacterPosture {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentCharacterPosture {
    /// # Safety
    /// Caller identifies the loaded ER module and holds an exclusive permitted phase with the
    /// complete character/module/parameter/game-data graph pinned and no overlapping Rust aliases,
    /// code patches or teardown. Epochs and copied ownership do not provide these guarantees.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero posture epoch"));
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
        kind: PostureKind,
    ) -> Result<Option<CharacterPostureObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .character_posture(target, kind)
    }
    /// Calls the exact current-version SuperArmor virtual implementation once. The Player
    /// implementation refreshes game-data calculations, so this is not a read-only observation.
    /// No automatic retry, restoration or Drop call occurs; retain the returned actual-call receipt.
    /// # Safety
    /// Bind's phase, complete graph, code ownership and lifetime requirements hold through the
    /// call and postflight. Caller owns the receipt even when maximum or postflight is an error.
    pub unsafe fn query_super_armor_maximum(
        &self,
        expected: CharacterPostureObservation,
    ) -> Result<NativeSuperArmorMaximumCall> {
        let rva = maximum_entry(
            expected,
            self.observe(expected.module.character, expected.kind)?,
            self.epoch,
        )?;
        let address = self
            .base
            .checked_add(u64::from(rva))
            .ok_or(Error::AddressOverflow)?;
        let address = usize::try_from(address).map_err(|_| Error::AddressOverflow)?;
        maximum_entry(
            expected,
            self.observe(expected.module.character, expected.kind)?,
            self.epoch,
        )?;
        // SAFETY: Exact independently identified implementation, MS x64 RCX=this and XMM0=float.
        // Caller holds the whole transitive graph and exclusive engine phase across the call.
        let function: unsafe extern "system" fn(u64) -> f32 =
            unsafe { std::mem::transmute(address) };
        Ok(commit_maximum(
            expected,
            |module| {
                // SAFETY: Same forwarded caller contract; this is the single actual native call.
                unsafe { function(module) }
            },
            || {
                self.observe(expected.module.character, expected.kind)?
                    .ok_or(Error::InvalidProfile(
                        "posture disappeared after maximum query",
                    ))
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    fn source() -> CharacterPostureObservation {
        CharacterPostureObservation {
            module: CharacterModuleObservation {
                character: CharacterIdentity {
                    address: 0x1000,
                    world_address: 0x2000,
                    native_identifier: 9,
                    epoch: 3,
                },
                module: CharacterModule::SuperArmor,
                container_address: 0x3000,
                address: 0x4000,
            },
            kind: PostureKind::SuperArmor,
            object_kind: ObjectKind::Player,
            current_bits: 20f32.to_bits(),
            previous_bits: 21f32.to_bits(),
            reference_bits: (-2f32).to_bits(),
        }
    }
    #[test]
    fn maximum_uses_native_base_plus_adjustment_and_retains_every_actual_call_failure() {
        let source = source();
        let calls = Cell::new(0);
        let receipt = commit_maximum(
            source,
            |address| {
                assert_eq!(address, 0x4000);
                calls.set(calls.get() + 1);
                100.0
            },
            || Ok(source),
        );
        assert_eq!(receipt.maximum.unwrap(), 98.0);
        assert_eq!(receipt.postflight.unwrap(), source);
        let receipt = commit_maximum(
            source,
            |_| {
                calls.set(calls.get() + 1);
                f32::NAN
            },
            || Err(Error::InvalidProfile("synthetic unload")),
        );
        assert!(receipt.maximum.is_err());
        assert!(receipt.postflight.is_err());
        assert!(f32::from_bits(receipt.native_base_bits).is_nan());
        assert_eq!(calls.get(), 2);
        let changed = CharacterPostureObservation {
            current_bits: 0,
            ..source
        };
        let receipt = commit_maximum(source, |_| 100.0, || Ok(changed));
        assert!(receipt.postflight.is_err());
        assert_eq!(receipt.maximum.unwrap(), 98.0);
    }
    #[test]
    fn maximum_dispatch_refuses_stale_epoch_other_modules_and_changed_full_identity() {
        let source = source();
        for (kind, rva) in [
            (ObjectKind::Character, 0x47dff0),
            (ObjectKind::Enemy, 0x47ef20),
            (ObjectKind::Player, 0x47f380),
        ] {
            let expected = CharacterPostureObservation {
                object_kind: kind,
                ..source
            };
            assert_eq!(maximum_entry(expected, Some(expected), 3).unwrap(), rva);
        }
        assert!(maximum_entry(source, Some(source), 4).is_err());
        assert!(maximum_entry(source, None, 3).is_err());
        for changed in [
            CharacterPostureObservation {
                reference_bits: 0,
                ..source
            },
            CharacterPostureObservation {
                kind: PostureKind::Toughness,
                ..source
            },
            CharacterPostureObservation {
                module: CharacterModuleObservation {
                    address: 0x5000,
                    ..source.module
                },
                ..source
            },
        ] {
            assert!(maximum_entry(source, Some(changed), 3).is_err());
        }
    }
}
