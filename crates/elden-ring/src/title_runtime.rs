//! Raw title-consumer inputs copied together; scene decisions remain caller policy.
use crate::{
    TaskCatalog, TaskPhase, Ww171File,
    character_sets::CharacterIdentity,
    menu_state::{FrontendStateObservation, LoadingPulseObservation, MenuTransitionObservation},
    objects::GameObjectCatalog,
    task_group_state::{PhaseGroupStateObservation, validate_record_limit},
};
use shikifsrs_runtime::{Error, Result, code_patches::OwnedCodePatches, memory::AddressSpace};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleRuntimeObservation {
    pub module_base: u64,
    pub epoch: u64,
    pub frontend: Option<FrontendStateObservation>,
    pub menu: Option<MenuTransitionObservation>,
    pub loading_pulses: Option<LoadingPulseObservation>,
    pub primary_player: Option<CharacterIdentity>,
    /// Title flow, title step, in-game menu flow, common menu flow, in that order.
    pub groups: [Option<PhaseGroupStateObservation>; 4],
}
pub struct TitleRuntimeCatalog<'b, 'a, M> {
    pub(crate) objects: GameObjectCatalog<'b, 'a, M>,
    pub(crate) tasks: TaskCatalog<'b, 'a, M>,
    pub(crate) phases: [TaskPhase; 4],
}
impl<'a> Ww171File<'a> {
    pub fn bind_title<'b, M: AddressSpace>(
        &'b self,
        memory: &'b M,
        base: u64,
    ) -> Result<TitleRuntimeCatalog<'b, 'a, M>> {
        self.bind_title_with_patches(memory, base, None)
    }
    pub fn bind_title_with_patches<'b, M: AddressSpace>(
        &'b self,
        memory: &'b M,
        base: u64,
        patches: Option<&'b OwnedCodePatches>,
    ) -> Result<TitleRuntimeCatalog<'b, 'a, M>> {
        // Resolve exact original consumer labels from the independently extracted native table.
        let phases = [
            self.phase("GameFlowInGame_TitleMenu")?,
            self.phase("TaskLineIdx_InGame_TitleMenuStep")?,
            self.phase("GameFlowInGame_InGameMenu")?,
            self.phase("GameFlowInGame_CommonMenu")?,
        ];
        Ok(TitleRuntimeCatalog {
            objects: self.bind_objects_with_patches(memory, base, patches)?,
            tasks: self.bind_with_patches(memory, base, patches)?,
            phases,
        })
    }
}
impl<M: AddressSpace> TitleRuntimeCatalog<'_, '_, M> {
    /// Read only the primary identity for consumer policies that must not depend on
    /// menu/frontend/loading diagnostics. Uses the same fresh full-ID/epoch object reader.
    /// This establishes no scene classification or native storage lifetime.
    pub fn primary_player_identity(&self, epoch: u64) -> Result<Option<CharacterIdentity>> {
        self.objects.primary_player_identity(epoch)
    }

    /// Missing individual sources remain None, not fabricated false. The second complete
    /// copy rejects later reads that observe an earlier component changing. This is not an
    /// atomic engine snapshot, scene classifier, movie query or permission to mutate state.
    pub fn snapshot(&self, epoch: u64, record_limit: usize) -> Result<TitleRuntimeObservation> {
        validate_record_limit(record_limit)?;
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero title observation epoch"));
        }
        let first = self.copy(epoch, record_limit)?;
        if first != self.copy(epoch, record_limit)? {
            return Err(Error::InvalidProfile(
                "title input snapshot changed during observation",
            ));
        }
        Ok(first)
    }
    fn copy(&self, epoch: u64, limit: usize) -> Result<TitleRuntimeObservation> {
        let mut groups = [None; 4];
        let frontend = self.objects.frontend_state(epoch)?;
        let menu = self.objects.menu_transition()?;
        let loading_pulses = self.objects.loading_helper_pulses()?;
        let primary_player = self.objects.primary_player_identity(epoch)?;
        for (group, phase) in groups.iter_mut().zip(self.phases) {
            *group = self.tasks.phase_group_state(phase, limit)?;
        }
        Ok(TitleRuntimeObservation {
            module_base: self.objects.module.base(),
            epoch,
            frontend,
            menu,
            loading_pulses,
            primary_player,
            groups,
        })
    }
}
