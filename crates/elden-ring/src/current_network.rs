//! Current experimental packet entry adapter; never installed or invoked automatically.
//! The project workflow validates this statically and with Rust synthetic entries only.
use crate::{
    WW_171, Ww171File,
    network_sessions::{self as n, BroadcastAttempt, NetworkSessionObservation},
};
use shikifsrs_runtime::{
    Error, Result,
    code_patches::OwnedCodePatches,
    memory::{AddressSpace, CurrentProcess},
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

static NETWORK_METADATA: crate::file_metadata::FileMetadataGate =
    crate::file_metadata::FileMetadataGate::new(WW_171);
pub struct CurrentNetworkSessions {
    file: OwnedVerifiedFile,
    base: u64,
    epoch: u64,
    patches: Option<Arc<OwnedCodePatches>>,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentNetworkSessions {
    /// # Safety
    /// Current module/SDK remain resident. Caller supplies every coherent player/holder/session,
    /// participant/transport graph lifetime, synchronization and actual permitted network phase.
    /// Protected identity helper and transport internals remain unaccepted; file/bytes are not a
    /// graph lease or permission for an online test. Owned hook manifest contracts also apply.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero network epoch"));
        }
        let file = OwnedVerifiedFile::verify(bytes, WW_171)?;
        let borrowed = file.borrow()?;
        NETWORK_METADATA.verify(&borrowed, || {
            for (name, table) in [
                (".?AVPlayerNetworkSession@CS@@", n::SESSION_VTABLE),
                (".?AVPlayerSessionHolder@CS@@", n::HOLDER_VTABLE),
            ] {
                let vtables: Vec<_> = borrowed
                    .image()
                    .vtables(name)?
                    .into_iter()
                    .filter(|v| v.object_offset == 0)
                    .collect();
                if vtables.len() != 1 || vtables[0].table_rva != table {
                    return Err(Error::InvalidProfile("network RTTI differs"));
                }
            }
            Ok(())
        })?;
        let this = Self {
            file,
            base,
            epoch,
            patches,
            _thread: PhantomData,
        };
        this.checked(|_| Ok(()))?;
        Ok(this)
    }
    fn checked<T>(
        &self,
        action: impl FnOnce(&crate::objects::GameObjectCatalog<'_, '_, CurrentProcess>) -> Result<T>,
    ) -> Result<T> {
        let borrowed = self.file.borrow()?;
        let file = Ww171File::from_verified(borrowed)?;
        let objects =
            file.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?;
        for range in n::NETWORK_EVIDENCE {
            objects.module.code_address(*range)?;
        }
        action(&objects)
    }
    pub fn observe(&self, player_limit: usize) -> Result<Vec<NetworkSessionObservation>> {
        self.checked(|objects| {
            let Some(players) = objects.world_players(player_limit)? else {
                return Ok(Vec::new());
            };
            let mut result = Vec::new();
            for player in players.set.characters {
                let Some(target) =
                    objects.character_identity(player.character.address, self.epoch)?
                else {
                    continue;
                };
                if let Some(session) = objects.player_network_session(target)? {
                    result.push(session);
                }
            }
            Ok(result)
        })
    }
    fn current(
        &self,
        objects: &crate::objects::GameObjectCatalog<'_, '_, CurrentProcess>,
        expected: NetworkSessionObservation,
        slot: u32,
        rva: u32,
    ) -> Result<usize> {
        if expected.character.epoch != self.epoch
            || objects.player_network_session(expected.character)? != Some(expected)
        {
            return Err(Error::InvalidProfile(
                "network session changed before operation",
            ));
        }
        let memory = objects.module.memory();
        let global = self
            .base
            .checked_add(u64::from(n::TRANSPORT_GLOBAL))
            .ok_or(Error::AddressOverflow)?;
        let transport = memory.read_u64(global)?;
        if transport == 0 || !transport.is_multiple_of(8) || memory.read_u64(global)? != transport {
            return Err(Error::NativeOperationRejected(
                "network transport unavailable",
            ));
        }
        let table = self
            .base
            .checked_add(u64::from(n::SESSION_VTABLE))
            .ok_or(Error::AddressOverflow)?;
        let range = objects.module.checked_virtual_function(table, slot)?;
        if range.rva != rva {
            return Err(Error::InvalidProfile("network virtual entry differs"));
        }
        Ok(objects.module.code_address(range)? as usize)
    }
    /// # Safety
    /// bind's complete exclusive graph and network authority apply for the entire synchronous
    /// native operation. Native entry does not retain the borrowed bytes. No delivery is inferred.
    pub unsafe fn broadcast(
        &self,
        expected: NetworkSessionObservation,
        packet_type: u8,
        bytes: &[u8],
    ) -> Result<BroadcastAttempt> {
        n::validate_capacity(bytes.len())?;
        self.checked(|objects| {
            let address = self.current(objects, expected, 8, n::BROADCAST_RVA)?;
            // SAFETY: Exact virtual/code evidence plus caller's complete graph/native contracts.
            let entry: n::BroadcastFn = unsafe { std::mem::transmute(address) };
            // SAFETY: Checked entry and caller's complete synchronous graph contract.
            unsafe {
                n::broadcast_owned(entry, expected.session_address as usize, packet_type, bytes)
            }
        })
    }
    /// # Safety
    /// bind's complete exclusive graph and native transport authority apply for this call.
    /// Entry copies synchronously only into the provided owned allocation, retaining no pointers.
    pub unsafe fn receive(
        &self,
        expected: NetworkSessionObservation,
        packet_type: u8,
        capacity: usize,
    ) -> Result<Vec<u8>> {
        n::validate_capacity(capacity)?;
        self.checked(|objects| {
            let address = self.current(objects, expected, 0x18, n::RECEIVE_RVA)?;
            // SAFETY: Independently decoded this/output/allocation/type virtual ABI.
            let entry: n::ReceiveFn = unsafe { std::mem::transmute(address) };
            // SAFETY: Exact entry and caller's complete synchronous graph contract.
            unsafe {
                n::receive_owned(
                    entry,
                    expected.session_address as usize,
                    packet_type,
                    capacity,
                )
            }
        })
    }
}
