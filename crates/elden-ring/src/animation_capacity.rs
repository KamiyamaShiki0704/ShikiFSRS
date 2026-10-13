//! Experimental startup patch preparation for the fixed WW 1.17.1 animation path.
//! Preparation only reads memory. Installation, quiescence and process residency belong to the host.
use crate::Ww171File;
use shikifsrs_runtime::{
    Error, Result,
    code_patches::{CodePatch, PreparedCodePatches},
    memory::AddressSpace,
    profile::{CodeRange, VerifiedFile},
};

pub const ORIGINAL_WORKER_BUDGET: u32 = 128 * 1024;
pub const EXPANDED_WORKER_BUDGET: u32 = 1024 * 1024;
const WORLD_GLOBAL: u32 = 0x3d69ff8;

pub(crate) const CAPACITY_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0xe83c70,
        length: 0x2d4,
    },
    CodeRange {
        rva: 0x1460830,
        length: 0x323,
    },
    CodeRange {
        rva: 0x14617e0,
        length: 0xf5,
    },
    CodeRange {
        rva: 0x1515110,
        length: 0x232,
    },
    CodeRange {
        rva: 0x1534f20,
        length: 0x318,
    },
    CodeRange {
        rva: 0x1462320,
        length: 0x8,
    },
    CodeRange {
        rva: 0x1461a90,
        length: 0x8,
    },
    CodeRange {
        rva: 0x1ed8760,
        length: 0x184,
    },
    CodeRange {
        rva: 0x1f3c550,
        length: 0x9e,
    },
    CodeRange {
        rva: 0x1ebc820,
        length: 0x2ac,
    },
    CodeRange {
        rva: 0x1f35530,
        length: 0x50,
    },
    CodeRange {
        rva: 0x1f3c7e0,
        length: 0x5e,
    },
    CodeRange {
        rva: 0x1ebcba0,
        length: 0x2f,
    },
    CodeRange {
        rva: 0x1f353c0,
        length: 0x2a,
    },
    CodeRange {
        rva: 0x1ebd3f0,
        length: 0xca,
    },
];

/// Both high unsigned values are engine sentinels, rather than archive entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipSelection {
    Archive(ClipIndex),
    Dynamic,
    Unselected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipIndex(u16);

impl ClipIndex {
    pub fn new(value: u32) -> Result<Self> {
        if value > 65_533 {
            return Err(Error::InvalidProfile("clip index must be 0..=65533"));
        }
        Ok(Self(value as u16))
    }

    pub fn value(self) -> u16 {
        self.0
    }
}

impl ClipSelection {
    pub fn from_storage(value: u16) -> Self {
        match value {
            0xfffe => Self::Dynamic,
            0xffff => Self::Unselected,
            value => Self::Archive(ClipIndex(value)),
        }
    }

    pub fn storage(self) -> u16 {
        match self {
            Self::Archive(value) => value.value(),
            Self::Dynamic => 0xfffe,
            Self::Unselected => 0xffff,
        }
    }

    /// Resolve copied archive identifiers, preserving signed results and both sentinels.
    /// A short table is an error. No native pointer or animation state is read.
    pub fn resolve(self, archive: Option<&[i32]>) -> Result<i32> {
        match self {
            Self::Dynamic => Ok(-2),
            Self::Unselected => Ok(-1),
            Self::Archive(index) => match archive {
                None => Ok(index.value() as i32),
                Some(entries) => entries
                    .get(index.value() as usize)
                    .copied()
                    .ok_or(Error::InvalidProfile("clip index exceeds copied archive")),
            },
        }
    }
}

/// Allocation-free instruction metadata for the fixed WW 1.17.1 startup installer.
/// It grants no version, native phase, quiescence or lifetime authority.
#[derive(Debug, Clone, Copy)]
pub struct AnimationCapacityEdit {
    pub evidence: CodeRange,
    pub rva: u32,
    pub replacement: &'static [u8],
}

/// Fixed, independently located edits. Native -2/-1 paths remain unchanged.
/// A loader-lock installer can consume this immutable table without heap or file I/O.
/// The installer still validates its complete supported loaded image/guards, establishes
/// pre-worker quiescence, and handles page protection/partial writes itself.
const ANIMATION_CAPACITY_EDITS: &[AnimationCapacityEdit] = &[
    AnimationCapacityEdit {
        evidence: CAPACITY_EVIDENCE[0],
        rva: 0xe83e1f,
        replacement: &[0x10],
    },
    AnimationCapacityEdit {
        evidence: CAPACITY_EVIDENCE[1],
        rva: 0x14608e7,
        replacement: &[
            0x0f, 0xb7, 0x8f, 0xc6, 0, 0, 0, 0x48, 0x89, 0x6c, 0x24, 0x58, 0x66, 0x83, 0xf9, 0xff,
            0x0f, 0x84, 0x1e, 1, 0, 0, 0x0f, 0xbf, 0xd1, 0x66, 0x83, 0xf9, 0xfe, 0x74, 0x1b, 0x0f,
            0xb7, 0xd1, 0x49, 0x8b, 0x46, 0x10, 0x48, 0x8b, 0x80, 0x10, 1, 0, 0, 0x48, 0x85, 0xc0,
            0x74, 8, 0x48, 0x8b, 0x40, 0x18, 0x8b, 0x14, 0x90, 0x90,
        ],
    },
    AnimationCapacityEdit {
        evidence: CAPACITY_EVIDENCE[4],
        rva: 0x15351c0,
        replacement: &[
            0x0f, 0xbf, 0x81, 0xc6, 0, 0, 0, 0x83, 0xf8, 0xfe, 0x73, 0x0f, 0x0f, 0xb7, 0xc0, 0x4d,
            0x85, 0xed, 0x74, 7, 0x49, 0x8b, 0x4d, 0x18, 0x8b, 4, 0x81,
        ],
    },
    AnimationCapacityEdit {
        evidence: CAPACITY_EVIDENCE[3],
        rva: 0x151524b,
        replacement: &[0x44, 0x0f, 0xb7, 0x46, 0x60],
    },
    AnimationCapacityEdit {
        evidence: CAPACITY_EVIDENCE[3],
        rva: 0x1515274,
        replacement: &[0x0f, 0xb7, 0x83, 0xc6, 0, 0, 0, 0x3b, 0x46, 0x60],
    },
];

pub const fn animation_capacity_edits() -> &'static [AnimationCapacityEdit] {
    ANIMATION_CAPACITY_EDITS
}

/// Owned preparation metadata for callers outside the loader lock.
pub fn animation_capacity_patches() -> Vec<CodePatch> {
    animation_capacity_edits()
        .iter()
        .map(|edit| CodePatch {
            evidence: edit.evidence,
            rva: edit.rva,
            replacement: edit.replacement.to_vec(),
        })
        .collect()
}

impl Ww171File<'_> {
    /// Preflight original code, all dependent evidence, and the still-absent world.
    /// This is a read-only plan, not a permit to install patches or resize existing workers.
    /// The host must separately guarantee that no worker has been constructed, all readers
    /// are quiescent through installation, and the DLL remains resident for the process.
    pub fn prepare_animation_capacity<M: AddressSpace>(
        &self,
        memory: &M,
        base: u64,
    ) -> Result<PreparedCodePatches> {
        prepare(self.file(), memory, base)
    }
}

fn prepare<M: AddressSpace>(
    file: &VerifiedFile<'_>,
    memory: &M,
    base: u64,
) -> Result<PreparedCodePatches> {
    file.bind(memory, base, CAPACITY_EVIDENCE)?;
    let slot = base
        .checked_add(WORLD_GLOBAL as u64)
        .ok_or(Error::AddressOverflow)?;
    if memory.read_u64(slot)? != 0 {
        return Err(Error::NativeOperationRejected(
            "animation capacity requires startup before world creation",
        ));
    }
    let prepared = PreparedCodePatches::prepare(file, memory, base, &animation_capacity_patches())?;
    if memory.read_u64(slot)? != 0 {
        return Err(Error::NativeOperationRejected(
            "world appeared during capacity preparation",
        ));
    }
    Ok(prepared)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_storage_values_preserve_sentinels_and_extended_unsigned_indices() {
        let entries: Vec<_> = (0..65_534).map(|i| 70_000 - i).collect();
        for raw in 0..=u16::MAX {
            let selection = ClipSelection::from_storage(raw);
            assert_eq!(selection.storage(), raw);
            let expected = match raw {
                0xfffe => -2,
                0xffff => -1,
                _ => raw as i32,
            };
            assert_eq!(selection.resolve(None).unwrap(), expected);
            let mapped = if raw < 0xfffe {
                70_000 - raw as i32
            } else {
                expected
            };
            assert_eq!(selection.resolve(Some(&entries)).unwrap(), mapped);
        }
        for invalid in [65_534, 65_535, 65_536, u32::MAX] {
            assert!(ClipIndex::new(invalid).is_err());
        }
        assert!(
            ClipSelection::from_storage(3)
                .resolve(Some(&[1, 2, 3]))
                .is_err()
        );
        assert_eq!(
            ClipSelection::from_storage(0)
                .resolve(Some(&[i32::MIN]))
                .unwrap(),
            i32::MIN
        );
    }
}
