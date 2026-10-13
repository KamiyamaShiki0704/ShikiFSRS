//! Role -> controller override -> debug publication, with conditional reverse restoration.
use crate::{
    character_control_channels::{CharacterControlChannels, ManipulatorObservation},
    character_sets::CharacterIdentity,
    current_control_links::{ControlLinkWrite, CurrentControlLinks},
    debug_world::DebugWorldLinksObservation,
    objects::ObjectKind,
};
use shikifsrs_runtime::{Error, Result};
use std::{marker::PhantomData, rc::Rc};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlLinkField {
    InputRole,
    ControllerOverride,
    DebugCharacter,
}
const FIELDS: [ControlLinkField; 3] = [
    ControlLinkField::InputRole,
    ControlLinkField::ControllerOverride,
    ControlLinkField::DebugCharacter,
];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlLinkSnapshot {
    pub primary: CharacterIdentity,
    pub channels: CharacterControlChannels,
    pub debug: DebugWorldLinksObservation,
    pub input: ManipulatorObservation,
}
#[derive(Debug)]
pub struct ControlLinkStop {
    pub field: Option<ControlLinkField>,
    pub outcome: Result<ControlLinkWrite>,
}
#[derive(Debug, Default)]
pub struct ControlLinkSessionReport {
    pub applied: Vec<ControlLinkField>,
    pub superseded: Vec<ControlLinkField>,
    pub stopped: Option<ControlLinkStop>,
    pub pending_restore: usize,
}
/// Owns only the three recorded stores. No engine reference, resource or free authority.
/// The caller must retain both characters, old/new inputs and native consumers until explicit
/// unlink/reconciliation. Dropping this journal does not unlink pointers or release the game.
pub struct ControlLinkSession {
    baseline: ControlLinkSnapshot,
    owned: [bool; 3],
    closing: bool,
    _thread: PhantomData<Rc<()>>,
}
impl ControlLinkSession {
    /// Copy an idle input publication twice, preserving primary as the supplied body.
    /// Existing override/debug publications and a foreign input role are not adopted.
    pub fn capture(
        access: &CurrentControlLinks,
        body: CharacterIdentity,
        target: CharacterIdentity,
    ) -> Result<Option<Self>> {
        capture_with(access, body, target)
    }
    pub fn baseline(&self) -> ControlLinkSnapshot {
        self.baseline
    }
    pub fn pending_restore(&self) -> usize {
        self.owned.iter().filter(|v| **v).count()
    }
    pub fn closing(&self) -> bool {
        self.closing
    }
    /// # Safety
    /// CurrentControlLinks' full writable graph/residency/exclusive phase contract applies
    /// for both characters, controller, input and debug singleton throughout. Caller owns
    /// the target and independently proves direct role reassignment/control compatibility.
    /// No virtual initialization, reference acquisition or gameplay permission is inferred.
    /// After any partial result retain this journal and all pointees; no automatic rollback.
    pub unsafe fn apply(&mut self, access: &CurrentControlLinks) -> ControlLinkSessionReport {
        apply_with(self, access)
    }
    /// # Safety
    /// Same pinned graph/exclusive phase contract as apply, including original body/input
    /// dependencies needed for restoration. Restore debug first, then override, then role.
    /// Foreign values are preserved; an unresolved original storage retains its obligation.
    /// Pending zero means only these stores resolved, never callback drain or permission to free.
    pub unsafe fn restore(&mut self, access: &CurrentControlLinks) -> ControlLinkSessionReport {
        restore_with(self, access)
    }
    fn expected(&self) -> ControlLinkSnapshot {
        let mut state = self.baseline;
        if self.owned[0] {
            state.input.character = Some(state.channels.controller.character);
            state.input.native_character_address = state.channels.controller.character.address;
            state.debug.input = Some(state.input);
            if state
                .channels
                .ordinary
                .is_some_and(|v| v.object == state.input.object)
            {
                state.channels.ordinary = Some(state.input);
            }
        }
        if self.owned[1] {
            state.channels.override_input = Some(state.input);
        }
        if self.owned[2] {
            state.debug.character_override = Some(state.channels.controller.character);
        }
        state
    }
}
// Closed internal adapter: Err means no store. CurrentControlLinks has no fallible operation
// after Applied. Actual stores are journaled before any subsequent fallible observation.
trait Access {
    fn primary(&self) -> Result<Option<CharacterIdentity>>;
    fn channels(&self, target: CharacterIdentity) -> Result<Option<CharacterControlChannels>>;
    fn debug(&self) -> Result<Option<DebugWorldLinksObservation>>;
    fn input(&self, address: u64) -> Result<Option<ManipulatorObservation>>;
    fn role(
        &self,
        before: ManipulatorObservation,
        value: Option<CharacterIdentity>,
    ) -> Result<ControlLinkWrite>;
    fn override_input(
        &self,
        before: CharacterControlChannels,
        value: Option<ManipulatorObservation>,
    ) -> Result<ControlLinkWrite>;
    fn debug_character(
        &self,
        before: DebugWorldLinksObservation,
        value: Option<CharacterIdentity>,
    ) -> Result<ControlLinkWrite>;
}
impl Access for CurrentControlLinks {
    fn primary(&self) -> Result<Option<CharacterIdentity>> {
        self.observe_primary_identity()
    }
    fn channels(&self, target: CharacterIdentity) -> Result<Option<CharacterControlChannels>> {
        self.observe_channels(target)
    }
    fn debug(&self) -> Result<Option<DebugWorldLinksObservation>> {
        self.observe_debug_links()
    }
    fn input(&self, address: u64) -> Result<Option<ManipulatorObservation>> {
        self.observe_manipulator(address)
    }
    fn role(
        &self,
        before: ManipulatorObservation,
        value: Option<CharacterIdentity>,
    ) -> Result<ControlLinkWrite> {
        // SAFETY: Only the unsafe session methods invoke mutation under the full caller contract.
        unsafe { self.compare_replace_character(before, value) }
    }
    fn override_input(
        &self,
        before: CharacterControlChannels,
        value: Option<ManipulatorObservation>,
    ) -> Result<ControlLinkWrite> {
        // SAFETY: Session caller pins/exclusively owns this controller and old/new input graph.
        unsafe { self.compare_replace_override(before, value) }
    }
    fn debug_character(
        &self,
        before: DebugWorldLinksObservation,
        value: Option<CharacterIdentity>,
    ) -> Result<ControlLinkWrite> {
        // SAFETY: Session caller supplies the complete pinned debug/world/native consumer graph.
        unsafe { self.compare_replace_debug_character(before, value) }
    }
}
fn snapshot<A: Access>(
    access: &A,
    target: CharacterIdentity,
    input_address: Option<u64>,
) -> Result<Option<ControlLinkSnapshot>> {
    let Some(primary) = access.primary()? else {
        return Ok(None);
    };
    let Some(debug) = access.debug()? else {
        return Ok(None);
    };
    let Some(address) = input_address.or_else(|| debug.input.map(|v| v.object.address)) else {
        return Ok(None);
    };
    let Some(input) = access.input(address)? else {
        return Ok(None);
    };
    let Some(channels) = access.channels(target)? else {
        return Ok(None);
    };
    Ok(Some(ControlLinkSnapshot {
        primary,
        channels,
        debug,
        input,
    }))
}
fn stable_snapshot<A: Access>(
    access: &A,
    target: CharacterIdentity,
    input_address: Option<u64>,
) -> Result<Option<ControlLinkSnapshot>> {
    let first = snapshot(access, target, input_address)?;
    if snapshot(access, target, input_address)? != first {
        return Err(Error::InvalidProfile("whole control link snapshot changed"));
    }
    Ok(first)
}
fn capture_with<A: Access>(
    access: &A,
    body: CharacterIdentity,
    target: CharacterIdentity,
) -> Result<Option<ControlLinkSession>> {
    if body.epoch == 0
        || body.epoch != target.epoch
        || body.world_address == 0
        || body.world_address != target.world_address
        || body.address == 0
        || target.address == 0
        || !body.address.is_multiple_of(16)
        || !target.address.is_multiple_of(16)
        || body.address == target.address
        || body.native_identifier == target.native_identifier
    {
        return Err(Error::InvalidProfile("invalid control session identities"));
    }
    let Some(state) = stable_snapshot(access, target, None)? else {
        return Ok(None);
    };
    if state.primary != body
        || state.channels.controller.character != target
        || state.channels.override_input.is_some()
        || state.debug.character_override.is_some()
        || state.debug.object.kind != ObjectKind::DebugWorld
        || state.debug.epoch != body.epoch
        || state.debug.input != Some(state.input)
        || state.input.epoch != body.epoch
        || (state.input.character.is_some() && state.input.character != Some(body))
        || state.input.native_character_address != state.input.character.map_or(0, |v| v.address)
    {
        return Err(Error::NativeOperationRejected(
            "control session source is occupied or foreign",
        ));
    }
    Ok(Some(ControlLinkSession {
        baseline: state,
        owned: [false; 3],
        closing: false,
        _thread: PhantomData,
    }))
}
fn stop(
    report: &mut ControlLinkSessionReport,
    field: Option<ControlLinkField>,
    outcome: Result<ControlLinkWrite>,
) {
    report.stopped = Some(ControlLinkStop { field, outcome });
}
fn apply_with<A: Access>(session: &mut ControlLinkSession, access: &A) -> ControlLinkSessionReport {
    let mut report = ControlLinkSessionReport::default();
    if session.closing {
        stop(
            &mut report,
            None,
            Err(Error::NativeOperationRejected(
                "control session already closing",
            )),
        );
    } else {
        for (index, field) in FIELDS.iter().copied().enumerate() {
            let expected = session.expected();
            let current = stable_snapshot(
                access,
                expected.channels.controller.character,
                Some(expected.input.object.address),
            );
            match current {
                Ok(Some(value)) if value == expected => (),
                Ok(Some(_)) => {
                    stop(&mut report, Some(field), Ok(ControlLinkWrite::Superseded));
                    break;
                }
                Ok(None) => {
                    stop(&mut report, Some(field), Ok(ControlLinkWrite::Unavailable));
                    break;
                }
                Err(error) => {
                    stop(&mut report, Some(field), Err(error));
                    break;
                }
            }
            if session.owned[index] {
                continue;
            }
            let target = expected.channels.controller.character;
            let outcome = match field {
                ControlLinkField::InputRole => access.role(expected.input, Some(target)),
                ControlLinkField::ControllerOverride => {
                    access.override_input(expected.channels, Some(expected.input))
                }
                ControlLinkField::DebugCharacter => {
                    access.debug_character(expected.debug, Some(target))
                }
            };
            if outcome == Ok(ControlLinkWrite::Applied) {
                session.owned[index] = true;
                report.applied.push(field);
            } else {
                stop(&mut report, Some(field), outcome);
                break;
            }
        }
        if report.stopped.is_none() {
            let expected = session.expected();
            match stable_snapshot(
                access,
                expected.channels.controller.character,
                Some(expected.input.object.address),
            ) {
                Ok(Some(value)) if value == expected => (),
                Ok(_) => stop(&mut report, None, Ok(ControlLinkWrite::Superseded)),
                Err(error) => stop(&mut report, None, Err(error)),
            }
        }
    }
    report.pending_restore = session.pending_restore();
    report
}
fn restore_field<A: Access>(
    session: &ControlLinkSession,
    access: &A,
    field: ControlLinkField,
) -> Result<ControlLinkWrite> {
    let original = session.baseline;
    let target = original.channels.controller.character;
    let mut linked_input = original.input;
    linked_input.character = Some(target);
    linked_input.native_character_address = target.address;
    match field {
        ControlLinkField::DebugCharacter => {
            let Some(now) = access.debug()? else {
                return Ok(ControlLinkWrite::Unavailable);
            };
            if (now.object, now.epoch) != (original.debug.object, original.debug.epoch) {
                return Ok(ControlLinkWrite::Unavailable);
            }
            if now.character_override != Some(target) {
                return Ok(ControlLinkWrite::Superseded);
            }
            access.debug_character(now, original.debug.character_override)
        }
        ControlLinkField::ControllerOverride => {
            let Some(now) = access.channels(target)? else {
                return Ok(ControlLinkWrite::Unavailable);
            };
            if (
                now.controller.character,
                now.controller.address,
                now.controller.kind,
            ) != (
                original.channels.controller.character,
                original.channels.controller.address,
                original.channels.controller.kind,
            ) {
                return Ok(ControlLinkWrite::Unavailable);
            }
            if now.override_input != Some(linked_input) {
                return Ok(ControlLinkWrite::Superseded);
            }
            access.override_input(now, original.channels.override_input)
        }
        ControlLinkField::InputRole => {
            let Some(now) = access.input(original.input.object.address)? else {
                return Ok(ControlLinkWrite::Unavailable);
            };
            if (now.object, now.epoch) != (original.input.object, original.input.epoch) {
                return Ok(ControlLinkWrite::Unavailable);
            }
            if now.character != Some(target) || now.native_character_address != target.address {
                return Ok(ControlLinkWrite::Superseded);
            }
            access.role(now, original.input.character)
        }
    }
}
fn restore_with<A: Access>(
    session: &mut ControlLinkSession,
    access: &A,
) -> ControlLinkSessionReport {
    session.closing = true;
    let mut report = ControlLinkSessionReport::default();
    for (index, field) in FIELDS.iter().copied().enumerate().rev() {
        if !session.owned[index] {
            continue;
        }
        match restore_field(session, access, field) {
            Ok(ControlLinkWrite::Applied | ControlLinkWrite::Unchanged) => {
                session.owned[index] = false;
                report.applied.push(field);
            }
            Ok(ControlLinkWrite::Superseded) => {
                session.owned[index] = false;
                report.superseded.push(field);
            }
            outcome => {
                stop(&mut report, Some(field), outcome);
                break;
            }
        }
    }
    report.pending_restore = session.pending_restore();
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{character_controller::CharacterControllerObservation, objects::ObjectObservation};
    use std::cell::{Cell, RefCell};
    fn character(address: u64) -> CharacterIdentity {
        CharacterIdentity {
            address,
            world_address: 0x3000,
            native_identifier: 0xabcdef0000000000 | address,
            epoch: 9,
        }
    }
    struct Mock {
        state: RefCell<ControlLinkSnapshot>,
        calls: RefCell<Vec<ControlLinkField>>,
        writes: RefCell<Vec<ControlLinkField>>,
        fail_call: Cell<Option<usize>>,
        fail_read_after: Cell<Option<usize>>,
        available: Cell<bool>,
        late_primary: Cell<bool>,
    }
    impl Mock {
        fn new(alias: bool) -> Self {
            let body = character(0x1000);
            let target = character(0x2000);
            let input = ManipulatorObservation {
                object: ObjectObservation {
                    address: 0x6000,
                    kind: ObjectKind::PadManipulator,
                },
                epoch: 9,
                character: Some(body),
                native_character_address: body.address,
            };
            Self {
                state: RefCell::new(ControlLinkSnapshot {
                    primary: body,
                    input,
                    debug: DebugWorldLinksObservation {
                        object: ObjectObservation {
                            address: 0x5000,
                            kind: ObjectKind::DebugWorld,
                        },
                        epoch: 9,
                        input: Some(input),
                        character_override: None,
                    },
                    channels: CharacterControlChannels {
                        controller: CharacterControllerObservation {
                            character: target,
                            address: 0x4000,
                            kind: ObjectKind::EnemyController,
                            axis_scale_bits: [1.0f32.to_bits(); 3],
                        },
                        ordinary: if alias { Some(input) } else { None },
                        override_input: None,
                    },
                }),
                calls: RefCell::new(vec![]),
                writes: RefCell::new(vec![]),
                fail_call: Cell::new(None),
                fail_read_after: Cell::new(None),
                available: Cell::new(true),
                late_primary: Cell::new(false),
            }
        }
        fn readable(&self) -> Result<bool> {
            if self
                .fail_read_after
                .get()
                .is_some_and(|n| self.writes.borrow().len() >= n)
            {
                return Err(Error::InvalidProfile("late read failure"));
            }
            Ok(self.available.get())
        }
        fn begin_write(&self, field: ControlLinkField) -> Result<()> {
            let index = self.calls.borrow().len();
            self.calls.borrow_mut().push(field);
            if self.fail_call.get() == Some(index) {
                return Err(Error::NativeOperationRejected("pre-store failure"));
            }
            self.writes.borrow_mut().push(field);
            Ok(())
        }
        fn foreign_role(&self, owner: CharacterIdentity) {
            let mut state = self.state.borrow_mut();
            state.input.character = Some(owner);
            state.input.native_character_address = owner.address;
            let input = state.input;
            if state.debug.input.is_some_and(|v| v.object == input.object) {
                state.debug.input = Some(input)
            }
            if state
                .channels
                .ordinary
                .is_some_and(|v| v.object == input.object)
            {
                state.channels.ordinary = Some(input)
            }
            if state
                .channels
                .override_input
                .is_some_and(|v| v.object == input.object)
            {
                state.channels.override_input = Some(input)
            }
        }
    }
    impl Access for Mock {
        fn primary(&self) -> Result<Option<CharacterIdentity>> {
            Ok(self.readable()?.then(|| self.state.borrow().primary))
        }
        fn channels(&self, _: CharacterIdentity) -> Result<Option<CharacterControlChannels>> {
            Ok(self.readable()?.then(|| self.state.borrow().channels))
        }
        fn debug(&self) -> Result<Option<DebugWorldLinksObservation>> {
            if self.late_primary.replace(false) {
                self.state.borrow_mut().primary = character(0x7000);
            }
            Ok(self.readable()?.then(|| self.state.borrow().debug))
        }
        fn input(&self, _: u64) -> Result<Option<ManipulatorObservation>> {
            Ok(self.readable()?.then(|| self.state.borrow().input))
        }
        fn role(
            &self,
            before: ManipulatorObservation,
            value: Option<CharacterIdentity>,
        ) -> Result<ControlLinkWrite> {
            assert_eq!(self.state.borrow().input, before);
            self.begin_write(ControlLinkField::InputRole)?;
            let mut state = self.state.borrow_mut();
            state.input.character = value;
            state.input.native_character_address = value.map_or(0, |v| v.address);
            let input = state.input;
            if state.debug.input.is_some_and(|v| v.object == input.object) {
                state.debug.input = Some(input)
            }
            if state
                .channels
                .ordinary
                .is_some_and(|v| v.object == input.object)
            {
                state.channels.ordinary = Some(input)
            }
            if state
                .channels
                .override_input
                .is_some_and(|v| v.object == input.object)
            {
                state.channels.override_input = Some(input)
            }
            Ok(ControlLinkWrite::Applied)
        }
        fn override_input(
            &self,
            before: CharacterControlChannels,
            value: Option<ManipulatorObservation>,
        ) -> Result<ControlLinkWrite> {
            assert_eq!(self.state.borrow().channels, before);
            self.begin_write(ControlLinkField::ControllerOverride)?;
            self.state.borrow_mut().channels.override_input = value;
            Ok(ControlLinkWrite::Applied)
        }
        fn debug_character(
            &self,
            before: DebugWorldLinksObservation,
            value: Option<CharacterIdentity>,
        ) -> Result<ControlLinkWrite> {
            assert_eq!(self.state.borrow().debug, before);
            self.begin_write(ControlLinkField::DebugCharacter)?;
            self.state.borrow_mut().debug.character_override = value;
            Ok(ControlLinkWrite::Applied)
        }
    }
    fn captured(access: &Mock) -> ControlLinkSession {
        capture_with(access, character(0x1000), character(0x2000))
            .unwrap()
            .unwrap()
    }
    #[test]
    fn link_session_publishes_in_order_handles_aliases_and_restores_only_selected_fields() {
        for alias in [false, true] {
            let access = Mock::new(alias);
            let mut session = captured(&access);
            let baseline = session.baseline();
            let report = apply_with(&mut session, &access);
            assert!(report.stopped.is_none());
            assert_eq!(report.pending_restore, 3);
            assert_eq!(report.applied, FIELDS);
            assert_eq!(*access.state.borrow(), session.expected());
            assert!(apply_with(&mut session, &access).applied.is_empty());
            assert_eq!(access.writes.borrow().len(), 3);
            let foreign_input = ManipulatorObservation {
                object: ObjectObservation {
                    address: 0x8000,
                    kind: ObjectKind::CommandManipulator,
                },
                character: None,
                native_character_address: 0,
                ..baseline.input
            };
            {
                let mut state = access.state.borrow_mut();
                state.primary = character(0x7000);
                state.channels.controller.axis_scale_bits[1] = 2.0f32.to_bits();
                state.debug.input = Some(foreign_input);
            }
            let restored = restore_with(&mut session, &access);
            assert!(restored.stopped.is_none());
            assert_eq!(restored.pending_restore, 0);
            assert_eq!(
                restored.applied,
                [
                    ControlLinkField::DebugCharacter,
                    ControlLinkField::ControllerOverride,
                    ControlLinkField::InputRole
                ]
            );
            let state = *access.state.borrow();
            assert_eq!(state.input, baseline.input);
            assert_eq!(state.channels.override_input, None);
            assert_eq!(state.debug.character_override, None);
            assert_eq!(state.debug.input, Some(foreign_input));
            assert_eq!(
                state.channels.controller.axis_scale_bits[1],
                2.0f32.to_bits()
            );
            assert_eq!(state.primary, character(0x7000));
            assert!(apply_with(&mut session, &access).stopped.is_some());
            assert!(restore_with(&mut session, &access).applied.is_empty());
            {
                let _finished_session = session;
            }
            assert_eq!(access.writes.borrow().len(), 6);
        }
    }
    #[test]
    fn link_session_preserves_every_actual_prefix_and_resumes_without_duplicate_stores() {
        for failed in 0..3 {
            let access = Mock::new(true);
            let mut session = captured(&access);
            access.fail_call.set(Some(failed));
            let partial = apply_with(&mut session, &access);
            assert!(partial.stopped.is_some());
            assert_eq!(partial.pending_restore, failed);
            assert_eq!(&*access.writes.borrow(), &FIELDS[..failed]);
            access.fail_call.set(None);
            assert!(apply_with(&mut session, &access).stopped.is_none());
            assert_eq!(&*access.writes.borrow(), &FIELDS);
            access.fail_call.set(Some(access.calls.borrow().len()));
            assert_eq!(restore_with(&mut session, &access).pending_restore, 3);
            access.fail_call.set(None);
            assert_eq!(restore_with(&mut session, &access).pending_restore, 0);
            assert_eq!(
                &access.writes.borrow()[3..],
                &[
                    ControlLinkField::DebugCharacter,
                    ControlLinkField::ControllerOverride,
                    ControlLinkField::InputRole
                ]
            );
        }
        for applied in 1..=3 {
            let access = Mock::new(false);
            let mut session = captured(&access);
            access.fail_read_after.set(Some(applied));
            let partial = apply_with(&mut session, &access);
            assert!(partial.stopped.is_some());
            assert_eq!(partial.pending_restore, applied);
            assert_eq!(&*access.writes.borrow(), &FIELDS[..applied]);
            access.fail_read_after.set(None);
            assert!(apply_with(&mut session, &access).stopped.is_none());
            assert_eq!(&*access.writes.borrow(), &FIELDS);
            assert_eq!(restore_with(&mut session, &access).pending_restore, 0);
        }
    }
    #[test]
    fn link_session_rejects_occupied_or_mixed_capture_and_preserves_foreign_reassignment() {
        for invalid in 0..6 {
            let access = Mock::new(false);
            match invalid {
                0 => access.state.borrow_mut().debug.character_override = Some(character(0x7000)),
                1 => {
                    let input = access.state.borrow().input;
                    access.state.borrow_mut().channels.override_input = Some(input);
                }
                2 => access.foreign_role(character(0x7000)),
                3 => access.state.borrow_mut().input.native_character_address = 0x7000,
                4 => access.state.borrow_mut().primary = character(0x7000),
                _ => access.late_primary.set(true),
            }
            assert!(
                capture_with(&access, character(0x1000), character(0x2000)).is_err(),
                "capture {invalid}"
            );
            assert!(access.writes.borrow().is_empty());
        }
        let access = Mock::new(false);
        access.available.set(false);
        assert!(
            capture_with(&access, character(0x1000), character(0x2000))
                .unwrap()
                .is_none()
        );
        let access = Mock::new(false);
        let mut target = character(0x2000);
        target.epoch = 0;
        assert!(capture_with(&access, character(0x1000), target).is_err());
        let mut session = captured(&access);
        assert!(apply_with(&mut session, &access).stopped.is_none());
        access.foreign_role(character(0x7000));
        let restored = restore_with(&mut session, &access);
        assert!(restored.stopped.is_none());
        assert_eq!(restored.pending_restore, 0);
        assert_eq!(
            restored.superseded,
            [
                ControlLinkField::ControllerOverride,
                ControlLinkField::InputRole
            ]
        );
        assert_eq!(
            access.state.borrow().input.character,
            Some(character(0x7000))
        );
        assert_eq!(
            access
                .state
                .borrow()
                .channels
                .override_input
                .unwrap()
                .character,
            Some(character(0x7000))
        );
        assert_eq!(access.writes.borrow().len(), 4);
        let access = Mock::new(false);
        let mut session = captured(&access);
        apply_with(&mut session, &access);
        let original_debug = access.state.borrow().debug.object;
        access.state.borrow_mut().debug.object.address += 8;
        let blocked = restore_with(&mut session, &access);
        assert_eq!(blocked.pending_restore, 3);
        assert!(blocked.stopped.is_some());
        assert_eq!(access.writes.borrow().len(), 3);
        access.state.borrow_mut().debug.object = original_debug;
        assert_eq!(restore_with(&mut session, &access).pending_restore, 0);
    }
}
