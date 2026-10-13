//! Experimental native getters. They may update pose buffers and dirty flags.
//! ABI tests execute only original Rust entries; game-runtime acceptance is pending.
use crate::{
    WW_171, Ww171File,
    character_sets::CharacterIdentity,
    character_visuals::{CharacterVisualLink, CharacterVisualObservation},
    pose_buffers::{
        PoseBuffer, PoseRecordBits, PoseRecordCopy, PoseStateObservation, PoseWordStore,
        commit_pose_words, pose_word_plan,
    },
    pose_transforms::{AffineTransformBits, MatrixTransformBits},
    skeleton_mapping::{MAPPING_EVIDENCE, SkeletonMappingObservation},
};
use shikifsrs_runtime::code_patches::OwnedCodePatches;
use shikifsrs_runtime::{
    Error, Result,
    memory::{AddressSpace, CurrentProcess},
    profile::{CodeRange, OwnedVerifiedFile},
};
use std::{ffi::c_void, marker::PhantomData, rc::Rc, sync::Arc};

const SKELETON_BINDING_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0xb45540,
        length: 0xcf,
    },
    CodeRange {
        rva: 0xb45610,
        length: 0x15f,
    },
    CodeRange {
        rva: 0xb45770,
        length: 0xe3,
    },
    CodeRange {
        rva: 0xb4b950,
        length: 0x9,
    },
    CodeRange {
        rva: 0x1654e90,
        length: 0x8f,
    },
    CodeRange {
        rva: 0x16549b0,
        length: 0x2e,
    },
    CodeRange {
        rva: 0x16549e0,
        length: 0x2e,
    },
];
const AFFINE_ENTRY: CodeRange = SKELETON_BINDING_EVIDENCE[0];
const AFFINE_RANGE_ENTRY: CodeRange = SKELETON_BINDING_EVIDENCE[1];
const MATRIX_ENTRY: CodeRange = SKELETON_BINDING_EVIDENCE[2];
const NATIVE_INDEX_LIMIT: u32 = 65536;
const EMBEDDED_POSE_OFFSET: u32 = 0x48;
type PoseEntry = unsafe extern "system" fn(*mut c_void);
type PoseEditEntry = unsafe extern "system" fn(*mut c_void) -> *mut c_void;

#[derive(Debug)]
pub struct NativePoseEditCall {
    pub source: PoseStateObservation,
    pub buffer: PoseBuffer,
    pub returned_array_header: u64,
    pub postflight: Result<PoseStateObservation>,
}
#[derive(Debug)]
pub struct NativePoseRecordWrite {
    /// None means no words changed and no native cache/flag operation occurred.
    pub preparation: Option<NativePoseEditCall>,
    pub source: PoseRecordCopy,
    pub replacement: Vec<PoseRecordBits>,
    pub stores: Vec<PoseWordStore>,
    /// Error retains actual native preparation and every performed word store.
    pub postflight: Result<PoseRecordCopy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativePoseOperation {
    SynchronizeModel,
    ResetReference,
}
/// Native array/cache writes occurred even when the copied postflight fails.
#[derive(Debug)]
pub struct NativePoseCall {
    pub source: PoseStateObservation,
    pub operation: NativePoseOperation,
    pub postflight: Result<PoseStateObservation>,
}
type AffineEntry = unsafe extern "system" fn(*mut c_void, *mut AffineTransformBits, u32) -> u8;
type AffineRangeEntry =
    unsafe extern "system" fn(*mut c_void, *mut AffineTransformBits, u32, u32) -> u32;
type MatrixEntry = unsafe extern "system" fn(*mut c_void, *mut MatrixTransformBits, u32) -> u8;

/// Preserve that a native call occurred even when post-call identity validation fails.
/// A false getter result does not imply no pose update or other native effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeBoneCall<T> {
    pub target: CharacterIdentity,
    pub mapper: CharacterVisualObservation,
    pub called: bool,
    pub value: T,
    /// Rechecked mapping/link/identity. Err does not retract the call or undo its effects.
    pub postflight: Result<()>,
}

/// Native range count denotes processed entries, not individual fallback success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeAffineBatch {
    pub first_model_object: u32,
    pub requested: u32,
    pub reported_count: u32,
    /// Only the reported prefix, bounded by allocated capacity. Each entry was zero-initialized.
    /// Native fallback getters can fail without the range getter exposing per-entry status;
    /// an output may therefore retain initial zero bits. Do not classify every entry as valid.
    pub records: Vec<AffineTransformBits>,
}

/// Confined to the binding thread. Owns file evidence, not any game reference or lock.
pub struct CurrentSkeletonTransforms {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}

impl CurrentSkeletonTransforms {
    /// Check exact file, loaded profile and independently proved entry spans. No native call.
    /// # Safety
    /// base is the exact current ER module. Caller retains this module/SDK and establishes a
    /// permitted exclusive pose-update phase. For each call, mapper, both weak-reference
    /// targets/control blocks, embedded pose, skeleton, parent indices, flags, local/model
    /// buffers, mapping table/indices and fallback locations must be initialized, stable and
    /// independently held alive with native-required alignment/extent. Every selected mapped
    /// index and parent must fit the real buffers; native dirty-update destinations are writable.
    /// No producer, solver, callback, teardown, patch, address reuse, alias or unload overlaps.
    /// The SDK does not obtain these guards. Observations/epoch are not a substitute.
    pub unsafe fn bind(bytes: Arc<[u8]>, base: u64, epoch: u64) -> Result<Self> {
        // SAFETY: Same caller obligations; no code replacements accepted by default.
        unsafe { Self::bind_with_patches(bytes, base, epoch, None) }
    }

    /// Bind with an explicitly owned code replacement manifest.
    /// # Safety
    /// All bind obligations apply, plus the manifest owner's ABI, semantic, lifetime and
    /// quiescence guarantees for every registered hook throughout all operations.
    pub unsafe fn bind_with_patches(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero skeleton transform epoch"));
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

    /// Revoke old character selections after an independently established lifetime transition.
    /// # Safety
    /// Caller reestablishes bind's module, lifetime, initialization and exclusive-phase contract.
    pub unsafe fn advance_epoch(&mut self, epoch: u64) -> Result<()> {
        if epoch <= self.epoch {
            return Err(Error::InvalidProfile(
                "skeleton transform epoch must advance",
            ));
        }
        self.epoch = epoch;
        Ok(())
    }

    pub fn observe_pose_state(
        &self,
        target: CharacterIdentity,
        link: CharacterVisualLink,
    ) -> Result<Option<PoseStateObservation>> {
        if target.epoch != self.epoch {
            return Err(Error::InvalidProfile("stale pose state epoch"));
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .pose_state(target, link, NATIVE_INDEX_LIMIT)
    }

    /// Native array edit preparation, not freshness synchronization or a write lease.
    /// Local editing marks every model record dirty; model editing marks every local record dirty.
    /// # Safety
    /// Bind's complete pinned, initialized, writable, exclusive pose graph is held. Caller proves
    /// the selected source space is already current and understands that native per-bone flags
    /// are overwritten for the whole skeleton. No solver/producer/alias/teardown/ABA overlaps.
    /// Keep those guards during subsequent writes; returned header is not an owned allocation.
    pub unsafe fn prepare_pose_edit(
        &self,
        expected: PoseStateObservation,
        buffer: PoseBuffer,
    ) -> Result<NativePoseEditCall> {
        pose_preflight(
            self.observe_pose_state(expected.local.visual.character, expected.local.visual.link)?,
            expected,
            self.epoch,
        )?;
        let index = match buffer {
            PoseBuffer::Primary => 5,
            PoseBuffer::Secondary => 6,
        };
        let argument = expected
            .local
            .visual
            .address
            .checked_add(u64::from(EMBEDDED_POSE_OFFSET))
            .ok_or(Error::AddressOverflow)?;
        let header_offset = match buffer {
            PoseBuffer::Primary => 8,
            PoseBuffer::Secondary => 0x18,
        };
        let expected_header = argument
            .checked_add(header_offset)
            .ok_or(Error::AddressOverflow)?;
        let entry_address = self.entry(SKELETON_BINDING_EVIDENCE[index])?;
        // SAFETY: Complete owned spans establish RCX embedded pose and RAX array-header pointer.
        let entry: PoseEditEntry = unsafe { std::mem::transmute(entry_address) };
        // SAFETY: Caller holds initialized writable native flags/cache and full engine graph.
        let returned_array_header = unsafe { invoke_pose_edit(entry, argument) };
        let postflight = self
            .observe_pose_state(expected.local.visual.character, expected.local.visual.link)
            .and_then(|after| {
                let after = after.ok_or(Error::NativeOperationRejected(
                    "pose absent after edit preparation",
                ))?;
                let cache = match buffer {
                    PoseBuffer::Primary => [0, 1],
                    PoseBuffer::Secondary => [1, 0],
                };
                if returned_array_header != expected_header
                    || !expected.same_storage(after)
                    || after.cache_bytes != cache
                {
                    return Err(Error::NativeOperationRejected(
                        "pose edit preparation postflight differs",
                    ));
                }
                Ok(after)
            });
        Ok(NativePoseEditCall {
            source: expected,
            buffer,
            returned_array_header,
            postflight,
        })
    }

    /// Replace a bounded raw record range through native edit preparation, retaining actual stores.
    /// Explicit restoration uses the same API with a fresh expected copy and the original baseline.
    /// # Safety
    /// prepare_pose_edit's entire graph/phase contract holds until all stores/postflight finish.
    /// Caller proves every selected source and replacement word is initialized and numerically
    /// appropriate, and uniquely owns writable record extents. No overlap, ABA or engine update
    /// occurs between record-boundary checks. This is sequential, not atomic; failed receipts
    /// must be reconciled without automatic retry, rollback, Drop or implicit cache restoration.
    pub unsafe fn compare_pose_records(
        &self,
        state: PoseStateObservation,
        expected: PoseRecordCopy,
        replacement: &[PoseRecordBits],
    ) -> Result<NativePoseRecordWrite> {
        let plan = pose_word_plan(&expected, replacement)?;
        let source = match expected.source.buffer {
            PoseBuffer::Primary => state.local,
            PoseBuffer::Secondary => state.model,
        };
        if source != expected.source {
            return Err(Error::NativeOperationRejected(
                "pose copy does not match state",
            ));
        }
        let target = source.visual.character;
        let link = source.visual.link;
        pose_preflight(self.observe_pose_state(target, link)?, state, self.epoch)?;
        let memory = CurrentProcess;
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let count = expected.records.len() as u32;
        // SAFETY: Caller holds initialized exact source range and the exclusive pose graph.
        let fresh = unsafe {
            objects.copy_pose_records(
                target,
                link,
                source.buffer,
                expected.first,
                count,
                NATIVE_INDEX_LIMIT,
            )
        };
        if fresh? != Some(expected.clone()) {
            return Err(Error::NativeOperationRejected("pose records superseded"));
        }
        let mut receipt = NativePoseRecordWrite {
            preparation: None,
            source: expected.clone(),
            replacement: replacement.to_vec(),
            stores: Vec::new(),
            postflight: Ok(expected),
        };
        if plan.is_empty() {
            return Ok(receipt);
        }
        // SAFETY: Same complete initialized/exclusive contract; native flags are changed once.
        let preparation = unsafe { self.prepare_pose_edit(state, source.buffer)? };
        let prepared = preparation.postflight.clone();
        receipt.preparation = Some(preparation);
        let prepared = match prepared {
            Ok(value) => value,
            Err(error) => {
                receipt.postflight = Err(error);
                return Ok(receipt);
            }
        };
        let (stores, status) = commit_pose_words(
            plan,
            |address| memory.read_u32(address),
            |address, bits| {
                // SAFETY: Caller uniquely pins this initialized aligned writable four-byte extent.
                unsafe { (address as *mut u32).write_volatile(bits) };
            },
            || {
                if objects.pose_state(target, link, NATIVE_INDEX_LIMIT)? != Some(prepared) {
                    return Err(Error::NativeOperationRejected(
                        "pose storage superseded during write",
                    ));
                }
                Ok(())
            },
        );
        receipt.stores = stores;
        receipt.postflight = status.and_then(|()| {
            // SAFETY: Successful storage checks plus caller's uninterrupted exclusive initialized graph.
            let after = unsafe {
                objects.copy_pose_records(
                    target,
                    link,
                    source.buffer,
                    receipt.source.first,
                    count,
                    NATIVE_INDEX_LIMIT,
                )
            }?
            .ok_or(Error::NativeOperationRejected(
                "pose absent after records write",
            ))?;
            if after.source != receipt.source.source || after.records != replacement {
                return Err(Error::NativeOperationRejected(
                    "pose records postflight differs",
                ));
            }
            Ok(after)
        });
        Ok(receipt)
    }

    /// Native b4b950 forwards importer+48 to16551c0 and consumes model dirty flags.
    /// # Safety
    /// Caller pins initialized importer/skeleton/parents/flags/local/model arrays, proves
    /// every parent is -1 or an earlier valid bone, maintains the native quaternion/numerical
    /// invariants, supplies writable model/cache/flags and an exclusive permitted phase.
    /// No solver/producer/hook/teardown/ABA/unload overlaps. Bind's graph contracts apply.
    /// A metadata postflight does not certify computed transforms or visual acceptance.
    pub unsafe fn synchronize_model_pose(
        &self,
        expected: PoseStateObservation,
    ) -> Result<NativePoseCall> {
        // SAFETY: Caller guarantees the complete initialized native graph and exclusive phase.
        unsafe { self.change_pose(expected, NativePoseOperation::SynchronizeModel) }
    }

    /// Reset local records from the native reference skeleton and mark model records dirty.
    /// This discards current local animation; it is not restoration of an SDK baseline.
    /// # Safety
    /// Caller meets synchronize_model_pose's graph/phase contract, independently proves the
    /// skeleton+40 reference allocation contains bone_count initialized aligned48-byte records,
    /// and that native skeleton+68 floating-slot count fits the importer+88 destination.
    /// Local/flags/cache/floating-slot destinations are writable. No pending overlay/animation
    /// ownership is silently discharged; retain every earlier obligation and this receipt.
    pub unsafe fn reset_reference_pose(
        &self,
        expected: PoseStateObservation,
    ) -> Result<NativePoseCall> {
        // SAFETY: Caller additionally guarantees reference and floating-slot extents/ownership.
        unsafe { self.change_pose(expected, NativePoseOperation::ResetReference) }
    }

    unsafe fn change_pose(
        &self,
        expected: PoseStateObservation,
        operation: NativePoseOperation,
    ) -> Result<NativePoseCall> {
        let target = expected.local.visual.character;
        let link = expected.local.visual.link;
        let current = self.observe_pose_state(target, link)?;
        pose_preflight(current, expected, self.epoch)?;
        let index = match operation {
            NativePoseOperation::SynchronizeModel => 3,
            NativePoseOperation::ResetReference => 4,
        };
        let entry_address = self.entry(SKELETON_BINDING_EVIDENCE[index])?;
        let importer = expected.local.visual.address;
        // Compute argument before invocation so address failure never retracts a performed call.
        let argument = pose_argument(importer, operation)?;
        // SAFETY: Complete owned spans prove void RCX importer/embedded-pose ABI respectively.
        let entry: PoseEntry = unsafe { std::mem::transmute(entry_address) };
        // SAFETY: Independent caller graph/phase obligations plus exact entry and fresh metadata.
        unsafe { invoke_pose(entry, argument) };
        let postflight = self.observe_pose_state(target, link).and_then(|after| {
            let after = after.ok_or(Error::NativeOperationRejected(
                "pose absent after native call",
            ))?;
            if !expected.same_storage(after) {
                return Err(Error::NativeOperationRejected(
                    "pose storage changed after native call",
                ));
            }
            Ok(after)
        });
        Ok(NativePoseCall {
            source: expected,
            operation,
            postflight,
        })
    }

    /// Native 48-byte affine getter, including native negative-index fallback.
    /// # Safety
    /// Caller fulfills bind for the entire call and postflight, especially referenced buffer
    /// initialization/extent and permission for lazy model-pose updates. Not a read-only call.
    /// Outer Err occurs before invocation; postflight Err retains the performed call receipt.
    pub unsafe fn affine(
        &self,
        target: CharacterIdentity,
        model_object: u32,
    ) -> Result<NativeBoneCall<Option<AffineTransformBits>>> {
        let mapping = self.prepare(target)?;
        if model_object >= mapping.native_indices.len() as u32 {
            return Ok(receipt(mapping, false, None, Ok(())));
        }
        let entry_address = self.entry(AFFINE_ENTRY)?;
        // SAFETY: Full native entry proves rcx mapper, rdx aligned48-byte output, r8d index,
        // low-byte boolean result. Caller supplies all transitive engine lifetime/phase obligations.
        let entry: AffineEntry = unsafe { std::mem::transmute(entry_address) };
        // SAFETY: Exact typed mapper and entry validated; caller guarantees live native graph.
        let value = unsafe { invoke_affine(entry, mapping.visual.address, model_object) };
        let postflight = self.postflight(&mapping);
        Ok(receipt(mapping, true, value, postflight))
    }

    /// Native 64-byte column matrix getter. Numerical globals are used by the native engine.
    /// # Safety
    /// Same obligations and partial-effect boundary as affine, with aligned64-byte output.
    pub unsafe fn matrix(
        &self,
        target: CharacterIdentity,
        model_object: u32,
    ) -> Result<NativeBoneCall<Option<MatrixTransformBits>>> {
        let mapping = self.prepare(target)?;
        if model_object >= mapping.native_indices.len() as u32 {
            return Ok(receipt(mapping, false, None, Ok(())));
        }
        let entry_address = self.entry(MATRIX_ENTRY)?;
        // SAFETY: Independently proved rcx mapper, rdx aligned64-byte output, r8d index, AL result.
        let entry: MatrixEntry = unsafe { std::mem::transmute(entry_address) };
        // SAFETY: Preflight plus caller's independent native-graph lifetime/update contract.
        let value = unsafe { invoke_matrix(entry, mapping.visual.address, model_object) };
        let postflight = self.postflight(&mapping);
        Ok(receipt(mapping, true, value, postflight))
    }

    /// Native affine range getter. Empty/outside selections skip the native entry.
    /// # Safety
    /// Same as affine for every selected entry. The range path may execute fallback virtual
    /// getters, synthesize defaults, and update model pose. reported_count is not a success count.
    pub unsafe fn affine_range(
        &self,
        target: CharacterIdentity,
        first_model_object: u32,
        requested: u32,
    ) -> Result<NativeBoneCall<NativeAffineBatch>> {
        let mapping = self.prepare(target)?;
        let capacity = batch_capacity(
            mapping.native_indices.len() as u32,
            first_model_object,
            requested,
        )?;
        if capacity == 0 {
            let batch = NativeAffineBatch {
                first_model_object,
                requested,
                reported_count: 0,
                records: Vec::new(),
            };
            return Ok(receipt(mapping, false, batch, Ok(())));
        }
        let entry_address = self.entry(AFFINE_RANGE_ENTRY)?;
        // SAFETY: Independently proved rcx mapper, rdx aligned record array, r8d requested,
        // r9d first-model index and EAX processed count; each record stride30 is16-aligned.
        let entry: AffineRangeEntry = unsafe { std::mem::transmute(entry_address) };
        // SAFETY: Stable mapping capacity bounds native writes; caller holds all source graph.
        let (batch, count_check) = unsafe {
            invoke_range(
                entry,
                mapping.visual.address,
                first_model_object,
                requested,
                capacity,
            )
        };
        let postflight = count_check.and_then(|()| self.postflight(&mapping));
        Ok(receipt(mapping, true, batch, postflight))
    }

    fn prepare(&self, target: CharacterIdentity) -> Result<SkeletonMappingObservation> {
        if target.epoch != self.epoch {
            return Err(Error::InvalidProfile("stale skeleton transform epoch"));
        }
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        for range in MAPPING_EVIDENCE {
            objects.module.code_address(*range)?;
        }
        objects
            .skeleton_mapping(target, NATIVE_INDEX_LIMIT)?
            .ok_or(Error::NativeOperationRejected(
                "skeleton mapper or mapping table is absent",
            ))
    }
    fn entry(&self, range: CodeRange) -> Result<usize> {
        let file = self.file.borrow()?;
        let memory = CurrentProcess;
        let module =
            file.bind_with_patches(&memory, self.base, &[range], self.patches.as_deref())?;
        usize::try_from(module.code_address(range)?).map_err(|_| Error::AddressOverflow)
    }
    fn postflight(&self, expected: &SkeletonMappingObservation) -> Result<()> {
        if self.prepare(expected.visual.character)? != *expected {
            return Err(Error::NativeOperationRejected(
                "skeleton mapping or character changed after native call",
            ));
        }
        Ok(())
    }
}

fn pose_argument(importer: u64, operation: NativePoseOperation) -> Result<u64> {
    match operation {
        NativePoseOperation::SynchronizeModel => Ok(importer),
        NativePoseOperation::ResetReference => importer
            .checked_add(u64::from(EMBEDDED_POSE_OFFSET))
            .ok_or(Error::AddressOverflow),
    }
}
unsafe fn invoke_pose(entry: PoseEntry, argument: u64) {
    // SAFETY: Caller supplies exact native ABI/argument and the complete writable live graph.
    unsafe { entry(argument as *mut c_void) };
}
unsafe fn invoke_pose_edit(entry: PoseEditEntry, argument: u64) -> u64 {
    // SAFETY: Caller supplies exact entry/argument and the complete initialized writable graph.
    unsafe { entry(argument as *mut c_void) as u64 }
}
fn pose_preflight(
    current: Option<PoseStateObservation>,
    expected: PoseStateObservation,
    epoch: u64,
) -> Result<()> {
    if epoch == 0 || expected.local.visual.character.epoch != epoch || current != Some(expected) {
        return Err(Error::NativeOperationRejected("pose source superseded"));
    }
    Ok(())
}

fn receipt<T>(
    mapping: SkeletonMappingObservation,
    called: bool,
    value: T,
    postflight: Result<()>,
) -> NativeBoneCall<T> {
    NativeBoneCall {
        target: mapping.visual.character,
        mapper: mapping.visual,
        called,
        value,
        postflight,
    }
}
fn batch_capacity(mapping_count: u32, first: u32, requested: u32) -> Result<u32> {
    if mapping_count > NATIVE_INDEX_LIMIT || requested > NATIVE_INDEX_LIMIT {
        return Err(Error::InvalidProfile(
            "native affine range exceeds SDK limit",
        ));
    }
    Ok(requested.min(mapping_count.saturating_sub(first)))
}
unsafe fn invoke_affine(
    entry: AffineEntry,
    mapper: u64,
    index: u32,
) -> Option<AffineTransformBits> {
    let mut output = AffineTransformBits([[0; 4]; 3]);
    // SAFETY: Caller provides the exact entry/native graph; local output is live/aligned/initialized.
    (unsafe { entry(mapper as *mut c_void, &mut output, index) } != 0).then_some(output)
}
unsafe fn invoke_matrix(
    entry: MatrixEntry,
    mapper: u64,
    index: u32,
) -> Option<MatrixTransformBits> {
    let mut output = MatrixTransformBits([[0; 4]; 4]);
    // SAFETY: Caller provides exact entry/native graph; output is live/aligned/initialized64 bytes.
    (unsafe { entry(mapper as *mut c_void, &mut output, index) } != 0).then_some(output)
}
unsafe fn invoke_range(
    entry: AffineRangeEntry,
    mapper: u64,
    first: u32,
    requested: u32,
    capacity: u32,
) -> (NativeAffineBatch, Result<()>) {
    let mut records = vec![AffineTransformBits([[0; 4]; 3]); capacity as usize];
    // SAFETY: Caller proves stable native mapping count clamps requested to allocated capacity;
    // Vec records have16-aligned base/48-byte stride and all output bytes are initialized.
    let reported_count = unsafe {
        entry(
            mapper as *mut c_void,
            records.as_mut_ptr(),
            requested,
            first,
        )
    };
    let count_check = if reported_count <= capacity {
        Ok(())
    } else {
        Err(Error::NativeOperationRejected(
            "native affine count exceeds allocated output",
        ))
    };
    records.truncate(reported_count.min(capacity) as usize);
    (
        NativeAffineBatch {
            first_model_object: first,
            requested,
            reported_count,
            records,
        },
        count_check,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{character_visuals::CharacterVisualLink, objects::ObjectKind};
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn pose_dispatch_uses_distinct_importer_and_embedded_arguments_on_owned_storage() {
        #[repr(C, align(16))]
        struct Owned([u8; 160]);
        unsafe extern "system" fn update(argument: *mut c_void) {
            // SAFETY: Test passes the unique initialized Owned base, covering byte80.
            let bytes = unsafe { &mut *(argument as *mut Owned) };
            assert_eq!(bytes.0[0], 0x33);
            bytes.0[0x80] = 1;
        }
        unsafe extern "system" fn reset(argument: *mut c_void) {
            // SAFETY: Test passes unique embedded offset48; initialized storage covers38/39.
            let bytes = argument as *mut u8;
            // SAFETY: The same unique owned storage covers all three tested byte locations.
            unsafe {
                assert_eq!(*bytes, 0x44);
                *bytes.add(0x38) = 0;
                *bytes.add(0x39) = 1;
            }
        }
        unsafe extern "system" fn local_edit(argument: *mut c_void) -> *mut c_void {
            let bytes = argument as *mut u8;
            // SAFETY: Original test owns the embedded pose through its cache bytes and header.
            unsafe {
                *bytes.add(0x38) = 0;
                *bytes.add(0x39) = 1;
                bytes.add(8).cast()
            }
        }
        unsafe extern "system" fn model_edit(argument: *mut c_void) -> *mut c_void {
            let bytes = argument as *mut u8;
            // SAFETY: Original test owns the embedded pose through its cache bytes and header.
            unsafe {
                *bytes.add(0x38) = 1;
                *bytes.add(0x39) = 0;
                bytes.add(0x18).cast()
            }
        }
        let mut storage = Owned([0x55; 160]);
        storage.0[0] = 0x33;
        storage.0[0x48] = 0x44;
        let importer = (&mut storage as *mut Owned) as u64;
        // SAFETY: Only original test entries and this live unique owned array are used.
        unsafe {
            invoke_pose(
                update,
                pose_argument(importer, NativePoseOperation::SynchronizeModel).unwrap(),
            );
            invoke_pose(
                reset,
                pose_argument(importer, NativePoseOperation::ResetReference).unwrap(),
            );
        }
        assert_eq!(&storage.0[0x80..0x82], &[0, 1]);
        assert_eq!((storage.0[0x7f], storage.0[0x82]), (0x55, 0x55));
        // SAFETY: Original entries receive this unique live test allocation's embedded pose.
        unsafe {
            assert_eq!(
                invoke_pose_edit(local_edit, importer + 0x48),
                importer + 0x50
            );
            assert_eq!(&storage.0[0x80..0x82], &[0, 1]);
            assert_eq!(
                invoke_pose_edit(model_edit, importer + 0x48),
                importer + 0x60
            );
            assert_eq!(&storage.0[0x80..0x82], &[1, 0]);
        }
        assert!(pose_argument(u64::MAX - 0x47, NativePoseOperation::ResetReference).is_err());
    }

    struct Mock {
        mapper: usize,
        index: u32,
        requested: u32,
        return_count: u32,
        single_ok: u8,
        calls: AtomicU32,
    }
    unsafe extern "system" fn affine_entry(
        mapper: *mut c_void,
        output: *mut AffineTransformBits,
        index: u32,
    ) -> u8 {
        // SAFETY: Tests pass a uniquely borrowed live Mock as the mapper and aligned initialized output.
        let state = unsafe { &*(mapper as *const Mock) };
        assert_eq!(mapper as usize, state.mapper);
        assert_eq!(index, state.index);
        assert!((output as usize).is_multiple_of(16));
        // SAFETY: Invoke helper owns exactly one writable output; no other alias used in test.
        unsafe {
            assert_eq!((*output).0, [[0; 4]; 3]);
            (*output).0 = [[0x7fc01234; 4]; 3];
        }
        state.calls.fetch_add(1, Ordering::Relaxed);
        state.single_ok
    }
    unsafe extern "system" fn matrix_entry(
        mapper: *mut c_void,
        output: *mut MatrixTransformBits,
        index: u32,
    ) -> u8 {
        // SAFETY: Original test entry receives live Mock and one unique output.
        let state = unsafe { &*(mapper as *const Mock) };
        assert_eq!(index, state.index);
        assert!((output as usize).is_multiple_of(16));
        // SAFETY: Caller supplies a writable64-byte initialized output owned by invoke_matrix.
        unsafe {
            assert_eq!((*output).0, [[0; 4]; 4]);
            (*output).0 = [[0x80000000; 4]; 4];
        }
        state.calls.fetch_add(1, Ordering::Relaxed);
        state.single_ok
    }
    unsafe extern "system" fn range_entry(
        mapper: *mut c_void,
        output: *mut AffineTransformBits,
        requested: u32,
        first: u32,
    ) -> u32 {
        // SAFETY: Original entry receives live Mock and preallocated initialized records.
        let state = unsafe { &*(mapper as *const Mock) };
        assert_eq!((first, requested), (state.index, state.requested));
        for i in 0..state.return_count.min(3) {
            // SAFETY: Test caller allocates three records, independent of the artificial return count.
            let at = unsafe { output.add(i as usize) };
            assert!((at as usize).is_multiple_of(16));
            // SAFETY: Each record is unique/live/initialized. Deliberately retain middle zero record
            // to demonstrate that processed count does not imply fallback success.
            unsafe {
                assert_eq!((*at).0, [[0; 4]; 3]);
                if i != 1 {
                    (*at).0 = [[i + 1; 4]; 3];
                }
            }
        }
        state.calls.fetch_add(1, Ordering::Relaxed);
        state.return_count
    }
    fn state(ok: u8) -> Mock {
        Mock {
            mapper: 0,
            index: 17,
            requested: 5,
            return_count: 3,
            single_ok: ok,
            calls: AtomicU32::new(0),
        }
    }
    fn mapping() -> SkeletonMappingObservation {
        SkeletonMappingObservation {
            visual: CharacterVisualObservation {
                character: CharacterIdentity {
                    address: 0x1000,
                    world_address: 0x2000,
                    native_identifier: 0x123456789abcdef0,
                    epoch: 9,
                },
                link: CharacterVisualLink::SkeletonMapper,
                kind: ObjectKind::SkeletonMapper,
                address: 0x3000,
            },
            table_address: 0x4000,
            indices_address: 0x5000,
            native_indices: vec![1, -1, 2],
        }
    }

    #[test]
    fn affine_abi_preserves_output_bits_and_low_byte_result_without_game_execution() {
        for ok in [0, 1, 0x80] {
            let mut mock = state(ok);
            mock.mapper = &mock as *const Mock as usize;
            // SAFETY: Original Rust entry, owned Mock and helper-owned initialized output only.
            let output = unsafe { invoke_affine(affine_entry, mock.mapper as u64, mock.index) };
            assert_eq!(
                output,
                if ok == 0 {
                    None
                } else {
                    Some(AffineTransformBits([[0x7fc01234; 4]; 3]))
                }
            );
            assert_eq!(mock.calls.load(Ordering::Relaxed), 1);
        }
    }
    #[test]
    fn matrix_abi_has_independent64_byte_aligned_output_and_retains_false_call_effect() {
        for ok in [0, 1] {
            let mut mock = state(ok);
            mock.mapper = &mock as *const Mock as usize;
            // SAFETY: Self-authored mock and owned local output; no native game entry called.
            let output = unsafe { invoke_matrix(matrix_entry, mock.mapper as u64, mock.index) };
            assert_eq!(
                output,
                if ok == 0 {
                    None
                } else {
                    Some(MatrixTransformBits([[0x80000000; 4]; 4]))
                }
            );
            assert_eq!(mock.calls.load(Ordering::Relaxed), 1);
        }
    }
    #[test]
    fn range_abi_passes_requested_before_first_clamps_storage_and_retains_zero_fallback() {
        let mut mock = state(1);
        mock.mapper = &mock as *const Mock as usize;
        // SAFETY: Mock writes at most three owned records; argument order validated independently.
        let (batch, checked) = unsafe { invoke_range(range_entry, mock.mapper as u64, 17, 5, 3) };
        assert_eq!(
            (
                batch.first_model_object,
                batch.requested,
                batch.reported_count
            ),
            (17, 5, 3)
        );
        assert_eq!(
            batch.records,
            [
                AffineTransformBits([[1; 4]; 3]),
                AffineTransformBits([[0; 4]; 3]),
                AffineTransformBits([[3; 4]; 3])
            ]
        );
        assert_eq!(checked, Ok(()));
        assert_eq!(mock.calls.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn range_count_errors_retain_receipt_and_zero_return_removes_unprocessed_storage() {
        for count in [0, 2, 4] {
            let mut mock = state(1);
            mock.return_count = count;
            mock.mapper = &mock as *const Mock as usize;
            // SAFETY: Mock always bounds writes to three owned records, even for invalid return4.
            let (batch, check) = unsafe { invoke_range(range_entry, mock.mapper as u64, 17, 5, 3) };
            assert_eq!(batch.reported_count, count);
            assert_eq!(batch.records.len(), count.min(3) as usize);
            assert_eq!(check.is_ok(), count <= 3);
            assert_eq!(mock.calls.load(Ordering::Relaxed), 1);
        }
    }
    #[test]
    fn range_plans_handle_zero_outside_large_first_and_limit_without_wrapping() {
        for (count, first, requested, expected) in [
            (8, 3, 5, 5),
            (8, 7, 5, 1),
            (8, 8, 5, 0),
            (8, u32::MAX, 5, 0),
            (8, 0, 0, 0),
            (0, 0, 5, 0),
            (65536, 0, 65536, 65536),
        ] {
            assert_eq!(batch_capacity(count, first, requested).unwrap(), expected);
        }
        assert!(batch_capacity(65537, 0, 1).is_err());
        assert!(batch_capacity(8, 0, 65537).is_err());
    }
    #[test]
    fn receipts_keep_native_attempt_output_and_original_identity_on_postflight_failure() {
        let expected = mapping();
        let call = receipt(
            expected.clone(),
            true,
            Some(AffineTransformBits([[7; 4]; 3])),
            Err(Error::NativeOperationRejected("changed")),
        );
        assert!(call.called);
        assert_eq!(call.target, expected.visual.character);
        assert_eq!(call.mapper, expected.visual);
        assert_eq!(call.value, Some(AffineTransformBits([[7; 4]; 3])));
        assert!(call.postflight.is_err());
        let skipped = receipt(expected, false, None::<AffineTransformBits>, Ok(()));
        assert!(!skipped.called);
        assert_eq!(skipped.value, None);
        assert!(skipped.postflight.is_ok());
    }
}
