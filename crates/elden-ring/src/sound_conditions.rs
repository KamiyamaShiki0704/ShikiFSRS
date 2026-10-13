//! Experimental condition copies for the owned sound-bank consumer. No audio calls.
use crate::{
    CharacterEffectsObservation,
    character_equipment::{WeaponSelectionMode, weapon_base_parameter_id},
    character_sets::CharacterIdentity,
    objects::GameObjectCatalog,
    parameter_fields::{ParameterField, ScalarValue},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponCategoryObservation {
    AssemblyMissing,
    NotEquipped {
        native_id: i32,
    },
    RowMissing {
        parameter_id: u32,
    },
    Category {
        parameter_id: u32,
        row_address: u64,
        category: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundPlayerConditions {
    pub character: CharacterIdentity,
    pub weapon: WeaponCategoryObservation,
    pub effects: Option<CharacterEffectsObservation>,
}

/// Successfully traversed set and complete effect chains, not an atomic gameplay
/// snapshot or a lifetime guard. Errors never produce an empty success snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundConditionSnapshot {
    world_address: u64,
    players: Vec<SoundPlayerConditions>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConditionBankMapping {
    pub condition_id: i32,
    pub bank_id: u32,
}

impl SoundConditionSnapshot {
    pub fn world_address(&self) -> u64 {
        self.world_address
    }
    pub fn players(&self) -> &[SoundPlayerConditions] {
        &self.players
    }

    /// Union categories and watched effect IDs across every observed player;
    /// duplicate mappings and shared banks are deduplicated. This pure result is
    /// desired policy, not permission to unload external or uncertain ownership.
    pub fn bank_targets(
        &self,
        weapon_mappings: &[ConditionBankMapping],
        effect_mappings: &[ConditionBankMapping],
    ) -> Result<BTreeSet<u32>> {
        if weapon_mappings.len() > 65_536 || effect_mappings.len() > 65_536 {
            return Err(Error::InvalidProfile("too many condition bank mappings"));
        }
        let mut categories = BTreeSet::new();
        let mut effects = BTreeSet::new();
        for player in &self.players {
            if let WeaponCategoryObservation::Category { category, .. } = player.weapon {
                categories.insert(i32::from(category));
            }
            if let Some(chain) = &player.effects {
                effects.extend(chain.entries.iter().map(|entry| entry.native_id));
            }
        }
        Ok(weapon_mappings
            .iter()
            .filter(|mapping| categories.contains(&mapping.condition_id))
            .chain(
                effect_mappings
                    .iter()
                    .filter(|mapping| effects.contains(&mapping.condition_id)),
            )
            .map(|mapping| mapping.bank_id)
            .collect())
    }
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    pub fn sound_player_conditions(
        &self,
        target: CharacterIdentity,
        selection: WeaponSelectionMode,
        row_limit: usize,
        effect_limit: usize,
    ) -> Result<Option<SoundPlayerConditions>> {
        if !(1..=65_536).contains(&row_limit) || !(1..=65_536).contains(&effect_limit) {
            return Err(Error::InvalidProfile(
                "invalid sound condition observation limit",
            ));
        }
        let Some(identity) = self.character_identity(target.address, target.epoch)? else {
            return Ok(None);
        };
        if identity != target {
            return Err(Error::InvalidProfile(
                "stale sound condition character identity",
            ));
        }
        let equipment = self.character_equipment(target)?;
        let mut parameter_query = None;
        let weapon = if let Some(equipment) = equipment {
            let selected = equipment.selected_weapon(selection)?;
            if selected.id <= 0 {
                WeaponCategoryObservation::NotEquipped {
                    native_id: selected.id,
                }
            } else {
                // Positive ID was checked; integer division preserves the consumer policy.
                let id = weapon_base_parameter_id(selected.id).expect("positive weapon id");
                let field = self.experimental_parameter_field(
                    ParameterField::WeaponSpAttackCategory,
                    0,
                    id,
                    row_limit,
                )?;
                parameter_query = Some((id, field));
                match field {
                    None => WeaponCategoryObservation::RowMissing { parameter_id: id },
                    Some(field) => {
                        let ScalarValue::U16(category) = field.value else {
                            return Err(Error::InvalidProfile(
                                "weapon category scalar type differs",
                            ));
                        };
                        WeaponCategoryObservation::Category {
                            parameter_id: id,
                            row_address: field.row_address,
                            category,
                        }
                    }
                }
            }
        } else {
            WeaponCategoryObservation::AssemblyMissing
        };
        let effects = self.character_effects(target, effect_limit)?;
        if let Some((id, expected)) = parameter_query
            && self.experimental_parameter_field(
                ParameterField::WeaponSpAttackCategory,
                0,
                id,
                row_limit,
            )? != expected
        {
            return Err(Error::InvalidProfile("sound condition parameter changed"));
        }
        if self.character_equipment(target)? != equipment
            || self.character_identity(target.address, target.epoch)? != Some(target)
        {
            return Err(Error::InvalidProfile(
                "sound condition equipment or character changed",
            ));
        }
        Ok(Some(SoundPlayerConditions {
            character: target,
            weapon,
            effects,
        }))
    }

    /// Enumerate all non-null players with complete per-player conditions and
    /// recheck the entire set and all players as two complete copies. Unknown styles, missing fields, stale identities,
    /// cyclic chains and exceeded bounds fail the whole observation. World None
    /// remains distinct from a present world's empty player set. Caller supplies
    /// the phase/lifetime and advances epoch on reuse; identical ABA is not detected.
    pub fn world_sound_conditions(
        &self,
        epoch: u64,
        selection: WeaponSelectionMode,
        player_limit: usize,
        row_limit: usize,
        effect_limit: usize,
    ) -> Result<Option<SoundConditionSnapshot>> {
        let Some(first) = self.copy_world_sound_conditions(
            epoch,
            selection,
            player_limit,
            row_limit,
            effect_limit,
        )?
        else {
            return Ok(None);
        };
        if self
            .copy_world_sound_conditions(epoch, selection, player_limit, row_limit, effect_limit)?
            .as_ref()
            != Some(&first)
        {
            return Err(Error::InvalidProfile(
                "complete sound condition inputs changed",
            ));
        }
        Ok(Some(first))
    }
    fn copy_world_sound_conditions(
        &self,
        epoch: u64,
        selection: WeaponSelectionMode,
        player_limit: usize,
        row_limit: usize,
        effect_limit: usize,
    ) -> Result<Option<SoundConditionSnapshot>> {
        if epoch == 0
            || [player_limit, row_limit, effect_limit]
                .iter()
                .any(|limit| !(1..=65_536).contains(limit))
        {
            return Err(Error::InvalidProfile(
                "invalid sound condition epoch or limits",
            ));
        }
        let Some(world) = self.world_players(player_limit)? else {
            return Ok(None);
        };
        let mut players = Vec::with_capacity(world.set.characters.len());
        for entry in &world.set.characters {
            let character = CharacterIdentity {
                address: entry.character.address,
                world_address: world.world_address,
                native_identifier: entry.native_identifier,
                epoch,
            };
            players.push(
                self.sound_player_conditions(character, selection, row_limit, effect_limit)?
                    .ok_or(Error::InvalidProfile("sound condition player disappeared"))?,
            );
        }
        if self.world_players(player_limit)?.as_ref() != Some(&world) {
            return Err(Error::InvalidProfile("sound condition player set changed"));
        }
        Ok(Some(SoundConditionSnapshot {
            world_address: world.world_address,
            players,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EffectNodeObservation;

    #[test]
    fn target_union_covers_all_players_duplicates_signed_effects_and_full_categories() {
        let character = CharacterIdentity {
            address: 8,
            world_address: 16,
            native_identifier: 0x1234,
            epoch: 1,
        };
        let player = |category, ids: &[i32]| SoundPlayerConditions {
            character,
            weapon: WeaponCategoryObservation::Category {
                parameter_id: 100,
                row_address: 32,
                category,
            },
            effects: Some(CharacterEffectsObservation {
                character,
                container_address: 40,
                entries: ids
                    .iter()
                    .enumerate()
                    .map(|(i, &native_id)| EffectNodeObservation {
                        address: 48 + i as u64 * 8,
                        native_id,
                    })
                    .collect(),
            }),
        };
        let snapshot = SoundConditionSnapshot {
            world_address: 16,
            players: vec![player(73, &[100008, 100008]), player(u16::MAX, &[-1, 0])],
        };
        let mapping = |condition_id, bank_id| ConditionBankMapping {
            condition_id,
            bank_id,
        };
        assert_eq!(
            snapshot
                .bank_targets(
                    &[
                        mapping(73, 7800),
                        mapping(73, 7800),
                        mapping(65535, 1),
                        mapping(-1, 2)
                    ],
                    &[
                        mapping(100008, 4205),
                        mapping(-1, 1),
                        mapping(0, 0),
                        mapping(99, 9)
                    ]
                )
                .unwrap(),
            BTreeSet::from([0, 1, 4205, 7800])
        );
        assert_eq!(snapshot.players().len(), 2);
        assert_eq!(snapshot.world_address(), 16);
        assert!(
            snapshot
                .bank_targets(&vec![mapping(0, 0); 65_537], &[])
                .is_err()
        );
        assert!(
            snapshot
                .bank_targets(&[], &vec![mapping(0, 0); 65_537])
                .is_err()
        );
        assert!(
            SoundConditionSnapshot {
                world_address: 16,
                players: vec![]
            }
            .bank_targets(&[], &[])
            .unwrap()
            .is_empty()
        );
    }
}
