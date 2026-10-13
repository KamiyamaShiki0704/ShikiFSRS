//! Copied native handle decoding and ride links. Neither observation pins allocations.
use crate::{
    character_modules::{CharacterModule, CharacterModuleObservation},
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const HANDLE_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x508970,
        length: 0xdd,
    },
    CodeRange {
        rva: 0x62f640,
        length: 0x53,
    },
    CodeRange {
        rva: 0x62f5d0,
        length: 0x3a,
    },
    CodeRange {
        rva: 0x1d2f70,
        length: 0x26,
    },
    CodeRange {
        rva: 0x4751b0,
        length: 0x5a,
    },
    CodeRange {
        rva: 0x478460,
        length: 0x8,
    },
    CodeRange {
        rva: 0x476e20,
        length: 0x63,
    },
    CodeRange {
        rva: 0x493c50,
        length: 0x13,
    },
    CodeRange {
        rva: 0x477ce0,
        length: 0xe,
    },
    CodeRange {
        rva: 0x477cf0,
        length: 0x4,
    },
    CodeRange {
        rva: 0x477d30,
        length: 0x5,
    },
    CodeRange {
        rva: 0x474e20,
        length: 0x50,
    },
    CodeRange {
        rva: 0x650c90,
        length: 0x85f,
    },
    CodeRange {
        rva: 0x7170b0,
        length: 0x1264,
    },
    CodeRange {
        rva: 0x714c00,
        length: 0x6c,
    },
    CodeRange {
        rva: 0x3f4270,
        length: 0x3f,
    },
    CodeRange {
        rva: 0x3e6e30,
        length: 0x99f,
    },
    CodeRange {
        rva: 0x3f59c0,
        length: 0x2dc,
    },
    CodeRange {
        rva: 0x3fb010,
        length: 0x263,
    },
    CodeRange {
        rva: 0x3fc4a0,
        length: 0x1d1,
    },
];
const DECODER_TABLE: u32 = 0x3b37920;
const DECODER_FALLBACK: u32 = 0x2a7c450;
const DECODER_STRIDE: u32 = 0x18;
const DECODER_COUNT: u32 = 9;
const WORLD_SET_OFFSET: u32 = 0x1ded8;
const WORLD_SET_COUNT: u32 = 0xc5;
const RIDE_NODE_OFFSET: u32 = 0x10;
const RIDE_IDENTIFIER_OFFSET: u32 = 0x28;
const RIDE_FLAG_OFFSET: u32 = 0x30;
const NODE_STATE_OFFSET: u32 = 0x50;
const NODE_FLAG_OFFSET: u32 = 0x70;
const RIDE_ROLE_TABLE: u32 = 0x2a3d520;
const PRIMARY_TARGET_OFFSET: u32 = 0x6b0;
const LAST_EFFECT_SOURCE_OFFSET: u32 = 0x180;

/// Copied source handle recorded by the native effect path for a qualifying PARAM row.
/// This is not a universal last attacker, damage event, death cause or allocation lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastEffectSourceObservation {
    pub character: CharacterIdentity,
    pub handle: NativeCharacterHandle,
}
#[derive(Debug)]
pub struct EffectSourceClear {
    pub source: LastEffectSourceObservation,
    pub changed: bool,
    /// A failed postflight retains the actual store; no retry or restore on Drop.
    pub postflight: Result<LastEffectSourceObservation>,
}
pub(crate) struct EffectSourceClearPlan {
    source: LastEffectSourceObservation,
    destination: Option<usize>,
}
pub(crate) fn effect_source_clear_plan(
    current: Option<LastEffectSourceObservation>,
    expected: LastEffectSourceObservation,
) -> Result<EffectSourceClearPlan> {
    let target = expected.character;
    if current != Some(expected)
        || target.epoch == 0
        || target.world_address == 0
        || !target.world_address.is_multiple_of(8)
        || target.address == 0
        || !target.address.is_multiple_of(8)
    {
        return Err(Error::NativeOperationRejected(
            "effect source clear scope/source superseded",
        ));
    }
    let destination = if expected.handle.0 == u64::MAX {
        None
    } else {
        Some(
            usize::try_from(at(target.address, LAST_EFFECT_SOURCE_OFFSET)?)
                .map_err(|_| Error::AddressOverflow)?,
        )
    };
    Ok(EffectSourceClearPlan {
        source: expected,
        destination,
    })
}
pub(crate) fn commit_effect_source_clear(
    plan: EffectSourceClearPlan,
    store: impl FnOnce(usize, u64),
    observe: impl FnOnce() -> Result<LastEffectSourceObservation>,
) -> EffectSourceClear {
    let changed = plan.destination.is_some();
    let postflight = if let Some(destination) = plan.destination {
        store(destination, u64::MAX);
        let expected = LastEffectSourceObservation {
            handle: NativeCharacterHandle(u64::MAX),
            ..plan.source
        };
        observe().and_then(|after| {
            if after == expected {
                Ok(after)
            } else {
                Err(Error::InvalidProfile("effect source changed after clear"))
            }
        })
    } else {
        Ok(plan.source)
    };
    EffectSourceClear {
        source: plan.source,
        changed,
        postflight,
    }
}

/// Copied primary PlayerIns target selection. No target allocation is dereferenced or pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimaryPlayerTargetObservation {
    pub player: CharacterIdentity,
    pub handle: NativeCharacterHandle,
}
#[derive(Debug)]
pub struct PrimaryPlayerTargetClear {
    pub source: PrimaryPlayerTargetObservation,
    pub target: CharacterIdentity,
    pub changed: bool,
    /// If changed, the store already occurred even when this fails. No restore/retry on Drop.
    pub postflight: Result<PrimaryPlayerTargetObservation>,
}
pub(crate) struct PrimaryTargetClearPlan {
    source: PrimaryPlayerTargetObservation,
    target: CharacterIdentity,
    destination: Option<usize>,
}
pub(crate) fn primary_target_clear_plan(
    current: Option<PrimaryPlayerTargetObservation>,
    expected: PrimaryPlayerTargetObservation,
    target_current: Option<CharacterIdentity>,
    target: CharacterIdentity,
) -> Result<PrimaryTargetClearPlan> {
    if current != Some(expected)
        || target_current != Some(target)
        || expected.player.epoch == 0
        || target.epoch != expected.player.epoch
        || target.world_address != expected.player.world_address
        || target.address == expected.player.address
        || expected.player.world_address == 0
        || !expected.player.world_address.is_multiple_of(8)
        || expected.player.address == 0
        || !expected.player.address.is_multiple_of(8)
        || target.address == 0
        || !target.address.is_multiple_of(8)
    {
        return Err(Error::NativeOperationRejected(
            "primary target clear scope/source superseded",
        ));
    }
    let destination =
        if !expected.handle.is_absent() && expected.handle.0 == target.native_identifier {
            Some(
                usize::try_from(at(expected.player.address, PRIMARY_TARGET_OFFSET)?)
                    .map_err(|_| Error::AddressOverflow)?,
            )
        } else {
            None
        };
    Ok(PrimaryTargetClearPlan {
        source: expected,
        target,
        destination,
    })
}
pub(crate) fn commit_primary_target_clear(
    plan: PrimaryTargetClearPlan,
    store: impl FnOnce(usize, u64),
    observe: impl FnOnce() -> Result<PrimaryPlayerTargetObservation>,
) -> PrimaryPlayerTargetClear {
    let changed = plan.destination.is_some();
    let postflight = if let Some(destination) = plan.destination {
        store(destination, u64::MAX);
        let expected = PrimaryPlayerTargetObservation {
            handle: NativeCharacterHandle(u64::MAX),
            ..plan.source
        };
        observe().and_then(|after| {
            if after == expected {
                Ok(after)
            } else {
                Err(Error::InvalidProfile("primary target changed after clear"))
            }
        })
    } else {
        Ok(plan.source)
    };
    PrimaryPlayerTargetClear {
        source: plan.source,
        target: plan.target,
        changed,
        postflight,
    }
}
pub(crate) unsafe fn store_native_handle(address: usize, value: u64) {
    // SAFETY: Caller supplies exclusive pinned initialized aligned eight-byte destination.
    unsafe {
        (address as *mut u64).write(value);
    }
}

/// Full native value. No invented generation, lifetime or network role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeCharacterHandle(pub u64);

impl NativeCharacterHandle {
    pub fn kind(self) -> u8 {
        ((self.0 >> 28) & 15) as u8
    }
    pub fn is_absent(self) -> bool {
        self.0 as u32 == u32::MAX
    }
    /// Native area decoder maps an all-zero upper word to the absent sentinel.
    pub fn area(self) -> Option<u32> {
        let upper = (self.0 >> 32) as u32;
        (upper != 0 && upper != u32::MAX).then_some(upper)
    }
    pub fn decode(self, record: HandleDecoderRecord) -> DecodedCharacterHandle {
        DecodedCharacterHandle {
            handle: self,
            set_index: ((self.0 >> (record.set_shift & 63)) & u64::from(record.set_mask)) as u32,
            ordinal: (self.0 & u64::from(record.ordinal_mask)) as u32,
        }
    }
}

/// Selected runtime record, not the writable table's file initialization values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandleDecoderRecord {
    pub set_mask: u32,
    pub set_shift: u32,
    pub ordinal_mask: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedCharacterHandle {
    pub handle: NativeCharacterHandle,
    pub set_index: u32,
    pub ordinal: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandleDecodeObservation {
    pub record_address: u64,
    pub record: HandleDecoderRecord,
    pub decoded: DecodedCharacterHandle,
}

/// Related handle and raw node state. A nonzero state is the native predicate used
/// by 478460; it does not establish the active mount, rider or transition outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterRideObservation {
    pub module: CharacterModuleObservation,
    pub related_handle: NativeCharacterHandle,
    pub raw_flag: u8,
    pub node_address: Option<u64>,
    pub node_state: Option<u32>,
}

/// Copied predicates of the native relation. Role numbers are preserved without
/// claiming a rider/mount name or completed gameplay transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RidePhaseObservation {
    pub ride: CharacterRideObservation,
    pub node_flag: u8,
    pub native_role: u32,
    pub native_active: bool,
    pub native_endpoint_one_query: bool,
}

fn decode_ride_role(bytes: &[u8], state: u32) -> Result<u32> {
    if bytes.len() != 40 {
        return Err(Error::InvalidProfile("truncated ride role table"));
    }
    let mut roles = [0; 10];
    for (role, record) in roles.iter_mut().zip(bytes.chunks_exact(4)) {
        *role = u32::from_le_bytes(record.try_into().expect("checked width"));
    }
    if roles != [0, 1, 2, 1, 2, 1, 2, 1, 2, 0] {
        return Err(Error::InvalidProfile(
            "ride role table differs from native evidence",
        ));
    }
    roles
        .get(state as usize)
        .copied()
        .ok_or(Error::InvalidProfile("unknown ride node state"))
}

fn endpoint_one_query(role: u32, state: u32, flag: u8) -> bool {
    role == 1 && (flag != 0 || matches!(state, 1 | 3))
}

fn at(base: u64, offset: u32) -> Result<u64> {
    base.checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy full native source storage without resolving or dereferencing that source.
    /// Unknown handles and noncanonical absent values remain intact.
    pub fn last_effect_source(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<LastEffectSourceObservation>> {
        let current = self.character_identity(target.address, target.epoch)?;
        let Some(character) = current else {
            return Ok(None);
        };
        if character != target || target.epoch == 0 {
            return Err(Error::InvalidProfile("effect source character superseded"));
        }
        let address = at(target.address, LAST_EFFECT_SOURCE_OFFSET)?;
        let handle = NativeCharacterHandle(self.module.memory().read_u64(address)?);
        if self.module.memory().read_u64(address)? != handle.0
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile(
                "effect source changed during observation",
            ));
        }
        Ok(Some(LastEffectSourceObservation { character, handle }))
    }
    /// Copy the independently proved primary-player selection, distinct from debug overrides.
    /// Unknown/nonempty handles remain opaque; no resolver, engine guard or network role is implied.
    pub fn primary_player_target(
        &self,
        epoch: u64,
    ) -> Result<Option<PrimaryPlayerTargetObservation>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero primary target epoch"));
        }
        let Some(player) = self.primary_player_identity(epoch)? else {
            return Ok(None);
        };
        let address = at(player.address, PRIMARY_TARGET_OFFSET)?;
        let handle = NativeCharacterHandle(self.module.memory().read_u64(address)?);
        if self.module.memory().read_u64(address)? != handle.0
            || self.primary_player_identity(epoch)? != Some(player)
        {
            return Err(Error::InvalidProfile(
                "primary target selection changed during observation",
            ));
        }
        Ok(Some(PrimaryPlayerTargetObservation { player, handle }))
    }
    /// Reproduce the checked ordinary native state predicates without calling
    /// the engine. Missing module/node returns None; unknown states fail before
    /// table indexing, unlike the native unchecked leaf. No relationship lease.
    pub fn character_ride_phase(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<RidePhaseObservation>> {
        let Some(ride) = self.character_ride(target)? else {
            return Ok(None);
        };
        let (Some(node), Some(state)) = (ride.node_address, ride.node_state) else {
            return Ok(None);
        };
        let image = self.module.file().image();
        let section = image
            .section_at(RIDE_ROLE_TABLE)
            .filter(|section| !section.is_writable() && !section.is_executable())
            .ok_or(Error::InvalidProfile(
                "ride role table is not readonly data",
            ))?;
        if u64::from(RIDE_ROLE_TABLE) + 40 > u64::from(section.rva) + u64::from(section.raw_size) {
            return Err(Error::InvalidProfile("ride role table crosses its section"));
        }
        let bytes = image.read_rva(RIDE_ROLE_TABLE, 40)?;
        let role = decode_ride_role(bytes, state)?;
        let table_address = at(self.module.base(), RIDE_ROLE_TABLE)?;
        let mut live = [0; 40];
        self.module.memory().read_exact(table_address, &mut live)?;
        if live != bytes {
            return Err(Error::ImageMismatch {
                rva: RIDE_ROLE_TABLE,
            });
        }
        let flag_address = at(node, NODE_FLAG_OFFSET)?;
        let flag = self.module.memory().read_u8(flag_address)?;
        if self.module.memory().read_u8(flag_address)? != flag
            || self.character_ride(target)? != Some(ride)
        {
            return Err(Error::InvalidProfile(
                "ride phase changed during observation",
            ));
        }
        self.module.memory().read_exact(table_address, &mut live)?;
        if live != bytes {
            return Err(Error::ImageMismatch {
                rva: RIDE_ROLE_TABLE,
            });
        }
        Ok(Some(RidePhaseObservation {
            ride,
            node_flag: flag,
            native_role: role,
            native_active: state != 0,
            native_endpoint_one_query: endpoint_one_query(role, state, flag),
        }))
    }
    pub fn decode_character_handle(
        &self,
        handle: NativeCharacterHandle,
    ) -> Result<HandleDecodeObservation> {
        let kind = u32::from(handle.kind());
        let rva = if kind < DECODER_COUNT {
            DECODER_TABLE + kind * DECODER_STRIDE
        } else {
            DECODER_FALLBACK
        };
        let address = at(self.module.base(), rva)?;
        let read = || -> Result<HandleDecoderRecord> {
            let memory = self.module.memory();
            Ok(HandleDecoderRecord {
                set_mask: memory.read_u32(address)?,
                set_shift: memory.read_u32(at(address, 4)?)?,
                ordinal_mask: memory.read_u32(at(address, 8)?)?,
            })
        };
        let record = read()?;
        if read()? != record {
            return Err(Error::InvalidProfile(
                "handle decoder changed during observation",
            ));
        }
        Ok(HandleDecodeObservation {
            record_address: address,
            record,
            decoded: handle.decode(record),
        })
    }

    /// Global-set branch only. Area-backed routing is explicitly unsupported here.
    /// Requires exact ChrSet and full identifier agreement; no native calls or lease.
    pub fn global_character_handle(
        &self,
        handle: NativeCharacterHandle,
        epoch: u64,
        limit: usize,
    ) -> Result<Option<CharacterIdentity>> {
        if epoch == 0 || !(1..=65536).contains(&limit) {
            return Err(Error::InvalidProfile(
                "invalid handle lookup epoch or limit",
            ));
        }
        if handle.is_absent() || handle.kind() != 1 {
            return Ok(None);
        }
        if handle.area().is_some() {
            return Err(Error::InvalidProfile(
                "area-backed handle lookup requires a separate route",
            ));
        }
        let Some(world) = self.world()? else {
            return Ok(None);
        };
        let decoded = self.decode_character_handle(handle)?;
        if decoded.decoded.set_index >= WORLD_SET_COUNT {
            return Ok(None);
        }
        let slot = at(
            world.address,
            WORLD_SET_OFFSET + decoded.decoded.set_index * 8,
        )?;
        let address = self.module.memory().read_u64(slot)?;
        let set = self.character_set(address, limit)?;
        let candidate = set.as_ref().and_then(|set| {
            set.characters
                .iter()
                .find(|entry| entry.ordinal == decoded.decoded.ordinal)
        });
        let result = match candidate {
            Some(entry) if entry.native_identifier == handle.0 => {
                self.character_identity(entry.character.address, epoch)?
            }
            Some(_) => {
                return Err(Error::InvalidProfile(
                    "handle lookup full identifier mismatch",
                ));
            }
            None => None,
        };
        if self.world()? != Some(world)
            || self.decode_character_handle(handle)? != decoded
            || self.module.memory().read_u64(slot)? != address
            || self.character_set(address, limit)? != set
        {
            return Err(Error::InvalidProfile(
                "handle lookup changed during observation",
            ));
        }
        if let Some(identity) = result
            && self.character_identity(identity.address, epoch)? != Some(identity)
        {
            return Err(Error::InvalidProfile(
                "handle target changed during observation",
            ));
        }
        Ok(result)
    }

    pub fn character_ride(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterRideObservation>> {
        let Some(module) = self.character_module(target, CharacterModule::Ride)? else {
            return Ok(None);
        };
        let read = || -> Result<CharacterRideObservation> {
            let memory = self.module.memory();
            let node = memory.read_u64(at(module.address, RIDE_NODE_OFFSET)?)?;
            let typed = self.require(node, ObjectKind::CharacterRideNode)?;
            let state = typed
                .map(|_| memory.read_u32(at(node, NODE_STATE_OFFSET)?))
                .transpose()?;
            Ok(CharacterRideObservation {
                module,
                related_handle: NativeCharacterHandle(
                    memory.read_u64(at(module.address, RIDE_IDENTIFIER_OFFSET)?)?,
                ),
                raw_flag: memory.read_u8(at(module.address, RIDE_FLAG_OFFSET)?)?,
                node_address: typed.map(|object| object.address),
                node_state: state,
            })
        };
        let result = read()?;
        if read()? != result
            || self.character_module(target, CharacterModule::Ride)? != Some(module)
        {
            return Err(Error::InvalidProfile(
                "ride links changed during observation",
            ));
        }
        Ok(Some(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effect_source_clear_uses_full_raw_word_and_preserves_neighbors() {
        #[repr(align(8))]
        struct Storage([u8; 0x200]);
        let mut storage = Storage([0x5a; 0x200]);
        for raw in [u64::MAX, 0x12345678ffffffff, 0, 0x1234567810000003] {
            storage.0[0x180..0x188].copy_from_slice(&raw.to_le_bytes());
            let before = storage.0;
            let source = LastEffectSourceObservation {
                character: CharacterIdentity {
                    address: storage.0.as_mut_ptr() as u64,
                    ..selected_target()
                },
                handle: NativeCharacterHandle(raw),
            };
            let plan = effect_source_clear_plan(Some(source), source).unwrap();
            let calls = std::cell::Cell::new(0);
            let receipt = commit_effect_source_clear(
                plan,
                |address, value| {
                    calls.set(calls.get() + 1);
                    // SAFETY: Exclusive aligned owned initialized test bytes, no native game calls.
                    unsafe { store_native_handle(address, value) }
                },
                || {
                    Ok(LastEffectSourceObservation {
                        handle: NativeCharacterHandle(u64::MAX),
                        ..source
                    })
                },
            );
            assert_eq!(receipt.changed, raw != u64::MAX);
            assert_eq!(calls.get(), u32::from(raw != u64::MAX));
            assert!(receipt.postflight.is_ok());
            let mut expected = before;
            expected[0x180..0x188].copy_from_slice(&u64::MAX.to_le_bytes());
            assert_eq!(storage.0, expected);
        }
    }
    #[test]
    fn effect_source_clear_rejects_superseded_graph_and_retains_failed_store() {
        let source = LastEffectSourceObservation {
            character: selected_target(),
            handle: NativeCharacterHandle(0x1234567810000003),
        };
        assert!(effect_source_clear_plan(None, source).is_err());
        for mode in 0..9 {
            let mut newer = source;
            match mode {
                0 => newer.handle.0 ^= 1 << 32,
                1 => newer.character.native_identifier ^= 1 << 32,
                2 => newer.character.world_address += 8,
                3 => newer.character.epoch += 1,
                4 => newer.character.address += 8,
                5 => newer.character.address = 0,
                6 => newer.character.world_address = 1,
                7 => newer.character.address = 1,
                _ => newer.character.epoch = 0,
            }
            assert!(effect_source_clear_plan(Some(newer), source).is_err());
            if mode >= 5 {
                assert!(effect_source_clear_plan(Some(newer), newer).is_err());
            }
        }
        let calls = std::cell::Cell::new(0);
        for failed_read in [false, true] {
            let plan = effect_source_clear_plan(Some(source), source).unwrap();
            let receipt = commit_effect_source_clear(
                plan,
                |address, value| {
                    assert_eq!(address, 0x3180);
                    assert_eq!(value, u64::MAX);
                    calls.set(calls.get() + 1);
                },
                || {
                    if failed_read {
                        Err(Error::InvalidProfile("lost target"))
                    } else {
                        Ok(source)
                    }
                },
            );
            assert!(receipt.changed);
            assert!(receipt.postflight.is_err());
            assert_eq!(receipt.source, source);
        }
        assert_eq!(calls.get(), 2);
    }
    fn primary_source(address: u64, handle: u64) -> PrimaryPlayerTargetObservation {
        PrimaryPlayerTargetObservation {
            player: CharacterIdentity {
                address,
                world_address: 0x2000,
                native_identifier: 2,
                epoch: 4,
            },
            handle: NativeCharacterHandle(handle),
        }
    }
    fn selected_target() -> CharacterIdentity {
        CharacterIdentity {
            address: 0x3000,
            world_address: 0x2000,
            native_identifier: 0x1234567810000003,
            epoch: 4,
        }
    }
    #[test]
    fn exact_full_target_clear_preserves_every_neighbor_and_different_or_absent_selection() {
        #[repr(align(8))]
        struct Storage([u8; 0x740]);
        let mut storage = Storage([0x5a; 0x740]);
        let address = storage.0.as_mut_ptr() as u64;
        let target = selected_target();
        for handle in [
            target.native_identifier,
            u64::MAX,
            0x12345678ffffffff,
            target.native_identifier ^ (1 << 32),
            0,
        ] {
            storage.0[0x6b0..0x6b8].copy_from_slice(&handle.to_le_bytes());
            let original = storage.0;
            let source = primary_source(address, handle);
            let plan =
                primary_target_clear_plan(Some(source), source, Some(target), target).unwrap();
            let change = handle == target.native_identifier;
            let mut expected = source;
            if change {
                expected.handle = NativeCharacterHandle(u64::MAX);
            }
            let calls = std::cell::Cell::new(0);
            let receipt = commit_primary_target_clear(
                plan,
                |addr, value| {
                    calls.set(calls.get() + 1);
                    // SAFETY: Original aligned exclusive initialized test array, no game memory.
                    unsafe {
                        store_native_handle(addr, value);
                    }
                },
                || Ok(expected),
            );
            assert_eq!(receipt.changed, change);
            assert!(receipt.postflight.is_ok());
            assert_eq!(calls.get(), u32::from(change));
            let mut bytes = original;
            if change {
                bytes[0x6b0..0x6b8].copy_from_slice(&u64::MAX.to_le_bytes());
            }
            assert_eq!(storage.0, bytes);
        }
    }
    #[test]
    fn target_clear_rejects_foreign_scope_and_preserves_failed_store_obligation() {
        let target = selected_target();
        let source = primary_source(0x1000, target.native_identifier);
        assert!(primary_target_clear_plan(None, source, Some(target), target).is_err());
        assert!(primary_target_clear_plan(Some(source), source, None, target).is_err());
        for mode in 0..8 {
            let mut foreign = target;
            match mode {
                0 => foreign.epoch += 1,
                1 => foreign.world_address += 8,
                2 => foreign.native_identifier += 1,
                3 => foreign.address += 8,
                4 => foreign.address = source.player.address,
                5 => foreign.address = 0,
                6 => foreign.address = 1,
                _ => foreign.epoch = 0,
            }
            assert!(
                primary_target_clear_plan(Some(source), source, Some(foreign), target).is_err()
            );
            if !matches!(mode, 2 | 3) {
                assert!(
                    primary_target_clear_plan(Some(source), source, Some(foreign), foreign)
                        .is_err()
                );
            }
        }
        let mut newer = source;
        newer.handle = NativeCharacterHandle(0);
        assert!(primary_target_clear_plan(Some(newer), source, Some(target), target).is_err());
        let plan = primary_target_clear_plan(Some(source), source, Some(target), target).unwrap();
        let calls = std::cell::Cell::new(0);
        {
            let receipt = commit_primary_target_clear(
                plan,
                |addr, value| {
                    assert_eq!(addr, 0x16b0);
                    assert_eq!(value, u64::MAX);
                    calls.set(calls.get() + 1);
                },
                || Err(Error::InvalidProfile("lost primary")),
            );
            assert!(receipt.changed);
            assert!(receipt.postflight.is_err());
            assert_eq!(receipt.target, target);
            assert_eq!(receipt.source, source);
        }
        assert_eq!(calls.get(), 1);
        let plan = primary_target_clear_plan(Some(source), source, Some(target), target).unwrap();
        let receipt = commit_primary_target_clear(plan, |_, _| {}, || Ok(source));
        assert!(receipt.postflight.is_err());
    }
    #[test]
    fn native_ride_predicates_cover_all_known_states_flags_and_reject_unbounded_indices() {
        let roles = [0u32, 1, 2, 1, 2, 1, 2, 1, 2, 0];
        let bytes: Vec<_> = roles.iter().flat_map(|role| role.to_le_bytes()).collect();
        for (state, role) in roles.into_iter().enumerate() {
            assert_eq!(decode_ride_role(&bytes, state as u32).unwrap(), role);
            for flag in 0..=u8::MAX {
                let expected = matches!(state, 1 | 3) || (matches!(state, 5 | 7) && flag != 0);
                assert_eq!(endpoint_one_query(role, state as u32, flag), expected);
            }
        }
        for state in [10, 100, u32::MAX] {
            assert!(decode_ride_role(&bytes, state).is_err());
        }
        assert!(decode_ride_role(&bytes[..39], 0).is_err());
        let mut changed = bytes;
        changed[4] = 3;
        assert!(decode_ride_role(&changed, 0).is_err());
    }
}
