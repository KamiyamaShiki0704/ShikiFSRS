//! Explicit creator preparation, scheduling selection/mark and direct creation.
use crate::{
    WW_171, Ww171File,
    character_sets::{
        CharacterIdentity, CharacterRegistrationObservation, WorldCreationSetObservation,
    },
    debug_creation_fields::OrdinaryCreationRoute,
    debug_creator::{
        CREATOR_EVIDENCE, DebugCreationBackreadObservation, DebugCreatorDispatchObservation,
        DebugCreatorObservation, DebugCreatorStep, INPUT_OFFSET, NAME_OBJECT_OFFSET, RESULT_OFFSET,
    },
    npc_player_data::{NpcPlayerDataPoolObservation, PlayerNpcDataObservation},
};
use shikifsrs_runtime::{
    Error, Result,
    code_patches::OwnedCodePatches,
    memory::{AddressSpace, CurrentProcess},
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

/// A native call occurred even when either observation fails. Retain this receipt to
/// coordinate ownership; never retry just because postflight failed.
#[derive(Debug)]
pub struct NativeDebugCreationCall {
    pub source: DebugCreatorObservation,
    pub raw_result: Result<u64>,
    pub postflight: Result<DebugCreatorObservation>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugCreationAttemptStage {
    Prepared,
    SelectingLoad,
    LoadSelected,
    MarkingAdvance,
    Scheduled,
    /// The unique scheduled dispatcher was invoked; never repeat it after missing postflight.
    DispatchAttempted,
    Dispatched,
    ScheduledResultObserved,
    SchedulingUncertain,
    Attempted,
    Returned,
    /// Clearing was attempted; failed postflight does not permit an implicit retry.
    ResultClearAttempted,
    ResultCleared,
}
/// Unique SDK attempt record. No cloning, native allocation release or engine pin on Drop.
pub struct PreparedDebugCreation {
    source: DebugCreatorObservation,
    baseline: WorldCreationSetObservation,
    route: OrdinaryCreationRoute,
    npc_pool: Option<NpcPlayerDataPoolObservation>,
    primary_player: Option<CharacterIdentity>,
    base: u64,
    file_sha256: [u8; 32],
    limit: usize,
    stage: DebugCreationAttemptStage,
    _thread: PhantomData<Rc<()>>,
}
impl PreparedDebugCreation {
    pub fn source(&self) -> &DebugCreatorObservation {
        &self.source
    }
    pub fn baseline(&self) -> &WorldCreationSetObservation {
        &self.baseline
    }
    pub fn stage(&self) -> DebugCreationAttemptStage {
        self.stage
    }
    pub fn route(&self) -> OrdinaryCreationRoute {
        self.route
    }
    pub fn npc_pool_before(&self) -> Option<&NpcPlayerDataPoolObservation> {
        self.npc_pool.as_ref()
    }
    pub fn primary_player_before(&self) -> Option<CharacterIdentity> {
        self.primary_player
    }
}
#[derive(Debug)]
pub struct NativeRegisteredCreationCall {
    pub call: NativeDebugCreationCall,
    /// Some means exactly one new member of the prepared ordinary route agrees with the result and
    /// reciprocal entry link. None means zero result with unchanged copied set. Neither is
    /// an engine ownership/pin/free/readiness guarantee. Failure preserves the actual call.
    pub registration: Result<Option<CharacterRegistrationObservation>>,
    /// Independent whole-pool and primary-player observations retained even if admission fails.
    pub npc_pool_after: Result<Option<NpcPlayerDataPoolObservation>>,
    pub primary_player_after: Result<Option<CharacterIdentity>>,
    /// Player admission also requires this result: exactly one first-free NPC slot agrees
    /// with the new character's typed +580 link, preserving every old allocation/identifier.
    /// Enemy/zero results require the entire NPC pool unchanged. No data ownership granted.
    pub npc_data: Result<Option<PlayerNpcDataObservation>>,
}

/// Complete copied admission state; no allocation, pin, resource or release authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugCreationResultObservation {
    pub creator: DebugCreatorObservation,
    pub set: WorldCreationSetObservation,
    pub registration: Option<CharacterRegistrationObservation>,
    pub npc_pool: Option<NpcPlayerDataPoolObservation>,
    pub npc_data: Option<PlayerNpcDataObservation>,
    pub primary_player: Option<CharacterIdentity>,
}
/// Clearing this result slot never retires or frees the created character.
#[derive(Debug)]
pub struct DebugCreationResultClear {
    pub before: DebugCreationResultObservation,
    /// Exactly one eight-byte store occurred; postflight failure retains this fact.
    pub changed: bool,
    pub postflight: Result<DebugCreationResultObservation>,
}

/// A bounded consumed-name assignment ran; retain its postflight even if output differs.
#[derive(Debug)]
pub struct NativeCreationNameAssignment {
    pub source: DebugCreatorObservation,
    pub assigned_name: Vec<u16>,
    pub returned_destination: bool,
    pub postflight: Result<DebugCreatorObservation>,
}
#[derive(Debug)]
pub struct NativeCreatorAdvanceCall {
    pub source: DebugCreatorObservation,
    pub operation: CreatorAdvanceOperation,
    /// Explicit single native flag-changing attempt, even if unchanged. No retry/Drop restore.
    pub postflight: Result<DebugCreatorObservation>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreatorAdvanceOperation {
    Mark,
    Clear,
}
impl CreatorAdvanceOperation {
    fn value(self) -> u8 {
        match self {
            Self::Mark => 1,
            Self::Clear => 0,
        }
    }
    fn entry(self) -> u32 {
        match self {
            Self::Mark => 0x52baa0,
            Self::Clear => 0x52a9f0,
        }
    }
}
#[derive(Debug)]
pub struct NativeCreatorStepCall {
    pub source: DebugCreatorObservation,
    pub selected_step: DebugCreatorStep,
    /// A single native selection attempt occurred; failed observation never permits retry.
    pub postflight: Result<DebugCreatorObservation>,
}
/// At least the selection call happened. Err advance retains a partial sequence;
/// successful postflights prove the two stores, not later engine consumption or loading.
#[derive(Debug)]
pub struct NativeCreationSchedule {
    pub selection: NativeCreatorStepCall,
    pub advance: Result<NativeCreatorAdvanceCall>,
}
/// The dispatcher was called once even when its complete result copy failed.
/// Admission/clearing remain separate operations; no native resource/free authority is granted.
#[derive(Debug)]
pub struct NativeScheduledDispatchCall {
    pub source: DebugCreatorDispatchObservation,
    pub postflight: Result<DebugCreatorObservation>,
}
/// A copied result associated with one scheduled input. No Clone or engine cleanup on Drop.
pub struct ScheduledCreationResult {
    source: DebugCreatorObservation,
    route: OrdinaryCreationRoute,
    base: u64,
    file_sha256: [u8; 32],
    state: DebugCreationResultObservation,
    _thread: PhantomData<Rc<()>>,
}
impl ScheduledCreationResult {
    pub fn observation(&self) -> &DebugCreationResultObservation {
        &self.state
    }
}
pub enum ScheduledCreationPoll {
    /// A zero slot is not proof that the engine ran or creation failed. Retain the ticket.
    Pending(Box<DebugCreationResultObservation>),
    Registered(Box<ScheduledCreationResult>),
}

pub struct CurrentDebugCreator {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentDebugCreator {
    /// # Safety
    /// Caller pins world/creator/name storage, excludes aliases, teardown/ABA and unload,
    /// and supplies each operation's exclusive permitted phase and complete native graph.
    /// Creation additionally requires all initialized allocator/PARAM/model/animation/physics/
    /// task/callback dependencies and resource readiness. Name assignment needs stable owned
    /// string storage and its copy graph. Every hook/DLL residency contract holds. No guards
    /// are acquired by binding's file/code checks.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero native debug creator epoch"));
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

    pub fn observe(&self) -> Result<Option<DebugCreatorObservation>> {
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .debug_creator(self.epoch)
    }
    /// Caller retains binding's complete target/world lifetime contract for this copy.
    /// A native ready bit does not satisfy prepare/execute's resource prerequisites.
    pub fn observe_creation_backread(
        &self,
        expected: CharacterRegistrationObservation,
        limit: usize,
    ) -> Result<Option<DebugCreationBackreadObservation>> {
        if expected.character.epoch != self.epoch {
            return Err(Error::InvalidProfile("foreign creation backread epoch"));
        }
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .creation_backread(expected, limit)
    }
    /// # Safety
    /// Caller owns/pins the full creator/scheduler/request graph and its engine readers, supplies
    /// an exclusive permitted phase and writable advance byte, excludes callback/alias/ABA/
    /// teardown/unload, and independently establishes that eventual native consumption of this
    /// mark under current/next steps is valid. Keep request/resource/task/native dependencies
    /// alive for that later engine consumption. This only sets48 through native52baa0: no step
    /// selection, resource loading/ready predicate, request publication, immediate creation or
    /// automatic retry/clear/Drop restoration. Unknown descriptor steps fail closed.
    pub unsafe fn mark_schedule_advance(
        &self,
        expected: DebugCreatorObservation,
    ) -> Result<NativeCreatorAdvanceCall> {
        // SAFETY: Caller supplies the complete graph and exclusive mark contract above.
        unsafe { self.change_schedule_advance(expected, CreatorAdvanceOperation::Mark) }
    }
    /// Clear only the pending advancement byte, without changing the selected descriptor.
    /// # Safety
    /// Caller owns/pins the complete scheduler/request graph, proves the pending mark may be
    /// cancelled in this exclusive phase, and excludes readers/callbacks/aliases/ABA/teardown/
    /// unload throughout dispatch. Keep dependencies valid until independently proving no
    /// earlier engine consumption or outstanding callback/request remains. A zero postflight
    /// byte does not establish drain, idle, resource release or permission to free/unload.
    /// No automatic retry/rollback/Drop follows observation failure.
    pub unsafe fn clear_schedule_advance(
        &self,
        expected: DebugCreatorObservation,
    ) -> Result<NativeCreatorAdvanceCall> {
        // SAFETY: Caller supplies the complete graph and exclusive cancellation contract above.
        unsafe { self.change_schedule_advance(expected, CreatorAdvanceOperation::Clear) }
    }
    unsafe fn change_schedule_advance(
        &self,
        expected: DebugCreatorObservation,
        operation: CreatorAdvanceOperation,
    ) -> Result<NativeCreatorAdvanceCall> {
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let current = objects.debug_creator(self.epoch)?;
        schedule_preflight(current.as_ref(), &expected, self.epoch)?;
        let range = CREATOR_EVIDENCE
            .iter()
            .find(|r| r.rva == operation.entry())
            .ok_or(Error::InvalidProfile("missing creator advance evidence"))?;
        let entry = objects.module.code_address(*range)?;
        // SAFETY: Each operation has a complete single-RCX/void thunk and13-byte return target.
        let function: ChangeAdvance = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Caller owns valid scheduler operation and pins all outstanding dependencies.
        Ok(unsafe {
            invoke_advance(function, expected, operation, || {
                objects
                    .debug_creator(self.epoch)?
                    .ok_or(Error::InvalidProfile(
                        "creator unavailable after advance mark",
                    ))
            })
        })
    }
    /// Select only the next descriptor; advancement remains a separate explicit operation.
    /// # Safety
    /// Caller owns/pins the complete creator/scheduler/request graph, supplies an exclusive
    /// permitted phase and writable next-step word, and excludes callbacks, aliases, ABA,
    /// teardown and unload throughout dispatch. Current step must remain known0..2 so the
    /// independently reconstructed permitted return path applies. Caller independently proves
    /// the selected step is valid for the request and keeps its full resource/task/native graph
    /// alive for later engine consumption. This does not acquire ownership, load resources,
    /// establish readiness, publish a complete request or form a transaction with advance.
    /// No automatic retry, rollback or Drop restoration follows a failed postflight.
    pub unsafe fn select_next_step(
        &self,
        expected: DebugCreatorObservation,
        selected_step: DebugCreatorStep,
    ) -> Result<NativeCreatorStepCall> {
        let raw_step = selected_step_raw(selected_step)?;
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let current = objects.debug_creator(self.epoch)?;
        schedule_preflight(current.as_ref(), &expected, self.epoch)?;
        let range = CREATOR_EVIDENCE
            .iter()
            .find(|r| r.rva == 0x52bbc0)
            .ok_or(Error::InvalidProfile("missing creator step evidence"))?;
        let entry = objects.module.code_address(*range)?;
        // SAFETY: Actual entry, reconstructed RCX/EDX/void ABI and known-state return path.
        let function: SelectNextStep = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Caller excludes current-step changes and owns the full future request graph.
        Ok(unsafe {
            invoke_next_step(function, expected, selected_step, raw_step, || {
                objects
                    .debug_creator(self.epoch)?
                    .ok_or(Error::InvalidProfile(
                        "creator unavailable after step selection",
                    ))
            })
        })
    }
    /// Assign only the factory-consumed string, without initializing/resetting its storage.
    /// # Safety
    /// Caller proves exclusive idle creator/string access and initialized writable storage
    /// for its full capacity, pins the complete native assignment/copy graph, and prevents
    /// UI writers or callbacks from replacing/synchronizing it during/after preparation.
    /// Does not claim UI buffer50 synchronization, loading readiness or request submission.
    /// Current capacity must fit the supplied name, so no native allocation is requested.
    pub unsafe fn assign_consumed_name(
        &self,
        expected: DebugCreatorObservation,
        name: Vec<u16>,
    ) -> Result<NativeCreationNameAssignment> {
        if expected.epoch != self.epoch {
            return Err(Error::InvalidProfile("stale name assignment epoch"));
        }
        validate_name_assignment(&expected, &name)?;
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        if objects.debug_creator(self.epoch)? != Some(expected.clone()) {
            return Err(Error::NativeOperationRejected(
                "creator name source superseded",
            ));
        }
        let destination = expected
            .object
            .address
            .checked_add(u64::from(INPUT_OFFSET) + u64::from(NAME_OBJECT_OFFSET))
            .ok_or(Error::AddressOverflow)?;
        let range =
            CREATOR_EVIDENCE
                .iter()
                .find(|x| x.rva == 0x1602d0)
                .ok_or(Error::InvalidProfile(
                    "missing consumed-name setter evidence",
                ))?;
        let entry = objects.module.code_address(*range)?;
        // SAFETY: Complete RCX initialized string/RDX source UTF16/R8 count/RAX destination ABI.
        let function: AssignName = unsafe { std::mem::transmute(entry as usize) };
        let before = expected.clone();
        let requested = name.clone();
        // SAFETY: Caller supplies stable destination/copy graph; helper owns disjoint source.
        unsafe {
            invoke_name(function, destination, expected, name, || {
                let after = objects
                    .debug_creator(self.epoch)?
                    .ok_or(Error::InvalidProfile(
                        "creator vanished after name assignment",
                    ))?;
                check_name_postflight(&before, &requested, &after)?;
                Ok(after)
            })
        }
    }

    /// Change exactly one proved input word, retaining the attempted write and full postflight.
    /// # Safety
    /// Caller owns exclusive idle creator access and the writable four-byte storage, pins all
    /// related objects/name storage, and excludes engine/UI callbacks, teardown/ABA/aliases.
    /// Use each successful receipt's updated snapshot for the next word. A failed postflight
    /// requires reconciliation before any further preparation/creation; no automatic retry or
    /// rollback runs. This is neither a batch transaction nor a resource request/submit action.
    pub unsafe fn replace_prepared_word(
        &self,
        expected: DebugCreatorObservation,
        field: crate::debug_creation_fields::CreatorInputWord,
        bits: u32,
    ) -> Result<crate::debug_creation_fields::CreatorInputWordWrite> {
        use crate::debug_creation_fields::{commit_word, store_word, word_plan};
        if expected.epoch != self.epoch {
            return Err(Error::InvalidProfile("stale creator preparation epoch"));
        }
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let plan = word_plan(objects.debug_creator(self.epoch)?, expected, field, bits)?;
        Ok(commit_word(
            plan,
            |address, value| {
                // SAFETY: Caller pins exclusive writable input; plan verifies full snapshot/alignment.
                unsafe { store_word(address, value) };
            },
            || {
                objects
                    .debug_creator(self.epoch)?
                    .ok_or(Error::InvalidProfile(
                        "creator unavailable after preparation",
                    ))
            },
        ))
    }

    /// Select the independently proved C8/C9 zero ordinary EnemyIns factory route.
    /// # Safety
    /// Same exclusive idle/full source/name lifetime and writable input contract as word
    /// preparation. Caller owns this new creation request; opaque borrowed argument must be
    /// null. Does not copy UI state, acquire ownership/resources, submit, or free anything.
    /// Failed postflight preserves the write and requires external reconciliation.
    pub unsafe fn select_ordinary_enemy(
        &self,
        expected: DebugCreatorObservation,
    ) -> Result<crate::debug_creation_fields::CreatorEnemyRouteWrite> {
        use crate::debug_creation_fields::{
            commit_enemy_route, enemy_route_plan, store_enemy_route,
        };
        if expected.epoch != self.epoch {
            return Err(Error::InvalidProfile("stale creator route epoch"));
        }
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let plan = enemy_route_plan(objects.debug_creator(self.epoch)?, expected)?;
        Ok(commit_enemy_route(
            plan,
            |address| {
                // SAFETY: Source plan validates full identity/alignment; caller pins writable kinds.
                unsafe { store_enemy_route(address) };
            },
            || {
                objects
                    .debug_creator(self.epoch)?
                    .ok_or(Error::InvalidProfile(
                        "creator unavailable after route selection",
                    ))
            },
        ))
    }

    /// Select C8=1/C9=0 ordinary PlayerIns, without allocating or submitting a request.
    /// # Safety
    /// Same exclusive idle/full source/writable-input contract as select_ordinary_enemy.
    /// Caller owns this new request and keeps the future Player PARAM/data/equipment/resource
    /// graph valid; this store neither initializes that graph nor grants engine ownership.
    pub unsafe fn select_ordinary_player(
        &self,
        expected: DebugCreatorObservation,
    ) -> Result<crate::debug_creation_fields::CreatorRouteWrite> {
        use crate::debug_creation_fields::{commit_route, route_plan, store_route};
        if expected.epoch != self.epoch {
            return Err(Error::InvalidProfile("stale creator route epoch"));
        }
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let plan = route_plan(
            objects.debug_creator(self.epoch)?,
            expected,
            OrdinaryCreationRoute::Player,
        )?;
        Ok(commit_route(
            plan,
            |address, bytes| {
                // SAFETY: Full source validated; caller pins and exclusively owns both writable bytes.
                unsafe { store_route(address, bytes) };
            },
            || {
                objects
                    .debug_creator(self.epoch)?
                    .ok_or(Error::InvalidProfile(
                        "creator unavailable after Player route selection",
                    ))
            },
        ))
    }

    /// Execute already prepared, resource-ready inputs using complete native 52b5e0.
    /// # Safety
    /// Bind's contract holds throughout the call and observations. Caller independently
    /// proves the creator idle and owns this pending operation; count must be exactly one,
    /// previous result storage empty. This bypasses the engine Load/LoadWait scheduling:
    /// it does not load resources, submit a flag, or replace a readiness check. A returned
    /// address gives no pin/free authority. On any post-call failure reconcile the actual
    /// operation; repeating may create another character. SDK never frees on Drop.
    pub unsafe fn execute_ready_single(
        &self,
        expected: DebugCreatorObservation,
    ) -> Result<NativeDebugCreationCall> {
        // SAFETY: Same full resource-ready native graph and exclusive caller phase contract.
        unsafe { self.execute_single_with(expected, || {}) }
    }
    /// Prepare one ordinary Enemy/Player attempt with copied set, NPC pool and primary baseline.
    /// Reads only; does not initialize input/name, load resources, lock the engine or acquire
    /// permission to dispatch. The execute method requires its full unsafe contract separately.
    pub fn prepare_ready_single(
        &self,
        expected: DebugCreatorObservation,
        limit: usize,
    ) -> Result<PreparedDebugCreation> {
        validate_ready_single(&expected)?;
        schedule_preflight(Some(&expected), &expected, self.epoch)?;
        let route = OrdinaryCreationRoute::from_bytes(expected.input.kind_bytes)?;
        if expected.input.native_argument != 0 {
            return Err(Error::NativeOperationRejected(
                "registered creation requires ordinary route without borrowed argument",
            ));
        }
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let baseline = objects
            .world_creation_set(limit)?
            .ok_or(Error::InvalidProfile("world creation set unavailable"))?;
        let npc_pool = objects.npc_player_data_pool(self.epoch)?;
        let primary_player = objects.primary_player_identity(self.epoch)?;
        validate_creation_pool_baseline(route, npc_pool.as_ref(), self.base, self.epoch)?;
        if baseline.world_address != expected.world.address
            || objects.debug_creator(self.epoch)? != Some(expected.clone())
            || objects.npc_player_data_pool(self.epoch)? != npc_pool
            || objects.primary_player_identity(self.epoch)? != primary_player
            || objects.world_creation_set(limit)? != Some(baseline.clone())
        {
            return Err(Error::NativeOperationRejected(
                "creation preparation superseded",
            ));
        }
        Ok(PreparedDebugCreation {
            source: expected,
            baseline,
            route,
            npc_pool,
            primary_player,
            base: self.base,
            file_sha256: WW_171.sha256,
            limit,
            stage: DebugCreationAttemptStage::Prepared,
            _thread: PhantomData,
        })
    }
    /// Select Load and mark advancement for one unique prepared ordinary input.
    /// Returns Err only before any native selection call. A partial receipt preserves
    /// the selection and forbids repeating it; resume_prepared_schedule may finish only
    /// a known unattempted mark while its exact selected source is still unchanged.
    /// # Safety
    /// The complete select_next_step/mark_schedule_advance native, residency and exclusive
    /// phase contracts hold for the entire sequence. Caller independently owns this request,
    /// pins all resources and native dependencies until engine consumption or reconciliation,
    /// and excludes all creator/world/set/NPC/primary writers, callbacks, teardown and ABA.
    /// Caller proves Load/LoadWait is valid for these resource-ready inputs and the actual
    /// engine dispatcher. This does not submit assets, prove readiness, run the dispatcher
    /// or call the factory. No rollback, repeated native call or Drop cleanup occurs.
    pub unsafe fn schedule_prepared_single(
        &self,
        prepared: &mut PreparedDebugCreation,
    ) -> Result<NativeCreationSchedule> {
        validate_prepared_ticket(prepared, self.base, self.epoch)?;
        validate_schedule_source(&prepared.source)?;
        self.check_schedule_baseline(prepared, &prepared.source)?;
        schedule_with(
            prepared,
            |source| {
                // SAFETY: Caller guarantees known idle/Load transition and the full pinned graph.
                unsafe { self.select_next_step(source, DebugCreatorStep::Load) }
            },
            |source| {
                // SAFETY: Same exclusive sequence; the observed selected source is unchanged.
                unsafe { self.mark_schedule_advance(source) }
            },
        )
    }
    /// Finish only the mark whose prior preflight rejected it before any native mark call.
    /// # Safety
    /// schedule_prepared_single's full contract still holds without releasing its request
    /// ownership/dependencies. Caller excludes dispatcher consumption between the selected
    /// snapshot and this explicit continuation; a readback is not a synchronization barrier.
    /// An uncertain postflight never permits this method or an automatic retry.
    pub unsafe fn resume_prepared_schedule(
        &self,
        prepared: &mut PreparedDebugCreation,
        expected: DebugCreatorObservation,
    ) -> Result<NativeCreatorAdvanceCall> {
        validate_selected_ticket(prepared, &expected, self.base, self.epoch)?;
        self.check_schedule_baseline(prepared, &expected)?;
        mark_selected_with(prepared, expected, |source| {
            // SAFETY: Caller retains the exact selected request and all phase/residency guarantees.
            unsafe { self.mark_schedule_advance(source) }
        })
    }
    fn check_schedule_baseline(
        &self,
        prepared: &PreparedDebugCreation,
        expected: &DebugCreatorObservation,
    ) -> Result<()> {
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        for _ in 0..2 {
            if objects.debug_creator(self.epoch)? != Some(expected.clone())
                || objects.world_creation_set(prepared.limit)? != Some(prepared.baseline.clone())
                || objects.npc_player_data_pool(self.epoch)? != prepared.npc_pool
                || objects.primary_player_identity(self.epoch)? != prepared.primary_player
            {
                return Err(Error::NativeOperationRejected(
                    "creation scheduling baseline superseded",
                ));
            }
        }
        Ok(())
    }

    /// Run the independently reconstructed no-dependency/no-queue ordinary dispatch path once.
    /// Err means no call occurred. A returned receipt means the call occurred, including failed
    /// postflight; only poll_scheduled_result may subsequently reconcile a result.
    /// # Safety
    /// All execute_prepared_single resource-ready/allocator/PARAM/physics/animation/subtype and
    /// residency obligations hold for this entire call and subsequent engine use. Supply the
    /// legal exclusive dispatcher phase and retain this scheduled request, its full native graph
    /// and initialized descriptor/vtable storage. Exclude every other dispatcher/callback/writer,
    /// dependency publication, queue mutation, teardown and ABA. The guards are snapshots, not
    /// locks. No asset request, readiness detection, worker or implicit retry is provided.
    pub unsafe fn dispatch_scheduled_single(
        &self,
        prepared: &mut PreparedDebugCreation,
    ) -> Result<NativeScheduledDispatchCall> {
        validate_scheduled_ticket(prepared, self.base, self.epoch)?;
        if prepared.stage != DebugCreationAttemptStage::Scheduled {
            return Err(Error::NativeOperationRejected(
                "scheduled dispatcher already attempted",
            ));
        }
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let source = objects
            .debug_creator_dispatch(self.epoch)?
            .ok_or(Error::InvalidProfile(
                "scheduled dispatch creator unavailable",
            ))?;
        validate_dispatch_source(prepared, &source)?;
        self.check_schedule_baseline(prepared, &source.creator)?;
        if objects.debug_creator_dispatch(self.epoch)? != Some(source.clone()) {
            return Err(Error::NativeOperationRejected(
                "scheduled dispatch header superseded",
            ));
        }
        let range = CREATOR_EVIDENCE
            .iter()
            .find(|r| r.rva == 0x52ae80)
            .ok_or(Error::InvalidProfile("missing creator dispatcher evidence"))?;
        let entry = objects.module.code_address(*range)?;
        // SAFETY: Actual two-register entry and the complete guarded path, checked against fixed code.
        let function: ExecuteScheduled = unsafe { std::mem::transmute(entry as usize) };
        // SAFETY: Caller retains the exclusive valid phase/full ready graph through call and observation.
        unsafe {
            dispatch_scheduled_with(prepared, source, function, || {
                objects
                    .debug_creator(self.epoch)?
                    .ok_or(Error::InvalidProfile(
                        "creator unavailable after dispatcher",
                    ))
            })
        }
    }

    /// Observe one successfully scheduled/dispatched ticket without invoking the dispatcher/factory.
    /// A zero result with unchanged creation set/pool remains Pending, even after a timeout.
    /// Two full copies admit only one new reciprocal member and its Player data association.
    /// This consumes result observation only when admitted, not engine ownership/resources.
    pub fn poll_scheduled_result(
        &self,
        prepared: &mut PreparedDebugCreation,
    ) -> Result<ScheduledCreationPoll> {
        validate_scheduled_ticket(prepared, self.base, self.epoch)?;
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        poll_scheduled_with(prepared, |ticket| {
            let creator = objects
                .debug_creator(self.epoch)?
                .ok_or(Error::InvalidProfile("scheduled creator unavailable"))?;
            check_scheduled_progress(&ticket.source, &creator)?;
            let target = if creator.last_created_address == 0 {
                None
            } else {
                Some(
                    objects
                        .character_identity(creator.last_created_address, self.epoch)?
                        .ok_or(Error::InvalidProfile(
                            "scheduled result identity unavailable",
                        ))?,
                )
            };
            let state = read_creation_result_components(&objects, ticket, target)?;
            if state.creator != creator {
                return Err(Error::InvalidProfile(
                    "scheduled result changed during identity lookup",
                ));
            }
            Ok(state)
        })
    }
    /// Clear only this admitted asynchronous result's exact eight-byte slot.
    /// # Safety
    /// The full bind/scheduling/native graph and exclusive writable phase contract holds.
    /// Caller pins the admitted character/set/pool/result storage and excludes all other
    /// writers, callbacks, identity reuse and teardown. Legitimate scheduler progression
    /// since polling is allowed before preflight; all input/result and member associations
    /// must still match. Within the two copies and store, the whole graph is stable.
    /// Clearing grants no resource, retirement or free authority; postflight failure keeps
    /// the actual store and forbids repeating it. Drop never changes engine state.
    pub unsafe fn clear_scheduled_result(
        &self,
        prepared: &mut PreparedDebugCreation,
        result: &ScheduledCreationResult,
    ) -> Result<DebugCreationResultClear> {
        validate_scheduled_result_ticket(prepared, result, self.base, self.epoch)?;
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let target = result.state.registration.map(|v| v.character);
        let before = read_creation_result_components(&objects, prepared, target)?;
        validate_scheduled_clear_state(prepared, result, &before)?;
        if read_creation_result_components(&objects, prepared, target)? != before {
            return Err(Error::NativeOperationRejected(
                "scheduled clearing source changed",
            ));
        }
        let address = result_clear_address(&before.creator)?;
        Ok(commit_result_clear(
            prepared,
            before,
            || {
                // SAFETY: Exact proved initialized eight-byte slot and full exclusive caller contract.
                unsafe { (address as *mut u64).write(0) };
            },
            |ticket| {
                let first = read_creation_result_components(&objects, ticket, target)?;
                if read_creation_result_components(&objects, ticket, target)? != first {
                    return Err(Error::InvalidProfile("scheduled clear postflight changed"));
                }
                Ok(first)
            },
        ))
    }

    /// Execute a unique ordinary Enemy/Player attempt and observe its new member and NPC data.
    /// # Safety
    /// execute_ready_single's resource-ready, complete graph, exclusive permitted phase,
    /// caller ownership and residency contracts hold throughout. Caller excludes all other
    /// mutations of the entire embedded set and creator, identity reuse and asynchronous
    /// teardown. Native internal allocation/registration and all resource dependencies are
    /// valid independently. A registered observation does not grant pin/destructor/free rights.
    /// Player additionally requires all GameDataMan/NPC pool, allocator, CharaInitParam23,
    /// inventory/equipment and subtype-mode dependencies pinned and initialized. Caller excludes
    /// all other pool writers. Player destruction releases its data via257850; never separately
    /// free the still-linked data. Primary-player changes or partial pool effects fail admission.
    /// The ticket becomes Attempted before dispatch, including if the observer later fails.
    pub unsafe fn execute_prepared_single(
        &self,
        prepared: &mut PreparedDebugCreation,
    ) -> Result<NativeRegisteredCreationCall> {
        validate_prepared_ticket(prepared, self.base, self.epoch)?;
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        if objects.world_creation_set(prepared.limit)? != Some(prepared.baseline.clone())
            || objects.npc_player_data_pool(self.epoch)? != prepared.npc_pool
            || objects.primary_player_identity(self.epoch)? != prepared.primary_player
        {
            return Err(Error::NativeOperationRejected(
                "creation set baseline superseded",
            ));
        }
        let expected = prepared.source.clone();
        // SAFETY: Caller supplies complete native graph/phase. The callback marks unique attempt
        // only after all ordinary dispatch checks and immediately before the native invocation.
        let call = unsafe {
            self.execute_single_with(expected, || {
                prepared.stage = DebugCreationAttemptStage::Attempted;
            })
        }?;
        prepared.stage = DebugCreationAttemptStage::Returned;
        let npc_pool_after = objects.npc_player_data_pool(self.epoch);
        let primary_player_after = objects.primary_player_identity(self.epoch);
        let registration = (|| {
            if primary_player_after
                .as_ref()
                .map_err(|_| Error::InvalidProfile("primary player observation failed"))?
                != &prepared.primary_player
            {
                return Err(Error::InvalidProfile(
                    "primary player changed during creation",
                ));
            }
            let raw = *call
                .raw_result
                .as_ref()
                .map_err(|_| Error::InvalidProfile("creation result observation failed"))?;
            let post = call
                .postflight
                .as_ref()
                .map_err(|_| Error::InvalidProfile("creation postflight failed"))?;
            let after = objects
                .world_creation_set(prepared.limit)?
                .ok_or(Error::InvalidProfile("creation set unavailable after call"))?;
            if objects.debug_creator(self.epoch)? != Some(post.clone()) {
                return Err(Error::InvalidProfile(
                    "creator superseded after creation call",
                ));
            }
            if raw == 0 {
                if after != prepared.baseline {
                    return Err(Error::InvalidProfile(
                        "zero creation result with changed set",
                    ));
                }
                return Ok(None);
            }
            let identity =
                objects
                    .character_identity(raw, self.epoch)?
                    .ok_or(Error::InvalidProfile(
                        "native creation result character unavailable",
                    ))?;
            let registration = objects
                .character_registration(identity, after.set.address, prepared.limit)?
                .ok_or(Error::InvalidProfile(
                    "native result is not registered in creation set",
                ))?;
            check_new_creation_member(
                &prepared.baseline,
                &after,
                identity,
                registration,
                prepared.route,
            )?;
            if objects.world_creation_set(prepared.limit)? != Some(after) {
                return Err(Error::InvalidProfile(
                    "creation registration changed during observation",
                ));
            }
            Ok(Some(registration))
        })();
        let npc_data = (|| {
            let member = registration
                .as_ref()
                .map_err(|_| Error::InvalidProfile("creation registration not admitted"))?;
            let pool = npc_pool_after
                .as_ref()
                .map_err(|_| Error::InvalidProfile("creation NPC pool observation failed"))?;
            let associated = match (prepared.route, member) {
                (OrdinaryCreationRoute::Player, Some(member)) => Some(
                    objects
                        .npc_player_data_for_character(member.character)?
                        .ok_or(Error::InvalidProfile(
                            "new PlayerIns data is outside NPC pool",
                        ))?,
                ),
                _ => None,
            };
            check_creation_pool_delta(
                prepared.route,
                prepared.npc_pool.as_ref(),
                pool.as_ref(),
                associated.as_ref(),
                *member,
            )?;
            if objects.npc_player_data_pool(self.epoch)? != *pool
                || objects.primary_player_identity(self.epoch)? != prepared.primary_player
            {
                return Err(Error::InvalidProfile(
                    "creation data or primary superseded during admission",
                ));
            }
            Ok(associated)
        })();
        Ok(NativeRegisteredCreationCall {
            call,
            registration,
            npc_pool_after,
            primary_player_after,
            npc_data,
        })
    }
    /// Clear the result after admission of this unique ordinary creation attempt.
    /// A zero-result attempt is consumed without writing. The complete creator, set,
    /// reciprocal identity, primary and NPC association are checked twice before the store.
    /// # Safety
    /// The execution contract and exclusive permitted phase remain valid. Caller owns
    /// this exact completed attempt, pins the full graph and writable result slot, excludes
    /// all other creator/set/pool/primary writers and identity reuse. These observations
    /// acquire no engine ownership or resource readiness. Clearing does not release the
    /// character or PlayerGameData. Postflight errors require observation and reconciliation;
    /// this ticket cannot repeat the store. Drop performs no engine cleanup.
    pub unsafe fn clear_prepared_result(
        &self,
        prepared: &mut PreparedDebugCreation,
        call: &NativeRegisteredCreationCall,
    ) -> Result<DebugCreationResultClear> {
        let expected = result_clear_source(prepared, call, self.base, self.epoch)?;
        let view = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            view.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        let before = observe_result_state(&objects, prepared, call, &expected)?;
        if observe_result_state(&objects, prepared, call, &expected)? != before {
            return Err(Error::NativeOperationRejected(
                "creation result admission changed",
            ));
        }
        let address = result_clear_address(&expected)?;
        let mut cleared = expected;
        cleared.last_created_address = 0;
        Ok(commit_result_clear(
            prepared,
            before,
            || {
                // SAFETY: Full preflight and caller's pinned, exclusively writable slot.
                unsafe { (address as *mut u64).write(0) };
            },
            |ticket| {
                let first = observe_result_state(&objects, ticket, call, &cleared)?;
                if observe_result_state(&objects, ticket, call, &cleared)? != first {
                    return Err(Error::InvalidProfile("cleared creation result changed"));
                }
                Ok(first)
            },
        ))
    }

    unsafe fn execute_single_with(
        &self,
        expected: DebugCreatorObservation,
        before_dispatch: impl FnOnce(),
    ) -> Result<NativeDebugCreationCall> {
        if expected.epoch != self.epoch {
            return Err(Error::InvalidProfile("stale native debug creation epoch"));
        }
        validate_ready_single(&expected)?;
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        let memory = CurrentProcess;
        let objects =
            file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
        if objects.debug_creator(self.epoch)? != Some(expected.clone()) {
            return Err(Error::NativeOperationRejected(
                "debug creation input superseded",
            ));
        }
        let result_slot = expected
            .object
            .address
            .checked_add(u64::from(RESULT_OFFSET))
            .ok_or(Error::AddressOverflow)?;
        let entry = objects.module.code_address(CREATOR_EVIDENCE[1])?;
        // SAFETY: Complete ordinary single-RCX creator ABI; caller owns full graph/phase.
        let function: ExecuteCreation = unsafe { std::mem::transmute(entry as usize) };
        let baseline = expected.clone();
        // SAFETY: Caller guarantees live creator and all resource/engine preconditions.
        Ok(unsafe {
            invoke(
                function,
                expected,
                before_dispatch,
                || memory.read_u64(result_slot),
                || {
                    let observed =
                        objects
                            .debug_creator(self.epoch)?
                            .ok_or(Error::InvalidProfile(
                                "creator unavailable after native creation",
                            ))?;
                    check_postflight(&baseline, &observed)?;
                    Ok(observed)
                },
            )
        })
    }
}
fn validate_schedule_source(source: &DebugCreatorObservation) -> Result<()> {
    validate_ready_single(source)?;
    schedule_preflight(Some(source), source, source.epoch)?;
    if source.schedule.current_step != 0
        || source.schedule.next_step != 0
        || source.schedule.advance_byte != 0
        || source.input.native_argument != 0
    {
        return Err(Error::NativeOperationRejected(
            "creation scheduling requires ordinary idle source",
        ));
    }
    Ok(())
}
fn validate_selected_ticket(
    prepared: &PreparedDebugCreation,
    expected: &DebugCreatorObservation,
    base: u64,
    epoch: u64,
) -> Result<()> {
    if prepared.stage != DebugCreationAttemptStage::LoadSelected
        || prepared.base != base
        || prepared.file_sha256 != WW_171.sha256
        || epoch == 0
        || prepared.source.epoch != epoch
        || OrdinaryCreationRoute::from_bytes(prepared.source.input.kind_bytes)? != prepared.route
    {
        return Err(Error::NativeOperationRejected(
            "foreign or already marked scheduling ticket",
        ));
    }
    validate_schedule_source(&prepared.source)?;
    validate_creation_pool_baseline(prepared.route, prepared.npc_pool.as_ref(), base, epoch)?;
    let mut selected = prepared.source.clone();
    selected.schedule.next_step = 1;
    if expected != &selected {
        return Err(Error::NativeOperationRejected(
            "selected Load source superseded",
        ));
    }
    Ok(())
}
fn schedule_with(
    prepared: &mut PreparedDebugCreation,
    select: impl FnOnce(DebugCreatorObservation) -> Result<NativeCreatorStepCall>,
    mark: impl FnOnce(DebugCreatorObservation) -> Result<NativeCreatorAdvanceCall>,
) -> Result<NativeCreationSchedule> {
    prepared.stage = DebugCreationAttemptStage::SelectingLoad;
    let selection = match select(prepared.source.clone()) {
        Ok(receipt) => receipt,
        Err(error) => {
            // Production Err means selection failed before invocation; no receipt is invented.
            prepared.stage = DebugCreationAttemptStage::Prepared;
            return Err(error);
        }
    };
    let mut selected = prepared.source.clone();
    selected.schedule.next_step = 1;
    let admitted = selection.source == prepared.source
        && selection.selected_step == DebugCreatorStep::Load
        && selection.postflight.as_ref().ok() == Some(&selected);
    let advance = if admitted {
        prepared.stage = DebugCreationAttemptStage::LoadSelected;
        mark_selected_with(prepared, selected, mark)
    } else {
        prepared.stage = DebugCreationAttemptStage::SchedulingUncertain;
        Err(Error::InvalidProfile(
            "Load selection postflight not admitted",
        ))
    };
    Ok(NativeCreationSchedule { selection, advance })
}
fn mark_selected_with(
    prepared: &mut PreparedDebugCreation,
    selected: DebugCreatorObservation,
    mark: impl FnOnce(DebugCreatorObservation) -> Result<NativeCreatorAdvanceCall>,
) -> Result<NativeCreatorAdvanceCall> {
    prepared.stage = DebugCreationAttemptStage::MarkingAdvance;
    let receipt = match mark(selected.clone()) {
        Ok(receipt) => receipt,
        Err(error) => {
            // No native mark occurred; only this mark can be explicitly continued.
            prepared.stage = DebugCreationAttemptStage::LoadSelected;
            return Err(error);
        }
    };
    let mut marked = selected.clone();
    marked.schedule.advance_byte = 1;
    prepared.stage = if receipt.source == selected
        && receipt.operation == CreatorAdvanceOperation::Mark
        && receipt.postflight.as_ref().ok() == Some(&marked)
    {
        DebugCreationAttemptStage::Scheduled
    } else {
        DebugCreationAttemptStage::SchedulingUncertain
    };
    Ok(receipt)
}

fn validate_scheduled_ticket(
    prepared: &PreparedDebugCreation,
    base: u64,
    epoch: u64,
) -> Result<()> {
    if !matches!(
        prepared.stage,
        DebugCreationAttemptStage::Scheduled | DebugCreationAttemptStage::Dispatched
    ) || epoch == 0
        || prepared.base != base
        || prepared.file_sha256 != WW_171.sha256
        || prepared.source.epoch != epoch
        || OrdinaryCreationRoute::from_bytes(prepared.source.input.kind_bytes)? != prepared.route
    {
        return Err(Error::NativeOperationRejected(
            "foreign or consumed scheduled ticket",
        ));
    }
    validate_schedule_source(&prepared.source)?;
    validate_creation_pool_baseline(prepared.route, prepared.npc_pool.as_ref(), base, epoch)
}
fn validate_dispatch_source(
    prepared: &PreparedDebugCreation,
    source: &DebugCreatorDispatchObservation,
) -> Result<()> {
    let base = prepared.base;
    let address = |rva: u64| base.checked_add(rva).ok_or(Error::AddressOverflow);
    let mut expected = prepared.source.clone();
    expected.schedule.next_step = 1;
    expected.schedule.advance_byte = 1;
    if source.creator != expected
        || source.descriptor_address != address(0x3d6a468)?
        || source.dependency_count != 0
        || source.waiting_queue != 0
        || source.special_mode != 0
        || source.descriptors
            != [
                address(0x52b540)?,
                address(0x2a50b00)?,
                address(0x52b5b0)?,
                address(0x2a50b40)?,
                address(0x52b5e0)?,
                address(0x2a50b80)?,
            ]
        || source.scheduler_callbacks
            != [address(0x52b500)?, address(0x52bbc0)?, address(0x52baa0)?]
    {
        return Err(Error::NativeOperationRejected(
            "creator not on bounded ordinary dispatch path",
        ));
    }
    Ok(())
}
type ExecuteScheduled = unsafe extern "system" fn(u64, u64);
unsafe fn dispatch_scheduled_with(
    prepared: &mut PreparedDebugCreation,
    source: DebugCreatorDispatchObservation,
    function: ExecuteScheduled,
    observe: impl FnOnce() -> Result<DebugCreatorObservation>,
) -> Result<NativeScheduledDispatchCall> {
    validate_scheduled_ticket(prepared, prepared.base, prepared.source.epoch)?;
    if prepared.stage != DebugCreationAttemptStage::Scheduled {
        return Err(Error::NativeOperationRejected(
            "scheduled dispatcher already attempted",
        ));
    }
    validate_dispatch_source(prepared, &source)?;
    prepared.stage = DebugCreationAttemptStage::DispatchAttempted;
    // SAFETY: Caller supplies fixed entry, exclusive ready graph and complete bounded path.
    // RDX is unused on this exact zero-dependency path and its Load/ordinary factory callbacks.
    unsafe { function(source.creator.object.address, 0) };
    prepared.stage = DebugCreationAttemptStage::Dispatched;
    let postflight = observe().and_then(|after| {
        check_scheduled_progress(&prepared.source, &after)?;
        if after.schedule.current_step != 0
            || after.schedule.next_step != 0
            || after.schedule.advance_byte != 0
            || after.phase_byte != 0
        {
            return Err(Error::InvalidProfile(
                "ordinary dispatcher return state disagrees",
            ));
        }
        Ok(after)
    });
    Ok(NativeScheduledDispatchCall { source, postflight })
}
fn check_scheduled_progress(
    source: &DebugCreatorObservation,
    after: &DebugCreatorObservation,
) -> Result<()> {
    let mut normalized = after.clone();
    normalized.schedule = source.schedule;
    normalized.phase_byte = source.phase_byte;
    normalized.last_created_address = source.last_created_address;
    if normalized != *source
        || !(0..=2).contains(&after.schedule.current_step)
        || !(0..=2).contains(&after.schedule.next_step)
        || after.schedule.advance_byte > 1
        || after.phase_byte > 1
    {
        return Err(Error::InvalidProfile(
            "scheduled input/storage changed or unknown progress",
        ));
    }
    Ok(())
}
fn validate_scheduled_state(
    prepared: &PreparedDebugCreation,
    state: &DebugCreationResultObservation,
) -> Result<()> {
    check_scheduled_progress(&prepared.source, &state.creator)?;
    if state.primary_player != prepared.primary_player {
        return Err(Error::InvalidProfile("scheduled primary player changed"));
    }
    let raw = state.creator.last_created_address;
    match state.registration {
        Some(member) if raw != 0 => {
            if member.character.address != raw || member.character.epoch != prepared.source.epoch {
                return Err(Error::InvalidProfile("scheduled result identity disagrees"));
            }
            check_new_creation_member(
                &prepared.baseline,
                &state.set,
                member.character,
                member,
                prepared.route,
            )?;
        }
        None if raw == 0 && state.set == prepared.baseline => (),
        _ => {
            return Err(Error::InvalidProfile(
                "scheduled result is not a unique admitted member",
            ));
        }
    }
    check_creation_pool_delta(
        prepared.route,
        prepared.npc_pool.as_ref(),
        state.npc_pool.as_ref(),
        state.npc_data.as_ref(),
        state.registration,
    )
}
fn poll_scheduled_with(
    prepared: &mut PreparedDebugCreation,
    mut observe: impl FnMut(&PreparedDebugCreation) -> Result<DebugCreationResultObservation>,
) -> Result<ScheduledCreationPoll> {
    validate_scheduled_ticket(prepared, prepared.base, prepared.source.epoch)?;
    let first = observe(prepared)?;
    validate_scheduled_state(prepared, &first)?;
    let second = observe(prepared)?;
    validate_scheduled_state(prepared, &second)?;
    if first != second {
        return Err(Error::InvalidProfile("scheduled whole result changed"));
    }
    if first.registration.is_none() {
        return Ok(ScheduledCreationPoll::Pending(Box::new(first)));
    }
    prepared.stage = DebugCreationAttemptStage::ScheduledResultObserved;
    Ok(ScheduledCreationPoll::Registered(Box::new(
        ScheduledCreationResult {
            source: prepared.source.clone(),
            route: prepared.route,
            base: prepared.base,
            file_sha256: prepared.file_sha256,
            state: first,
            _thread: PhantomData,
        },
    )))
}
fn validate_scheduled_result_ticket(
    prepared: &PreparedDebugCreation,
    result: &ScheduledCreationResult,
    base: u64,
    epoch: u64,
) -> Result<()> {
    if prepared.stage != DebugCreationAttemptStage::ScheduledResultObserved
        || epoch == 0
        || prepared.source.epoch != epoch
        || prepared.base != base
        || prepared.file_sha256 != WW_171.sha256
        || result.file_sha256 != prepared.file_sha256
        || result.base != base
        || result.route != prepared.route
        || result.source != prepared.source
        || result.state.registration.is_none()
        || OrdinaryCreationRoute::from_bytes(prepared.source.input.kind_bytes)? != prepared.route
    {
        return Err(Error::NativeOperationRejected(
            "foreign or consumed scheduled result",
        ));
    }
    validate_schedule_source(&prepared.source)?;
    validate_creation_pool_baseline(prepared.route, prepared.npc_pool.as_ref(), base, epoch)?;
    validate_scheduled_state(prepared, &result.state)
}
fn validate_scheduled_clear_state(
    prepared: &PreparedDebugCreation,
    result: &ScheduledCreationResult,
    current: &DebugCreationResultObservation,
) -> Result<()> {
    validate_scheduled_state(prepared, current)?;
    let mut expected = result.state.clone();
    expected.creator = current.creator.clone();
    if current != &expected
        || current.creator.last_created_address != result.state.creator.last_created_address
    {
        return Err(Error::NativeOperationRejected(
            "scheduled result association superseded",
        ));
    }
    Ok(())
}

fn result_clear_source(
    prepared: &PreparedDebugCreation,
    call: &NativeRegisteredCreationCall,
    base: u64,
    epoch: u64,
) -> Result<DebugCreatorObservation> {
    if prepared.stage != DebugCreationAttemptStage::Returned
        || epoch == 0
        || prepared.base != base
        || prepared.file_sha256 != WW_171.sha256
        || prepared.source.epoch != epoch
        || call.call.source != prepared.source
        || OrdinaryCreationRoute::from_bytes(prepared.source.input.kind_bytes)? != prepared.route
    {
        return Err(Error::NativeOperationRejected(
            "foreign or consumed result ticket",
        ));
    }
    validate_ready_single(&prepared.source)?;
    schedule_preflight(Some(&prepared.source), &prepared.source, epoch)?;
    validate_creation_pool_baseline(prepared.route, prepared.npc_pool.as_ref(), base, epoch)?;
    let raw = *call
        .call
        .raw_result
        .as_ref()
        .map_err(|_| Error::NativeOperationRejected("creation result was not observed"))?;
    let post = call
        .call
        .postflight
        .as_ref()
        .map_err(|_| Error::NativeOperationRejected("creation postflight was not admitted"))?;
    check_postflight(&prepared.source, post)?;
    let member = *call
        .registration
        .as_ref()
        .map_err(|_| Error::NativeOperationRejected("creation registration was not admitted"))?;
    let pool = call
        .npc_pool_after
        .as_ref()
        .map_err(|_| Error::NativeOperationRejected("creation pool was not observed"))?;
    let data = call
        .npc_data
        .as_ref()
        .map_err(|_| Error::NativeOperationRejected("creation data was not admitted"))?;
    let primary = call
        .primary_player_after
        .as_ref()
        .map_err(|_| Error::NativeOperationRejected("creation primary was not observed"))?;
    if post.last_created_address != raw
        || member.map_or(0, |v| v.character.address) != raw
        || member.is_some_and(|v| {
            v.character.epoch != epoch || v.character.world_address != prepared.source.world.address
        })
        || primary != &prepared.primary_player
        || prepared.source.input.native_argument != 0
    {
        return Err(Error::NativeOperationRejected(
            "creation result receipt does not match ticket",
        ));
    }
    check_creation_pool_delta(
        prepared.route,
        prepared.npc_pool.as_ref(),
        pool.as_ref(),
        data.as_ref(),
        member,
    )?;
    result_clear_address(post)?;
    Ok(post.clone())
}
fn result_clear_address(source: &DebugCreatorObservation) -> Result<usize> {
    if source.object.address == 0 || !source.object.address.is_multiple_of(16) {
        return Err(Error::InvalidProfile("unaligned creator result storage"));
    }
    let address = source
        .object
        .address
        .checked_add(u64::from(RESULT_OFFSET))
        .ok_or(Error::AddressOverflow)?;
    address.checked_add(8).ok_or(Error::AddressOverflow)?;
    usize::try_from(address).map_err(|_| Error::AddressOverflow)
}
fn read_creation_result_components<M: AddressSpace>(
    objects: &crate::objects::GameObjectCatalog<'_, '_, M>,
    prepared: &PreparedDebugCreation,
    target: Option<CharacterIdentity>,
) -> Result<DebugCreationResultObservation> {
    let creator = objects
        .debug_creator(prepared.source.epoch)?
        .ok_or(Error::InvalidProfile("creation result source unavailable"))?;
    let set = objects
        .world_creation_set(prepared.limit)?
        .ok_or(Error::InvalidProfile("creation result set unavailable"))?;
    let registration = match target {
        Some(character) => {
            objects.character_registration(character, set.set.address, prepared.limit)?
        }
        None => None,
    };
    let npc_data = match (prepared.route, target) {
        (OrdinaryCreationRoute::Player, Some(character)) => {
            objects.npc_player_data_for_character(character)?
        }
        _ => None,
    };
    let npc_pool = objects.npc_player_data_pool(prepared.source.epoch)?;
    let primary_player = objects.primary_player_identity(prepared.source.epoch)?;
    Ok(DebugCreationResultObservation {
        creator,
        set,
        registration,
        npc_pool,
        npc_data,
        primary_player,
    })
}
fn observe_result_state<M: AddressSpace>(
    objects: &crate::objects::GameObjectCatalog<'_, '_, M>,
    prepared: &PreparedDebugCreation,
    call: &NativeRegisteredCreationCall,
    expected: &DebugCreatorObservation,
) -> Result<DebugCreationResultObservation> {
    let target = *call
        .registration
        .as_ref()
        .map_err(|_| Error::InvalidProfile("creation member not admitted"))?;
    let state = read_creation_result_components(objects, prepared, target.map(|v| v.character))?;
    validate_result_state(prepared, call, expected, &state)?;
    Ok(state)
}

fn validate_result_state(
    prepared: &PreparedDebugCreation,
    call: &NativeRegisteredCreationCall,
    expected: &DebugCreatorObservation,
    state: &DebugCreationResultObservation,
) -> Result<()> {
    if &state.creator != expected
        || call.registration.as_ref().ok() != Some(&state.registration)
        || call.npc_pool_after.as_ref().ok() != Some(&state.npc_pool)
        || call.npc_data.as_ref().ok() != Some(&state.npc_data)
        || state.primary_player != prepared.primary_player
    {
        return Err(Error::NativeOperationRejected(
            "creation result source or association superseded",
        ));
    }
    match state.registration {
        Some(member) => check_new_creation_member(
            &prepared.baseline,
            &state.set,
            member.character,
            member,
            prepared.route,
        )?,
        None if state.set != prepared.baseline => {
            return Err(Error::NativeOperationRejected(
                "zero result with changed creation set",
            ));
        }
        None => (),
    }
    check_creation_pool_delta(
        prepared.route,
        prepared.npc_pool.as_ref(),
        state.npc_pool.as_ref(),
        state.npc_data.as_ref(),
        state.registration,
    )
}
fn commit_result_clear(
    prepared: &mut PreparedDebugCreation,
    before: DebugCreationResultObservation,
    write: impl FnOnce(),
    observe: impl FnOnce(&PreparedDebugCreation) -> Result<DebugCreationResultObservation>,
) -> DebugCreationResultClear {
    let changed = before.creator.last_created_address != 0;
    prepared.stage = DebugCreationAttemptStage::ResultClearAttempted;
    if changed {
        write();
    }
    let mut planned = before.clone();
    planned.creator.last_created_address = 0;
    let postflight = observe(prepared).and_then(|after| {
        if after != planned {
            return Err(Error::InvalidProfile(
                "result clearing changed unrelated creation state",
            ));
        }
        Ok(after)
    });
    if postflight.is_ok() {
        prepared.stage = DebugCreationAttemptStage::ResultCleared;
    }
    DebugCreationResultClear {
        before,
        changed,
        postflight,
    }
}

fn validate_prepared_ticket(prepared: &PreparedDebugCreation, base: u64, epoch: u64) -> Result<()> {
    if prepared.stage != DebugCreationAttemptStage::Prepared
        || prepared.base != base
        || prepared.file_sha256 != WW_171.sha256
        || prepared.source.epoch != epoch
        || OrdinaryCreationRoute::from_bytes(prepared.source.input.kind_bytes)? != prepared.route
    {
        return Err(Error::NativeOperationRejected(
            "foreign or already attempted creation ticket",
        ));
    }
    validate_creation_pool_baseline(prepared.route, prepared.npc_pool.as_ref(), base, epoch)
}

fn validate_creation_pool_baseline(
    route: OrdinaryCreationRoute,
    pool: Option<&NpcPlayerDataPoolObservation>,
    base: u64,
    epoch: u64,
) -> Result<()> {
    if pool.is_some_and(|p| p.module_base != base || p.epoch != epoch) {
        return Err(Error::NativeOperationRejected(
            "foreign creation NPC pool baseline",
        ));
    }
    if route == OrdinaryCreationRoute::Player
        && pool.is_none_or(|p| p.slots.iter().all(Option::is_some))
    {
        return Err(Error::NativeOperationRejected(
            "Player creation requires a free NPC data slot",
        ));
    }
    Ok(())
}

fn check_creation_pool_delta(
    route: OrdinaryCreationRoute,
    before: Option<&NpcPlayerDataPoolObservation>,
    after: Option<&NpcPlayerDataPoolObservation>,
    associated: Option<&PlayerNpcDataObservation>,
    member: Option<CharacterRegistrationObservation>,
) -> Result<()> {
    if route == OrdinaryCreationRoute::Enemy || member.is_none() {
        if before != after || associated.is_some() {
            return Err(Error::InvalidProfile(
                "non-Player or zero creation changed NPC data pool",
            ));
        }
        return Ok(());
    }
    let before = before.ok_or(Error::InvalidProfile(
        "Player creation lacked NPC pool baseline",
    ))?;
    let after = after.ok_or(Error::InvalidProfile("Player creation lost NPC data pool"))?;
    let associated = associated.ok_or(Error::InvalidProfile(
        "Player creation missing NPC data association",
    ))?;
    let member = member.expect("checked nonzero member");
    let index = before
        .slots
        .iter()
        .position(Option::is_none)
        .ok_or(Error::InvalidProfile("Player creation pool was full"))?;
    let added = associated.slot;
    if after.module_base != before.module_base
        || after.manager_address != before.manager_address
        || after.slots_address != before.slots_address
        || after.epoch != before.epoch
        || associated.pool != *after
        || associated.character != member.character
        || added.index as usize != index
        || added.data_address == 0
        || !added.data_address.is_multiple_of(8)
        || after.slots[index] != Some(added)
        || before
            .occupied()
            .any(|v| v.data_address == added.data_address)
        || before
            .slots
            .iter()
            .zip(&after.slots)
            .enumerate()
            .any(|(i, (old, new))| i != index && old != new)
    {
        return Err(Error::InvalidProfile(
            "Player creation did not add exactly its first-free NPC data slot",
        ));
    }
    Ok(())
}

pub(crate) fn schedule_preflight(
    current: Option<&DebugCreatorObservation>,
    expected: &DebugCreatorObservation,
    epoch: u64,
) -> Result<()> {
    if epoch == 0
        || expected.epoch != epoch
        || expected.object.kind != crate::objects::ObjectKind::DebugCharacterCreator
        || expected.object.address == 0
        || !expected.object.address.is_multiple_of(16)
        || expected.world.kind != crate::objects::ObjectKind::WorldCharacters
        || expected.world.address == 0
        || !expected.world.address.is_multiple_of(8)
        || matches!(
            DebugCreatorStep::from_raw(expected.schedule.current_step),
            DebugCreatorStep::Other(_)
        )
        || matches!(
            DebugCreatorStep::from_raw(expected.schedule.next_step),
            DebugCreatorStep::Other(_)
        )
    {
        return Err(Error::InvalidProfile("invalid creator scheduling source"));
    }
    expected
        .object
        .address
        .checked_add(0x49)
        .ok_or(Error::AddressOverflow)?;
    if current != Some(expected) {
        return Err(Error::NativeOperationRejected(
            "creator scheduling source superseded",
        ));
    }
    Ok(())
}
fn check_new_creation_member(
    before: &WorldCreationSetObservation,
    after: &WorldCreationSetObservation,
    target: CharacterIdentity,
    registration: CharacterRegistrationObservation,
    route: OrdinaryCreationRoute,
) -> Result<()> {
    if before.world_address == 0
        || after.world_address != before.world_address
        || target.world_address != before.world_address
        || target.epoch == 0
        || target.address == 0
        || !target.address.is_multiple_of(16)
        || after.set.address != before.set.address
        || after.set.capacity < before.set.capacity
        || after.set.characters.len()
            != before
                .set
                .characters
                .len()
                .checked_add(1)
                .ok_or(Error::AddressOverflow)?
        || before
            .set
            .characters
            .iter()
            .any(|v| v.character.address == target.address)
        || registration.character != target
        || registration.set_address != after.set.address
    {
        return Err(Error::InvalidProfile(
            "native creation result is not a unique new set member",
        ));
    }
    for previous in &before.set.characters {
        if !after.set.characters.iter().any(|v| {
            v.ordinal == previous.ordinal
                && v.character == previous.character
                && v.native_identifier == previous.native_identifier
        }) {
            return Err(Error::InvalidProfile("preexisting creation member changed"));
        }
    }
    let added = after
        .set
        .characters
        .iter()
        .find(|v| v.character.address == target.address)
        .ok_or(Error::InvalidProfile(
            "creation result absent from after set",
        ))?;
    if added.character.kind != route.kind()
        || added.native_identifier != target.native_identifier
        || added.entry_address != registration.entry_address
        || added.ordinal != registration.ordinal
    {
        return Err(Error::InvalidProfile(
            "native result and reciprocal creation entry disagree",
        ));
    }
    Ok(())
}
fn selected_step_raw(step: DebugCreatorStep) -> Result<i32> {
    match step {
        DebugCreatorStep::Idle => Ok(0),
        DebugCreatorStep::Load => Ok(1),
        DebugCreatorStep::LoadWait => Ok(2),
        DebugCreatorStep::Other(_) => Err(Error::NativeOperationRejected(
            "unknown creator descriptor selection",
        )),
    }
}
type SelectNextStep = unsafe extern "system" fn(u64, i32);
unsafe fn invoke_next_step(
    function: SelectNextStep,
    source: DebugCreatorObservation,
    selected_step: DebugCreatorStep,
    raw_step: i32,
    observe: impl FnOnce() -> Result<DebugCreatorObservation>,
) -> NativeCreatorStepCall {
    // SAFETY: Known-state dispatch and complete stable caller-owned scheduler/request graph.
    unsafe { function(source.object.address, raw_step) };
    let mut planned = source.clone();
    planned.schedule.next_step = raw_step;
    NativeCreatorStepCall {
        source,
        selected_step,
        postflight: observe().and_then(|after| {
            if after != planned {
                return Err(Error::InvalidProfile("creator step postflight changed"));
            }
            Ok(after)
        }),
    }
}
type ChangeAdvance = unsafe extern "system" fn(u64);
unsafe fn invoke_advance(
    function: ChangeAdvance,
    source: DebugCreatorObservation,
    operation: CreatorAdvanceOperation,
    observe: impl FnOnce() -> Result<DebugCreatorObservation>,
) -> NativeCreatorAdvanceCall {
    // SAFETY: Complete native contract and stable caller-owned creator/scheduler graph.
    unsafe { function(source.object.address) };
    let mut planned = source.clone();
    planned.schedule.advance_byte = operation.value();
    NativeCreatorAdvanceCall {
        source,
        operation,
        postflight: observe().and_then(|after| {
            if after != planned {
                return Err(Error::InvalidProfile("creator advance postflight changed"));
            }
            Ok(after)
        }),
    }
}
fn validate_ready_single(source: &DebugCreatorObservation) -> Result<()> {
    if source.count != 1 || source.last_created_address != 0 {
        return Err(Error::NativeOperationRejected(
            "creation requires single count and empty result",
        ));
    }
    if source.input.name_units.is_empty()
        || source.input.name_units.len() > 31
        || source.input.name_units.contains(&0)
        || String::from_utf16(&source.input.name_units).is_err()
    {
        return Err(Error::NativeOperationRejected(
            "invalid prepared creation name",
        ));
    }
    if source
        .input
        .position_bits
        .into_iter()
        .chain(source.input.rotation_bits)
        .any(|bits| !f32::from_bits(bits).is_finite())
    {
        return Err(Error::NativeOperationRejected(
            "nonfinite prepared creation transform",
        ));
    }
    Ok(())
}

fn check_postflight(
    source: &DebugCreatorObservation,
    observed: &DebugCreatorObservation,
) -> Result<()> {
    if source.world != observed.world
        || source.object != observed.object
        || source.epoch != observed.epoch
        || source.input != observed.input
        || source.count != observed.count
        || source.animation_id != observed.animation_id
        || source.schedule.current_step != observed.schedule.current_step
        || observed.schedule.next_step != 0
        || source.schedule.advance_byte != observed.schedule.advance_byte
        || observed.phase_byte != 0
    {
        return Err(Error::InvalidProfile(
            "native creation storage/input changed unexpectedly",
        ));
    }
    Ok(())
}
pub(crate) fn validate_name_assignment(
    source: &DebugCreatorObservation,
    name: &[u16],
) -> Result<()> {
    if source.count != 1 || source.last_created_address != 0 {
        return Err(Error::NativeOperationRejected(
            "name assignment requires single idle-owned input and empty result",
        ));
    }
    if name.is_empty()
        || name.len() > 31
        || name.contains(&0)
        || String::from_utf16(name).is_err()
        || name.len() as u64 > source.input.name_capacity
        || !(7..=31).contains(&source.input.name_capacity)
    {
        return Err(Error::NativeOperationRejected(
            "invalid or insufficient-capacity creation name",
        ));
    }
    Ok(())
}
fn check_name_postflight(
    before: &DebugCreatorObservation,
    name: &[u16],
    after: &DebugCreatorObservation,
) -> Result<()> {
    let mut expected = before.clone();
    expected.input.name_units = name.to_vec();
    if *after != expected {
        return Err(Error::InvalidProfile(
            "name assignment storage/input changed",
        ));
    }
    Ok(())
}
type AssignName = unsafe extern "system" fn(u64, *const u16, u64) -> u64;
unsafe fn invoke_name(
    function: AssignName,
    destination: u64,
    source: DebugCreatorObservation,
    name: Vec<u16>,
    postflight: impl FnOnce() -> Result<DebugCreatorObservation>,
) -> Result<NativeCreationNameAssignment> {
    let mut terminated = name.clone();
    terminated.push(0);
    let start = terminated.as_ptr() as u64;
    let end = start
        .checked_add(terminated.len() as u64 * 2)
        .ok_or(Error::AddressOverflow)?;
    let buffer = source.input.name_buffer_address;
    let extent = source
        .input
        .name_capacity
        .checked_add(1)
        .and_then(|x| x.checked_mul(2))
        .ok_or(Error::AddressOverflow)?;
    let buffer_end = buffer.checked_add(extent).ok_or(Error::AddressOverflow)?;
    if start < buffer_end && buffer < end {
        return Err(Error::NativeOperationRejected(
            "creation name source overlaps native storage",
        ));
    }
    // SAFETY: Initialized disjoint SDK-owned terminated source, bounded count; caller owns
    // initialized native string destination/copy graph and prevents concurrent access.
    let returned = unsafe { function(destination, terminated.as_ptr(), name.len() as u64) };
    let returned_destination = returned == destination;
    let postflight = postflight().and_then(|after| {
        if !returned_destination {
            return Err(Error::InvalidProfile(
                "name setter returned another destination",
            ));
        }
        Ok(after)
    });
    Ok(NativeCreationNameAssignment {
        source,
        assigned_name: name,
        returned_destination,
        postflight,
    })
}

type ExecuteCreation = unsafe extern "system" fn(u64);
unsafe fn invoke(
    function: ExecuteCreation,
    source: DebugCreatorObservation,
    before_dispatch: impl FnOnce(),
    read_result: impl FnOnce() -> Result<u64>,
    postflight: impl FnOnce() -> Result<DebugCreatorObservation>,
) -> NativeDebugCreationCall {
    before_dispatch();
    // SAFETY: Caller provides a proved entry, stable input and exclusive execution phase.
    unsafe { function(source.object.address) };
    // Both attempted observations survive independently, including a failed raw read.
    let raw_result = read_result();
    let postflight = postflight().and_then(|observed| {
        if raw_result
            .as_ref()
            .is_ok_and(|raw| *raw != observed.last_created_address)
        {
            return Err(Error::InvalidProfile(
                "native creation result changed between observations",
            ));
        }
        Ok(observed)
    });
    NativeDebugCreationCall {
        source,
        raw_result,
        postflight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        debug_creator::DebugCreationInput,
        objects::{ObjectKind, ObjectObservation},
    };
    use std::cell::Cell;
    struct Mock {
        calls: Cell<usize>,
        result: Cell<u64>,
    }
    unsafe extern "system" fn execute(address: u64) {
        // SAFETY: Tests pass the live, original Mock as the one RCX argument.
        let mock = unsafe { &*(address as *const Mock) };
        mock.calls.set(mock.calls.get() + 1);
        mock.result.set(0x1000);
    }
    fn source(mock: &Mock) -> DebugCreatorObservation {
        DebugCreatorObservation {
            world: ObjectObservation {
                address: 0x2000,
                kind: ObjectKind::WorldCharacters,
            },
            object: ObjectObservation {
                address: mock as *const Mock as u64,
                kind: ObjectKind::DebugCharacterCreator,
            },
            epoch: 9,
            schedule: crate::debug_creator::DebugCreatorSchedule {
                current_step: 0,
                next_step: 0,
                advance_byte: 0,
            },
            count: 1,
            animation_id: -1,
            phase_byte: 1,
            last_created_address: 0,
            input: DebugCreationInput {
                position_bits: [0; 4],
                rotation_bits: [0; 4],
                auxiliary20_bits: [0; 4],
                auxiliary30_bits: [0; 4],
                npc_param_id: 1000,
                npc_think_param_id: 2000,
                event_entity_id: u32::MAX,
                auxiliary_id4c: 7,
                name_units: vec![99, 48, 48, 49, 48],
                name_buffer_address: 0x3000,
                name_capacity: 7,
                native_argument: 0,
                kind_bytes: [0, 0],
                chara_init_param_id: 0,
                mode: 5,
            },
        }
    }
    #[test]
    fn direct_creation_abi_runs_once_and_preserves_result_without_claiming_ownership() {
        let mock = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        let source = source(&mock);
        validate_ready_single(&source).unwrap();
        let mut observed = source.clone();
        observed.phase_byte = 0;
        observed.last_created_address = 0x1000;
        // SAFETY: Self-authored mock only; no game entry/storage or engine allocation.
        let call = unsafe {
            invoke(
                execute,
                source.clone(),
                || {},
                || Ok(mock.result.get()),
                || Ok(observed.clone()),
            )
        };
        assert_eq!(mock.calls.get(), 1);
        assert_eq!(call.source, source);
        assert_eq!(call.raw_result.unwrap(), 0x1000);
        let observed = call.postflight.unwrap();
        check_postflight(&source, &observed).unwrap();
        assert_eq!(observed.last_created_address, 0x1000); // Never dereferenced.
    }
    #[test]
    fn attempted_creation_survives_independent_observation_failures_without_retry() {
        for (raw_failure, post_failure) in [(true, false), (false, true), (true, true)] {
            let mock = Mock {
                calls: Cell::new(0),
                result: Cell::new(0),
            };
            let source = source(&mock);
            let mut observed = source.clone();
            observed.last_created_address = 0x1000;
            // SAFETY: Owned self-authored mock entry, with deliberately failing observations.
            let call = unsafe {
                invoke(
                    execute,
                    source.clone(),
                    || {},
                    || {
                        if raw_failure {
                            Err(Error::InvalidProfile("unavailable result"))
                        } else {
                            Ok(mock.result.get())
                        }
                    },
                    || {
                        if post_failure {
                            Err(Error::InvalidProfile("unavailable creator"))
                        } else {
                            Ok(observed)
                        }
                    },
                )
            };
            assert_eq!(mock.calls.get(), 1);
            assert_eq!(call.source, source);
            assert_eq!(call.raw_result.is_err(), raw_failure);
            assert_eq!(call.postflight.is_err(), post_failure);
        }
    }
    #[test]
    fn invalid_prepared_input_and_foreign_postflight_are_rejected() {
        let mock = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        let baseline = source(&mock);
        for mode in 0..10 {
            let mut bad = baseline.clone();
            match mode {
                0 => bad.count = 0,
                1 => bad.count = 2,
                2 => bad.last_created_address = 1,
                3 => bad.input.name_units.clear(),
                4 => bad.input.name_units.push(0),
                5 => bad.input.name_units = vec![0xd800],
                6 => bad.input.position_bits[0] = f32::NAN.to_bits(),
                7 => bad.input.rotation_bits[3] = f32::INFINITY.to_bits(),
                8 => bad.count = -1,
                _ => bad.input.name_units = vec![99; 32],
            }
            assert!(validate_ready_single(&bad).is_err());
        }
        for mode in 0..11 {
            let mut foreign = baseline.clone();
            foreign.phase_byte = 0;
            match mode {
                0 => foreign.world.address += 8,
                1 => foreign.object.address += 8,
                2 => foreign.epoch += 1,
                3 => foreign.input.npc_param_id += 1,
                4 => foreign.count += 1,
                5 => foreign.animation_id += 1,
                6 => foreign.input.name_units.push(99),
                7 => foreign.phase_byte = 1,
                8 => foreign.schedule.next_step = 1,
                9 => foreign.schedule.current_step = 1,
                _ => foreign.schedule.advance_byte = 1,
            }
            assert!(check_postflight(&baseline, &foreign).is_err());
        }
        assert_eq!(mock.calls.get(), 0);
    }
    #[test]
    fn changed_result_between_raw_read_and_full_observation_retains_original_attempt() {
        let mock = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        let source = source(&mock);
        let mut observed = source.clone();
        observed.last_created_address = 0x2000;
        // SAFETY: Owned mock only; second observation simulates a visible foreign transition.
        let call = unsafe {
            invoke(
                execute,
                source,
                || {},
                || Ok(mock.result.get()),
                || Ok(observed),
            )
        };
        assert_eq!(mock.calls.get(), 1);
        assert_eq!(call.raw_result.unwrap(), 0x1000);
        assert!(call.postflight.is_err());
    }
    struct NameMock {
        calls: Cell<usize>,
        received: std::cell::RefCell<Vec<u16>>,
        wrong_return: bool,
    }
    unsafe extern "system" fn assign_name(destination: u64, text: *const u16, count: u64) -> u64 {
        // SAFETY: Tests provide an original live mock and helper-owned count+1 source.
        let mock = unsafe { &*(destination as *const NameMock) };
        // SAFETY: Helper owns a live count+1 UTF16 allocation for this synchronous call.
        let units = unsafe { std::slice::from_raw_parts(text, count as usize + 1) };
        assert_eq!(units[count as usize], 0);
        mock.calls.set(mock.calls.get() + 1);
        mock.received.replace(units[..count as usize].to_vec());
        if mock.wrong_return {
            destination + 8
        } else {
            destination
        }
    }
    #[test]
    fn consumed_name_assignment_uses_utf16_count_and_preserves_attempt_on_postfailure() {
        let creation = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        for failure in 0..3 {
            let mock = NameMock {
                calls: Cell::new(0),
                received: std::cell::RefCell::new(vec![]),
                wrong_return: failure == 1,
            };
            let mut before = source(&creation);
            before.input.name_capacity = 31;
            let name: Vec<u16> = "c0010🦊".encode_utf16().collect();
            validate_name_assignment(&before, &name).unwrap();
            let mut after = before.clone();
            after.input.name_units = name.clone();
            check_name_postflight(&before, &name, &after).unwrap();
            // SAFETY: Original local ABI mock and owned storage; no game call.
            let receipt = unsafe {
                invoke_name(
                    assign_name,
                    &mock as *const NameMock as u64,
                    before.clone(),
                    name.clone(),
                    || {
                        if failure == 2 {
                            Err(Error::InvalidProfile("lost creator"))
                        } else {
                            Ok(after)
                        }
                    },
                )
            }
            .unwrap();
            assert_eq!(mock.calls.get(), 1);
            assert_eq!(*mock.received.borrow(), name);
            assert_eq!(receipt.source, before);
            assert_eq!(receipt.assigned_name, name);
            assert_eq!(receipt.returned_destination, failure != 1);
            assert_eq!(receipt.postflight.is_err(), failure != 0);
        }
    }
    #[test]
    fn name_preparation_rejects_invalid_capacity_and_foreign_storage_without_calls() {
        let creation = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        let before = source(&creation);
        for name in [vec![], vec![0], vec![0xd800], vec![99; 8], vec![99; 32]] {
            assert!(validate_name_assignment(&before, &name).is_err());
        }
        let name = vec![99; 7];
        validate_name_assignment(&before, &name).unwrap();
        for mode in 0..4 {
            let mut bad = before.clone();
            match mode {
                0 => bad.count = 2,
                1 => bad.last_created_address = 1,
                2 => bad.input.name_capacity = 6,
                _ => bad.input.name_capacity = u64::MAX,
            }
            assert!(validate_name_assignment(&bad, &name).is_err());
        }
        for mode in 0..6 {
            let mut after = before.clone();
            after.input.name_units = name.clone();
            match mode {
                0 => after.input.name_buffer_address += 8,
                1 => after.input.name_capacity += 1,
                2 => after.phase_byte = 0,
                3 => after.last_created_address = 1,
                4 => after.input.npc_param_id += 1,
                _ => after.epoch += 1,
            }
            assert!(check_name_postflight(&before, &name, &after).is_err());
        }
        let mock = NameMock {
            calls: Cell::new(0),
            received: std::cell::RefCell::new(vec![]),
            wrong_return: false,
        };
        let mut overflow = before;
        overflow.input.name_capacity = u64::MAX;
        // SAFETY: Helper rejects arithmetic overflow before calling the owned mock.
        assert!(
            // SAFETY: Invalid extent is rejected before the local mock can execute.
            unsafe {
                invoke_name(
                    assign_name,
                    &mock as *const NameMock as u64,
                    overflow,
                    name,
                    || unreachable!(),
                )
            }
            .is_err()
        );
        assert_eq!(mock.calls.get(), 0);
    }
    #[test]
    fn prepared_words_write_only_proved_lanes_and_support_explicit_conditional_restore() {
        use crate::debug_creation_fields::{
            CreatorInputWord as F, commit_word, store_word, word_plan,
        };
        #[repr(align(16))]
        struct Storage([u32; 0x1c0 / 4]);
        let mut storage = Storage([0x12345678; 0x1c0 / 4]);
        let mock = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        let mut before = source(&mock);
        before.object.address = storage.0.as_mut_ptr() as u64;
        let mut fields: Vec<_> = (0..4)
            .flat_map(|i| {
                [
                    (F::PositionLane(i), 0xb0 + u64::from(i) * 4),
                    (F::RotationLane(i), 0xc0 + u64::from(i) * 4),
                ]
            })
            .collect();
        fields.extend([
            (F::NpcParamId, 0xf0),
            (F::NpcThinkParamId, 0xf4),
            (F::EventEntityId, 0xf8),
            (F::TalkId, 0xfc),
            (F::CharaInitParamId, 0x17c),
            (F::AnimationId, 0x1a4),
        ]);
        for (field, offset) in fields {
            let old = field.bits(&before).unwrap();
            storage.0[offset as usize / 4] = old;
            let pristine = storage.0;
            let bits = if matches!(field, F::PositionLane(_) | F::RotationLane(_)) {
                2.5f32.to_bits()
            } else {
                if old == u32::MAX {
                    u32::MAX - 1
                } else {
                    u32::MAX
                }
            };
            let plan = word_plan(Some(before.clone()), before.clone(), field, bits).unwrap();
            let observed = plan_copy(&before, field, bits);
            let receipt = commit_word(
                plan,
                |addr, bits| {
                    // SAFETY: Original aligned writable owned array, exclusive test access.
                    unsafe { store_word(addr, bits) };
                },
                || Ok(observed.clone()),
            );
            assert!(receipt.changed);
            assert_eq!(receipt.postflight.unwrap(), observed);
            let mut expected = pristine;
            expected[offset as usize / 4] = bits;
            assert_eq!(storage.0, expected);
            let restore = word_plan(Some(observed.clone()), observed, field, old).unwrap();
            let receipt = commit_word(
                restore,
                |addr, bits| {
                    // SAFETY: Same owned original test array, no game access.
                    unsafe { store_word(addr, bits) };
                },
                || Ok(before.clone()),
            );
            assert!(receipt.postflight.is_ok());
            assert_eq!(storage.0, pristine);
        }
    }
    fn plan_copy(
        before: &DebugCreatorObservation,
        field: crate::debug_creation_fields::CreatorInputWord,
        bits: u32,
    ) -> DebugCreatorObservation {
        use crate::debug_creation_fields::CreatorInputWord as F;
        let mut after = before.clone();
        match field {
            F::PositionLane(i) => after.input.position_bits[i as usize] = bits,
            F::RotationLane(i) => after.input.rotation_bits[i as usize] = bits,
            F::NpcParamId => after.input.npc_param_id = bits as i32,
            F::NpcThinkParamId => after.input.npc_think_param_id = bits as i32,
            F::EventEntityId => after.input.event_entity_id = bits,
            F::TalkId => after.input.auxiliary_id4c = bits as i32,
            F::CharaInitParamId => after.input.chara_init_param_id = bits as i32,
            F::AnimationId => after.animation_id = bits as i32,
        };
        after
    }
    #[test]
    fn creator_word_preflight_never_writes_superseded_inputs_and_postfailure_keeps_attempt() {
        use crate::debug_creation_fields::{CreatorInputWord as F, commit_word, word_plan};
        let mock = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        let mut before = source(&mock);
        before.object.address = 0x1000;
        let field = F::NpcParamId;
        for mode in 0..8 {
            let mut current = before.clone();
            match mode {
                0 => current.epoch += 1,
                1 => current.object.address += 16,
                2 => current.world.address += 8,
                3 => current.input.npc_think_param_id += 1,
                4 => current.input.name_units.clear(),
                5 => current.phase_byte = 0,
                6 => current.last_created_address = 1,
                _ => current.count = 2,
            }
            assert!(word_plan(Some(current), before.clone(), field, 3).is_err());
        }
        assert!(word_plan(None, before.clone(), field, 3).is_err());
        for f in [F::PositionLane(4), F::RotationLane(255), F::PositionLane(0)] {
            assert!(
                word_plan(Some(before.clone()), before.clone(), f, f32::NAN.to_bits()).is_err()
            );
        }
        for address in [0, 1, u64::MAX - 15] {
            let mut bad = before.clone();
            bad.object.address = address;
            assert!(word_plan(Some(bad.clone()), bad, field, 3).is_err());
        }
        let calls = Cell::new(0);
        let unchanged = word_plan(
            Some(before.clone()),
            before.clone(),
            field,
            field.bits(&before).unwrap(),
        )
        .unwrap();
        let receipt = commit_word(unchanged, |_, _| unreachable!(), || unreachable!());
        assert!(!receipt.changed);
        for failure in 0..2 {
            let plan = word_plan(Some(before.clone()), before.clone(), field, 3).unwrap();
            let receipt = commit_word(
                plan,
                |_, _| calls.set(calls.get() + 1),
                || {
                    if failure == 0 {
                        Err(Error::InvalidProfile("lost input"))
                    } else {
                        Ok(before.clone())
                    }
                },
            );
            assert!(receipt.changed);
            assert!(receipt.postflight.is_err());
            assert_eq!(receipt.source, before);
        }
        assert_eq!(calls.get(), 2);
    }
    #[test]
    fn ordinary_enemy_route_changes_two_bytes_and_keeps_failed_attempt_visible() {
        use crate::debug_creation_fields::{
            commit_enemy_route, enemy_route_plan, store_enemy_route,
        };
        #[repr(align(16))]
        struct Storage([u8; 0x1c0]);
        let mut storage = Storage([0x5a; 0x1c0]);
        let mock = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        let mut before = source(&mock);
        before.object.address = storage.0.as_mut_ptr() as u64;
        before.input.kind_bytes = [2, 1];
        for failure in 0..3 {
            storage.0[0x178..0x17a].copy_from_slice(&before.input.kind_bytes);
            let plan = enemy_route_plan(Some(before.clone()), before.clone()).unwrap();
            let mut observed = before.clone();
            observed.input.kind_bytes = [0, 0];
            if failure == 2 {
                observed.input.chara_init_param_id += 1;
            }
            let receipt = commit_enemy_route(
                plan,
                |address| {
                    // SAFETY: Original exclusive writable aligned array only, no game storage.
                    unsafe { store_enemy_route(address) };
                },
                || {
                    if failure == 1 {
                        Err(Error::InvalidProfile("lost creator"))
                    } else {
                        Ok(observed)
                    }
                },
            );
            assert!(receipt.changed);
            assert_eq!(receipt.postflight.is_err(), failure != 0);
            assert_eq!(receipt.source, before);
            assert!(
                storage.0[..0x178]
                    .iter()
                    .chain(&storage.0[0x17a..])
                    .all(|x| *x == 0x5a)
            );
            assert_eq!(&storage.0[0x178..0x17a], &[0, 0]);
        }
        let mut borrowed = before.clone();
        borrowed.input.native_argument = 0x4000;
        assert!(enemy_route_plan(Some(borrowed.clone()), borrowed).is_err());
        let mut foreign = before.clone();
        foreign.schedule.next_step = 1;
        assert!(enemy_route_plan(Some(foreign), before.clone()).is_err());
        before.input.kind_bytes = [0, 0];
        let plan = enemy_route_plan(Some(before.clone()), before).unwrap();
        assert!(!commit_enemy_route(plan, |_| unreachable!(), || unreachable!()).changed);
    }

    #[repr(C, align(16))]
    struct AdvanceMock {
        bytes: std::cell::UnsafeCell<[u8; 0x50]>,
        calls: Cell<usize>,
    }
    unsafe extern "system" fn mark_advance(address: u64) {
        // SAFETY: Original owned aligned mock; only its writable scheduling byte is changed.
        let mock = unsafe { &*(address as *const AdvanceMock) };
        mock.calls.set(mock.calls.get() + 1);
        // SAFETY: Exclusive original mock invocation; no other reference to buffer is live.
        unsafe { (*mock.bytes.get())[0x48] = 1 };
    }
    unsafe extern "system" fn clear_advance(address: u64) {
        // SAFETY: Original live owned mock, exclusive byte-changing invocation.
        let mock = unsafe { &*(address as *const AdvanceMock) };
        mock.calls.set(mock.calls.get() + 1);
        // SAFETY: Exclusive original scheduling buffer access, adjacent bytes remain untouched.
        unsafe { (*mock.bytes.get())[0x48] = 0 };
    }
    #[test]
    fn native_clear_advance_changes_only_pending_byte_and_preserves_failed_attempt() {
        let stub = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        assert_eq!(CreatorAdvanceOperation::Mark.entry(), 0x52baa0);
        assert_eq!(CreatorAdvanceOperation::Clear.entry(), 0x52a9f0);
        for current in 0..3 {
            for next in 0..3 {
                for raw in [0u8, 1, 255] {
                    let mut before = [0xaau8; 0x50];
                    before[0x48] = raw;
                    let mock = AdvanceMock {
                        bytes: std::cell::UnsafeCell::new(before),
                        calls: Cell::new(0),
                    };
                    let mut expected = source(&stub);
                    expected.object.address = &mock as *const AdvanceMock as u64;
                    expected.schedule.current_step = current;
                    expected.schedule.next_step = next;
                    expected.schedule.advance_byte = raw;
                    schedule_preflight(Some(&expected), &expected, 9).unwrap();
                    // SAFETY: Original mock entry and complete live owned array, no game calls.
                    let receipt = unsafe {
                        invoke_advance(
                            clear_advance,
                            expected.clone(),
                            CreatorAdvanceOperation::Clear,
                            || {
                                let mut after = expected.clone();
                                after.schedule.advance_byte = (*mock.bytes.get())[0x48];
                                Ok(after)
                            },
                        )
                    };
                    assert_eq!(mock.calls.get(), 1);
                    assert_eq!(receipt.source, expected);
                    assert_eq!(receipt.operation, CreatorAdvanceOperation::Clear);
                    let after = receipt.postflight.unwrap();
                    assert_eq!(after.schedule.advance_byte, 0);
                    assert_eq!(after.schedule.current_step, current);
                    assert_eq!(after.schedule.next_step, next);
                    let mut planned = before;
                    planned[0x48] = 0;
                    // SAFETY: Copy owned array after exclusive callback finished.
                    assert_eq!(unsafe { *mock.bytes.get() }, planned);
                    // SAFETY: Same original mock; reject foreign current after one clear attempt.
                    let failed = unsafe {
                        invoke_advance(
                            clear_advance,
                            after.clone(),
                            CreatorAdvanceOperation::Clear,
                            || {
                                let mut foreign = after.clone();
                                foreign.schedule.current_step = 3;
                                Ok(foreign)
                            },
                        )
                    };
                    assert_eq!(mock.calls.get(), 2);
                    assert_eq!(failed.source, after);
                    assert!(failed.postflight.is_err());
                    // SAFETY: Failure does not restore mark or mutate any other owned field.
                    assert_eq!(unsafe { *mock.bytes.get() }, planned);
                }
            }
        }
        assert_eq!(stub.calls.get(), 0);
    }
    unsafe extern "system" fn select_step(address: u64, step: i32) {
        // SAFETY: Live original owned mock; exclusive four-byte field invocation.
        let mock = unsafe { &*(address as *const AdvanceMock) };
        mock.calls.set(mock.calls.get() + 1);
        // SAFETY: Exclusive mock buffer write; adjacent scheduling/input bytes are preserved.
        unsafe { (&mut *mock.bytes.get())[0x44..0x48].copy_from_slice(&step.to_le_bytes()) };
    }
    #[test]
    fn native_next_step_abi_changes_exact_word_once_even_unchanged_and_retains_failure() {
        let stub = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        for current in 0..3 {
            for next in 0..3 {
                for selected in [
                    DebugCreatorStep::Idle,
                    DebugCreatorStep::Load,
                    DebugCreatorStep::LoadWait,
                ] {
                    let raw = selected_step_raw(selected).unwrap();
                    let mut before = [0xaau8; 0x50];
                    before[0x40..0x44].copy_from_slice(&i32::to_le_bytes(current));
                    before[0x44..0x48].copy_from_slice(&i32::to_le_bytes(next));
                    let mock = AdvanceMock {
                        bytes: std::cell::UnsafeCell::new(before),
                        calls: Cell::new(0),
                    };
                    let mut expected = source(&stub);
                    expected.object.address = &mock as *const AdvanceMock as u64;
                    expected.schedule.current_step = current;
                    expected.schedule.next_step = next;
                    expected.schedule.advance_byte = before[0x48];
                    schedule_preflight(Some(&expected), &expected, 9).unwrap();
                    // SAFETY: Original mock entry and owned live buffer; no game memory or call.
                    let receipt = unsafe {
                        invoke_next_step(select_step, expected.clone(), selected, raw, || {
                            let mut after = expected.clone();
                            after.schedule.next_step = i32::from_le_bytes(
                                (&*mock.bytes.get())[0x44..0x48].try_into().unwrap(),
                            );
                            Ok(after)
                        })
                    };
                    assert_eq!(mock.calls.get(), 1);
                    assert_eq!(receipt.source, expected);
                    assert_eq!(receipt.selected_step, selected);
                    let after = receipt.postflight.unwrap();
                    assert_eq!(after.schedule.next_step, raw);
                    let mut planned = before;
                    planned[0x44..0x48].copy_from_slice(&raw.to_le_bytes());
                    // SAFETY: Read owned array after exclusive callback finished.
                    assert_eq!(unsafe { *mock.bytes.get() }, planned);
                    // SAFETY: Same mock, failed postflight still follows exactly one attempt.
                    let failed = unsafe {
                        invoke_next_step(select_step, after.clone(), selected, raw, || {
                            Err(Error::InvalidProfile("selection observer failed"))
                        })
                    };
                    assert_eq!(mock.calls.get(), 2);
                    assert_eq!(failed.source, after);
                    assert!(failed.postflight.is_err());
                }
            }
        }
        assert_eq!(stub.calls.get(), 0);
    }
    #[test]
    fn next_step_selection_rejects_unknown_variants_and_changed_postflight() {
        for raw in [i32::MIN, -1, 0, 1, 2, 3, i32::MAX] {
            assert!(selected_step_raw(DebugCreatorStep::Other(raw)).is_err());
        }
        let stub = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        let mock = AdvanceMock {
            bytes: std::cell::UnsafeCell::new([0; 0x50]),
            calls: Cell::new(0),
        };
        let mut expected = source(&stub);
        expected.object.address = &mock as *const AdvanceMock as u64;
        for field in 0..6 {
            // SAFETY: Exclusive original mock; changes test strict full postflight, not game behavior.
            let failed = unsafe {
                invoke_next_step(
                    select_step,
                    expected.clone(),
                    DebugCreatorStep::Load,
                    1,
                    || {
                        let mut foreign = expected.clone();
                        foreign.schedule.next_step = 1;
                        match field {
                            0 => foreign.schedule.current_step = 1,
                            1 => foreign.schedule.advance_byte = 1,
                            2 => foreign.input.position_bits[0] ^= 1,
                            3 => foreign.last_created_address = 0x5000,
                            4 => foreign.world.address += 16,
                            _ => foreign.schedule.next_step = 2,
                        }
                        Ok(foreign)
                    },
                )
            };
            assert_eq!(mock.calls.get(), field + 1);
            assert_eq!(failed.source, expected);
            assert!(failed.postflight.is_err());
        }
    }
    #[test]
    fn native_advance_abi_sets_exact_byte_once_and_keeps_failed_attempt() {
        let stub = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        for raw in [0u8, 1, 255] {
            let mut before = [0xaau8; 0x50];
            before[0x48] = raw;
            let mock = AdvanceMock {
                bytes: std::cell::UnsafeCell::new(before),
                calls: Cell::new(0),
            };
            let mut expected = source(&stub);
            expected.object.address = &mock as *const AdvanceMock as u64;
            expected.schedule.advance_byte = raw;
            schedule_preflight(Some(&expected), &expected, 9).unwrap();
            // SAFETY: Original owned mock entry, live writable byte and copied observations only.
            let receipt = unsafe {
                invoke_advance(
                    mark_advance,
                    expected.clone(),
                    CreatorAdvanceOperation::Mark,
                    || {
                        let mut after = expected.clone();
                        // SAFETY: Callback finished and no writable access is live.
                        after.schedule.advance_byte = (*mock.bytes.get())[0x48];
                        Ok(after)
                    },
                )
            };
            assert_eq!(mock.calls.get(), 1);
            assert_eq!(receipt.source, expected);
            let after = receipt.postflight.unwrap();
            assert_eq!(after.schedule.advance_byte, 1);
            let mut planned = before;
            planned[0x48] = 1;
            // SAFETY: Read owned array after exclusive callback, no native/game memory.
            assert_eq!(unsafe { *mock.bytes.get() }, planned);
            // SAFETY: Same original owned mock; observer fails after one attempt.
            let failed = unsafe {
                invoke_advance(
                    mark_advance,
                    after.clone(),
                    CreatorAdvanceOperation::Mark,
                    || Err(Error::InvalidProfile("advance observer failed")),
                )
            };
            assert_eq!(mock.calls.get(), 2);
            assert_eq!(failed.source, after);
            assert!(failed.postflight.is_err());
            assert_eq!(stub.calls.get(), 0);
        }
    }
    #[test]
    fn advance_requires_fresh_whole_creator_and_known_descriptor_steps() {
        let stub = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        let mut expected = source(&stub);
        expected.object.address = 0x4000;
        assert!(schedule_preflight(None, &expected, 9).is_err());
        for current in 0..3 {
            for next in 0..3 {
                let mut known = expected.clone();
                known.schedule.current_step = current;
                known.schedule.next_step = next;
                assert!(schedule_preflight(Some(&known), &known, 9).is_ok());
            }
        }
        for field in 0..11 {
            let mut foreign = expected.clone();
            match field {
                0 => foreign.world.address += 16,
                1 => foreign.object.address += 16,
                2 => foreign.epoch += 1,
                3 => foreign.schedule.current_step = 1,
                4 => foreign.schedule.next_step = 1,
                5 => foreign.schedule.advance_byte = 1,
                6 => foreign.input.position_bits[0] ^= 1,
                7 => foreign.input.name_units[0] += 1,
                8 => foreign.animation_id += 1,
                9 => foreign.count += 1,
                _ => foreign.last_created_address = 0x5000,
            }
            assert!(schedule_preflight(Some(&foreign), &expected, 9).is_err());
        }
        for field in 0..10 {
            let mut invalid = expected.clone();
            match field {
                0 => invalid.object.address = 0,
                1 => invalid.object.address += 8,
                2 => invalid.object.address = u64::MAX - 15,
                3 => invalid.object.kind = ObjectKind::Player,
                4 => invalid.world.kind = ObjectKind::Player,
                5 => invalid.world.address = 0,
                6 => invalid.world.address += 1,
                7 => invalid.schedule.current_step = -1,
                8 => invalid.schedule.next_step = 3,
                _ => invalid.epoch = 0,
            }
            assert!(schedule_preflight(Some(&invalid), &invalid, 9).is_err());
        }
        let mock = AdvanceMock {
            bytes: std::cell::UnsafeCell::new([0; 0x50]),
            calls: Cell::new(0),
        };
        expected.object.address = &mock as *const AdvanceMock as u64;
        // SAFETY: Original owned mock; deliberately changed next-step postflight must be retained.
        let failed = unsafe {
            invoke_advance(
                mark_advance,
                expected.clone(),
                CreatorAdvanceOperation::Mark,
                || {
                    let mut foreign = expected.clone();
                    foreign.schedule.advance_byte = 1;
                    foreign.schedule.next_step = 1;
                    Ok(foreign)
                },
            )
        };
        assert_eq!(mock.calls.get(), 1);
        assert!(failed.postflight.is_err());
        assert_eq!(failed.source, expected);
    }

    fn creation_members() -> (
        WorldCreationSetObservation,
        WorldCreationSetObservation,
        CharacterIdentity,
        CharacterRegistrationObservation,
    ) {
        use crate::character_sets::{CharacterEntryObservation, CharacterSetObservation};
        let old = CharacterEntryObservation {
            ordinal: 0,
            entry_address: 0x9000,
            character: ObjectObservation {
                address: 0x4000,
                kind: ObjectKind::Enemy,
            },
            native_identifier: 0xabcdef0000000001,
        };
        let before = WorldCreationSetObservation {
            world_address: 0x2000,
            set: CharacterSetObservation {
                address: 0x12fe8,
                capacity: 3,
                entries_address: 0x9000,
                characters: vec![old],
            },
        };
        let target = CharacterIdentity {
            address: 0x5000,
            world_address: 0x2000,
            native_identifier: 0xabcdef0000000002,
            epoch: 9,
        };
        let added = CharacterEntryObservation {
            ordinal: 1,
            entry_address: 0x10010,
            character: ObjectObservation {
                address: target.address,
                kind: ObjectKind::Enemy,
            },
            native_identifier: target.native_identifier,
        };
        // Native storage expansion is permitted; copied old identities/ordinals must be preserved.
        let after = WorldCreationSetObservation {
            world_address: 0x2000,
            set: CharacterSetObservation {
                address: before.set.address,
                capacity: 6,
                entries_address: 0x10000,
                characters: vec![
                    CharacterEntryObservation {
                        entry_address: 0x10000,
                        ..old
                    },
                    added,
                ],
            },
        };
        let registration = CharacterRegistrationObservation {
            character: target,
            set_address: after.set.address,
            entry_address: added.entry_address,
            ordinal: added.ordinal,
        };
        (before, after, target, registration)
    }
    #[test]
    fn registered_creation_requires_only_one_new_result_and_preserves_existing_members() {
        let (before, after, target, registration) = creation_members();
        check_new_creation_member(
            &before,
            &after,
            target,
            registration,
            OrdinaryCreationRoute::Enemy,
        )
        .unwrap();
        for field in 0..15 {
            let mut changed = after.clone();
            let mut identity = target;
            let mut link = registration;
            match field {
                0 => changed.world_address += 8,
                1 => changed.set.address += 8,
                2 => changed.set.capacity = 2,
                3 => {
                    changed.set.characters.pop();
                }
                4 => changed.set.characters.push(changed.set.characters[1]),
                5 => changed.set.characters[0].native_identifier ^= 1 << 63,
                6 => changed.set.characters[0].ordinal = 2,
                7 => changed.set.characters[1].character.kind = ObjectKind::Player,
                8 => changed.set.characters[1].native_identifier ^= 1 << 63,
                9 => link.entry_address += 16,
                10 => link.ordinal += 1,
                11 => link.character.native_identifier ^= 1,
                12 => identity.world_address += 8,
                13 => identity.address = before.set.characters[0].character.address,
                _ => identity.epoch = 0,
            }
            assert!(
                check_new_creation_member(
                    &before,
                    &changed,
                    identity,
                    link,
                    OrdinaryCreationRoute::Enemy
                )
                .is_err(),
                "field {field}"
            );
        }
    }
    #[test]
    fn ordinary_player_route_writes_only_two_bytes_and_normalizes_without_selecting_other_subtype()
    {
        use crate::debug_creation_fields::{commit_route, route_plan, store_route};
        #[repr(align(16))]
        struct Storage([u8; 0x1c0]);
        let mut storage = Storage([0x5a; 0x1c0]);
        let mock = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        let mut before = source(&mock);
        before.object.address = storage.0.as_mut_ptr() as u64;
        before.input.kind_bytes = [255, 7];
        for failure in 0..3 {
            storage.0[0x178..0x17a].copy_from_slice(&before.input.kind_bytes);
            let plan = route_plan(
                Some(before.clone()),
                before.clone(),
                OrdinaryCreationRoute::Player,
            )
            .unwrap();
            let mut after = before.clone();
            after.input.kind_bytes = [1, 0];
            if failure == 2 {
                after.input.npc_param_id += 1;
            }
            let receipt = commit_route(
                plan,
                |address, bytes| {
                    // SAFETY: Original owned aligned writable array, no game.
                    unsafe { store_route(address, bytes) };
                },
                || {
                    if failure == 1 {
                        Err(Error::InvalidProfile("lost Player route observation"))
                    } else {
                        Ok(after)
                    }
                },
            );
            assert!(receipt.changed);
            assert_eq!(receipt.route, OrdinaryCreationRoute::Player);
            assert_eq!(receipt.postflight.is_err(), failure != 0);
            assert_eq!(&storage.0[0x178..0x17a], &[1, 0]);
            assert!(
                storage.0[..0x178]
                    .iter()
                    .chain(&storage.0[0x17a..])
                    .all(|b| *b == 0x5a)
            );
        }
        before.input.kind_bytes = [1, 0];
        let plan = route_plan(
            Some(before.clone()),
            before.clone(),
            OrdinaryCreationRoute::Player,
        )
        .unwrap();
        assert!(!commit_route(plan, |_, _| unreachable!(), || unreachable!()).changed);
        before.input.native_argument = 8;
        assert!(route_plan(Some(before.clone()), before, OrdinaryCreationRoute::Player).is_err());
        for flag in 1..=255 {
            assert_eq!(
                OrdinaryCreationRoute::from_bytes([flag, 0]).unwrap(),
                OrdinaryCreationRoute::Player
            );
        }
        for flag in 1..=255 {
            assert!(OrdinaryCreationRoute::from_bytes([0, flag]).is_err());
        }
    }
    #[test]
    fn registered_player_creation_requires_exact_player_kind_and_one_new_npc_data_allocation() {
        use crate::npc_player_data::NpcPlayerDataSlot;
        let (before, mut after, target, registration) = creation_members();
        after.set.characters[1].character.kind = ObjectKind::Player;
        check_new_creation_member(
            &before,
            &after,
            target,
            registration,
            OrdinaryCreationRoute::Player,
        )
        .unwrap();
        assert!(
            check_new_creation_member(
                &before,
                &after,
                target,
                registration,
                OrdinaryCreationRoute::Enemy
            )
            .is_err()
        );
        let mut pool = NpcPlayerDataPoolObservation {
            module_base: 0x140000000,
            manager_address: 0x6000,
            slots_address: 0x7000,
            epoch: target.epoch,
            slots: [None; 40],
        };
        let old = NpcPlayerDataSlot {
            index: 0,
            data_address: 0x8000,
            native_identifier: 0,
        };
        pool.slots[0] = Some(old);
        let added = NpcPlayerDataSlot {
            index: 1,
            data_address: 0x9000,
            native_identifier: 10000001,
        };
        let mut next = pool.clone();
        next.slots[1] = Some(added);
        let association = PlayerNpcDataObservation {
            character: target,
            pool: next.clone(),
            slot: added,
        };
        check_creation_pool_delta(
            OrdinaryCreationRoute::Player,
            Some(&pool),
            Some(&next),
            Some(&association),
            Some(registration),
        )
        .unwrap();
        for field in 0..13 {
            let mut changed = next.clone();
            let mut associated = association.clone();
            match field {
                0 => changed.module_base += 8,
                1 => changed.manager_address += 8,
                2 => changed.slots_address += 8,
                3 => changed.epoch += 1,
                4 => changed.slots[0] = None,
                5 => changed.slots[0].as_mut().unwrap().native_identifier = 1,
                6 => {
                    changed.slots[2] = Some(NpcPlayerDataSlot {
                        index: 2,
                        data_address: 0xa000,
                        native_identifier: 10000002,
                    })
                }
                7 => changed.slots[1] = None,
                8 => associated.character.native_identifier ^= 1,
                9 => associated.slot.index = 2,
                10 => {
                    associated.slot.data_address = old.data_address;
                    changed.slots[1] = Some(associated.slot);
                }
                11 => {
                    associated.slot.data_address = 0;
                    changed.slots[1] = Some(associated.slot);
                }
                _ => associated.pool.slots[0] = None,
            }
            if field != 12 {
                associated.pool = changed.clone();
            }
            assert!(
                check_creation_pool_delta(
                    OrdinaryCreationRoute::Player,
                    Some(&pool),
                    Some(&changed),
                    Some(&associated),
                    Some(registration)
                )
                .is_err(),
                "field {field}"
            );
        }
        assert!(
            check_creation_pool_delta(
                OrdinaryCreationRoute::Player,
                Some(&pool),
                None,
                Some(&association),
                Some(registration)
            )
            .is_err()
        );
        assert!(
            check_creation_pool_delta(
                OrdinaryCreationRoute::Player,
                None,
                Some(&next),
                Some(&association),
                Some(registration)
            )
            .is_err()
        );
        assert!(
            check_creation_pool_delta(
                OrdinaryCreationRoute::Player,
                Some(&pool),
                Some(&next),
                None,
                Some(registration)
            )
            .is_err()
        );
    }
    #[test]
    fn player_pool_full_missing_foreign_or_changed_zero_result_cannot_be_admitted() {
        use crate::npc_player_data::NpcPlayerDataSlot;
        let pool = NpcPlayerDataPoolObservation {
            module_base: 0x140000000,
            manager_address: 0x6000,
            slots_address: 0x7000,
            epoch: 9,
            slots: [None; 40],
        };
        validate_creation_pool_baseline(
            OrdinaryCreationRoute::Player,
            Some(&pool),
            pool.module_base,
            9,
        )
        .unwrap();
        assert!(
            validate_creation_pool_baseline(
                OrdinaryCreationRoute::Player,
                None,
                pool.module_base,
                9
            )
            .is_err()
        );
        assert!(
            validate_creation_pool_baseline(
                OrdinaryCreationRoute::Player,
                Some(&pool),
                pool.module_base,
                10
            )
            .is_err()
        );
        assert!(
            validate_creation_pool_baseline(
                OrdinaryCreationRoute::Player,
                Some(&pool),
                pool.module_base + 8,
                9
            )
            .is_err()
        );
        let mut full = pool.clone();
        for (index, slot) in full.slots.iter_mut().enumerate() {
            *slot = Some(NpcPlayerDataSlot {
                index: index as u32,
                data_address: 0x10000 + index as u64 * 0x1000,
                native_identifier: 10000000 + index as i32,
            });
        }
        assert!(
            validate_creation_pool_baseline(
                OrdinaryCreationRoute::Player,
                Some(&full),
                pool.module_base,
                9
            )
            .is_err()
        );
        let (_, _, _, registration) = creation_members();
        for route in [OrdinaryCreationRoute::Enemy, OrdinaryCreationRoute::Player] {
            check_creation_pool_delta(route, Some(&pool), Some(&pool), None, None).unwrap();
            assert!(
                check_creation_pool_delta(route, Some(&pool), Some(&full), None, None).is_err()
            );
        }
        check_creation_pool_delta(
            OrdinaryCreationRoute::Enemy,
            None,
            None,
            None,
            Some(registration),
        )
        .unwrap();
        assert!(
            check_creation_pool_delta(
                OrdinaryCreationRoute::Enemy,
                Some(&pool),
                Some(&full),
                None,
                Some(registration)
            )
            .is_err()
        );
    }
    #[repr(C, align(16))]
    struct AttemptMock {
        calls: Cell<u32>,
        stage: Cell<DebugCreationAttemptStage>,
        result: Cell<u64>,
    }
    unsafe extern "system" fn execute_attempt(address: u64) {
        // SAFETY: Original owned mock callback; no engine memory or engine allocation.
        let mock = unsafe { &*(address as *const AttemptMock) };
        assert_eq!(mock.stage.get(), DebugCreationAttemptStage::Attempted);
        mock.calls.set(mock.calls.get() + 1);
        mock.result.set(0x5000);
    }
    #[test]
    fn prepared_creation_marks_before_native_dispatch_and_cannot_retry_after_observation_failure() {
        let template = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        let mock = AttemptMock {
            calls: Cell::new(0),
            stage: Cell::new(DebugCreationAttemptStage::Prepared),
            result: Cell::new(0),
        };
        let (baseline, _, _, _) = creation_members();
        let mut source = source(&template);
        source.object.address = &mock as *const AttemptMock as u64;
        let mut prepared = PreparedDebugCreation {
            source,
            baseline,
            route: OrdinaryCreationRoute::Enemy,
            npc_pool: None,
            primary_player: None,
            base: 0x140000000,
            file_sha256: WW_171.sha256,
            limit: 16,
            stage: DebugCreationAttemptStage::Prepared,
            _thread: PhantomData,
        };
        validate_prepared_ticket(&prepared, 0x140000000, 9).unwrap();
        assert!(validate_prepared_ticket(&prepared, 0x140001000, 9).is_err());
        assert!(validate_prepared_ticket(&prepared, 0x140000000, 10).is_err());
        let original_hash = prepared.file_sha256;
        prepared.file_sha256[0] ^= 1;
        assert!(validate_prepared_ticket(&prepared, 0x140000000, 9).is_err());
        prepared.file_sha256 = original_hash;
        // SAFETY: Original owned callback and source; failed postflight must retain actual attempt.
        let receipt = unsafe {
            invoke(
                execute_attempt,
                prepared.source.clone(),
                || {
                    assert_eq!(mock.calls.get(), 0);
                    prepared.stage = DebugCreationAttemptStage::Attempted;
                    mock.stage.set(prepared.stage);
                },
                || Ok(mock.result.get()),
                || Err(Error::InvalidProfile("creation observer unavailable")),
            )
        };
        assert_eq!(prepared.stage, DebugCreationAttemptStage::Attempted);
        assert_eq!(mock.calls.get(), 1);
        assert_eq!(receipt.raw_result.unwrap(), 0x5000);
        assert!(receipt.postflight.is_err());
        assert!(validate_prepared_ticket(&prepared, 0x140000000, 9).is_err());
        prepared.stage = DebugCreationAttemptStage::Returned;
        assert!(validate_prepared_ticket(&prepared, 0x140000000, 9).is_err());
        drop(prepared);
        assert_eq!(mock.calls.get(), 1);
    }
    fn result_clear_fixture(
        zero: bool,
    ) -> (
        PreparedDebugCreation,
        NativeRegisteredCreationCall,
        DebugCreationResultObservation,
    ) {
        let mock = Mock {
            calls: Cell::new(0),
            result: Cell::new(0),
        };
        let mut source = source(&mock);
        source.object.address = 0x4000;
        let (baseline, after, target, registration) = creation_members();
        let mut post = source.clone();
        post.phase_byte = 0;
        post.last_created_address = if zero { 0 } else { target.address };
        let member = if zero { None } else { Some(registration) };
        let prepared = PreparedDebugCreation {
            source: source.clone(),
            baseline: baseline.clone(),
            route: OrdinaryCreationRoute::Enemy,
            npc_pool: None,
            primary_player: None,
            base: 0x140000000,
            file_sha256: WW_171.sha256,
            limit: 16,
            stage: DebugCreationAttemptStage::Returned,
            _thread: PhantomData,
        };
        let call = NativeRegisteredCreationCall {
            call: NativeDebugCreationCall {
                source,
                raw_result: Ok(post.last_created_address),
                postflight: Ok(post.clone()),
            },
            registration: Ok(member),
            npc_pool_after: Ok(None),
            primary_player_after: Ok(None),
            npc_data: Ok(None),
        };
        let state = DebugCreationResultObservation {
            creator: post,
            set: if zero { baseline } else { after },
            registration: member,
            npc_pool: None,
            npc_data: None,
            primary_player: None,
        };
        (prepared, call, state)
    }
    #[test]
    fn creation_result_clear_rejects_foreign_partial_and_superseded_admission_before_store() {
        for zero in [false, true] {
            let (prepared, call, state) = result_clear_fixture(zero);
            let expected = result_clear_source(&prepared, &call, prepared.base, 9).unwrap();
            validate_result_state(&prepared, &call, &expected, &state).unwrap();
            assert!(result_clear_source(&prepared, &call, prepared.base + 0x1000, 9).is_err());
            assert!(result_clear_source(&prepared, &call, prepared.base, 0).is_err());
            for mutation in 0..12 {
                let (mut ticket, mut receipt, _) = result_clear_fixture(zero);
                match mutation {
                    0 => ticket.stage = DebugCreationAttemptStage::Prepared,
                    1 => ticket.stage = DebugCreationAttemptStage::ResultClearAttempted,
                    2 => ticket.stage = DebugCreationAttemptStage::ResultCleared,
                    3 => ticket.file_sha256[0] ^= 1,
                    4 => receipt.call.source.input.event_entity_id ^= 1,
                    5 => receipt.call.raw_result = Err(Error::InvalidProfile("lost result")),
                    6 => receipt.call.postflight = Err(Error::InvalidProfile("lost postflight")),
                    7 => {
                        receipt.registration = Err(Error::InvalidProfile("unadmitted registration"))
                    }
                    8 => receipt.npc_pool_after = Err(Error::InvalidProfile("lost pool")),
                    9 => receipt.npc_data = Err(Error::InvalidProfile("unadmitted data")),
                    10 => receipt.primary_player_after = Err(Error::InvalidProfile("lost primary")),
                    _ => receipt.call.raw_result = Ok(0x6000),
                }
                assert!(
                    result_clear_source(&ticket, &receipt, prepared.base, 9).is_err(),
                    "mutation {mutation}"
                );
            }
            for mutation in 0..8 {
                let mut changed = state.clone();
                match mutation {
                    0 => changed.creator.last_created_address ^= 0x1000,
                    1 => changed.creator.input.npc_param_id += 1,
                    2 => changed.creator.epoch += 1,
                    3 => changed.set.world_address += 8,
                    4 => changed
                        .set
                        .set
                        .characters
                        .push(changed.set.set.characters[0]),
                    5 => changed.set.set.characters[0].native_identifier ^= 1 << 63,
                    6 => {
                        changed.registration = if zero {
                            Some(creation_members().3)
                        } else {
                            None
                        }
                    }
                    _ => changed.primary_player = Some(creation_members().2),
                }
                assert!(
                    validate_result_state(&prepared, &call, &expected, &changed).is_err(),
                    "state {mutation}"
                );
            }
        }
        let (_, _, state) = result_clear_fixture(false);
        let mut invalid = state.creator;
        invalid.object.address = u64::MAX - 15;
        assert!(result_clear_address(&invalid).is_err());
        invalid.object.address = 0x4008;
        assert!(result_clear_address(&invalid).is_err());
    }
    #[test]
    fn creation_result_clear_records_actual_exact_store_and_consumes_zero_or_uncertain_attempts() {
        #[repr(align(16))]
        struct Storage([u8; 0x1c0]);
        for zero in [false, true] {
            for failure in 0..3 {
                let mut storage = Storage([0x5a; 0x1c0]);
                let (mut prepared, mut call, mut before) = result_clear_fixture(zero);
                let object = storage.0.as_mut_ptr() as u64;
                prepared.source.object.address = object;
                call.call.source.object.address = object;
                call.call.postflight.as_mut().unwrap().object.address = object;
                before.creator.object.address = object;
                storage.0[0x1b0..0x1b8]
                    .copy_from_slice(&before.creator.last_created_address.to_le_bytes());
                result_clear_source(&prepared, &call, prepared.base, 9).unwrap();
                let address = result_clear_address(&before.creator).unwrap();
                let writes = Cell::new(0);
                let mut after = before.clone();
                after.creator.last_created_address = 0;
                if failure == 2 {
                    after.set.set.entries_address += 0x100;
                }
                let receipt = commit_result_clear(
                    &mut prepared,
                    before,
                    || {
                        writes.set(writes.get() + 1);
                        // SAFETY: Exact initialized owned aligned eight-byte slot; no game memory.
                        unsafe { (address as *mut u64).write(0) };
                    },
                    |ticket| {
                        assert_eq!(
                            ticket.stage,
                            DebugCreationAttemptStage::ResultClearAttempted
                        );
                        if failure == 1 {
                            Err(Error::InvalidProfile("lost clearing observation"))
                        } else {
                            Ok(after)
                        }
                    },
                );
                assert_eq!(receipt.changed, !zero);
                assert_eq!(writes.get(), usize::from(!zero));
                assert_eq!(&storage.0[0x1b0..0x1b8], &[0; 8]);
                assert!(
                    storage.0[..0x1b0]
                        .iter()
                        .chain(&storage.0[0x1b8..])
                        .all(|b| *b == 0x5a)
                );
                assert_eq!(receipt.postflight.is_ok(), failure == 0);
                assert_eq!(
                    prepared.stage,
                    if failure == 0 {
                        DebugCreationAttemptStage::ResultCleared
                    } else {
                        DebugCreationAttemptStage::ResultClearAttempted
                    }
                );
                assert!(result_clear_source(&prepared, &call, prepared.base, 9).is_err());
                drop(prepared);
                assert_eq!(writes.get(), usize::from(!zero));
            }
        }
    }
    #[test]
    fn creation_result_clear_requires_player_data_first_free_delta_and_retains_association() {
        use crate::npc_player_data::NpcPlayerDataSlot;
        let (mut prepared, mut call, mut state) = result_clear_fixture(false);
        prepared.route = OrdinaryCreationRoute::Player;
        prepared.source.input.kind_bytes = [1, 0];
        call.call.source = prepared.source.clone();
        state.creator.input.kind_bytes = [1, 0];
        state.set.set.characters[1].character.kind = ObjectKind::Player;
        let before_pool = NpcPlayerDataPoolObservation {
            module_base: prepared.base,
            manager_address: 0x6000,
            slots_address: 0x7000,
            epoch: 9,
            slots: [None; 40],
        };
        let mut after_pool = before_pool.clone();
        let slot = NpcPlayerDataSlot {
            index: 0,
            data_address: 0x8000,
            native_identifier: 10000000,
        };
        after_pool.slots[0] = Some(slot);
        let data = PlayerNpcDataObservation {
            character: state.registration.unwrap().character,
            pool: after_pool.clone(),
            slot,
        };
        prepared.npc_pool = Some(before_pool);
        state.npc_pool = Some(after_pool.clone());
        state.npc_data = Some(data.clone());
        call.call.postflight = Ok(state.creator.clone());
        call.npc_pool_after = Ok(Some(after_pool));
        call.npc_data = Ok(Some(data));
        let expected = result_clear_source(&prepared, &call, prepared.base, 9).unwrap();
        validate_result_state(&prepared, &call, &expected, &state).unwrap();
        let mut changed = state.clone();
        changed
            .npc_data
            .as_mut()
            .unwrap()
            .character
            .native_identifier ^= 1 << 63;
        assert!(validate_result_state(&prepared, &call, &expected, &changed).is_err());
        // Even a mutually changed receipt/state cannot waive first-free allocation admission.
        let data = changed.npc_data.clone();
        call.npc_data = Ok(data);
        assert!(result_clear_source(&prepared, &call, prepared.base, 9).is_err());
        let writes = Cell::new(0);
        let mut after = state.clone();
        after.creator.last_created_address = 0;
        let receipt = commit_result_clear(
            &mut prepared,
            state.clone(),
            || writes.set(writes.get() + 1),
            |_| Ok(after),
        );
        assert_eq!(writes.get(), 1);
        assert_eq!(receipt.postflight.unwrap().npc_data, state.npc_data);
    }
    #[repr(C, align(16))]
    struct ScheduleMock {
        bytes: std::cell::UnsafeCell<[u8; 0x1c0]>,
        selections: Cell<usize>,
        marks: Cell<usize>,
    }
    unsafe extern "system" fn select_load_mock(address: u64, step: i32) {
        assert_eq!(step, 1);
        // SAFETY: Tests pass their original live exclusively accessed aligned ScheduleMock.
        let mock = unsafe { &*(address as *const ScheduleMock) };
        mock.selections.set(mock.selections.get() + 1);
        // SAFETY: Original owned initialized writable four-byte array slot, no engine memory.
        unsafe { ((address + 0x44) as *mut i32).write(step) };
    }
    unsafe extern "system" fn mark_load_mock(address: u64) {
        // SAFETY: Tests pass their original live exclusively accessed aligned ScheduleMock.
        let mock = unsafe { &*(address as *const ScheduleMock) };
        mock.marks.set(mock.marks.get() + 1);
        // SAFETY: Original owned initialized writable one-byte array slot, no engine memory.
        unsafe { ((address + 0x48) as *mut u8).write(1) };
    }
    fn schedule_copy(
        source: &DebugCreatorObservation,
        mock: &ScheduleMock,
    ) -> DebugCreatorObservation {
        let mut copy = source.clone();
        // SAFETY: Original test storage; callbacks finished and no concurrent writer exists.
        let bytes = unsafe { &*mock.bytes.get() };
        copy.schedule.next_step = i32::from_le_bytes(bytes[0x44..0x48].try_into().unwrap());
        copy.schedule.advance_byte = bytes[0x48];
        copy
    }
    #[test]
    fn prepared_scheduling_retains_actual_partial_native_calls_and_only_resumes_unattempted_mark() {
        for scenario in 0..7 {
            let mut mock = ScheduleMock {
                bytes: std::cell::UnsafeCell::new([0x5a; 0x1c0]),
                selections: Cell::new(0),
                marks: Cell::new(0),
            };
            mock.bytes.get_mut()[0x44..0x48].copy_from_slice(&0i32.to_le_bytes());
            mock.bytes.get_mut()[0x48] = 0;
            let (mut ticket, _, _) = result_clear_fixture(false);
            ticket.stage = DebugCreationAttemptStage::Prepared;
            ticket.source.object.address = &mock as *const ScheduleMock as u64;
            validate_schedule_source(&ticket.source).unwrap();
            let source = ticket.source.clone();
            let result = schedule_with(
                &mut ticket,
                |before| {
                    if scenario == 0 {
                        return Err(Error::NativeOperationRejected("selection not invoked"));
                    }
                    // SAFETY: Original test callback/owned byte slots, no native game call.
                    Ok(unsafe {
                        invoke_next_step(
                            select_load_mock,
                            before.clone(),
                            DebugCreatorStep::Load,
                            1,
                            || {
                                if scenario == 1 {
                                    return Err(Error::InvalidProfile(
                                        "lost selection observation",
                                    ));
                                }
                                let mut after = schedule_copy(&before, &mock);
                                if scenario == 2 {
                                    after.input.event_entity_id ^= 1;
                                }
                                Ok(after)
                            },
                        )
                    })
                },
                |before| {
                    if scenario == 3 {
                        return Err(Error::NativeOperationRejected("mark not invoked"));
                    }
                    // SAFETY: Original test callback/owned byte slot, no native game call.
                    Ok(unsafe {
                        invoke_advance(
                            mark_load_mock,
                            before.clone(),
                            CreatorAdvanceOperation::Mark,
                            || {
                                if scenario == 4 {
                                    return Err(Error::InvalidProfile("lost mark observation"));
                                }
                                let mut after = schedule_copy(&before, &mock);
                                if scenario == 5 {
                                    after.input.npc_param_id += 1;
                                }
                                Ok(after)
                            },
                        )
                    })
                },
            );
            assert_eq!(mock.selections.get(), usize::from(scenario != 0));
            assert_eq!(mock.marks.get(), usize::from(scenario >= 4));
            if scenario == 0 {
                assert!(result.is_err());
                assert_eq!(ticket.stage, DebugCreationAttemptStage::Prepared);
                validate_prepared_ticket(&ticket, ticket.base, 9).unwrap();
                continue;
            }
            let receipt = result.unwrap();
            assert!(validate_prepared_ticket(&ticket, ticket.base, 9).is_err());
            match scenario {
                1 | 2 => {
                    assert!(receipt.selection.postflight.is_err());
                    assert!(receipt.advance.is_err());
                    assert_eq!(ticket.stage, DebugCreationAttemptStage::SchedulingUncertain);
                }
                3 => {
                    assert!(receipt.selection.postflight.is_ok());
                    assert!(receipt.advance.is_err());
                    assert_eq!(ticket.stage, DebugCreationAttemptStage::LoadSelected);
                    let selected = receipt.selection.postflight.unwrap();
                    validate_selected_ticket(&ticket, &selected, ticket.base, 9).unwrap();
                    let resumed = mark_selected_with(&mut ticket, selected, |before| {
                        // SAFETY: Only original owned mock storage/native-shaped callback.
                        Ok(unsafe {
                            invoke_advance(
                                mark_load_mock,
                                before.clone(),
                                CreatorAdvanceOperation::Mark,
                                || Ok(schedule_copy(&before, &mock)),
                            )
                        })
                    })
                    .unwrap();
                    assert!(resumed.postflight.is_ok());
                    assert_eq!(mock.selections.get(), 1);
                    assert_eq!(mock.marks.get(), 1);
                    assert_eq!(ticket.stage, DebugCreationAttemptStage::Scheduled);
                }
                4 | 5 => {
                    let advance = receipt.advance.unwrap();
                    assert!(advance.postflight.is_err());
                    assert_eq!(ticket.stage, DebugCreationAttemptStage::SchedulingUncertain);
                }
                _ => {
                    assert!(receipt.advance.unwrap().postflight.is_ok());
                    assert_eq!(ticket.stage, DebugCreationAttemptStage::Scheduled);
                }
            }
            let selected = schedule_copy(&source, &mock);
            assert!(validate_selected_ticket(&ticket, &selected, ticket.base, 9).is_err());
            drop(ticket);
            // SAFETY: Callbacks and observers completed; no concurrent access exists.
            let bytes = unsafe { &*mock.bytes.get() };
            assert!(
                bytes[..0x44]
                    .iter()
                    .chain(&bytes[0x49..])
                    .all(|b| *b == 0x5a)
            );
            assert_eq!(mock.selections.get(), 1);
        }
    }
    #[test]
    fn prepared_scheduling_requires_idle_ordinary_source_and_exact_unique_selected_ticket() {
        let (mut ticket, _, _) = result_clear_fixture(false);
        ticket.stage = DebugCreationAttemptStage::LoadSelected;
        let mut selected = ticket.source.clone();
        selected.schedule.next_step = 1;
        validate_selected_ticket(&ticket, &selected, ticket.base, 9).unwrap();
        for mutation in 0..12 {
            let (mut changed, _, _) = result_clear_fixture(false);
            changed.stage = DebugCreationAttemptStage::LoadSelected;
            let mut after = selected.clone();
            match mutation {
                0 => changed.stage = DebugCreationAttemptStage::Scheduled,
                1 => changed.stage = DebugCreationAttemptStage::SchedulingUncertain,
                2 => changed.file_sha256[0] ^= 1,
                3 => changed.base += 0x1000,
                4 => changed.source.epoch = 0,
                5 => changed.source.schedule.current_step = 1,
                6 => changed.source.schedule.next_step = 1,
                7 => changed.source.schedule.advance_byte = 1,
                8 => changed.source.input.native_argument = 8,
                9 => after.input.npc_param_id += 1,
                10 => after.schedule.current_step = 1,
                _ => after.schedule.advance_byte = 1,
            }
            assert!(
                validate_selected_ticket(&changed, &after, ticket.base, 9).is_err(),
                "mutation {mutation}"
            );
        }
        assert!(validate_selected_ticket(&ticket, &selected, ticket.base, 0).is_err());
    }
    fn dispatch_fixture(ticket: &PreparedDebugCreation) -> DebugCreatorDispatchObservation {
        let base = ticket.base;
        let mut creator = ticket.source.clone();
        creator.schedule.next_step = 1;
        creator.schedule.advance_byte = 1;
        DebugCreatorDispatchObservation {
            creator,
            descriptor_address: base + 0x3d6a468,
            descriptors: [
                0x52b540, 0x2a50b00, 0x52b5b0, 0x2a50b40, 0x52b5e0, 0x2a50b80,
            ]
            .map(|rva| base + rva),
            dependency_count: 0,
            waiting_queue: 0,
            special_mode: 0,
            scheduler_callbacks: [0x52b500, 0x52bbc0, 0x52baa0].map(|rva| base + rva),
        }
    }
    unsafe extern "system" fn scheduled_dispatch_mock(address: u64, context: u64) {
        assert_eq!(context, 0);
        // SAFETY: Live original owned test callback, not a game address.
        let mock = unsafe { &*(address as *const Mock) };
        mock.calls.set(mock.calls.get() + 1);
    }
    #[test]
    fn scheduled_dispatch_rejects_unknown_paths_and_keeps_one_attempt_through_failed_postflight() {
        for scenario in 0..4 {
            let mock = Mock {
                calls: Cell::new(0),
                result: Cell::new(0),
            };
            let (mut ticket, _, mut after) = result_clear_fixture(scenario == 0);
            ticket.source.object.address = &mock as *const Mock as u64;
            after.creator.object.address = ticket.source.object.address;
            ticket.stage = DebugCreationAttemptStage::Scheduled;
            let source = dispatch_fixture(&ticket);
            for mutation in 0..14 {
                let mut bad = source.clone();
                match mutation {
                    0 => bad.dependency_count = 1,
                    1 => bad.waiting_queue = 1,
                    2 => bad.special_mode = 1,
                    3 => bad.descriptor_address += 8,
                    4..=9 => bad.descriptors[mutation - 4] += 8,
                    10..=12 => bad.scheduler_callbacks[mutation - 10] += 8,
                    _ => bad.creator.schedule.current_step = 1,
                }
                assert!(
                    // SAFETY: Only original ABI mock, owned request, no invocation for invalid guards.
                    unsafe {
                        dispatch_scheduled_with(
                            &mut ticket,
                            bad,
                            scheduled_dispatch_mock,
                            || unreachable!(),
                        )
                    }
                    .is_err()
                );
                assert_eq!(ticket.stage, DebugCreationAttemptStage::Scheduled);
                assert_eq!(mock.calls.get(), 0);
            }
            // SAFETY: Same live original mock, validates the two-register ABI and unique attempt journal.
            let receipt = unsafe {
                dispatch_scheduled_with(
                    &mut ticket,
                    source.clone(),
                    scheduled_dispatch_mock,
                    || {
                        assert_eq!(mock.calls.get(), 1);
                        if scenario == 2 {
                            return Err(Error::InvalidProfile("lost postflight"));
                        }
                        let mut copy = after.creator.clone();
                        if scenario == 3 {
                            copy.schedule.advance_byte = 1;
                        }
                        Ok(copy)
                    },
                )
            }
            .unwrap();
            assert_eq!(receipt.source, source);
            assert_eq!(receipt.postflight.is_err(), scenario >= 2);
            assert_eq!(ticket.stage, DebugCreationAttemptStage::Dispatched);
            assert!(
                // SAFETY: Same live original mock; failed observation never permits a second call.
                unsafe {
                    dispatch_scheduled_with(
                        &mut ticket,
                        receipt.source,
                        scheduled_dispatch_mock,
                        || unreachable!(),
                    )
                }
                .is_err()
            );
            assert_eq!(mock.calls.get(), 1);
            let reconciled = poll_scheduled_with(&mut ticket, |_| Ok(after.clone())).unwrap();
            if scenario == 0 {
                assert!(matches!(reconciled, ScheduledCreationPoll::Pending(_)));
                assert_eq!(ticket.stage, DebugCreationAttemptStage::Dispatched);
            } else {
                let ScheduledCreationPoll::Registered(result) = reconciled else {
                    panic!("no result");
                };
                validate_scheduled_result_ticket(
                    &ticket,
                    &result,
                    ticket.base,
                    ticket.source.epoch,
                )
                .unwrap();
                assert_eq!(
                    ticket.stage,
                    DebugCreationAttemptStage::ScheduledResultObserved
                );
            }
            assert_eq!(mock.calls.get(), 1);
        }
    }
    #[test]
    fn scheduled_poll_keeps_zero_pending_admits_one_result_and_rejects_whole_copy_changes() {
        let (mut ticket, _, mut waiting) = result_clear_fixture(true);
        ticket.stage = DebugCreationAttemptStage::Scheduled;
        waiting.creator.phase_byte = 1;
        waiting.creator.schedule.next_step = 1;
        waiting.creator.schedule.advance_byte = 1;
        let reads = Cell::new(0);
        for _ in 0..3 {
            let result = poll_scheduled_with(&mut ticket, |_| {
                reads.set(reads.get() + 1);
                Ok(waiting.clone())
            })
            .unwrap();
            let ScheduledCreationPoll::Pending(pending) = result else {
                panic!("zero must remain pending")
            };
            assert_eq!(*pending, waiting);
            assert_eq!(ticket.stage, DebugCreationAttemptStage::Scheduled);
        }
        assert_eq!(reads.get(), 6);
        let (mut ticket, _, ready) = result_clear_fixture(false);
        ticket.stage = DebugCreationAttemptStage::Scheduled;
        let result = poll_scheduled_with(&mut ticket, |_| Ok(ready.clone())).unwrap();
        let ScheduledCreationPoll::Registered(result) = result else {
            panic!("unique result not admitted")
        };
        assert_eq!(result.observation(), &ready);
        assert_eq!(
            ticket.stage,
            DebugCreationAttemptStage::ScheduledResultObserved
        );
        assert!(poll_scheduled_with(&mut ticket, |_| unreachable!()).is_err());
        validate_scheduled_result_ticket(&ticket, &result, ticket.base, 9).unwrap();
        // Engine may progress from LoadWait's phase0 to Idle's phase1 before clearing.
        let mut progressed = ready.clone();
        progressed.creator.phase_byte = 1;
        validate_scheduled_clear_state(&ticket, &result, &progressed).unwrap();
        for mutation in 0..13 {
            let (mut pending, _, initial) = result_clear_fixture(false);
            pending.stage = DebugCreationAttemptStage::Scheduled;
            let reads = Cell::new(0);
            let rejected = poll_scheduled_with(&mut pending, |_| {
                let mut state = initial.clone();
                if reads.get() == 1 {
                    match mutation {
                        0 => state.creator.input.npc_param_id += 1,
                        1 => state.creator.last_created_address = 0,
                        2 => {
                            state
                                .registration
                                .as_mut()
                                .unwrap()
                                .character
                                .native_identifier ^= 1 << 63
                        }
                        3 => state.set.set.characters[0].native_identifier ^= 1 << 63,
                        4 => state.set.set.characters[1].ordinal += 1,
                        5 => state.set.set.characters.push(state.set.set.characters[1]),
                        6 => state.creator.schedule.current_step = -1,
                        7 => state.creator.schedule.next_step = 3,
                        8 => state.creator.schedule.advance_byte = 255,
                        9 => state.creator.phase_byte = 255,
                        10 => state.primary_player = Some(creation_members().2),
                        11 => state.creator.phase_byte = 1,
                        _ => state.set.set.entries_address += 0x100,
                    }
                }
                reads.set(reads.get() + 1);
                Ok(state)
            });
            assert!(rejected.is_err(), "late mutation {mutation}");
            assert_eq!(pending.stage, DebugCreationAttemptStage::Scheduled);
        }
        // No raw result plus a set delta is ambiguous, never reported as a failed creation.
        let (mut ticket, _, mut lost) = result_clear_fixture(false);
        ticket.stage = DebugCreationAttemptStage::Scheduled;
        lost.creator.last_created_address = 0;
        lost.registration = None;
        assert!(poll_scheduled_with(&mut ticket, |_| Ok(lost.clone())).is_err());
        assert_eq!(ticket.stage, DebugCreationAttemptStage::Scheduled);
    }
    #[test]
    fn scheduled_player_admission_and_clearing_preserve_full_data_and_unique_provenance() {
        use crate::npc_player_data::NpcPlayerDataSlot;
        let (mut ticket, _, mut ready) = result_clear_fixture(false);
        ticket.stage = DebugCreationAttemptStage::Scheduled;
        ticket.route = OrdinaryCreationRoute::Player;
        ticket.source.input.kind_bytes = [1, 0];
        ready.creator.input.kind_bytes = [1, 0];
        ready.set.set.characters[1].character.kind = ObjectKind::Player;
        let pool = NpcPlayerDataPoolObservation {
            module_base: ticket.base,
            manager_address: 0x6000,
            slots_address: 0x7000,
            epoch: 9,
            slots: [None; 40],
        };
        let mut after_pool = pool.clone();
        let slot = NpcPlayerDataSlot {
            index: 0,
            data_address: 0x8000,
            native_identifier: 10000000,
        };
        after_pool.slots[0] = Some(slot);
        ticket.npc_pool = Some(pool);
        ready.npc_pool = Some(after_pool.clone());
        ready.npc_data = Some(PlayerNpcDataObservation {
            character: ready.registration.unwrap().character,
            pool: after_pool,
            slot,
        });
        let result = poll_scheduled_with(&mut ticket, |_| Ok(ready.clone())).unwrap();
        let ScheduledCreationPoll::Registered(mut result) = result else {
            panic!("Player not admitted")
        };
        validate_scheduled_result_ticket(&ticket, &result, ticket.base, 9).unwrap();
        assert!(
            validate_scheduled_result_ticket(&ticket, &result, ticket.base + 0x1000, 9).is_err()
        );
        assert!(validate_scheduled_result_ticket(&ticket, &result, ticket.base, 0).is_err());
        result.file_sha256[0] ^= 1;
        assert!(validate_scheduled_result_ticket(&ticket, &result, ticket.base, 9).is_err());
        result.file_sha256[0] ^= 1;
        result.source.input.event_entity_id ^= 1;
        assert!(validate_scheduled_result_ticket(&ticket, &result, ticket.base, 9).is_err());
        result.source.input.event_entity_id ^= 1;
        for changed_field in 0..5 {
            let mut changed = ready.clone();
            match changed_field {
                0 => {
                    changed
                        .npc_data
                        .as_mut()
                        .unwrap()
                        .character
                        .native_identifier ^= 1 << 63
                }
                1 => {
                    changed.npc_pool.as_mut().unwrap().slots[1] = Some(NpcPlayerDataSlot {
                        index: 1,
                        data_address: 0x9000,
                        native_identifier: 10000001,
                    })
                }
                2 => changed.creator.last_created_address = 0x6000,
                3 => changed.creator.input.name_units[1] += 1,
                _ => changed.registration.as_mut().unwrap().entry_address += 16,
            }
            assert!(validate_scheduled_clear_state(&ticket, &result, &changed).is_err());
        }
        // Read failure while waiting consumes nothing; an uncertain schedule cannot be polled.
        let (mut waiting, _, _) = result_clear_fixture(true);
        waiting.stage = DebugCreationAttemptStage::Scheduled;
        assert!(
            poll_scheduled_with(&mut waiting, |_| Err(Error::InvalidProfile(
                "reader unavailable"
            )))
            .is_err()
        );
        assert_eq!(waiting.stage, DebugCreationAttemptStage::Scheduled);
        waiting.stage = DebugCreationAttemptStage::SchedulingUncertain;
        assert!(poll_scheduled_with(&mut waiting, |_| unreachable!()).is_err());
        let writes = Cell::new(0);
        let mut after = ready.clone();
        after.creator.last_created_address = 0;
        let receipt = commit_result_clear(
            &mut ticket,
            ready.clone(),
            || writes.set(writes.get() + 1),
            |_| Ok(after),
        );
        assert_eq!(writes.get(), 1);
        assert_eq!(receipt.postflight.unwrap().npc_data, ready.npc_data);
        assert!(validate_scheduled_result_ticket(&ticket, &result, ticket.base, 9).is_err());
        drop(ticket);
        drop(result);
        assert_eq!(writes.get(), 1);
    }
}
