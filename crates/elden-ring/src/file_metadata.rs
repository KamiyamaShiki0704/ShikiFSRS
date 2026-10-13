//! Cache immutable file-profile facts, never loaded code, addresses or game observations.
use shikifsrs_runtime::{
    Error, Result,
    profile::{ExecutableIdentity, VerifiedFile},
};
use std::sync::OnceLock;

pub(crate) struct FileMetadataGate {
    identity: ExecutableIdentity,
    checked: OnceLock<Result<()>>,
}
impl FileMetadataGate {
    pub(crate) const fn new(identity: ExecutableIdentity) -> Self {
        Self {
            identity,
            checked: OnceLock::new(),
        }
    }
    pub(crate) fn verify(
        &self,
        file: &VerifiedFile<'_>,
        check: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        // VerifiedFile has already hashed its immutable borrowed/owned bytes. Scope cached
        // results to that exact digest and size before consulting even a successful cache.
        let actual = file.identity();
        if actual.sha256 != self.identity.sha256 || actual.file_size != self.identity.file_size {
            return Err(Error::UnsupportedImage {
                expected: self.identity.name,
                actual_sha256: actual.sha256,
            });
        }
        self.checked.get_or_init(check).clone()
    }
}
