//! Experimental menu interpolation and loading-helper pulses, without scene classification.
use crate::objects::{GameObjectCatalog, ObjectKind, ObjectObservation};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const MENU_STATE_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x2a24d0,
        length: 0x3c,
    },
    CodeRange {
        rva: 0x765ef0,
        length: 0x228,
    },
    CodeRange {
        rva: 0x2a2030,
        length: 0x15b,
    },
    CodeRange {
        rva: 0xdf14c0,
        length: 0x6c0,
    },
    CodeRange {
        rva: 0x861d00,
        length: 0x2e,
    },
    CodeRange {
        rva: 0x861d30,
        length: 0x3b,
    },
    CodeRange {
        rva: 0x2a2b90,
        length: 0xb4,
    },
    CodeRange {
        rva: 0x76b530,
        length: 0x4df,
    },
    CodeRange {
        rva: 0x766e90,
        length: 0xdd,
    },
    CodeRange {
        rva: 0x7c2470,
        length: 0x39,
    },
];
pub(crate) const MENU_GLOBAL: u32 = 0x3d6f820;
pub(crate) const LOADING_HELPER_GLOBAL: u32 = 0x3d64f28;
const TRANSITION_OFFSET: u32 = 0x720;
const TRANSITION_TABLE: u32 = 0x2a9ee90;
const TRANSITION_SELECTOR_OFFSET: u32 = 0x14;
const TRANSITION_VALUES_OFFSET: u32 = 0x18;
const PULSES_OFFSET: u32 = 0xec;
const LOADING_SEQUENCE_OFFSET: u32 = 0xd0;
const FRONTEND_GLOBAL: u32 = 0x3d6f8f0;
const FRONTEND_STATE_OFFSET: u32 = 0x653c;

/// Exact frontend singleton and signed word returned by native 7c2470.
/// Unknown values, including -1, remain data; absence is represented separately.
/// No title/loading/HUD visibility or old enum label is inferred from this word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrontendStateObservation {
    pub module_base: u64,
    pub epoch: u64,
    pub frontend: ObjectObservation,
    pub native_state: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuTransitionObservation {
    pub menu: ObjectObservation,
    /// Native signed selector. Every negative value makes the proved getter return zero.
    pub native_selector: i32,
    /// Initial, final, duration and elapsed values as unmodified native bits.
    pub value_bits: [u32; 4],
}
impl MenuTransitionObservation {
    /// Pure copied-value equivalent of the ordinary getter for finite used operands.
    /// Not a timer, loading-screen test, render state, native call or interpolation update.
    pub fn interpolated_value(&self) -> Result<f32> {
        if self.native_selector < 0 {
            return Ok(0.0);
        }
        let [initial, final_value, duration, elapsed] = self.value_bits.map(f32::from_bits);
        if !elapsed.is_finite() {
            return Err(Error::InvalidProfile("nonfinite menu elapsed value"));
        }
        let result = if elapsed <= 0.0 {
            initial
        } else {
            if !duration.is_finite() {
                return Err(Error::InvalidProfile("nonfinite menu duration"));
            }
            if elapsed < duration {
                (final_value - initial) * (elapsed / duration) + initial
            } else {
                final_value
            }
        };
        if !result.is_finite() {
            return Err(Error::InvalidProfile("unrepresentable menu interpolation"));
        }
        Ok(result)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadingPulseObservation {
    pub helper: ObjectObservation,
    /// Native tick copies current to previous then clears current. Values are not normalized.
    pub current: u8,
    pub previous: u8,
}

/// Raw loading-helper choice sequence, not a monotonic counter or scene generation.
/// Native 2a24d0 increments this word modulo 34 when selecting a loading message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadingSequenceObservation {
    pub module_base: u64,
    pub epoch: u64,
    pub helper: ObjectObservation,
    pub native_sequence: i32,
}

fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    pub fn loading_sequence(&self, epoch: u64) -> Result<Option<LoadingSequenceObservation>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero loading sequence epoch"));
        }
        let Some(helper) = self.singleton(LOADING_HELPER_GLOBAL, ObjectKind::LoadingHelper)? else {
            return Ok(None);
        };
        let memory = self.module.memory();
        let address = at(helper.address, LOADING_SEQUENCE_OFFSET)?;
        let native_sequence = memory.read_u32(address)? as i32;
        if memory.read_u32(address)? as i32 != native_sequence
            || self.singleton(LOADING_HELPER_GLOBAL, ObjectKind::LoadingHelper)? != Some(helper)
        {
            return Err(Error::InvalidProfile("loading sequence source changed"));
        }
        Ok(Some(LoadingSequenceObservation {
            module_base: self.module.base(),
            epoch,
            helper,
            native_sequence,
        }))
    }
    /// Copies only the proved signed state, with exact singleton/type/value
    /// rechecks. No world/player is required and no protected reset is called.
    pub fn frontend_state(&self, epoch: u64) -> Result<Option<FrontendStateObservation>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero frontend observation epoch"));
        }
        let Some(frontend) = self.singleton(FRONTEND_GLOBAL, ObjectKind::Frontend)? else {
            return Ok(None);
        };
        let memory = self.module.memory();
        let address = at(frontend.address, FRONTEND_STATE_OFFSET)?;
        let native_state = memory.read_u32(address)? as i32;
        if memory.read_u32(address)? as i32 != native_state
            || self.singleton(FRONTEND_GLOBAL, ObjectKind::Frontend)? != Some(frontend)
        {
            return Err(Error::InvalidProfile("frontend state source changed"));
        }
        Ok(Some(FrontendStateObservation {
            module_base: self.module.base(),
            epoch,
            frontend,
            native_state,
        }))
    }

    pub fn menu_transition(&self) -> Result<Option<MenuTransitionObservation>> {
        let Some(menu) = self.singleton(MENU_GLOBAL, ObjectKind::MenuManager)? else {
            return Ok(None);
        };
        let memory = self.module.memory();
        let embedded = at(menu.address, TRANSITION_OFFSET)?;
        let table = self
            .module
            .base()
            .checked_add(u64::from(TRANSITION_TABLE))
            .ok_or(Error::AddressOverflow)?;
        let read = || -> Result<(i32, [u32; 4])> {
            if memory.read_u64(embedded)? != table {
                return Err(Error::InvalidProfile("wrong menu transition primary table"));
            }
            let selector = memory.read_u32(at(embedded, TRANSITION_SELECTOR_OFFSET)?)? as i32;
            let mut bits = [0; 16];
            memory.read_exact(at(embedded, TRANSITION_VALUES_OFFSET)?, &mut bits)?;
            Ok((
                selector,
                std::array::from_fn(|i| {
                    u32::from_le_bytes(bits[i * 4..i * 4 + 4].try_into().unwrap())
                }),
            ))
        };
        let (native_selector, value_bits) = read()?;
        if read()? != (native_selector, value_bits)
            || self.singleton(MENU_GLOBAL, ObjectKind::MenuManager)? != Some(menu)
        {
            return Err(Error::InvalidProfile(
                "menu transition changed during observation",
            ));
        }
        Ok(Some(MenuTransitionObservation {
            menu,
            native_selector,
            value_bits,
        }))
    }

    /// Copies exactly the two initialized pulse bytes. Adjacent +ee/+ef are not read.
    /// Missing helper remains None; nonzero pulses alone do not prove a loading screen.
    pub fn loading_helper_pulses(&self) -> Result<Option<LoadingPulseObservation>> {
        let Some(helper) = self.singleton(LOADING_HELPER_GLOBAL, ObjectKind::LoadingHelper)? else {
            return Ok(None);
        };
        let memory = self.module.memory();
        let mut pulses = [0; 2];
        memory.read_exact(at(helper.address, PULSES_OFFSET)?, &mut pulses)?;
        let mut after = [0; 2];
        memory.read_exact(at(helper.address, PULSES_OFFSET)?, &mut after)?;
        if pulses != after
            || self.singleton(LOADING_HELPER_GLOBAL, ObjectKind::LoadingHelper)? != Some(helper)
        {
            return Err(Error::InvalidProfile(
                "loading helper pulses changed during observation",
            ));
        }
        Ok(Some(LoadingPulseObservation {
            helper,
            current: pulses[0],
            previous: pulses[1],
        }))
    }
}
