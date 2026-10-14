//! Experimental copied prediction of NR's original 0x5077a0 effect ID remapping.
//! No native call or effect mutation. A plan is a snapshot, not an execution receipt.
use crate::{
    effect_addition::EffectAdditionLimits, parameter_fields::effect, reader::NightreignReader,
    roots::RootKind,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectTierOutcome {
    Unchanged,
    NextTier,
    ExhaustedReject,
    ExhaustedKeep,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectIdPlan {
    pub requested_id: i32,
    /// -1 is the native rejection sentinel for an exhausted category-204 tier.
    pub effective_id: i32,
    pub requested_parameter_address: u64,
    pub matched_node_address: Option<u64>,
    pub outcome: EffectTierOutcome,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TierNode {
    address: u64,
    id: i32,
    category: i16,
    priority: u8,
}
impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    /// Predicts original remapping using default variant-zero PARAM and the observed list.
    /// Query exclusion flags deliberately do not filter tier matches. Rejects i32 increment
    /// overflow rather than presenting a wrapped negative ID as supported. Caller supplies
    /// stable memory/lifetimes; repeated copies detect observed changes, not concurrent ABA.
    pub fn effect_id_plan(
        &self,
        character: u64,
        id: i32,
        limits: EffectAdditionLimits,
    ) -> Result<EffectIdPlan> {
        if id <= 0 {
            return Err(Error::InvalidProfile(
                "NR effect tier plan requires a positive ID",
            ));
        }
        let effects = self
            .special_effects(character, limits.nodes)?
            .ok_or(Error::NativeOperationRejected("NR effect object is absent"))?;
        effects.check_owner_links()?;
        let root = self
            .root(RootKind::Parameters)?
            .ok_or(Error::NativeOperationRejected(
                "NR PARAM repository is absent",
            ))?;
        let rows = self.parameter_rows(root, "SpEffectParam", 0, limits.parameter_rows)?;
        let schema = self.core().parameter_schema(&rows)?;
        schema.check_definition(effect::STATE_INFO.definition())?;
        let requested = self
            .copy_parameter_row(
                root,
                "SpEffectParam",
                0,
                id as u32,
                effect::STATE_INFO.definition().packed_size,
                limits.parameter_rows,
            )?
            .ok_or(Error::NativeOperationRejected(
                "NR requested effect PARAM is absent",
            ))?;
        let category = effect::SP_CATEGORY.read_row_bytes(&requested.bytes)?;
        let priority = effect::CATEGORY_PRIORITY.read_row_bytes(&requested.bytes)?;
        let mut nodes = Vec::new();
        let mut observations = Vec::new();
        if matches!(category, 204 | 206) && priority != 0 {
            for node in &effects.nodes {
                if node.parameter_address == 0 {
                    continue;
                }
                if !node.parameter_address.is_multiple_of(4) {
                    return Err(Error::InvalidProfile("unaligned NR tier PARAM"));
                }
                let address = node
                    .parameter_address
                    .checked_add(0x13e)
                    .ok_or(Error::AddressOverflow)?;
                let mut key = [0; 3];
                self.module.memory().read_exact(address, &mut key)?;
                let key_category = i16::from_le_bytes(key[..2].try_into().expect("key"));
                let key_priority = key[2];
                nodes.push(TierNode {
                    address: node.address,
                    id: node.id,
                    category: key_category,
                    priority: key_priority,
                });
                observations.push((address, key));
                // The original helper stops at the first match, including nonpositive IDs.
                if key_category == category && key_priority == priority {
                    break;
                }
            }
        }
        let mut plan = plan_from_nodes(id, category, priority, &nodes, |next| {
            rows.row(next as u32).is_some()
        })?;
        plan.requested_parameter_address = requested.row.address;
        for (address, key) in observations {
            let mut repeated = [0; 3];
            self.module.memory().read_exact(address, &mut repeated)?;
            if repeated != key {
                return Err(Error::InvalidProfile("NR effect tier key changed"));
            }
        }
        if self.root(RootKind::Parameters)? != Some(root)
            || self.parameter_rows(root, "SpEffectParam", 0, limits.parameter_rows)? != rows
            || self.core().parameter_schema(&rows)? != schema
            || self.copy_parameter_row(
                root,
                "SpEffectParam",
                0,
                id as u32,
                requested.bytes.len(),
                limits.parameter_rows,
            )? != Some(requested)
            || self.special_effects(character, limits.nodes)? != Some(effects)
        {
            return Err(Error::InvalidProfile(
                "NR effect tier inputs changed during plan",
            ));
        }
        Ok(plan)
    }
}
fn plan_from_nodes(
    id: i32,
    category: i16,
    priority: u8,
    nodes: &[TierNode],
    mut has_next: impl FnMut(i32) -> bool,
) -> Result<EffectIdPlan> {
    let mut plan = EffectIdPlan {
        requested_id: id,
        effective_id: id,
        requested_parameter_address: 0,
        matched_node_address: None,
        outcome: EffectTierOutcome::Unchanged,
    };
    if !matches!(category, 204 | 206) || priority == 0 {
        return Ok(plan);
    }
    let Some(node) = nodes
        .iter()
        .find(|node| node.category == category && node.priority == priority)
    else {
        return Ok(plan);
    };
    plan.matched_node_address = Some(node.address);
    if node.id <= 0 {
        return Ok(plan);
    }
    let next = node.id.checked_add(1).ok_or(Error::InvalidProfile(
        "NR effect tier increment overflows supported positive i32 domain",
    ))?;
    if has_next(next) {
        plan.effective_id = next;
        plan.outcome = EffectTierOutcome::NextTier;
    } else if category == 204 {
        plan.effective_id = -1;
        plan.outcome = EffectTierOutcome::ExhaustedReject;
    } else {
        plan.effective_id = node.id;
        plan.outcome = EffectTierOutcome::ExhaustedKeep;
    }
    Ok(plan)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_matching_node_controls_next_tier_not_last_or_highest() {
        let nodes = [
            TierNode {
                address: 1,
                id: 50,
                category: 204,
                priority: 2,
            },
            TierNode {
                address: 2,
                id: 99,
                category: 204,
                priority: 1,
            },
            TierNode {
                address: 3,
                id: 101,
                category: 204,
                priority: 1,
            },
        ];
        let mut lookup = Vec::new();
        let plan = plan_from_nodes(80, 204, 1, &nodes, |id| {
            lookup.push(id);
            id == 100
        })
        .unwrap();
        assert_eq!(plan.effective_id, 100);
        assert_eq!(plan.matched_node_address, Some(2));
        assert_eq!(lookup, [100]);
        assert_eq!(plan.outcome, EffectTierOutcome::NextTier);
    }
    #[test]
    fn exhausted_categories_have_different_native_outcomes() {
        for (category, effective, outcome) in [
            (204, -1, EffectTierOutcome::ExhaustedReject),
            (206, 99, EffectTierOutcome::ExhaustedKeep),
        ] {
            let nodes = [TierNode {
                address: 1,
                id: 99,
                category,
                priority: 1,
            }];
            let plan = plan_from_nodes(80, category, 1, &nodes, |_| false).unwrap();
            assert_eq!(plan.effective_id, effective);
            assert_eq!(plan.outcome, outcome);
        }
    }
    #[test]
    fn zero_priority_other_category_and_nonpositive_match_do_not_lookup_next() {
        for (category, priority, current) in [(204, 0, 99), (205, 1, 99), (204, 1, -1)] {
            let nodes = [TierNode {
                address: 1,
                id: current,
                category,
                priority,
            }];
            let plan = plan_from_nodes(80, category, priority, &nodes, |_| {
                panic!("unexpected next lookup")
            })
            .unwrap();
            assert_eq!(plan.effective_id, 80);
            assert_eq!(plan.outcome, EffectTierOutcome::Unchanged);
        }
    }
    #[test]
    fn increment_overflow_fails_before_lookup() {
        let nodes = [TierNode {
            address: 1,
            id: i32::MAX,
            category: 204,
            priority: 1,
        }];
        assert!(
            plan_from_nodes(80, 204, 1, &nodes, |_| panic!("unexpected wrapped lookup")).is_err()
        );
    }
}
