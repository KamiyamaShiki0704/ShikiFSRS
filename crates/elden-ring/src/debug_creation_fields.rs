//! Conditional preparation of individually proved creator input words. No submit switch.
use crate::debug_creator::{
    ANIMATION_OFFSET, CHARA_INIT_ID_OFFSET, DebugCreatorObservation, INPUT_OFFSET,
};
use shikifsrs_runtime::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreatorInputWord {
    PositionLane(u8),
    RotationLane(u8),
    NpcParamId,
    NpcThinkParamId,
    EventEntityId,
    /// Input+4c -> ordinary factory data+58 -> exact CSChrTalkModule stored ID+10.
    TalkId,
    CharaInitParamId,
    AnimationId,
}
impl CreatorInputWord {
    fn offset(self) -> Result<u64> {
        Ok(u64::from(match self {
            Self::PositionLane(i) if i < 4 => INPUT_OFFSET + u32::from(i) * 4,
            Self::RotationLane(i) if i < 4 => INPUT_OFFSET + 0x10 + u32::from(i) * 4,
            Self::NpcParamId => INPUT_OFFSET + 0x40,
            Self::NpcThinkParamId => INPUT_OFFSET + 0x44,
            Self::EventEntityId => INPUT_OFFSET + 0x48,
            Self::TalkId => INPUT_OFFSET + 0x4c,
            Self::CharaInitParamId => INPUT_OFFSET + CHARA_INIT_ID_OFFSET,
            Self::AnimationId => ANIMATION_OFFSET,
            _ => return Err(Error::InvalidProfile("invalid creator vector lane")),
        }))
    }
    /// Copied bits only; signed PARAM/animation IDs retain all bits, including -1 sentinel.
    pub fn bits(self, copy: &DebugCreatorObservation) -> Result<u32> {
        self.offset()?;
        Ok(match self {
            Self::PositionLane(i) => copy.input.position_bits[usize::from(i)],
            Self::RotationLane(i) => copy.input.rotation_bits[usize::from(i)],
            Self::NpcParamId => copy.input.npc_param_id as u32,
            Self::NpcThinkParamId => copy.input.npc_think_param_id as u32,
            Self::EventEntityId => copy.input.event_entity_id,
            Self::TalkId => copy.input.auxiliary_id4c as u32,
            Self::CharaInitParamId => copy.input.chara_init_param_id as u32,
            Self::AnimationId => copy.animation_id as u32,
        })
    }
    pub(crate) fn replace_copy(self, copy: &mut DebugCreatorObservation, bits: u32) {
        match self {
            Self::PositionLane(i) => copy.input.position_bits[usize::from(i)] = bits,
            Self::RotationLane(i) => copy.input.rotation_bits[usize::from(i)] = bits,
            Self::NpcParamId => copy.input.npc_param_id = bits as i32,
            Self::NpcThinkParamId => copy.input.npc_think_param_id = bits as i32,
            Self::EventEntityId => copy.input.event_entity_id = bits,
            Self::TalkId => copy.input.auxiliary_id4c = bits as i32,
            Self::CharaInitParamId => copy.input.chara_init_param_id = bits as i32,
            Self::AnimationId => copy.animation_id = bits as i32,
        }
    }
}
#[derive(Debug)]
pub struct CreatorInputWordWrite {
    pub source: DebugCreatorObservation,
    pub field: CreatorInputWord,
    pub before_bits: u32,
    pub after_bits: u32,
    /// An actual four-byte store occurred; postflight failure does not revoke that fact.
    pub changed: bool,
    pub postflight: Result<DebugCreatorObservation>,
}
pub(crate) struct WordPlan {
    source: DebugCreatorObservation,
    after: DebugCreatorObservation,
    field: CreatorInputWord,
    address: usize,
    before_bits: u32,
    bits: u32,
}
pub(crate) fn word_plan(
    current: Option<DebugCreatorObservation>,
    expected: DebugCreatorObservation,
    field: CreatorInputWord,
    bits: u32,
) -> Result<WordPlan> {
    let offset = field.offset()?;
    if matches!(
        field,
        CreatorInputWord::PositionLane(_) | CreatorInputWord::RotationLane(_)
    ) && !f32::from_bits(bits).is_finite()
    {
        return Err(Error::InvalidProfile("nonfinite creator vector input"));
    }
    if expected.epoch == 0 || expected.count != 1 || expected.last_created_address != 0 {
        return Err(Error::NativeOperationRejected(
            "creator preparation requires single idle-owned input and empty result",
        ));
    }
    if current.as_ref() != Some(&expected) {
        return Err(Error::NativeOperationRejected(
            "creator preparation source superseded",
        ));
    }
    let address = expected
        .object
        .address
        .checked_add(offset)
        .ok_or(Error::AddressOverflow)?;
    address.checked_add(4).ok_or(Error::AddressOverflow)?;
    if expected.object.address == 0
        || !expected.object.address.is_multiple_of(16)
        || !address.is_multiple_of(4)
    {
        return Err(Error::InvalidProfile(
            "unaligned creator preparation storage",
        ));
    }
    let before_bits = field.bits(&expected)?;
    let mut after = expected.clone();
    field.replace_copy(&mut after, bits);
    Ok(WordPlan {
        source: expected,
        after,
        field,
        address: usize::try_from(address).map_err(|_| Error::AddressOverflow)?,
        before_bits,
        bits,
    })
}
pub(crate) fn commit_word(
    plan: WordPlan,
    write: impl FnOnce(usize, u32),
    observe: impl FnOnce() -> Result<DebugCreatorObservation>,
) -> CreatorInputWordWrite {
    let changed = plan.before_bits != plan.bits;
    let postflight = if changed {
        write(plan.address, plan.bits);
        observe().and_then(|copy| {
            if copy != plan.after {
                Err(Error::InvalidProfile(
                    "creator preparation postflight changed",
                ))
            } else {
                Ok(copy)
            }
        })
    } else {
        Ok(plan.source.clone())
    };
    CreatorInputWordWrite {
        source: plan.source,
        field: plan.field,
        before_bits: plan.before_bits,
        after_bits: plan.bits,
        changed,
        postflight,
    }
}
pub(crate) unsafe fn store_word(address: usize, bits: u32) {
    // SAFETY: Caller owns stable initialized writable aligned four-byte storage, no aliases.
    unsafe {
        (address as *mut u32).write(bits);
    }
}

/// An exact two-byte kind selection, with no access to adjacent padding or CharaInit ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrdinaryCreationRoute {
    Enemy,
    Player,
}
impl OrdinaryCreationRoute {
    pub(crate) fn bytes(self) -> [u8; 2] {
        match self {
            Self::Enemy => [0, 0],
            Self::Player => [1, 0],
        }
    }
    pub(crate) fn from_bytes(bytes: [u8; 2]) -> Result<Self> {
        match bytes {
            [0, 0] => Ok(Self::Enemy),
            [_, 0] => Ok(Self::Player),
            _ => Err(Error::NativeOperationRejected(
                "unsupported distinct creator subtype",
            )),
        }
    }
    pub(crate) fn kind(self) -> crate::objects::ObjectKind {
        match self {
            Self::Enemy => crate::objects::ObjectKind::Enemy,
            Self::Player => crate::objects::ObjectKind::Player,
        }
    }
}
#[derive(Debug)]
pub struct CreatorRouteWrite {
    pub source: DebugCreatorObservation,
    pub route: OrdinaryCreationRoute,
    pub changed: bool,
    pub postflight: Result<DebugCreatorObservation>,
}
pub type CreatorEnemyRouteWrite = CreatorRouteWrite;
pub(crate) struct RoutePlan {
    source: DebugCreatorObservation,
    after: DebugCreatorObservation,
    route: OrdinaryCreationRoute,
    address: usize,
}
pub(crate) fn enemy_route_plan(
    current: Option<DebugCreatorObservation>,
    expected: DebugCreatorObservation,
) -> Result<RoutePlan> {
    route_plan(current, expected, OrdinaryCreationRoute::Enemy)
}
pub(crate) fn route_plan(
    current: Option<DebugCreatorObservation>,
    expected: DebugCreatorObservation,
    route: OrdinaryCreationRoute,
) -> Result<RoutePlan> {
    if expected.input.native_argument != 0 {
        return Err(Error::NativeOperationRejected(
            "borrowed creator argument cannot select new ordinary route",
        ));
    }
    // Reuse full creator identity/count/result/alignment validation; no word is committed.
    word_plan(
        current,
        expected.clone(),
        CreatorInputWord::EventEntityId,
        expected.input.event_entity_id,
    )?;
    let address = expected
        .object
        .address
        .checked_add(u64::from(INPUT_OFFSET) + u64::from(crate::debug_creator::KIND_BYTES_OFFSET))
        .ok_or(Error::AddressOverflow)?;
    address.checked_add(2).ok_or(Error::AddressOverflow)?;
    let mut after = expected.clone();
    after.input.kind_bytes = route.bytes();
    Ok(RoutePlan {
        source: expected,
        after,
        route,
        address: usize::try_from(address).map_err(|_| Error::AddressOverflow)?,
    })
}
pub(crate) fn commit_enemy_route(
    plan: RoutePlan,
    write: impl FnOnce(usize),
    observe: impl FnOnce() -> Result<DebugCreatorObservation>,
) -> CreatorEnemyRouteWrite {
    commit_route(plan, |address, _| write(address), observe)
}
pub(crate) fn commit_route(
    plan: RoutePlan,
    write: impl FnOnce(usize, [u8; 2]),
    observe: impl FnOnce() -> Result<DebugCreatorObservation>,
) -> CreatorRouteWrite {
    let changed = plan.source.input.kind_bytes != plan.route.bytes();
    let postflight = if changed {
        write(plan.address, plan.route.bytes());
        observe().and_then(|copy| {
            if copy != plan.after {
                Err(Error::InvalidProfile(
                    "ordinary route preparation postflight changed",
                ))
            } else {
                Ok(copy)
            }
        })
    } else {
        Ok(plan.source.clone())
    };
    CreatorRouteWrite {
        source: plan.source,
        route: plan.route,
        changed,
        postflight,
    }
}
pub(crate) unsafe fn store_enemy_route(address: usize) {
    // SAFETY: Caller supplies exclusive writable aligned initialized two-byte kind storage.
    unsafe {
        store_route(address, [0, 0]);
    }
}
pub(crate) unsafe fn store_route(address: usize, bytes: [u8; 2]) {
    // SAFETY: Caller supplies exclusive writable aligned initialized two-byte kind storage.
    unsafe {
        (address as *mut u16).write(u16::from_le_bytes(bytes));
    }
}
