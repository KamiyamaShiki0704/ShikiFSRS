//! Experimental native map registration and explicit alias removal. No ownership or auto retry.
use crate::{
    WW_171, Ww171File,
    character_events::{CharacterEventMappingObservation, EVENT_EVIDENCE, EVENT_GETTER_SLOT},
    objects::GameObjectCatalog,
};
use shikifsrs_runtime::{
    Error, Result,
    code_patches::OwnedCodePatches,
    memory::{AddressSpace, CurrentProcess},
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

#[derive(Debug)]
pub enum EventEntityInsertion {
    AlreadyPresent(CharacterEventMappingObservation),
    /// Native entry ran once. An Err after it does not undo insertion or permit retry.
    Submitted {
        before: CharacterEventMappingObservation,
        after: Result<CharacterEventMappingObservation>,
    },
}
#[derive(Debug)]
pub struct EventGroupRegistration {
    pub before: CharacterEventMappingObservation,
    /// Complete observed memberships of this entry, including model-group IDs.
    /// Does not prove event-manager callbacks completed or EMEVD can find the character.
    pub after: Result<CharacterEventMappingObservation>,
}
#[derive(Debug)]
pub struct EventEntityRemoval {
    pub key: u32,
    pub before: CharacterEventMappingObservation,
    /// Independently proved AL result, copied even if postflight fails.
    pub native_removed: u8,
    pub after: Result<CharacterEventMappingObservation>,
}
#[derive(Debug)]
pub struct EventGroupRemoval {
    pub before: CharacterEventMappingObservation,
    /// Success confirms no copied group node points to this entry, preserving all foreign
    /// group nodes and entity mappings. It does not prove external event-manager/reference drain.
    /// Failure can follow partial native removal/callbacks; never retry the original snapshot.
    pub after: Result<CharacterEventMappingObservation>,
}
#[derive(Debug)]
pub struct EventEntityRemovalRecord {
    pub key: u32,
    pub native_removed: u8,
    pub postflight: Result<()>,
}
#[derive(Debug)]
pub struct EventEntityRelease {
    pub before: CharacterEventMappingObservation,
    /// One record per actual native call. Stops at the first failed call/postflight/preflight.
    pub calls: Vec<EventEntityRemovalRecord>,
    /// Full failed call, when one occurred. Successful steps use compact records so a batch
    /// retains O(map size + alias count) storage rather than a full map per successful alias.
    pub failed_call: Option<EventEntityRemoval>,
    /// Success means all entity aliases were removed. Groups/character/set remain registered.
    /// Error retains the actual calls; it must remain pending/uncertain rather than be retried
    /// with the old snapshot or treated as a completed hide/destroy.
    pub after: Result<CharacterEventMappingObservation>,
}
pub struct CurrentCharacterEvents {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentCharacterEvents {
    /// Copy mappings of this exact live registration. No native calls or registration lease.
    pub fn observe(
        &self,
        expected: crate::character_sets::CharacterRegistrationObservation,
        limit: usize,
    ) -> Result<Option<CharacterEventMappingObservation>> {
        if expected.character.epoch != self.epoch {
            return Err(Error::NativeOperationRejected(
                "foreign event observation epoch",
            ));
        }
        let observed = Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .character_event_mappings(expected.character, expected.set_address, limit)?;
        if observed
            .as_ref()
            .is_some_and(|value| value.registration != expected)
        {
            return Err(Error::NativeOperationRejected(
                "event registration superseded",
            ));
        }
        Ok(observed)
    }
    /// # Safety
    /// Caller pins character, exact owning set/entry, allocator, maps, model-group data,
    /// event manager and the entire reachable native graph, supplies an exclusive permitted
    /// registration phase, excludes teardown/ABA/other users and Rust aliases, and retains
    /// DLL/hook residency. Binding neither proves world ownership nor acquires these guards.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero event registration epoch"));
        }
        let file = OwnedVerifiedFile::verify(bytes, WW_171)?;
        Ww171File::from_verified(file.borrow()?)?.bind_objects_with_patches(
            &CurrentProcess,
            base,
            patches.as_deref(),
        )?;
        Ok(Self {
            file,
            patches,
            base,
            epoch,
            _thread: PhantomData,
        })
    }

    /// Insert the explicitly copied stored ID into the unique entity map. Does not write
    /// the character field or substitute PlayerIns's virtual effective ID.
    /// # Safety
    /// Bind's full contract holds through postflight. Caller owns this registration's
    /// lifetime and cleanup; insertion may allocate and native failure may terminate.
    pub unsafe fn insert_stored_id(
        &self,
        expected: CharacterEventMappingObservation,
        limit: usize,
    ) -> Result<EventEntityInsertion> {
        validate_source(&expected, self.epoch)?;
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        if objects.character_event_mappings(
            expected.id.character,
            expected.registration.set_address,
            limit,
        )? != Some(expected.clone())
        {
            return Err(Error::NativeOperationRejected(
                "event registration source superseded",
            ));
        }
        if expected.entity_entry == Some(expected.registration.entry_address) {
            return Ok(EventEntityInsertion::AlreadyPresent(expected));
        }
        if expected.entity_entry.is_some() {
            return Err(Error::NativeOperationRejected(
                "event ID already belongs to another entry",
            ));
        }
        let address = objects.module.code_address(EVENT_EVIDENCE[3])?;
        // SAFETY: Proved RCX set/RDX u32-key pointer/R8 entry. Return register is ignored.
        let function: InsertEntity = unsafe { std::mem::transmute(address as usize) };
        let before = expected.clone();
        // SAFETY: Caller owns all allocator/map/entry/phase obligations; no implicit retry.
        Ok(unsafe {
            invoke_insert(function, expected, || {
                let after = objects
                    .character_event_mappings(
                        before.id.character,
                        before.registration.set_address,
                        limit,
                    )?
                    .ok_or(Error::InvalidProfile("event entry vanished"))?;
                check_insert(&before, &after)?;
                Ok(after)
            })
        })
    }

    /// Remove one copied nonzero unique-map alias belonging to this exact entry.
    /// The character's stored ID may be zero or belong to another entry; it is never used
    /// as a substitute for the explicitly supplied key and is never changed.
    /// # Safety
    /// Bind's full contract holds through postflight, including exclusive map ownership and
    /// allocator/entry/character lifetimes. Native removal frees its tree node, not the character.
    pub unsafe fn remove_entity_alias(
        &self,
        expected: CharacterEventMappingObservation,
        key: u32,
        limit: usize,
    ) -> Result<EventEntityRemoval> {
        removal_preflight(&expected, self.epoch, key)?;
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        if objects.character_event_mappings(
            expected.id.character,
            expected.registration.set_address,
            limit,
        )? != Some(expected.clone())
        {
            return Err(Error::NativeOperationRejected(
                "entity removal source superseded",
            ));
        }
        let address = objects.module.code_address(EVENT_EVIDENCE[10])?;
        // SAFETY: Complete native RCX set/RDX initialized u32 key pointer/AL result ABI.
        let function: RemoveEntity = unsafe { std::mem::transmute(address as usize) };
        let before = expected.clone();
        // SAFETY: Caller pins the entire map/allocator graph in an exclusive legal phase.
        Ok(unsafe {
            invoke_remove(function, expected, key, |native_removed| {
                let after = objects
                    .character_event_mappings(
                        before.id.character,
                        before.registration.set_address,
                        limit,
                    )?
                    .ok_or(Error::InvalidProfile("event removal target vanished"))?;
                check_remove(&before, key, native_removed, &after)?;
                Ok(after)
            })
        })
    }

    /// Release every currently copied entity alias, preserving foreign mappings and groups.
    /// A zero alias is rejected before any call because this native entry cannot erase key zero.
    /// # Safety
    /// Bind's full contract and exclusive legal map phase hold for the entire bounded batch.
    /// Caller retains failed/uncertain cleanup state and never interprets this as a reference
    /// drain, group-manager release, hide completion, detach, destructor or free permission.
    pub unsafe fn release_entity_aliases(
        &self,
        expected: CharacterEventMappingObservation,
        limit: usize,
    ) -> Result<EventEntityRelease> {
        validate_identity(&expected, self.epoch)?;
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        if objects.character_event_mappings(
            expected.id.character,
            expected.registration.set_address,
            limit,
        )? != Some(expected.clone())
        {
            return Err(Error::NativeOperationRejected(
                "entity release source superseded",
            ));
        }
        // SAFETY: Each step rechecks the complete source and the same exclusive caller contract.
        unsafe {
            release_aliases_with(expected, self.epoch, |source, key| {
                self.remove_entity_alias(source, key, limit)
            })
        }
    }

    /// Run the engine's model-group/fallback-effective-ID removal path exactly once.
    /// It may remove only one matching node per model ID. Leftover/unknown/zero groups cause
    /// failed postflight; the SDK neither erases raw nodes nor repeats manager callbacks.
    /// # Safety
    /// Bind's full contract holds, including the complete model-group iterator and effective
    /// Player ID graph, allocator and external manager callback lifetimes/exclusive phase.
    pub unsafe fn unregister_groups(
        &self,
        expected: CharacterEventMappingObservation,
        limit: usize,
    ) -> Result<EventGroupRemoval> {
        validate_identity(&expected, self.epoch)?;
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        if objects.character_event_mappings(
            expected.id.character,
            expected.registration.set_address,
            limit,
        )? != Some(expected.clone())
        {
            return Err(Error::NativeOperationRejected(
                "group removal source superseded",
            ));
        }
        check_event_getter(&objects, &expected)?;
        let address = objects.module.code_address(EVENT_EVIDENCE[11])?;
        // SAFETY: Complete RCX set/RDX character/void ABI; native selects model/fallback groups.
        let function: RemoveGroups = unsafe { std::mem::transmute(address as usize) };
        let before = expected.clone();
        // SAFETY: Caller owns all model/iterator/manager/map lifetimes in the legal phase.
        Ok(unsafe {
            invoke_group_remove(function, expected, || {
                let after = objects
                    .character_event_mappings(
                        before.id.character,
                        before.registration.set_address,
                        limit,
                    )?
                    .ok_or(Error::InvalidProfile("group removal target vanished"))?;
                check_group_remove(&before, &after)?;
                Ok(after)
            })
        })
    }

    /// Run native registration once for an entry with no observed group memberships.
    /// Native code chooses model groups, falling back to its virtual event ID getter.
    /// # Safety
    /// Bind's full contract holds; caller proves this is the first registration, including
    /// external event-manager state. Empty copied memberships do not prove that by themselves.
    pub unsafe fn register_groups(
        &self,
        expected: CharacterEventMappingObservation,
        limit: usize,
    ) -> Result<EventGroupRegistration> {
        validate_source(&expected, self.epoch)?;
        if !expected.groups.is_empty() {
            return Err(Error::NativeOperationRejected(
                "entry already has group memberships",
            ));
        }
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        if objects.character_event_mappings(
            expected.id.character,
            expected.registration.set_address,
            limit,
        )? != Some(expected.clone())
        {
            return Err(Error::NativeOperationRejected(
                "group registration source superseded",
            ));
        }
        check_event_getter(&objects, &expected)?;
        let address = objects.module.code_address(EVENT_EVIDENCE[4])?;
        // SAFETY: Complete native RCX set/RDX character/R8 entry void ABI.
        let function: RegisterGroups = unsafe { std::mem::transmute(address as usize) };
        let before = expected.clone();
        // SAFETY: Caller supplies all model-group/manager/allocator lifetimes and exclusive phase.
        Ok(unsafe {
            invoke_groups(function, expected, || {
                let after = objects
                    .character_event_mappings(
                        before.id.character,
                        before.registration.set_address,
                        limit,
                    )?
                    .ok_or(Error::InvalidProfile("group entry vanished"))?;
                if after.registration != before.registration
                    || after.id != before.id
                    || after.entity_entry != before.entity_entry
                    || after.entities != before.entities
                    || after
                        .group_entries
                        .iter()
                        .filter(|entry| entry.value != before.registration.entry_address)
                        .copied()
                        .collect::<Vec<_>>()
                        != before.group_entries
                {
                    return Err(Error::InvalidProfile(
                        "group registration identity/entity map changed",
                    ));
                }
                Ok(after)
            })
        })
    }
}
fn check_event_getter<M: AddressSpace>(
    objects: &GameObjectCatalog<'_, '_, M>,
    source: &CharacterEventMappingObservation,
) -> Result<()> {
    let character =
        objects
            .character(source.id.character.address)?
            .ok_or(Error::NativeOperationRejected(
                "missing native group character",
            ))?;
    let table = objects.module.memory().read_u64(character.address)?;
    let getter_slot = table
        .checked_add(u64::from(EVENT_GETTER_SLOT))
        .ok_or(Error::AddressOverflow)?;
    let range = EVENT_EVIDENCE[usize::from(character.kind == crate::objects::ObjectKind::Player)];
    if objects.module.memory().read_u64(getter_slot)? != objects.module.code_address(range)? {
        return Err(Error::InvalidProfile(
            "native event getter virtual slot changed",
        ));
    }
    Ok(())
}
fn check_group_remove(
    before: &CharacterEventMappingObservation,
    after: &CharacterEventMappingObservation,
) -> Result<()> {
    let remaining = before
        .group_entries
        .iter()
        .filter(|entry| entry.value != before.registration.entry_address)
        .copied()
        .collect::<Vec<_>>();
    if before.registration != after.registration
        || before.id != after.id
        || before.entity_entry != after.entity_entry
        || before.entities != after.entities
        || !after.groups.is_empty()
        || after.group_entries != remaining
    {
        return Err(Error::InvalidProfile(
            "group removal incomplete or other mapping changed",
        ));
    }
    Ok(())
}
unsafe fn invoke_group_remove(
    function: RemoveGroups,
    before: CharacterEventMappingObservation,
    after: impl FnOnce() -> Result<CharacterEventMappingObservation>,
) -> EventGroupRemoval {
    // SAFETY: Caller pins target/set/model/iterator/manager graph and provides exclusive phase.
    unsafe { function(before.registration.set_address, before.id.character.address) };
    EventGroupRemoval {
        before,
        after: after(),
    }
}
fn validate_source(source: &CharacterEventMappingObservation, epoch: u64) -> Result<()> {
    validate_identity(source, epoch)?;
    if source.id.stored_id == 0 {
        return Err(Error::NativeOperationRejected("zero stored event ID"));
    }
    Ok(())
}
fn validate_identity(source: &CharacterEventMappingObservation, epoch: u64) -> Result<()> {
    if source.id.character.epoch != epoch
        || epoch == 0
        || source.id.character != source.registration.character
    {
        return Err(Error::InvalidProfile(
            "stale or inconsistent event registration identity",
        ));
    }
    Ok(())
}
fn check_insert(
    before: &CharacterEventMappingObservation,
    after: &CharacterEventMappingObservation,
) -> Result<()> {
    if before.registration != after.registration
        || before.id != after.id
        || before.groups != after.groups
        || before.group_entries != after.group_entries
        || after.entity_entry != Some(before.registration.entry_address)
        || after
            .entities
            .iter()
            .filter(|entry| entry.key == before.id.stored_id)
            .count()
            != 1
        || after.entities.iter().any(|entry| {
            entry.key == before.id.stored_id && entry.value != before.registration.entry_address
        })
        || after
            .entities
            .iter()
            .filter(|entry| entry.key != before.id.stored_id)
            .copied()
            .collect::<Vec<_>>()
            != before.entities
    {
        return Err(Error::InvalidProfile(
            "event insertion not confirmed for original entry",
        ));
    }
    Ok(())
}
// Native 4947a0 leaves a low-byte result derived from the entry. It is not a bool.
// C ABI callers may ignore a returned scalar; do not interpret unspecified upper bits.
type InsertEntity = unsafe extern "system" fn(u64, *const u32, u64);
type RegisterGroups = unsafe extern "system" fn(u64, u64, u64);
type RemoveGroups = unsafe extern "system" fn(u64, u64);
type RemoveEntity = unsafe extern "system" fn(u64, *const u32) -> u8;
fn removal_preflight(
    source: &CharacterEventMappingObservation,
    epoch: u64,
    key: u32,
) -> Result<()> {
    validate_identity(source, epoch)?;
    if key == 0 || !source.entity_aliases().any(|entry| entry.key == key) {
        return Err(Error::NativeOperationRejected(
            "missing, foreign or zero entity alias",
        ));
    }
    Ok(())
}
fn check_remove(
    before: &CharacterEventMappingObservation,
    key: u32,
    native_removed: u8,
    after: &CharacterEventMappingObservation,
) -> Result<()> {
    let remaining = before
        .entities
        .iter()
        .filter(|entry| entry.key != key)
        .copied()
        .collect::<Vec<_>>();
    let stored = remaining
        .iter()
        .find(|entry| entry.key == before.id.stored_id)
        .map(|entry| entry.value);
    if native_removed != 1
        || before.registration != after.registration
        || before.id != after.id
        || before.groups != after.groups
        || before.group_entries != after.group_entries
        || remaining != after.entities
        || after.entity_entry != stored
    {
        return Err(Error::InvalidProfile(
            "entity removal not confirmed or other mappings changed",
        ));
    }
    Ok(())
}
unsafe fn invoke_remove(
    function: RemoveEntity,
    before: CharacterEventMappingObservation,
    key: u32,
    after: impl FnOnce(u8) -> Result<CharacterEventMappingObservation>,
) -> EventEntityRemoval {
    // SAFETY: Caller owns map/allocator graph; this initialized key is owned for the full call.
    let native_removed = unsafe { function(before.registration.set_address, &key) };
    EventEntityRemoval {
        key,
        before,
        native_removed,
        after: after(native_removed),
    }
}
fn release_aliases_with(
    before: CharacterEventMappingObservation,
    epoch: u64,
    mut remove: impl FnMut(CharacterEventMappingObservation, u32) -> Result<EventEntityRemoval>,
) -> Result<EventEntityRelease> {
    validate_identity(&before, epoch)?;
    let keys: Vec<_> = before.entity_aliases().map(|entry| entry.key).collect();
    if keys.contains(&0) {
        return Err(Error::NativeOperationRejected(
            "zero entity alias cannot be removed by this native entry",
        ));
    }
    let mut current = before.clone();
    let mut calls = Vec::with_capacity(keys.len());
    for key in keys {
        let call = match remove(current.clone(), key) {
            Ok(call) => call,
            Err(error) => {
                return Ok(EventEntityRelease {
                    before,
                    calls,
                    failed_call: None,
                    after: Err(error),
                });
            }
        };
        // Do not accept a backend record for a different attempt or incomplete postflight.
        let next = if call.key != key || call.before != current {
            Err(Error::InvalidProfile(
                "entity release receipt belongs to another attempt",
            ))
        } else {
            call.after.clone().and_then(|after| {
                check_remove(&current, key, call.native_removed, &after)?;
                Ok(after)
            })
        };
        calls.push(EventEntityRemovalRecord {
            key: call.key,
            native_removed: call.native_removed,
            postflight: next.as_ref().map(|_| ()).map_err(Clone::clone),
        });
        match next {
            Ok(after) => current = after,
            Err(error) => {
                return Ok(EventEntityRelease {
                    before,
                    calls,
                    failed_call: Some(call),
                    after: Err(error),
                });
            }
        }
    }
    Ok(EventEntityRelease {
        before,
        calls,
        failed_call: None,
        after: Ok(current),
    })
}
unsafe fn invoke_insert(
    function: InsertEntity,
    before: CharacterEventMappingObservation,
    after: impl FnOnce() -> Result<CharacterEventMappingObservation>,
) -> EventEntityInsertion {
    let key = before.id.stored_id;
    // SAFETY: Caller supplies live set/entry/graph; key is initialized SDK-owned u32.
    unsafe {
        function(
            before.registration.set_address,
            &key,
            before.registration.entry_address,
        )
    };
    EventEntityInsertion::Submitted {
        before,
        after: after(),
    }
}
unsafe fn invoke_groups(
    function: RegisterGroups,
    before: CharacterEventMappingObservation,
    after: impl FnOnce() -> Result<CharacterEventMappingObservation>,
) -> EventGroupRegistration {
    // SAFETY: Caller supplies live pinned set, character, entry and entire graph.
    unsafe {
        function(
            before.registration.set_address,
            before.id.character.address,
            before.registration.entry_address,
        )
    };
    EventGroupRegistration {
        before,
        after: after(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        character_events::CharacterEventIdObservation,
        character_sets::{CharacterIdentity, CharacterRegistrationObservation},
    };
    use std::cell::RefCell;
    struct Mock {
        calls: RefCell<Vec<(u32, u64)>>,
    }
    unsafe extern "system" fn insert(set: u64, key: *const u32, entry: u64) {
        // SAFETY: Tests pass an owned Mock and helper-owned initialized key.
        let mock = unsafe { &*(set as *const Mock) };
        // SAFETY: Key remains live through this call.
        mock.calls.borrow_mut().push((unsafe { *key }, entry));
    }
    unsafe extern "system" fn groups(set: u64, character: u64, entry: u64) {
        // SAFETY: Tests pass an owned Mock; other arguments are copied tags only.
        let mock = unsafe { &*(set as *const Mock) };
        assert_eq!(character, 0x1000);
        mock.calls.borrow_mut().push((0, entry));
    }
    fn source(mock: &Mock) -> CharacterEventMappingObservation {
        let character = CharacterIdentity {
            address: 0x1000,
            world_address: 0x2000,
            native_identifier: 0xfedcba9876543210,
            epoch: 9,
        };
        CharacterEventMappingObservation {
            registration: CharacterRegistrationObservation {
                character,
                set_address: mock as *const Mock as u64,
                entry_address: 0x3000,
                ordinal: 4,
            },
            id: CharacterEventIdObservation {
                character,
                stored_id: u32::MAX,
            },
            entity_entry: None,
            entities: vec![],
            groups: vec![],
            group_entries: vec![],
        }
    }
    #[test]
    fn native_arguments_run_once_and_failed_postflight_retains_both_attempts() {
        let mock = Mock {
            calls: RefCell::new(vec![]),
        };
        let before = source(&mock);
        // SAFETY: Original test ABI, owned Mock/key; no game code.
        let insertion = unsafe {
            invoke_insert(insert, before.clone(), || {
                Err(Error::InvalidProfile("after"))
            })
        };
        match insertion {
            EventEntityInsertion::Submitted {
                before: original,
                after,
            } => {
                assert_eq!(original, before);
                assert!(after.is_err());
            }
            _ => panic!("missing attempt"),
        }
        // SAFETY: Original test ABI and owned Mock; addresses are tags, not dereferenced.
        let registration = unsafe {
            invoke_groups(groups, before.clone(), || {
                Err(Error::InvalidProfile("after"))
            })
        };
        assert_eq!(registration.before, before);
        assert!(registration.after.is_err());
        assert_eq!(*mock.calls.borrow(), vec![(u32::MAX, 0x3000), (0, 0x3000)]);
    }
    #[test]
    fn insertion_requires_original_identity_and_actual_mapping_not_changed_field_only() {
        let mock = Mock {
            calls: RefCell::new(vec![]),
        };
        let before = source(&mock);
        let mut after = before.clone();
        assert!(check_insert(&before, &after).is_err());
        after.entity_entry = Some(0x3000);
        after
            .entities
            .push(shikifsrs_runtime::native_tree::PointerTreeEntry {
                node: 0x4000,
                key: u32::MAX,
                value: 0x3000,
            });
        assert!(check_insert(&before, &after).is_ok());
        after.entity_entry = Some(0x4000);
        assert!(check_insert(&before, &after).is_err());
        after.entity_entry = Some(0x3000);
        after.id.character.native_identifier ^= 1;
        assert!(check_insert(&before, &after).is_err());
        assert!(validate_source(&after, 9).is_err());
        assert!(validate_source(&before, 10).is_err());
        after = before;
        after.id.stored_id = 0;
        assert!(validate_source(&after, 9).is_err());
    }
    fn aliases(mock: &Mock) -> CharacterEventMappingObservation {
        use shikifsrs_runtime::native_tree::PointerTreeEntry;
        let mut source = source(mock);
        // The character's stored ID belongs to a foreign entry; two other IDs are ours.
        source.entities = vec![
            PointerTreeEntry {
                node: 0x4000,
                key: 1,
                value: 0x3000,
            },
            PointerTreeEntry {
                node: 0x4040,
                key: 0x80000000,
                value: 0x3000,
            },
            PointerTreeEntry {
                node: 0x4080,
                key: u32::MAX,
                value: 0x5000,
            },
        ];
        source.entity_entry = Some(0x5000);
        source
    }
    fn removed(
        before: &CharacterEventMappingObservation,
        key: u32,
    ) -> CharacterEventMappingObservation {
        let mut after = before.clone();
        after.entities.retain(|entry| entry.key != key);
        after.entity_entry = after
            .entities
            .iter()
            .find(|entry| entry.key == after.id.stored_id)
            .map(|entry| entry.value);
        after
    }
    unsafe extern "system" fn remove(set: u64, key: *const u32) -> u8 {
        // SAFETY: Original test Mock is owned; helper-owned key is initialized throughout.
        let mock = unsafe { &*(set as *const Mock) };
        // SAFETY: Native wrapper owns this initialized u32 through return.
        mock.calls.borrow_mut().push((unsafe { *key }, 0));
        1
    }
    #[test]
    fn alias_release_preserves_foreign_stored_id_and_stops_after_partial_failure() {
        let mock = Mock {
            calls: RefCell::new(vec![]),
        };
        let before = aliases(&mock);
        let mut attempts = 0;
        let release = release_aliases_with(before.clone(), 9, |source, key| {
            attempts += 1;
            // SAFETY: Original owned Mock ABI only, never game code.
            Ok(unsafe {
                invoke_remove(remove, source.clone(), key, |result| {
                    assert_eq!(result, 1);
                    if attempts == 2 {
                        return Err(Error::InvalidProfile("unreadable postflight"));
                    }
                    let after = removed(&source, key);
                    check_remove(&source, key, result, &after)?;
                    Ok(after)
                })
            })
        })
        .unwrap();
        assert_eq!(release.before, before);
        assert_eq!(release.calls.len(), 2);
        assert!(release.calls[0].postflight.is_ok());
        assert!(release.calls[1].postflight.is_err());
        assert_eq!(release.failed_call.as_ref().unwrap().key, 0x80000000);
        assert!(release.after.is_err());
        assert_eq!(*mock.calls.borrow(), vec![(1, 0), (0x80000000, 0)]);
        drop(release);
        assert_eq!(mock.calls.borrow().len(), 2); // No Drop retry/removal.
        mock.calls.borrow_mut().clear();
        let complete =
            release_aliases_with(before.clone(), 9, |source, key| {
                // SAFETY: Original owned Mock and SDK-owned key remain live through return.
                Ok(unsafe {
                    invoke_remove(remove, source.clone(), key, |_| Ok(removed(&source, key)))
                })
            })
            .unwrap();
        let after = complete.after.unwrap();
        assert!(complete.failed_call.is_none());
        assert_eq!(after.entities, vec![before.entities[2]]);
        assert_eq!(after.entity_entry, Some(0x5000));
        assert_eq!(after.entity_aliases().count(), 0);
        assert_eq!(after.id, before.id);
        assert_eq!(after.registration, before.registration);
        let mut called = false;
        let empty = release_aliases_with(after, 9, |_, _| {
            called = true;
            unreachable!()
        })
        .unwrap();
        assert!(!called);
        assert!(empty.calls.is_empty());
        assert!(empty.after.is_ok());
        let early = release_aliases_with(before, 9, |_, _| {
            Err(Error::NativeOperationRejected("superseded"))
        })
        .unwrap();
        assert!(early.calls.is_empty());
        assert!(early.failed_call.is_none());
        assert!(early.after.is_err());
    }
    #[test]
    fn removal_rejects_zero_foreign_keys_and_any_unconfirmed_or_extra_change() {
        let mock = Mock {
            calls: RefCell::new(vec![]),
        };
        let before = aliases(&mock);
        assert!(removal_preflight(&before, 9, 1).is_ok());
        for key in [0, 2, u32::MAX] {
            assert!(removal_preflight(&before, 9, key).is_err());
        }
        assert!(removal_preflight(&before, 10, 1).is_err());
        let after = removed(&before, 1);
        for raw in [0, 2, 255] {
            assert!(check_remove(&before, 1, raw, &after).is_err());
        }
        assert!(check_remove(&before, 1, 1, &before).is_err());
        assert!(check_remove(&before, 1, 1, &after).is_ok());
        for variant in 0..9 {
            let mut changed = after.clone();
            match variant {
                0 => changed.registration.ordinal ^= 1,
                1 => changed.id.stored_id = 0,
                2 => changed.id.character.epoch += 1,
                3 => changed.entity_entry = None,
                4 => changed.entities[1].value = 0x3000,
                5 => changed.entities[0].node += 8,
                6 => {
                    changed.entities.remove(0);
                }
                7 => changed.entities.push(before.entities[0]),
                8 => changed.groups.push(before.entities[0]),
                _ => unreachable!(),
            }
            assert!(check_remove(&before, 1, 1, &changed).is_err(), "{variant}");
        }
        let mut zero_stored = before.clone();
        zero_stored.id.stored_id = 0;
        zero_stored.entity_entry = None;
        assert!(removal_preflight(&zero_stored, 9, 1).is_ok());
        zero_stored.entities[0].key = 0;
        let mut called = false;
        assert!(
            release_aliases_with(zero_stored, 9, |_, _| {
                called = true;
                unreachable!()
            })
            .is_err()
        );
        assert!(!called);
        let invalid = release_aliases_with(before, 9, |source, key| {
            Ok(EventEntityRemoval {
                key: key + 1,
                before: source.clone(),
                native_removed: 1,
                after: Ok(removed(&source, key)),
            })
        })
        .unwrap();
        assert_eq!(invalid.calls.len(), 1);
        assert!(invalid.after.is_err());
    }
    unsafe extern "system" fn group_remove(set: u64, character: u64) {
        // SAFETY: Original Mock is owned through the call; character is an opaque test tag.
        let mock = unsafe { &*(set as *const Mock) };
        assert_eq!(character, 0x1000);
        mock.calls.borrow_mut().push((0, character));
    }
    fn grouped(mock: &Mock) -> CharacterEventMappingObservation {
        use shikifsrs_runtime::native_tree::PointerTreeEntry;
        let mut source = aliases(mock);
        source.group_entries = vec![
            PointerTreeEntry {
                node: 0x6000,
                key: 10,
                value: 0x3000,
            },
            PointerTreeEntry {
                node: 0x6040,
                key: 10,
                value: u64::MAX,
            },
            PointerTreeEntry {
                node: 0x6080,
                key: 10,
                value: 0x3000,
            },
            PointerTreeEntry {
                node: 0x60c0,
                key: u32::MAX,
                value: 0,
            },
        ];
        source.groups = vec![source.group_entries[0], source.group_entries[2]];
        source
    }
    fn ungrouped(before: &CharacterEventMappingObservation) -> CharacterEventMappingObservation {
        let mut after = before.clone();
        after.groups.clear();
        after
            .group_entries
            .retain(|entry| entry.value != before.registration.entry_address);
        after
    }
    #[test]
    fn group_removal_abi_retains_single_attempt_and_partial_failure_without_drop_retry() {
        let mock = Mock {
            calls: RefCell::new(vec![]),
        };
        let before = grouped(&mock);
        // SAFETY: Original two-argument ABI and owned Mock only; no game code.
        let success = unsafe {
            invoke_group_remove(group_remove, before.clone(), || {
                let after = ungrouped(&before);
                check_group_remove(&before, &after)?;
                Ok(after)
            })
        };
        assert_eq!(success.before, before);
        let after = success.after.unwrap();
        assert_eq!(
            after.group_entries,
            vec![before.group_entries[1], before.group_entries[3]]
        );
        assert_eq!(after.entities, before.entities);
        // SAFETY: Same original owned mock; caller explicitly performs this second test call.
        let partial = unsafe {
            invoke_group_remove(group_remove, before.clone(), || {
                let mut after = before.clone();
                after.group_entries.remove(0);
                after.groups.remove(0);
                check_group_remove(&before, &after)?;
                Ok(after)
            })
        };
        assert!(partial.after.is_err());
        assert_eq!(partial.before, before);
        drop(partial);
        assert_eq!(*mock.calls.borrow(), vec![(0, 0x1000), (0, 0x1000)]);
    }
    #[test]
    fn group_removal_requires_complete_membership_release_and_all_foreign_nodes_unchanged() {
        let mock = Mock {
            calls: RefCell::new(vec![]),
        };
        let mut before = grouped(&mock);
        before.id.stored_id = 0;
        before.entity_entry = None;
        assert!(validate_identity(&before, 9).is_ok());
        assert!(validate_source(&before, 9).is_err()); // Registration still requires stored ID.
        let after = ungrouped(&before);
        assert!(check_group_remove(&before, &after).is_ok());
        for variant in 0..10 {
            let mut changed = after.clone();
            match variant {
                0 => changed.groups.push(before.groups[0]),
                1 => changed.group_entries.push(before.groups[0]),
                2 => changed.group_entries[0].value = 0,
                3 => changed.group_entries[0].node += 8,
                4 => changed.group_entries[1].key = 10,
                5 => {
                    changed.group_entries.remove(0);
                }
                6 => changed.entities[0].value = 0,
                7 => changed.id.stored_id = 10,
                8 => changed.entity_entry = Some(0x5000),
                9 => changed.registration.entry_address += 16,
                _ => unreachable!(),
            }
            assert!(check_group_remove(&before, &changed).is_err(), "{variant}");
        }
        let empty = ungrouped(&before);
        assert!(check_group_remove(&empty, &empty).is_ok());
    }
}
