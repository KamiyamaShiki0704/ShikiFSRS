//! keyboard-ex's complete input/effect requirement preset; no native call or task installation.
use crate::effects::{EffectId, InputEffect};
use shikifsrs_runtime::{
    Result,
    input::{GamepadButton, Input, VirtualKey},
};

/// All 87 original bindings in consumer order. A caller may feed these into InputEffectTask.
/// Key codes are Windows VKs; gamepad masks are XInput buttons. FrameBegin task polling uses
/// controller0 and strict trigger>30. Effects must exist in the consumer's game PARAM setup.
/// This constructor acquires no input state, character lifetime or native execution authority.
pub fn keyboard_ex_bindings() -> Result<Vec<InputEffect>> {
    let mut out = Vec::with_capacity(87);
    for (start, effect, count) in [
        (0x41, 899000, 26),
        (0x30, 899100, 10),
        (0x25, 899200, 4),
        (0x70, 899400, 12),
    ] {
        for offset in 0..count {
            out.push(InputEffect {
                input: Input::Key(VirtualKey::new(start + offset)?),
                effect: EffectId::new(effect + i32::from(offset))?,
            });
        }
    }
    for (effect, codes) in [
        (
            899500,
            &[0x20, 0x0d, 0x1b, 0x08, 0x09, 0x10, 0x11, 0x12][..],
        ),
        (
            899600,
            &[
                0xba, 0xbb, 0xbc, 0xbd, 0xbe, 0xbf, 0xc0, 0xdb, 0xdc, 0xdd, 0xde,
            ][..],
        ),
    ] {
        for (offset, &code) in codes.iter().enumerate() {
            out.push(InputEffect {
                input: Input::Key(VirtualKey::new(code)?),
                effect: EffectId::new(effect + offset as i32)?,
            });
        }
    }
    // Native XInput numbering is not alphabetical; preserve the consumer's effect ordering.
    for (offset, mask) in [
        0x1000, 0x2000, 0x4000, 0x8000, 0x100, 0x200, 0, 0, 0x20, 0x10, 0x40, 0x80, 1, 2, 4, 8,
    ]
    .into_iter()
    .enumerate()
    {
        let input = match offset {
            6 => Input::LeftTrigger,
            7 => Input::RightTrigger,
            _ => Input::Button(GamepadButton::new(mask)?),
        };
        out.push(InputEffect {
            input,
            effect: EffectId::new(899700 + offset as i32)?,
        });
    }
    Ok(out)
}
