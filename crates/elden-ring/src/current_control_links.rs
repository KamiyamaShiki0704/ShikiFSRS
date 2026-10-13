//! Conditional pointer and handle stores. No native reference ownership or automatic cleanup.
use crate::{
    WW_171, Ww171File,
    character_control_channels::{
        CharacterControlChannels, INPUT_CHARACTER_OFFSET, ManipulatorObservation,
        OVERRIDE_INPUT_OFFSET,
    },
    character_handles::{
        EffectSourceClear, LastEffectSourceObservation, PrimaryPlayerTargetClear,
        PrimaryPlayerTargetObservation, commit_effect_source_clear, commit_primary_target_clear,
        effect_source_clear_plan, primary_target_clear_plan, store_native_handle,
    },
    character_sets::CharacterIdentity,
};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlLinkWrite {
    Applied,
    Unchanged,
    Superseded,
    Unavailable,
}

/// A thread-confined access session; it never acquires engine ownership or writes on Drop.
pub struct CurrentControlLinks {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentControlLinks {
    pub fn observe_channels(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterControlChannels>> {
        if target.epoch != self.epoch {
            return Err(Error::NativeOperationRejected(
                "foreign control channel epoch",
            ));
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .character_control_channels(target)
    }
    pub fn observe_manipulator(&self, address: u64) -> Result<Option<ManipulatorObservation>> {
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .manipulator(address, self.epoch)
    }
    pub fn observe_debug_links(
        &self,
    ) -> Result<Option<crate::debug_world::DebugWorldLinksObservation>> {
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .debug_world_links(self.epoch)
    }
    pub fn observe_primary_identity(&self) -> Result<Option<CharacterIdentity>> {
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .primary_player_identity(self.epoch)
    }
    pub fn observe_effect_source(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<LastEffectSourceObservation>> {
        if target.epoch != self.epoch {
            return Err(Error::NativeOperationRejected(
                "foreign effect source epoch",
            ));
        }
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        file.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .last_effect_source(target)
    }
    /// Explicitly clear the observed source storage of this exact live character.
    /// The previous source is never resolved or required to remain alive.
    /// # Safety
    /// Bind's contract includes initialized writable aligned ChrIns+180 and exclusive
    /// access through postflight. Caller pins the target graph, excludes effect callbacks
    /// and all concurrent native writes. Clearing is not revival, death reset or a barrier.
    /// Failed postflight retains the single store; do not infer no effect or blindly retry.
    pub unsafe fn clear_effect_source(
        &self,
        expected: LastEffectSourceObservation,
    ) -> Result<EffectSourceClear> {
        if expected.character.epoch != self.epoch {
            return Err(Error::NativeOperationRejected(
                "foreign effect source epoch",
            ));
        }
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let objects =
            file.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?;
        let plan =
            effect_source_clear_plan(objects.last_effect_source(expected.character)?, expected)?;
        Ok(commit_effect_source_clear(
            plan,
            |address, value| {
                // SAFETY: Caller owns the pinned exclusive initialized eight-byte target field.
                unsafe { store_native_handle(address, value) }
            },
            || {
                objects
                    .last_effect_source(expected.character)?
                    .ok_or(Error::InvalidProfile(
                        "character unavailable after effect source clear",
                    ))
            },
        ))
    }
    pub fn observe_primary_target(&self) -> Result<Option<PrimaryPlayerTargetObservation>> {
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        file.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .primary_player_target(self.epoch)
    }
    /// Clear only if this primary selection still equals the exact live target's full handle.
    /// An absent or different target is left intact; no automatic restore after successful clear.
    /// # Safety
    /// Bind's contract additionally includes initialized writable primary PlayerIns+6b0,
    /// primary-player identity, target lifetime and exclusive selection mutation phase. Invoke
    /// before target detach/destruction. This is not a camera shutdown, engine barrier or pin.
    /// Failed postflight retains the actual store; do not assume it did not occur or retry.
    pub unsafe fn clear_primary_target(
        &self,
        expected: PrimaryPlayerTargetObservation,
        target: CharacterIdentity,
    ) -> Result<PrimaryPlayerTargetClear> {
        if expected.player.epoch != self.epoch || target.epoch != self.epoch {
            return Err(Error::NativeOperationRejected(
                "foreign primary target epoch",
            ));
        }
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let objects =
            file.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?;
        let plan = primary_target_clear_plan(
            objects.primary_player_target(self.epoch)?,
            expected,
            objects.character_identity(target.address, self.epoch)?,
            target,
        )?;
        Ok(commit_primary_target_clear(
            plan,
            |address, value| {
                // SAFETY: Caller guarantees exclusive live primary eight-byte destination and both identities.
                unsafe {
                    store_native_handle(address, value);
                }
            },
            || {
                if objects.character_identity(target.address, self.epoch)? != Some(target) {
                    return Err(Error::InvalidProfile(
                        "target identity changed after selection clear",
                    ));
                }
                objects
                    .primary_player_target(self.epoch)?
                    .ok_or(Error::InvalidProfile(
                        "primary player unavailable after target clear",
                    ))
            },
        ))
    }
    /// Read-only bind to the fixed executable and optional owned hook manifest.
    /// # Safety
    /// Caller pins all controller/input/character allocations and code/data reachable from
    /// their future use, holds an exclusive permitted engine phase, proves each pointer field
    /// writable and aligned, and excludes callbacks, engine writes, teardown, address reuse,
    /// unload and Rust aliases. Both old and new pointees remain live until explicit unlink.
    /// Direct pointer stores do not perform the native setter's virtual initialization or
    /// establish valid gameplay control. Caller proves input compatibility and complete
    /// graph initialization independently. Every hook's transitive ABI/lifetime contract holds.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero control link epoch"));
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

    /// # Safety
    /// Reestablish bind's full access contract for the new epoch after a lifetime transition.
    /// Advancing does not unlink pointers, release references or complete callback drain.
    pub unsafe fn advance_epoch(&mut self, epoch: u64) -> Result<()> {
        if epoch <= self.epoch {
            return Err(Error::InvalidProfile("control link epoch must advance"));
        }
        self.epoch = epoch;
        Ok(())
    }

    /// Replace or restore only controller+3b0. Null and shared input links are permitted.
    /// # Safety
    /// Bind's exclusive access contract holds through fresh observation and exactly one store.
    /// Caller records any Applied baseline and explicitly restores in reverse publication
    /// order before releasing allocations. No implicit rollback or atomic multi-link claim.
    pub unsafe fn compare_replace_override(
        &self,
        expected: CharacterControlChannels,
        replacement: Option<ManipulatorObservation>,
    ) -> Result<ControlLinkWrite> {
        if expected.controller.character.epoch != self.epoch
            || replacement.is_some_and(|v| v.epoch != self.epoch)
        {
            return Ok(ControlLinkWrite::Unavailable);
        }
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        if let Some(value) = replacement
            && objects.manipulator(value.object.address, self.epoch)? != Some(value)
        {
            return Ok(ControlLinkWrite::Unavailable);
        }
        let current = objects.character_control_channels(expected.controller.character)?;
        let plan = override_destination(current, expected, replacement)?;
        // SAFETY: Caller pins the exclusive writable destination and pointee graph.
        unsafe { execute(plan) }
    }

    /// Conditionally publish or restore debug+b8, without changing primary_player.
    /// # Safety
    /// Bind's full contract includes the pinned debug singleton and all native consumers of
    /// its override. This override also affects native world queries, not only cameras.
    /// For a multi-link publication, publish this last and restore first. Record Applied
    /// obligations even when another link fails. Input field changes are not overwritten.
    pub unsafe fn compare_replace_debug_character(
        &self,
        expected: crate::debug_world::DebugWorldLinksObservation,
        replacement: Option<CharacterIdentity>,
    ) -> Result<ControlLinkWrite> {
        if expected.epoch != self.epoch || replacement.is_some_and(|v| v.epoch != self.epoch) {
            return Ok(ControlLinkWrite::Unavailable);
        }
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        if let Some(value) = replacement
            && objects.character_identity(value.address, self.epoch)? != Some(value)
        {
            return Ok(ControlLinkWrite::Unavailable);
        }
        let current = objects.debug_world_links(self.epoch)?;
        let plan = debug_character_destination(current, expected, replacement)?;
        // SAFETY: Caller owns the exclusive pinned eight-byte field and old/new character graph.
        unsafe { execute(plan) }
    }

    /// Replace or restore only input+a8. Does not alter controller ownership or ordinary18.
    /// # Safety
    /// Bind's contract holds; caller proves direct reassignment is valid for this input type
    /// and its fully initialized native graph. No virtual setter, attach or detach is called.
    /// For a two-link publication, set the role first and publish override last; restore
    /// override first. Record Applied values even if a later operation fails.
    pub unsafe fn compare_replace_character(
        &self,
        expected: ManipulatorObservation,
        replacement: Option<CharacterIdentity>,
    ) -> Result<ControlLinkWrite> {
        if expected.epoch != self.epoch || replacement.is_some_and(|v| v.epoch != self.epoch) {
            return Ok(ControlLinkWrite::Unavailable);
        }
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        if let Some(value) = replacement
            && objects.character_identity(value.address, self.epoch)? != Some(value)
        {
            return Ok(ControlLinkWrite::Unavailable);
        }
        let current = objects.manipulator(expected.object.address, self.epoch)?;
        let plan = character_destination(current, expected, replacement)?;
        // SAFETY: Caller pins the exclusive writable input field and all future consumers.
        unsafe { execute(plan) }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Destination {
    Stop(ControlLinkWrite),
    Store { address: usize, value: u64 },
}

fn pointer_destination(storage: u64, offset: u32, before: u64, after: u64) -> Result<Destination> {
    if storage == 0 || !storage.is_multiple_of(8) || (after != 0 && !after.is_multiple_of(8)) {
        return Err(Error::InvalidProfile(
            "unaligned control link destination or pointee",
        ));
    }
    let address = storage
        .checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)?;
    let address = usize::try_from(address).map_err(|_| Error::AddressOverflow)?;
    if before == after {
        return Ok(Destination::Stop(ControlLinkWrite::Unchanged));
    }
    Ok(Destination::Store {
        address,
        value: after,
    })
}

fn override_destination(
    current: Option<CharacterControlChannels>,
    expected: CharacterControlChannels,
    replacement: Option<ManipulatorObservation>,
) -> Result<Destination> {
    let Some(current) = current else {
        return Ok(Destination::Stop(ControlLinkWrite::Unavailable));
    };
    if (
        current.controller.character,
        current.controller.address,
        current.controller.kind,
    ) != (
        expected.controller.character,
        expected.controller.address,
        expected.controller.kind,
    ) {
        return Ok(Destination::Stop(ControlLinkWrite::Unavailable));
    }
    if current.override_input != expected.override_input {
        return Ok(Destination::Stop(ControlLinkWrite::Superseded));
    }
    pointer_destination(
        current.controller.address,
        OVERRIDE_INPUT_OFFSET,
        current.override_input.map_or(0, |v| v.object.address),
        replacement.map_or(0, |v| v.object.address),
    )
}

fn debug_character_destination(
    current: Option<crate::debug_world::DebugWorldLinksObservation>,
    expected: crate::debug_world::DebugWorldLinksObservation,
    replacement: Option<CharacterIdentity>,
) -> Result<Destination> {
    let Some(current) = current else {
        return Ok(Destination::Stop(ControlLinkWrite::Unavailable));
    };
    if (current.object, current.epoch) != (expected.object, expected.epoch) {
        return Ok(Destination::Stop(ControlLinkWrite::Unavailable));
    }
    if current.character_override != expected.character_override {
        return Ok(Destination::Stop(ControlLinkWrite::Superseded));
    }
    pointer_destination(
        current.object.address,
        crate::debug_world::CHARACTER_OVERRIDE_OFFSET,
        current.character_override.map_or(0, |v| v.address),
        replacement.map_or(0, |v| v.address),
    )
}

fn character_destination(
    current: Option<ManipulatorObservation>,
    expected: ManipulatorObservation,
    replacement: Option<CharacterIdentity>,
) -> Result<Destination> {
    let Some(current) = current else {
        return Ok(Destination::Stop(ControlLinkWrite::Unavailable));
    };
    if (current.object, current.epoch) != (expected.object, expected.epoch) {
        return Ok(Destination::Stop(ControlLinkWrite::Unavailable));
    }
    if current.character != expected.character
        || current.native_character_address != expected.native_character_address
    {
        return Ok(Destination::Stop(ControlLinkWrite::Superseded));
    }
    pointer_destination(
        current.object.address,
        INPUT_CHARACTER_OFFSET,
        current.native_character_address,
        replacement.map_or(0, |v| v.address),
    )
}

unsafe fn execute(plan: Destination) -> Result<ControlLinkWrite> {
    match plan {
        Destination::Stop(outcome) => Ok(outcome),
        Destination::Store { address, value } => {
            // SAFETY: Caller proves eight writable aligned bytes, stable graph and exclusivity.
            // Exactly one pointer store occurs. No fallible operation follows the write.
            unsafe { (address as *mut u64).write(value) };
            Ok(ControlLinkWrite::Applied)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        character_controller::CharacterControllerObservation,
        objects::{ObjectKind, ObjectObservation},
    };
    fn character(address: u64) -> CharacterIdentity {
        CharacterIdentity {
            address,
            world_address: 0x2000,
            native_identifier: address + 1,
            epoch: 3,
        }
    }
    fn input(address: u64, owner: Option<CharacterIdentity>) -> ManipulatorObservation {
        ManipulatorObservation {
            object: ObjectObservation {
                address,
                kind: ObjectKind::PadManipulator,
            },
            epoch: 3,
            character: owner,
            native_character_address: owner.map_or(0, |v| v.address),
        }
    }
    fn channels() -> CharacterControlChannels {
        CharacterControlChannels {
            controller: CharacterControllerObservation {
                character: character(0x3000),
                address: 0x4000,
                kind: ObjectKind::PlayerController,
                axis_scale_bits: [0; 3],
            },
            ordinary: Some(input(0x5000, Some(character(0x3000)))),
            override_input: None,
        }
    }
    #[test]
    fn conditional_link_plan_preserves_foreign_input_and_storage_identity_changes() {
        let baseline = channels();
        let after = Some(input(0x6000, Some(character(0x7000))));
        assert_eq!(
            override_destination(Some(baseline), baseline, after).unwrap(),
            Destination::Store {
                address: 0x43b0,
                value: 0x6000
            }
        );
        assert_eq!(
            override_destination(Some(baseline), baseline, None).unwrap(),
            Destination::Stop(ControlLinkWrite::Unchanged)
        );
        assert_eq!(
            override_destination(None, baseline, after).unwrap(),
            Destination::Stop(ControlLinkWrite::Unavailable)
        );
        for change in 0..7 {
            let mut now = baseline;
            match change {
                0 => now.controller.character.native_identifier += 1,
                1 => now.controller.character.world_address += 8,
                2 => now.controller.character.epoch += 1,
                3 => now.controller.address += 8,
                4 => now.controller.kind = ObjectKind::EnemyController,
                5 => now.override_input = after,
                _ => {
                    now.controller.axis_scale_bits = [1; 3];
                    now.ordinary = None;
                }
            }
            let plan = override_destination(Some(now), baseline, after).unwrap();
            assert_eq!(
                plan,
                match change {
                    5 => Destination::Stop(ControlLinkWrite::Superseded),
                    6 => Destination::Store {
                        address: 0x43b0,
                        value: 0x6000
                    },
                    _ => Destination::Stop(ControlLinkWrite::Unavailable),
                }
            );
        }
    }
    #[test]
    fn input_character_plan_uses_full_identity_and_allows_explicit_null_restore() {
        let baseline = input(0x6000, None);
        let owner = character(0x7000);
        assert_eq!(
            character_destination(Some(baseline), baseline, Some(owner)).unwrap(),
            Destination::Store {
                address: 0x60a8,
                value: 0x7000
            }
        );
        let applied = input(0x6000, Some(owner));
        assert_eq!(
            character_destination(Some(applied), applied, None).unwrap(),
            Destination::Store {
                address: 0x60a8,
                value: 0
            }
        );
        for change in 0..7 {
            let mut now = applied;
            match change {
                0 => now.object.address += 8,
                1 => now.object.kind = ObjectKind::CommandManipulator,
                2 => now.epoch += 1,
                3 => now.native_character_address += 8,
                4 => now.character.as_mut().unwrap().native_identifier += 1,
                5 => now.character.as_mut().unwrap().world_address += 8,
                _ => now.character.as_mut().unwrap().epoch += 1,
            }
            assert_eq!(
                character_destination(Some(now), applied, None).unwrap(),
                Destination::Stop(if change < 3 {
                    ControlLinkWrite::Unavailable
                } else {
                    ControlLinkWrite::Superseded
                })
            );
        }
    }
    #[test]
    fn input_pointer_destination_rejects_bad_alignment_overflow_and_preserves_noop() {
        for (storage, value) in [(0, 8), (1, 8), (8, 1), (u64::MAX - 7, 8)] {
            assert!(pointer_destination(storage, INPUT_CHARACTER_OFFSET, 0, value).is_err());
        }
        assert_eq!(
            pointer_destination(8, INPUT_CHARACTER_OFFSET, 0, 0).unwrap(),
            Destination::Stop(ControlLinkWrite::Unchanged)
        );
    }
    #[test]
    fn original_owned_two_link_storage_supports_reverse_conditional_restoration() {
        // Original test-owned arrays; no game pointer, callback or engine call is used.
        let mut controller = [0x1111_2222_3333_4444u64; 120];
        let mut manipulator = [0x5555_6666_7777_8888u64; 24];
        controller[118] = 0;
        manipulator[21] = 0;
        let mut original_channels = channels();
        original_channels.controller.address = controller.as_mut_ptr() as u64;
        let original_input = input(manipulator.as_mut_ptr() as u64, None);
        let applied_input = input(original_input.object.address, Some(character(0x7000)));
        // SAFETY: Both destinations are aligned original arrays with exclusive access.
        unsafe {
            assert_eq!(
                execute(
                    character_destination(
                        Some(original_input),
                        original_input,
                        applied_input.character
                    )
                    .unwrap()
                )
                .unwrap(),
                ControlLinkWrite::Applied
            );
            assert_eq!(
                execute(
                    override_destination(
                        Some(original_channels),
                        original_channels,
                        Some(applied_input)
                    )
                    .unwrap()
                )
                .unwrap(),
                ControlLinkWrite::Applied
            );
        }
        assert_eq!(controller[118], original_input.object.address);
        assert_eq!(manipulator[21], 0x7000);
        let published = CharacterControlChannels {
            override_input: Some(applied_input),
            ..original_channels
        };
        // A read failure/missing target does not erase the caller's Applied baseline.
        assert_eq!(
            override_destination(None, published, None).unwrap(),
            Destination::Stop(ControlLinkWrite::Unavailable)
        );
        // SAFETY: Same exclusive original buffers; unpublish before restoring input role.
        unsafe {
            assert_eq!(
                execute(override_destination(Some(published), published, None).unwrap()).unwrap(),
                ControlLinkWrite::Applied
            );
            assert_eq!(
                execute(character_destination(Some(applied_input), applied_input, None).unwrap())
                    .unwrap(),
                ControlLinkWrite::Applied
            );
        }
        assert_eq!(controller[118], 0);
        assert_eq!(manipulator[21], 0);
        for (i, v) in controller.into_iter().enumerate() {
            if i != 118 {
                assert_eq!(v, 0x1111_2222_3333_4444)
            }
        }
        for (i, v) in manipulator.into_iter().enumerate() {
            if i != 21 {
                assert_eq!(v, 0x5555_6666_7777_8888)
            }
        }
    }

    fn debug_links(address: u64) -> crate::debug_world::DebugWorldLinksObservation {
        crate::debug_world::DebugWorldLinksObservation {
            object: ObjectObservation {
                address,
                kind: ObjectKind::DebugWorld,
            },
            epoch: 3,
            input: Some(input(0x5000, Some(character(0x3000)))),
            character_override: Some(character(0x3000)),
        }
    }
    #[test]
    fn debug_override_plan_preserves_replaced_storage_and_foreign_full_character_identity() {
        let baseline = debug_links(0x6000);
        assert_eq!(
            debug_character_destination(None, baseline, None).unwrap(),
            Destination::Stop(ControlLinkWrite::Unavailable)
        );
        assert_eq!(
            debug_character_destination(Some(baseline), baseline, baseline.character_override)
                .unwrap(),
            Destination::Stop(ControlLinkWrite::Unchanged)
        );
        for change in 0..7 {
            let mut now = baseline;
            match change {
                0 => now.object.address += 8,
                1 => now.epoch += 1,
                2 => now.object.kind = ObjectKind::WorldCharacters,
                3 => now.character_override.as_mut().unwrap().native_identifier += 1,
                4 => now.character_override.as_mut().unwrap().world_address += 8,
                5 => now.character_override = None,
                _ => now.input = None,
            }
            let result = debug_character_destination(Some(now), baseline, None).unwrap();
            assert_eq!(
                result,
                if change < 3 {
                    Destination::Stop(ControlLinkWrite::Unavailable)
                } else if change < 6 {
                    Destination::Stop(ControlLinkWrite::Superseded)
                } else {
                    Destination::Store {
                        address: 0x60b8,
                        value: 0,
                    }
                }
            );
        }
    }
    #[test]
    fn debug_override_exact_store_and_reverse_restore_leave_neighbors_and_input_untouched() {
        let mut storage = [0xaaaa_bbbb_cccc_ddddu64; 25];
        storage[23] = 0x3000;
        let baseline = debug_links(storage.as_mut_ptr() as u64);
        let next = Some(character(0x7000));
        let plan = debug_character_destination(Some(baseline), baseline, next).unwrap();
        // SAFETY: Destination is one exclusive aligned lane in this owned array; no game memory.
        assert_eq!(unsafe { execute(plan) }.unwrap(), ControlLinkWrite::Applied);
        assert_eq!(storage[23], 0x7000);
        let published = crate::debug_world::DebugWorldLinksObservation {
            character_override: next,
            ..baseline
        };
        let plan =
            debug_character_destination(Some(published), published, baseline.character_override)
                .unwrap();
        // SAFETY: Same owned array and lane; preserves every neighbor.
        assert_eq!(unsafe { execute(plan) }.unwrap(), ControlLinkWrite::Applied);
        assert_eq!(storage[23], 0x3000);
        for (index, value) in storage.into_iter().enumerate() {
            if index != 23 {
                assert_eq!(value, 0xaaaa_bbbb_cccc_dddd)
            }
        }
    }
}
