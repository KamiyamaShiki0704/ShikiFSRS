//! Common-event Gparam interface facts, independently derived from current ER.
use crate::objects::{GameObjectCatalog, ObjectKind, ObjectObservation};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const GPARAM_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0xab9cb0,
        length: 0x104,
    },
    CodeRange {
        rva: 0x1f2420,
        length: 0x11b,
    },
    CodeRange {
        rva: 0x1f8310,
        length: 0x4eb,
    },
    CodeRange {
        rva: 0x1efea0,
        length: 0x1ca,
    },
    CodeRange {
        rva: 0x61f650,
        length: 0xaa7,
    },
    CodeRange {
        rva: 0xaba6f0,
        length: 0xf9,
    },
    CodeRange {
        rva: 0xb5df60,
        length: 0x136,
    },
    CodeRange {
        rva: 0xb60fb0,
        length: 0x11f,
    },
    CodeRange {
        rva: 0xb5e5b0,
        length: 0x2c,
    },
    CodeRange {
        rva: 0xb60060,
        length: 0x1d0,
    },
    CodeRange {
        rva: 0x213470,
        length: 0xa3,
    },
    CodeRange {
        rva: 0x2656130,
        length: 0x9,
    },
    CodeRange {
        rva: 0x2654430,
        length: 0xfd,
    },
    CodeRange {
        rva: 0x2656350,
        length: 0x223,
    },
];
pub(crate) const FILE_REQUEST_SLOT: u32 = 0x29d4818;
const FILE_MANAGER_GLOBAL: u32 = 0x3d5f158;
const DRAW_MANAGER_GLOBAL: u32 = 0x3d735c8;
const GPARAM_REPOSITORY_GLOBAL: u32 = 0x3d76ba8;

/// The native prime entry consumes DX, not the full supplied EDX.
/// Reject high bits so the request path and native resource key remain the same ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommonEventId(u16);
impl CommonEventId {
    pub fn new(id: u32) -> Result<Self> {
        Ok(Self(u16::try_from(id).map_err(|_| {
            Error::InvalidProfile("common-event ID exceeds native16 bits")
        })?))
    }
    pub fn value(self) -> u16 {
        self.0
    }
    /// Native formatter uses minimum four digits, allowing five for u16's upper values.
    pub fn path(self) -> String {
        format!("gparam:/m00_00_{:04}_CommonEvent.gparam", self.0)
    }
    pub fn wide_path(self) -> Vec<u16> {
        self.path().encode_utf16().chain([0]).collect()
    }
}
/// One actual invocation, including null/void results. Postflight failure does
/// not erase submission or authorize a retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeGparamCall<T> {
    pub manager: ObjectObservation,
    pub id: CommonEventId,
    pub value: T,
    pub postflight: Result<()>,
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    pub fn gparam_file_manager(&self) -> Result<Option<ObjectObservation>> {
        self.singleton(FILE_MANAGER_GLOBAL, ObjectKind::FileResourceManager)
    }
    pub fn gparam_draw_manager(&self) -> Result<Option<ObjectObservation>> {
        self.singleton(DRAW_MANAGER_GLOBAL, ObjectKind::WorldSceneDrawParameters)
    }
    pub fn gparam_repository(&self) -> Result<Option<ObjectObservation>> {
        self.singleton(GPARAM_REPOSITORY_GLOBAL, ObjectKind::GparamRepository)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn common_event_paths_are_bounded_consistent_with_native_low16_key_and_nul_terminated() {
        for (id, digits) in [
            (0, "0000"),
            (5, "0005"),
            (9999, "9999"),
            (10000, "10000"),
            (65535, "65535"),
        ] {
            let event = CommonEventId::new(id).unwrap();
            assert_eq!(event.value() as u32, id);
            let text = format!("gparam:/m00_00_{digits}_CommonEvent.gparam");
            assert_eq!(event.path(), text);
            let wide = event.wide_path();
            assert_eq!(wide.last(), Some(&0));
            assert_eq!(wide.iter().filter(|x| **x == 0).count(), 1);
            assert_eq!(String::from_utf16(&wide[..wide.len() - 1]).unwrap(), text);
        }
        for id in [65536, u32::MAX] {
            assert!(CommonEventId::new(id).is_err());
        }
    }
}
