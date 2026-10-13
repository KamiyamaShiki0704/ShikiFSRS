//! Current-process condition adapter; no audio calls, polling thread or file I/O.
use crate::{
    WW_171, Ww171File, character_equipment::WeaponSelectionMode,
    sound_conditions::ConditionBankMapping,
};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{collections::BTreeSet, marker::PhantomData, rc::Rc, sync::Arc};
#[derive(Debug, Clone)]
pub struct SoundConditionReadConfig {
    pub selection: WeaponSelectionMode,
    pub player_limit: usize,
    pub row_limit: usize,
    pub effect_limit: usize,
    pub weapon_mappings: Vec<ConditionBankMapping>,
    pub effect_mappings: Vec<ConditionBankMapping>,
}
impl Default for SoundConditionReadConfig {
    fn default() -> Self {
        Self {
            selection: WeaponSelectionMode::AutoTwoHand,
            player_limit: 128,
            row_limit: 65536,
            effect_limit: 65536,
            weapon_mappings: Vec::new(),
            effect_mappings: Vec::new(),
        }
    }
}
impl SoundConditionReadConfig {
    pub(crate) fn validate(&self) -> Result<()> {
        if [self.player_limit, self.row_limit, self.effect_limit]
            .iter()
            .any(|limit| !(1..=65536).contains(limit))
            || self.weapon_mappings.len() > 65536
            || self.effect_mappings.len() > 65536
        {
            return Err(Error::InvalidProfile(
                "invalid current sound condition bounds",
            ));
        }
        Ok(())
    }
}
/// A thread-confined reader suitable for the bank task's callback-created observer.
/// Immutable file proof is hashed once; each observation freshly binds native ranges,
/// copies all selected player conditions twice and retains unknown/missing distinctions.
pub struct CurrentSoundConditions {
    file: OwnedVerifiedFile,
    base: u64,
    patches: Option<Arc<OwnedCodePatches>>,
    config: SoundConditionReadConfig,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentSoundConditions {
    /// Verification/readonly binding only. No IsInitialized or bank operation.
    /// # Safety
    /// Caller retains the exact module, all player/effect/equipment/PARAM graph,
    /// permitted observation phase and original storage, excluding teardown,
    /// concurrent producers and reuse. Epoch advances on scope/object reuse.
    /// Code, identity and repeated copies do not supply a native lifetime lease.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        patches: Option<Arc<OwnedCodePatches>>,
        config: SoundConditionReadConfig,
    ) -> Result<Self> {
        config.validate()?;
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
            config,
            _thread: PhantomData,
        })
    }
    /// None means absent world, Some(empty) means a fully observed world with no
    /// matching demands. Read/type/epoch/change errors never become empty success.
    pub fn targets(&self, epoch: u64) -> Result<Option<BTreeSet<u32>>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero sound condition epoch"));
        }
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let objects =
            view.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?;
        let copy = objects.world_sound_conditions(
            epoch,
            self.config.selection,
            self.config.player_limit,
            self.config.row_limit,
            self.config.effect_limit,
        )?;
        copy.map(|copy| {
            copy.bank_targets(&self.config.weapon_mappings, &self.config.effect_mappings)
        })
        .transpose()
    }
}
