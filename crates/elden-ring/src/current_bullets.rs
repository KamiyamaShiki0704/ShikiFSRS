//! Thread-confined current-process bullet observations and explicit velocity stores.
use crate::{
    WW_171, Ww171File,
    bullet_spawn::{
        BulletSpawnCall, BulletSpawnInput, BulletSpawnObservation, BulletSpawnTargetWrite,
        SPAWN_EVIDENCE, Spawn, commit_target, invoke_spawn, target_plan,
    },
    bullets::{
        BulletIdentity, BulletListObservation, BulletObservation, BulletVelocityWrite,
        commit_velocity, velocity_plan,
    },
};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

pub struct CurrentBullets {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentBullets {
    pub fn spawn_input(&self, source_address: u64) -> Result<BulletSpawnObservation> {
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .bullet_spawn_input(source_address, self.epoch)
    }
    /// Retarget the current hook's input without copying unknown padding or rewriting neighbors.
    /// # Safety
    /// Caller retains the original input and proves writable target extent, exclusive permitted
    /// spawn phase and all referenced allocations. New target is a valid retained full handle.
    /// Bind obligations hold throughout. Postflight failure retains the actual store receipt.
    pub unsafe fn compare_spawn_target(
        &self,
        expected: BulletSpawnObservation,
        target: u64,
    ) -> Result<BulletSpawnTargetWrite> {
        if expected.module_base != self.base || expected.epoch != self.epoch {
            return Err(Error::InvalidProfile("foreign bullet spawn input scope"));
        }
        let plan = target_plan(
            self.spawn_input(expected.source_address)?,
            expected.clone(),
            target,
        )?;
        Ok(commit_target(
            plan,
            |address, target| {
                // SAFETY: Caller retains exclusive initialized writable aligned eight-byte target.
                unsafe { (address as *mut u64).write(target) }
            },
            || self.spawn_input(expected.source_address),
        ))
    }
    /// Call the independent four-argument entry once with captured fields and two owned outputs.
    /// # Safety
    /// Caller provides complete live allocator/resource/PARAM/character/targeting/native-helper
    /// graph and exclusive permitted spawn phase. All opaque input references and native code,
    /// hooks and DLL remain valid throughout, and hook recursion is controlled by the caller.
    /// Captured bytes do not retain those references. Native failure may terminate; a returned
    /// receipt does not acknowledge a live bullet. Never retry a call because postflight failed.
    pub unsafe fn spawn(
        &self,
        expected_manager: u64,
        input: &BulletSpawnInput,
        limit: usize,
    ) -> Result<BulletSpawnCall> {
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let objects =
            file.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?;
        let before = objects
            .bullets(self.epoch, limit)?
            .ok_or(Error::InvalidProfile("bullet manager unavailable"))?;
        if before.manager_address != expected_manager {
            return Err(Error::InvalidProfile("bullet spawn manager superseded"));
        }
        let address = usize::try_from(objects.module.code_address(SPAWN_EVIDENCE[0])?)
            .map_err(|_| Error::AddressOverflow)?;
        // SAFETY: Complete entry proves RCX manager/RDX u32 output/R8 input/R9 u32 status,
        // and returns RDX's original pointer in RAX after native local cleanup.
        let function: Spawn = unsafe { std::mem::transmute(address) };
        // SAFETY: Caller retains the graph and exclusive phase; outputs are initialized locals.
        Ok(unsafe {
            invoke_spawn(function, expected_manager, input, || {
                let after = objects
                    .bullets(self.epoch, limit)?
                    .ok_or(Error::InvalidProfile("bullet manager vanished after spawn"))?;
                if after.manager_address != before.manager_address
                    || after.pool_address != before.pool_address
                {
                    return Err(Error::InvalidProfile(
                        "bullet spawn manager changed after call",
                    ));
                }
                Ok(())
            })
        })
    }
    /// # Safety
    /// Caller identifies the loaded ER image and retains its code, patched handlers and all
    /// used bullet/manager allocations. Stores require exclusive permitted engine phase,
    /// writable initialized XYZ extent and no aliases, teardown, replacement or address reuse.
    /// Epoch is caller-owned; observations do not establish allocation lifetime or a lock.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero bullet lifetime epoch"));
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
    pub fn observe(&self, limit: usize) -> Result<Option<BulletListObservation>> {
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .bullets(self.epoch, limit)
    }
    pub fn resolve(
        &self,
        identity: BulletIdentity,
        limit: usize,
    ) -> Result<Option<BulletObservation>> {
        if identity.epoch != self.epoch {
            return Ok(None);
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .bullet(identity, limit)
    }
    /// Replace XYZ only, preserving W, direction and the separate cached velocity vector.
    /// # Safety
    /// Bind's exclusive phase, lifetime and writable extent obligations hold through postflight.
    /// Three stores are not atomic. Failed postflight reports any actual write; caller reconciles
    /// before later work. Does not call the native redirect helper or rebuild any engine cache.
    pub unsafe fn compare_velocity(
        &self,
        expected: BulletObservation,
        xyz: [f32; 3],
        limit: usize,
    ) -> Result<BulletVelocityWrite> {
        let bits = xyz.map(f32::to_bits);
        if xyz.into_iter().any(|value| !value.is_finite()) {
            return Err(Error::InvalidProfile("nonfinite requested bullet velocity"));
        }
        let plan = velocity_plan(self.resolve(expected.identity, limit)?, expected, bits)?;
        Ok(commit_velocity(
            plan,
            |address, bits| {
                // SAFETY: Caller provides exclusive live initialized writable twelve-byte extent.
                unsafe { (address as *mut [u32; 3]).write(bits) }
            },
            || self.resolve(expected.identity, limit),
        ))
    }
    /// # Safety
    /// Caller identifies a new known allocation lifetime generation with bind's obligations.
    pub unsafe fn advance_epoch(&mut self, epoch: u64) -> Result<()> {
        if epoch <= self.epoch {
            return Err(Error::InvalidProfile("bullet epoch must advance"));
        }
        self.epoch = epoch;
        Ok(())
    }
}
