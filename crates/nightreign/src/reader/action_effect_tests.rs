use super::*;
use crate::{
    character_actions::ActionChannel, character_modules::ModuleKind, parameter_schema::SchemaPolicy,
};

fn action_fixture() -> Fixture {
    let mut fixture = Fixture::default();
    fixture.object(0x10000, PLAYER_TABLE);
    fixture.word(0x101b8, 0x20000);
    fixture.word(0x20080, 0x30000);
    fixture.object(0x30000, ModuleKind::ActionRequests.profile().1);
    fixture.word(0x30008, 0x10000);
    fixture.put(0x30010, &[0; 0x149]);
    fixture.word(0x30050, 0x30000);
    fixture.word(0x30030, 1u64 << 63);
    fixture.word(0x300a8, 1u64 << 9);
    fixture.put(0x30158, &[1]);
    fixture.dword(0x30098, 42);
    for (offset, storage, mask) in [(0x60, 0x40000, 1u64 << 40), (0x80, 0x50000, 1u64 << 7)] {
        fixture.word(0x30000 + offset, storage);
        fixture.word(0x30000 + offset + 8, storage + 32);
        fixture.word(0x30000 + offset + 16, storage + 32);
        fixture.put(storage, &[0; 32]);
        fixture.dword(storage, 12);
        fixture.word(storage + 8, 0xffff);
        fixture.dword(storage + 16, 42);
        fixture.word(storage + 24, mask);
    }
    fixture
}
#[test]
fn action_context_selection_and_suppression_preserve_native_predicates() {
    let mut fixture = action_fixture();
    let view = fixture.core().action_requests(0x10000, 2).unwrap().unwrap();
    assert!(view.has_any_requests(ActionChannel::Default));
    assert!(!view.has_any_requests(ActionChannel::Context)); // Native predicate uses low 37 bits.
    assert!(view.filtered_requested(ActionChannel::Context, 7).unwrap());
    assert!(!view.filtered_requested(ActionChannel::Default, 7).unwrap());
    assert!(view.filtered_requested(ActionChannel::Default, 9).unwrap());
    assert!(view.primary_requested(64).is_err());
    fixture.dword(0x30120, 0x800);
    let blocked = fixture.core().action_requests(0x10000, 2).unwrap().unwrap();
    assert!(
        !blocked
            .filtered_requested(ActionChannel::Context, 7)
            .unwrap()
    );
    fixture.word(0x30020, 1u64 << 63);
    let primary = fixture.core().action_requests(0x10000, 2).unwrap().unwrap();
    assert!(primary.has_any_requests(ActionChannel::Context));
    assert!(primary.primary_requested(63).unwrap());
}
#[test]
fn absent_context_uses_zero_and_disabled_context_uses_default() {
    let mut fixture = action_fixture();
    fixture.dword(0x30098, 99);
    let missing = fixture.core().action_requests(0x10000, 2).unwrap().unwrap();
    assert_eq!(missing.context_filtered_mask, 0);
    fixture.put(0x30158, &[0]);
    fixture.word(0x30060, u64::MAX); // Disabled storage must not be traversed.
    let disabled = fixture.core().action_requests(0x10000, 2).unwrap().unwrap();
    assert!(
        disabled
            .filtered_requested(ActionChannel::Context, 9)
            .unwrap()
    );
}
#[test]
fn action_context_rejects_bounds_owner_and_changing_records() {
    let mut fixture = action_fixture();
    assert!(fixture.core().action_requests(0x10000, 1).is_err());
    fixture.word(0x30050, 0x9999);
    assert!(fixture.core().action_requests(0x10000, 2).is_err());
    fixture.word(0x30050, 0x30000);
    fixture.word(0x30068, 0x3ffff);
    assert!(fixture.core().action_requests(0x10000, 2).is_err());
    fixture.word(0x30068, 0x40020);
    fixture.reads.borrow_mut().clear();
    fixture.replacement = Some((0x40000, vec![0; 32]));
    assert!(fixture.core().action_requests(0x10000, 2).is_err());
}
fn effect_fixture() -> Fixture {
    let mut fixture = Fixture::default();
    fixture.object(0x10000, PLAYER_TABLE);
    fixture.word(0x10190, 0x20000);
    fixture.object(0x20000, crate::character_effects::EFFECT_TABLE);
    fixture.word(0x20008, 0x30000);
    fixture.word(0x20010, 0x10000);
    fixture.word(0x20028, u64::MAX);
    for (address, next, param, id, flags, state) in [
        (0x30000, 0x40000, 0x60000, 101, 0, 449u16),
        (0x40000, 0x50000, u64::MAX, 102, 0x80000000, 0),
        (0x50000, 0, 0, 103, 0, 0),
    ] {
        fixture.put(address, &[0; 0x6c]);
        fixture.word(address, param);
        fixture.dword(address + 8, id);
        fixture.word(address + 0x38, next);
        fixture.dword(address + 0x68, flags);
        if flags == 0 && param != 0 {
            fixture.put(param + 0x156, &state.to_le_bytes());
        }
    }
    fixture
}
#[test]
fn effects_skip_excluded_nodes_and_preserve_summary_buckets() {
    let mut fixture = effect_fixture();
    let effects = fixture.core().special_effects(0x10000, 3).unwrap().unwrap();
    assert!(effects.contains_id(101));
    assert!(!effects.contains_id(102));
    assert!(effects.contains_id(103));
    assert!(effects.contains_state(449)); // Remainder 14: top nibble at bit 60.
    assert!(effects.contains_state(0)); // Native null PARAM comparison.
    assert!(!effects.contains_state(434)); // Same bucket, different state.
    assert_eq!(effects.nodes[1].state_info, None); // Excluded invalid PARAM is untouched.
    fixture.word(0x20028, 0xf);
    let restricted = fixture.core().special_effects(0x10000, 3).unwrap().unwrap();
    assert!(!restricted.contains_state(449));
    assert!(restricted.contains_state(0));
}
#[test]
fn effects_reject_cycles_limits_wrong_types_and_mutation() {
    let mut fixture = effect_fixture();
    assert!(fixture.core().special_effects(0x10000, 2).is_err());
    fixture.word(0x50038, 0x30000);
    assert!(fixture.core().special_effects(0x10000, 4).is_err());
    fixture.word(0x50038, 0);
    fixture.object(0x20000, PLAYER_TABLE);
    assert!(fixture.core().special_effects(0x10000, 3).is_err());
    fixture.object(0x20000, crate::character_effects::EFFECT_TABLE);
    fixture.reads.borrow_mut().clear();
    fixture.replacement = Some((0x60156, 0u16.to_le_bytes().to_vec()));
    assert!(fixture.core().special_effects(0x10000, 3).is_err());
}
#[test]
fn strict_schema_policy_rejects_partial_index_and_changing_type() {
    let mut fixture = super::schema_fixture(4);
    let rows = fixture.core().parameter_rows(0x10000, 8).unwrap();
    let complete = fixture.core().parameter_schema(&rows).unwrap();
    complete
        .check_definition_with_policy(
            crate::parameter_fields::npc::HP.definition(),
            SchemaPolicy::RequireCompleteStride,
        )
        .unwrap();
    let mut partial = rows.clone();
    partial.rows.truncate(1);
    let incomplete = fixture.core().parameter_schema(&partial).unwrap();
    assert!(
        incomplete
            .check_definition_with_policy(
                crate::parameter_fields::npc::HP.definition(),
                SchemaPolicy::RequireCompleteStride
            )
            .is_err()
    );
    fixture.reads.borrow_mut().clear();
    fixture.replacement = Some((0x3000c, vec![0; 32]));
    assert!(fixture.core().parameter_schema(&rows).is_err());
}

#[test]
fn removal_preflight_checks_owner_and_previous_links_before_calling() {
    let mut fixture = effect_fixture();
    fixture.put(0x6013e, &1i16.to_le_bytes());
    fixture.put(0x6031c, &7i16.to_le_bytes());
    let snapshot = fixture.core().special_effects(0x10000, 3).unwrap().unwrap();
    assert!(fixture.core().check_effect_removal(&snapshot, 101).is_err()); // Invalid previous links.
    fixture.word(0x40040, 0x30000);
    fixture.word(0x50040, 0x40000);
    fixture.word(0x40000, 0); // Null PARAM is valid for native removal; invalid excluded pointer is not.
    fixture.put(0x6013e, &1i16.to_le_bytes());
    fixture.put(0x6031c, &7i16.to_le_bytes());
    let snapshot = fixture.core().special_effects(0x10000, 3).unwrap().unwrap();
    assert!(fixture.core().check_effect_removal(&snapshot, 101).unwrap());
    assert!(!fixture.core().check_effect_removal(&snapshot, 102).unwrap());
    let mut wrong = snapshot;
    wrong.owner_address = 0;
    assert!(fixture.core().check_effect_removal(&wrong, 101).is_err());
}
#[test]
fn removal_preflight_reads_param_dependencies_on_excluded_matches() {
    let mut fixture = effect_fixture();
    fixture.word(0x40040, 0x30000);
    fixture.word(0x50040, 0x40000);
    fixture.word(0x40000, 0x70000);
    fixture.put(0x6013e, &0i16.to_le_bytes());
    fixture.put(0x6031c, &0i16.to_le_bytes());
    let snapshot = fixture.core().special_effects(0x10000, 3).unwrap().unwrap();
    assert!(fixture.core().check_effect_removal(&snapshot, 102).is_err());
    fixture.put(0x7013e, &0i16.to_le_bytes());
    fixture.put(0x70156, &3u16.to_le_bytes());
    fixture.put(0x7031c, &9i16.to_le_bytes());
    assert!(fixture.core().check_effect_removal(&snapshot, 102).unwrap());
}

fn goal_fixture() -> Fixture {
    let mut fixture = Fixture::default();
    fixture.word(0x10000, 0x90000);
    fixture.object(0x10018, crate::goal_actions::GOAL_FUNC_TABLE);
    fixture.word(0x10020, 0x10000);
    fixture.object(0x10300, crate::goal_actions::CONTROL_TABLE);
    fixture.word(0x10308, 0);
    fixture.put(0x10310, &[0; 13]);
    fixture.dword(0x10314, u32::MAX);
    fixture.dword(0x10318, 0xfedcba98);
    fixture.put(0x1031c, &[255]);
    fixture
}
#[test]
fn goal_action_state_preserves_raw_getter_bits_and_null_link() {
    let fixture = goal_fixture();
    let state = fixture.core().goal_action_state(0x10000).unwrap();
    assert_eq!(state.controller_address, 0x10300);
    assert_eq!(state.raw_parent_address, 0x90000);
    assert_eq!(state.raw_controller_link, 0);
    assert_eq!(state.raw_word_14, -1);
    assert_eq!(state.raw_id_bits, 0xfedcba98);
    assert_eq!(state.raw_flag, 255);
}
#[test]
fn goal_state_rejects_wrong_interface_controller_and_foreign_self_link() {
    let mut fixture = goal_fixture();
    fixture.object(0x10018, PLAYER_TABLE);
    assert!(fixture.core().goal_action_state(0x10000).is_err());
    fixture.object(0x10018, crate::goal_actions::GOAL_FUNC_TABLE);
    fixture.word(0x10020, 0x90000);
    assert!(fixture.core().goal_action_state(0x10000).is_err());
    fixture.word(0x10020, 0x10000);
    fixture.object(0x10300, PLAYER_TABLE);
    assert!(fixture.core().goal_action_state(0x10000).is_err());
}
#[test]
fn goal_state_rejects_control_changes_and_address_overflow() {
    let mut fixture = goal_fixture();
    fixture.replacement = Some((0x10310, vec![0; 13]));
    assert!(fixture.core().goal_action_state(0x10000).is_err());
    assert!(fixture.core().goal_action_state(u64::MAX - 7).is_err());
    assert!(fixture.core().goal_action_state(0x10001).is_err());
    let mut null = Fixture::default();
    null.word(0, 0);
    null.object(0x18, crate::goal_actions::GOAL_FUNC_TABLE);
    null.word(0x20, 0);
    null.object(0x300, crate::goal_actions::CONTROL_TABLE);
    null.word(0x308, 0);
    null.put(0x310, &[0; 13]);
    assert!(null.core().goal_action_state(0).is_err()); // Valid-looking embedded headers cannot validate a null owner.
}

#[test]
fn goal_request_plan_uses_inline_parameter_zero_and_exact_implementation() {
    let mut fixture = goal_fixture();
    fixture.word(0x100a8, 0x40000);
    fixture.object(0x40000, crate::goal_requests::EXEC_GOAL_TABLE);
    fixture.dword(0x10060, 42.75f32.to_bits());
    let plan = fixture.core().goal_action_request_plan(0x10000).unwrap();
    assert_eq!(plan.interface_address, 0x10018);
    assert_eq!(plan.implementation_address, 0x40000);
    assert_eq!(plan.requested_id_bits, 42);
    assert_eq!(plan.parameter_bits, 42.75f32.to_bits());
    fixture.object(0x40000, PLAYER_TABLE);
    assert!(fixture.core().goal_action_request_plan(0x10000).is_err());
}
#[test]
fn goal_request_plan_rejects_changed_parameter_and_invalid_conversion() {
    let mut fixture = goal_fixture();
    fixture.word(0x100a8, 0x40000);
    fixture.object(0x40000, crate::goal_requests::EXEC_GOAL_TABLE);
    fixture.dword(0x10060, 42.75f32.to_bits());
    fixture.replacement = Some((0x10060, 43f32.to_bits().to_le_bytes().to_vec()));
    assert!(fixture.core().goal_action_request_plan(0x10000).is_err());
    fixture.replacement = None;
    fixture.dword(0x10060, f32::NAN.to_bits());
    assert!(fixture.core().goal_action_request_plan(0x10000).is_err());
}

fn think_queue_fixture() -> Fixture {
    let mut fixture = goal_fixture();
    fixture.word(0x90008, 0xa0000);
    fixture.object(0xa0000, crate::think_actions::AI_FUNC_TABLE);
    fixture.word(0xa0008, 0x90000);
    for (offset, value) in [(0, 0x90000), (8, 0xd0000), (0x10, 0xb0000), (0x18, 2)] {
        fixture.word(0x9db70 + offset, value);
    }
    fixture.word(0xb0000, 0xc0000);
    fixture.word(0xb0008, 0xc1000);
    for (address, next, previous, key, flag, tag) in [
        (0xc0000, 0xc1000, 0xb0000, 100, 0xaau8, 1),
        (0xc1000, 0xb0000, 0xc0000, 200, 0x55u8, 2),
    ] {
        fixture.put(address, &[0; 40]);
        fixture.word(address, next);
        fixture.word(address + 8, previous);
        fixture.word(address + 0x10, 0x90000);
        fixture.dword(address + 0x18, key);
        fixture.dword(address + 0x1c, 42);
        fixture.put(address + 0x20, &[flag]);
        fixture.dword(address + 0x24, tag);
    }
    fixture
}
#[test]
fn think_event_queue_copies_order_numeric_keys_and_full_flags() {
    let fixture = think_queue_fixture();
    let queue = fixture
        .core()
        .think_action_queue(0x10000, 2)
        .unwrap()
        .unwrap();
    assert_eq!(queue.think_address, 0x90000);
    assert_eq!(queue.interface_address, 0xa0000);
    assert_eq!(queue.events[0].raw_key_a, 100);
    assert_eq!(queue.events[1].raw_key_a, 200);
    assert_eq!(queue.events[0].raw_flag, 0xaa);
    assert_eq!(queue.events[1].raw_flag, 0x55);
    assert_eq!(queue.events[0].raw_tag, 1);
    assert_eq!(queue.events[1].raw_tag, 2);
}
#[test]
fn think_queue_absent_parent_and_empty_sentinel_remain_distinct() {
    let mut fixture = think_queue_fixture();
    fixture.word(0x9db88, 0);
    fixture.word(0xb0000, 0xb0000);
    fixture.word(0xb0008, 0xb0000);
    assert!(
        fixture
            .core()
            .think_action_queue(0x10000, 2)
            .unwrap()
            .unwrap()
            .events
            .is_empty()
    );
    fixture.word(0x10000, 0);
    assert_eq!(fixture.core().think_action_queue(0x10000, 2).unwrap(), None);
    assert!(fixture.core().think_action_queue(0x10000, 0).is_err());
}
#[test]
fn think_queue_rejects_count_cycles_owner_and_backlink_errors() {
    let mut fixture = think_queue_fixture();
    assert!(fixture.core().think_action_queue(0x10000, 1).is_err());
    fixture.word(0xc1000, 0xc0000);
    assert!(fixture.core().think_action_queue(0x10000, 3).is_err());
    fixture.word(0xc1000, 0xb0000);
    fixture.word(0xc1008, 0xb0000);
    assert!(fixture.core().think_action_queue(0x10000, 3).is_err());
    fixture.word(0xc1008, 0xc0000);
    fixture.word(0xc1010, 0x90008);
    assert!(fixture.core().think_action_queue(0x10000, 3).is_err());
}
#[test]
fn think_queue_rejects_foreign_interface_and_record_changes() {
    let mut fixture = think_queue_fixture();
    fixture.object(0xa0000, PLAYER_TABLE);
    assert!(fixture.core().think_action_queue(0x10000, 2).is_err());
    fixture.object(0xa0000, crate::think_actions::AI_FUNC_TABLE);
    fixture.word(0xa0008, 0x90008);
    assert!(fixture.core().think_action_queue(0x10000, 2).is_err());
    fixture.word(0xa0008, 0x90000);
    fixture.reads.borrow_mut().clear();
    fixture.replacement = Some((0xc0000, vec![0; 40]));
    assert!(fixture.core().think_action_queue(0x10000, 2).is_err());
}

#[test]
fn think_transitions_preserve_unknown_tags_and_do_not_guess_from_flags() {
    use crate::think_actions::ThinkPredicateTransition;
    let mut fixture = think_queue_fixture();
    fixture.dword(0xc0024, 0);
    fixture.dword(0xc1024, u32::MAX);
    let queue = fixture
        .core()
        .think_action_queue(0x10000, 2)
        .unwrap()
        .unwrap();
    assert_eq!(
        queue.events[0].predicate_transition(),
        ThinkPredicateTransition::NoChange
    );
    assert_eq!(
        queue.events[1].predicate_transition(),
        ThinkPredicateTransition::Unknown(u32::MAX)
    );
    assert_eq!(queue.events[0].raw_flag, 0xaa);
    assert_eq!(queue.predicate_event_mask(), 0);
}

#[test]
fn think_transition_queries_use_key_b_and_native_list_order() {
    use crate::think_actions::{
        ThinkPredicateEdge as Edge, ThinkPredicateTransition as Transition,
    };
    let mut fixture = think_queue_fixture();
    fixture.dword(0xc001c, 0);
    fixture.dword(0xc1024, 1);
    fixture.dword(0xc101c, u32::MAX);
    let queue = fixture
        .core()
        .think_action_queue(0x10000, 2)
        .unwrap()
        .unwrap();
    assert_eq!(
        queue.events[0].predicate_transition(),
        Transition::BecameTrue
    );
    assert_eq!(queue.first_transition_key_b(Edge::BecameTrue), Some(0));
    assert_eq!(queue.first_transition_key_b(Edge::BecameFalse), None);
    assert!(queue.has_transition_for_key_b(Edge::BecameTrue, u32::MAX));
    assert!(queue.has_transition_for_key_b(Edge::BecameTrue, 0));
    assert!(!queue.has_transition_for_key_b(Edge::BecameTrue, 100)); // key A is not the queried key.
    assert!(!queue.has_transition_for_key_b(Edge::BecameFalse, u32::MAX));
}

#[test]
fn think_transition_mask_is_only_the_two_queue_produced_bits() {
    use crate::think_actions::{
        ThinkPredicateEdge as Edge, ThinkPredicateTransition as Transition,
    };
    let fixture = think_queue_fixture();
    let mut queue = fixture
        .core()
        .think_action_queue(0x10000, 2)
        .unwrap()
        .unwrap();
    assert_eq!(
        queue.events[1].predicate_transition(),
        Transition::BecameFalse
    );
    assert!(queue.has_transition_for_key_b(Edge::BecameFalse, 42));
    assert_eq!(queue.predicate_event_mask(), (1u64 << 43) | (1u64 << 44));
    queue.events[0].raw_tag = 99;
    assert_eq!(queue.predicate_event_mask(), 1u64 << 44);
    queue.events.clear();
    assert_eq!(queue.predicate_event_mask(), 0);
    assert_eq!(queue.first_transition_key_b(Edge::BecameFalse), None);
}

fn self_target_fixture() -> Fixture {
    let mut fixture = effect_fixture();
    fixture.bytes.extend(goal_fixture().bytes);
    fixture.word(0x90008, 0xa0000);
    fixture.object(0xa0000, crate::think_actions::AI_FUNC_TABLE);
    fixture.word(0xa0008, 0x90000);
    fixture.object(0x9c958, crate::think_targets::SELF_TARGET_TABLE);
    fixture.word(0x9c960, 0x80000);
    fixture.word(0x80000, 0x70000);
    fixture.object(0x70000, PLAYER_TABLE);
    fixture.word(0x70018, 0x80000);
    fixture.word(0x70190, 0x20000);
    fixture.word(0x20010, 0x70000);
    fixture
}
#[test]
fn think_self_target_distinguishes_missing_parent_and_unbound_handles() {
    let mut fixture = self_target_fixture();
    let target = fixture.core().think_self_target(0x10000).unwrap().unwrap();
    assert_eq!(target.target_address, 0x9c958);
    assert_eq!(target.handle_address, 0x80000);
    assert_eq!(target.character_address, Some(0x70000));
    fixture.word(0x80000, 0);
    let unbound = fixture.core().think_self_target(0x10000).unwrap().unwrap();
    assert_eq!(unbound.character_address, None);
    assert!(
        !fixture
            .core()
            .think_self_effect_predicate(0x10000, 101, 3)
            .unwrap()
            .unwrap()
            .present
    );
    fixture.word(0x9c960, 0);
    assert_eq!(
        fixture
            .core()
            .think_self_target(0x10000)
            .unwrap()
            .unwrap()
            .handle_address,
        0
    );
    fixture.word(0x10000, 0);
    assert_eq!(fixture.core().think_self_target(0x10000).unwrap(), None);
    assert_eq!(
        fixture
            .core()
            .think_self_effect_predicate(0x10000, 101, 3)
            .unwrap(),
        None
    );
}
#[test]
fn think_self_effect_predicate_includes_excluded_ids_and_preserves_all_id_bits() {
    let mut fixture = self_target_fixture();
    let effects = fixture.core().special_effects(0x70000, 3).unwrap().unwrap();
    assert!(!effects.contains_id(102));
    assert!(effects.contains_unfiltered_id(102));
    assert!(
        fixture
            .core()
            .think_self_effect_predicate(0x10000, 102, 3)
            .unwrap()
            .unwrap()
            .present
    );
    assert!(
        !fixture
            .core()
            .think_self_effect_predicate(0x10000, 999, 3)
            .unwrap()
            .unwrap()
            .present
    );
    fixture.dword(0x40008, u32::MAX);
    assert!(
        fixture
            .core()
            .think_self_effect_predicate(0x10000, -1, 3)
            .unwrap()
            .unwrap()
            .present
    );
    fixture.dword(0x40008, 0);
    assert!(
        fixture
            .core()
            .think_self_effect_predicate(0x10000, 0, 3)
            .unwrap()
            .unwrap()
            .present
    );
    assert!(
        fixture
            .core()
            .think_self_effect_predicate(0x10000, 101, 2)
            .is_err()
    );
    assert!(
        fixture
            .core()
            .think_self_effect_predicate(0x10000, 101, 0)
            .is_err()
    );
    fixture.word(0x70190, 0);
    assert!(
        !fixture
            .core()
            .think_self_effect_predicate(0x10000, 101, 3)
            .unwrap()
            .unwrap()
            .present
    );
}
#[test]
fn think_self_target_rejects_wrong_types_owners_and_reciprocal_handles() {
    let mut fixture = self_target_fixture();
    fixture.object(0x9c958, PLAYER_TABLE);
    assert!(fixture.core().think_self_target(0x10000).is_err());
    fixture.object(0x9c958, crate::think_targets::SELF_TARGET_TABLE);
    fixture.word(0x70018, 0x80008);
    assert!(fixture.core().think_self_target(0x10000).is_err());
    fixture.word(0x70018, 0x80000);
    fixture.word(0xa0008, 0x90008);
    assert!(fixture.core().think_self_target(0x10000).is_err());
    fixture.word(0xa0008, 0x90000);
    fixture.word(0x20010, 0x70008);
    assert!(
        fixture
            .core()
            .think_self_effect_predicate(0x10000, 101, 3)
            .is_err()
    );
}
#[test]
fn think_self_target_and_predicate_reject_changing_associations() {
    for (address, replacement) in [(0x80000, 0u64), (0x9c960, 0x80008), (0x70000, 0)] {
        let mut fixture = self_target_fixture();
        fixture.replacement = Some((address, replacement.to_le_bytes().to_vec()));
        assert!(fixture.core().think_self_target(0x10000).is_err());
    }
    let mut fixture = self_target_fixture();
    fixture.replacement = Some((0x70190, 0u64.to_le_bytes().to_vec()));
    assert!(
        fixture
            .core()
            .think_self_effect_predicate(0x10000, 101, 3)
            .is_err()
    );
}

fn character_think_fixture() -> Fixture {
    let mut fixture = self_target_fixture();
    fixture.word(0x705a8, 0xb0000);
    fixture.object(0xb0000, crate::character_think::COM_MANIPULATOR_TABLE);
    fixture.word(0xb00a8, 0x70000);
    fixture.word(0xb00c0, 0x90000);
    fixture.object(0xb00c8, crate::character_think::COM_OWNER_TABLE);
    fixture.word(0xb00d0, 0xb0000);
    fixture.word(0x90000, 0xb00c8);
    fixture
}
#[test]
fn character_think_uses_distinct_enemy_and_player_manipulator_slots() {
    let mut fixture = character_think_fixture();
    fixture.word(0x705a0, u64::MAX); // Player's +5a0 is not the ComManipulator slot.
    let player = fixture
        .core()
        .character_com_think(0x70000)
        .unwrap()
        .unwrap();
    assert_eq!(player.manipulator_address, 0xb0000);
    assert_eq!(player.owner_interface_address, 0xb00c8);
    assert_eq!(player.think_address, 0x90000);
    assert_eq!(player.handle_address, 0x80000);
    fixture.object(0x70000, ENEMY_TABLE);
    fixture.word(0x705a0, 0xb0000);
    fixture.word(0x705a8, u64::MAX); // Enemy's +5a8 is not the selected slot.
    assert_eq!(
        fixture.core().character_com_think(0x70000).unwrap(),
        Some(player)
    );
}
#[test]
fn character_think_absent_slots_are_distinct_from_unsupported_manipulators() {
    let mut fixture = character_think_fixture();
    fixture.word(0x705a8, 0);
    assert_eq!(fixture.core().character_com_think(0x70000).unwrap(), None);
    fixture.word(0x705a8, 0xb0000);
    fixture.word(0xb00c0, 0);
    assert_eq!(fixture.core().character_com_think(0x70000).unwrap(), None);
    fixture.object(0xb0000, PLAYER_TABLE);
    assert!(fixture.core().character_com_think(0x70000).is_err());
    fixture.object(0x70000, CHARACTER_TABLE);
    assert!(fixture.core().character_com_think(0x70000).is_err());
}
#[test]
fn character_think_rejects_foreign_owners_interfaces_and_handles() {
    for (address, value) in [
        (0xb00a8, 0x70008u64),
        (0xb00d0, 0xb0008),
        (0x90000, 0xb00d0),
        (0xa0008, 0x90008),
        (0x80000, 0x70008),
        (0x70018, 0x80008),
    ] {
        let mut fixture = character_think_fixture();
        fixture.word(address, value);
        assert!(fixture.core().character_com_think(0x70000).is_err());
    }
    let mut fixture = character_think_fixture();
    fixture.object(0xb00c8, PLAYER_TABLE);
    assert!(fixture.core().character_com_think(0x70000).is_err());
}
#[test]
fn character_think_rechecks_all_copied_association_links() {
    for address in [
        0x705a8, 0xb00a8, 0xb00c0, 0xb00d0, 0x90000, 0x90008, 0x80000,
    ] {
        let mut fixture = character_think_fixture();
        fixture.replacement = Some((address, 0u64.to_le_bytes().to_vec()));
        assert!(fixture.core().character_com_think(0x70000).is_err());
    }
}
