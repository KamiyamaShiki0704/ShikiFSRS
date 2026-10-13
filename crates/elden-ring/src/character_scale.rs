//! Sequential scaling of proved controller axes and selected physics scalar fields.
//! Geometry, poses, cloth, collision rebuild and full-body behavior stay separate.
use crate::{
    character_scalars::{
        CharacterReplaceOutcome, CharacterScalar as F, CharacterScalarAccess, CharacterScalarKey,
        CharacterScalarOverlays, CharacterScalarRestoreReport, CharacterScalarSnapshot,
    },
    character_sets::CharacterIdentity,
};
use shikifsrs_runtime::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WeightScalePolicy {
    Unchanged,
    /// Multiply the original signed weight; optional nonnegative floor is consumer policy.
    Multiply {
        floor: Option<f32>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterScalePolicy {
    pub collision_dimensions: bool,
    pub weight: WeightScalePolicy,
}
#[derive(Debug)]
pub struct CharacterScaleStop {
    pub key: CharacterScalarKey,
    /// Conflict/Unavailable or a write error; all preceding Applied receipts remain owned.
    pub outcome: Result<CharacterReplaceOutcome>,
}
#[derive(Debug, Default)]
pub struct CharacterScaleReport {
    pub applied: Vec<CharacterScalarKey>,
    pub unchanged: Vec<CharacterScalarKey>,
    pub stopped: Option<CharacterScaleStop>,
    pub pending_restore: usize,
}
impl CharacterScaleReport {
    /// Only the selected scalar operation completed, not geometry or engine acceptance.
    pub fn complete(&self) -> bool {
        self.stopped.is_none()
    }
}
/// One original character/storage baseline across all updates. No Drop writes.
/// Restore is one-way; callers retain this journal until all pending restores resolve.
#[derive(Debug)]
pub struct CharacterScaleSession {
    baseline: Vec<CharacterScalarSnapshot>,
    policy: CharacterScalePolicy,
    overlays: CharacterScalarOverlays,
    closing: bool,
}
impl CharacterScaleSession {
    /// Copy selected fields twice before retaining a baseline; no stores/native calls.
    /// Unavailable selected fields return None. Requires the backend's stable observation
    /// phase/lifetime contract; copies themselves are not an atomic snapshot or lease.
    pub fn capture<A: CharacterScalarAccess>(
        access: &A,
        target: CharacterIdentity,
        policy: CharacterScalePolicy,
    ) -> Result<Option<Self>> {
        if target.epoch == 0
            || matches!(policy.weight,
            WeightScalePolicy::Multiply { floor: Some(floor) } if !floor.is_finite() || floor < 0.0)
        {
            return Err(Error::InvalidProfile(
                "invalid character scale policy or epoch",
            ));
        }
        let mut fields = vec![F::AxisX, F::AxisY, F::AxisZ];
        if policy.collision_dimensions {
            fields.extend([
                F::CharacterHitHeight,
                F::CharacterHitRadius,
                F::MapHitHeight,
                F::MapHitRadius,
            ]);
        }
        if matches!(policy.weight, WeightScalePolicy::Multiply { .. }) {
            fields.push(F::Weight);
        }
        let mut baseline = Vec::with_capacity(fields.len());
        for field in fields {
            let key = CharacterScalarKey {
                character: target,
                field,
            };
            let Some(copy) = access.observe(key)? else {
                return Ok(None);
            };
            if copy.key != key || !field.valid(copy.bits) {
                return Err(Error::InvalidProfile("invalid character scale baseline"));
            }
            baseline.push(copy);
        }
        for copy in &baseline {
            if access.observe(copy.key)? != Some(*copy) {
                return Err(Error::InvalidProfile("character scale baseline changed"));
            }
        }
        Ok(Some(Self {
            baseline,
            policy,
            overlays: CharacterScalarOverlays::default(),
            closing: false,
        }))
    }
    pub fn baseline(&self) -> &[CharacterScalarSnapshot] {
        &self.baseline
    }
    pub fn pending_restore(&self) -> usize {
        self.overlays.pending()
    }
    pub fn closing(&self) -> bool {
        self.closing
    }
    fn scaled(&self, multiplier: f32) -> Result<Vec<u32>> {
        if !multiplier.is_finite() || multiplier <= 0.0 {
            return Err(Error::InvalidProfile("invalid character scale multiplier"));
        }
        self.baseline
            .iter()
            .map(|original| {
                let value = f32::from_bits(original.bits);
                let mut scaled = value * multiplier;
                if !scaled.is_finite() || (value != 0.0 && scaled == 0.0) {
                    return Err(Error::InvalidProfile("unrepresentable character scale"));
                }
                if original.key.field == F::Weight
                    && let WeightScalePolicy::Multiply { floor: Some(floor) } = self.policy.weight
                {
                    scaled = scaled.max(floor);
                }
                if !original.key.field.valid(scaled.to_bits()) {
                    return Err(Error::InvalidProfile("invalid scaled character field"));
                }
                Ok(scaled.to_bits())
            })
            .collect()
    }
    /// Compute every desired value from the first original, then preflight the whole selection
    /// twice before any store. Interrupted writes retain their actual prefix for retry/restore.
    /// Errors returned directly perform no writes; a stopped report may contain Applied fields.
    /// # Safety
    /// Caller satisfies CharacterScalarAccess's exclusive engine phase, live writable storage,
    /// code/SDK residency and no aliases/reuse/teardown for every read and conditional store.
    /// No collision/cloth/model updates or native lifetime acquisition occur here.
    pub unsafe fn apply<A: CharacterScalarAccess>(
        &mut self,
        access: &A,
        multiplier: f32,
    ) -> Result<CharacterScaleReport> {
        if self.closing {
            return Err(Error::InvalidProfile("character scale session is closing"));
        }
        let desired = self.scaled(multiplier)?;
        let expected: Vec<_> = self
            .baseline
            .iter()
            .map(|baseline| self.overlays.expected(baseline.key).unwrap_or(*baseline))
            .collect();
        for _ in 0..2 {
            for copy in &expected {
                if access.observe(copy.key)? != Some(*copy) {
                    return Err(Error::InvalidProfile("character scale preflight changed"));
                }
            }
        }
        let mut report = CharacterScaleReport::default();
        for ((baseline, expected), bits) in self.baseline.iter().zip(expected).zip(desired) {
            if expected.bits == bits {
                report.unchanged.push(baseline.key);
                continue;
            }
            // SAFETY: Caller retains the full exclusive phase/extent; existing journal retains receipts.
            match unsafe {
                self.overlays
                    .apply_bits(access, baseline.key, bits, Some(*baseline))
            } {
                Ok(CharacterReplaceOutcome::Applied) => report.applied.push(baseline.key),
                outcome => {
                    report.stopped = Some(CharacterScaleStop {
                        key: baseline.key,
                        outcome,
                    });
                    break;
                }
            }
        }
        report.pending_restore = self.overlays.pending();
        Ok(report)
    }
    /// Reverse-order conditional restore; foreign values/storage remain untouched, read/write
    /// failures retain pending obligations. Once requested, this session cannot apply again.
    /// # Safety
    /// Caller supplies the backend's full exclusive phase and lifetime for every restore.
    pub unsafe fn restore<A: CharacterScalarAccess>(
        &mut self,
        access: &A,
    ) -> CharacterScalarRestoreReport {
        self.closing = true;
        // SAFETY: Explicit caller contract, no Drop, automatic retry or replaced-storage write.
        unsafe { self.overlays.restore(access) }
    }
}

#[cfg(test)]
mod tests;
