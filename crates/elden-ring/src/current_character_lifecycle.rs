//! Explicit native detach, destructor(flags=0), original-allocator free. Offline experimental.
use crate::{
    WW_171, Ww171File,
    character_events::EVENT_EVIDENCE,
    character_sets::CharacterRegistrationObservation,
    npc_player_data::{NpcPlayerDataPoolObservation, PlayerNpcDataObservation},
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::{
    Error, Result,
    code_patches::OwnedCodePatches,
    memory::{AddressSpace, CurrentProcess},
    native_tree::{TreeKeys, U32PointerTree},
    profile::{CodeRange, OwnedVerifiedFile},
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

pub(crate) const LIFECYCLE_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x494f10,
        length: 0x9a,
    },
    CodeRange {
        rva: 0x495ab0,
        length: 0x67,
    },
    CodeRange {
        rva: 0x495b20,
        length: 0x315,
    },
    CodeRange {
        rva: 0x3c3810,
        length: 0x11,
    },
    CodeRange {
        rva: 0x3c3830,
        length: 0x12,
    },
    CodeRange {
        rva: 0x495aa0,
        length: 0x3,
    },
    CodeRange {
        rva: 0xe1c080,
        length: 0x36,
    },
    CodeRange {
        rva: 0x1ec7c50,
        length: 0x4e,
    },
    CodeRange {
        rva: 0x3e81f0,
        length: 0x34,
    },
    CodeRange {
        rva: 0x4cf6b0,
        length: 0x34,
    },
    CodeRange {
        rva: 0x651a00,
        length: 0x34,
    },
];
const SET_GET_SLOT: u32 = 0x8;
const SET_HANDLE_REMOVE_SLOT: u32 = 0x50;
const CHARACTER_DESTRUCTOR_SLOT: u32 = 0x8;
const ALLOCATOR_FREE_SLOT: u32 = 0x68;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterRetirementStage {
    Prepared,
    DetachAttempted,
    Detached,
    DestroyAttempted,
    Destructed,
    FreeAttempted,
    FreeCallReturned,
    Quarantined,
}
/// Unique operation record, never cloned and never destroys/frees on Drop. It is not a pin.
pub struct PreparedCharacterRetirement {
    source: CharacterRegistrationObservation,
    base: u64,
    file_sha256: [u8; 32],
    owner: u64,
    owner_table: u64,
    free_code: CodeRange,
    destructor_code: CodeRange,
    npc_data: Option<PlayerNpcDataObservation>,
    stage: CharacterRetirementStage,
    _thread: PhantomData<Rc<()>>,
}
impl PreparedCharacterRetirement {
    pub fn source(&self) -> CharacterRegistrationObservation {
        self.source
    }
    pub fn owner_address(&self) -> u64 {
        self.owner
    }
    pub fn stage(&self) -> CharacterRetirementStage {
        self.stage
    }
    pub fn npc_data_before(&self) -> Option<&PlayerNpcDataObservation> {
        self.npc_data.as_ref()
    }
    /// Retain/quarantine storage; this never frees it or establishes that it was leaked safely.
    pub fn quarantine(&mut self) {
        self.stage = CharacterRetirementStage::Quarantined;
    }
}
#[derive(Debug)]
pub struct NativeCharacterDetach {
    pub returned_address: u64,
    pub postflight: Result<()>,
}
#[derive(Debug)]
pub struct NativeCharacterDestruction {
    pub returned_address: u64,
    /// This is a post-call code/world/allocator check; it never reads the destructed character.
    pub postflight: Result<()>,
    /// For NPC Players, copies the surviving pool only. The released data is never read.
    /// Failure retains the actual native attempt and forbids release_storage/retry.
    pub npc_pool_after: Option<Result<NpcPlayerDataPoolObservation>>,
}
pub struct NativeCharacterOwnerQuery {
    pub returned_owner: u64,
    /// Querying the allocator can initialize native allocator machinery. Failed preparation
    /// retains the call result; it does not grant ownership or permit a detach/free.
    pub prepared: Result<PreparedCharacterRetirement>,
}
/// Partial known-reader observation, never a destructor or free authorization.
#[derive(Debug)]
pub struct CharacterRetirementObservation {
    pub references: crate::world_references::WorldReferenceObservation,
    pub tasks: crate::character_tasks::CharacterTasksObservation,
}
pub struct CurrentCharacterLifecycle {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentCharacterLifecycle {
    /// Observe only a still-live detached ticket on its originating thread/epoch.
    /// Does not read destructed/freed storage, call native code or acknowledge cancellation.
    pub fn observe_retirement(
        &self,
        ticket: &PreparedCharacterRetirement,
        limit: usize,
    ) -> Result<CharacterRetirementObservation> {
        self.session(ticket)?;
        require_stage(ticket, CharacterRetirementStage::Detached)?;
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let objects =
            file.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?;
        Ok(CharacterRetirementObservation {
            references: objects
                .world_character_references(ticket.source.character, limit)?
                .ok_or(Error::NativeOperationRejected(
                    "retirement world unavailable",
                ))?,
            tasks: objects.character_tasks(ticket.source.character)?.ok_or(
                Error::NativeOperationRejected("retirement tasks unavailable"),
            )?,
        })
    }
    /// # Safety
    /// Caller pins world/set/character/entry/allocator and all resources reachable from native
    /// detach, model-group/event callbacks, destructors and allocator functions; proves exclusive
    /// lifetime ownership for eventual destruction/free and appropriate engine phase. Exclude
    /// every native reader, task callback, teardown/ABA, aliases and unload as required per stage.
    /// Hooks/DLLs remain resident. No read, empty counter or SDK ticket acquires these guarantees.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero character retirement epoch"));
        }
        let file = OwnedVerifiedFile::verify(bytes, WW_171)?;
        let view = Ww171File::from_verified(file.borrow()?)?;
        let memory = CurrentProcess;
        let objects = view.bind_objects_with_patches(&memory, base, patches.as_deref())?;
        for range in LIFECYCLE_EVIDENCE {
            objects.module.code_address(*range)?;
        }
        Ok(Self {
            file,
            patches,
            base,
            epoch,
            _thread: PhantomData,
        })
    }
    fn session(&self, ticket: &PreparedCharacterRetirement) -> Result<()> {
        if ticket.base != self.base
            || ticket.file_sha256 != WW_171.sha256
            || ticket.source.character.epoch != self.epoch
        {
            return Err(Error::InvalidProfile("foreign retirement session/epoch"));
        }
        Ok(())
    }
    /// Obtain the allocator and exact scalar destructor/free bindings while still registered.
    /// # Safety
    /// Bind's full graph and ownership contract holds. Caller proves this allocation uniquely
    /// belongs to this operation, not the primary player or somebody else's registered object.
    /// Preparation invokes native owner lookup once; no detach/destructor/free occurs here.
    pub unsafe fn prepare(
        &self,
        expected: CharacterRegistrationObservation,
        limit: usize,
    ) -> Result<NativeCharacterOwnerQuery> {
        if expected.character.epoch != self.epoch {
            return Err(Error::InvalidProfile("stale retirement preparation"));
        }
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        for range in LIFECYCLE_EVIDENCE {
            objects.module.code_address(*range)?;
        }
        if objects.character_registration(expected.character, expected.set_address, limit)?
            != Some(expected)
            || objects
                .primary_player()?
                .is_some_and(|p| p.address == expected.character.address)
        {
            return Err(Error::NativeOperationRejected(
                "unavailable or primary retirement target",
            ));
        }
        check_set_slots(&objects, expected.set_address)?;
        let kind = objects
            .character(expected.character.address)?
            .ok_or(Error::InvalidProfile("missing retirement target"))?
            .kind;
        let destructor_code = LIFECYCLE_EVIDENCE[match kind {
            ObjectKind::Character => 8,
            ObjectKind::Enemy => 9,
            ObjectKind::Player => 10,
            _ => return Err(Error::InvalidProfile("unsupported destructor type")),
        }];
        let npc_data = if kind == ObjectKind::Player {
            Some(
                objects
                    .npc_player_data_for_character(expected.character)?
                    .ok_or(Error::NativeOperationRejected(
                        "retirement PlayerIns data is outside NPC pool",
                    ))?,
            )
        } else {
            None
        };
        check_slot(
            &objects,
            expected.character.address,
            CHARACTER_DESTRUCTOR_SLOT,
            destructor_code,
        )?;
        let entry = objects.module.code_address(LIFECYCLE_EVIDENCE[6])?;
        // SAFETY: Complete e1c080 RCX allocation/RAX allocator ABI and checked fallback code.
        let query: OwnerQuery = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Caller owns live allocation, complete allocator graph and permitted phase.
        let returned_owner = unsafe { query(expected.character.address) };
        let prepared = (|| {
            if returned_owner == 0 || !returned_owner.is_multiple_of(8) {
                return Err(Error::InvalidProfile(
                    "invalid original character allocator",
                ));
            }
            let owner_table = memory.read_u64(returned_owner)?;
            let free_code = objects
                .module
                .checked_virtual_function(owner_table, ALLOCATOR_FREE_SLOT)?;
            if objects.character_registration(expected.character, expected.set_address, limit)?
                != Some(expected)
            {
                return Err(Error::InvalidProfile(
                    "retirement source changed after owner lookup",
                ));
            }
            check_slot(
                &objects,
                expected.character.address,
                CHARACTER_DESTRUCTOR_SLOT,
                destructor_code,
            )?;
            if let Some(before) = &npc_data {
                check_retirement_npc_source(
                    Some(before),
                    objects
                        .npc_player_data_for_character(expected.character)?
                        .as_ref(),
                )?;
            }
            Ok(PreparedCharacterRetirement {
                source: expected,
                base: self.base,
                file_sha256: WW_171.sha256,
                owner: returned_owner,
                owner_table,
                free_code,
                destructor_code,
                npc_data,
                stage: CharacterRetirementStage::Prepared,
                _thread: PhantomData,
            })
        })();
        Ok(NativeCharacterOwnerQuery {
            returned_owner,
            prepared,
        })
    }
    /// # Safety
    /// Caller owns the registered entry, has stopped producers and initiated required task
    /// cancellations, preserves all allocation/resource lifetimes and uses the permitted phase.
    /// Native detach can remove event groups; a failed postflight needs reconciliation, not retry.
    pub unsafe fn detach(
        &self,
        ticket: &mut PreparedCharacterRetirement,
        limit: usize,
    ) -> Result<NativeCharacterDetach> {
        self.session(ticket)?;
        require_stage(ticket, CharacterRetirementStage::Prepared)?;
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        for range in LIFECYCLE_EVIDENCE {
            objects.module.code_address(*range)?;
        }
        let source = ticket.source;
        if objects.character_registration(source.character, source.set_address, limit)?
            != Some(source)
        {
            return Err(Error::NativeOperationRejected(
                "retirement entry superseded",
            ));
        }
        check_set_slots(&objects, source.set_address)?;
        let kind = objects
            .character(source.character.address)?
            .ok_or(Error::InvalidProfile("missing detach target"))?
            .kind;
        check_slot(
            &objects,
            source.character.address,
            0x300,
            EVENT_EVIDENCE[usize::from(kind == ObjectKind::Player)],
        )?;
        check_allocator(&objects, ticket)?;
        if let Some(source) = &ticket.npc_data {
            check_retirement_npc_source(
                Some(source),
                objects
                    .npc_player_data_for_character(source.character)?
                    .as_ref(),
            )?;
        }
        let entry = objects.module.code_address(LIFECYCLE_EVIDENCE[0])?;
        // SAFETY: Complete native RCX set/EDX ordinal/RAX character ABI.
        let function: Detach = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Caller supplies live original entry, full detach graph and exclusive phase.
        Ok(unsafe {
            invoke_detach(function, ticket, || {
                if memory.read_u64(source.entry_address)? != 0
                    || objects
                        .character_set(source.set_address, limit)?
                        .is_none_or(|set| {
                            set.characters
                                .iter()
                                .any(|x| x.character.address == source.character.address)
                        })
                {
                    return Err(Error::InvalidProfile("native detach not confirmed"));
                }
                for (offset, keys) in [(0x28, TreeKeys::Unique), (0x40, TreeKeys::Repeated)] {
                    let tree = source
                        .set_address
                        .checked_add(offset)
                        .ok_or(Error::AddressOverflow)?;
                    if U32PointerTree::new(&memory, tree, limit)?
                        .snapshot(keys)?
                        .iter()
                        .any(|x| x.value == source.entry_address)
                    {
                        return Err(Error::InvalidProfile(
                            "detached entry still has event mappings",
                        ));
                    }
                }
                if objects.character_identity(source.character.address, source.character.epoch)?
                    != Some(source.character)
                {
                    return Err(Error::InvalidProfile("detached character identity changed"));
                }
                Ok(())
            })
        })
    }
    /// Invoke the scalar destructor with flags=0, preserving allocator-owned storage.
    /// # Safety
    /// Caller independently proves complete quiescence of every engine reader/callback and
    /// unique destruction authority, not merely empty known collections or two quiet frames.
    /// Full destructor/resource graph remains live, teardown is excluded. Player destruction
    /// itself releases the linked NPC data; never separately free it. SDK checks the surviving
    /// pool without reading the released data, and never writes the detached character entry.
    pub unsafe fn destroy_without_free(
        &self,
        ticket: &mut PreparedCharacterRetirement,
        limit: usize,
    ) -> Result<NativeCharacterDestruction> {
        self.session(ticket)?;
        require_stage(ticket, CharacterRetirementStage::Detached)?;
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        if objects.character_identity(ticket.source.character.address, self.epoch)?
            != Some(ticket.source.character)
        {
            return Err(Error::NativeOperationRejected("destruction target changed"));
        }
        let references = objects
            .world_character_references(ticket.source.character, limit)?
            .ok_or(Error::NativeOperationRejected("missing retirement world"))?;
        let tasks = objects
            .character_tasks(ticket.source.character)?
            .ok_or(Error::NativeOperationRejected("missing retired tasks"))?;
        if references.primary_slot_matches
            || references.collections.iter().any(|x| x.matches != 0)
            || tasks.tasks.iter().any(|x| x.registration.is_some())
        {
            return Err(Error::NativeOperationRejected(
                "known character readers/tasks remain",
            ));
        }
        check_allocator(&objects, ticket)?;
        let npc_before = if let Some(source) = &ticket.npc_data {
            let current = objects.npc_player_data_for_character(source.character)?;
            check_retirement_npc_source(Some(source), current.as_ref())?;
            current
        } else {
            None
        };
        check_slot(
            &objects,
            ticket.source.character.address,
            CHARACTER_DESTRUCTOR_SLOT,
            ticket.destructor_code,
        )?;
        let entry = objects.module.code_address(ticket.destructor_code)?;
        // SAFETY: Complete scalar-destructor RCX allocation/EDX flags/RAX allocation ABI.
        let function: Destructor = unsafe { std::mem::transmute(entry as usize) };
        let owner = ticket.owner;
        let table = ticket.owner_table;
        let free_code = ticket.free_code;
        let world = ticket.source.character.world_address;
        let mut npc_pool_after = None;
        // SAFETY: Caller uniquely owns already detached, fully drained allocation/resource graph.
        let mut receipt = unsafe {
            invoke_destroy(function, ticket, || {
                // Never read the destructed character or possibly reused set entry.
                if let Some(before) = &npc_before {
                    npc_pool_after =
                        Some(objects.npc_player_data_pool_after_release(&before.pool, before.slot));
                    npc_pool_after
                        .as_ref()
                        .expect("just observed pool")
                        .as_ref()
                        .map_err(|_| {
                            Error::InvalidProfile(
                                "Player destructor NPC data release not confirmed",
                            )
                        })?;
                }
                if objects.world()?.is_none_or(|x| x.address != world)
                    || memory.read_u64(owner)? != table
                    || !same_code(
                        objects
                            .module
                            .checked_virtual_function(table, ALLOCATOR_FREE_SLOT)?,
                        free_code,
                    )
                {
                    return Err(Error::InvalidProfile(
                        "post-destruction world/allocator changed",
                    ));
                }
                Ok(())
            })
        };
        receipt.npc_pool_after = npc_pool_after;
        Ok(receipt)
    }
    /// Release storage exactly once through the original prepared allocator.
    /// # Safety
    /// Caller proves successful completed destructor and exclusive allocation ownership,
    /// all foreign frames/readers drained and allocator/full free graph live. No game object
    /// or entry is read after destruction/free. A returned native free is not DLL unload proof.
    pub unsafe fn release_storage(&self, ticket: &mut PreparedCharacterRetirement) -> Result<()> {
        self.session(ticket)?;
        require_stage(ticket, CharacterRetirementStage::Destructed)?;
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        if objects
            .world()?
            .is_none_or(|x| x.address != ticket.source.character.world_address)
        {
            return Err(Error::NativeOperationRejected(
                "retired allocation world changed",
            ));
        }
        check_allocator(&objects, ticket)?;
        let entry = objects.module.code_address(ticket.free_code)?;
        // SAFETY: Exact allocator slot68 RCX allocator/RDX destructed allocation void ABI.
        let function: Free = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Caller supplies unique destructed allocation and original live allocator.
        unsafe { invoke_free(function, ticket) };
        Ok(())
    }
}
fn require_stage(
    ticket: &PreparedCharacterRetirement,
    expected: CharacterRetirementStage,
) -> Result<()> {
    if ticket.stage != expected {
        return Err(Error::NativeOperationRejected(
            "invalid character retirement stage",
        ));
    }
    Ok(())
}
fn check_retirement_npc_source(
    expected: Option<&PlayerNpcDataObservation>,
    current: Option<&PlayerNpcDataObservation>,
) -> Result<()> {
    match (expected, current) {
        (None, None) => Ok(()),
        (Some(old), Some(now))
            if old.character == now.character
                && old.slot == now.slot
                && old.pool.module_base == now.pool.module_base
                && old.pool.epoch == now.pool.epoch
                && old.pool.manager_address == now.pool.manager_address
                && old.pool.slots_address == now.pool.slots_address =>
        {
            Ok(())
        }
        _ => Err(Error::NativeOperationRejected(
            "retirement NPC data association superseded",
        )),
    }
}
fn check_slot<M: AddressSpace>(
    objects: &GameObjectCatalog<'_, '_, M>,
    object: u64,
    slot: u32,
    expected: CodeRange,
) -> Result<()> {
    let table = objects.module.memory().read_u64(object)?;
    let address = table
        .checked_add(u64::from(slot))
        .ok_or(Error::AddressOverflow)?;
    if objects.module.memory().read_u64(address)? != objects.module.code_address(expected)? {
        return Err(Error::InvalidProfile(
            "character retirement virtual slot changed",
        ));
    }
    Ok(())
}
fn check_set_slots<M: AddressSpace>(
    objects: &GameObjectCatalog<'_, '_, M>,
    set: u64,
) -> Result<()> {
    check_slot(
        objects,
        set,
        SET_GET_SLOT,
        crate::character_sets::SET_EVIDENCE[3],
    )?;
    check_slot(objects, set, SET_HANDLE_REMOVE_SLOT, LIFECYCLE_EVIDENCE[5])
}
fn same_code(a: CodeRange, b: CodeRange) -> bool {
    (a.rva, a.length) == (b.rva, b.length)
}
fn check_allocator<M: AddressSpace>(
    objects: &GameObjectCatalog<'_, '_, M>,
    ticket: &PreparedCharacterRetirement,
) -> Result<()> {
    if objects.module.memory().read_u64(ticket.owner)? != ticket.owner_table
        || !same_code(
            objects
                .module
                .checked_virtual_function(ticket.owner_table, ALLOCATOR_FREE_SLOT)?,
            ticket.free_code,
        )
    {
        return Err(Error::InvalidProfile(
            "original character allocator changed",
        ));
    }
    Ok(())
}
type OwnerQuery = unsafe extern "system" fn(u64) -> u64;
type Detach = unsafe extern "system" fn(u64, u32) -> u64;
type Destructor = unsafe extern "system" fn(u64, u32) -> u64;
type Free = unsafe extern "system" fn(u64, u64);
unsafe fn invoke_detach(
    function: Detach,
    ticket: &mut PreparedCharacterRetirement,
    postflight: impl FnOnce() -> Result<()>,
) -> NativeCharacterDetach {
    ticket.stage = CharacterRetirementStage::DetachAttempted;
    // SAFETY: Caller guarantees original live set/ordinal and exclusive detach phase.
    let returned_address = unsafe { function(ticket.source.set_address, ticket.source.ordinal) };
    let postflight = postflight().and(if returned_address == ticket.source.character.address {
        Ok(())
    } else {
        Err(Error::InvalidProfile("detach returned another allocation"))
    });
    if postflight.is_ok() {
        ticket.stage = CharacterRetirementStage::Detached;
    }
    NativeCharacterDetach {
        returned_address,
        postflight,
    }
}
unsafe fn invoke_destroy(
    function: Destructor,
    ticket: &mut PreparedCharacterRetirement,
    postflight: impl FnOnce() -> Result<()>,
) -> NativeCharacterDestruction {
    ticket.stage = CharacterRetirementStage::DestroyAttempted;
    // SAFETY: Caller supplies uniquely owned drained allocation; flags0 prevents CRT delete.
    let returned_address = unsafe { function(ticket.source.character.address, 0) };
    let postflight = postflight().and(if returned_address == ticket.source.character.address {
        Ok(())
    } else {
        Err(Error::InvalidProfile(
            "destructor returned another allocation",
        ))
    });
    if postflight.is_ok() {
        ticket.stage = CharacterRetirementStage::Destructed;
    }
    NativeCharacterDestruction {
        returned_address,
        postflight,
        npc_pool_after: None,
    }
}
unsafe fn invoke_free(function: Free, ticket: &mut PreparedCharacterRetirement) {
    ticket.stage = CharacterRetirementStage::FreeAttempted;
    // SAFETY: Caller supplies uniquely owned destructed allocation and original live allocator.
    unsafe { function(ticket.owner, ticket.source.character.address) };
    ticket.stage = CharacterRetirementStage::FreeCallReturned;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character_sets::CharacterIdentity;
    use std::cell::{Cell, RefCell};
    struct Mock {
        calls: RefCell<Vec<&'static str>>,
        wrong: Cell<bool>,
        untouched_entry: Cell<u64>,
    }
    unsafe extern "system" fn detach(set: u64, ordinal: u32) -> u64 {
        // SAFETY: Tests supply owned Mock, copied ordinal.
        let mock = unsafe { &*(set as *const Mock) };
        assert_eq!(ordinal, 2);
        mock.calls.borrow_mut().push("detach");
        if mock.wrong.get() { 0xdeadbeef } else { set }
    }
    unsafe extern "system" fn destroy(character: u64, flags: u32) -> u64 {
        // SAFETY: Tests supply owned Mock; no actual destruction.
        let mock = unsafe { &*(character as *const Mock) };
        assert_eq!(flags, 0);
        mock.calls.borrow_mut().push("destructor0");
        if mock.wrong.get() {
            0xdeadbeef
        } else {
            character
        }
    }
    unsafe extern "system" fn free(owner: u64, character: u64) {
        // SAFETY: Tests supply owned Mock; this records, never frees Rust storage.
        let mock = unsafe { &*(owner as *const Mock) };
        assert_eq!(owner, character);
        mock.calls.borrow_mut().push("original-allocator-free");
    }
    fn mock() -> Mock {
        Mock {
            calls: RefCell::new(vec![]),
            wrong: Cell::new(false),
            untouched_entry: Cell::new(0x7777),
        }
    }
    fn make_ticket(mock: &Mock) -> PreparedCharacterRetirement {
        let address = mock as *const Mock as u64;
        PreparedCharacterRetirement {
            source: CharacterRegistrationObservation {
                character: CharacterIdentity {
                    address,
                    world_address: 0x2000,
                    native_identifier: 0xfedcba9876543210,
                    epoch: 9,
                },
                set_address: address,
                entry_address: 0x3000,
                ordinal: 2,
            },
            base: 0x140000000,
            file_sha256: WW_171.sha256,
            owner: address,
            owner_table: 0x4000,
            free_code: CodeRange {
                rva: 0x1000,
                length: 0x20,
            },
            destructor_code: LIFECYCLE_EVIDENCE[9],
            npc_data: None,
            stage: CharacterRetirementStage::Prepared,
            _thread: PhantomData,
        }
    }
    #[test]
    fn npc_retirement_keeps_owned_association_while_other_units_may_change_between_phases() {
        use crate::npc_player_data::NpcPlayerDataSlot;
        let mock = mock();
        let ticket = make_ticket(&mock);
        let slot = NpcPlayerDataSlot {
            index: 2,
            data_address: 0x7000,
            native_identifier: 10000002,
        };
        let mut pool = NpcPlayerDataPoolObservation {
            module_base: ticket.base,
            manager_address: 0x5000,
            slots_address: 0x6000,
            epoch: 9,
            slots: [None; 40],
        };
        pool.slots[2] = Some(slot);
        let before = PlayerNpcDataObservation {
            character: ticket.source.character,
            pool,
            slot,
        };
        let mut now = before.clone();
        now.pool.slots[0] = Some(NpcPlayerDataSlot {
            index: 0,
            data_address: 0x8000,
            native_identifier: 10000000,
        });
        check_retirement_npc_source(Some(&before), Some(&now)).unwrap();
        for field in 0..8 {
            let mut changed = now.clone();
            match field {
                0 => changed.character.native_identifier ^= 1,
                1 => changed.slot.index += 1,
                2 => changed.slot.data_address += 8,
                3 => changed.slot.native_identifier += 1,
                4 => changed.pool.module_base += 8,
                5 => changed.pool.manager_address += 8,
                6 => changed.pool.slots_address += 8,
                _ => changed.pool.epoch += 1,
            }
            assert!(
                check_retirement_npc_source(Some(&before), Some(&changed)).is_err(),
                "field {field}"
            );
        }
        assert!(check_retirement_npc_source(Some(&before), None).is_err());
        assert!(check_retirement_npc_source(None, Some(&now)).is_err());
        check_retirement_npc_source(None, None).unwrap();
        {
            let mut ticket = make_ticket(&mock);
            ticket.npc_data = Some(before);
            ticket.stage = CharacterRetirementStage::Detached;
            // SAFETY: Original owned ABI mock, deliberately uncertain pool observation; no game.
            let receipt = unsafe {
                invoke_destroy(destroy, &mut ticket, || {
                    Err(Error::InvalidProfile("NPC pool release not confirmed"))
                })
            };
            assert_eq!(receipt.returned_address, ticket.source.character.address);
            assert!(receipt.postflight.is_err());
            assert_eq!(ticket.stage(), CharacterRetirementStage::DestroyAttempted);
            assert!(require_stage(&ticket, CharacterRetirementStage::Detached).is_err());
            assert!(require_stage(&ticket, CharacterRetirementStage::Destructed).is_err());
        }
        assert_eq!(mock.calls.borrow().as_slice(), ["destructor0"]);
    }
    #[test]
    fn three_native_abis_use_original_allocator_flags_zero_and_order_once() {
        let mock = mock();
        let mut ticket = make_ticket(&mock);
        assert!(require_stage(&ticket, CharacterRetirementStage::Prepared).is_ok());
        // SAFETY: Original self-authored ABI and owned Mock; no game operations.
        let detached = unsafe { invoke_detach(detach, &mut ticket, || Ok(())) };
        assert!(detached.postflight.is_ok());
        assert_eq!(ticket.stage(), CharacterRetirementStage::Detached);
        assert!(require_stage(&ticket, CharacterRetirementStage::Prepared).is_err());
        // SAFETY: Mock only records destructor0; owned storage remains alive.
        let destroyed = unsafe { invoke_destroy(destroy, &mut ticket, || Ok(())) };
        assert!(destroyed.postflight.is_ok());
        assert_eq!(ticket.stage(), CharacterRetirementStage::Destructed);
        assert!(require_stage(&ticket, CharacterRetirementStage::Detached).is_err());
        // SAFETY: Mock only records allocator/target; does not release Rust storage.
        unsafe { invoke_free(free, &mut ticket) };
        assert_eq!(ticket.stage(), CharacterRetirementStage::FreeCallReturned);
        assert!(require_stage(&ticket, CharacterRetirementStage::Destructed).is_err());
        assert_eq!(
            *mock.calls.borrow(),
            vec!["detach", "destructor0", "original-allocator-free"]
        );
        assert_eq!(mock.untouched_entry.get(), 0x7777);
        drop(Box::new(ticket));
        assert_eq!(mock.calls.borrow().len(), 3);
    }
    #[test]
    fn failed_postflight_and_foreign_return_leave_attempts_nonretryable() {
        for wrong in [false, true] {
            let mock = mock();
            mock.wrong.set(wrong);
            let mut ticket = make_ticket(&mock);
            // SAFETY: Owned original Mock with either return tag; no game calls.
            let call = unsafe {
                invoke_detach(detach, &mut ticket, || {
                    if wrong {
                        Ok(())
                    } else {
                        Err(Error::InvalidProfile("after"))
                    }
                })
            };
            assert!(call.postflight.is_err());
            assert_eq!(ticket.stage(), CharacterRetirementStage::DetachAttempted);
            assert!(require_stage(&ticket, CharacterRetirementStage::Prepared).is_err());
            assert!(require_stage(&ticket, CharacterRetirementStage::Detached).is_err());
            ticket.quarantine();
            drop(Box::new(ticket));
            assert_eq!(*mock.calls.borrow(), vec!["detach"]);
            let mut ticket = make_ticket(&mock);
            ticket.stage = CharacterRetirementStage::Detached;
            // SAFETY: Owned Mock; destructor only records flags and returns a tag.
            let call = unsafe {
                invoke_destroy(destroy, &mut ticket, || {
                    if wrong {
                        Ok(())
                    } else {
                        Err(Error::InvalidProfile("after"))
                    }
                })
            };
            assert!(call.postflight.is_err());
            assert_eq!(ticket.stage(), CharacterRetirementStage::DestroyAttempted);
            assert!(require_stage(&ticket, CharacterRetirementStage::Detached).is_err());
            assert!(require_stage(&ticket, CharacterRetirementStage::Destructed).is_err());
        }
    }
    #[test]
    fn abandoned_prepared_detached_destructed_and_quarantined_records_do_nothing_on_drop() {
        let mock = mock();
        for stage in [
            CharacterRetirementStage::Prepared,
            CharacterRetirementStage::Detached,
            CharacterRetirementStage::Destructed,
            CharacterRetirementStage::Quarantined,
        ] {
            let mut ticket = make_ticket(&mock);
            ticket.stage = stage;
            drop(Box::new(ticket));
        }
        assert!(mock.calls.borrow().is_empty());
        assert_eq!(mock.untouched_entry.get(), 0x7777);
    }
}
