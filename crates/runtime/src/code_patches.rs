//! Explicit owned code replacements. This module never installs or restores a patch.
use crate::{
    Error, Result,
    memory::AddressSpace,
    profile::{CodeRange, VerifiedFile},
};

const MAX_PATCHES: usize = 64;
const MAX_PATCH_BYTES: usize = 4096;
const MAX_EVIDENCE_BYTES: usize = 1024 * 1024;

/// The replacement must lie wholly inside an independently validated executable span.
#[derive(Debug, Clone)]
pub struct CodePatch {
    pub evidence: CodeRange,
    pub rva: u32,
    pub replacement: Vec<u8>,
}

/// A bounded future replacement whose bytes are allocated by its hook owner later.
#[derive(Debug, Clone, Copy)]
pub struct CodePatchSite {
    pub evidence: CodeRange,
    pub rva: u32,
    pub length: usize,
}

/// Original-only preparation. Cannot be used as an accepted live hook manifest.
pub struct PreparedCodeSites {
    prepared: PreparedCodePatches,
}
impl PreparedCodeSites {
    /// Validate all original evidence before the owner allocates/installs its hooks.
    /// No patch is installed, inferred, or accepted from current process memory.
    pub fn prepare<M: AddressSpace>(
        file: &VerifiedFile<'_>,
        memory: &M,
        base: u64,
        sites: &[CodePatchSite],
    ) -> Result<Self> {
        if sites.is_empty() || sites.len() > MAX_PATCHES {
            return Err(Error::InvalidProfile("code site count must be 1..=64"));
        }
        let mut patches = Vec::with_capacity(sites.len());
        for site in sites {
            if site.length == 0 || site.length > MAX_PATCH_BYTES {
                return Err(Error::InvalidProfile("invalid code site length"));
            }
            // This placeholder only reuses original-span validation. It is private and
            // cannot become an OwnedCodePatches until explicit owner declarations replace it.
            let original = file.image().read_rva(site.rva, site.length)?;
            let mut placeholder = original.to_vec();
            placeholder[0] ^= 0xff;
            patches.push(CodePatch {
                evidence: site.evidence,
                rva: site.rva,
                replacement: placeholder,
            });
        }
        Ok(Self {
            prepared: PreparedCodePatches::prepare(file, memory, base, &patches)?,
        })
    }
    /// Supply exact owner-authored bytes for every prepared site, in any order.
    /// Does not read or bless bytes from process memory, expand sites, or install code.
    /// The separate unsafe ownership declaration still supplies complete hook contracts.
    pub fn declare_replacements(
        mut self,
        patches: &[(u32, Vec<u8>)],
    ) -> Result<PreparedCodePatches> {
        if patches.len() != self.prepared.patches.len() {
            return Err(Error::InvalidProfile("incomplete code site declarations"));
        }
        let mut seen = Vec::with_capacity(patches.len());
        for (rva, replacement) in patches {
            if seen.contains(rva) {
                return Err(Error::InvalidProfile("duplicate code site declaration"));
            }
            seen.push(*rva);
            let patch = self
                .prepared
                .patches
                .iter_mut()
                .find(|p| p.rva == *rva)
                .ok_or(Error::InvalidProfile("unprepared code site declaration"))?;
            if replacement.len() != patch.original.len() || *replacement == patch.original {
                return Err(Error::InvalidProfile(
                    "code site declaration has wrong length or is unchanged",
                ));
            }
            patch.replacement.clone_from(replacement);
        }
        Ok(self.prepared)
    }
}

#[derive(Debug)]
struct PreparedPatch {
    rva: u32,
    original: Vec<u8>,
    replacement: Vec<u8>,
}

/// File/base-bound byte evidence prepared while all declared spans are still original.
/// Preparing is read-only and does not authorize a native calling convention or ownership.
#[derive(Debug)]
pub struct PreparedCodePatches {
    sha256: [u8; 32],
    file_size: usize,
    base: u64,
    patches: Vec<PreparedPatch>,
}

/// Immutable declaration of replacements whose contract is supplied by their owner.
/// Exact bytes do not prove hook correctness, synchronized installation, or safe unloading.
#[derive(Debug)]
pub struct OwnedCodePatches(PreparedCodePatches);

impl PreparedCodePatches {
    /// Checks identity, loaded headers and whole evidence spans before retaining originals.
    /// No live-memory pointer, executable allocation, or game reference is retained.
    pub fn prepare<M: AddressSpace>(
        file: &VerifiedFile<'_>,
        memory: &M,
        base: u64,
        patches: &[CodePatch],
    ) -> Result<Self> {
        if patches.is_empty() || patches.len() > MAX_PATCHES {
            return Err(Error::InvalidProfile("code patch count must be 1..=64"));
        }
        let mut prepared = Vec::with_capacity(patches.len());
        for patch in patches {
            let length = patch.replacement.len();
            if length == 0 || length > MAX_PATCH_BYTES || patch.evidence.length > MAX_EVIDENCE_BYTES
            {
                return Err(Error::InvalidProfile(
                    "code patch or evidence exceeds its bound",
                ));
            }
            let end = patch.rva as u64 + length as u64;
            let evidence_end = patch.evidence.rva as u64 + patch.evidence.length as u64;
            if patch.rva < patch.evidence.rva || end > evidence_end {
                return Err(Error::InvalidProfile(
                    "code patch lies outside validated evidence",
                ));
            }
            let original = file.image().read_rva(patch.rva, length)?.to_vec();
            if original == patch.replacement {
                return Err(Error::InvalidProfile("code patch is unchanged"));
            }
            prepared.push(PreparedPatch {
                rva: patch.rva,
                original,
                replacement: patch.replacement.clone(),
            });
        }
        prepared.sort_by_key(|p| p.rva);
        for pair in prepared.windows(2) {
            if pair[0].rva as u64 + pair[0].original.len() as u64 > pair[1].rva as u64 {
                return Err(Error::InvalidProfile("overlapping code patches"));
            }
        }
        let evidence: Vec<_> = patches.iter().map(|p| p.evidence).collect();
        file.bind(memory, base, &evidence)?;
        let identity = file.identity();
        Ok(Self {
            sha256: identity.sha256,
            file_size: identity.file_size,
            base,
            patches: prepared,
        })
    }

    /// Retain this immutable manifest for an independently implemented hook owner.
    /// # Safety
    /// Caller owns every replacement and its complete transitive executable/data lifetime.
    /// Hooks preserve the independently proved ABI, object layouts and operation semantics
    /// needed by every SDK interface used with this manifest. Caller installs/restores them
    /// only under independently established quiescence, and excludes all concurrent patching
    /// during validation and native operations. Every original or replacement must remain
    /// callable for the entire operation, including callbacks, trampolines and return frames.
    /// This declaration does not acquire guards, install bytes, or permit DLL unloading.
    pub unsafe fn into_owned(self) -> OwnedCodePatches {
        OwnedCodePatches(self)
    }
}

impl OwnedCodePatches {
    pub(crate) fn check_binding(&self, file: &VerifiedFile<'_>, base: u64) -> Result<()> {
        let id = file.identity();
        if self.0.base != base || self.0.sha256 != id.sha256 || self.0.file_size != id.file_size {
            return Err(Error::InvalidProfile(
                "code patches belong to another file or module base",
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_all<M: AddressSpace>(&self, memory: &M) -> Result<()> {
        for patch in &self.0.patches {
            self.copy_live(memory, patch)?;
        }
        Ok(())
    }

    fn copy_live<M: AddressSpace>(&self, memory: &M, patch: &PreparedPatch) -> Result<Vec<u8>> {
        let at = self
            .0
            .base
            .checked_add(patch.rva as u64)
            .ok_or(Error::AddressOverflow)?;
        let mut bytes = vec![0; patch.original.len()];
        memory.read_exact(at, &mut bytes)?;
        if bytes != patch.original && bytes != patch.replacement {
            return Err(Error::ImageMismatch { rva: patch.rva });
        }
        Ok(bytes)
    }

    /// Compare actual memory, never fabricate original bytes through the AddressSpace seam.
    /// Full replacements are checked even when only one byte intersects the requested range.
    pub(crate) fn compare<M: AddressSpace>(
        &self,
        memory: &M,
        rva: u32,
        original: &[u8],
    ) -> Result<()> {
        let end = rva as u64 + original.len() as u64;
        let mut selected = Vec::new();
        for patch in &self.0.patches {
            if patch.rva as u64 >= end
                || patch.rva as u64 + patch.original.len() as u64 <= rva as u64
            {
                continue;
            }
            selected.push((patch, self.copy_live(memory, patch)?));
        }
        let mut buffer = [0; 4096];
        for (index, chunk) in original.chunks(buffer.len()).enumerate() {
            let start = rva as u64 + (index * buffer.len()) as u64;
            let finish = start + chunk.len() as u64;
            let mut expected = chunk.to_vec();
            for (patch, bytes) in &selected {
                let first = start.max(patch.rva as u64);
                let last = finish.min(patch.rva as u64 + bytes.len() as u64);
                if first < last {
                    expected[(first - start) as usize..(last - start) as usize].copy_from_slice(
                        &bytes[(first - patch.rva as u64) as usize
                            ..(last - patch.rva as u64) as usize],
                    );
                }
            }
            let at = self
                .0
                .base
                .checked_add(start)
                .ok_or(Error::AddressOverflow)?;
            memory.read_exact(at, &mut buffer[..chunk.len()])?;
            if expected != buffer[..chunk.len()] {
                return Err(Error::ImageMismatch { rva });
            }
        }
        for (patch, before) in selected {
            if self.copy_live(memory, patch)? != before {
                return Err(Error::ImageMismatch { rva: patch.rva });
            }
        }
        Ok(())
    }
}
