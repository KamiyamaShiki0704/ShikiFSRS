//! Foreground-scoped input polling and copied frame transitions. Never injects input.
use crate::{Error, Result};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtualKey(u8);
impl VirtualKey {
    pub fn new(code: u16) -> Result<Self> {
        if !(1..=255).contains(&code) {
            return Err(Error::InvalidProfile("invalid virtual key"));
        }
        Ok(Self(code as u8))
    }
    pub fn code(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GamepadButton(u16);
impl GamepadButton {
    pub fn new(mask: u16) -> Result<Self> {
        if mask.count_ones() != 1 || mask & !0xf3ff != 0 {
            return Err(Error::InvalidProfile("invalid or reserved gamepad button"));
        }
        Ok(Self(mask))
    }
    pub fn mask(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Input {
    Key(VirtualKey),
    Button(GamepadButton),
    LeftTrigger,
    RightTrigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForegroundWindow {
    pub handle: usize,
    pub process_id: u32,
}

/// Raw XInput values; no automatic deadzone, normalization, or game interpretation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GamepadState {
    pub packet: u32,
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
}

pub trait InputSource {
    fn foreground(&self) -> Result<Option<ForegroundWindow>>;
    fn key_state(&self, key: VirtualKey) -> Result<i16>;
    /// None means disconnected, not a successful state full of zeroes.
    fn gamepad(&self, index: u8) -> Result<Option<GamepadState>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputFrame {
    pub focused: bool,
    pub down: BTreeSet<Input>,
    pub gamepad: Option<GamepadState>,
    /// Controller errors do not discard valid keyboard samples.
    pub gamepad_error: Option<Error>,
}
impl InputFrame {
    fn inactive() -> Self {
        Self {
            focused: false,
            down: BTreeSet::new(),
            gamepad: None,
            gamepad_error: None,
        }
    }
}

pub struct InputPoller {
    process_id: u32,
    bindings: BTreeSet<Input>,
    gamepad_index: u8,
    trigger_threshold: u8,
}

impl InputPoller {
    pub fn new(
        process_id: u32,
        bindings: impl IntoIterator<Item = Input>,
        gamepad_index: u8,
        trigger_threshold: u8,
    ) -> Result<Self> {
        if process_id == 0 || gamepad_index > 3 {
            return Err(Error::InvalidProfile(
                "invalid input process or controller index",
            ));
        }
        Ok(Self {
            process_id,
            bindings: bindings.into_iter().collect(),
            gamepad_index,
            trigger_threshold,
        })
    }

    pub fn poll(&self, source: &impl InputSource) -> Result<InputFrame> {
        let foreground = source.foreground()?;
        if !foreground
            .is_some_and(|window| window.handle != 0 && window.process_id == self.process_id)
        {
            return Ok(InputFrame::inactive());
        }
        let needs_pad = self
            .bindings
            .iter()
            .any(|input| !matches!(input, Input::Key(_)));
        let (gamepad, gamepad_error) = if needs_pad {
            match source.gamepad(self.gamepad_index) {
                Ok(pad) => (pad, None),
                Err(error) => (None, Some(error)),
            }
        } else {
            (None, None)
        };
        let mut down = BTreeSet::new();
        for input in &self.bindings {
            let pressed = match *input {
                // Only the high bit is current-down state; the low history bit is unreliable.
                Input::Key(key) => source.key_state(key)? < 0,
                Input::Button(button) => gamepad.is_some_and(|pad| pad.buttons & button.0 != 0),
                Input::LeftTrigger => {
                    gamepad.is_some_and(|pad| pad.left_trigger > self.trigger_threshold)
                }
                Input::RightTrigger => {
                    gamepad.is_some_and(|pad| pad.right_trigger > self.trigger_threshold)
                }
            };
            if pressed {
                down.insert(*input);
            }
        }
        // Focus may change between key samples. Reject that frame's active inputs.
        if source.foreground()? != foreground {
            return Ok(InputFrame::inactive());
        }
        Ok(InputFrame {
            focused: true,
            down,
            gamepad,
            gamepad_error,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputUpdate {
    /// Every held input on every frame, for consumers that must reapply while held.
    pub held: BTreeSet<Input>,
    pub pressed: BTreeSet<Input>,
    pub released: BTreeSet<Input>,
    pub error: Option<Error>,
}

#[derive(Default)]
pub struct InputTracker {
    previous: BTreeSet<Input>,
}
impl InputTracker {
    /// A missing target forgets prior state without requesting native cleanup on a missing object.
    /// Poll failures and focus loss release the old state when the target still exists.
    pub fn update(&mut self, frame: Result<InputFrame>, target_present: bool) -> InputUpdate {
        let (held, error) = match frame {
            Ok(frame) => (
                if frame.focused && target_present {
                    frame.down
                } else {
                    BTreeSet::new()
                },
                frame.gamepad_error,
            ),
            Err(error) => (BTreeSet::new(), Some(error)),
        };
        let pressed = held.difference(&self.previous).copied().collect();
        let released = if target_present {
            self.previous.difference(&held).copied().collect()
        } else {
            BTreeSet::new()
        };
        self.previous.clone_from(&held);
        InputUpdate {
            held,
            pressed,
            released,
            error,
        }
    }
}

/// Read-only Windows User32 / XInput backend; not exercised against user input by SDK tests.
#[cfg(windows)]
pub struct WindowsInput;

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::c_void;
    #[repr(C)]
    #[derive(Default)]
    pub(super) struct RawGamepad {
        buttons: u16,
        left_trigger: u8,
        right_trigger: u8,
        left_x: i16,
        left_y: i16,
        right_x: i16,
        right_y: i16,
    }
    #[repr(C)]
    #[derive(Default)]
    pub(super) struct RawState {
        packet: u32,
        pad: RawGamepad,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> *mut c_void;
        fn GetWindowThreadProcessId(window: *mut c_void, process: *mut u32) -> u32;
        fn GetAsyncKeyState(key: i32) -> i16;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLastError() -> u32;
    }
    #[link(name = "xinput")]
    unsafe extern "system" {
        fn XInputGetState(index: u32, state: *mut RawState) -> u32;
    }

    impl InputSource for WindowsInput {
        fn foreground(&self) -> Result<Option<ForegroundWindow>> {
            // SAFETY: Read-only OS function with no arguments.
            let handle = unsafe { GetForegroundWindow() };
            if handle.is_null() {
                return Ok(None);
            }
            let mut process_id = 0;
            // SAFETY: OS-supplied handle and writable u32 out parameter, no retained pointer.
            if unsafe { GetWindowThreadProcessId(handle, &mut process_id) } == 0 {
                // SAFETY: Retrieve the failing OS call's error immediately on this thread.
                return Err(Error::WindowsCall {
                    operation: "GetWindowThreadProcessId",
                    // SAFETY: Retrieve this thread's last OS error immediately.
                    code: unsafe { GetLastError() },
                });
            }
            Ok(Some(ForegroundWindow {
                handle: handle as usize,
                process_id,
            }))
        }
        fn key_state(&self, key: VirtualKey) -> Result<i16> {
            // SAFETY: Validated 1..=255 virtual key; read-only OS polling.
            Ok(unsafe { GetAsyncKeyState(key.0 as i32) })
        }
        fn gamepad(&self, index: u8) -> Result<Option<GamepadState>> {
            if index > 3 {
                return Err(Error::InvalidProfile("invalid XInput index"));
            }
            let mut raw = RawState::default();
            // SAFETY: XInput index in 0..=3 and correctly laid-out writable state, not retained.
            match unsafe { XInputGetState(index as u32, &mut raw) } {
                0 => Ok(Some(GamepadState {
                    packet: raw.packet,
                    buttons: raw.pad.buttons,
                    left_trigger: raw.pad.left_trigger,
                    right_trigger: raw.pad.right_trigger,
                    left_x: raw.pad.left_x,
                    left_y: raw.pad.left_y,
                    right_x: raw.pad.right_x,
                    right_y: raw.pad.right_y,
                })),
                1167 => Ok(None),
                code => Err(Error::WindowsCall {
                    operation: "XInputGetState",
                    code,
                }),
            }
        }
    }
}

#[cfg(all(test, windows))]
mod abi_tests {
    use super::windows::{RawGamepad, RawState};
    #[test]
    fn documented_windows_gamepad_and_state_abi_sizes_are_preserved() {
        assert_eq!(std::mem::size_of::<RawGamepad>(), 12);
        assert_eq!(std::mem::align_of::<RawGamepad>(), 2);
        assert_eq!(std::mem::size_of::<RawState>(), 16);
        assert_eq!(std::mem::align_of::<RawState>(), 4);
    }
}
