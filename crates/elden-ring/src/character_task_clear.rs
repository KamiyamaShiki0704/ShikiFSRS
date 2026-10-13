//! Observe earlier clear requests without dereferencing the old, possibly freed proxy.
use crate::{
    TaskCatalog, character_sets::CharacterIdentity, character_tasks::CharacterTaskRegistration,
    objects::GameObjectCatalog,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};

/// Copied submission metadata, not ownership of native storage or a cancellation receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterTaskClearTicket {
    pub(crate) target: CharacterIdentity,
    pub(crate) ordinal: u8,
    pub(crate) phase_index: u32,
    pub(crate) group_id: u32,
    pub(crate) previous_proxy: u64,
}

impl CharacterTaskClearTicket {
    pub fn target(&self) -> CharacterIdentity {
        self.target
    }
    pub fn ordinal(&self) -> u8 {
        self.ordinal
    }
    pub fn previous_proxy(&self) -> u64 {
        self.previous_proxy
    }
    pub fn phase_index(&self) -> u32 {
        self.phase_index
    }
    pub fn native_group_id(&self) -> u32 {
        self.group_id
    }
}

/// Registration absence is not a proxy free receipt, callback drain, or permission to free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterTaskClearObservation {
    pub ticket: CharacterTaskClearTicket,
    /// A new registration is reported, never silently counted as a cleared slot.
    pub slot_registration: Option<CharacterTaskRegistration>,
    /// True can mean queued or rejected cancellation; no distinction is inferred.
    pub previous_proxy_in_registry: bool,
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Observe the original full group and current slot under the same memory/module binding.
    /// Target disappearance returns None; changed group identity or detectable changes fail.
    /// `limit` bounds the existing native tree/record readers. Old proxy bytes are never read.
    pub fn character_task_clear_status(
        &self,
        ticket: CharacterTaskClearTicket,
        tasks: &TaskCatalog<'_, '_, M>,
        limit: usize,
    ) -> Result<Option<CharacterTaskClearObservation>> {
        if ticket.ordinal >= 6
            || ticket.group_id == 0
            || ticket.previous_proxy == 0
            || self.module.base() != tasks.module.base()
            || !std::ptr::eq(self.module.memory(), tasks.module.memory())
            || !(1..=65536).contains(&limit)
        {
            return Err(Error::InvalidProfile(
                "invalid task clear observation binding",
            ));
        }
        let Some(before) = self.character_tasks(ticket.target)? else {
            return Ok(None);
        };
        let phase = tasks
            .observe(ticket.phase_index as usize)?
            .ok_or(Error::InvalidProfile("cleared task phase disappeared"))?;
        if phase.native_group_id() != Some(ticket.group_id) {
            return Err(Error::InvalidProfile(
                "cleared task full group identity changed",
            ));
        }
        let records = tasks
            .task_registrations(ticket.group_id, limit, limit)?
            .ok_or(Error::InvalidProfile(
                "cleared task registered group disappeared",
            ))?;
        let previous_proxy_in_registry = records
            .iter()
            .any(|r| r.task_address == ticket.previous_proxy);
        if self.character_tasks(ticket.target)? != Some(before.clone())
            || tasks.observe(ticket.phase_index as usize)? != Some(phase)
            || tasks.task_registrations(ticket.group_id, limit, limit)? != Some(records)
        {
            return Err(Error::InvalidProfile(
                "cleared task state changed during observation",
            ));
        }
        Ok(Some(CharacterTaskClearObservation {
            ticket,
            slot_registration: before.tasks[ticket.ordinal as usize].registration,
            previous_proxy_in_registry,
        }))
    }
}
