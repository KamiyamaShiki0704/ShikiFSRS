//! Copied primary bullet-pool observations and explicit velocity replacement receipts.
//! List membership is not a promise that a projectile is active or its resources are ready.
use crate::objects::{GameObjectCatalog, ObjectKind};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};
use std::collections::BTreeSet;

pub(crate) const BULLET_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x62eeb0,
        length: 0x1b5,
    },
    CodeRange {
        rva: 0x3a10e0,
        length: 0x49f,
    },
    CodeRange {
        rva: 0x3a7c20,
        length: 0x95,
    },
    CodeRange {
        rva: 0x3a7100,
        length: 0x81,
    },
    CodeRange {
        rva: 0x395820,
        length: 0x223,
    },
    CodeRange {
        rva: 0x3a3ac0,
        length: 0x79,
    },
    CodeRange {
        rva: 0x39f860,
        length: 0x1c6,
    },
    CodeRange {
        rva: 0x39a110,
        length: 0x709,
    },
    CodeRange {
        rva: 0x3938c0,
        length: 0x39,
    },
    CodeRange {
        rva: 0x3935f0,
        length: 0x2f,
    },
    CodeRange {
        rva: 0x3ab910,
        length: 0x64,
    },
    CodeRange {
        rva: 0x3ab8c0,
        length: 0x46,
    },
    CodeRange {
        rva: 0xd355d0,
        length: 0x17f,
    },
    CodeRange {
        rva: 0x38eb70,
        length: 0x15c,
    },
    CodeRange {
        rva: 0x3ac250,
        length: 0x50,
    },
    CodeRange {
        rva: 0x3ac360,
        length: 0xb,
    },
    CodeRange {
        rva: 0x3ac760,
        length: 0x2e,
    },
];
const BULLET_MANAGER_GLOBAL: u32 = 0x3d667a8;
const BULLET_STRIDE: u32 = 0xe30;
const BULLET_NEXT: u32 = 0xe18;
const BULLET_OWNER: u32 = 0x4c0;
const BULLET_ELAPSED: u32 = 0xb24;
const BULLET_POSITION: u32 = 0x10;
pub(crate) const BULLET_VELOCITY: u32 = 0x30;
const BULLET_WAIT: u32 = 0xd90;
const BULLET_FLY: u32 = 0xdc0;
const BULLET_EXPLOSION: u32 = 0xdf0;
const BULLET_TARGETING_OWNER: u32 = 0xb30;
pub const MAX_PRIMARY_BULLET_MEMBERS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulletIdentity {
    pub module_base: u64,
    pub manager_address: u64,
    pub address: u64,
    pub native_identifier: u64,
    /// Caller-owned known lifetime generation; does not pin an engine allocation.
    pub epoch: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulletParameterState {
    pub address: u64,
    /// Copied opaque row pointer. Not dereferenced or treated as a resource reference.
    pub row_address: u64,
    pub parameter_id: i32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulletObservation {
    pub identity: BulletIdentity,
    pub next_address: u64,
    pub owner_identifier: u64,
    /// Separate targeting-system owner handle, initialized by the native owner setter.
    pub targeting_owner_identifier: u64,
    pub position_bits: [u32; 4],
    pub velocity_bits: [u32; 4],
    /// Native float delta accumulator. Units and pause behavior require game acceptance.
    pub elapsed_step_bits: u32,
    pub wait: BulletParameterState,
    pub fly: BulletParameterState,
    pub explosion: BulletParameterState,
}
impl BulletObservation {
    pub fn finite_position(&self) -> Result<[f32; 3]> {
        finite_xyz(self.position_bits)
    }
    pub fn finite_velocity(&self) -> Result<[f32; 3]> {
        finite_xyz(self.velocity_bits)
    }
}
fn finite_xyz(bits: [u32; 4]) -> Result<[f32; 3]> {
    let xyz = std::array::from_fn(|i| f32::from_bits(bits[i]));
    if xyz.into_iter().any(|value| !value.is_finite()) {
        return Err(Error::InvalidProfile("nonfinite bullet vector"));
    }
    Ok(xyz)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulletListObservation {
    pub manager_address: u64,
    pub pool_address: u64,
    pub head_address: u64,
    pub extra_allocation_count: u32,
    pub generation_counter: u32,
    pub members: Vec<BulletObservation>,
}
fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}
fn aligned(address: u64) -> Result<()> {
    if address == 0 || !address.is_multiple_of(8) {
        return Err(Error::InvalidProfile("invalid bullet pool pointer"));
    }
    Ok(())
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copies only the primary chain: 128 fixed slots and at most 128 spill allocations.
    /// Two complete copies detect visible changes; they are not a lock or an ABA guard.
    /// Manager is independently proved nonpolymorphic; its first word is the pool pointer.
    pub fn bullets(&self, epoch: u64, limit: usize) -> Result<Option<BulletListObservation>> {
        if epoch == 0 || !(1..=MAX_PRIMARY_BULLET_MEMBERS).contains(&limit) {
            return Err(Error::InvalidProfile(
                "invalid bullet observation bound/epoch",
            ));
        }
        let source = self.module.global_pointer(BULLET_MANAGER_GLOBAL)?;
        let Some(manager) = source else {
            if self.module.global_pointer(BULLET_MANAGER_GLOBAL)?.is_some() {
                return Err(Error::InvalidProfile(
                    "bullet manager appeared during observation",
                ));
            }
            return Ok(None);
        };
        aligned(manager)?;
        let read = || -> Result<BulletListObservation> {
            let memory = self.module.memory();
            let pool_address = memory.read_u64(manager)?;
            if pool_address != 0 {
                aligned(pool_address)?;
            }
            let head_address = memory.read_u64(at(manager, 8)?)?;
            let extra_allocation_count = memory.read_u32(at(manager, 0x18)?)?;
            let generation_counter = memory.read_u32(at(manager, 0x1c)?)?;
            if extra_allocation_count > 128 {
                return Err(Error::InvalidProfile(
                    "bullet spill count exceeds native capacity",
                ));
            }
            let mut members = Vec::new();
            let mut addresses = BTreeSet::new();
            let mut identifiers = BTreeSet::new();
            let mut spill_count = 0;
            let mut cursor = head_address;
            while cursor != 0 {
                if members.len() == limit || !addresses.insert(cursor) {
                    return Err(Error::InvalidProfile("bullet chain bound or cycle"));
                }
                self.require(cursor, ObjectKind::Bullet)?;
                let native_identifier = memory.read_u64(at(cursor, 8)?)?;
                if (native_identifier >> 28) & 0xf != 3 || !identifiers.insert(native_identifier) {
                    return Err(Error::InvalidProfile(
                        "invalid or duplicate full bullet identity",
                    ));
                }
                match native_identifier as u8 {
                    index @ 0..=127 => {
                        aligned(pool_address)?;
                        let slot = pool_address
                            .checked_add(u64::from(index) * u64::from(BULLET_STRIDE))
                            .ok_or(Error::AddressOverflow)?;
                        if slot != cursor {
                            return Err(Error::InvalidProfile(
                                "bullet fixed slot identity disagrees",
                            ));
                        }
                    }
                    0xfe => {
                        spill_count += 1;
                        if spill_count > extra_allocation_count {
                            return Err(Error::InvalidProfile(
                                "bullet spill chain exceeds allocation count",
                            ));
                        }
                    }
                    _ => return Err(Error::InvalidProfile("unknown bullet slot selector")),
                }
                let state = |offset, kind| -> Result<BulletParameterState> {
                    let address = at(cursor, offset)?;
                    self.require(address, kind)?;
                    Ok(BulletParameterState {
                        address,
                        row_address: memory.read_u64(at(address, 8)?)?,
                        parameter_id: memory.read_u32(at(address, 0x10)?)? as i32,
                    })
                };
                let vector = |offset| -> Result<[u32; 4]> {
                    let mut bytes = [0; 16];
                    memory.read_exact(at(cursor, offset)?, &mut bytes)?;
                    Ok(std::array::from_fn(|i| {
                        u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                    }))
                };
                let member = BulletObservation {
                    identity: BulletIdentity {
                        module_base: self.module.base(),
                        manager_address: manager,
                        address: cursor,
                        native_identifier,
                        epoch,
                    },
                    next_address: memory.read_u64(at(cursor, BULLET_NEXT)?)?,
                    owner_identifier: memory.read_u64(at(cursor, BULLET_OWNER)?)?,
                    targeting_owner_identifier: {
                        let owner = at(cursor, BULLET_TARGETING_OWNER)?;
                        self.require(owner, ObjectKind::BulletTargetingOwner)?;
                        if memory.read_u64(at(owner, 8)?)? != cursor {
                            return Err(Error::InvalidProfile(
                                "bullet targeting owner backlink disagrees",
                            ));
                        }
                        memory.read_u64(at(owner, 0x10)?)?
                    },
                    position_bits: vector(BULLET_POSITION)?,
                    velocity_bits: vector(BULLET_VELOCITY)?,
                    elapsed_step_bits: memory.read_u32(at(cursor, BULLET_ELAPSED)?)?,
                    wait: state(BULLET_WAIT, ObjectKind::BulletWaitState)?,
                    fly: state(BULLET_FLY, ObjectKind::BulletFlyState)?,
                    explosion: state(BULLET_EXPLOSION, ObjectKind::BulletExplosionState)?,
                };
                cursor = member.next_address;
                members.push(member);
            }
            Ok(BulletListObservation {
                manager_address: manager,
                pool_address,
                head_address,
                extra_allocation_count,
                generation_counter,
                members,
            })
        };
        let first = read()?;
        if read()? != first || self.module.global_pointer(BULLET_MANAGER_GLOBAL)? != source {
            return Err(Error::InvalidProfile(
                "bullet pool changed during observation",
            ));
        }
        Ok(Some(first))
    }
    /// Exact full identifier and address, without the native low-word wildcard convention.
    pub fn bullet(
        &self,
        identity: BulletIdentity,
        limit: usize,
    ) -> Result<Option<BulletObservation>> {
        if identity.module_base != self.module.base() {
            return Ok(None);
        }
        Ok(self.bullets(identity.epoch, limit)?.and_then(|list| {
            list.members
                .into_iter()
                .find(|member| member.identity == identity)
        }))
    }
}

#[derive(Debug)]
pub struct BulletVelocityWrite {
    pub source: BulletObservation,
    pub requested_bits: [u32; 3],
    /// XYZ changed together under caller-exclusive access. Fourth lane remains untouched.
    pub stored: bool,
    /// Failure retains the actual store receipt. No automatic rollback or retry.
    pub postflight: Result<BulletObservation>,
}
pub(crate) struct VelocityPlan {
    source: BulletObservation,
    bits: [u32; 3],
    address: usize,
}
pub(crate) fn velocity_plan(
    current: Option<BulletObservation>,
    expected: BulletObservation,
    bits: [u32; 3],
) -> Result<VelocityPlan> {
    if bits
        .into_iter()
        .any(|bits| !f32::from_bits(bits).is_finite())
    {
        return Err(Error::InvalidProfile("nonfinite requested bullet velocity"));
    }
    if current != Some(expected) {
        return Err(Error::InvalidProfile(
            "bullet velocity source unavailable or changed",
        ));
    }
    let address = usize::try_from(at(expected.identity.address, BULLET_VELOCITY)?)
        .map_err(|_| Error::AddressOverflow)?;
    address.checked_add(12).ok_or(Error::AddressOverflow)?;
    if address == 0 || !address.is_multiple_of(4) {
        return Err(Error::InvalidProfile(
            "unaligned bullet velocity destination",
        ));
    }
    Ok(VelocityPlan {
        source: expected,
        bits,
        address,
    })
}
pub(crate) fn commit_velocity(
    plan: VelocityPlan,
    store: impl FnOnce(usize, [u32; 3]),
    observe: impl FnOnce() -> Result<Option<BulletObservation>>,
) -> BulletVelocityWrite {
    let mut expected = plan.source;
    let stored = plan.bits != expected.velocity_bits[..3];
    if stored {
        store(plan.address, plan.bits);
        expected.velocity_bits[..3].copy_from_slice(&plan.bits);
    }
    let postflight = observe().and_then(|current| {
        if current == Some(expected) {
            Ok(expected)
        } else {
            Err(Error::InvalidProfile(
                "bullet changed after velocity replacement",
            ))
        }
    });
    BulletVelocityWrite {
        source: plan.source,
        requested_bits: plan.bits,
        stored,
        postflight,
    }
}
