use super::*;
use crate::character_sets::CharacterIdentity;

fn registration() -> CharacterRegistrationObservation {
    CharacterRegistrationObservation {
        character: CharacterIdentity {
            address: 0x1000,
            world_address: 0x2000,
            native_identifier: 3,
            epoch: 4,
        },
        set_address: 0x3000,
        entry_address: 0x4000,
        ordinal: 5,
    }
}
fn policy(removal: SummonRemovalMode, reuse_hidden: bool) -> SummonPolicy {
    SummonPolicy {
        lifetime: SummonLifetime::default(),
        removal,
        reuse_hidden,
        destroy_delay_ms: 50,
    }
}
#[test]
fn ten_second_hidden_cycle_reuses_only_confirmed_unit_and_refreshes_original_request_deadline() {
    let mut unit = SummonLifecycle::new(
        registration(),
        policy(SummonRemovalMode::Hide, true),
        100,
        100,
    )
    .unwrap();
    unit.advance(10_099, None).unwrap();
    assert_eq!(unit.stage(), SummonStage::Active);
    unit.advance(10_100, None).unwrap();
    assert_eq!(
        unit.removal_reason(),
        Some(SummonRemovalReason::LifetimeExpired)
    );
    assert!(
        unit.claim_action(TaskPhase::FRAME_BEGIN, 10_100)
            .unwrap()
            .is_none()
    );
    let hide = unit
        .claim_action(TaskPhase::CHR_POST_PHYSICS, 10_100)
        .unwrap()
        .unwrap();
    assert_eq!(
        hide.kind(),
        SummonActionKind::Hide {
            retain_for_reuse: true
        }
    );
    assert_eq!(hide.source(), registration());
    assert!(unit.claim_reactivation(10_100).is_err());
    unit.complete_action(hide, 10_101, SummonActionOutcome::Confirmed)
        .unwrap();
    assert_eq!(unit.stage(), SummonStage::HiddenReusable);
    let reactivate = unit.claim_reactivation(20_000).unwrap();
    assert!(unit.claim_reactivation(20_000).is_err());
    unit.complete_action(reactivate, 20_001, SummonActionOutcome::Confirmed)
        .unwrap();
    assert_eq!(unit.stage(), SummonStage::Active);
    assert_eq!(unit.requested_at(), 20_000);
    assert_eq!(unit.expires_at(), Some(30_000));
    assert_eq!(unit.removal_reason(), None);
    unit.advance(30_000, None).unwrap();
    let hide = unit
        .claim_action(TaskPhase::CHR_POST_PHYSICS, 30_000)
        .unwrap()
        .unwrap();
    unit.complete_action(hide, 30_000, SummonActionOutcome::Confirmed)
        .unwrap();
    let late = unit.claim_reactivation(40_000).unwrap();
    unit.complete_action(late, 50_000, SummonActionOutcome::Confirmed)
        .unwrap();
    assert_eq!(unit.stage(), SummonStage::CleanupPending);
    assert_eq!(unit.expires_at(), Some(50_000));
}
#[test]
fn infinite_lifetime_requires_semantic_removal_and_hide_without_reuse_retains_record() {
    let mut settings = policy(SummonRemovalMode::Hide, false);
    settings.lifetime = SummonLifetime::from_config_millis(-1);
    let mut unit = SummonLifecycle::new(registration(), settings, 0, 0).unwrap();
    unit.advance(u64::MAX, None).unwrap();
    assert_eq!(unit.stage(), SummonStage::Active);
    assert_eq!(unit.expires_at(), None);
    unit.advance(u64::MAX, Some(SummonRemovalReason::VanishRequested))
        .unwrap();
    let hide = unit
        .claim_action(TaskPhase::CHR_POST_PHYSICS, u64::MAX)
        .unwrap()
        .unwrap();
    unit.complete_action(hide, u64::MAX, SummonActionOutcome::Confirmed)
        .unwrap();
    assert_eq!(unit.stage(), SummonStage::HiddenRetained);
    assert_eq!(unit.source(), registration());
    assert!(unit.claim_reactivation(u64::MAX).is_err());
    assert!(
        unit.claim_action(TaskPhase::FRAME_END, u64::MAX)
            .unwrap()
            .is_none()
    );
}
#[test]
fn destroy_never_reuses_and_delay_only_qualifies_explicit_frame_end_intent() {
    let mut unit = SummonLifecycle::new(
        registration(),
        policy(SummonRemovalMode::Destroy, true),
        0,
        0,
    )
    .unwrap();
    unit.advance(10_000, None).unwrap();
    let cleanup = unit
        .claim_action(TaskPhase::CHR_POST_PHYSICS, 10_000)
        .unwrap()
        .unwrap();
    assert_eq!(cleanup.kind(), SummonActionKind::StageRetirement);
    assert!(
        unit.claim_action(TaskPhase::FRAME_END, 10_100)
            .unwrap()
            .is_none()
    );
    unit.complete_action(cleanup, 10_100, SummonActionOutcome::Confirmed)
        .unwrap();
    assert_eq!(unit.retirement_at(), Some(10_150));
    assert!(
        unit.claim_action(TaskPhase::FRAME_END, 10_149)
            .unwrap()
            .is_none()
    );
    assert!(
        unit.claim_action(TaskPhase::CHR_POST_PHYSICS, 10_150)
            .unwrap()
            .is_none()
    );
    assert!(unit.claim_reactivation(10_150).is_err());
    let retire = unit
        .claim_action(TaskPhase::FRAME_END, 10_150)
        .unwrap()
        .unwrap();
    assert_eq!(retire.kind(), SummonActionKind::Retire);
    assert_eq!(unit.stage(), SummonStage::RetirementAttempted);
    assert!(
        unit.claim_action(TaskPhase::FRAME_END, 99_999)
            .unwrap()
            .is_none()
    );
    unit.complete_action(retire, 99_999, SummonActionOutcome::Confirmed)
        .unwrap();
    assert_eq!(unit.stage(), SummonStage::Retired);
    assert!(unit.claim_reactivation(99_999).is_err());
    assert_eq!(unit.source(), registration()); // Bookkeeping retains identity, never dereferences.
}
#[test]
fn partial_cleanup_or_retirement_remains_uncertain_even_after_more_frames() {
    for fail_retirement in [false, true] {
        let mut unit = SummonLifecycle::new(
            registration(),
            policy(SummonRemovalMode::Destroy, false),
            0,
            0,
        )
        .unwrap();
        unit.advance(1, Some(SummonRemovalReason::Dead)).unwrap();
        let cleanup = unit
            .claim_action(TaskPhase::CHR_POST_PHYSICS, 1)
            .unwrap()
            .unwrap();
        let action = if fail_retirement {
            unit.complete_action(cleanup, 2, SummonActionOutcome::Confirmed)
                .unwrap();
            unit.claim_action(TaskPhase::FRAME_END, 52)
                .unwrap()
                .unwrap()
        } else {
            cleanup
        };
        unit.complete_action(action, 52, SummonActionOutcome::Uncertain)
            .unwrap();
        unit.advance(100_000, None).unwrap();
        assert_eq!(unit.stage(), SummonStage::Uncertain);
        assert!(
            unit.claim_action(TaskPhase::CHR_POST_PHYSICS, 100_000)
                .unwrap()
                .is_none()
        );
        assert!(
            unit.claim_action(TaskPhase::FRAME_END, 100_000)
                .unwrap()
                .is_none()
        );
        assert!(unit.claim_reactivation(100_000).is_err());
        assert_eq!(unit.source(), registration());
    }
}
#[test]
fn dropped_tickets_foreign_controllers_and_quarantined_world_never_replay_or_acknowledge() {
    let settings = policy(SummonRemovalMode::Hide, true);
    let mut first = SummonLifecycle::new(registration(), settings, 0, 0).unwrap();
    let mut second = SummonLifecycle::new(registration(), settings, 0, 0).unwrap();
    first
        .advance(1, Some(SummonRemovalReason::ExplicitCleanup))
        .unwrap();
    second
        .advance(1, Some(SummonRemovalReason::ExplicitCleanup))
        .unwrap();
    let dropped = first
        .claim_action(TaskPhase::CHR_POST_PHYSICS, 1)
        .unwrap()
        .unwrap();
    let foreign = second
        .claim_action(TaskPhase::CHR_POST_PHYSICS, 1)
        .unwrap()
        .unwrap();
    assert!(
        first
            .complete_action(foreign, 1, SummonActionOutcome::Confirmed)
            .is_err()
    );
    assert_eq!(first.stage(), SummonStage::CleanupAttempted);
    drop(dropped);
    assert!(
        first
            .claim_action(TaskPhase::CHR_POST_PHYSICS, 1)
            .unwrap()
            .is_none()
    );
    assert_eq!(second.stage(), SummonStage::CleanupAttempted);
    let mut third = SummonLifecycle::new(registration(), settings, 0, 0).unwrap();
    third
        .advance(1, Some(SummonRemovalReason::ExplicitCleanup))
        .unwrap();
    let outstanding = third
        .claim_action(TaskPhase::CHR_POST_PHYSICS, 1)
        .unwrap()
        .unwrap();
    third.quarantine();
    assert!(
        third
            .complete_action(outstanding, 1, SummonActionOutcome::Confirmed)
            .is_err()
    );
    assert_eq!(third.stage(), SummonStage::Quarantined);
    assert!(third.claim_reactivation(1).is_err());
    assert_eq!(third.source(), registration());
}
#[test]
fn only_confirmed_not_attempted_can_issue_new_intent_and_failed_clock_keeps_attempt() {
    let mut unit = SummonLifecycle::new(
        registration(),
        policy(SummonRemovalMode::Destroy, false),
        0,
        0,
    )
    .unwrap();
    unit.advance(10, Some(SummonRemovalReason::ExplicitCleanup))
        .unwrap();
    let cleanup = unit
        .claim_action(TaskPhase::CHR_POST_PHYSICS, 10)
        .unwrap()
        .unwrap();
    unit.complete_action(cleanup, 10, SummonActionOutcome::NotAttempted)
        .unwrap();
    let retry = unit
        .claim_action(TaskPhase::CHR_POST_PHYSICS, 11)
        .unwrap()
        .unwrap();
    assert!(
        unit.complete_action(retry, 10, SummonActionOutcome::Confirmed)
            .is_err()
    );
    assert_eq!(unit.stage(), SummonStage::CleanupAttempted);
    assert!(
        unit.claim_action(TaskPhase::CHR_POST_PHYSICS, 12)
            .unwrap()
            .is_none()
    );
    assert!(unit.advance(9, None).is_err());
}
#[test]
fn zero_negative_lifetimes_late_registration_unbound_timeout_and_overflow_are_explicit() {
    for value in [i64::MIN, -100, -2, 0, 1] {
        let mut settings = policy(SummonRemovalMode::Destroy, false);
        settings.lifetime = SummonLifetime::from_config_millis(value);
        assert_eq!(settings.lifetime, SummonLifetime::Milliseconds(1));
        let unit = SummonLifecycle::new(registration(), settings, 100, 101).unwrap();
        assert_eq!(unit.stage(), SummonStage::CleanupPending);
        assert_eq!(unit.expires_at(), Some(101));
    }
    let wait = SummonCreationWait::new(100, 0).unwrap();
    assert!(!wait.expired(100).unwrap());
    assert!(wait.expired(101).unwrap());
    assert!(wait.expired(99).is_err());
    assert!(SummonCreationWait::new(u64::MAX, 0).is_err());
    let settings = policy(SummonRemovalMode::Destroy, false);
    assert!(SummonLifecycle::new(registration(), settings, u64::MAX, u64::MAX).is_err());
    assert!(SummonLifecycle::new(registration(), settings, 1, 0).is_err());
    let mut invalid = registration();
    invalid.character.epoch = 0;
    assert!(SummonLifecycle::new(invalid, settings, 0, 0).is_err());
    let mut unit = SummonLifecycle::new(registration(), settings, 0, 0).unwrap();
    unit.advance(u64::MAX, None).unwrap();
    let cleanup = unit
        .claim_action(TaskPhase::CHR_POST_PHYSICS, u64::MAX)
        .unwrap()
        .unwrap();
    assert!(
        unit.complete_action(cleanup, u64::MAX, SummonActionOutcome::Confirmed)
            .is_err()
    );
    assert_eq!(unit.stage(), SummonStage::CleanupAttempted);
    assert!(
        unit.claim_action(TaskPhase::FRAME_END, u64::MAX)
            .unwrap()
            .is_none()
    );
}
