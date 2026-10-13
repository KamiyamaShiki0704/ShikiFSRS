//! Thread-confined sparse frontend HUD observation and explicit suppression stores.
use crate::{
    WW_171, Ww171File,
    friendly_hud::{
        FrontendHudObservation, FrontendHudWrite, FrontendViewLink, HudSelection, commit_hud,
        hud_plan,
    },
};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

pub struct CurrentFrontendHud {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentFrontendHud {
    /// # Safety
    /// Caller identifies loaded image, retains code/handlers and live observed frontend/view
    /// allocations through every operation. Epoch is a known lifetime, not an SDK reference.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero frontend HUD lifetime epoch"));
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
    pub fn observe(&self) -> Result<Option<FrontendHudObservation>> {
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .frontend_hud(self.epoch)
    }
    pub fn view_link(&self, view_address: u64) -> Result<Option<FrontendViewLink>> {
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .frontend_view_link(view_address, self.epoch)
    }
    /// Clear selected gauge markers and optionally the five spirit records/count once.
    /// # Safety
    /// Caller proves exclusive permitted HUD phase, retained initialized writable extents,
    /// known allocation lifetimes and no aliases/engine updates/teardown/reuse through the
    /// full operation. The native view must retain its original frontend-values alias.
    /// Stores are not atomic and rendering may occur later. Actual stores survive failed
    /// postflight; no rollback, retry, string/UI mutation, reference release or native call.
    pub unsafe fn compare_suppress(
        &self,
        expected: FrontendHudObservation,
        selection: HudSelection<'_>,
        suppress_spirits: bool,
    ) -> Result<FrontendHudWrite> {
        if expected.module_base != self.base || expected.epoch != self.epoch {
            return Err(Error::InvalidProfile("foreign frontend HUD scope"));
        }
        let plan = hud_plan(self.observe()?, expected, selection, suppress_spirits)?;
        Ok(commit_hud(
            plan,
            |address, bytes| {
                // SAFETY: Caller retains exclusive initialized writable destination extents.
                unsafe {
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len());
                }
            },
            || self.observe(),
        ))
    }
    /// # Safety
    /// Caller proves a new known allocation lifetime under bind's obligations.
    pub unsafe fn advance_epoch(&mut self, epoch: u64) -> Result<()> {
        if epoch <= self.epoch {
            return Err(Error::InvalidProfile("frontend HUD epoch must advance"));
        }
        self.epoch = epoch;
        Ok(())
    }
}
