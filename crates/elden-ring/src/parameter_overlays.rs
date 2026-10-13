//! Conditional, explicit restoration of experimental parameter overrides.
//! Snapshots carry caller-owned epochs; addresses alone never pin a native row.
use crate::parameter_fields::{ParameterField, ScalarType, ScalarValue};
use shikifsrs_runtime::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldKey {
    pub field: ParameterField,
    pub variant: u32,
    pub row_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowIdentity {
    /// Caller must advance this when resources can unload or addresses can be reused.
    pub epoch: u64,
    pub resource: u64,
    pub header: u64,
    pub index: u64,
    pub row: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSnapshot {
    pub key: FieldKey,
    pub identity: RowIdentity,
    /// Exact little-endian scalar bits, including the sign bit of floating zero.
    pub bits: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceOutcome {
    Applied,
    Conflict,
    Unavailable,
}

/// Backend providing conditional scalar replacement under exclusive engine access.
/// # Safety
/// Implementations must re-resolve the key rather than dereference old snapshot addresses.
/// compare_replace must check identity and exact bits together while no concurrent writer,
/// teardown or reuse is possible. Errors and Conflict/Unavailable perform no writes.
/// Caller proves live schema, allocation extent, writable storage and engine phase; a copied
/// observation, Paramdex definition or epoch number does not provide those guarantees.
pub unsafe trait ParameterAccess {
    fn observe(&self, key: FieldKey) -> Result<Option<FieldSnapshot>>;
    /// # Safety
    /// Caller holds the backend's exclusive phase and resource lifetime for the whole operation.
    unsafe fn compare_replace(&self, expected: FieldSnapshot, bits: u32) -> Result<ReplaceOutcome>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Override {
    original: FieldSnapshot,
    applied: u32,
}

#[derive(Debug, Default)]
pub struct ParameterOverlays {
    entries: Vec<Override>,
}

#[derive(Debug, Default)]
pub struct RestoreReport {
    pub restored: Vec<FieldKey>,
    pub superseded: Vec<FieldKey>,
    pub unavailable: Vec<FieldKey>,
    pub failures: Vec<(FieldKey, Error)>,
    pub pending: usize,
}

impl ParameterOverlays {
    pub fn pending(&self) -> usize {
        self.entries.len()
    }

    /// Apply or update one field. Multiple fields are sequential, not an atomic transaction.
    /// The first original value remains the restoration target across updates.
    /// # Safety
    /// Caller satisfies ParameterAccess's phase/schema/lifetime contract.
    pub unsafe fn apply<A: ParameterAccess>(
        &mut self,
        access: &A,
        key: FieldKey,
        value: ScalarValue,
    ) -> Result<ReplaceOutcome> {
        if value.scalar_type() != key.field.definition().scalar {
            return Err(Error::InvalidProfile("override scalar type mismatch"));
        }
        let bits = value.bits()?;
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.original.key == key)
        {
            let expected = FieldSnapshot {
                bits: entry.applied,
                ..entry.original
            };
            // SAFETY: Forwarded caller contract; the original identity is deliberately retained.
            let outcome = unsafe { access.compare_replace(expected, bits)? };
            if outcome == ReplaceOutcome::Applied {
                entry.applied = bits;
            }
            return Ok(outcome);
        }
        if self.entries.len() == 64 {
            return Err(Error::InvalidProfile("parameter override limit exceeded"));
        }
        let Some(original) = access.observe(key)? else {
            return Ok(ReplaceOutcome::Unavailable);
        };
        if original.key != key {
            return Err(Error::InvalidProfile(
                "parameter backend returned another key",
            ));
        }
        let scalar = key.field.definition().scalar;
        if matches!(scalar, ScalarType::U8 | ScalarType::I8) && original.bits > u8::MAX as u32
            || scalar == ScalarType::U16 && original.bits > u16::MAX as u32
            || scalar == ScalarType::F32 && !f32::from_bits(original.bits).is_finite()
        {
            return Err(Error::InvalidProfile(
                "original parameter scalar cannot be restored",
            ));
        }
        // SAFETY: Forwarded caller contract; backend compares current identity and bits.
        let outcome = unsafe { access.compare_replace(original, bits)? };
        if outcome == ReplaceOutcome::Applied && original.bits != bits {
            self.entries.push(Override {
                original,
                applied: bits,
            });
        }
        Ok(outcome)
    }

    /// Restore in reverse application order. Failed writes remain owned for explicit retry.
    /// Foreign modifications and replaced resources are reported and released without writes.
    /// No Drop implementation calls the backend; losing this owner does not restore anything.
    /// # Safety
    /// Caller satisfies ParameterAccess's exclusive phase and lifetime contract.
    pub unsafe fn restore<A: ParameterAccess>(&mut self, access: &A) -> RestoreReport {
        let mut report = RestoreReport::default();
        for index in (0..self.entries.len()).rev() {
            let entry = self.entries[index];
            let expected = FieldSnapshot {
                bits: entry.applied,
                ..entry.original
            };
            // SAFETY: Forwarded caller contract. This never blindly writes the old row address.
            match unsafe { access.compare_replace(expected, entry.original.bits) } {
                Ok(ReplaceOutcome::Applied) => {
                    report.restored.push(expected.key);
                }
                Ok(ReplaceOutcome::Conflict) => {
                    report.superseded.push(expected.key);
                }
                Ok(ReplaceOutcome::Unavailable) => {
                    report.unavailable.push(expected.key);
                }
                Err(error) => {
                    report.failures.push((expected.key, error));
                    continue;
                }
            }
            self.entries.remove(index);
        }
        report.pending = self.entries.len();
        report
    }
}

#[cfg(test)]
mod tests;
