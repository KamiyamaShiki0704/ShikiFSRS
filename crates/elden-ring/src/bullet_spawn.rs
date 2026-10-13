//! Captured native-consumed spawn fields. Copies do not acquire engine references.
use crate::objects::GameObjectCatalog;
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const SPAWN_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x3a2cb0,
        length: 0x13c,
    },
    CodeRange {
        rva: 0x3a2c90,
        length: 0x1c,
    },
    CodeRange {
        rva: 0x3a6d70,
        length: 0x14f,
    },
    CodeRange {
        rva: 0x38c580,
        length: 0xd9,
    },
    CodeRange {
        rva: 0x393d40,
        length: 0x46,
    },
    CodeRange {
        rva: 0x393db0,
        length: 0x53,
    },
    CodeRange {
        rva: 0x394e80,
        length: 0xf9,
    },
    CodeRange {
        rva: 0x38b9b0,
        length: 0x260,
    },
    CodeRange {
        rva: 0x38e550,
        length: 0x1ff,
    },
    CodeRange {
        rva: 0x38e220,
        length: 0x16c,
    },
    CodeRange {
        rva: 0x38e390,
        length: 0x1bd,
    },
    CodeRange {
        rva: 0x38e750,
        length: 0x29e,
    },
    CodeRange {
        rva: 0x38c860,
        length: 0x145,
    },
    CodeRange {
        rva: 0x3a5a20,
        length: 0x1e0,
    },
];
pub const BULLET_SPAWN_HOOK_RVA: u32 = 0x3a2cb0;
pub(crate) const SPAWN_TARGET: u32 = 0x20;
// Original sparse layout from constructor and complete input-consuming functions.
// Padding is zero in SDK storage, never read from the engine's input or compared.
const CONSUMED_SPANS: &[(usize, usize)] = &[
    (0, 0x48),
    (0x50, 0x59),
    (0xb0, 1),
    (0xb8, 0x10),
    (0xd0, 0x24),
    (0x100, 1),
];

#[repr(C, align(16))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulletSpawnInput {
    bytes: [u8; 0x110],
}
impl BulletSpawnInput {
    pub fn owner_identifier(&self) -> u64 {
        self.word64(0)
    }
    pub fn target_identifier(&self) -> u64 {
        self.word64(0x20)
    }
    pub fn bullet_parameter_id(&self) -> i32 {
        i32::from_le_bytes(self.bytes[0x14..0x18].try_into().unwrap())
    }
    pub fn with_target(&self, target: u64) -> Self {
        let mut copy = self.clone();
        copy.bytes[0x20..0x28].copy_from_slice(&target.to_le_bytes());
        copy
    }
    fn word64(&self, offset: usize) -> u64 {
        u64::from_le_bytes(self.bytes[offset..offset + 8].try_into().unwrap())
    }
    /// Three opaque references consumed by native helpers. Copying does not retain them.
    pub fn reference_addresses(&self) -> [u64; 3] {
        [self.word64(0x98), self.word64(0xa0), self.word64(0xc0)]
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulletSpawnObservation {
    pub module_base: u64,
    pub source_address: u64,
    pub epoch: u64,
    pub input: BulletSpawnInput,
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Input need not be a polymorphic object. Copies initialized consumed spans only.
    /// No native reference operation, input destructor or recursive traversal is performed.
    pub fn bullet_spawn_input(&self, address: u64, epoch: u64) -> Result<BulletSpawnObservation> {
        if epoch == 0 || address == 0 || !address.is_multiple_of(16) {
            return Err(Error::InvalidProfile(
                "invalid bullet spawn input address/epoch",
            ));
        }
        let read = || -> Result<BulletSpawnInput> {
            let mut input = BulletSpawnInput { bytes: [0; 0x110] };
            for &(offset, length) in CONSUMED_SPANS {
                let source = address
                    .checked_add(offset as u64)
                    .ok_or(Error::AddressOverflow)?;
                self.module
                    .memory()
                    .read_exact(source, &mut input.bytes[offset..offset + length])?;
            }
            Ok(input)
        };
        let input = read()?;
        if read()? != input {
            return Err(Error::InvalidProfile(
                "bullet spawn input changed during observation",
            ));
        }
        Ok(BulletSpawnObservation {
            module_base: self.module.base(),
            source_address: address,
            epoch,
            input,
        })
    }
}
#[derive(Debug)]
pub struct BulletSpawnTargetWrite {
    pub source: BulletSpawnObservation,
    pub requested_identifier: u64,
    pub stored: bool,
    pub postflight: Result<BulletSpawnObservation>,
}
pub(crate) struct SpawnTargetPlan {
    source: BulletSpawnObservation,
    target: u64,
    address: usize,
}
pub(crate) fn target_plan(
    current: BulletSpawnObservation,
    expected: BulletSpawnObservation,
    target: u64,
) -> Result<SpawnTargetPlan> {
    if current != expected || expected.epoch == 0 {
        return Err(Error::InvalidProfile("bullet spawn input superseded"));
    }
    let address = usize::try_from(
        expected
            .source_address
            .checked_add(u64::from(SPAWN_TARGET))
            .ok_or(Error::AddressOverflow)?,
    )
    .map_err(|_| Error::AddressOverflow)?;
    address.checked_add(8).ok_or(Error::AddressOverflow)?;
    if address == 0 || !address.is_multiple_of(8) {
        return Err(Error::InvalidProfile(
            "unaligned bullet spawn target destination",
        ));
    }
    Ok(SpawnTargetPlan {
        source: expected,
        target,
        address,
    })
}
pub(crate) fn commit_target(
    plan: SpawnTargetPlan,
    store: impl FnOnce(usize, u64),
    observe: impl FnOnce() -> Result<BulletSpawnObservation>,
) -> BulletSpawnTargetWrite {
    let stored = plan.target != plan.source.input.target_identifier();
    let mut expected = plan.source.clone();
    expected.input = expected.input.with_target(plan.target);
    if stored {
        store(plan.address, plan.target);
    }
    let postflight = observe().and_then(|current| {
        if current == expected {
            Ok(current)
        } else {
            Err(Error::InvalidProfile(
                "bullet spawn input changed after target store",
            ))
        }
    });
    BulletSpawnTargetWrite {
        source: plan.source,
        requested_identifier: plan.target,
        stored,
        postflight,
    }
}

#[derive(Debug)]
pub struct BulletSpawnCall {
    pub manager_address: u64,
    /// Output is a native secondary-emitter key, not a full CSBulletIns handle.
    pub native_output: u32,
    /// Raw result/status. Initialized to ffffffff; no success classification is inferred.
    pub native_status: u32,
    pub returned_output_pointer: bool,
    /// Actual call happened once even when this check fails. Does not prove a live projectile.
    pub postflight: Result<()>,
}
pub(crate) type Spawn =
    unsafe extern "win64" fn(u64, *mut u32, *const BulletSpawnInput, *mut u32) -> *mut u32;
pub(crate) unsafe fn invoke_spawn(
    function: Spawn,
    manager: u64,
    input: &BulletSpawnInput,
    postflight: impl FnOnce() -> Result<()>,
) -> BulletSpawnCall {
    let mut native_output = u32::MAX;
    let mut native_status = u32::MAX;
    let output_pointer = &mut native_output as *mut u32;
    // SAFETY: Caller provides the proven native ABI, complete engine graph and exclusive phase.
    // Both outputs and the aligned captured consumed fields remain live through the call.
    let returned = unsafe { function(manager, output_pointer, input, &mut native_status) };
    let returned_output_pointer = returned == output_pointer;
    let postflight = postflight().and({
        if returned_output_pointer {
            Ok(())
        } else {
            Err(Error::InvalidProfile(
                "bullet spawn returned foreign output pointer",
            ))
        }
    });
    BulletSpawnCall {
        manager_address: manager,
        native_output,
        native_status,
        returned_output_pointer,
        postflight,
    }
}
