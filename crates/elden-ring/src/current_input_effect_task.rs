//! Current ER backend for the FrameBegin input/effect pipeline. Never polls at bind.
use crate::{
    WW_171, Ww171File,
    character_sets::CharacterIdentity,
    current_effects::CurrentEffectEngine,
    effects::{EffectApplication, EffectEngine, EffectId},
    input_effect_task::InputEffectTaskBackend,
    objects::{GameObjectCatalog, OBJECT_EVIDENCE},
};
use shikifsrs_runtime::{
    Error, Result,
    code_patches::OwnedCodePatches,
    input::{ForegroundWindow, GamepadState, InputSource, VirtualKey, WindowsInput},
    memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// A requested generation is admitted only by the next backend target poll. No input or game call.
#[derive(Clone)]
pub struct CurrentInputEffectEpochControl {
    requested: Arc<AtomicU64>,
}
impl CurrentInputEffectEpochControl {
    /// A standalone numeric control acquires no native lifetime or authority.
    pub fn new(epoch: u64) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero input effect epoch"));
        }
        Ok(Self {
            requested: Arc::new(AtomicU64::new(epoch)),
        })
    }
    pub fn requested_epoch(&self) -> u64 {
        self.requested.load(Ordering::Acquire)
    }
    /// Publish a known later epoch; equal requests are no-ops, zero/regression are rejected.
    /// # Safety
    /// Caller independently establishes this generation's native lifetime and serializes the
    /// transition against all current effect callbacks/engine work. The old target must not be
    /// reclaimed while a frame still uses it. The next FrameBegin's full graph/phase/PARAM/heap
    /// obligations hold; unresolved old effects need external reconciliation. This numeric request
    /// supplies no scene-change detector, pin, callback drain, teardown barrier or cleanup proof.
    pub unsafe fn request_epoch(&self, epoch: u64) -> Result<bool> {
        let mut previous = self.requested_epoch();
        loop {
            if epoch_transition(previous, epoch)?.is_none() {
                return Ok(false);
            }
            match self.requested.compare_exchange_weak(
                previous,
                epoch,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(true),
                Err(current) => previous = current,
            }
        }
    }
}
fn epoch_transition(current: u64, requested: u64) -> Result<Option<u64>> {
    if current == 0 || requested < current {
        return Err(Error::InvalidProfile("input effect epoch cannot regress"));
    }
    Ok((requested != current).then_some(requested))
}

pub struct CurrentInputEffectBackend {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    epochs: CurrentInputEffectEpochControl,
    effects: CurrentEffectEngine,
    input: WindowsInput,
}
impl CurrentInputEffectBackend {
    /// # Safety
    /// CurrentEffectEngine bind obligations hold for every eventual callback: permitted
    /// FrameBegin mutation phase, fresh pinned target/PARAM/allocator/fullgraph, no aliases,
    /// teardown/ABA, native readers racing these operations or code/DLL unload. Caller-provided
    /// epoch stays valid until an independently known later generation is requested through
    /// epoch_control; the caller serializes actual lifetime transitions against every callback.
    /// Binder verifies fixed file/loaded evidence, invokes no game function or input sampler.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        effect_limit: usize,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero input pipeline epoch"));
        }
        let file = OwnedVerifiedFile::verify(Arc::clone(&bytes), WW_171)?;
        let view = Ww171File::from_verified(file.borrow()?)?;
        view.bind_objects_with_patches(&CurrentProcess, base, patches.as_deref())?;
        // SAFETY: Caller supplies the same native graph/phase/epoch/residency contract.
        let effects = unsafe {
            CurrentEffectEngine::bind_with_patches(
                bytes,
                base,
                effect_limit,
                epoch,
                patches.clone(),
            )
        }?;
        Ok(Self {
            file,
            patches,
            base,
            epoch,
            epochs: CurrentInputEffectEpochControl::new(epoch)?,
            effects,
            input: WindowsInput,
        })
    }
    pub fn epoch_control(&self) -> CurrentInputEffectEpochControl {
        self.epochs.clone()
    }
}
impl InputSource for CurrentInputEffectBackend {
    fn foreground(&self) -> Result<Option<ForegroundWindow>> {
        self.input.foreground()
    }
    fn key_state(&self, key: VirtualKey) -> Result<i16> {
        self.input.key_state(key)
    }
    fn gamepad(&self, index: u8) -> Result<Option<GamepadState>> {
        self.input.gamepad(index)
    }
}
// SAFETY: Operations forward the checked effect adapter under its explicit unsafe caller contract.
unsafe impl EffectEngine for CurrentInputEffectBackend {
    unsafe fn apply(
        &self,
        target: CharacterIdentity,
        effect: EffectId,
        force: bool,
    ) -> Result<EffectApplication> {
        // SAFETY: Pipeline creator supplies all per-frame target/phase/native graph guarantees.
        unsafe { self.effects.apply(target, effect, force) }
    }
    unsafe fn remove(&self, target: CharacterIdentity, effect: EffectId) -> Result<bool> {
        // SAFETY: Same contract includes effect unlink callbacks and original target identity.
        unsafe { self.effects.remove(target, effect) }
    }
}
impl InputEffectTaskBackend for CurrentInputEffectBackend {
    fn target(&mut self) -> Result<Option<CharacterIdentity>> {
        if let Some(epoch) = epoch_transition(self.epoch, self.epochs.requested_epoch())? {
            // SAFETY: Bind plus the unsafe epoch requester establish this new generation's complete
            // native lifetime/phase. Exclusive backend callback access excludes overlapping effects.
            unsafe { self.effects.advance_epoch(epoch) }?;
            self.epoch = epoch;
        }
        // Immutable file identity/RTTI was proved at bind. Per-frame checks retain loaded
        // constructors and fresh full world/player identity without rescanning RTTI/type lists.
        let file = self.file.borrow()?;
        let memory = CurrentProcess;
        let module =
            file.bind_with_patches(&memory, self.base, OBJECT_EVIDENCE, self.patches.as_deref())?;
        GameObjectCatalog { module }.primary_player_identity(self.epoch)
    }
}

#[cfg(test)]
mod epoch_tests {
    use super::*;
    #[test]
    fn requested_epoch_is_monotonic_shared_and_admitted_without_implicit_cleanup_or_overflow() {
        assert!(CurrentInputEffectEpochControl::new(0).is_err());
        let control = CurrentInputEffectEpochControl::new(7).unwrap();
        let clone = control.clone();
        for invalid in [0, 1, 6] {
            // SAFETY: Standalone original numeric fixture; no native graph or callback attached.
            assert!(unsafe { control.request_epoch(invalid) }.is_err());
            assert_eq!(clone.requested_epoch(), 7);
        }
        // SAFETY: Original numeric-only epoch fixture, no object mutation or lifetime claim.
        assert!(!unsafe { control.request_epoch(7) }.unwrap());
        // SAFETY: Same original fixture, only numeric publication occurs.
        assert!(unsafe { clone.request_epoch(12) }.unwrap());
        assert_eq!(control.requested_epoch(), 12);
        assert_eq!(
            epoch_transition(7, control.requested_epoch()).unwrap(),
            Some(12)
        );
        assert_eq!(
            epoch_transition(12, control.requested_epoch()).unwrap(),
            None
        );
        assert!(epoch_transition(13, control.requested_epoch()).is_err());
        assert!(epoch_transition(0, 0).is_err());
        // SAFETY: Final original numeric fixture generation; no addition or wraparound is performed.
        assert!(unsafe { control.request_epoch(u64::MAX) }.unwrap());
        // SAFETY: Repeated final generation is a numeric no-op in this fixture.
        assert!(!unsafe { clone.request_epoch(u64::MAX) }.unwrap());
        // SAFETY: Regression must reject before any publication in this fixture.
        assert!(unsafe { clone.request_epoch(12) }.is_err());
        assert_eq!(
            epoch_transition(12, control.requested_epoch()).unwrap(),
            Some(u64::MAX)
        );
    }
}
