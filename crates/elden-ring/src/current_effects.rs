//! Current-process ER effects. Independent entry evidence, no game-runtime acceptance.
use crate::{
    WW_171, Ww171File,
    character_sets::CharacterIdentity,
    characters::CharacterEffectsObservation,
    effects::{EffectApplication, EffectEngine, EffectId},
    objects::GameObjectCatalog,
    summon_triggers::SummonTriggerEffectBackend,
};
use shikifsrs_runtime::code_patches::OwnedCodePatches;
use shikifsrs_runtime::{
    Error, Result,
    memory::{AddressSpace, CurrentProcess},
    profile::{CodeRange, OwnedVerifiedFile, VerifiedModule},
};
use std::{ffi::c_void, sync::Arc};

const EFFECT_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3e8dc0,
        length: 0xa7,
    },
    CodeRange {
        rva: 0x3e8e70,
        length: 0x346,
    },
    CodeRange {
        rva: 0x3fb010,
        length: 0x263,
    },
    CodeRange {
        rva: 0x4f7680,
        length: 0x34,
    },
    CodeRange {
        rva: 0x4fc500,
        length: 0x1ac,
    },
    CodeRange {
        rva: 0x4fb6b0,
        length: 0x119,
    },
    CodeRange {
        rva: 0x4ffa60,
        length: 0x1f,
    },
    CodeRange {
        rva: 0x4fd2b0,
        length: 0x1a7,
    },
    CodeRange {
        rva: 0x4fde60,
        length: 0x323,
    },
];
const EFFECT_CONSTANTS: &[CodeRange] = &[
    CodeRange {
        rva: 0x32a2690,
        length: 0x10,
    },
    CodeRange {
        rva: 0x32a1ab0,
        length: 0x10,
    },
    CodeRange {
        rva: 0x32a2600,
        length: 0x10,
    },
    CodeRange {
        rva: 0x32a1938,
        length: 0x4,
    },
];
const GAME_STATE_GLOBAL: u32 = 0x3d7e540;

type ApplyEntry = unsafe extern "system" fn(*mut c_void, i32, u8);
type RemoveEntry = unsafe extern "system" fn(*mut c_void, i32) -> u8;

pub struct CurrentEffectEngine {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    limit: usize,
    epoch: u64,
}

impl SummonTriggerEffectBackend for CurrentEffectEngine {
    fn primary_effects(&self) -> Result<Option<CharacterEffectsObservation>> {
        let file = self.file.borrow()?;
        let view = Ww171File::from_verified(file)?;
        let catalog =
            view.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?;
        let primary = catalog.primary_player_identity(self.epoch)?;
        let result = match primary {
            Some(primary) => Some(catalog.character_effects(primary, self.limit)?.ok_or(
                Error::NativeOperationRejected("primary summon effect storage is absent"),
            )?),
            None => None,
        };
        if catalog.primary_player_identity(self.epoch)? != primary {
            return Err(Error::NativeOperationRejected(
                "primary changed during summon effect copy",
            ));
        }
        Ok(result)
    }
}
impl CurrentEffectEngine {
    /// Revoke old copied targets after an independently known lifetime transition. No game call.
    /// # Safety
    /// Caller exclusively owns this backend and establishes the new true nonzero epoch, native
    /// graph/phase/PARAM/heap lifetime for all future operations. Resolve old effect obligations
    /// independently; advancing a number does not clear old effects, pin objects or detect ABA.
    pub unsafe fn advance_epoch(&mut self, epoch: u64) -> Result<()> {
        if epoch <= self.epoch {
            return Err(Error::InvalidProfile("effect epoch must advance"));
        }
        self.epoch = epoch;
        Ok(())
    }
    /// File identity is checked before current-process reads; binding calls no native function.
    /// # Safety
    /// base identifies this process's ER module. Caller owns an initialized engine mutation
    /// phase, live character and all required native modules, heaps and PARAM resources.
    /// No concurrent teardown, object reuse or code patch may overlap any operation.
    /// Native acceptance/force behavior and unlink side effects await runtime validation.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        effect_limit: usize,
        epoch: u64,
    ) -> Result<Self> {
        // SAFETY: Original bind obligations; no registered replacements by default.
        unsafe { Self::bind_with_patches(bytes, base, effect_limit, epoch, None) }
    }

    /// Retain an explicitly owned code replacement manifest.
    /// # Safety
    /// bind and the manifest owner's ABI, semantics, transitive lifetime and patch
    /// quiescence obligations apply throughout every native call and return frame.
    pub unsafe fn bind_with_patches(
        bytes: Arc<[u8]>,
        base: u64,
        effect_limit: usize,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if !(1..=65536).contains(&effect_limit) || epoch == 0 {
            return Err(Error::InvalidProfile("invalid effect mutation limit"));
        }
        let file = OwnedVerifiedFile::verify(bytes, WW_171)?;
        let view = file.borrow()?;
        Ww171File::from_verified(view)?.object_types()?;
        let checked = file.borrow()?;
        let module = checked.bind_with_patches(
            &CurrentProcess,
            base,
            EFFECT_EVIDENCE,
            patches.as_deref(),
        )?;
        validate_constants(&module)?;
        Ok(Self {
            file,
            patches,
            base,
            limit: effect_limit,
            epoch,
        })
    }

    fn checked<R>(
        &self,
        target: CharacterIdentity,
        action: impl FnOnce(&GameObjectCatalog<'_, '_, CurrentProcess>, u64) -> Result<R>,
    ) -> Result<R> {
        let view = self.file.borrow()?;
        let memory = CurrentProcess;
        let module =
            view.bind_with_patches(&memory, self.base, EFFECT_EVIDENCE, self.patches.as_deref())?;
        for range in crate::characters::CHARACTER_EVIDENCE {
            module.code_address(*range)?;
        }
        validate_constants(&module)?;
        let catalog = GameObjectCatalog { module };
        if target.epoch != self.epoch
            || catalog.character_identity(target.address, self.epoch)? != Some(target)
        {
            return Err(Error::NativeOperationRejected(
                "effect target identity changed",
            ));
        }
        if catalog.character_data(target.address)?.is_none()
            || catalog.world()?.is_none()
            || catalog.parameter_repository()?.is_none()
        {
            return Err(Error::NativeOperationRejected(
                "character modules/world/PARAM repository are not initialized",
            ));
        }
        let (container, _) = catalog
            .effect_state(target.address, None, self.limit)?
            .ok_or(Error::NativeOperationRejected("effect container is absent"))?;
        action(&catalog, container)
    }
}

fn validate_constants<M: AddressSpace>(module: &VerifiedModule<'_, '_, M>) -> Result<()> {
    for span in EFFECT_CONSTANTS {
        let image = module.file().image();
        let section = image
            .section_at(span.rva)
            .filter(|s| !s.is_writable() && !s.is_executable())
            .ok_or(Error::InvalidProfile(
                "effect defaults are not readonly file data",
            ))?;
        if span.rva as u64 + span.length as u64 > section.rva as u64 + section.raw_size as u64 {
            return Err(Error::InvalidProfile("effect defaults cross file data"));
        }
        let expected = image.read_rva(span.rva, span.length)?;
        let mut actual = vec![0; span.length];
        let address = module
            .base()
            .checked_add(span.rva as u64)
            .ok_or(Error::AddressOverflow)?;
        module.memory().read_exact(address, &mut actual)?;
        if expected != actual {
            return Err(Error::ImageMismatch { rva: span.rva });
        }
    }
    Ok(())
}

fn entry<M: AddressSpace>(module: &VerifiedModule<'_, '_, M>, rva: u32) -> Result<usize> {
    let span = EFFECT_EVIDENCE
        .iter()
        .find(|span| span.rva == rva)
        .ok_or(Error::InvalidProfile(
            "effect entry has no independent evidence",
        ))?;
    Ok(module.code_address(*span)? as usize)
}

// SAFETY: Exact file identity/loaded entries/defaults and copied target/owner checks precede
// native operations. Caller owns the explicit unsafe phase, heap, PARAM and lifetime contract.
unsafe impl EffectEngine for CurrentEffectEngine {
    unsafe fn apply(
        &self,
        target: CharacterIdentity,
        effect: EffectId,
        force: bool,
    ) -> Result<EffectApplication> {
        self.checked(target, |catalog, _| {
            let state = catalog
                .module
                .global_pointer(GAME_STATE_GLOBAL)?
                .filter(|p| p.is_multiple_of(8))
                .ok_or(Error::NativeOperationRejected(
                    "native game state is uninitialized",
                ))?;
            catalog
                .module
                .memory()
                .read_u32(state.checked_add(0x10).ok_or(Error::AddressOverflow)?)?;
            let address = entry(&catalog.module, 0x3e8dc0)?;
            // SAFETY: Independently proven AMD64 wrapper consumes rcx target, edx ID, r8b force.
            let apply: ApplyEntry = unsafe { std::mem::transmute(address) };
            // SAFETY: Caller owns mutation phase and all live dependencies; wrapper has no receipt.
            unsafe { invoke_apply(apply, target.address, effect, force) };
            Ok(EffectApplication::Submitted)
        })
    }
    unsafe fn remove(&self, target: CharacterIdentity, effect: EffectId) -> Result<bool> {
        self.checked(target, |catalog, container| {
            let address = entry(&catalog.module, 0x4f7680)?;
            // SAFETY: Independently proven container/ID ABI, not ordinal or category removal.
            let remove: RemoveEntry = unsafe { std::mem::transmute(address) };
            // SAFETY: Owner and complete chain checked; caller guarantees unlink phase and lifetime.
            unsafe { invoke_remove(remove, container, effect) }
        })
    }
}

unsafe fn invoke_apply(apply: ApplyEntry, target: u64, effect: EffectId, force: bool) {
    // SAFETY: Caller supplies the validated entry and its live target/phase contract.
    unsafe { apply(target as *mut c_void, effect.value(), u8::from(force)) };
}
unsafe fn invoke_remove(remove: RemoveEntry, container: u64, effect: EffectId) -> Result<bool> {
    // SAFETY: Caller supplies the validated entry and its live container/phase contract.
    match unsafe { remove(container as *mut c_void, effect.value()) } {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::NativeOperationRejected(
            "noncanonical effect removal receipt",
        )),
    }
}

#[cfg(test)]
mod tests;
