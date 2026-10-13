//! Experimental current-process scalar access. No runtime schema acceptance is implied.
use crate::{
    WW_171, Ww171File,
    objects::GameObjectCatalog,
    parameter_fields::ScalarType,
    parameter_overlays::{FieldKey, FieldSnapshot, ParameterAccess, ReplaceOutcome, RowIdentity},
    parameters::ParameterSlot,
};
use shikifsrs_runtime::code_patches::OwnedCodePatches;
use shikifsrs_runtime::{Error, Result, memory::CurrentProcess, profile::OwnedVerifiedFile};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

/// A thread-confined unsafe session; it does not acquire an engine lock or pin PARAM resources.
pub struct CurrentParameterAccess {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    limit: usize,
    _thread: PhantomData<Rc<()>>,
}

impl CurrentParameterAccess {
    /// Checks file identity before any current-process memory access. Does not write at bind.
    /// # Safety
    /// Caller identifies the ER module and independently confirms every accessed live field's
    /// schema and writable allocation extent. Caller holds an exclusive engine phase and pins
    /// resources for the session; advance epoch after potential unload/reuse. No concurrent
    /// writers, teardown, unload, code patching or aliasing Rust references may overlap access.
    /// Pinned definitions alone do not satisfy this contract. All interfaces remain experimental.
    pub unsafe fn bind(bytes: Arc<[u8]>, base: u64, epoch: u64, limit: usize) -> Result<Self> {
        // SAFETY: Same caller obligations; no code replacements accepted by default.
        unsafe { Self::bind_with_patches(bytes, base, epoch, limit, None) }
    }

    /// Bind with an explicitly owned code replacement manifest.
    /// # Safety
    /// All bind obligations apply, plus the manifest owner's ABI, semantic, lifetime and
    /// quiescence guarantees for every registered hook throughout all operations.
    pub unsafe fn bind_with_patches(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        limit: usize,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 || !(1..=65535).contains(&limit) {
            return Err(Error::InvalidProfile(
                "invalid parameter session epoch or bound",
            ));
        }
        let file = OwnedVerifiedFile::verify(bytes, WW_171)?;
        Ww171File::from_verified(file.borrow()?)?.object_types()?;
        let view = file.borrow()?;
        Ww171File::from_verified(view)?.bind_objects_with_patches(
            &CurrentProcess,
            base,
            patches.as_deref(),
        )?;
        Ok(Self {
            file,
            patches,
            base,
            epoch,
            limit,
            _thread: PhantomData,
        })
    }
}

// SAFETY: Operations freshly resolve rows and verify copied metadata. The unsafe session
// contract supplies the exclusive access, live schema, writable extent and resource pinning.
unsafe impl ParameterAccess for CurrentParameterAccess {
    fn observe(&self, key: FieldKey) -> Result<Option<FieldSnapshot>> {
        let view = self.file.borrow()?;
        let file = Ww171File::from_verified(view)?;
        let memory = CurrentProcess;
        let catalog: GameObjectCatalog<'_, '_, _> =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let definition = key.field.definition();
        let slot = ParameterSlot::new(definition.slot)?;
        let Some(rows) = catalog.parameter_rows(slot, key.variant, self.limit)? else {
            return Ok(None);
        };
        let Some(copy) = catalog.copy_parameter_row(
            slot,
            key.variant,
            key.row_id,
            definition.offset + definition.scalar.width(),
            self.limit,
        )?
        else {
            return Ok(None);
        };
        if catalog.parameter_rows(slot, key.variant, self.limit)? != Some(rows.clone()) {
            return Err(Error::InvalidProfile(
                "parameter rows changed during field observation",
            ));
        }
        if !rows.rows.contains(&copy.row) {
            return Err(Error::InvalidProfile(
                "parameter copy belongs to another row snapshot",
            ));
        }
        let bytes = &copy.bytes[definition.offset..];
        let bits = match definition.scalar {
            ScalarType::U8 | ScalarType::I8 => u32::from(bytes[0]),
            ScalarType::U16 => {
                u32::from(u16::from_le_bytes(bytes.try_into().expect("field width")))
            }
            ScalarType::U32 | ScalarType::I32 | ScalarType::F32 => {
                u32::from_le_bytes(bytes.try_into().expect("field width"))
            }
        };
        Ok(Some(FieldSnapshot {
            key,
            bits,
            identity: RowIdentity {
                epoch: self.epoch,
                resource: rows.resource_address,
                header: rows.header_address,
                index: rows.native_index_address,
                row: copy.row.address,
            },
        }))
    }

    unsafe fn compare_replace(&self, expected: FieldSnapshot, bits: u32) -> Result<ReplaceOutcome> {
        let definition = expected.key.field.definition();
        if matches!(definition.scalar, ScalarType::U8 | ScalarType::I8) && bits > u8::MAX as u32
            || definition.scalar == ScalarType::U16 && bits > u16::MAX as u32
            || definition.scalar == ScalarType::F32 && !f32::from_bits(bits).is_finite()
        {
            return Err(Error::InvalidProfile("invalid replacement scalar bits"));
        }
        let Some(current) = self.observe(expected.key)? else {
            return Ok(ReplaceOutcome::Unavailable);
        };
        if current.identity != expected.identity {
            return Ok(ReplaceOutcome::Unavailable);
        }
        if current.bits != expected.bits {
            return Ok(ReplaceOutcome::Conflict);
        }
        let address = current
            .identity
            .row
            .checked_add(definition.offset as u64)
            .ok_or(Error::AddressOverflow)?;
        let address = usize::try_from(address).map_err(|_| Error::AddressOverflow)?;
        if address == 0 || !address.is_multiple_of(definition.scalar.width()) {
            return Err(Error::InvalidProfile("unaligned parameter scalar write"));
        }
        // SAFETY: Session caller guarantees exclusive live writable storage of the independently
        // confirmed schema. Checks precede one aligned scalar store; no fallible step follows it.
        unsafe {
            write_scalar(address, definition.scalar, bits);
        }
        Ok(ReplaceOutcome::Applied)
    }
}

unsafe fn write_scalar(address: usize, scalar: ScalarType, bits: u32) {
    match scalar {
        ScalarType::U8 | ScalarType::I8 => {
            // SAFETY: Caller provides an exact live initialized exclusive writable byte.
            unsafe {
                (address as *mut u8).write(bits as u8);
            }
        }
        ScalarType::U16 => {
            // SAFETY: Caller provides a live, writable, aligned two-byte destination.
            unsafe {
                (address as *mut u16).write(bits as u16);
            }
        }
        ScalarType::U32 | ScalarType::I32 | ScalarType::F32 => {
            // SAFETY: Caller provides a live, writable, aligned four-byte destination.
            unsafe {
                (address as *mut u32).write(bits);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scalar_stores_touch_only_the_selected_width_and_keep_raw_bits() {
        let mut storage = [0xa5a5_a5a5u32; 3];
        // SAFETY: Original owned byte at odd offset, exact single-byte extent and no aliases.
        unsafe {
            write_scalar(
                storage.as_mut_ptr().cast::<u8>().add(5) as usize,
                ScalarType::U8,
                0x77,
            );
        }
        assert_eq!(storage, [0xa5a5_a5a5, 0xa5a5_77a5, 0xa5a5_a5a5]);
        // SAFETY: Same exact owned byte; the encoded negative value must not widen the store.
        unsafe {
            write_scalar(
                storage.as_mut_ptr().cast::<u8>().add(5) as usize,
                ScalarType::I8,
                crate::parameter_fields::ScalarValue::I8(-1).bits().unwrap(),
            );
        }
        assert_eq!(storage, [0xa5a5_a5a5, 0xa5a5_ffa5, 0xa5a5_a5a5]);
        storage.fill(0xa5a5_a5a5);
        // SAFETY: Local aligned owned storage, no aliases across the calls.
        unsafe {
            write_scalar(
                storage.as_mut_ptr().add(1) as usize,
                ScalarType::U16,
                0x1122,
            );
        }
        assert_eq!(storage, [0xa5a5_a5a5, 0xa5a5_1122, 0xa5a5_a5a5]);
        // SAFETY: Same local aligned owned scalar storage.
        unsafe {
            write_scalar(
                storage.as_mut_ptr().add(1) as usize,
                ScalarType::F32,
                (-0.0f32).to_bits(),
            );
        }
        assert_eq!(storage, [0xa5a5_a5a5, 0x8000_0000, 0xa5a5_a5a5]);
    }
    #[test]
    fn wrong_file_is_rejected_before_any_process_read_or_write() {
        // SAFETY: Unknown file must fail before the deliberately invalid module address is used.
        let result = unsafe { CurrentParameterAccess::bind(Arc::from([0u8; 2]), u64::MAX, 1, 8) };
        assert!(result.is_err());
    }
}
