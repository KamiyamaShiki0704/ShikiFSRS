//! Checked, file-only Nightreign metadata. No live object access or native calls.
use shikifsrs_runtime::{
    Error, Result,
    profile::{ExecutableIdentity, VerifiedFile},
    task_definitions::TaskDefinitions,
};

/// Executable file version, not the game's App or Regulation version.
pub const EXE_1330: ExecutableIdentity = ExecutableIdentity {
    name: "Nightreign / EXE 1.3.3.0",
    file_size: 118_835_320,
    sha256: [
        0x95, 0x45, 0xe8, 0x73, 0x29, 0x19, 0xbb, 0x6a, 0x63, 0x90, 0x4c, 0xb7, 0x32, 0x23, 0x64,
        0x1b, 0x3c, 0x4f, 0x9a, 0x67, 0x86, 0x9b, 0xe5, 0x72, 0x26, 0x12, 0xac, 0xca, 0x41, 0x3e,
        0x8b, 0x3a,
    ],
};

// The initializer's first loop consumes [0x2cdf8e0, 0x2ce0c60) in 32-byte rows.
// Dummy is included in that file table; this count does not authorize live registration.
const DEFINITION_TABLE: u32 = 0x2cdf8e0;
pub const TASK_DEFINITION_COUNT: usize = 156;

pub struct NightreignFile<'a> {
    file: VerifiedFile<'a>,
    definitions: TaskDefinitions,
}

impl<'a> NightreignFile<'a> {
    /// Accepts only the exact evidenced immutable file. Modified and unknown files fail.
    pub fn verify(bytes: &'a [u8]) -> Result<Self> {
        let file = VerifiedFile::verify(bytes, EXE_1330)?;
        Self::from_verified(file)
    }

    /// Reuses one checked immutable file without hashing it again.
    pub fn from_verified(file: VerifiedFile<'a>) -> Result<Self> {
        let identity = file.identity();
        if identity.file_size != EXE_1330.file_size || identity.sha256 != EXE_1330.sha256 {
            return Err(Error::UnsupportedImage {
                expected: EXE_1330.name,
                actual_sha256: identity.sha256,
            });
        }
        let definitions =
            TaskDefinitions::read(file.image(), DEFINITION_TABLE, TASK_DEFINITION_COUNT)?;
        for (name, index) in [
            ("FrameBegin", 0),
            ("DrawParamUpdate", 92),
            ("SoundStep", 94),
            ("ChrIns_PostPhysics", 98),
            ("FrameEnd", 154),
            ("Dummy", 155),
        ] {
            if definitions.find(name).map(|row| row.registration_index) != Some(index) {
                return Err(Error::InvalidProfile(
                    "Nightreign task definitions differ from the evidenced file",
                ));
            }
        }
        Ok(Self { file, definitions })
    }

    pub fn file(&self) -> &VerifiedFile<'a> {
        &self.file
    }

    pub fn task_definitions(&self) -> &TaskDefinitions {
        &self.definitions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_files_are_rejected_before_metadata_access() {
        for bytes in [&[][..], b"MZ".as_slice(), &[0; 1024][..]] {
            assert!(matches!(
                NightreignFile::verify(bytes),
                Err(Error::UnsupportedImage { .. })
            ));
        }
    }
}
