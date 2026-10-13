//! Thread-confined conditional signed current-word stores, without native calls or Drop cleanup.
use crate::{
    WW_171, Ww171File,
    character_sets::CharacterIdentity,
    character_vitals::{
        CharacterVital, CharacterVitalWrite, CharacterVitalsObservation, commit_vital, store_vital,
        vital_plan, vital_restore_plan,
    },
};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};
pub struct CurrentCharacterVitals {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentCharacterVitals {
    /// # Safety
    /// Caller pins the whole character/module graph and initialized writable current-word
    /// extent, provides the exclusive permitted phase/nonzero epoch and excludes callbacks,
    /// Rust aliases, teardown/reuse and unload. Read/code/identity checks supply no engine guard.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero vital store epoch"));
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
    pub fn observe(&self, target: CharacterIdentity) -> Result<Option<CharacterVitalsObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .character_vitals(target)
    }
    /// # Safety
    /// Bind's full writable phase/lifetime contract holds. Caller chooses a gameplay-appropriate
    /// signed value; negative stamina and unknown raw values are not silently clamped. This
    /// changes one current word, not its maximum, death state, animation or load/activation.
    /// A failed postflight retains the executed store, requiring reconciliation before further
    /// operations. No retry or restoration happens automatically.
    pub unsafe fn compare_current(
        &self,
        expected: CharacterVitalsObservation,
        field: CharacterVital,
        value: i32,
    ) -> Result<CharacterVitalWrite> {
        let plan = vital_plan(self.observe(expected.character)?, expected, field, value)?;
        Ok(commit_vital(
            plan,
            |address, value| {
                // SAFETY: Caller guarantees full exclusive pinned writable extent; plan checks source.
                unsafe { store_vital(address, value) };
            },
            || {
                self.observe(expected.character)?
                    .ok_or(Error::InvalidProfile("vitals unavailable after store"))
            },
        ))
    }
    /// Restore only a still-owned current word, preserving current maxima and other vitals.
    /// # Safety
    /// Same full contract as compare_current. Caller independently confirms that the saved
    /// original remains appropriate under today's maxima/engine state; no HP-bound or alive
    /// inference is made. Receipt must accurately represent this lifetime's actual store.
    pub unsafe fn restore_current(
        &self,
        applied: &CharacterVitalWrite,
    ) -> Result<CharacterVitalWrite> {
        let plan = vital_restore_plan(self.observe(applied.source.character)?, applied)?;
        Ok(commit_vital(
            plan,
            |address, value| {
                // SAFETY: Same full stable exclusive current-word extent as original store.
                unsafe { store_vital(address, value) };
            },
            || {
                self.observe(applied.source.character)?
                    .ok_or(Error::InvalidProfile("vitals unavailable after restore"))
            },
        ))
    }
}
