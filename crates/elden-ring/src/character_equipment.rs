//! Experimental copied PlayerIns assembly fields; no equipment writes or engine references.
use crate::{
    character_sets::CharacterIdentity,
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const EQUIPMENT_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0x650c90,
        length: 0x85f,
    },
    CodeRange {
        rva: 0x3be1c0,
        length: 0xa1,
    },
    CodeRange {
        rva: 0x4c54c0,
        length: 0x18,
    },
    CodeRange {
        rva: 0x4c54e0,
        length: 0x4d,
    },
    // Complete dispatch instructions and its adjacent 19-entry RVA switch table.
    CodeRange {
        rva: 0x3be440,
        length: 0x124,
    },
    CodeRange {
        rva: 0x3bea40,
        length: 0x1b,
    },
    CodeRange {
        rva: 0x656270,
        length: 0xb6,
    },
    CodeRange {
        rva: 0xd563b0,
        length: 0x1c0,
    },
    // Ordinary leaf: only native styles 2 and 3 satisfy the two-hand predicate.
    CodeRange {
        rva: 0x4c55f0,
        length: 0xc,
    },
    CodeRange {
        rva: 0x4d1c80,
        length: 0xa5,
    },
];
const PLAYER_ASSEMBLY_OFFSET: u32 = 0x638;
const ARM_STYLE_OFFSET: u32 = 8;
const SELECTED_SLOTS_OFFSET: u32 = 0xc;
const PARAMETER_IDS_OFFSET: u32 = 0x7c;

/// Copied ChrAsm storage reached through the exact PlayerIns. ChrAsm has no
/// primary RTTI/vtable in this constructor; its first word is not a virtual table.
/// No storage pin or atomicity is implied. Arm-style meanings remain unvalidated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterEquipmentObservation {
    pub character: CharacterIdentity,
    pub assembly_address: u64,
    pub native_arm_style: u32,
    pub native_selected_slots: [u32; 6],
    pub parameter_ids: [i32; 22],
}

/// Native selectors -2 and -1 respectively map to even/odd weapon slots.
/// Left/right and auto-hand semantics are deliberately not asserted here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedWeaponParameters {
    pub even_slot: usize,
    pub even_id: i32,
    pub odd_slot: usize,
    pub odd_id: i32,
}

/// Storage-side selectors. Consumer policy maps its left/right names to these
/// pairs; this API does not infer those names from native filenames or RTTI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponSelectionMode {
    Even,
    Odd,
    /// Styles 0/1 use odd, 2 uses even, 3 uses odd. Future/unknown styles fail.
    AutoTwoHand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedWeaponParameter {
    pub slot: usize,
    pub id: i32,
}

impl CharacterEquipmentObservation {
    /// The independently observed two-hand predicate; unknown values are not
    /// silently accepted as a supported future arm style.
    pub fn is_two_handed(&self) -> Result<bool> {
        match self.native_arm_style {
            0 | 1 => Ok(false),
            2 | 3 => Ok(true),
            _ => Err(Error::InvalidProfile("unknown native arm style")),
        }
    }

    /// Read only the selected side's selector. An invalid unused side does not
    /// prevent an explicit side query, matching the owned consumer's policy.
    pub fn selected_weapon(&self, mode: WeaponSelectionMode) -> Result<SelectedWeaponParameter> {
        let side = match mode {
            WeaponSelectionMode::Even => 0,
            WeaponSelectionMode::Odd => 1,
            WeaponSelectionMode::AutoTwoHand => match self.native_arm_style {
                0 | 1 | 3 => 1,
                2 => 0,
                _ => return Err(Error::InvalidProfile("unknown native arm style")),
            },
        };
        let selected = self.native_selected_slots[side];
        if selected > 2 {
            return Err(Error::InvalidProfile(
                "selected weapon position exceeds three slots",
            ));
        }
        let slot = selected as usize * 2 + side;
        Ok(SelectedWeaponParameter {
            slot,
            id: self.parameter_ids[slot],
        })
    }

    /// Bound each weapon selection to the three paired weapon positions. Invalid
    /// selections fail; negative/zero equipment IDs are copied, never made valid.
    pub fn selected_weapon_parameters(&self) -> Result<SelectedWeaponParameters> {
        let [even, odd, ..] = self.native_selected_slots;
        if even > 2 || odd > 2 {
            return Err(Error::InvalidProfile(
                "selected weapon position exceeds three slots",
            ));
        }
        let even_slot = even as usize * 2;
        let odd_slot = odd as usize * 2 + 1;
        Ok(SelectedWeaponParameters {
            even_slot,
            even_id: self.parameter_ids[even_slot],
            odd_slot,
            odd_id: self.parameter_ids[odd_slot],
        })
    }
}

/// Native slot-0 weapon resolution strips the two decimal upgrade digits.
/// Zero is retained as a query ID; this does not prove a row or equipped weapon exists.
pub fn weapon_base_parameter_id(id: i32) -> Option<u32> {
    (id >= 0).then(|| (id as u32 / 100) * 100)
}

fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Copy only the arm style, six native selections and 22 parameter IDs.
    /// Non-PlayerIns is rejected. Missing world/character/assembly returns None;
    /// stale identities, truncation or detectable changes fail. Unknown handles,
    /// padding, inventory entries and assembly's first word are never read.
    pub fn character_equipment(
        &self,
        target: CharacterIdentity,
    ) -> Result<Option<CharacterEquipmentObservation>> {
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile("stale character equipment identity"));
        }
        let player = self.require(target.address, ObjectKind::Player)?;
        let memory = self.module.memory();
        let link = at(target.address, PLAYER_ASSEMBLY_OFFSET)?;
        let assembly = memory.read_u64(link)?;
        // Native constructor requests four-byte alignment, not eight.
        if !assembly.is_multiple_of(4) {
            return Err(Error::InvalidProfile("unaligned player assembly"));
        }
        let read = || -> Result<CharacterEquipmentObservation> {
            let mut selections = [0; 6];
            let mut ids = [0; 22];
            let style = memory.read_u32(at(assembly, ARM_STYLE_OFFSET)?)?;
            for (i, value) in selections.iter_mut().enumerate() {
                *value = memory.read_u32(at(assembly, SELECTED_SLOTS_OFFSET + i as u32 * 4)?)?;
            }
            for (i, value) in ids.iter_mut().enumerate() {
                *value =
                    memory.read_u32(at(assembly, PARAMETER_IDS_OFFSET + i as u32 * 4)?)? as i32;
            }
            Ok(CharacterEquipmentObservation {
                character: target,
                assembly_address: assembly,
                native_arm_style: style,
                native_selected_slots: selections,
                parameter_ids: ids,
            })
        };
        let result = if assembly == 0 {
            None
        } else {
            let fields = read()?;
            if read()? != fields {
                return Err(Error::InvalidProfile(
                    "player assembly fields changed during observation",
                ));
            }
            Some(fields)
        };
        if memory.read_u64(link)? != assembly
            || self.require(target.address, ObjectKind::Player)? != player
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile(
                "player assembly changed during observation",
            ));
        }
        Ok(result)
    }
}
