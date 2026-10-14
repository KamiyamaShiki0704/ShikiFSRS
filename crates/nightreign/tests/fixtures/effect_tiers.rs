use super::*;
use shikifsrs_nightreign::{
    effect_addition::EffectAdditionLimits, effect_tiers::EffectTierOutcome,
};
fn clone_fixture<'a>(fixture: &MappedFixture<'a>) -> MappedFixture<'a> {
    MappedFixture {
        image: fixture.image,
        base: fixture.base,
        objects: fixture.objects.clone(),
        changed_byte: None,
    }
}
fn prepare_category(fixture: &mut MappedFixture<'_>, category: i16) {
    for address in [0x20113e, 0x1b013e] {
        fixture.put(address, &category.to_le_bytes());
        fixture.put(address + 2, &[1]);
    }
    // Native tier selection includes this excluded first node.
    fixture.dword(0x1a0068, 0x80000000);
}
struct ChangingTier<'a> {
    fixture: &'a MappedFixture<'a>,
    reads: RefCell<usize>,
}
impl AddressSpace for ChangingTier<'_> {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        self.fixture.read_exact(address, output)?;
        if address == 0x1b013e && output.len() == 3 {
            let mut count = self.reads.borrow_mut();
            *count += 1;
            if *count > 1 {
                output[0] = 205;
            }
        }
        Ok(())
    }
}
pub(super) fn check_effect_tier_plans(file: &NightreignFile<'_>, fixture: &MappedFixture<'_>) {
    let limits = EffectAdditionLimits {
        nodes: 8,
        parameter_rows: 8,
    };
    let mut next = clone_fixture(fixture);
    prepare_category(&mut next, 204);
    // Two complete ordinal records with the pinned 976-byte stride.
    next.put(0x20000a, &2u16.to_le_bytes());
    next.dword(0x1ffff4, 2);
    next.dword(0x200118, 124);
    next.dword(0x20011c, 1);
    next.word(0x200060, 0x13d0);
    next.put(0x2013d0, &[0; 976]);
    let reader = file.bind_reader(&next, next.base).unwrap();
    let plan = reader.effect_id_plan(0x50000, 123, limits).unwrap();
    assert_eq!(plan.effective_id, 124);
    assert_eq!(plan.outcome, EffectTierOutcome::NextTier);
    assert_eq!(plan.matched_node_address, Some(0x1a0000));
    assert_eq!(plan.requested_parameter_address, 0x201000);
    assert!(
        reader
            .effect_id_plan(
                0x50000,
                123,
                EffectAdditionLimits {
                    nodes: 8,
                    parameter_rows: 1
                }
            )
            .is_err()
    );
    assert!(reader.effect_id_plan(0x50000, 999, limits).is_err());
    for (category, effective, outcome) in [
        (204, -1, EffectTierOutcome::ExhaustedReject),
        (206, 123, EffectTierOutcome::ExhaustedKeep),
    ] {
        let mut last = clone_fixture(fixture);
        prepare_category(&mut last, category);
        let reader = file.bind_reader(&last, last.base).unwrap();
        let plan = reader.effect_id_plan(0x50000, 123, limits).unwrap();
        assert_eq!(plan.effective_id, effective);
        assert_eq!(plan.outcome, outcome);
    }
    let changing = ChangingTier {
        fixture: &next,
        reads: RefCell::new(0),
    };
    let reader = file.bind_reader(&changing, next.base).unwrap();
    assert!(reader.effect_id_plan(0x50000, 123, limits).is_err());
    let mut absent = clone_fixture(fixture);
    prepare_category(&mut absent, 204);
    absent.word(0x1a0000, 0);
    let reader = file.bind_reader(&absent, absent.base).unwrap();
    let plan = reader.effect_id_plan(0x50000, 123, limits).unwrap();
    assert_eq!(plan.outcome, EffectTierOutcome::Unchanged);
    assert_eq!(plan.matched_node_address, None);
}
