//! Experimental read-only prediction of the original owner ID-offset lookup.
//! A resource pointer is a lookup observation, not animation readiness or execution.
use crate::{
    character_think::{COM_OWNER_TABLE, CharacterThinkAssociation},
    reader::{NightreignReader, ReaderCore},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionIdResolutionOutcome {
    OwnerContextMissing,
    NoAvailableResource,
    ResourceFound,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionIdCandidate {
    pub offset_slot: u8,
    pub offset: i32,
    pub id_bits: u32,
    pub record_address: Option<u64>,
    pub resource_address: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionIdResolutionPlan {
    pub association: CharacterThinkAssociation,
    pub requested_id_bits: u32,
    pub resolved_id_bits: u32,
    pub native_offset_bits: u32,
    pub owner_context_address: u64,
    pub resolver_address: Option<u64>,
    pub offsets: Option<[i32; 5]>,
    pub catalog_address: Option<u64>,
    pub candidates: Vec<ActionIdCandidate>,
    pub outcome: ActionIdResolutionOutcome,
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// Uses the original owner slot-170 default mode, offsets 4..0 and bounded
    /// 72-byte resource records. Preserves modular u32 ID arithmetic. No callbacks,
    /// resource pointer dereferences, request writes or ownership transfer occur.
    pub fn character_action_id_resolution_plan(
        &self,
        character: u64,
        id_bits: u32,
        record_limit: usize,
    ) -> Result<Option<ActionIdResolutionPlan>> {
        self.core()
            .character_action_id_resolution_plan(character, id_bits, record_limit)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn character_action_id_resolution_plan(
        &self,
        character: u64,
        id_bits: u32,
        record_limit: usize,
    ) -> Result<Option<ActionIdResolutionPlan>> {
        if !(1..=4096).contains(&record_limit) {
            return Err(Error::InvalidProfile(
                "NR action resource record limit must be 1..=4096",
            ));
        }
        let Some(association) = self.character_com_think(character)? else {
            return Ok(None);
        };
        let slot = self
            .base
            .checked_add(COM_OWNER_TABLE as u64 + 0x170)
            .ok_or(Error::AddressOverflow)?;
        let target = self
            .base
            .checked_add(0x282380)
            .ok_or(Error::AddressOverflow)?;
        if self.u64(slot, 0)? != target {
            return Err(Error::InvalidProfile("NR owner ID adjustment slot differs"));
        }
        let owner = association.owner_interface_address;
        let context = self.u64(owner, 0x10)?;
        let mut plan = ActionIdResolutionPlan {
            association,
            requested_id_bits: id_bits,
            resolved_id_bits: id_bits.wrapping_sub(1),
            native_offset_bits: u32::MAX,
            owner_context_address: context,
            resolver_address: None,
            offsets: None,
            catalog_address: None,
            candidates: Vec::new(),
            outcome: ActionIdResolutionOutcome::OwnerContextMissing,
        };
        if context != 0 {
            if !context.is_multiple_of(8) || self.u64(context, 0x10)? != character {
                return Err(Error::InvalidProfile(
                    "NR owner ID context character differs",
                ));
            }
            let resolver = self.u64(context, 0x20)?;
            if resolver == 0 || !resolver.is_multiple_of(8) {
                return Err(Error::InvalidProfile("invalid NR action ID resolver"));
            }
            let offset_address = resolver.checked_add(8).ok_or(Error::AddressOverflow)?;
            let mut offset_bytes = [0; 20];
            self.memory.read_exact(offset_address, &mut offset_bytes)?;
            let offsets: [i32; 5] = std::array::from_fn(|j| {
                i32::from_le_bytes(offset_bytes[j * 4..j * 4 + 4].try_into().expect("offset"))
            });
            let object = self.u64(resolver, 0)?;
            let mut catalog = None;
            let mut links = None;
            let mut rows = Vec::new();
            let mut metadata = None;
            if object != 0 {
                let holder = self.u64(object, 0x78)?;
                if holder == 0 {
                    return Err(Error::InvalidProfile("null NR action ID resource holder"));
                }
                let table = self.u64(holder, 0x60)?;
                if table == 0 {
                    return Err(Error::InvalidProfile("null NR action ID resource catalog"));
                }
                let storage = self.u64(table, 0xa0)?;
                let count = self.u32(table, 0x98)?;
                if storage != 0 {
                    if !storage.is_multiple_of(8) || count as usize > record_limit {
                        return Err(Error::InvalidProfile(
                            "NR action resource records exceed bound or alignment",
                        ));
                    }
                    for j in 0..count {
                        let address = storage
                            .checked_add(j as u64 * 72)
                            .ok_or(Error::AddressOverflow)?;
                        let mut raw = [0; 16];
                        self.memory.read_exact(address, &mut raw)?;
                        let key = u32::from_le_bytes(raw[..4].try_into().expect("key"));
                        let resource = u64::from_le_bytes(raw[8..16].try_into().expect("resource"));
                        if rows
                            .last()
                            .is_some_and(|row: &(u32, u64, u64, [u8; 16])| row.0 >= key)
                        {
                            return Err(Error::InvalidProfile(
                                "NR action resource keys not unsigned sorted",
                            ));
                        }
                        rows.push((key, address, resource, raw));
                    }
                }
                catalog = Some(table);
                links = Some((object, holder, table));
                metadata = Some((table, storage, count));
            }
            plan.resolver_address = Some(resolver);
            plan.offsets = Some(offsets);
            plan.catalog_address = catalog;
            plan.native_offset_bits = 0;
            plan.resolved_id_bits = id_bits;
            plan.outcome = ActionIdResolutionOutcome::NoAvailableResource;
            for j in (0..5).rev() {
                if offsets[j] < 0 {
                    continue;
                }
                let candidate = id_bits.wrapping_add(offsets[j] as u32);
                let row = rows
                    .binary_search_by_key(&candidate, |r| r.0)
                    .ok()
                    .map(|index| rows[index]);
                let resource = row.and_then(|r| (r.2 != 0).then_some(r.2));
                plan.candidates.push(ActionIdCandidate {
                    offset_slot: j as u8,
                    offset: offsets[j],
                    id_bits: candidate,
                    record_address: row.map(|r| r.1),
                    resource_address: resource,
                });
                if resource.is_some() {
                    plan.native_offset_bits = offsets[j] as u32;
                    plan.resolved_id_bits = candidate;
                    plan.outcome = ActionIdResolutionOutcome::ResourceFound;
                    break;
                }
            }
            for (_, address, _, raw) in rows {
                let mut repeated = [0; 16];
                self.memory.read_exact(address, &mut repeated)?;
                if repeated != raw {
                    return Err(Error::InvalidProfile("NR action resource record changed"));
                }
            }
            if let Some((table, storage, count)) = metadata {
                self.unchanged_u64(table, 0xa0, storage)?;
                if self.u32(table, 0x98)? != count {
                    return Err(Error::InvalidProfile("NR action resource count changed"));
                }
            }
            if let Some((object, holder, table)) = links {
                self.unchanged_u64(object, 0x78, holder)?;
                self.unchanged_u64(holder, 0x60, table)?;
            }
            let mut repeated = [0; 20];
            self.memory.read_exact(offset_address, &mut repeated)?;
            if repeated != offset_bytes {
                return Err(Error::InvalidProfile("NR action ID offsets changed"));
            }
            self.unchanged_u64(resolver, 0, object)?;
            self.unchanged_u64(context, 0x20, resolver)?;
            self.unchanged_u64(context, 0x10, character)?;
        }
        self.unchanged_u64(owner, 0x10, context)?;
        self.unchanged_u64(slot, 0, target)?;
        if self.character_com_think(character)? != Some(association) {
            return Err(Error::InvalidProfile(
                "NR ID resolution actor association changed",
            ));
        }
        Ok(Some(plan))
    }
}
