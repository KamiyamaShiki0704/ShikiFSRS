//! Exact executable selection and shared file metadata for ER and NR.
//! Native interfaces are game-specific and remain experimental.
use shikifsrs_elden_ring::{WW_171, Ww171File};
use shikifsrs_nightreign::{EXE_1330, NightreignFile};
use shikifsrs_runtime::{Result, profile::VerifiedFile, task_definitions::TaskDefinitions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Game {
    EldenRing,
    Nightreign,
}

/// The selected variant exposes only interfaces implemented for that exact game profile.
pub enum GameFile<'a> {
    EldenRing(Ww171File<'a>),
    Nightreign(NightreignFile<'a>),
}

impl<'a> GameFile<'a> {
    /// Identifies the immutable file by exact size and SHA256, without opening a process.
    /// A file name or product version never selects a profile.
    pub fn verify(bytes: &'a [u8]) -> Result<Self> {
        let file = VerifiedFile::identify(bytes, &[WW_171, EXE_1330])?;
        if file.identity().sha256 == WW_171.sha256 {
            Ok(Self::EldenRing(Ww171File::from_verified(file)?))
        } else {
            Ok(Self::Nightreign(NightreignFile::from_verified(file)?))
        }
    }

    pub fn game(&self) -> Game {
        match self {
            Self::EldenRing(_) => Game::EldenRing,
            Self::Nightreign(_) => Game::Nightreign,
        }
    }

    pub fn file(&self) -> &VerifiedFile<'a> {
        match self {
            Self::EldenRing(file) => file.file(),
            Self::Nightreign(file) => file.file(),
        }
    }

    pub fn task_definitions(&self) -> Result<TaskDefinitions> {
        match self {
            Self::EldenRing(file) => file.task_definitions(),
            Self::Nightreign(file) => Ok(file.task_definitions().clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shikifsrs_runtime::Error;

    #[test]
    fn names_or_version_text_do_not_admit_unknown_files() {
        for bytes in [
            b"eldenring.exe 1.17.1".as_slice(),
            b"nightreign.exe 1.3.3.0",
        ] {
            assert!(matches!(
                GameFile::verify(bytes),
                Err(Error::UnsupportedImage { .. })
            ));
        }
    }
}
