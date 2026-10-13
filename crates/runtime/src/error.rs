use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    WindowsCall {
        operation: &'static str,
        code: u32,
    },
    AddressOverflow,
    OutOfBounds {
        address: u64,
        length: usize,
    },
    InvalidPe(&'static str),
    UnsupportedMachine(u16),
    InvalidPattern {
        token: String,
    },
    EmptyPattern,
    PatternNotFound,
    TypeNotFound(String),
    AmbiguousType(String),
    UnsupportedImage {
        expected: &'static str,
        actual_sha256: [u8; 32],
    },
    ImageMismatch {
        rva: u32,
    },
    InvalidProfile(&'static str),
    NativeAllocationFailed,
    NativeOperationRejected(&'static str),
    AmbiguousPattern {
        first: usize,
        second: usize,
    },
    ProcessReadFailed {
        address: u64,
        length: usize,
        code: u32,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowsCall { operation, code } => {
                write!(f, "Windows {operation} failed with error {code}")
            }
            Self::AddressOverflow => f.write_str("address calculation overflow"),
            Self::OutOfBounds { address, length } => {
                write!(f, "unreadable span at {address:#x}, length {length:#x}")
            }
            Self::InvalidPe(reason) => write!(f, "invalid PE image: {reason}"),
            Self::UnsupportedMachine(machine) => {
                write!(f, "unsupported PE machine {machine:#x}; AMD64 required")
            }
            Self::InvalidPattern { token } => write!(
                f,
                "invalid pattern token {token:?}; use two hex digits or ??"
            ),
            Self::EmptyPattern => {
                f.write_str("an empty or entirely wildcard pattern cannot identify an address")
            }
            Self::PatternNotFound => f.write_str("pattern was not found"),
            Self::TypeNotFound(name) => write!(f, "RTTI type {name:?} was not found"),
            Self::AmbiguousType(name) => write!(f, "RTTI type {name:?} has multiple descriptors"),
            Self::UnsupportedImage {
                expected,
                actual_sha256,
            } => {
                write!(
                    f,
                    "unsupported executable; expected {expected}, actual SHA256="
                )?;
                for byte in actual_sha256 {
                    write!(f, "{byte:02x}")?;
                }
                Ok(())
            }
            Self::ImageMismatch { rva } => write!(
                f,
                "loaded image differs from validated file at RVA {rva:#x}"
            ),
            Self::InvalidProfile(reason) => write!(f, "invalid verification profile: {reason}"),
            Self::NativeAllocationFailed => f.write_str("native allocator returned null"),
            Self::NativeOperationRejected(reason) => {
                write!(f, "native operation rejected: {reason}")
            }
            Self::AmbiguousPattern { first, second } => {
                write!(f, "pattern is ambiguous at {first:#x} and {second:#x}")
            }
            Self::ProcessReadFailed {
                address,
                length,
                code,
            } => write!(
                f,
                "process read failed at {address:#x}, length {length:#x}, Windows error {code}"
            ),
        }
    }
}

impl std::error::Error for Error {}
