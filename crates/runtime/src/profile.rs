//! File identity and live-image validation share the same checked memory seam.
use crate::{Error, Result, code_patches::OwnedCodePatches, memory::AddressSpace, pe::FileImage};
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub struct ExecutableIdentity {
    pub name: &'static str,
    pub file_size: usize,
    pub sha256: [u8; 32],
}

#[derive(Debug, Clone, Copy)]
pub struct CodeRange {
    pub rva: u32,
    pub length: usize,
}

#[derive(Debug)]
pub struct VerifiedFile<'a> {
    image: FileImage<'a>,
    identity: ExecutableIdentity,
}

/// Immutable owned input: hashes once, then lends checked PE views without self references.
#[derive(Debug)]
pub struct OwnedVerifiedFile {
    bytes: Arc<[u8]>,
    identity: ExecutableIdentity,
}

impl OwnedVerifiedFile {
    pub fn verify(bytes: Arc<[u8]>, identity: ExecutableIdentity) -> Result<Self> {
        VerifiedFile::verify(&bytes, identity)?;
        Ok(Self { bytes, identity })
    }

    pub fn borrow(&self) -> Result<VerifiedFile<'_>> {
        // Private immutable Arc storage keeps the previously checked identity valid.
        Ok(VerifiedFile {
            image: FileImage::parse(&self.bytes)?,
            identity: self.identity,
        })
    }
}

impl<'a> VerifiedFile<'a> {
    /// Exact identity is mandatory. A matching product-version string is insufficient.
    pub fn verify(bytes: &'a [u8], identity: ExecutableIdentity) -> Result<Self> {
        Self::identify(bytes, &[identity])
    }

    /// Selects one exact file identity with a single digest calculation.
    /// Empty registries and ambiguous matching identities are rejected.
    pub fn identify(bytes: &'a [u8], identities: &[ExecutableIdentity]) -> Result<Self> {
        if identities.is_empty() {
            return Err(Error::InvalidProfile("empty executable identity registry"));
        }
        let actual_sha256: [u8; 32] = Sha256::digest(bytes).into();
        let mut matches = identities.iter().filter(|identity| {
            bytes.len() == identity.file_size && actual_sha256 == identity.sha256
        });
        let identity = *matches.next().ok_or(Error::UnsupportedImage {
            expected: if identities.len() == 1 {
                identities[0].name
            } else {
                "a registered exact executable identity"
            },
            actual_sha256,
        })?;
        if matches.next().is_some() {
            return Err(Error::InvalidProfile(
                "ambiguous executable identity registry",
            ));
        }
        let image = FileImage::parse(bytes)?;
        Ok(Self { image, identity })
    }

    pub fn image(&self) -> &FileImage<'a> {
        &self.image
    }
    pub fn identity(&self) -> ExecutableIdentity {
        self.identity
    }

    /// Reads and compares complete file-backed executable spans before exposing addresses.
    /// This validates identity and bytes, not an FFI signature or the engine's task phase.
    pub fn bind<'b, M: AddressSpace>(
        &'b self,
        memory: &'b M,
        base: u64,
        code: &[CodeRange],
    ) -> Result<VerifiedModule<'b, 'a, M>> {
        self.bind_with_patches(memory, base, code, None)
    }

    /// Explicit opt-in for an owned hook manifest; the default bind remains strict.
    /// Headers, virtual slots, globals and undeclared code retain their original checks.
    pub fn bind_with_patches<'b, M: AddressSpace>(
        &'b self,
        memory: &'b M,
        base: u64,
        code: &[CodeRange],
        patches: Option<&'b OwnedCodePatches>,
    ) -> Result<VerifiedModule<'b, 'a, M>> {
        base.checked_add(self.image.image_size as u64)
            .ok_or(Error::AddressOverflow)?;
        if base == 0 || code.is_empty() {
            return Err(Error::InvalidProfile(
                "nonzero base and executable evidence are required",
            ));
        }
        if let Some(patches) = patches {
            patches.check_binding(self, base)?;
        }
        let module = VerifiedModule {
            file: self,
            memory,
            base,
            patches,
        };
        module.compare(0, self.image.header_size as usize)?;
        if let Some(patches) = patches {
            patches.validate_all(memory)?;
        }
        for range in code {
            module.code_address(*range)?;
        }
        Ok(module)
    }
}

pub struct VerifiedModule<'b, 'a, M> {
    file: &'b VerifiedFile<'a>,
    memory: &'b M,
    base: u64,
    patches: Option<&'b OwnedCodePatches>,
}

impl<M: AddressSpace> VerifiedModule<'_, '_, M> {
    pub fn base(&self) -> u64 {
        self.base
    }
    pub fn memory(&self) -> &M {
        self.memory
    }
    pub fn file(&self) -> &VerifiedFile<'_> {
        self.file
    }

    fn compare(&self, rva: u32, length: usize) -> Result<u64> {
        let expected = self.file.image.read_rva(rva, length)?;
        let address = self
            .base
            .checked_add(rva as u64)
            .ok_or(Error::AddressOverflow)?;
        let mut buffer = [0; 4096];
        for (index, chunk) in expected.chunks(buffer.len()).enumerate() {
            let offset = index
                .checked_mul(buffer.len())
                .ok_or(Error::AddressOverflow)?;
            let at = address
                .checked_add(offset as u64)
                .ok_or(Error::AddressOverflow)?;
            self.memory.read_exact(at, &mut buffer[..chunk.len()])?;
            if chunk != &buffer[..chunk.len()] {
                return Err(Error::ImageMismatch { rva });
            }
        }
        Ok(address)
    }

    /// Revalidates the requested bytes at lookup time, including after another mod patches code.
    pub fn code_address(&self, range: CodeRange) -> Result<u64> {
        if range.length == 0 {
            return Err(Error::InvalidProfile("empty code range"));
        }
        let section = self
            .file
            .image
            .section_at(range.rva)
            .filter(|s| s.is_executable())
            .ok_or(Error::InvalidProfile("code range is not executable"))?;
        let end = (range.rva as u64)
            .checked_add(range.length as u64)
            .ok_or(Error::AddressOverflow)?;
        if end > section.rva as u64 + section.raw_size as u64 {
            return Err(Error::InvalidProfile(
                "code range crosses executable section",
            ));
        }
        if let Some(patches) = self.patches {
            patches.compare(
                self.memory,
                range.rva,
                self.file.image.read_rva(range.rva, range.length)?,
            )?;
            self.base
                .checked_add(range.rva as u64)
                .ok_or(Error::AddressOverflow)
        } else {
            self.compare(range.rva, range.length)
        }
    }

    /// Verify a readonly virtual slot with ASLR and its exact exception-directory entry.
    /// This checks identity/bytes, not the callee's signature or a valid calling phase.
    pub fn checked_virtual_function(&self, table: u64, slot_offset: u32) -> Result<CodeRange> {
        if !table.is_multiple_of(8) || !slot_offset.is_multiple_of(8) {
            return Err(Error::InvalidProfile("unaligned virtual function slot"));
        }
        let slot = table
            .checked_add(slot_offset as u64)
            .ok_or(Error::AddressOverflow)?;
        let slot_rva = slot
            .checked_sub(self.base)
            .and_then(|rva| u32::try_from(rva).ok())
            .ok_or(Error::InvalidProfile(
                "virtual function slot is outside the module",
            ))?;
        let section = self
            .file
            .image
            .section_at(slot_rva)
            .filter(|s| {
                !s.is_writable() && !s.is_executable() && s.characteristics & 0x40000000 != 0
            })
            .ok_or(Error::InvalidProfile(
                "virtual function slot is not readonly data",
            ))?;
        if slot_rva as u64 + 8 > section.rva as u64 + section.raw_size as u64 {
            return Err(Error::InvalidProfile(
                "virtual function slot crosses file-backed data",
            ));
        }
        let pointer = u64::from_le_bytes(
            self.file
                .image
                .read_rva(slot_rva, 8)?
                .try_into()
                .expect("checked width"),
        );
        let target = pointer
            .checked_sub(self.file.image.preferred_base)
            .and_then(|rva| u32::try_from(rva).ok())
            .ok_or(Error::InvalidProfile(
                "virtual function target is outside the file image",
            ))?;
        let range = self.file.image.runtime_function(target)?;
        let address = self.code_address(range)?;
        if self.memory.read_u64(slot)? != address {
            return Err(Error::ImageMismatch { rva: slot_rva });
        }
        Ok(range)
    }

    /// Copies a nullable global pointer from a validated writable data section.
    pub fn global_pointer(&self, rva: u32) -> Result<Option<u64>> {
        let section = self
            .file
            .image
            .section_at(rva)
            .filter(|s| s.is_writable() && !s.is_executable())
            .ok_or(Error::InvalidProfile("global pointer is not writable data"))?;
        if !rva.is_multiple_of(8)
            || rva as u64 + 8
                > section.rva as u64 + section.virtual_size.max(section.raw_size) as u64
        {
            return Err(Error::InvalidProfile(
                "global pointer is unaligned or crosses section",
            ));
        }
        let address = self
            .base
            .checked_add(rva as u64)
            .ok_or(Error::AddressOverflow)?;
        let pointer = self.memory.read_u64(address)?;
        Ok((pointer != 0).then_some(pointer))
    }
}
