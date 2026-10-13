use super::*;
use shikifsrs_runtime::input::{
    ForegroundWindow, GamepadState, InputPoller, InputSource, VirtualKey,
};
use std::cell::{Cell, RefCell};

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Apply(u64, i32, bool),
    Remove(u64, i32),
}
#[derive(Default)]
struct Engine {
    calls: RefCell<Vec<Call>>,
    fail_apply: Cell<bool>,
    fail_remove: Cell<bool>,
}
// SAFETY: This independent fake records parameters only, with no memory, game calls or lifetimes.
unsafe impl EffectEngine for Engine {
    unsafe fn apply(
        &self,
        target: CharacterIdentity,
        id: EffectId,
        force: bool,
    ) -> Result<EffectApplication> {
        self.calls
            .borrow_mut()
            .push(Call::Apply(target.address, id.value(), force));
        if self.fail_apply.replace(false) {
            Err(Error::NativeOperationRejected("synthetic apply failure"))
        } else {
            Ok(EffectApplication::Submitted)
        }
    }
    unsafe fn remove(&self, target: CharacterIdentity, id: EffectId) -> Result<bool> {
        self.calls
            .borrow_mut()
            .push(Call::Remove(target.address, id.value()));
        if self.fail_remove.replace(false) {
            Err(Error::NativeOperationRejected("synthetic remove failure"))
        } else {
            Ok(false)
        } // An already absent effect still completes cleanup.
    }
}

struct Source {
    focused: bool,
    down: bool,
}
impl InputSource for Source {
    fn foreground(&self) -> Result<Option<ForegroundWindow>> {
        Ok(Some(ForegroundWindow {
            handle: 1,
            process_id: if self.focused { 42 } else { 99 },
        }))
    }
    fn key_state(&self, _: VirtualKey) -> Result<i16> {
        Ok(if self.down { i16::MIN } else { 0 })
    }
    fn gamepad(&self, _: u8) -> Result<Option<GamepadState>> {
        Ok(None)
    }
}
fn key(code: u16) -> Input {
    Input::Key(VirtualKey::new(code).unwrap())
}
fn effect(id: i32) -> EffectId {
    EffectId::new(id).unwrap()
}
fn controller() -> InputEffectController {
    InputEffectController::new([InputEffect {
        input: key(0x41),
        effect: effect(899000),
    }])
    .unwrap()
}
fn frame(inputs: &[Input]) -> Result<InputFrame> {
    Ok(InputFrame {
        focused: true,
        down: inputs.iter().copied().collect(),
        gamepad: None,
        gamepad_error: None,
    })
}
fn update(
    c: &mut InputEffectController,
    e: &Engine,
    target: Option<u64>,
    frame: Result<InputFrame>,
) -> EffectFrameReport {
    // SAFETY: Independent fake engine; no target addresses are accessed or game effects mutated.
    unsafe { c.update(e, target.map(identity), frame) }
}

fn identity(address: u64) -> CharacterIdentity {
    CharacterIdentity {
        address,
        world_address: 0x1000,
        native_identifier: address + 1,
        epoch: 1,
    }
}

#[test]
fn polling_to_effects_reapplies_held_and_cleans_release_and_focus_loss() {
    let mut c = controller();
    let e = Engine::default();
    let poller = InputPoller::new(42, [key(0x41)], 0, 30).unwrap();
    for (focused, down) in [
        (true, true),
        (true, true),
        (true, false),
        (true, true),
        (false, true),
    ] {
        let report = update(&mut c, &e, Some(8), poller.poll(&Source { focused, down }));
        assert!(report.failures.is_empty());
    }
    assert!(c.is_idle());
    assert_eq!(
        *e.calls.borrow(),
        vec![
            Call::Apply(8, 899000, true),
            Call::Apply(8, 899000, true),
            Call::Remove(8, 899000),
            Call::Apply(8, 899000, true),
            Call::Remove(8, 899000)
        ]
    );
}

#[test]
fn failed_removal_remains_pending_and_retries_without_reapplying() {
    let mut c = controller();
    let e = Engine::default();
    update(&mut c, &e, Some(8), frame(&[key(0x41)]));
    e.fail_remove.set(true);
    let report = update(&mut c, &e, Some(8), frame(&[]));
    assert_eq!(report.cleanup_pending, vec![effect(899000)]);
    assert_eq!(report.failures[0].operation, EffectOperation::Remove);
    assert!(!c.is_idle());
    // SAFETY: Synthetic engine and current target; stop does not access game state.
    let report = unsafe { c.stop(&e, Some(identity(8))) };
    assert_eq!(report.cleared, vec![effect(899000)]);
    assert!(report.cleanup_pending.is_empty() && c.is_idle());
    assert_eq!(e.calls.borrow().len(), 3);
}

#[test]
fn application_failure_keeps_cleanup_obligation_and_input_failure_releases() {
    let mut c = controller();
    let e = Engine::default();
    e.fail_apply.set(true);
    let report = update(&mut c, &e, Some(8), frame(&[key(0x41)]));
    assert_eq!(report.failures[0].operation, EffectOperation::Apply);
    assert!(!c.is_idle());
    let report = update(
        &mut c,
        &e,
        Some(8),
        Err(Error::WindowsCall {
            operation: "synthetic input",
            code: 5,
        }),
    );
    assert!(report.input_error.is_some());
    assert_eq!(report.cleared, vec![effect(899000)]);
    assert!(c.is_idle());
}

#[test]
fn disappeared_or_changed_target_is_reported_without_using_the_old_address() {
    let mut c = controller();
    let e = Engine::default();
    update(&mut c, &e, Some(8), frame(&[key(0x41)]));
    let report = update(&mut c, &e, Some(16), frame(&[key(0x41)]));
    assert_eq!(report.abandoned, vec![effect(899000)]);
    let report = update(&mut c, &e, None, frame(&[key(0x41)]));
    assert_eq!(report.abandoned, vec![effect(899000)]);
    update(&mut c, &e, Some(16), frame(&[]));
    assert!(c.is_idle());
    assert_eq!(
        *e.calls.borrow(),
        vec![Call::Apply(8, 899000, true), Call::Apply(16, 899000, true)]
    );
}

#[test]
fn same_address_with_a_new_identifier_world_or_epoch_abandons_old_cleanup() {
    for changed in [
        CharacterIdentity {
            native_identifier: 999,
            ..identity(8)
        },
        CharacterIdentity {
            world_address: 0x2000,
            ..identity(8)
        },
        CharacterIdentity {
            epoch: 2,
            ..identity(8)
        },
    ] {
        let mut c = controller();
        let e = Engine::default();
        update(&mut c, &e, Some(8), frame(&[key(0x41)]));
        // SAFETY: Synthetic identities and backend only; no native addresses are accessed.
        let report = unsafe { c.update(&e, Some(changed), frame(&[])) };
        assert_eq!(report.abandoned, [effect(899000)]);
        assert!(report.cleared.is_empty());
        assert_eq!(*e.calls.borrow(), [Call::Apply(8, 899000, true)]);
        assert!(c.is_idle());
    }
}

#[test]
fn aliases_hold_an_effect_until_all_bound_inputs_are_released() {
    let mut c = InputEffectController::new([
        InputEffect {
            input: key(0x41),
            effect: effect(7),
        },
        InputEffect {
            input: key(0x42),
            effect: effect(7),
        },
    ])
    .unwrap();
    let e = Engine::default();
    for inputs in [&[key(0x41), key(0x42)][..], &[key(0x42)][..], &[][..]] {
        update(&mut c, &e, Some(8), frame(inputs));
    }
    assert_eq!(
        *e.calls.borrow(),
        vec![
            Call::Apply(8, 7, true),
            Call::Apply(8, 7, true),
            Call::Remove(8, 7)
        ]
    );
}

#[test]
fn rejects_invalid_effect_ids_and_unbounded_configuration() {
    for id in [i32::MIN, -1, 0] {
        assert!(EffectId::new(id).is_err());
    }
    assert!(EffectId::new(i32::MAX).is_ok());
    assert!(
        InputEffectController::new(std::iter::repeat_n(
            InputEffect {
                input: key(0x41),
                effect: effect(7)
            },
            257
        ))
        .is_err()
    );
}
