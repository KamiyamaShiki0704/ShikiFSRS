//! Thread-confined summon announcement marker adapter; no string/resource ownership.
use crate::{
    WW_171, Ww171File,
    summon_hud::{SummonHudObservation, SummonHudWrite, commit_suppression, suppression_plan},
};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

pub struct CurrentSummonHud {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentSummonHud {
    /// # Safety
    /// Caller identifies the loaded image and retains code, handlers and observed frontend
    /// allocations through every operation. Epoch denotes a known allocation lifetime.
    /// Observations do not acquire native references or prove an exclusive engine phase.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero summon HUD lifetime epoch"));
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
    pub fn observe(&self) -> Result<Option<SummonHudObservation>> {
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .summon_hud(self.epoch)
    }
    /// Replace native announcement markers with their independently proved reset values.
    /// # Safety
    /// Caller proves exclusive permitted HUD phase, live initialized writable extents and
    /// no aliases, engine updates, teardown or reuse through preflight/stores/postflight.
    /// Up to 18 stores are not atomic. Strings/list links remain native. No destructor,
    /// resource release or automatic retry/rollback occurs, including postflight failure.
    /// Marker reset alone does not prove rendered HUD suppression or queue removal.
    pub unsafe fn compare_suppress(
        &self,
        expected: SummonHudObservation,
    ) -> Result<SummonHudWrite> {
        if expected.module_base != self.base || expected.epoch != self.epoch {
            return Err(Error::InvalidProfile("foreign summon HUD scope"));
        }
        let plan = suppression_plan(self.observe()?, expected)?;
        Ok(commit_suppression(
            plan,
            |address, bytes| {
                // SAFETY: Caller retains exclusive initialized writable extents for each store.
                unsafe {
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len());
                }
            },
            || self.observe(),
        ))
    }
    /// # Safety
    /// Caller proves a new allocation lifetime under bind's obligations.
    pub unsafe fn advance_epoch(&mut self, epoch: u64) -> Result<()> {
        if epoch <= self.epoch {
            return Err(Error::InvalidProfile("summon HUD epoch must advance"));
        }
        self.epoch = epoch;
        Ok(())
    }
}
