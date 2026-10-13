//! Copied character/map collision dimensions from an identified physics module.
use crate::{
    character_modules::{CharacterModule, CharacterModuleObservation},
    character_sets::CharacterIdentity,
    objects::GameObjectCatalog,
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const PHYSICS_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x45ef20,
        length: 0x473,
    },
    CodeRange {
        rva: 0x45ff30,
        length: 0x673,
    },
    CodeRange {
        rva: 0x4605b0,
        length: 0x192,
    },
    CodeRange {
        rva: 0xc5cb90,
        length: 0x235,
    },
    CodeRange {
        rva: 0xc5db50,
        length: 0xa6,
    },
    CodeRange {
        rva: 0xc667a0,
        length: 0x1db,
    },
    CodeRange {
        rva: 0x464af0,
        length: 0x25f,
    },
    CodeRange {
        rva: 0x464d50,
        length: 0x12b,
    },
];
pub(crate) const CHARACTER_COLLISION_OFFSET: u32 = 0x330;
pub(crate) const MAP_COLLISION_OFFSET: u32 = 0x340;
pub(crate) const WEIGHT_OFFSET: u32 = 0xc8;
const INITIAL_WEIGHT_OFFSET: u32 = 0x1c8;

/// Raw native f32 bits. Dimensions may be zero or invalid during initialization;
/// observation does not clamp values or imply collision objects are ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterPhysicsObservation {
    pub module: CharacterModuleObservation,
    pub character_collision_bits: [u32; 2],
    pub map_collision_bits: [u32; 2],
    /// Native signed-integer-to-float conversion of NPC weight at initialization.
    /// This is not the unsigned PARAM word, and may subsequently change.
    pub weight_bits: u32,
    pub initial_weight_bits: u32,
}

impl CharacterPhysicsObservation {
    /// Each pair is [height, radius], preserving native bits including NaN payloads.
    pub fn collision_dimensions(&self) -> [[f32; 2]; 2] {
        [
            self.character_collision_bits.map(f32::from_bits),
            self.map_collision_bits.map(f32::from_bits),
        ]
    }

    /// Calculate uniform dimensions from this copied baseline. Weight is unchanged.
    /// No collision rebuild, native call, transform write or compounded scaling.
    pub fn multiplied_dimensions(&self, multiplier: f32) -> Result<[[f32; 2]; 2]> {
        let original = self.collision_dimensions();
        let result = original.map(|pair| pair.map(|v| v * multiplier));
        if !multiplier.is_finite()
            || multiplier <= 0.0
            || original
                .into_iter()
                .chain(result)
                .flatten()
                .any(|v| !v.is_finite() || v <= 0.0)
        {
            return Err(Error::InvalidProfile(
                "invalid or unrepresentable collision scale",
            ));
        }
        Ok(result)
    }
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy only proved dimension and weight fields; recheck payload and full link.
    /// Missing links return None, detectable replacement or inconsistent reads fail.
    /// Does not pin the module or inspect its collision-object pointers or padding.
    pub fn character_physics(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterPhysicsObservation>> {
        let Some(module) = self.character_module(target, CharacterModule::Physics)? else {
            return Ok(None);
        };
        let read = || -> Result<[[u8; 8]; 4]> {
            let mut fields = [[0; 8]; 4];
            for ((offset, width), output) in [
                (CHARACTER_COLLISION_OFFSET, 8),
                (MAP_COLLISION_OFFSET, 8),
                (WEIGHT_OFFSET, 4),
                (INITIAL_WEIGHT_OFFSET, 4),
            ]
            .into_iter()
            .zip(&mut fields)
            {
                let address = module
                    .address
                    .checked_add(u64::from(offset))
                    .ok_or(Error::AddressOverflow)?;
                self.module
                    .memory()
                    .read_exact(address, &mut output[..width])?;
            }
            Ok(fields)
        };
        let fields = read()?;
        if read()? != fields
            || self.character_module(target, CharacterModule::Physics)? != Some(module)
        {
            return Err(Error::InvalidProfile(
                "character physics changed during observation",
            ));
        }
        let pair = |bytes: [u8; 8]| {
            std::array::from_fn(|i| {
                u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("four bytes"))
            })
        };
        Ok(Some(CharacterPhysicsObservation {
            module,
            character_collision_bits: pair(fields[0]),
            map_collision_bits: pair(fields[1]),
            weight_bits: pair(fields[2])[0],
            initial_weight_bits: pair(fields[3])[0],
        }))
    }
}
