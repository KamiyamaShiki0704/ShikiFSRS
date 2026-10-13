//! Cached file identity for existing copied object readers. No native calls or game leases.
use crate::{WW_171, Ww171File, objects::GameObjectCatalog};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

/// Thread confined. Results contain copied values; they cannot borrow the temporary catalog.
pub struct CurrentObjects {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentObjects {
    /// Verify the file before reading the supplied module; no engine function is invoked.
    /// # Safety
    /// Caller identifies this process's ER module, retains module/SDK/hook residency,
    /// supplies a permitted read phase and lifetime of every traversed initialized allocation,
    /// and advances the epoch after possible teardown/reuse. Every selected reader's bounds,
    /// schema and graph obligations still apply. Copies do not pin engine objects or prevent ABA.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero object reader epoch"));
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

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Reuse the verified owned file and strict loaded-code checks for selected readers.
    /// This exposes the catalog's copied read interfaces, not writable engine references.
    pub fn observe<R>(
        &self,
        read: impl FnOnce(&GameObjectCatalog<'_, '_, CurrentProcess>, u64) -> Result<R>,
    ) -> Result<R> {
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        read(&objects, self.epoch)
    }

    /// Revoke old copied identities without touching their former allocations.
    /// # Safety
    /// Caller identifies the new lifetime scope and satisfies bind's full contract for it.
    /// This is not a native reference-release or callback-quiescence operation.
    pub unsafe fn advance_epoch(&mut self, epoch: u64) -> Result<()> {
        if epoch <= self.epoch {
            return Err(Error::InvalidProfile("object reader epoch must advance"));
        }
        self.epoch = epoch;
        Ok(())
    }
}
