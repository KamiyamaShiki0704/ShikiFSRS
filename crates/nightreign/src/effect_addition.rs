//! Experimental original default effect addition, using only value arguments.
//! Native callbacks, eligibility, allocation and notifications remain engine responsibilities.
use crate::{
    NightreignFile, character_effects::SpecialEffectsSnapshot, parameter_fields::effect,
    reader::NightreignReader, roots::RootKind,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
const ADD_DEFAULT: CodeRange = CodeRange {
    rva: 0x3c7fd0,
    length: 199,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectAdditionLimits {
    pub nodes: usize,
    pub parameter_rows: usize,
}
/// Invocation seam for original AMD64 `u8(Character*, i32, u8)` with third argument zero.
/// # Safety
/// Execute only the supplied original entry, preserving its entire native policy path.
/// Memory must represent the same engine. Do not replace it with low-level node insertion
/// or skip native callbacks/notifications. No SDK buffer, heap storage or ownership is
/// transferred. The original wrapper constructs its own defaults. No unwinding or patching;
/// errors may follow mutations. Host supplies all native object/PARAM/heap/global lifetimes.
pub unsafe trait EffectAdditionBackend: AddressSpace {
    /// # Safety
    /// Caller holds a live exclusive native character and offline mutation phase.
    unsafe fn invoke_add_default(&self, entry: u64, character: u64, id: i32) -> Result<bool>;
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectAdditionReceipt {
    pub requested_id: i32,
    pub parameter_address: u64,
    pub before: SpecialEffectsSnapshot,
    /// Native acceptance result; true does not imply a persistent, eligible or unique node.
    pub native_result: Result<bool>,
    /// Observed values or post-call error, retained even when native_result is an error.
    pub after: Result<SpecialEffectsSnapshot>,
}
pub struct EffectAddition<'r, 'f, B> {
    reader: NightreignReader<'r, 'f, B>,
}
impl<'f> NightreignFile<'f> {
    /// Byte/metadata checks only. Binding does not allocate or execute native game code.
    pub fn bind_effect_addition<'r, B: EffectAdditionBackend>(
        &'r self,
        backend: &'r B,
        base: u64,
    ) -> Result<EffectAddition<'r, 'f, B>> {
        let addition = EffectAddition {
            reader: self.bind_reader(backend, base)?,
        };
        addition.check_defaults()?;
        Ok(addition)
    }
}
impl<B: EffectAdditionBackend> EffectAddition<'_, '_, B> {
    /// Default variant-zero PARAM, default native descriptors and force flag zero.
    /// Categories 204/206 with nonzero priority are excluded because native ID tier remapping
    /// can select another row. No customized source, descriptors or force options are exposed.
    /// # Safety
    /// Caller supplies an offline exclusive engine mutation phase and stable full native
    /// object graph, PARAM rows, heap registry, callback and notification subsystems. All
    /// engine/module/SDK residency extends through transitive callbacks and return frames.
    /// Prevent teardown/ABA and patches. Actual scheduling, side effects and runtime schema
    /// acceptance remain experimental. Never automatically retry based on an error receipt.
    pub unsafe fn add_default(
        &self,
        character: u64,
        id: i32,
        limits: EffectAdditionLimits,
    ) -> Result<EffectAdditionReceipt> {
        if id <= 0 {
            return Err(Error::InvalidProfile(
                "NR default effect addition requires a positive ID",
            ));
        }
        let before = self
            .reader
            .special_effects(character, limits.nodes)?
            .ok_or(Error::NativeOperationRejected("NR effect object is absent"))?;
        before.check_owner_links()?;
        let root =
            self.reader
                .root(RootKind::Parameters)?
                .ok_or(Error::NativeOperationRejected(
                    "NR PARAM repository is absent",
                ))?;
        let schema =
            self.reader
                .parameter_schema(root, "SpEffectParam", 0, limits.parameter_rows)?;
        schema.check_definition(effect::STATE_INFO.definition())?;
        let row = self
            .reader
            .copy_parameter_row(
                root,
                "SpEffectParam",
                0,
                id as u32,
                effect::STATE_INFO.definition().packed_size,
                limits.parameter_rows,
            )?
            .ok_or(Error::NativeOperationRejected(
                "NR requested effect PARAM is absent",
            ))?;
        check_default_row(&row.bytes)?;
        let entry = self.reader.module.code_address(ADD_DEFAULT)?;
        self.check_defaults()?;
        if self.reader.root(RootKind::Parameters)? != Some(root)
            || self
                .reader
                .parameter_schema(root, "SpEffectParam", 0, limits.parameter_rows)?
                != schema
            || self.reader.copy_parameter_row(
                root,
                "SpEffectParam",
                0,
                id as u32,
                row.bytes.len(),
                limits.parameter_rows,
            )? != Some(row.clone())
            || self.reader.special_effects(character, limits.nodes)? != Some(before.clone())
        {
            return Err(Error::InvalidProfile("NR default effect preflight changed"));
        }
        // SAFETY: Exact original entry/owner/PARAM/default subset checked. Host supplies
        // native phase, full callback/notification graph residency and original heap ownership.
        let native_result = unsafe {
            self.reader
                .module
                .memory()
                .invoke_add_default(entry, character, id)
        };
        let after = self
            .reader
            .special_effects(character, limits.nodes)
            .and_then(|value| {
                let value = value.ok_or(Error::InvalidProfile(
                    "NR effect object disappeared after addition",
                ))?;
                value.check_owner_links()?;
                if value.effect_address != before.effect_address {
                    return Err(Error::InvalidProfile(
                        "NR effect object replaced after addition",
                    ));
                }
                Ok(value)
            });
        Ok(EffectAdditionReceipt {
            requested_id: id,
            parameter_address: row.row.address,
            before,
            native_result,
            after,
        })
    }
    fn check_defaults(&self) -> Result<()> {
        let image = self.reader.file.file().image();
        for rva in [0x32f3a50, 0x32f37b0, 0x32f3a40] {
            image
                .section_at(rva)
                .filter(|s| !s.is_writable() && !s.is_executable())
                .ok_or(Error::InvalidProfile(
                    "NR default descriptor is not readonly file data",
                ))?;
            let expected = image.read_rva(rva, 16)?;
            let mut actual = [0; 16];
            self.reader.module.memory().read_exact(
                self.reader
                    .module
                    .base()
                    .checked_add(rva as u64)
                    .ok_or(Error::AddressOverflow)?,
                &mut actual,
            )?;
            if actual != expected {
                return Err(Error::ImageMismatch { rva });
            }
        }
        Ok(())
    }
}
fn check_default_row(bytes: &[u8]) -> Result<()> {
    let category = effect::SP_CATEGORY.read_row_bytes(bytes)?;
    let priority = effect::CATEGORY_PRIORITY.read_row_bytes(bytes)?;
    if matches!(category, 204 | 206) && priority != 0 {
        return Err(Error::InvalidProfile(
            "NR native effect tier remapping is outside the default supported subset",
        ));
    }
    Ok(())
}
#[cfg(windows)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CurrentEffectAdditionBackend;
#[cfg(windows)]
impl AddressSpace for CurrentEffectAdditionBackend {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        shikifsrs_runtime::memory::CurrentProcess.read_exact(address, output)
    }
}
#[cfg(windows)]
// SAFETY: Uses the original checked default wrapper; unsafe callers supply all native lifetimes.
unsafe impl EffectAdditionBackend for CurrentEffectAdditionBackend {
    unsafe fn invoke_add_default(&self, entry: u64, character: u64, id: i32) -> Result<bool> {
        type Add = unsafe extern "system" fn(*mut std::ffi::c_void, i32, u8) -> u8;
        // SAFETY: Independently evidenced AMD64 ABI, original default wrapper entry.
        let function: Add = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Caller supplies exclusive live character and full native subsystems; flag zero.
        Ok(unsafe { function(character as *mut std::ffi::c_void, id, 0) } != 0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tiered_categories_need_mapping_but_zero_priority_keeps_original_id() {
        let mut row = vec![0; 976];
        for category in [204i16, 206] {
            row[318..320].copy_from_slice(&category.to_le_bytes());
            row[320] = 0;
            assert!(check_default_row(&row).is_ok());
            row[320] = 1;
            assert!(check_default_row(&row).is_err());
        }
        row[318..320].copy_from_slice(&205i16.to_le_bytes());
        assert!(check_default_row(&row).is_ok());
        assert!(check_default_row(&row[..320]).is_err());
    }
}
