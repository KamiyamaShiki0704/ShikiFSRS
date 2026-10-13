//! Thread-confined explicit stores in original summon-manager storage; no native call.
use crate::{
    WW_171, Ww171File,
    buddy_groups::{
        BuddyControlObservation, BuddyControlWord, BuddyControlWrite, commit_control, control_plan,
    },
};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};
pub struct CurrentBuddyControl {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentBuddyControl {
    /// # Safety
    /// Pin the exact loaded image/patch handlers and complete live world/manager/set graph.
    /// Supply a known allocation epoch; no engine references are acquired.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero buddy control epoch"));
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
    pub fn observe(&self) -> Result<Option<BuddyControlObservation>> {
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .buddy_control(self.epoch)
    }
    /// # Safety
    /// Retain the complete initialized graph and original writable selected four bytes in an
    /// exclusive permitted engine phase, without updates/teardown/reuse/aliases. Storage success
    /// does not prove summon success. Failed postflight retains the actual store; no retry,
    /// rollback or Drop cleanup. Caller preserves pending intent if dispatch unwinds.
    pub unsafe fn compare_word(
        &self,
        source: BuddyControlObservation,
        field: BuddyControlWord,
        replacement_bits: u32,
    ) -> Result<BuddyControlWrite> {
        if source.module_base != self.base || source.epoch != self.epoch {
            return Err(Error::NativeOperationRejected(
                "foreign buddy control scope",
            ));
        }
        let plan = control_plan(self.observe()?, source, field, replacement_bits)?;
        control_plan(self.observe()?, source, field, replacement_bits)?;
        Ok(commit_control(
            plan,
            |destination, bits| {
                // SAFETY: Caller retains this exact initialized exclusive writable native word.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        bits.to_le_bytes().as_ptr(),
                        destination as *mut u8,
                        4,
                    );
                }
            },
            || self.observe(),
        ))
    }
}
