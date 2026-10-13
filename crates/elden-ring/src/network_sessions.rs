//! Experimental session storage and bounded packet ABI. No network policy or role inference.
//! Complete transport graph, protected identity helper, phase and lifetime remain caller obligations.
use crate::{
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const NETWORK_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x650c90,
        length: 0x85f,
    },
    CodeRange {
        rva: 0xcc6cd0,
        length: 0x36,
    },
    CodeRange {
        rva: 0xcc66f0,
        length: 0x48,
    },
    CodeRange {
        rva: 0xcc6f90,
        length: 0x75,
    },
    CodeRange {
        rva: 0xcc6810,
        length: 0xe5,
    },
    CodeRange {
        rva: 0xcc6c10,
        length: 0x6e,
    },
    CodeRange {
        rva: 0xcb0930,
        length: 0xa3,
    },
    CodeRange {
        rva: 0xcb0d60,
        length: 0x7a,
    },
    CodeRange {
        rva: 0xcb0a40,
        length: 0x244,
    },
];
pub(crate) const PLAYER_HOLDER_OFFSET: u32 = 0x598;
pub(crate) const HOLDER_NETWORK_OFFSET: u32 = 0x10;
pub(crate) const SESSION_IDENTITY_OFFSET: u32 = 8;
pub(crate) const HOLDER_VTABLE: u32 = 0x2ba1d90;
pub(crate) const SESSION_VTABLE: u32 = 0x2ba1c20;
pub(crate) const BROADCAST_RVA: u32 = 0xcc6c10;
pub(crate) const RECEIVE_RVA: u32 = 0xcc6810;
pub(crate) const TRANSPORT_GLOBAL: u32 = 0x3d7e540;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkSessionObservation {
    pub character: CharacterIdentity,
    pub holder_address: u64,
    pub session_address: u64,
    /// Opaque raw +8 identifier, not an authenticated peer, Steam ID or remote-role proof.
    pub native_identifier: u64,
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copies only the exact network pointer in an exact Player's embedded holder.
    /// Null pointer returns None; mismatched holder/session type rejects rather than casts.
    pub fn player_network_session(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<NetworkSessionObservation>> {
        if self.character_identity(target.address, target.epoch)? != Some(target) {
            return Err(Error::InvalidProfile("network character identity changed"));
        }
        if self.character(target.address)?.map(|v| v.kind) != Some(ObjectKind::Player) {
            return Err(Error::InvalidProfile(
                "network holder requires exact player",
            ));
        }
        let memory = self.module.memory();
        let holder = at(target.address, PLAYER_HOLDER_OFFSET)?;
        if memory.read_u64(holder)? != at(self.module.base(), HOLDER_VTABLE)? {
            return Err(Error::InvalidProfile("network holder type differs"));
        }
        let slot = at(holder, HOLDER_NETWORK_OFFSET)?;
        let session = memory.read_u64(slot)?;
        if session == 0 {
            return Ok(None);
        }
        if !session.is_multiple_of(8)
            || memory.read_u64(session)? != at(self.module.base(), SESSION_VTABLE)?
        {
            return Err(Error::InvalidProfile("network session type differs"));
        }
        let identity_slot = at(session, SESSION_IDENTITY_OFFSET)?;
        let identifier = memory.read_u64(identity_slot)?;
        if memory.read_u64(slot)? != session
            || memory.read_u64(identity_slot)? != identifier
            || memory.read_u64(holder)? != at(self.module.base(), HOLDER_VTABLE)?
            || memory.read_u64(session)? != at(self.module.base(), SESSION_VTABLE)?
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile("network graph changed during copy"));
        }
        Ok(Some(NetworkSessionObservation {
            character: target,
            holder_address: holder,
            session_address: session,
            native_identifier: identifier,
        }))
    }
}
fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

/// Independently decoded allocation: pointer +0, low32 capacity +8; reserved bytes initialized.
#[repr(C)]
pub(crate) struct NativeAllocation {
    buffer: *mut u8,
    capacity: u32,
    reserved: u32,
}
/// Native hidden aggregate output; padding is initialized but never interpreted.
#[repr(C)]
pub(crate) struct NativeReceived {
    buffer: *mut u8,
    capacity: u32,
    reserved0: u32,
    received: u32,
    reserved1: u32,
}
pub(crate) type ReceiveFn = unsafe extern "system" fn(
    usize,
    *mut NativeReceived,
    *const NativeAllocation,
    u32,
) -> *mut NativeReceived;
pub(crate) type BroadcastFn = unsafe extern "system" fn(usize, *const u8, u32, u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BroadcastAttempt {
    pub bytes: u32,
    pub packet_type: u8,
}
pub(crate) fn validate_capacity(capacity: usize) -> Result<u32> {
    if !(1..=65536).contains(&capacity) {
        return Err(Error::InvalidProfile(
            "network packet bound must be 1..=65536",
        ));
    }
    Ok(capacity as u32)
}
/// # Safety
/// Caller supplies the coherent exclusive session/transport graph and synchronous native ABI.
/// Entry does not retain bytes; a void return is only an attempted call, never delivery.
pub(crate) unsafe fn broadcast_owned(
    entry: BroadcastFn,
    session: usize,
    packet_type: u8,
    bytes: &[u8],
) -> Result<BroadcastAttempt> {
    let length = validate_capacity(bytes.len())?;
    // SAFETY: Caller establishes the graph and exact this/data/length/type ABI.
    unsafe {
        entry(session, bytes.as_ptr(), length, u32::from(packet_type));
    }
    Ok(BroadcastAttempt {
        bytes: length,
        packet_type,
    })
}
/// # Safety
/// Entry/session/transport graph are coherent, exclusive and resident; native copies synchronously
/// into only this call's allocation and writes the complete output before returning.
pub(crate) unsafe fn receive_owned(
    entry: ReceiveFn,
    session: usize,
    packet_type: u8,
    capacity: usize,
) -> Result<Vec<u8>> {
    let capacity32 = validate_capacity(capacity)?;
    let mut bytes = vec![0; capacity];
    let allocation = NativeAllocation {
        buffer: bytes.as_mut_ptr(),
        capacity: capacity32,
        reserved: 0,
    };
    let mut output = NativeReceived {
        buffer: std::ptr::null_mut(),
        capacity: 0,
        reserved0: 0,
        received: 0,
        reserved1: 0,
    };
    // SAFETY: Caller supplies the decoded this/output/allocation/type ABI and complete graph.
    let returned = unsafe { entry(session, &mut output, &allocation, u32::from(packet_type)) };
    if !std::ptr::eq(returned, &output) {
        return Err(Error::InvalidProfile("receive output address differs"));
    }
    if output.received == 0 {
        return Ok(Vec::new());
    }
    if output.buffer != allocation.buffer
        || output.capacity != capacity32
        || output.received > capacity32
    {
        return Err(Error::InvalidProfile(
            "receive escaped owned buffer or exceeded capacity",
        ));
    }
    bytes.truncate(output.received as usize);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "system" fn synthetic(
        session: usize,
        out: *mut NativeReceived,
        input: *const NativeAllocation,
        packet: u32,
    ) -> *mut NativeReceived {
        assert_eq!(session, 77);
        assert_eq!(packet, 253);
        // SAFETY: receive_owned passes its original owned allocation and output for this call.
        unsafe {
            assert_eq!((*input).capacity, 256);
            std::ptr::copy_nonoverlapping([1, 2, 3, 4].as_ptr(), (*input).buffer, 4);
            (*out).buffer = (*input).buffer;
            (*out).capacity = (*input).capacity;
            (*out).received = 4;
            (*out).reserved1 = u32::MAX;
        }
        out
    }
    unsafe extern "system" fn broadcast_synthetic(
        session: usize,
        bytes: *const u8,
        length: u32,
        packet: u32,
    ) {
        assert_eq!((session, length, packet), (77, 2, 203));
        // SAFETY: broadcast_owned provides this call's two original borrowed bytes.
        let observed = unsafe { std::slice::from_raw_parts(bytes, length as usize) };
        assert_eq!(observed, &[4, 5]);
    }
    #[test]
    fn broadcast_abi_keeps_type_in_fourth_argument_and_reports_only_attempt() {
        // SAFETY: Original Rust synthetic entry and owned input only, no network operations.
        let receipt = unsafe { broadcast_owned(broadcast_synthetic, 77, 203, &[4, 5]) }.unwrap();
        assert_eq!(
            receipt,
            BroadcastAttempt {
                bytes: 2,
                packet_type: 203
            }
        );
        // SAFETY: Invalid bounds reject before synthetic/native entry invocation.
        let rejected = unsafe { broadcast_owned(broadcast_synthetic, 0, 0, &[]) };
        assert!(rejected.is_err());
    }
    #[test]
    fn receive_abi_is_this_output_input_type_and_does_not_read_output_padding() {
        // SAFETY: Only the original Rust synthetic entry, never a game/network call.
        let observed = unsafe { receive_owned(synthetic, 77, 253, 256) }.unwrap();
        assert_eq!(observed, vec![1, 2, 3, 4]);
    }
    unsafe extern "system" fn corrupt(
        session: usize,
        out: *mut NativeReceived,
        input: *const NativeAllocation,
        _: u32,
    ) -> *mut NativeReceived {
        // SAFETY: Original live owned input/output, written only within their declared structs.
        unsafe {
            (*out).buffer = (*input).buffer;
            (*out).capacity = (*input).capacity;
            (*out).received = if session == 1 { 257 } else { 1 };
            if session == 2 {
                (*out).buffer = std::ptr::dangling_mut();
            }
            if session == 3 {
                (*out).capacity = 255;
            }
        }
        if session == 4 {
            std::ptr::null_mut()
        } else {
            out
        }
    }
    #[test]
    fn receive_rejects_external_buffer_oversize_capacity_and_wrong_return_without_copying() {
        for scenario in 1..=4 {
            // SAFETY: Synthetic entry only; escaped pointer is rejected without dereferencing it.
            let rejected = unsafe { receive_owned(corrupt, scenario, 1, 256) };
            assert!(rejected.is_err());
        }
        for capacity in [0, 65537, usize::MAX] {
            // SAFETY: Bound rejection occurs before invoking the entry.
            let rejected = unsafe { receive_owned(corrupt, 0, 1, capacity) };
            assert!(rejected.is_err());
        }
    }
}
