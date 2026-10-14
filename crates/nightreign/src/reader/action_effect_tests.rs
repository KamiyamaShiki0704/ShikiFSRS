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
