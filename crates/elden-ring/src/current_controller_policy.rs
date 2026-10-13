//! Explicit conditional stores for copied controller policy; no native calls or Drop actions.
use crate::{
    WW_171, Ww171File,
    character_sets::CharacterIdentity,
    controller_policy::{
        ControllerPolicyField, ControllerPolicyObservation, ControllerPolicyWrite, commit_policy,
        policy_plan, policy_restore_plan, store_policy,
    },
};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};
pub struct CurrentControllerPolicy {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentControllerPolicy {
    /// Change the native request consumed by the lock target record filter.
    /// # Safety
    /// compare_mask's full graph/exclusive phase contract holds. This does not clear an
    /// existing camera target or prove immediate exclusion; native selection runs later.
    pub unsafe fn set_lock_target_suppressed(
        &self,
        expected: ControllerPolicyObservation,
        suppressed: bool,
    ) -> Result<ControllerPolicyWrite> {
        // SAFETY: Caller supplies the complete compare_mask contract for this exact word.
        unsafe {
            self.compare_mask(
                expected,
                ControllerPolicyField::ModifierWord10,
                4,
                u32::from(suppressed) << 2,
            )
        }
    }
    /// Change only the independently proved native event-death mark.
    /// # Safety
    /// compare_mask's full graph, phase, lifetime and writable-storage contract holds.
    /// Clearing this mark does not run native revival/reset or reset dependent modules;
    /// setting it does not dispatch death, rewards, effects or removal. Reconcile failed
    /// postflight before any later step, retaining this actual store obligation.
    pub unsafe fn set_event_dead_marked(
        &self,
        expected: ControllerPolicyObservation,
        marked: bool,
    ) -> Result<ControllerPolicyWrite> {
        // SAFETY: Caller establishes the complete compare_mask contract.
        unsafe {
            self.compare_mask(
                expected,
                ControllerPolicyField::ModifierWord1c,
                1,
                u32::from(marked),
            )
        }
    }
    /// # Safety
    /// Caller independently pins character/controller/modifier/full native graph, supplies true
    /// nonzero epoch, exclusive permitted phase and exact writable field extent. Excludes aliases,
    /// native updates/reset, callbacks, teardown, ABA, hooks and unload. Copies acquire no guards.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero controller policy epoch"));
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
    ) -> Result<Option<ControllerPolicyObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .controller_policy(target)
    }
    /// # Safety
    /// Bind's full phase/lifetime/exclusivity obligations hold. Caller independently establishes
    /// desired bit semantics and ownership. Store success does not prove hit protection/action
    /// suppression. Reconcile a failed postflight before further writes; no automatic retry.
    pub unsafe fn compare_mask(
        &self,
        expected: ControllerPolicyObservation,
        field: ControllerPolicyField,
        mask: u32,
        bits: u32,
    ) -> Result<ControllerPolicyWrite> {
        let plan = policy_plan(
            self.observe(expected.controller.character)?,
            expected,
            field,
            mask,
            bits,
        )?;
        Ok(commit_policy(
            plan,
            |address, width, value| {
                // SAFETY: Caller independently supplies exact exclusive pinned writable field/phase.
                unsafe { store_policy(address, width, value) };
            },
            || {
                self.observe(expected.controller.character)?
                    .ok_or(Error::InvalidProfile(
                        "controller policy unavailable after store",
                    ))
            },
        ))
    }
    /// Explicitly restore still-owned masked bits while preserving current other bits/fields.
    /// # Safety
    /// Same complete graph lifetime/phase/no-ABA obligations as compare_mask. No Drop restoration.
    pub unsafe fn restore_mask(
        &self,
        applied: &ControllerPolicyWrite,
    ) -> Result<ControllerPolicyWrite> {
        let plan =
            policy_restore_plan(self.observe(applied.source.controller.character)?, applied)?;
        Ok(commit_policy(
            plan,
            |address, width, value| {
                // SAFETY: Caller independently holds original live exact storage exclusively.
                unsafe { store_policy(address, width, value) };
            },
            || {
                self.observe(applied.source.controller.character)?
                    .ok_or(Error::InvalidProfile(
                        "controller policy unavailable after restore",
                    ))
            },
        ))
    }
}
