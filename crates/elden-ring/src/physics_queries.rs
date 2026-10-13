//! Copied physics query graph and experimental explicit native line-query ABI.
use crate::objects::{GameObjectCatalog, ObjectKind, ObjectObservation};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const QUERY_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0xc4f9c0,
        length: 0x440,
    },
    CodeRange {
        rva: 0xdf0c50,
        length: 0x7a1,
    },
    CodeRange {
        rva: 0xc51850,
        length: 0x1b2,
    },
    CodeRange {
        rva: 0xc70860,
        length: 0x3c2,
    },
    CodeRange {
        rva: 0x18a85d0,
        length: 0x140e,
    },
    CodeRange {
        rva: 0xc71aa0,
        length: 0x2c7,
    },
    CodeRange {
        rva: 0xc71e00,
        length: 0x252,
    },
    CodeRange {
        rva: 0x187f7d0,
        length: 0x83,
    },
    CodeRange {
        rva: 0xe12180,
        length: 0x2b,
    },
    CodeRange {
        rva: 0xc5a5e0,
        length: 0x1b5,
    },
    CodeRange {
        rva: 0xc5a9f0,
        length: 0xb3,
    },
    CodeRange {
        rva: 0xc5abe0,
        length: 0x8,
    },
    CodeRange {
        rva: 0xc5abf0,
        length: 0xb1,
    },
    CodeRange {
        rva: 0xc5a8e0,
        length: 0x109,
    },
    CodeRange {
        rva: 0xc5aab0,
        length: 0xa2,
    },
    CodeRange {
        rva: 0xc5a7a0,
        length: 0xb6,
    },
    CodeRange {
        rva: 0x1682c90,
        length: 0xb,
    },
    CodeRange {
        rva: 0x3fb990,
        length: 0x4d8,
    },
    CodeRange {
        rva: 0x3f4840,
        length: 0x13,
    },
    CodeRange {
        rva: 0x474fd0,
        length: 0x9,
    },
    CodeRange {
        rva: 0x477d30,
        length: 0x5,
    },
    CodeRange {
        rva: 0xc77200,
        length: 0x7,
    },
    CodeRange {
        rva: 0xc68700,
        length: 0x7d,
    },
    CodeRange {
        rva: 0xc75120,
        length: 0x17,
    },
    CodeRange {
        rva: 0xc75570,
        length: 0xfe,
    },
    CodeRange {
        rva: 0x474ab0,
        length: 0x9,
    },
];
const HAVOK_MANAGER_GLOBAL: u32 = 0x3d7a0d0;
const PHYSICS_WORLD_OFFSET: u32 = 0x98;
const HAVOK_WORLD_OFFSET: u32 = 8;
const SPECIAL_FILTER_OFFSET: u32 = 0xc0;
const WORLD_FILTER_OFFSET: u32 = 0x4d0;
const QUERY_DISPATCH_OFFSET: u32 = 0x4d8;
const QUERY_CONTEXT_OFFSET: u32 = 0xb00;

/// Exact typed chain with copied opaque query dependencies. A non-null pointer
/// says nothing about initialization, dispatch ABI, stability or safe query phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsQueryGraphObservation {
    pub module_base: u64,
    pub epoch: u64,
    pub manager: ObjectObservation,
    pub physics_world: ObjectObservation,
    pub havok_world: ObjectObservation,
    /// Manager +c0: used by the native character-aware ray/sweep paths.
    pub special_filter: u64,
    /// hknpWorld +4d0: used by the ordinary native line query.
    pub world_filter: u64,
    /// hknpWorld +4d8: native dispatch receiver; never dereferenced here.
    pub query_dispatch: u64,
    /// hknpWorld +b00: native query context; never dereferenced here.
    pub query_context: u64,
}

/// Finite origin and displacement, without normalizing or changing filter bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineQuery {
    pub filter: u32,
    origin_bits: [u32; 3],
    delta_bits: [u32; 3],
}

impl LineQuery {
    pub fn new(filter: u32, origin: [f32; 3], delta: [f32; 3]) -> Result<Self> {
        if origin.into_iter().chain(delta).any(|x| !x.is_finite())
            || delta.into_iter().all(|x| x == 0.0)
            || (0..3).any(|i| !(origin[i] + delta[i]).is_finite())
        {
            return Err(Error::InvalidProfile(
                "invalid or unrepresentable line query",
            ));
        }
        Ok(Self {
            filter,
            origin_bits: origin.map(f32::to_bits),
            delta_bits: delta.map(f32::to_bits),
        })
    }
    pub fn origin(&self) -> [f32; 3] {
        self.origin_bits.map(f32::from_bits)
    }
    pub fn displacement(&self) -> [f32; 3] {
        self.delta_bits.map(f32::from_bits)
    }
}

/// Actual native attempt, including its initialized output even when postflight
/// fails. A successful query is not proof that all filters exclude the character.
#[derive(Debug)]
pub struct NativeLineQueryCall {
    pub source: PhysicsQueryGraphObservation,
    pub excluded: Option<crate::character_modules::CharacterModuleObservation>,
    pub query: LineQuery,
    pub native_result: u8,
    pub position_bits: [u32; 4],
    pub postflight: Result<()>,
}

impl NativeLineQueryCall {
    /// Native AL 0 means no hit; 1 means a copied hit point. Unknown results and
    /// changed sources fail rather than masquerading as either classification.
    pub fn hit_position(&self) -> Result<Option<[f32; 3]>> {
        self.postflight.clone()?;
        match self.native_result {
            0 => Ok(None),
            1 => {
                let xyz = std::array::from_fn(|i| f32::from_bits(self.position_bits[i]));
                if xyz.into_iter().any(|x| !x.is_finite()) {
                    return Err(Error::InvalidProfile("nonfinite native line hit position"));
                }
                Ok(Some(xyz))
            }
            _ => Err(Error::InvalidProfile("unknown native line query result")),
        }
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
pub use current::CurrentLineQueries;

#[cfg(all(windows, target_arch = "x86_64"))]
mod current {
    use super::*;
    use crate::{
        WW_171, Ww171File,
        character_modules::{CharacterModule, CharacterModuleObservation},
        character_sets::CharacterIdentity,
    };
    use shikifsrs_runtime::{
        code_patches::OwnedCodePatches, memory::CurrentProcess, profile::OwnedVerifiedFile,
    };
    use std::{marker::PhantomData, rc::Rc, sync::Arc};

    #[repr(C, align(16))]
    struct Vector {
        bits: [u32; 4],
    }
    type LineEntry =
        unsafe extern "system" fn(u64, u32, *const Vector, *const Vector, *mut Vector, u64) -> u8;

    pub struct CurrentLineQueries {
        file: OwnedVerifiedFile,
        patches: Option<Arc<OwnedCodePatches>>,
        base: u64,
        epoch: u64,
        _thread: PhantomData<Rc<()>>,
    }
    impl CurrentLineQueries {
        /// # Safety
        /// Caller identifies ER and pins initialized manager/CSPhysWorld/hknpWorld,
        /// filter, query context/dispatch receiver, complete transitive query code,
        /// profiling TLS, scene/body tables and numerical globals through every call.
        /// Caller independently supplies a permitted exclusive query phase; no scene
        /// simulation, unload, reuse, teardown, Rust aliases or code changes overlap.
        /// Excluded characters require their entire collision/userdata graph too.
        /// This session acquires no engine references, locks or quiescence guarantees.
        pub unsafe fn bind(
            bytes: Arc<[u8]>,
            base: u64,
            epoch: u64,
            patches: Option<Arc<OwnedCodePatches>>,
        ) -> Result<Self> {
            if epoch == 0 {
                return Err(Error::InvalidProfile("zero line query epoch"));
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
        pub fn observe(&self) -> Result<Option<PhysicsQueryGraphObservation>> {
            Ww171File::from_verified(self.file.borrow()?)?
                .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
                .physics_query_graph(self.epoch)
        }
        pub fn observe_exclusion(
            &self,
            target: CharacterIdentity,
        ) -> Result<Option<CharacterModuleObservation>> {
            if target.epoch != self.epoch {
                return Err(Error::InvalidProfile("foreign line query character epoch"));
            }
            Ww171File::from_verified(self.file.borrow()?)?
                .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
                .character_module(target, CharacterModule::Physics)
        }
        /// # Safety
        /// Bind's complete pinned exclusive query contract holds. For exclusions,
        /// pin Chr190→container e8→physics10 (including its collision-state byte70)
        /// and all collision userdata,
        /// body/shape/material lookups. Native Snipe filtering is conditional on
        /// character collision state and skips native material categories 2/5;
        /// it is not an unconditional ignore-self or all-geometry query.
        /// No automatic retry, asynchronous query or Drop/native cleanup is introduced.
        pub unsafe fn cast_line(
            &self,
            expected: PhysicsQueryGraphObservation,
            query: LineQuery,
            excluded: Option<CharacterModuleObservation>,
        ) -> Result<NativeLineQueryCall> {
            let file = Ww171File::from_verified(self.file.borrow()?)?;
            let memory = CurrentProcess;
            let objects =
                file.bind_objects_with_patches(&memory, self.base, self.patches.as_deref())?;
            let actual_excluded = match excluded {
                Some(x) => objects.character_module(x.character, CharacterModule::Physics)?,
                None => None,
            };
            check_sources(
                expected,
                objects.physics_query_graph(self.epoch)?,
                self.base,
                self.epoch,
                excluded,
                actual_excluded,
            )?;
            let entry = objects.module.code_address(QUERY_EVIDENCE[6])?;
            // SAFETY: Complete native entry proves six-argument RCX world/EDX filter,
            // aligned R8/R9 vectors, stack output/excluded Chr, and normalized AL result.
            let function: LineEntry = unsafe { std::mem::transmute(entry as usize) };
            // SAFETY: Caller pins transitive graph and phase; helper owns initialized vectors.
            Ok(unsafe {
                invoke(function, expected, query, excluded, || {
                    let actual_excluded = match excluded {
                        Some(x) => {
                            objects.character_module(x.character, CharacterModule::Physics)?
                        }
                        None => None,
                    };
                    check_sources(
                        expected,
                        objects.physics_query_graph(self.epoch)?,
                        self.base,
                        self.epoch,
                        excluded,
                        actual_excluded,
                    )
                })
            })
        }
        /// # Safety
        /// Caller revokes old observations and supplies bind's contract for the new lifetime.
        pub unsafe fn advance_epoch(&mut self, epoch: u64) -> Result<()> {
            if epoch <= self.epoch {
                return Err(Error::InvalidProfile("line query epoch must advance"));
            }
            self.epoch = epoch;
            Ok(())
        }
    }

    fn check_sources(
        expected: PhysicsQueryGraphObservation,
        actual: Option<PhysicsQueryGraphObservation>,
        base: u64,
        epoch: u64,
        excluded: Option<CharacterModuleObservation>,
        actual_excluded: Option<CharacterModuleObservation>,
    ) -> Result<()> {
        if epoch == 0
            || expected.epoch != epoch
            || expected.module_base != base
            || actual != Some(expected)
            || excluded != actual_excluded
        {
            return Err(Error::InvalidProfile(
                "line query sources changed or belong to another session",
            ));
        }
        for (object, kind, extent) in [
            (expected.manager, ObjectKind::HavokManager, 0x218u64),
            (expected.physics_world, ObjectKind::PhysicsWorld, 0x78),
            (expected.havok_world, ObjectKind::HavokWorld, 0xb70),
        ] {
            if object.kind != kind || object.address == 0 || !object.address.is_multiple_of(8) {
                return Err(Error::InvalidProfile("invalid line query object"));
            }
            object
                .address
                .checked_add(extent)
                .ok_or(Error::AddressOverflow)?;
        }
        let pointer = expected.query_dispatch;
        if pointer == 0 || !pointer.is_multiple_of(8) {
            return Err(Error::InvalidProfile(
                "missing or unaligned native query dependency",
            ));
        }
        pointer.checked_add(8).ok_or(Error::AddressOverflow)?;
        if let Some(x) = excluded
            && (x.module != CharacterModule::Physics
                || x.character.epoch != epoch
                || [
                    x.address,
                    x.container_address,
                    x.character.address,
                    x.character.world_address,
                ]
                .into_iter()
                .any(|p| p == 0 || !p.is_multiple_of(8)))
        {
            return Err(Error::InvalidProfile("invalid native line exclusion"));
        }
        Ok(())
    }

    unsafe fn invoke(
        function: LineEntry,
        source: PhysicsQueryGraphObservation,
        query: LineQuery,
        excluded: Option<CharacterModuleObservation>,
        postflight: impl FnOnce() -> Result<()>,
    ) -> NativeLineQueryCall {
        let origin = Vector {
            bits: [
                query.origin_bits[0],
                query.origin_bits[1],
                query.origin_bits[2],
                1f32.to_bits(),
            ],
        };
        let delta = Vector {
            bits: [
                query.delta_bits[0],
                query.delta_bits[1],
                query.delta_bits[2],
                0,
            ],
        };
        let mut output = Vector { bits: [0; 4] };
        // SAFETY: Verified six-argument ABI; caller pins graph/phase. Each vector is
        // initialized/aligned sixteen bytes; output survives raw result and failed postflight.
        let native_result = unsafe {
            function(
                source.physics_world.address,
                query.filter,
                &origin,
                &delta,
                &mut output,
                excluded.map_or(0, |x| x.character.address),
            )
        };
        NativeLineQueryCall {
            source,
            excluded,
            query,
            native_result,
            position_bits: output.bits,
            postflight: postflight(),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::cell::{Cell, RefCell};
        type Arguments = (u64, u32, [u32; 4], [u32; 4], u64);
        #[derive(Default)]
        struct Mock {
            calls: Cell<usize>,
            result: Cell<u8>,
            output: Cell<[u32; 4]>,
            arguments: RefCell<Option<Arguments>>,
        }
        thread_local! {static MOCK: Mock = Mock::default();}
        unsafe extern "system" fn line_mock(
            world: u64,
            filter: u32,
            origin: *const Vector,
            delta: *const Vector,
            output: *mut Vector,
            excluded: u64,
        ) -> u8 {
            assert_eq!((origin as usize | delta as usize | output as usize) & 15, 0);
            MOCK.with(|m| {
                m.calls.set(m.calls.get() + 1);
                // SAFETY: Original owned helper passes three initialized aligned stack vectors.
                let (origin, delta) = unsafe { ((*origin).bits, (*delta).bits) };
                m.arguments
                    .replace(Some((world, filter, origin, delta, excluded)));
                if m.result.get() != 0 {
                    // SAFETY: Original owned output is a live exclusive sixteen-byte Vector.
                    unsafe {
                        (*output).bits = m.output.get();
                    }
                }
                m.result.get()
            })
        }
        fn source() -> PhysicsQueryGraphObservation {
            PhysicsQueryGraphObservation {
                module_base: 0x140000000,
                epoch: 31,
                manager: ObjectObservation {
                    address: 0x200000,
                    kind: ObjectKind::HavokManager,
                },
                physics_world: ObjectObservation {
                    address: 0x300000,
                    kind: ObjectKind::PhysicsWorld,
                },
                havok_world: ObjectObservation {
                    address: 0x400000,
                    kind: ObjectKind::HavokWorld,
                },
                special_filter: 0,
                world_filter: 3,
                query_dispatch: 0x500000,
                query_context: 7,
            }
        }
        fn exclusion() -> CharacterModuleObservation {
            CharacterModuleObservation {
                character: CharacterIdentity {
                    address: 0x600000,
                    world_address: 0x700000,
                    native_identifier: 0xff012345678900ab,
                    epoch: 31,
                },
                module: CharacterModule::Physics,
                container_address: 0x800000,
                address: 0x900000,
            }
        }
        #[test]
        fn line_query_abi_preserves_filter_aligned_vectors_exclusion_and_all_actual_returns() {
            let source = source();
            let excluded = Some(exclusion());
            let query = LineQuery::new(u32::MAX, [12., -3., 7.], [0., 0., 99.]).unwrap();
            MOCK.with(|m| {
                m.calls.set(0);
                m.result.set(1);
                m.output.set([
                    10f32.to_bits(),
                    20f32.to_bits(),
                    30f32.to_bits(),
                    0x80000000,
                ]);
                // SAFETY: Original owned six-argument ABI mock; no game/process memory access.
                let hit = unsafe { invoke(line_mock, source, query, excluded, || Ok(())) };
                assert_eq!(hit.hit_position().unwrap(), Some([10., 20., 30.]));
                assert_eq!(hit.position_bits[3], 0x80000000);
                assert_eq!(
                    *m.arguments.borrow(),
                    Some((
                        source.physics_world.address,
                        u32::MAX,
                        [
                            12f32.to_bits(),
                            (-3f32).to_bits(),
                            7f32.to_bits(),
                            1f32.to_bits()
                        ],
                        [0, 0, 99f32.to_bits(), 0],
                        exclusion().character.address
                    ))
                );
                m.result.set(0);
                // SAFETY: Same owned mock; no-hit does not write output or read stale hit bits.
                let no_hit = unsafe { invoke(line_mock, source, query, None, || Ok(())) };
                assert_eq!(no_hit.position_bits, [0; 4]);
                assert_eq!(no_hit.hit_position().unwrap(), None);
                assert_eq!(m.arguments.borrow().unwrap().4, 0);
                m.result.set(2);
                m.output.set([0x7fc12345; 4]);
                // SAFETY: Same owned mock; raw unknown result/output survive classification failure.
                let unknown = unsafe { invoke(line_mock, source, query, excluded, || Ok(())) };
                assert_eq!(unknown.native_result, 2);
                assert_eq!(unknown.position_bits, [0x7fc12345; 4]);
                assert!(unknown.hit_position().is_err());
                m.result.set(1);
                // SAFETY: Same owned mock; a nonfinite actual result remains available in receipt.
                let nonfinite = unsafe { invoke(line_mock, source, query, excluded, || Ok(())) };
                assert!(nonfinite.hit_position().is_err());
                m.output.set([1f32.to_bits(); 4]);
                // SAFETY: Same owned mock; failed postflight must not hide or retry the actual call.
                let failed = unsafe {
                    invoke(line_mock, source, query, excluded, || {
                        Err(Error::InvalidProfile("mock scene changed after query"))
                    })
                };
                assert_eq!(failed.position_bits, [1f32.to_bits(); 4]);
                assert!(failed.hit_position().is_err());
                drop((hit, no_hit, unknown, nonfinite, failed));
                assert_eq!(m.calls.get(), 5);
            });
        }
        #[test]
        fn line_query_rejects_missing_stale_foreign_graph_and_exclusion_before_any_call() {
            let source = source();
            let excluded = Some(exclusion());
            assert!(
                check_sources(
                    source,
                    Some(source),
                    source.module_base,
                    31,
                    excluded,
                    excluded
                )
                .is_ok()
            );
            assert!(
                check_sources(source, None, source.module_base, 31, excluded, excluded).is_err()
            );
            for (base, epoch) in [
                (source.module_base + 0x1000, 31),
                (source.module_base, 0),
                (source.module_base, 32),
            ] {
                assert!(
                    check_sources(source, Some(source), base, epoch, excluded, excluded).is_err()
                );
            }
            for field in 0..8 {
                let mut changed = source;
                match field {
                    0 => changed.manager.address += 8,
                    1 => changed.physics_world.address += 8,
                    2 => changed.havok_world.address += 8,
                    3 => changed.special_filter ^= 1,
                    4 => changed.world_filter ^= 1,
                    5 => changed.query_dispatch += 8,
                    6 => changed.query_context ^= 1,
                    _ => changed.epoch += 1,
                }
                assert!(
                    check_sources(
                        source,
                        Some(changed),
                        source.module_base,
                        31,
                        excluded,
                        excluded
                    )
                    .is_err()
                );
            }
            for pointer in [0, 1, u64::MAX - 7] {
                let mut bad = source;
                bad.query_dispatch = pointer;
                assert!(
                    check_sources(bad, Some(bad), source.module_base, 31, excluded, excluded)
                        .is_err()
                );
            }
            let mut bad = source;
            bad.physics_world.kind = ObjectKind::HavokManager;
            assert!(
                check_sources(bad, Some(bad), source.module_base, 31, excluded, excluded).is_err()
            );
            assert!(
                check_sources(source, Some(source), source.module_base, 31, excluded, None)
                    .is_err()
            );
            for field in 0..5 {
                let mut changed = exclusion();
                match field {
                    0 => changed.character.native_identifier ^= 1 << 63,
                    1 => changed.container_address += 8,
                    2 => changed.address += 8,
                    3 => changed.character.world_address += 8,
                    _ => changed.character.epoch += 1,
                }
                assert!(
                    check_sources(
                        source,
                        Some(source),
                        source.module_base,
                        31,
                        excluded,
                        Some(changed)
                    )
                    .is_err()
                );
            }
        }
        #[test]
        fn line_query_inputs_preserve_displacement_and_reject_nonfinite_zero_and_overflow() {
            for filter in [0, 1, 2, 4, 8, 16, 32, 64, 128, u32::MAX] {
                let query = LineQuery::new(filter, [-0., 3., -7.], [1., 2., -99.]).unwrap();
                assert_eq!(query.filter, filter);
                assert_eq!(query.origin()[0].to_bits(), 0x80000000);
                assert_eq!(query.displacement(), [1., 2., -99.]);
            }
            for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                assert!(LineQuery::new(1, [bad, 0., 0.], [1., 0., 0.]).is_err());
                assert!(LineQuery::new(1, [0.; 3], [bad, 0., 0.]).is_err());
            }
            assert!(LineQuery::new(1, [0.; 3], [-0., 0., 0.]).is_err());
            assert!(LineQuery::new(1, [f32::MAX, 0., 0.], [f32::MAX, 0., 0.]).is_err());
        }
    }
}

fn at(base: u64, offset: u32) -> Result<u64> {
    base.checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Missing any typed chain link returns None; malformed types/reads or
    /// visible replacement return an error. No naked Havok allocator is a world.
    pub fn physics_query_graph(&self, epoch: u64) -> Result<Option<PhysicsQueryGraphObservation>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero physics query epoch"));
        }
        let read = || -> Result<Option<PhysicsQueryGraphObservation>> {
            let Some(manager) = self.singleton(HAVOK_MANAGER_GLOBAL, ObjectKind::HavokManager)?
            else {
                return Ok(None);
            };
            let memory = self.module.memory();
            let world_slot = at(manager.address, PHYSICS_WORLD_OFFSET)?;
            let Some(physics_world) =
                self.require(memory.read_u64(world_slot)?, ObjectKind::PhysicsWorld)?
            else {
                return Ok(None);
            };
            let havok_slot = at(physics_world.address, HAVOK_WORLD_OFFSET)?;
            let Some(havok_world) =
                self.require(memory.read_u64(havok_slot)?, ObjectKind::HavokWorld)?
            else {
                return Ok(None);
            };
            let result = PhysicsQueryGraphObservation {
                module_base: self.module.base(),
                epoch,
                manager,
                physics_world,
                havok_world,
                special_filter: memory.read_u64(at(manager.address, SPECIAL_FILTER_OFFSET)?)?,
                world_filter: memory.read_u64(at(havok_world.address, WORLD_FILTER_OFFSET)?)?,
                query_dispatch: memory.read_u64(at(havok_world.address, QUERY_DISPATCH_OFFSET)?)?,
                query_context: memory.read_u64(at(havok_world.address, QUERY_CONTEXT_OFFSET)?)?,
            };
            if self.singleton(HAVOK_MANAGER_GLOBAL, ObjectKind::HavokManager)? != Some(manager)
                || memory.read_u64(world_slot)? != physics_world.address
                || memory.read_u64(havok_slot)? != havok_world.address
                || self.require(physics_world.address, ObjectKind::PhysicsWorld)?
                    != Some(physics_world)
                || self.require(havok_world.address, ObjectKind::HavokWorld)? != Some(havok_world)
            {
                return Err(Error::InvalidProfile(
                    "physics query chain changed during observation",
                ));
            }
            Ok(Some(result))
        };
        let result = read()?;
        if read()? != result {
            return Err(Error::InvalidProfile(
                "physics query graph changed during observation",
            ));
        }
        Ok(result)
    }
}
