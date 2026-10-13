//! Current-process selected unit reader. No native calls, mutation or worker thread.
use crate::{
    WW_171, Ww171File,
    character_sets::CharacterIdentity,
    character_units::{CharacterUnitReadPlan, CharacterUnitSnapshot},
};
use shikifsrs_runtime::{
    Result, code_patches::OwnedCodePatches, memory::CurrentProcess, profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

/// Hashes the immutable fixed EXE once; each read freshly checks loaded evidence and graph.
/// Construct on the engine observation thread; no owned native reference or lifetime lock.
pub struct CurrentCharacterUnits {
    file: OwnedVerifiedFile,
    base: u64,
    patches: Option<Arc<OwnedCodePatches>>,
    plan: CharacterUnitReadPlan,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentCharacterUnits {
    /// Validate selected bounds before file or process inspection. Calls no game entry.
    /// # Safety
    /// Caller retains the exact module and selected character/controller/module/effect/
    /// PARAM/visual/ragdoll/storage graph in an independently permitted observation phase.
    /// No teardown, reuse, concurrent producers, unload or unknown code patches overlap;
    /// caller advances epoch after scope/reuse and guarantees any owned patch semantics.
    /// Repeated copied fields are not an atomic snapshot or native storage lease.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        patches: Option<Arc<OwnedCodePatches>>,
        plan: CharacterUnitReadPlan,
    ) -> Result<Self> {
        plan.validate()?;
        let file = OwnedVerifiedFile::verify(bytes, WW_171)?;
        Ww171File::from_verified(file.borrow()?)?.bind_objects_with_patches(
            &CurrentProcess,
            base,
            patches.as_deref(),
        )?;
        Ok(Self {
            file,
            base,
            patches,
            plan,
            _thread: PhantomData,
        })
    }
    pub fn plan(&self) -> &CharacterUnitReadPlan {
        &self.plan
    }
    pub fn unit(&self, target: CharacterIdentity) -> Result<Option<CharacterUnitSnapshot>> {
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .character_unit_snapshot(target, &self.plan)
    }
    pub fn primary(&self, epoch: u64) -> Result<Option<CharacterUnitSnapshot>> {
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .primary_unit_snapshot(epoch, &self.plan)
    }
}
