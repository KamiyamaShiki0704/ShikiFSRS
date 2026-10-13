//! Selected unit components copied and rechecked together, never a native lease.
use crate::{
    CharacterEffectsObservation,
    character_controller::CharacterControllerObservation,
    character_events::CharacterEventIdObservation,
    character_parameters::{
        CharacterNpcParameterObservation, CharacterNpcRequestObservation,
        CharacterSelectionObservation,
    },
    character_physics::CharacterPhysicsObservation,
    character_ragdoll::CharacterRagdollObservation,
    character_sets::{
        CharacterIdentity, WORLD_INDEXED_SET_COUNT, WorldCollectionSelection,
        WorldMemberStateObservation,
    },
    character_visuals::{CharacterVisualLink, CharacterVisualObservation},
    controller_storage::ControllerStorageObservation,
    objects::GameObjectCatalog,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};

/// Unrequested storage is never touched; requested missing storage is distinct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitComponent<T> {
    Unrequested,
    Missing,
    Present(T),
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CharacterUnitReadPlan {
    /// Selected native collection and per-set capacity bound. No automatic scan
    /// of all collections or interpretation of raw state4 as active/resource-ready.
    pub member: Option<(WorldCollectionSelection, usize)>,
    pub controller: bool,
    pub ragdoll: bool,
    pub controller_storage: bool,
    pub physics: bool,
    pub effects_limit: Option<usize>,
    /// At most five distinct links, retaining caller order. SkeletonMapper is
    /// the native mapping object, not an HkaSkeleton or bone-array lease.
    pub visuals: Vec<CharacterVisualLink>,
    pub npc_row_limit: Option<usize>,
    pub stored_npc_request_id: bool,
    pub stored_event_id: bool,
}
impl CharacterUnitReadPlan {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.visuals.len()>5 || self.visuals.iter().enumerate().any(|(i,link)|self.visuals[..i].contains(link))
            || self.member.is_some_and(|(selection,limit)| !(1..=65_536).contains(&limit) || matches!(selection,WorldCollectionSelection::Indexed(index) if u32::from(index)>=WORLD_INDEXED_SET_COUNT))
            || self.effects_limit.is_some_and(|limit|!(1..=65_536).contains(&limit))
            || self.npc_row_limit.is_some_and(|limit|!(1..=65_535).contains(&limit)) {
            return Err(Error::InvalidProfile("invalid character unit read plan"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterUnitSnapshot {
    pub plan: CharacterUnitReadPlan,
    pub selection: CharacterSelectionObservation,
    pub member: UnitComponent<WorldMemberStateObservation>,
    pub controller: UnitComponent<CharacterControllerObservation>,
    pub ragdoll: UnitComponent<CharacterRagdollObservation>,
    pub controller_storage: UnitComponent<ControllerStorageObservation>,
    pub physics: UnitComponent<CharacterPhysicsObservation>,
    pub effects: UnitComponent<CharacterEffectsObservation>,
    pub visuals: Vec<(CharacterVisualLink, Option<CharacterVisualObservation>)>,
    /// Last native NPC handle query; not the original spawn/stored NPC request ID.
    pub npc_parameter: UnitComponent<CharacterNpcParameterObservation>,
    pub stored_npc_request_id: UnitComponent<CharacterNpcRequestObservation>,
    /// Stored event ID, distinct from Player's possibly different virtual getter.
    pub stored_event_id: UnitComponent<CharacterEventIdObservation>,
}
fn selected<T>(
    enabled: bool,
    read: impl FnOnce() -> Result<Option<T>>,
) -> Result<UnitComponent<T>> {
    if !enabled {
        return Ok(UnitComponent::Unrequested);
    }
    Ok(match read()? {
        Some(value) => UnitComponent::Present(value),
        None => UnitComponent::Missing,
    })
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Reuses only independently proved per-component readers. All selected
    /// components are copied twice as a complete unit, so a change after an early
    /// reader's local postflight cannot silently authorize a mixed unit snapshot.
    /// Not atomic: same-value ABA, lifetime, aliases, exclusive phase and epoch
    /// remain caller obligations. This creates no game references or native calls.
    pub fn character_unit_snapshot(
        &self,
        target: CharacterIdentity,
        plan: &CharacterUnitReadPlan,
    ) -> Result<Option<CharacterUnitSnapshot>> {
        plan.validate()?;
        if target.epoch == 0 {
            return Err(Error::InvalidProfile("zero character unit epoch"));
        }
        let Some(first) = self.copy_unit(target, plan)? else {
            return Ok(None);
        };
        if self.copy_unit(target, plan)?.as_ref() != Some(&first) {
            return Err(Error::InvalidProfile(
                "selected character unit components changed",
            ));
        }
        Ok(Some(first))
    }
    /// Observe the primary identity and all selected components under the same caller epoch.
    /// A primary slot change after the unit copy rejects the result; no native lease is held.
    pub fn primary_unit_snapshot(
        &self,
        epoch: u64,
        plan: &CharacterUnitReadPlan,
    ) -> Result<Option<CharacterUnitSnapshot>> {
        plan.validate()?;
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero primary unit epoch"));
        }
        let target = self.primary_player_identity(epoch)?;
        let copy = target
            .map(|target| self.character_unit_snapshot(target, plan))
            .transpose()?
            .flatten();
        if self.primary_player_identity(epoch)? != target {
            return Err(Error::InvalidProfile(
                "primary changed during unit observation",
            ));
        }
        Ok(copy)
    }
    fn copy_unit(
        &self,
        target: CharacterIdentity,
        plan: &CharacterUnitReadPlan,
    ) -> Result<Option<CharacterUnitSnapshot>> {
        let Some(selection) = self.character_selection(target)? else {
            return Ok(None);
        };
        let member = selected(plan.member.is_some(), || {
            let (selection, limit) = plan.member.expect("selected plan");
            self.world_member_state(target, selection, limit)
        })?;
        let controller = selected(plan.controller, || self.character_controller(target))?;
        let ragdoll = selected(plan.ragdoll, || self.character_ragdoll(target))?;
        let controller_storage =
            selected(plan.controller_storage, || self.controller_storage(target))?;
        let physics = selected(plan.physics, || self.character_physics(target))?;
        let effects = selected(plan.effects_limit.is_some(), || {
            self.character_effects(target, plan.effects_limit.expect("selected plan"))
        })?;
        let mut visuals = Vec::with_capacity(plan.visuals.len());
        for &link in &plan.visuals {
            visuals.push((link, self.character_visual(target, link)?));
        }
        let npc_parameter = selected(plan.npc_row_limit.is_some(), || {
            self.character_npc_parameter(target, plan.npc_row_limit.expect("selected plan"))
        })?;
        let stored_event_id = selected(plan.stored_event_id, || self.character_event_id(target))?;
        let stored_npc_request_id = selected(plan.stored_npc_request_id, || {
            self.character_npc_request_id(target)
        })?;
        Ok(Some(CharacterUnitSnapshot {
            plan: plan.clone(),
            selection,
            member,
            controller,
            ragdoll,
            controller_storage,
            physics,
            effects,
            visuals,
            npc_parameter,
            stored_npc_request_id,
            stored_event_id,
        }))
    }
}
