//! Experimental effect operations and foreground input cleanup through one interface.
use crate::character_sets::CharacterIdentity;
use shikifsrs_runtime::{
    Error, Result,
    input::{Input, InputFrame},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EffectId(i32);
impl EffectId {
    pub fn new(id: i32) -> Result<Self> {
        if id <= 0 {
            return Err(Error::InvalidProfile("effect id must be positive"));
        }
        Ok(Self(id))
    }
    pub fn value(self) -> i32 {
        self.0
    }
}

#[cfg(test)]
mod tests;

/// The native convenience wrapper does not expose whether gameplay accepted the effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectApplication {
    Submitted,
}

/// # Safety
/// Implementations must use the caller's live target within the permitted engine phase.
/// A returned application receipt means the attempt returned, not gameplay acceptance.
/// Removal addresses the first matching native node; false means no match.
/// Errors may leave an attempted effect present, so callers must retain cleanup state.
pub unsafe trait EffectEngine {
    /// # Safety
    /// The target, engine state, module and heap must remain live in this mutation phase.
    unsafe fn apply(
        &self,
        target: CharacterIdentity,
        effect: EffectId,
        force: bool,
    ) -> Result<EffectApplication>;
    /// # Safety
    /// Same scheduling/lifetime requirements as apply, including any native unlink callbacks.
    unsafe fn remove(&self, target: CharacterIdentity, effect: EffectId) -> Result<bool>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputEffect {
    pub input: Input,
    pub effect: EffectId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectOperation {
    Apply,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectFailure {
    pub effect: EffectId,
    pub operation: EffectOperation,
    pub error: Error,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectFrameReport {
    pub submitted: Vec<EffectId>,
    /// Cleanup completed even if the native node was already absent.
    pub cleared: Vec<EffectId>,
    pub failures: Vec<EffectFailure>,
    pub input_error: Option<Error>,
    pub cleanup_pending: Vec<EffectId>,
    /// The previous target became unavailable or changed; no calls use a stale address.
    /// This is an unresolved cleanup outcome, not a claim that the old effects were removed.
    pub abandoned: Vec<EffectId>,
}

pub struct InputEffectController {
    bindings: BTreeMap<EffectId, BTreeSet<Input>>,
    target: Option<CharacterIdentity>,
    active: BTreeSet<EffectId>,
}

impl InputEffectController {
    pub fn new(bindings: impl IntoIterator<Item = InputEffect>) -> Result<Self> {
        let mut map = BTreeMap::<EffectId, BTreeSet<Input>>::new();
        for (count, binding) in bindings.into_iter().enumerate() {
            if count >= 256 {
                return Err(Error::InvalidProfile("too many input effect bindings"));
            }
            map.entry(binding.effect).or_default().insert(binding.input);
        }
        Ok(Self {
            bindings: map,
            target: None,
            active: BTreeSet::new(),
        })
    }

    pub fn is_idle(&self) -> bool {
        self.active.is_empty()
    }

    /// Held inputs reapply every frame; focus loss and polling failure trigger cleanup.
    /// Failed cleanup stays pending and is retried. Aliases of an effect act as logical OR.
    /// Missing/changed targets abandon their state explicitly without dereferencing old objects.
    /// World, full opaque identifier and caller epoch are compared even at the same address.
    /// Identical-identity ABA and object lifetime still require the caller's engine guarantees.
    /// # Safety
    /// Caller supplies the live current character in a mutation phase; this controller owns
    /// the configured effect IDs exclusively. A target change does not imply an old-object lease.
    pub unsafe fn update(
        &mut self,
        engine: &impl EffectEngine,
        target: Option<CharacterIdentity>,
        frame: Result<InputFrame>,
    ) -> EffectFrameReport {
        let mut report = EffectFrameReport::default();
        let target = target
            .filter(|target| target.address != 0 && target.world_address != 0 && target.epoch != 0);
        if target != self.target {
            report.abandoned.extend(self.active.iter().copied());
            self.active.clear();
            self.target = target;
        }
        let down = match frame {
            Ok(frame) => {
                report.input_error = frame.gamepad_error;
                if frame.focused && target.is_some() {
                    frame.down
                } else {
                    BTreeSet::new()
                }
            }
            Err(error) => {
                report.input_error = Some(error);
                BTreeSet::new()
            }
        };
        let Some(target) = target else {
            return report;
        };
        let desired: BTreeSet<_> = self
            .bindings
            .iter()
            .filter(|(_, inputs)| !inputs.is_disjoint(&down))
            .map(|(id, _)| *id)
            .collect();
        for effect in self
            .active
            .difference(&desired)
            .copied()
            .collect::<Vec<_>>()
        {
            // SAFETY: Only the caller-provided current live target is used; phase obligations above.
            match unsafe { engine.remove(target, effect) } {
                Ok(_) => {
                    self.active.remove(&effect);
                    report.cleared.push(effect);
                }
                Err(error) => report.failures.push(EffectFailure {
                    effect,
                    operation: EffectOperation::Remove,
                    error,
                }),
            }
        }
        for effect in desired {
            // Retain attempted IDs even on errors; a backend may have partially applied them.
            self.active.insert(effect);
            // SAFETY: Same current target and mutation phase; consumer requires force=true while held.
            match unsafe { engine.apply(target, effect, true) } {
                Ok(_) => report.submitted.push(effect),
                Err(error) => report.failures.push(EffectFailure {
                    effect,
                    operation: EffectOperation::Apply,
                    error,
                }),
            }
        }
        report.cleanup_pending.extend(
            self.active
                .iter()
                .filter(|id| !self.bindings[*id].iter().any(|input| down.contains(input)))
                .copied(),
        );
        report
    }

    /// Explicitly release this controller's effects. Drop never calls into the game.
    /// Retry while cleanup_pending is nonempty; abandoning a target is separately reported.
    /// # Safety
    /// Same target, engine phase, exclusive effect IDs and lifetime requirements as update.
    pub unsafe fn stop(
        &mut self,
        engine: &impl EffectEngine,
        target: Option<CharacterIdentity>,
    ) -> EffectFrameReport {
        // SAFETY: The caller provides the same live target/phase guarantees as update.
        unsafe {
            self.update(
                engine,
                target,
                Ok(InputFrame {
                    focused: false,
                    down: BTreeSet::new(),
                    gamepad: None,
                    gamepad_error: None,
                }),
            )
        }
    }
}
