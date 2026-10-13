//! Sparse copied frontend HUD fields; selection policy is supplied by the consumer.
use crate::objects::{GameObjectCatalog, ObjectKind, ObjectObservation};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const FRIENDLY_HUD_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x4377e0,
        length: 0xf,
    },
    CodeRange {
        rva: 0x437820,
        length: 0x7,
    },
    CodeRange {
        rva: 0x76bd00,
        length: 0x22c,
    },
    CodeRange {
        rva: 0x76b1c0,
        length: 0xdc,
    },
    CodeRange {
        rva: 0x76c5f0,
        length: 0x9a,
    },
    CodeRange {
        rva: 0x76bab0,
        length: 0xa0,
    },
    CodeRange {
        rva: 0x76d7b0,
        length: 0x19b,
    },
    CodeRange {
        rva: 0x774fd0,
        length: 0x6d7,
    },
    CodeRange {
        rva: 0x778200,
        length: 0xd71,
    },
    CodeRange {
        rva: 0x8d3a60,
        length: 0x8f,
    },
    CodeRange {
        rva: 0x8c9d10,
        length: 0xece,
    },
    CodeRange {
        rva: 0x8c33f0,
        length: 0x433,
    },
    CodeRange {
        rva: 0x8c2150,
        length: 0x345,
    },
    CodeRange {
        rva: 0x771a20,
        length: 0x4d2,
    },
    CodeRange {
        rva: 0x76ece0,
        length: 0x21,
    },
];
const FRONTEND_GLOBAL: u32 = 0x3d6f8f0;
const VALUES_OFFSET: u32 = 0x80;
const DISPLAY_OFFSET: u32 = 0x5c60;
const DISPLAY_STRIDE: u32 = 0xb0;
const DISPLAY_VISIBLE: u32 = 0x10;
const DISPLAY_OFFSCREEN: u32 = 0x12;
const DISPLAY_COLOR: u32 = 0x13;
const DISPLAY_HP: u32 = 0x78;
const DISPLAY_DAMAGE_DELTA: u32 = 0x8c;
const DISPLAY_HANDLE: u32 = 0x98;
const DISPLAY_DOWNSCALED: u32 = 0xa4;
const TAG_OFFSET: u32 = 0x1e88;
const TAG_STRIDE: u32 = 0x128;
const TAG_FLAGS: u32 = 0x0;
const TAG_HANDLE: u32 = 0x8;
const TAG_HP: u32 = 0x18;
const TAG_DAMAGE: u32 = 0xdc;
const TAG_COLOR: u32 = 0xe4;
const TAG_DOWNSCALED: u32 = 0xe5;
const SPIRIT_GROUP: u32 = 0x4cd8;
const SPIRIT_COUNT: u32 = 0x80;
const SPIRIT_DATA: u32 = 0x88;
const SPIRIT_STRIDE: u32 = 0x18;
const VIEW_VALUES: u32 = 0x25c00;
const VIEW_RENDER_VALUES: u32 = 0xa98;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FriendlyDisplay {
    pub address: u64,
    pub visible: u8,
    pub offscreen: u8,
    pub name_color: u8,
    pub downscaled: u8,
    pub identifier: u64,
    /// Four native gauge words in source order. No float reinterpretation.
    pub hp_words: [u32; 4],
    pub damage_delta_bits: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FriendlyTag {
    pub address: u64,
    /// Visible, update-position and offscreen; fourth byte remains opaque/unread.
    pub flags: [u8; 3],
    pub name_color: u8,
    pub downscaled: u8,
    pub identifier: u64,
    /// HP, recoverable HP, uncapped difference, uncapped maximum.
    pub hp_words: [u32; 4],
    pub last_damage: u32,
    pub last_hp: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpiritDisplay {
    pub address: u64,
    pub identifier: u64,
    pub hp_words: [u32; 4],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrontendHudObservation {
    pub module_base: u64,
    pub epoch: u64,
    pub frontend: ObjectObservation,
    pub values_address: u64,
    pub spirit_count: u32,
    pub spirits: [SpiritDisplay; 5],
    pub displays: [FriendlyDisplay; 7],
    pub tags: [FriendlyTag; 7],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrontendViewLink {
    pub view: ObjectObservation,
    pub frontend: ObjectObservation,
    pub values_address: u64,
    pub module_base: u64,
    pub epoch: u64,
}
fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}
fn words(memory: &impl AddressSpace, address: u64) -> Result<[u32; 4]> {
    let mut raw = [0; 16];
    memory.read_exact(address, &mut raw)?;
    Ok(std::array::from_fn(|i| {
        u32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Exact frontend and two full sparse copies. No names, strings, UI objects or resources.
    pub fn frontend_hud(&self, epoch: u64) -> Result<Option<FrontendHudObservation>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero frontend HUD lifetime epoch"));
        }
        let Some(frontend) = self.singleton(FRONTEND_GLOBAL, ObjectKind::Frontend)? else {
            if self
                .singleton(FRONTEND_GLOBAL, ObjectKind::Frontend)?
                .is_some()
            {
                return Err(Error::InvalidProfile("frontend appeared during HUD copy"));
            }
            return Ok(None);
        };
        let copy = || -> Result<FrontendHudObservation> {
            let memory = self.module.memory();
            let values = at(frontend.address, VALUES_OFFSET)?;
            let mut displays = Vec::with_capacity(7);
            let mut tags = Vec::with_capacity(7);
            for index in 0..7 {
                let address = at(frontend.address, DISPLAY_OFFSET + index * DISPLAY_STRIDE)?;
                displays.push(FriendlyDisplay {
                    address,
                    visible: memory.read_u8(at(address, DISPLAY_VISIBLE)?)?,
                    offscreen: memory.read_u8(at(address, DISPLAY_OFFSCREEN)?)?,
                    name_color: memory.read_u8(at(address, DISPLAY_COLOR)?)?,
                    downscaled: memory.read_u8(at(address, DISPLAY_DOWNSCALED)?)?,
                    identifier: memory.read_u64(at(address, DISPLAY_HANDLE)?)?,
                    hp_words: words(memory, at(address, DISPLAY_HP)?)?,
                    damage_delta_bits: memory.read_u32(at(address, DISPLAY_DAMAGE_DELTA)?)?,
                });
                let address = at(values, TAG_OFFSET + index * TAG_STRIDE)?;
                let mut flags = [0; 3];
                memory.read_exact(at(address, TAG_FLAGS)?, &mut flags)?;
                tags.push(FriendlyTag {
                    address,
                    flags,
                    name_color: memory.read_u8(at(address, TAG_COLOR)?)?,
                    downscaled: memory.read_u8(at(address, TAG_DOWNSCALED)?)?,
                    identifier: memory.read_u64(at(address, TAG_HANDLE)?)?,
                    hp_words: words(memory, at(address, TAG_HP)?)?,
                    last_damage: memory.read_u32(at(address, TAG_DAMAGE)?)?,
                    last_hp: memory.read_u32(at(address, TAG_DAMAGE + 4)?)?,
                });
            }
            let group = at(values, SPIRIT_GROUP)?;
            let mut spirits = Vec::with_capacity(5);
            for index in 0..5 {
                let address = at(group, SPIRIT_DATA + index * SPIRIT_STRIDE)?;
                spirits.push(SpiritDisplay {
                    address,
                    identifier: memory.read_u64(address)?,
                    hp_words: words(memory, at(address, 8)?)?,
                });
            }
            Ok(FrontendHudObservation {
                module_base: self.module.base(),
                epoch,
                frontend,
                values_address: values,
                spirit_count: memory.read_u32(at(group, SPIRIT_COUNT)?)?,
                spirits: spirits
                    .try_into()
                    .map_err(|_| Error::InvalidProfile("spirit HUD slot count"))?,
                displays: displays
                    .try_into()
                    .map_err(|_| Error::InvalidProfile("friendly display count"))?,
                tags: tags
                    .try_into()
                    .map_err(|_| Error::InvalidProfile("friendly tag count"))?,
            })
        };
        let first = copy()?;
        if copy()? != first
            || self.singleton(FRONTEND_GLOBAL, ObjectKind::Frontend)? != Some(frontend)
        {
            return Err(Error::InvalidProfile("frontend HUD changed during copy"));
        }
        Ok(Some(first))
    }
    /// Optional exact view provided by a known hook/caller. Both native values pointers must
    /// alias this frontend's embedded values; therefore duplicate writes are unnecessary.
    /// Does not guess an unproved Frontend->view pointer or follow shared ownership graphs.
    pub fn frontend_view_link(
        &self,
        view_address: u64,
        epoch: u64,
    ) -> Result<Option<FrontendViewLink>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero frontend view lifetime epoch"));
        }
        let Some(frontend) = self.singleton(FRONTEND_GLOBAL, ObjectKind::Frontend)? else {
            return Ok(None);
        };
        let Some(view) = self.require(view_address, ObjectKind::FrontendView)? else {
            return Ok(None);
        };
        let values_address = at(frontend.address, VALUES_OFFSET)?;
        let memory = self.module.memory();
        for _ in 0..2 {
            if memory.read_u64(at(view.address, VIEW_VALUES)?)? != values_address
                || memory.read_u64(at(view.address, VIEW_RENDER_VALUES)?)? != values_address
            {
                return Err(Error::InvalidProfile(
                    "frontend view values alias superseded",
                ));
            }
        }
        if self.require(view.address, ObjectKind::FrontendView)? != Some(view)
            || self.singleton(FRONTEND_GLOBAL, ObjectKind::Frontend)? != Some(frontend)
        {
            return Err(Error::InvalidProfile("frontend view link changed"));
        }
        Ok(Some(FrontendViewLink {
            view,
            frontend,
            values_address,
            module_base: self.module.base(),
            epoch,
        }))
    }
}

/// Independent of visibility/debug flags. Full handles match exactly; raw colors remain data.
pub struct HudSelection<'a> {
    pub identifiers: &'a [u64],
    pub name_color: Option<u8>,
    pub include_downscaled: bool,
}
impl HudSelection<'_> {
    pub fn matches(&self, identifier: u64, name_color: u8, downscaled: u8) -> bool {
        self.identifiers.contains(&identifier)
            || self.name_color == Some(name_color)
            || (self.include_downscaled && downscaled != 0)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudFieldStore {
    pub address: usize,
    pub before: Vec<u8>,
    pub written: Vec<u8>,
}
#[derive(Debug)]
pub struct FrontendHudWrite {
    pub source: FrontendHudObservation,
    pub selected_displays: Vec<usize>,
    pub selected_tags: Vec<usize>,
    pub stores: Vec<HudFieldStore>,
    pub postflight: Result<FrontendHudObservation>,
}
pub(crate) struct HudPlan {
    source: FrontendHudObservation,
    expected: FrontendHudObservation,
    displays: Vec<usize>,
    tags: Vec<usize>,
    stores: Vec<HudFieldStore>,
}
fn push(
    stores: &mut Vec<HudFieldStore>,
    address: u64,
    before: &[u8],
    written: &[u8],
) -> Result<()> {
    if before == written {
        return Ok(());
    }
    let address = usize::try_from(address).map_err(|_| Error::AddressOverflow)?;
    address
        .checked_add(written.len())
        .ok_or(Error::AddressOverflow)?;
    if address == 0 {
        return Err(Error::InvalidProfile("null frontend HUD destination"));
    }
    stores.push(HudFieldStore {
        address,
        before: before.to_vec(),
        written: written.to_vec(),
    });
    Ok(())
}
pub(crate) fn hud_plan(
    current: Option<FrontendHudObservation>,
    source: FrontendHudObservation,
    selection: HudSelection<'_>,
    suppress_spirits: bool,
) -> Result<HudPlan> {
    if source.epoch == 0 || current != Some(source) || selection.identifiers.len() > 256 {
        return Err(Error::InvalidProfile(
            "frontend HUD source superseded or selection unbounded",
        ));
    }
    let mut expected = source;
    let mut stores = Vec::new();
    let mut displays = Vec::new();
    let mut tags = Vec::new();
    for (index, value) in expected.displays.iter_mut().enumerate() {
        if !selection.matches(value.identifier, value.name_color, value.downscaled) {
            continue;
        }
        displays.push(index);
        push(
            &mut stores,
            at(value.address, DISPLAY_VISIBLE)?,
            &[value.visible],
            &[0],
        )?;
        push(
            &mut stores,
            at(value.address, DISPLAY_OFFSCREEN)?,
            &[value.offscreen],
            &[0],
        )?;
        push(
            &mut stores,
            at(value.address, DISPLAY_HANDLE)?,
            &value.identifier.to_le_bytes(),
            &u64::MAX.to_le_bytes(),
        )?;
        for (i, word) in value.hp_words.iter_mut().enumerate() {
            push(
                &mut stores,
                at(value.address, DISPLAY_HP + i as u32 * 4)?,
                &word.to_le_bytes(),
                &0u32.to_le_bytes(),
            )?;
            *word = 0;
        }
        push(
            &mut stores,
            at(value.address, DISPLAY_DAMAGE_DELTA)?,
            &value.damage_delta_bits.to_le_bytes(),
            &999.0f32.to_bits().to_le_bytes(),
        )?;
        value.visible = 0;
        value.offscreen = 0;
        value.identifier = u64::MAX;
        value.damage_delta_bits = 999.0f32.to_bits();
    }
    for (index, value) in expected.tags.iter_mut().enumerate() {
        if !selection.matches(value.identifier, value.name_color, value.downscaled) {
            continue;
        }
        tags.push(index);
        push(
            &mut stores,
            at(value.address, TAG_FLAGS)?,
            &value.flags,
            &[0; 3],
        )?;
        push(
            &mut stores,
            at(value.address, TAG_HANDLE)?,
            &value.identifier.to_le_bytes(),
            &u64::MAX.to_le_bytes(),
        )?;
        // Consumer clears HP/difference/uncapped, preserving recoverable HP and screen coords.
        for i in [0, 2, 3] {
            push(
                &mut stores,
                at(value.address, TAG_HP + i as u32 * 4)?,
                &value.hp_words[i].to_le_bytes(),
                &0u32.to_le_bytes(),
            )?;
            value.hp_words[i] = 0;
        }
        push(
            &mut stores,
            at(value.address, TAG_DAMAGE)?,
            &value.last_damage.to_le_bytes(),
            &0u32.to_le_bytes(),
        )?;
        push(
            &mut stores,
            at(value.address, TAG_DAMAGE + 4)?,
            &value.last_hp.to_le_bytes(),
            &0u32.to_le_bytes(),
        )?;
        value.flags = [0; 3];
        value.identifier = u64::MAX;
        value.last_damage = 0;
        value.last_hp = 0;
    }
    if suppress_spirits {
        push(
            &mut stores,
            at(at(source.values_address, SPIRIT_GROUP)?, SPIRIT_COUNT)?,
            &source.spirit_count.to_le_bytes(),
            &0u32.to_le_bytes(),
        )?;
        expected.spirit_count = 0;
        for value in &mut expected.spirits {
            push(
                &mut stores,
                value.address,
                &value.identifier.to_le_bytes(),
                &u64::MAX.to_le_bytes(),
            )?;
            for (i, word) in value.hp_words.iter_mut().enumerate() {
                push(
                    &mut stores,
                    at(value.address, 8 + i as u32 * 4)?,
                    &word.to_le_bytes(),
                    &0u32.to_le_bytes(),
                )?;
                *word = 0;
            }
            value.identifier = u64::MAX;
        }
    }
    Ok(HudPlan {
        source,
        expected,
        displays,
        tags,
        stores,
    })
}
pub(crate) fn commit_hud(
    plan: HudPlan,
    mut store: impl FnMut(usize, &[u8]),
    observe: impl FnOnce() -> Result<Option<FrontendHudObservation>>,
) -> FrontendHudWrite {
    for item in &plan.stores {
        store(item.address, &item.written);
    }
    let postflight = observe().and_then(|value| {
        if value == Some(plan.expected) {
            Ok(plan.expected)
        } else {
            Err(Error::InvalidProfile("frontend HUD changed after stores"))
        }
    });
    FrontendHudWrite {
        source: plan.source,
        selected_displays: plan.displays,
        selected_tags: plan.tags,
        stores: plan.stores,
        postflight,
    }
}
