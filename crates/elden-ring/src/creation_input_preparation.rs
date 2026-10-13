//! Bounded ordinary creator input preparation with actual partial receipts. No submission.
use crate::{
    current_debug_creator::{
        CurrentDebugCreator, NativeCreationNameAssignment, PreparedDebugCreation,
        schedule_preflight, validate_name_assignment,
    },
    debug_creation_fields::{
        CreatorInputWord, CreatorInputWordWrite, CreatorRouteWrite, OrdinaryCreationRoute,
        route_plan, word_plan,
    },
    debug_creator::DebugCreatorObservation,
};
use shikifsrs_runtime::{Error, Result};

/// Only fields whose factory consumers are independently proved. Auxiliary vectors,
/// mode/native argument and UI name buffer are preserved, not initialized or reinterpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrdinaryCreationInput {
    pub name_units: Vec<u16>,
    pub position_bits: [u32; 4],
    pub rotation_bits: [u32; 4],
    pub npc_param_id: i32,
    pub npc_think_param_id: i32,
    pub event_entity_id: u32,
    pub talk_id: i32,
    pub chara_init_param_id: i32,
    pub animation_id: i32,
    pub route: OrdinaryCreationRoute,
}
#[derive(Debug)]
pub enum CreationInputOperation {
    Word(CreatorInputWordWrite),
    Name(NativeCreationNameAssignment),
    Route(CreatorRouteWrite),
}
impl CreationInputOperation {
    fn source(&self) -> &DebugCreatorObservation {
        match self {
            Self::Word(v) => &v.source,
            Self::Name(v) => &v.source,
            Self::Route(v) => &v.source,
        }
    }
    fn postflight(&self) -> Result<&DebugCreatorObservation> {
        match self {
            Self::Word(v) => v.postflight.as_ref(),
            Self::Name(v) => v.postflight.as_ref(),
            Self::Route(v) => v.postflight.as_ref(),
        }
        .map_err(Clone::clone)
    }
}
/// Err completion preserves every actual operation, including a failed postflight. This
/// report is not a transaction or permission to retry, submit, free or restore on Drop.
pub struct CreationInputPreparation {
    pub source: DebugCreatorObservation,
    pub requested: OrdinaryCreationInput,
    pub operations: Vec<CreationInputOperation>,
    pub completion: Result<PreparedDebugCreation>,
}
#[derive(Debug, Clone)]
enum Step {
    Word(CreatorInputWord, u32),
    Name(Vec<u16>),
    Route(OrdinaryCreationRoute),
}
impl Step {
    fn planned(&self, source: &DebugCreatorObservation) -> DebugCreatorObservation {
        let mut planned = source.clone();
        match self {
            Self::Word(field, bits) => field.replace_copy(&mut planned, *bits),
            Self::Name(name) => planned.input.name_units = name.clone(),
            Self::Route(route) => planned.input.kind_bytes = route.bytes(),
        }
        planned
    }
    fn matches(&self, operation: &CreationInputOperation) -> bool {
        match (self, operation) {
            (Self::Word(field, bits), CreationInputOperation::Word(v)) => {
                v.field == *field && v.after_bits == *bits && v.changed
            }
            (Self::Name(name), CreationInputOperation::Name(v)) => {
                &v.assigned_name == name && v.returned_destination
            }
            (Self::Route(route), CreationInputOperation::Route(v)) => {
                v.route == *route && v.changed
            }
            _ => false,
        }
    }
}
fn plan(
    source: &DebugCreatorObservation,
    input: &OrdinaryCreationInput,
    limit: usize,
) -> Result<Vec<Step>> {
    if !(1..=65536).contains(&limit) {
        return Err(Error::InvalidProfile(
            "invalid creation preparation set limit",
        ));
    }
    schedule_preflight(Some(source), source, source.epoch)?;
    if source.schedule.current_step != 0
        || source.schedule.next_step != 0
        || source.schedule.advance_byte != 0
        || source.input.native_argument != 0
    {
        return Err(Error::NativeOperationRejected(
            "creation preparation requires caller-owned ordinary idle scheduler",
        ));
    }
    validate_name_assignment(source, &input.name_units)?;
    route_plan(Some(source.clone()), source.clone(), input.route)?;
    let mut words = Vec::with_capacity(14);
    for lane in 0..4 {
        words.push((
            CreatorInputWord::PositionLane(lane),
            input.position_bits[usize::from(lane)],
        ));
        words.push((
            CreatorInputWord::RotationLane(lane),
            input.rotation_bits[usize::from(lane)],
        ));
    }
    words.extend([
        (CreatorInputWord::NpcParamId, input.npc_param_id as u32),
        (
            CreatorInputWord::NpcThinkParamId,
            input.npc_think_param_id as u32,
        ),
        (CreatorInputWord::EventEntityId, input.event_entity_id),
        (CreatorInputWord::TalkId, input.talk_id as u32),
        (
            CreatorInputWord::CharaInitParamId,
            input.chara_init_param_id as u32,
        ),
        (CreatorInputWord::AnimationId, input.animation_id as u32),
    ]);
    let mut steps = Vec::with_capacity(16);
    // Validate all desired values/extents before any operation, even unchanged values.
    for (field, bits) in words {
        word_plan(Some(source.clone()), source.clone(), field, bits)?;
        if field.bits(source)? != bits {
            steps.push(Step::Word(field, bits));
        }
    }
    if source.input.name_units != input.name_units {
        steps.push(Step::Name(input.name_units.clone()));
    }
    if source.input.kind_bytes != input.route.bytes() {
        steps.push(Step::Route(input.route));
    }
    Ok(steps)
}
impl CurrentDebugCreator {
    /// Prepare the factory-consumed inputs and then capture a unique creation baseline.
    /// Bounds, all finite transforms, name capacity and idle source are checked before
    /// the first write. Unchanged fields do not invoke a write or name assignment.
    /// # Safety
    /// All CurrentDebugCreator preparation/name/route and prepare_ready_single contracts
    /// hold throughout. Caller exclusively owns the idle creator, initialized name storage,
    /// writable fields, full native copy graph and permitted phase; pins the world/set/NPC
    /// graph, excludes every engine/UI writer, callback, teardown, reuse and unload. Caller
    /// independently checks requested PARAM rows, route dependencies and resource readiness
    /// before any later creation. This does not synchronize UI buffer50, load resources,
    /// select a descriptor, mark advance, call the factory or grant ownership. Partial
    /// receipts must be reconciled explicitly; no rollback, repeated attempt or Drop write.
    pub unsafe fn prepare_single_input(
        &self,
        expected: DebugCreatorObservation,
        input: OrdinaryCreationInput,
        limit: usize,
    ) -> Result<CreationInputPreparation> {
        let steps = plan(&expected, &input, limit)?;
        if self.observe()? != Some(expected.clone()) {
            return Err(Error::NativeOperationRejected(
                "creation input baseline superseded",
            ));
        }
        let (operations, completion) = run_preparation(
            expected.clone(),
            steps,
            |before, step| {
                // SAFETY: Caller holds each native preparation contract over the entire sequence.
                unsafe {
                    match step {
                        Step::Word(field, bits) => self
                            .replace_prepared_word(before, field, bits)
                            .map(CreationInputOperation::Word),
                        Step::Name(name) => self
                            .assign_consumed_name(before, name)
                            .map(CreationInputOperation::Name),
                        Step::Route(OrdinaryCreationRoute::Enemy) => self
                            .select_ordinary_enemy(before)
                            .map(CreationInputOperation::Route),
                        Step::Route(OrdinaryCreationRoute::Player) => self
                            .select_ordinary_player(before)
                            .map(CreationInputOperation::Route),
                    }
                }
            },
            |after| self.prepare_ready_single(after, limit),
        );
        Ok(CreationInputPreparation {
            source: expected,
            requested: input,
            operations,
            completion,
        })
    }
}
fn run_preparation<T>(
    source: DebugCreatorObservation,
    steps: Vec<Step>,
    mut apply: impl FnMut(DebugCreatorObservation, Step) -> Result<CreationInputOperation>,
    finish: impl FnOnce(DebugCreatorObservation) -> Result<T>,
) -> (Vec<CreationInputOperation>, Result<T>) {
    let mut expected = source.clone();
    let mut operations = Vec::with_capacity(steps.len());
    let completion = (|| {
        for step in steps {
            let planned = step.planned(&expected);
            let operation = apply(expected.clone(), step.clone())?;
            let next = if operation.source() != &expected || !step.matches(&operation) {
                Err(Error::InvalidProfile(
                    "creation preparation receipt does not match step",
                ))
            } else {
                operation.postflight().and_then(|after| {
                    if after != &planned {
                        return Err(Error::InvalidProfile(
                            "creation preparation changed unrelated inputs",
                        ));
                    }
                    Ok(after.clone())
                })
            };
            // Keep actual effects even when their receipt/observer is rejected.
            operations.push(operation);
            expected = next?;
        }
        finish(expected)
    })();
    (operations, completion)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        debug_creator::{DebugCreationInput, DebugCreatorSchedule},
        objects::{ObjectKind, ObjectObservation},
    };
    use std::cell::Cell;
    fn fixture() -> (DebugCreatorObservation, OrdinaryCreationInput) {
        let source = DebugCreatorObservation {
            world: ObjectObservation {
                address: 0x2000,
                kind: ObjectKind::WorldCharacters,
            },
            object: ObjectObservation {
                address: 0x4000,
                kind: ObjectKind::DebugCharacterCreator,
            },
            epoch: 9,
            schedule: DebugCreatorSchedule {
                current_step: 0,
                next_step: 0,
                advance_byte: 0,
            },
            count: 1,
            animation_id: -1,
            phase_byte: 1,
            last_created_address: 0,
            input: DebugCreationInput {
                position_bits: [0; 4],
                rotation_bits: [0; 4],
                auxiliary20_bits: [0xabcdef01; 4],
                auxiliary30_bits: [0xfedcba01; 4],
                npc_param_id: 1000,
                npc_think_param_id: 2000,
                event_entity_id: u32::MAX,
                auxiliary_id4c: 99,
                name_units: "c0010".encode_utf16().collect(),
                name_buffer_address: 0x3000,
                name_capacity: 15,
                native_argument: 0,
                kind_bytes: [0, 0],
                chara_init_param_id: -1,
                mode: 7,
            },
        };
        let input = OrdinaryCreationInput {
            name_units: "c1234".encode_utf16().collect(),
            position_bits: [1.0f32.to_bits(); 4],
            rotation_bits: [(-2.0f32).to_bits(); 4],
            npc_param_id: i32::MIN,
            npc_think_param_id: i32::MAX,
            event_entity_id: 12345678,
            talk_id: i32::MIN + 1,
            chara_init_param_id: 100000,
            animation_id: 20012,
            route: OrdinaryCreationRoute::Player,
        };
        (source, input)
    }
    fn applied(source: DebugCreatorObservation, step: Step) -> CreationInputOperation {
        let after = step.planned(&source);
        match step {
            Step::Word(field, bits) => CreationInputOperation::Word(CreatorInputWordWrite {
                before_bits: field.bits(&source).unwrap(),
                source,
                field,
                after_bits: bits,
                changed: true,
                postflight: Ok(after),
            }),
            Step::Name(assigned_name) => {
                CreationInputOperation::Name(NativeCreationNameAssignment {
                    source,
                    assigned_name,
                    returned_destination: true,
                    postflight: Ok(after),
                })
            }
            Step::Route(route) => CreationInputOperation::Route(CreatorRouteWrite {
                source,
                route,
                changed: true,
                postflight: Ok(after),
            }),
        }
    }
    fn fail_observation(receipt: &mut CreationInputOperation) {
        let error = Error::InvalidProfile("lost partial observation");
        match receipt {
            CreationInputOperation::Word(v) => v.postflight = Err(error),
            CreationInputOperation::Name(v) => v.postflight = Err(error),
            CreationInputOperation::Route(v) => v.postflight = Err(error),
        }
    }
    #[test]
    fn ordinary_input_preparation_validates_all_inputs_before_operations_and_preserves_raw_fields()
    {
        let (source, input) = fixture();
        let steps = plan(&source, &input, 65536).unwrap();
        assert_eq!(steps.len(), 16);
        for invalid in 0..16 {
            let mut changed = input.clone();
            let mut before = source.clone();
            let mut limit = 16;
            match invalid {
                0 => limit = 0,
                1 => limit = 65537,
                2 => changed.position_bits[3] = f32::NAN.to_bits(),
                3 => changed.rotation_bits[3] = f32::INFINITY.to_bits(),
                4 => changed.name_units.clear(),
                5 => changed.name_units = vec![65; 32],
                6 => changed.name_units.push(0),
                7 => changed.name_units = vec![0xd800],
                8 => before.input.name_capacity = 4,
                9 => before.schedule.current_step = 1,
                10 => before.schedule.next_step = 2,
                11 => before.schedule.advance_byte = 1,
                12 => before.input.native_argument = 0x8000,
                13 => before.last_created_address = 0x5000,
                14 => before.count = 2,
                _ => before.epoch = 0,
            }
            assert!(plan(&before, &changed, limit).is_err(), "invalid {invalid}");
        }
        let (operations, completed) = run_preparation(
            source.clone(),
            steps,
            |before, step| Ok(applied(before, step)),
            Ok,
        );
        let after = completed.unwrap();
        assert_eq!(operations.len(), 16);
        assert_eq!(after.input.name_units, input.name_units);
        assert_eq!(after.input.position_bits, input.position_bits);
        assert_eq!(after.input.rotation_bits, input.rotation_bits);
        assert_eq!(after.input.npc_param_id, i32::MIN);
        assert_eq!(after.input.npc_think_param_id, i32::MAX);
        assert_eq!(after.input.kind_bytes, [1, 0]);
        assert_eq!(after.input.auxiliary20_bits, source.input.auxiliary20_bits);
        assert_eq!(after.input.auxiliary30_bits, source.input.auxiliary30_bits);
        assert_eq!(after.input.auxiliary_id4c, input.talk_id);
        assert_eq!(after.input.mode, source.input.mode);
        assert_eq!(
            after.input.name_buffer_address,
            source.input.name_buffer_address
        );
        assert_eq!(after.schedule, source.schedule);
        assert_eq!(after.phase_byte, source.phase_byte);
        assert!(plan(&after, &input, 16).unwrap().is_empty());
        let finished = Cell::new(0);
        let (operations, result) = run_preparation(
            after,
            vec![],
            |_, _| unreachable!(),
            |after| {
                finished.set(finished.get() + 1);
                Ok(after)
            },
        );
        assert!(operations.is_empty());
        assert!(result.is_ok());
        assert_eq!(finished.get(), 1);
    }
    #[test]
    fn ordinary_input_preparation_stops_at_every_pre_or_post_failure_and_retains_actual_prefix() {
        let (source, input) = fixture();
        for failed in 0..16 {
            for post_failure in [false, true] {
                let attempted = Cell::new(0);
                let actual = Cell::new(0);
                let (operations, result) = run_preparation(
                    source.clone(),
                    plan(&source, &input, 16).unwrap(),
                    |before, step| {
                        let index = attempted.get();
                        attempted.set(index + 1);
                        if index == failed && !post_failure {
                            return Err(Error::NativeOperationRejected("no operation occurred"));
                        }
                        actual.set(actual.get() + 1);
                        let mut receipt = applied(before, step);
                        if index == failed {
                            fail_observation(&mut receipt);
                        }
                        Ok(receipt)
                    },
                    |_| -> Result<()> {
                        unreachable!("failed inputs must never capture a baseline")
                    },
                );
                assert!(result.is_err());
                assert_eq!(attempted.get(), failed + 1);
                assert_eq!(actual.get(), failed + usize::from(post_failure));
                assert_eq!(operations.len(), actual.get());
                if post_failure {
                    assert!(operations.last().unwrap().postflight().is_err());
                }
                drop(operations);
                assert_eq!(actual.get(), failed + usize::from(post_failure));
            }
        }
        let (operations, result) = run_preparation(
            source.clone(),
            plan(&source, &input, 16).unwrap(),
            |before, step| Ok(applied(before, step)),
            |_| -> Result<()> { Err(Error::InvalidProfile("baseline capture failed")) },
        );
        assert_eq!(operations.len(), 16);
        assert!(
            operations
                .iter()
                .all(|operation| operation.postflight().is_ok())
        );
        assert!(result.is_err());
    }
    #[test]
    fn ordinary_input_preparation_rejects_inconsistent_receipt_even_if_postflight_says_success() {
        let (source, input) = fixture();
        for invalid in 0..5 {
            let calls = Cell::new(0);
            let (operations, result) = run_preparation(
                source.clone(),
                plan(&source, &input, 16).unwrap(),
                |before, step| {
                    calls.set(calls.get() + 1);
                    let mut receipt = applied(before, step);
                    let CreationInputOperation::Word(v) = &mut receipt else {
                        unreachable!()
                    };
                    match invalid {
                        0 => v.source.input.event_entity_id ^= 1,
                        1 => v.field = CreatorInputWord::AnimationId,
                        2 => v.after_bits ^= 1,
                        3 => v.changed = false,
                        _ => v.postflight.as_mut().unwrap().input.auxiliary_id4c += 1,
                    }
                    Ok(receipt)
                },
                |_| -> Result<()> { unreachable!() },
            );
            assert!(result.is_err());
            assert_eq!(calls.get(), 1);
            assert_eq!(operations.len(), 1);
        }
    }
}
