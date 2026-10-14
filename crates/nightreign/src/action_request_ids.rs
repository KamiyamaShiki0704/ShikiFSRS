//! Experimental copied original/resolved action-request IDs and native latch bytes.
//! These fields are observations, not animation execution or acceptance receipts.
use crate::{
    character_think::CharacterThinkAssociation,
    reader::{NightreignReader, ReaderCore},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionRequestIdState {
    pub association: CharacterThinkAssociation,
    pub request_address: u64,
    pub lookup_address: u64,
    pub raw_request_word_08: i32,
    pub raw_request_flags_cde: [u8; 3],
    pub raw_request_word_10: i32,
    /// Original input copied to Think+c438+14 by RVA 0x289030.
    pub requested_id: i32,
    /// The same setter copies +18; clearing +14 does not clear this word.
    pub retained_requested_id: i32,
    pub raw_request_flags_1c1d: [u8; 2],
    pub raw_lookup_flags_08_0a: [u8; 3],
    /// Owner-adjusted input copied to Think+de90+c by RVA 0x26bdb0.
    pub resolved_id: i32,
    pub retained_resolved_id: i32,
    pub raw_lookup_flag_14: u8,
    /// Native ID-table lookup result at +a48; no gameplay meaning is inferred.
    pub raw_lookup_match_flag: u8,
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// None means no bound ComThink. Checks both block owners and repeated bytes.
    /// Does not invoke ID adjustment, traverse the ID table, latch or clear fields.
    pub fn character_action_request_ids(
        &self,
        character: u64,
    ) -> Result<Option<ActionRequestIdState>> {
        self.core().character_action_request_ids(character)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn character_action_request_ids(
        &self,
        character: u64,
    ) -> Result<Option<ActionRequestIdState>> {
        let Some(association) = self.character_com_think(character)? else {
            return Ok(None);
        };
        let think = association.think_address;
        let request = think.checked_add(0xc438).ok_or(Error::AddressOverflow)?;
        let lookup = think.checked_add(0xde90).ok_or(Error::AddressOverflow)?;
        if self.u64(request, 0)? != think || self.u64(lookup, 0)? != think {
            return Err(Error::InvalidProfile(
                "NR action ID block Think owner differs",
            ));
        }
        let request_data = request.checked_add(8).ok_or(Error::AddressOverflow)?;
        let lookup_data = lookup.checked_add(8).ok_or(Error::AddressOverflow)?;
        let mut req = [0; 22];
        let mut resolved = [0; 13];
        self.memory.read_exact(request_data, &mut req)?;
        self.memory.read_exact(lookup_data, &mut resolved)?;
        let match_address = lookup.checked_add(0xa48).ok_or(Error::AddressOverflow)?;
        let mut matched = [0];
        self.memory.read_exact(match_address, &mut matched)?;
        let mut req2 = [0; 22];
        let mut resolved2 = [0; 13];
        let mut match2 = [0];
        self.memory.read_exact(request_data, &mut req2)?;
        self.memory.read_exact(lookup_data, &mut resolved2)?;
        self.memory.read_exact(match_address, &mut match2)?;
        if req2 != req || resolved2 != resolved || match2 != matched {
            return Err(Error::InvalidProfile(
                "NR action ID state changed during copy",
            ));
        }
        self.unchanged_u64(request, 0, think)?;
        self.unchanged_u64(lookup, 0, think)?;
        if self.character_com_think(character)? != Some(association) {
            return Err(Error::InvalidProfile(
                "NR action ID character association changed",
            ));
        }
        let word = |bytes: &[u8], at: usize| {
            i32::from_le_bytes(bytes[at..at + 4].try_into().expect("word"))
        };
        Ok(Some(ActionRequestIdState {
            association,
            request_address: request,
            lookup_address: lookup,
            raw_request_word_08: word(&req, 0),
            raw_request_flags_cde: req[4..7].try_into().expect("flags"),
            raw_request_word_10: word(&req, 8),
            requested_id: word(&req, 12),
            retained_requested_id: word(&req, 16),
            raw_request_flags_1c1d: req[20..22].try_into().expect("flags"),
            raw_lookup_flags_08_0a: resolved[..3].try_into().expect("flags"),
            resolved_id: word(&resolved, 4),
            retained_resolved_id: word(&resolved, 8),
            raw_lookup_flag_14: resolved[12],
            raw_lookup_match_flag: matched[0],
        }))
    }
}
