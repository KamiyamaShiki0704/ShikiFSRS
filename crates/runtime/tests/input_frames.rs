//! Original input fixtures; no keyboard, controller or game polling occurs here.
use shikifsrs_runtime::{
    Error, Result,
    input::{
        ForegroundWindow, GamepadButton, GamepadState, Input, InputFrame, InputPoller, InputSource,
        InputTracker, VirtualKey,
    },
};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
};

struct Source {
    foreground: Option<ForegroundWindow>,
    keys: BTreeMap<u8, i16>,
    pad: Result<Option<GamepadState>>,
    reads: Cell<usize>,
    focus_changes: bool,
    key_reads: Cell<usize>,
    pad_reads: Cell<usize>,
}
impl Source {
    fn focused() -> Self {
        Self {
            foreground: Some(ForegroundWindow {
                handle: 100,
                process_id: 42,
            }),
            keys: BTreeMap::new(),
            pad: Ok(None),
            reads: Cell::new(0),
            focus_changes: false,
            key_reads: Cell::new(0),
            pad_reads: Cell::new(0),
        }
    }
}
impl InputSource for Source {
    fn foreground(&self) -> Result<Option<ForegroundWindow>> {
        let previous = self.reads.replace(self.reads.get() + 1);
        Ok(if self.focus_changes && previous > 0 {
            None
        } else {
            self.foreground
        })
    }
    fn key_state(&self, key: VirtualKey) -> Result<i16> {
        self.key_reads.set(self.key_reads.get() + 1);
        self.keys
            .get(&key.code())
            .copied()
            .ok_or(Error::NativeOperationRejected("synthetic key read failed"))
    }
    fn gamepad(&self, index: u8) -> Result<Option<GamepadState>> {
        assert_eq!(index, 0);
        self.pad_reads.set(self.pad_reads.get() + 1);
        self.pad.clone()
    }
}
fn key() -> Input {
    Input::Key(VirtualKey::new(65).unwrap())
}
fn button() -> Input {
    Input::Button(GamepadButton::new(0x1000).unwrap())
}
fn poller(inputs: &[Input]) -> InputPoller {
    InputPoller::new(42, inputs.iter().copied(), 0, 30).unwrap()
}
fn frame(inputs: &[Input]) -> Result<InputFrame> {
    Ok(InputFrame {
        focused: true,
        down: inputs.iter().copied().collect(),
        gamepad: None,
        gamepad_error: None,
    })
}

#[test]
fn key_history_bit_never_becomes_down_and_duplicate_bindings_poll_once() {
    for (state, held) in [
        (0, false),
        (1, false),
        (i16::MIN, true),
        (i16::MIN + 1, true),
    ] {
        let mut source = Source::focused();
        source.keys.insert(65, state);
        let snapshot = poller(&[key(), key()]).poll(&source).unwrap();
        assert_eq!(snapshot.down.contains(&key()), held);
        assert_eq!(source.key_reads.get(), 1);
        assert_eq!(source.pad_reads.get(), 0);
    }
}

#[test]
fn unfocused_invalid_or_changed_windows_never_deliver_held_inputs() {
    for window in [
        None,
        Some(ForegroundWindow {
            handle: 0,
            process_id: 42,
        }),
        Some(ForegroundWindow {
            handle: 100,
            process_id: 43,
        }),
    ] {
        let mut source = Source::focused();
        source.foreground = window;
        assert!(!poller(&[key(), button()]).poll(&source).unwrap().focused);
        assert_eq!(source.key_reads.get(), 0);
        assert_eq!(source.pad_reads.get(), 0);
    }
    let mut source = Source::focused();
    source.keys.insert(65, i16::MIN);
    source.focus_changes = true;
    let snapshot = poller(&[key()]).poll(&source).unwrap();
    assert!(!snapshot.focused);
    assert!(snapshot.down.is_empty());
}

#[test]
fn controller_buttons_and_strict_trigger_threshold_keep_analog_values() {
    let mut source = Source::focused();
    source.pad = Ok(Some(GamepadState {
        packet: u32::MAX,
        buttons: 0x1000,
        left_trigger: 30,
        right_trigger: 31,
        left_x: i16::MIN,
        right_y: i16::MAX,
        ..Default::default()
    }));
    let snapshot = poller(&[button(), Input::LeftTrigger, Input::RightTrigger])
        .poll(&source)
        .unwrap();
    assert_eq!(
        snapshot.down,
        BTreeSet::from([button(), Input::RightTrigger])
    );
    assert_eq!(snapshot.gamepad.unwrap().left_x, i16::MIN);
    assert_eq!(snapshot.gamepad.unwrap().right_y, i16::MAX);
    assert_eq!(source.pad_reads.get(), 1);
    source.pad = Ok(None);
    let snapshot = poller(&[button(), Input::LeftTrigger])
        .poll(&source)
        .unwrap();
    assert!(snapshot.focused);
    assert!(snapshot.down.is_empty());
    assert_eq!(snapshot.gamepad, None);
}

#[test]
fn device_failure_preserves_keyboard_and_surfaces_the_xinput_return_code() {
    let mut source = Source::focused();
    source.keys.insert(65, i16::MIN);
    let error = Error::WindowsCall {
        operation: "XInputGetState",
        code: 5,
    };
    source.pad = Err(error.clone());
    let snapshot = poller(&[key(), button()]).poll(&source).unwrap();
    assert_eq!(snapshot.down, BTreeSet::from([key()]));
    let update = InputTracker::default().update(Ok(snapshot), true);
    assert_eq!(update.error, Some(error));
    assert_eq!(update.held, BTreeSet::from([key()]));
}

#[test]
fn held_inputs_repeat_but_press_and_release_edges_happen_once() {
    let mut tracker = InputTracker::default();
    let first = tracker.update(frame(&[key(), button()]), true);
    assert_eq!(first.pressed, first.held);
    assert!(first.released.is_empty());
    let next = tracker.update(frame(&[key(), button()]), true);
    assert_eq!(next.held, first.held);
    assert!(next.pressed.is_empty());
    let release = tracker.update(frame(&[button()]), true);
    assert_eq!(release.released, BTreeSet::from([key()]));
    assert!(tracker.update(frame(&[button()]), true).released.is_empty());
    let unfocused = InputFrame {
        focused: false,
        down: BTreeSet::from([button()]),
        gamepad: None,
        gamepad_error: None,
    };
    assert_eq!(
        tracker.update(Ok(unfocused), true).released,
        BTreeSet::from([button()])
    );
    assert!(tracker.update(frame(&[]), true).released.is_empty());
}

#[test]
fn missing_character_forgets_effect_state_without_native_cleanup_and_reappearance_reapplies() {
    let mut tracker = InputTracker::default();
    tracker.update(frame(&[key()]), true);
    let absent = tracker.update(frame(&[key()]), false);
    assert!(absent.held.is_empty());
    assert!(absent.released.is_empty());
    assert_eq!(
        tracker.update(frame(&[key()]), true).pressed,
        BTreeSet::from([key()])
    );
    let failed = tracker.update(
        Err(Error::NativeOperationRejected("synthetic focus failure")),
        true,
    );
    assert_eq!(failed.released, BTreeSet::from([key()]));
    assert!(failed.error.is_some());
    assert!(tracker.update(frame(&[]), true).released.is_empty());
}

#[test]
fn invalid_process_controller_keys_and_reserved_or_combined_buttons_are_rejected() {
    assert!(InputPoller::new(0, [], 0, 30).is_err());
    assert!(InputPoller::new(42, [], 4, 30).is_err());
    for code in [0, 256, u16::MAX] {
        assert!(VirtualKey::new(code).is_err());
    }
    for mask in [0, 3, 0x400, 0x800, 0xf000] {
        assert!(GamepadButton::new(mask).is_err());
    }
    let source = Source::focused();
    assert!(poller(&[key()]).poll(&source).is_err());
}
