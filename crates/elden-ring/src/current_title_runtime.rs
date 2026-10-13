//! Thread-confined current-process title input reader, with no hook/native dispatch.
use crate::{
    WW_171, Ww171File, character_sets::CharacterIdentity, task_group_state::validate_record_limit,
    title_runtime::TitleRuntimeObservation,
};
use shikifsrs_runtime::{
    Result, code_patches::OwnedCodePatches, memory::CurrentProcess, profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};
pub struct CurrentTitleRuntime {
    file: OwnedVerifiedFile,
    base: u64,
    patches: Option<Arc<OwnedCodePatches>>,
    record_limit: usize,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentTitleRuntime {
    /// # Safety
    /// Caller holds the resident module/SDK, live selected frontend/menu/helper/world/player
    /// and scheduler record allocations in a permitted observation phase. No teardown,
    /// address reuse/ABA, concurrent producers or unknown code patches overlap operations.
    /// Any owned patch's transitive contracts hold. Copies acquire no native storage lease.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        record_limit: usize,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        validate_record_limit(record_limit)?;
        let file = OwnedVerifiedFile::verify(bytes, WW_171)?;
        Ww171File::from_verified(file.borrow()?)?.bind_title_with_patches(
            &CurrentProcess,
            base,
            patches.as_deref(),
        )?;
        Ok(Self {
            file,
            base,
            patches,
            record_limit,
            _thread: PhantomData,
        })
    }
    /// Independent current primary read. A changing menu/pulse/group cannot invalidate
    /// the consumer's separate primary-presence stop policy. Same bind/lifetime obligations.
    pub fn primary_player_identity(&self, epoch: u64) -> Result<Option<CharacterIdentity>> {
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .primary_player_identity(epoch)
    }

    pub fn snapshot(&self, epoch: u64) -> Result<TitleRuntimeObservation> {
        if epoch == 0 {
            return Err(shikifsrs_runtime::Error::InvalidProfile(
                "zero title observation epoch",
            ));
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_title_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .snapshot(epoch, self.record_limit)
    }
}
