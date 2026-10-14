//! Experimental NR module graph and engine-space rigid pose copies.
//! Reads are bounded and checked; no engine writes or native transform setters are exposed.
use crate::reader::{CHARACTER_TABLE, ENEMY_TABLE, NightreignReader, PLAYER_TABLE, ReaderCore};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleKind {
    Data,
    ActionFlags,
    BehaviorScript,
    Resist,
    Behavior,
    BehaviorSync,
    Physics,
    ActionRequests,
    BehaviorData,
}
impl ModuleKind {
    pub fn slot(self) -> usize {
        self.profile().0
    }
    pub(crate) fn profile(self) -> (usize, u32) {
        match self {
            Self::Data => (0, 0x2c10b88),
            Self::ActionFlags => (1, 0x2c0c6d0),
            Self::BehaviorScript => (2, 0x2c10068),
            Self::Resist => (4, 0x2c113c8),
            Self::Behavior => (5, 0x2c0fef0),
            Self::BehaviorSync => (6, 0x2c10138),
            Self::Physics => (13, 0x2c13720),
            Self::ActionRequests => (16, 0x2c0c808),
            Self::BehaviorData => (24, 0x2c0f278),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleSnapshot {
    pub kind: ModuleKind,
    pub character_address: u64,
    pub bag_address: u64,
    pub module_address: u64,
}

/// Values used by NR's rigid-matrix builder at RVA 0x466620.
/// Coordinates retain the engine's current frame; map/cell rebasing is not reconstructed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicsTransform {
    pub module: ModuleSnapshot,
    pub rotation_xyzw: [f32; 4],
    pub translation: [f32; 4],
}
impl PhysicsTransform {
    /// Row-vector convention; translation occupies matrix row 3. Preserves the native
    /// quaternion formula without silently normalizing copied engine values.
    pub fn rigid_matrix(&self) -> Result<[[f32; 4]; 4]> {
        let [x, y, z, w] = self.rotation_xyzw;
        if !self
            .rotation_xyzw
            .iter()
            .chain(self.translation.iter())
            .all(|v| v.is_finite())
        {
            return Err(Error::InvalidProfile("nonfinite NR transform"));
        }
        let matrix = [
            [
                1.0 - 2.0 * y * y - 2.0 * z * z,
                2.0 * x * y + 2.0 * z * w,
                2.0 * x * z - 2.0 * y * w,
                0.0,
            ],
            [
                2.0 * x * y - 2.0 * z * w,
                1.0 - 2.0 * x * x - 2.0 * z * z,
                2.0 * y * z + 2.0 * x * w,
                0.0,
            ],
            [
                2.0 * x * z + 2.0 * y * w,
                2.0 * y * z - 2.0 * x * w,
                1.0 - 2.0 * x * x - 2.0 * y * y,
                0.0,
            ],
            [
                self.translation[0],
                self.translation[1],
                self.translation[2],
                1.0,
            ],
        ];
        if !matrix.iter().flatten().all(|v| v.is_finite()) {
            return Err(Error::InvalidProfile("NR transform matrix overflow"));
        }
        Ok(matrix)
    }
    pub fn transform_point(&self, point: [f32; 3]) -> Result<[f32; 3]> {
        if !point.iter().all(|v| v.is_finite()) {
            return Err(Error::InvalidProfile("nonfinite transform point"));
        }
        let matrix = self.rigid_matrix()?;
        let value: [f32; 3] = std::array::from_fn(|column| {
            point[0] * matrix[0][column]
                + point[1] * matrix[1][column]
                + point[2] * matrix[2][column]
                + matrix[3][column]
        });
        if !value.iter().all(|v| v.is_finite()) {
            return Err(Error::InvalidProfile("NR transformed point overflow"));
        }
        Ok(value)
    }
}

impl<M: AddressSpace> NightreignReader<'_, '_, M> {
    pub fn character_module(
        &self,
        character: u64,
        kind: ModuleKind,
    ) -> Result<Option<ModuleSnapshot>> {
        self.core().character_module(character, kind)
    }
    pub fn physics_transform(&self, character: u64) -> Result<Option<PhysicsTransform>> {
        self.core().physics_transform(character)
    }
}
impl<M: AddressSpace> ReaderCore<'_, M> {
    pub(crate) fn character_module(
        &self,
        character: u64,
        kind: ModuleKind,
    ) -> Result<Option<ModuleSnapshot>> {
        let class = self.check_type(character, &[CHARACTER_TABLE, PLAYER_TABLE, ENEMY_TABLE])?;
        let bag = self.u64(character, 0x1b8)?;
        if bag == 0 {
            return Ok(None);
        }
        if !bag.is_multiple_of(8) {
            return Err(Error::InvalidProfile("unaligned NR module bag"));
        }
        let (slot, table) = kind.profile();
        let address = self.u64(bag, slot as u64 * 8)?;
        if address == 0 {
            self.unchanged_u64(character, 0x1b8, bag)?;
            return Ok(None);
        }
        self.check_type(address, &[table])?;
        if self.u64(address, 8)? != character {
            return Err(Error::InvalidProfile(
                "NR module belongs to another character",
            ));
        }
        self.unchanged_u64(bag, slot as u64 * 8, address)?;
        self.unchanged_u64(character, 0x1b8, bag)?;
        self.check_type(character, &[class])?;
        self.unchanged_u64(address, 8, character)?;
        self.check_type(address, &[table])?;
        Ok(Some(ModuleSnapshot {
            kind,
            character_address: character,
            bag_address: bag,
            module_address: address,
        }))
    }
    pub(crate) fn physics_transform(&self, character: u64) -> Result<Option<PhysicsTransform>> {
        let Some(module) = self.character_module(character, ModuleKind::Physics)? else {
            return Ok(None);
        };
        let at = module
            .module_address
            .checked_add(0x60)
            .ok_or(Error::AddressOverflow)?;
        let mut bytes = [0; 32];
        self.memory.read_exact(at, &mut bytes)?;
        let rotation_xyzw = std::array::from_fn(|i| {
            f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("checked width"))
        });
        let translation = std::array::from_fn(|i| {
            f32::from_le_bytes(
                bytes[16 + i * 4..20 + i * 4]
                    .try_into()
                    .expect("checked width"),
            )
        });
        if !rotation_xyzw
            .iter()
            .chain(translation.iter())
            .all(|v| v.is_finite())
        {
            return Err(Error::InvalidProfile("nonfinite NR physics transform"));
        }
        let mut repeated = [0; 32];
        self.memory.read_exact(at, &mut repeated)?;
        if bytes != repeated
            || self.character_module(character, ModuleKind::Physics)? != Some(module)
        {
            return Err(Error::InvalidProfile(
                "NR transform graph changed during snapshot",
            ));
        }
        Ok(Some(PhysicsTransform {
            module,
            rotation_xyzw,
            translation,
        }))
    }
}
