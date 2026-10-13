//! Thread-confined backread requests. Native resource management runs on the engine's own update.
use crate::{
    WW_171, Ww171File,
    character_resources::{
        CharacterBackreadRequest, CharacterBackreadWrite, CharacterResourcesObservation,
        commit_request, request_plan,
    },
    character_sets::{CharacterIdentity, WorldCollectionSelection, WorldMemberStateObservation},
};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

pub struct CurrentCharacterResources {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentCharacterResources {
    /// # Safety
    /// Caller retains the entire character, registration and resource graph in a legal phase,
    /// advances epochs on reuse and excludes mutation/teardown/ABA while observing. Requests
    /// require exclusive writable entry storage and a pinned engine/module. No SDK lease is acquired.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero character resource epoch"));
        }
        let file = OwnedVerifiedFile::verify(bytes, WW_171)?;
        Ww171File::from_verified(file.borrow()?)?.bind_objects_with_patches(
            &CurrentProcess,
            base,
            patches.as_deref(),
        )?;
        Ok(Self {
            file,
            patches,
            base,
            epoch,
            _thread: PhantomData,
        })
    }
    pub fn observe(
        &self,
        target: CharacterIdentity,
        selection: WorldCollectionSelection,
        limit: usize,
    ) -> Result<Option<CharacterResourcesObservation>> {
        if target.epoch != self.epoch {
            return Ok(None);
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .character_resources(target, selection, limit)
    }
    /// # Safety
    /// Bind's full lifetime/exclusive phase obligations hold. Caller owns this request and all
    /// future engine consumption; coordinates updates/debug flags and presentation separately.
    /// Unload does not detach, drain, destroy or free. Present does not revive or prove activation.
    /// No native function is invoked here, and a failed postflight retains the actual write receipt.
    pub unsafe fn request(
        &self,
        expected: WorldMemberStateObservation,
        request: CharacterBackreadRequest,
        limit: usize,
    ) -> Result<CharacterBackreadWrite> {
        let target = expected.registration.character;
        if target.epoch != self.epoch {
            return Err(Error::NativeOperationRejected(
                "foreign resource request epoch",
            ));
        }
        let image = Ww171File::from_verified(self.file.borrow()?)?;
        let objects =
            image.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?;
        let primary = objects.primary_player_identity(self.epoch)?;
        let resources = objects
            .character_resources(target, expected.selection, limit)?
            .ok_or(Error::NativeOperationRejected(
                "character resource owner absent",
            ))?;
        let plan = request_plan(Some(resources.member), expected, request, primary)?;
        if objects.primary_player_identity(self.epoch)? != primary {
            return Err(Error::NativeOperationRejected(
                "primary changed before resource request",
            ));
        }
        Ok(commit_request(
            plan,
            |address, value| {
                // SAFETY: Caller owns exactly this initialized live entry byte in an exclusive engine phase.
                unsafe { (address as *mut u8).write(value) };
            },
            || {
                objects
                    .world_member_state(target, expected.selection, limit)?
                    .ok_or(Error::InvalidProfile("member lost after resource request"))
            },
        ))
    }
}
