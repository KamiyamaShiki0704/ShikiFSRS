//! Experimental bounded copies of the pose importer's two native record arrays.
use crate::{
    character_sets::CharacterIdentity,
    character_visuals::{CharacterVisualLink, CharacterVisualObservation},
    objects::{GameObjectCatalog, ObjectKind, ObjectObservation},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const POSE_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0xb4b380,
        length: 0x282,
    },
    CodeRange {
        rva: 0x17b9f10,
        length: 0xb4,
    },
];
const PRIMARY_ARRAY_OFFSET: u32 = 0x50;
const SECONDARY_ARRAY_OFFSET: u32 = 0x60;
const RECORD_STRIDE: u32 = 0x30;
const CAPACITY_MASK: u32 = 0x3fff_ffff;
const RECORD_LIMIT: u32 = 65536;
const SKELETON_OFFSET: u32 = 0x48;
const SKELETON_BONE_COUNT_OFFSET: u32 = 0x38;
const PARENT_ARRAY_OFFSET: u32 = 0x20;
const FLAGS_ARRAY_OFFSET: u32 = 0x70;
const CACHE_BYTES_OFFSET: u32 = 0x80;

/// Does not indicate current active source, solver generation or freshness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoseBuffer {
    Primary,
    Secondary,
}

#[cfg(test)]
mod write_tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn source(words: &[[u32; 12]]) -> PoseRecordCopy {
        PoseRecordCopy {
            source: PoseBufferObservation {
                visual: CharacterVisualObservation {
                    character: CharacterIdentity {
                        address: 0x1000,
                        world_address: 0x2000,
                        native_identifier: 7,
                        epoch: 1,
                    },
                    link: CharacterVisualLink::Pose,
                    address: 0x3000,
                    kind: ObjectKind::PoseImporter,
                },
                buffer: PoseBuffer::Primary,
                data_address: 0x4000,
                count: words.len() as u32,
                capacity_and_flags: 0x80000000 | words.len() as u32,
            },
            first: 1,
            records: words[1..3]
                .iter()
                .map(|r| {
                    PoseRecordBits(std::array::from_fn(|b| {
                        std::array::from_fn(|w| r[b * 4 + w])
                    }))
                })
                .collect(),
        }
    }
    #[test]
    fn bounded_pose_words_preserve_neighbors_raw_lanes_noops_and_explicit_baseline_restore() {
        let baseline = [
            [0x7fc01234; 12],
            [0x80000000; 12],
            [0xffffffff; 12],
            [9; 12],
        ];
        let expected = source(&baseline);
        let mut replacement = expected.records.clone();
        replacement[0].0[0][0] = 1.5f32.to_bits();
        replacement[1].0[2][2] = 0.5f32.to_bits();
        let plan = pose_word_plan(&expected, &replacement).unwrap();
        assert_eq!(
            plan.iter().map(|w| (w.record, w.word)).collect::<Vec<_>>(),
            [(1, 0), (2, 10)]
        );
        let storage = RefCell::new(baseline);
        let validations = Cell::new(0);
        let (stores, status) = commit_pose_words(
            plan,
            |a| {
                let i = (a - 0x4000) as usize / 4;
                Ok(storage.borrow()[i / 12][i % 12])
            },
            |a, b| {
                let i = (a - 0x4000) as usize / 4;
                storage.borrow_mut()[i / 12][i % 12] = b;
            },
            || {
                validations.set(validations.get() + 1);
                Ok(())
            },
        );
        status.unwrap();
        assert_eq!(stores.len(), 2);
        assert_eq!(validations.get(), 3);
        let current = source(&*storage.borrow());
        assert_eq!(current.records, replacement);
        assert!(pose_word_plan(&current, &replacement).unwrap().is_empty());
        let plan = pose_word_plan(&current, &expected.records).unwrap();
        let (_, status) = commit_pose_words(
            plan,
            |a| {
                let i = (a - 0x4000) as usize / 4;
                Ok(storage.borrow()[i / 12][i % 12])
            },
            |a, b| {
                let i = (a - 0x4000) as usize / 4;
                storage.borrow_mut()[i / 12][i % 12] = b;
            },
            || Ok(()),
        );
        status.unwrap();
        assert_eq!(*storage.borrow(), baseline);
        assert!(pose_word_plan(&expected, &replacement[..1]).is_err());
        let mut foreign = expected.clone();
        foreign.first = u32::MAX;
        assert!(pose_word_plan(&foreign, &replacement).is_err());
    }
    #[test]
    fn pose_word_failures_keep_the_actual_prefix_and_never_touch_remaining_records() {
        let baseline = [[9; 12]; 4];
        let expected = source(&baseline);
        let mut replacement = expected.records.clone();
        replacement[0].0[0][0] = 1;
        replacement[1].0[0][0] = 2;
        for failure in 0..3 {
            let storage = RefCell::new(baseline);
            let calls = Cell::new(0);
            let writes = Cell::new(0);
            let plan = pose_word_plan(&expected, &replacement).unwrap();
            let (actual, status) = commit_pose_words(
                plan,
                |a| {
                    let i = (a - 0x4000) as usize / 4;
                    if failure == 0 && writes.get() == 1 {
                        return Err(Error::InvalidProfile("original post-store read failure"));
                    }
                    Ok(storage.borrow()[i / 12][i % 12])
                },
                |a, b| {
                    let i = (a - 0x4000) as usize / 4;
                    storage.borrow_mut()[i / 12][i % 12] = b;
                    writes.set(writes.get() + 1);
                },
                || {
                    let count = calls.get();
                    calls.set(count + 1);
                    if (failure == 1 && count == 1) || failure == 2 {
                        return Err(Error::InvalidProfile("original pose graph transition"));
                    }
                    Ok(())
                },
            );
            assert!(status.is_err());
            assert_eq!(actual.len(), if failure == 2 { 0 } else { 1 });
            assert_eq!(storage.borrow()[2], baseline[2]);
            if failure != 2 {
                assert_eq!(storage.borrow()[1][0], 1);
            }
        }
        let storage = RefCell::new(baseline);
        storage.borrow_mut()[1][0] = 77;
        let (actual, status) = commit_pose_words(
            pose_word_plan(&expected, &replacement).unwrap(),
            |a| {
                let i = (a - 0x4000) as usize / 4;
                Ok(storage.borrow()[i / 12][i % 12])
            },
            |_, _| panic!("foreign source must never be written"),
            || Ok(()),
        );
        assert!(status.is_err());
        assert!(actual.is_empty());
        assert_eq!(storage.borrow()[1][0], 77);
    }
}
impl PoseBuffer {
    /// Native array role only. Caller still proves initialization and freshness.
    pub fn space(self) -> crate::pose_transforms::PoseSpace {
        match self {
            Self::Primary => crate::pose_transforms::PoseSpace::Local,
            Self::Secondary => crate::pose_transforms::PoseSpace::Model,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoseBufferObservation {
    pub visual: CharacterVisualObservation,
    pub buffer: PoseBuffer,
    pub data_address: u64,
    pub count: u32,
    /// Native top flag bits preserved; only low30 bits are interpreted as capacity.
    pub capacity_and_flags: u32,
}
impl PoseBufferObservation {
    pub fn capacity(&self) -> u32 {
        self.capacity_and_flags & CAPACITY_MASK
    }
}
/// Three native four-word blocks, copied without asserting initialized transform semantics.
/// Padding, NaN, signed-zero and unknown components are never normalized or rewritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoseRecordBits(pub [[u32; 4]; 3]);
impl PoseRecordBits {
    /// Interpret a copied record's independently proved component order. This pure view
    /// cannot certify the original allocation, initialized bytes or current engine generation.
    pub fn transform(self) -> crate::pose_transforms::PoseTransformBits {
        crate::pose_transforms::PoseTransformBits(self)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoseRecordCopy {
    pub source: PoseBufferObservation,
    pub first: u32,
    pub records: Vec<PoseRecordBits>,
}

/// One actual four-byte store. Multiword records are not atomic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoseWordStore {
    pub address: u64,
    pub record: u32,
    pub word: u8,
    pub before: u32,
    pub after: u32,
}
pub(crate) fn pose_word_plan(
    expected: &PoseRecordCopy,
    replacement: &[PoseRecordBits],
) -> Result<Vec<PoseWordStore>> {
    let count = u32::try_from(expected.records.len()).map_err(|_| Error::AddressOverflow)?;
    let end = expected
        .first
        .checked_add(count)
        .ok_or(Error::AddressOverflow)?;
    if expected.records.len() != replacement.len()
        || end > expected.source.count
        || expected.source.count > RECORD_LIMIT
        || expected.source.count > expected.source.capacity()
        || expected.source.visual.character.epoch == 0
        || expected.source.visual.character.address == 0
        || (expected.source.count != 0 && expected.source.data_address == 0)
        || !expected.source.data_address.is_multiple_of(4)
    {
        return Err(Error::InvalidProfile("invalid pose record replacement"));
    }
    expected
        .source
        .data_address
        .checked_add(u64::from(expected.source.count) * u64::from(RECORD_STRIDE))
        .ok_or(Error::AddressOverflow)?;
    let mut words = Vec::new();
    for (offset, (before, after)) in expected.records.iter().zip(replacement).enumerate() {
        let record = expected.first + offset as u32;
        for (word, (before, after)) in before
            .0
            .iter()
            .flatten()
            .zip(after.0.iter().flatten())
            .enumerate()
        {
            if before != after {
                words.push(PoseWordStore {
                    address: expected.source.data_address
                        + u64::from(record) * u64::from(RECORD_STRIDE)
                        + word as u64 * 4,
                    record,
                    word: word as u8,
                    before: *before,
                    after: *after,
                });
            }
        }
    }
    Ok(words)
}

/// Validate storage at record boundaries; compare and verify every changed word.
/// Caller excludes aliases/races between these checks. Actual stores survive every failure.
pub(crate) fn commit_pose_words(
    mut plan: Vec<PoseWordStore>,
    mut read: impl FnMut(u64) -> Result<u32>,
    mut write: impl FnMut(u64, u32),
    mut validate: impl FnMut() -> Result<()>,
) -> (Vec<PoseWordStore>, Result<()>) {
    let mut performed = 0;
    let result = (|| {
        let mut record = None;
        for word in &plan {
            if record != Some(word.record) {
                validate()?;
                record = Some(word.record);
            }
            if read(word.address)? != word.before {
                return Err(Error::NativeOperationRejected("pose word superseded"));
            }
            write(word.address, word.after);
            performed += 1;
            if read(word.address)? != word.after {
                return Err(Error::NativeOperationRejected(
                    "pose word postflight differs",
                ));
            }
        }
        if record.is_some() {
            validate()?;
        }
        Ok(())
    })();
    plan.truncate(performed);
    (plan, result)
}

/// Metadata only; array bounds do not prove initialized contents, parent ordering or freshness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoseStateObservation {
    pub local: PoseBufferObservation,
    pub model: PoseBufferObservation,
    pub skeleton: ObjectObservation,
    pub bone_count: u32,
    pub parents_address: u64,
    pub parents_capacity_and_flags: u32,
    pub flags_address: u64,
    pub flags_capacity_and_flags: u32,
    pub cache_bytes: [u8; 2],
}
impl PoseStateObservation {
    pub(crate) fn same_storage(self, mut other: Self) -> bool {
        other.cache_bytes = self.cache_bytes;
        self == other
    }
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Exact hkaSkeleton type and mutually consistent bone/local/model/parent/flag counts.
    /// No array contents, parent validity, producer generation or engine guards are inferred.
    pub fn pose_state(
        &self,
        target: CharacterIdentity,
        link: CharacterVisualLink,
        limit: u32,
    ) -> Result<Option<PoseStateObservation>> {
        let Some(local) = self.pose_buffer(target, link, PoseBuffer::Primary, limit)? else {
            return Ok(None);
        };
        let Some(model) = self.pose_buffer(target, link, PoseBuffer::Secondary, limit)? else {
            return Ok(None);
        };
        let memory = self.module.memory();
        let at = |base: u64, offset: u32| {
            base.checked_add(u64::from(offset))
                .ok_or(Error::AddressOverflow)
        };
        let skeleton_slot = at(local.visual.address, SKELETON_OFFSET)?;
        let skeleton_address = memory.read_u64(skeleton_slot)?;
        let Some(skeleton) = self.require(skeleton_address, ObjectKind::AnimationSkeleton)? else {
            return Ok(None);
        };
        let read_array = |base: u64, offset: u32, stride: u64| -> Result<(u64, u32, u32)> {
            let mut raw = [0; 16];
            memory.read_exact(at(base, offset)?, &mut raw)?;
            let data = u64::from_le_bytes(raw[..8].try_into().unwrap());
            let count = i32::from_le_bytes(raw[8..12].try_into().unwrap());
            let flags = u32::from_le_bytes(raw[12..].try_into().unwrap());
            if count < 0
                || count as u32 > limit
                || count as u32 > (flags & CAPACITY_MASK)
                || !data.is_multiple_of(stride)
                || (flags & CAPACITY_MASK != 0 && data == 0)
            {
                return Err(Error::InvalidProfile("invalid pose state array bounds"));
            }
            data.checked_add(u64::from(flags & CAPACITY_MASK) * stride)
                .ok_or(Error::AddressOverflow)?;
            Ok((data, count as u32, flags))
        };
        let read = || -> Result<PoseStateObservation> {
            let bone_count = memory.read_u32(at(skeleton.address, SKELETON_BONE_COUNT_OFFSET)?)?;
            let parents = read_array(skeleton.address, PARENT_ARRAY_OFFSET, 2)?;
            let flags = read_array(local.visual.address, FLAGS_ARRAY_OFFSET, 4)?;
            let mut cache_bytes = [0; 2];
            memory.read_exact(
                at(local.visual.address, CACHE_BYTES_OFFSET)?,
                &mut cache_bytes,
            )?;
            if local.visual != model.visual
                || bone_count != local.count
                || bone_count != model.count
                || bone_count != parents.1
                || bone_count != flags.1
                || bone_count > limit
            {
                return Err(Error::InvalidProfile("pose state counts disagree"));
            }
            Ok(PoseStateObservation {
                local,
                model,
                skeleton,
                bone_count,
                parents_address: parents.0,
                parents_capacity_and_flags: parents.2,
                flags_address: flags.0,
                flags_capacity_and_flags: flags.2,
                cache_bytes,
            })
        };
        let observation = read()?;
        if read()? != observation
            || memory.read_u64(skeleton_slot)? != skeleton_address
            || self.require(skeleton_address, ObjectKind::AnimationSkeleton)? != Some(skeleton)
            || self.pose_buffer(target, link, PoseBuffer::Primary, limit)? != Some(local)
            || self.pose_buffer(target, link, PoseBuffer::Secondary, limit)? != Some(model)
        {
            return Err(Error::InvalidProfile("pose state graph changed"));
        }
        Ok(Some(observation))
    }

    /// Read metadata only. Constructor allocation/count does not prove payload initialization.
    /// No source skeleton, dirty flags, native updates or other arrays are accessed.
    pub fn pose_buffer(
        &self,
        target: CharacterIdentity,
        link: CharacterVisualLink,
        buffer: PoseBuffer,
        limit: u32,
    ) -> Result<Option<PoseBufferObservation>> {
        if !(1..=RECORD_LIMIT).contains(&limit)
            || !matches!(
                link,
                CharacterVisualLink::Pose
                    | CharacterVisualLink::AlternatePose
                    | CharacterVisualLink::ClothPose
            )
        {
            return Err(Error::InvalidProfile("invalid pose buffer request"));
        }
        let Some(visual) = self.character_visual(target, link)? else {
            return Ok(None);
        };
        let offset = match buffer {
            PoseBuffer::Primary => PRIMARY_ARRAY_OFFSET,
            PoseBuffer::Secondary => SECONDARY_ARRAY_OFFSET,
        };
        let address = visual
            .address
            .checked_add(u64::from(offset))
            .ok_or(Error::AddressOverflow)?;
        let read = || -> Result<[u8; 16]> {
            let mut bytes = [0; 16];
            self.module.memory().read_exact(address, &mut bytes)?;
            Ok(bytes)
        };
        let bytes = read()?;
        let data_address = u64::from_le_bytes(bytes[..8].try_into().expect("eight bytes"));
        let count = i32::from_le_bytes(bytes[8..12].try_into().expect("four bytes"));
        let flags = u32::from_le_bytes(bytes[12..].try_into().expect("four bytes"));
        let capacity = flags & CAPACITY_MASK;
        if count < 0
            || count as u32 > limit
            || count as u32 > capacity
            || !data_address.is_multiple_of(std::mem::align_of::<u32>() as u64)
            || (capacity != 0 && data_address == 0)
        {
            return Err(Error::InvalidProfile("invalid pose buffer bounds"));
        }
        data_address
            .checked_add(u64::from(capacity) * u64::from(RECORD_STRIDE))
            .ok_or(Error::AddressOverflow)?;
        if read()? != bytes || self.character_visual(target, link)? != Some(visual) {
            return Err(Error::InvalidProfile(
                "pose buffer changed during observation",
            ));
        }
        Ok(Some(PoseBufferObservation {
            visual,
            buffer,
            data_address,
            count: count as u32,
            capacity_and_flags: flags,
        }))
    }

    /// Copy a selected range only; each record and metadata are rechecked.
    /// # Safety
    /// Caller independently proves every requested byte has been initialized, its live
    /// allocation extent, and stable character/importer/buffer lifetime in an exclusive
    /// permitted phase. No producer/solver update, replacement, teardown or reuse overlaps.
    /// Metadata and repeated reads cannot prove initialization or exclude all races/ABA.
    /// A copy is not a native pose update or permission to write/bypass the dirty mechanism.
    pub unsafe fn copy_pose_records(
        &self,
        target: CharacterIdentity,
        link: CharacterVisualLink,
        buffer: PoseBuffer,
        first: u32,
        count: u32,
        limit: u32,
    ) -> Result<Option<PoseRecordCopy>> {
        let Some(source) = self.pose_buffer(target, link, buffer, limit)? else {
            return Ok(None);
        };
        let end = first.checked_add(count).ok_or(Error::AddressOverflow)?;
        if end > source.count {
            return Err(Error::InvalidProfile("pose copy exceeds record range"));
        }
        let mut records = Vec::with_capacity(count as usize);
        for index in first..end {
            let address = source
                .data_address
                .checked_add(u64::from(index) * u64::from(RECORD_STRIDE))
                .ok_or(Error::AddressOverflow)?;
            let mut bytes = [0; RECORD_STRIDE as usize];
            self.module.memory().read_exact(address, &mut bytes)?;
            let mut check = [0; RECORD_STRIDE as usize];
            self.module.memory().read_exact(address, &mut check)?;
            if check != bytes {
                return Err(Error::InvalidProfile("pose record changed during copy"));
            }
            records.push(PoseRecordBits(std::array::from_fn(|block| {
                std::array::from_fn(|word| {
                    let offset = (block * 4 + word) * std::mem::size_of::<u32>();
                    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
                })
            })));
        }
        if self.pose_buffer(target, link, buffer, limit)? != Some(source) {
            return Err(Error::InvalidProfile("pose source changed during copy"));
        }
        Ok(Some(PoseRecordCopy {
            source,
            first,
            records,
        }))
    }
}
