//! Explicit thread-confined physics request stores, without native calls or Drop actions.
use crate::{
    WW_171, Ww171File,
    character_sets::CharacterIdentity,
    physics_suppression::{
        PhysicsSuppressionObservation, PhysicsSuppressionWrite, commit_suppression, store_request,
        suppression_plan, suppression_restore_plan,
    },
};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

pub struct CurrentPhysicsSuppression {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentPhysicsSuppression {
    /// # Safety
    /// Caller independently pins character/physics module/full native graph, supplies a true
    /// nonzero lifetime epoch, exclusive permitted native phase and exact writable request byte.
    /// Excludes aliases, concurrent engine policy, callbacks, teardown, ABA, hooks and unload.
    /// Typed copies and code validation acquire none of these guarantees.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero physics suppression epoch"));
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
    ) -> Result<Option<PhysicsSuppressionObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .physics_suppression(target)
    }
    /// Store only persistent request 1d3. Effective 1d2 is never written by this adapter.
    /// # Safety
    /// Bind's full lifetime/phase/writable obligations hold. Caller owns this request's meaning
    /// and reconciles a failed postflight before further writes. Nonzero request does not prove
    /// gravity/collision behavior; zero does not guarantee an unsuppressed native result.
    pub unsafe fn compare_request(
        &self,
        expected: PhysicsSuppressionObservation,
        value: u8,
    ) -> Result<PhysicsSuppressionWrite> {
        let plan = suppression_plan(self.observe(expected.module.character)?, expected, value)?;
        Ok(commit_suppression(
            plan,
            |address, value| {
                // SAFETY: Caller independently holds exclusive pinned writable request byte/phase.
                unsafe { store_request(address, value) };
            },
            || {
                self.observe(expected.module.character)?
                    .ok_or(Error::InvalidProfile(
                        "physics suppression unavailable after store",
                    ))
            },
        ))
    }
    /// Restore original request only while full original module identity and owned byte match.
    /// Preserve current engine output/adjacent policy. No retry, native call or Drop restoration.
    /// # Safety
    /// Same full phase/lifetime/exclusivity guarantees as compare_request, including no ABA.
    pub unsafe fn restore_request(
        &self,
        applied: &PhysicsSuppressionWrite,
    ) -> Result<PhysicsSuppressionWrite> {
        let plan =
            suppression_restore_plan(self.observe(applied.source.module.character)?, applied)?;
        Ok(commit_suppression(
            plan,
            |address, value| {
                // SAFETY: Caller holds original live exact request storage exclusively in this phase.
                unsafe { store_request(address, value) };
            },
            || {
                self.observe(applied.source.module.character)?
                    .ok_or(Error::InvalidProfile(
                        "physics suppression unavailable after restore",
                    ))
            },
        ))
    }
}
