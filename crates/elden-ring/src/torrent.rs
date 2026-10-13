//! Independently proved native four-slot appearance and saved selector interfaces.
//! Extended appearance IDs and network roles are consumer policy, not native facts.
use crate::{
    Ww171File,
    objects::{GameObjectCatalog, ObjectKind},
};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace, profile::CodeRange};

pub(crate) const TORRENT_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0xd42420,
        length: 0x17,
    },
    CodeRange {
        rva: 0xafeea0,
        length: 0x1d2,
    },
    CodeRange {
        rva: 0x255d10,
        length: 0x4b,
    },
    CodeRange {
        rva: 0x254650,
        length: 0x5d1,
    },
    CodeRange {
        rva: 0x25d550,
        length: 0x4bc,
    },
    CodeRange {
        rva: 0x50d640,
        length: 0x273,
    },
    CodeRange {
        rva: 0x404320,
        length: 0x241,
    },
];
const GAME_DATA_GLOBAL: u32 = 0x3d61f98;
const PLAYER_DATA_OFFSET: u32 = 8;
pub(crate) const SELECTOR_OFFSET: u32 = 0x960;
const APPEARANCE_TABLE: u32 = 0x2bb5028;
const SPAWN_RETURN: u32 = 0x50d740;
const ASSEMBLY_RETURN: u32 = 0x4043ef;
const NATIVE_APPEARANCES: [i32; 4] = [80020000, 80030000, 80040000, 80050000];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TorrentHookSites {
    pub appearance_resolver: u64,
    pub selector_refresh: u64,
    /// Return following resolver in the world character creation path. Not a network-role proof.
    pub spawn_return: u64,
    /// Return following resolver with packed two-bit selector in the assembly path.
    pub assembly_return: u64,
}

/// Copied addresses and byte, not allocation ownership or a game reference guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TorrentSelectorObservation {
    pub module_base: u64,
    pub manager_address: u64,
    pub player_data_address: u64,
    pub epoch: u64,
    pub selector: u8,
}

/// Ordered raw flag demands alongside the native0..3 saved mirror. This does not choose
/// a catalog slot, label local/remote callers or validate extended appearance resources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentFlagStateObservation {
    pub saved: TorrentSelectorObservation,
    pub flags: Vec<crate::event_flags::EventFlagObservation>,
}

impl Ww171File<'_> {
    /// Complete entry windows for independently owned resolver/refresh hook preparation.
    /// The file has already passed exact WW1.17.1 identity. This does not install hooks.
    pub fn torrent_hook_evidence(&self) -> [CodeRange; 2] {
        [TORRENT_EVIDENCE[0], TORRENT_EVIDENCE[1]]
    }
    /// Read the native readonly four-entry table from the fully verified file.
    /// Native u8 selector outside0..3 returns -1; no extended IDs are invented.
    pub fn torrent_appearance(&self, selector: u8) -> Result<i32> {
        let bytes = self.file().image().read_rva(APPEARANCE_TABLE, 16)?;
        let section = self
            .file()
            .image()
            .section_at(APPEARANCE_TABLE)
            .filter(|s| !s.is_writable() && !s.is_executable())
            .ok_or(Error::InvalidProfile(
                "torrent appearance table is not readonly data",
            ))?;
        if APPEARANCE_TABLE as u64 + 16 > section.rva as u64 + section.raw_size as u64 {
            return Err(Error::InvalidProfile(
                "torrent appearance table crosses its section",
            ));
        }
        decode_appearance(bytes, selector)
    }
}

fn decode_appearance(bytes: &[u8], selector: u8) -> Result<i32> {
    if bytes.len() != 16 {
        return Err(Error::InvalidProfile("truncated torrent appearance table"));
    }
    let mut ids = [0; 4];
    for (id, chunk) in ids.iter_mut().zip(bytes.chunks_exact(4)) {
        *id = i32::from_le_bytes(chunk.try_into().expect("checked width"));
    }
    if ids != NATIVE_APPEARANCES {
        return Err(Error::InvalidProfile(
            "torrent appearance table differs from native profile",
        ));
    }
    Ok(ids.get(selector as usize).copied().unwrap_or(-1))
}

impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Recheck the complete native selector and ordered flag selection together.
    /// Missing saved data or flag manager returns None, never a fabricated default selection.
    pub fn torrent_flag_state(
        &self,
        epoch: u64,
        ids: &[u32],
        node_limit: u32,
    ) -> Result<Option<TorrentFlagStateObservation>> {
        crate::event_flags::validate_flag_batch(ids, node_limit)?;
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero torrent flag state epoch"));
        }
        let copy = || -> Result<Option<TorrentFlagStateObservation>> {
            let Some(saved) = self.torrent_selector(epoch)? else {
                return Ok(None);
            };
            let Some(flags) = self.copy_event_flags(ids, node_limit)? else {
                return Ok(None);
            };
            Ok(Some(TorrentFlagStateObservation { saved, flags }))
        };
        let first = copy()?;
        if copy()? != first {
            return Err(Error::InvalidProfile(
                "torrent flag state changed during observation",
            ));
        }
        Ok(first)
    }

    /// Copy the saved native selector. Missing manager/player data returns None.
    /// The manager is naked storage (constructor starts with null, not a vtable).
    /// Its +8 link must identify the exact PlayerGameData primary vtable.
    pub fn torrent_selector(&self, epoch: u64) -> Result<Option<TorrentSelectorObservation>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero torrent selector epoch"));
        }
        let Some(manager) = self.module.global_pointer(GAME_DATA_GLOBAL)? else {
            return Ok(None);
        };
        if !manager.is_multiple_of(8) {
            return Err(Error::InvalidProfile("unaligned game data manager"));
        }
        let memory = self.module.memory();
        let link = at(manager, PLAYER_DATA_OFFSET)?;
        let player_data = memory.read_u64(link)?;
        let object = self.require(player_data, ObjectKind::PlayerGameData)?;
        let selector = if player_data == 0 {
            0
        } else {
            memory.read_u8(at(player_data, SELECTOR_OFFSET)?)?
        };
        if selector > 3 {
            return Err(Error::InvalidProfile(
                "saved torrent selector exceeds native four slots",
            ));
        }
        if (player_data != 0 && memory.read_u8(at(player_data, SELECTOR_OFFSET)?)? != selector)
            || memory.read_u64(link)? != player_data
            || self.require(player_data, ObjectKind::PlayerGameData)? != object
            || self.module.global_pointer(GAME_DATA_GLOBAL)? != Some(manager)
        {
            return Err(Error::InvalidProfile(
                "torrent selector or saved-state links changed",
            ));
        }
        Ok(object.map(|_| TorrentSelectorObservation {
            module_base: self.module.base(),
            manager_address: manager,
            player_data_address: player_data,
            epoch,
            selector,
        }))
    }

    /// Revalidate complete native code windows and readonly table before returning hook sites.
    /// No installation, native invocation, role inference or trampoline ownership occurs.
    pub fn torrent_hook_sites(&self) -> Result<TorrentHookSites> {
        for range in TORRENT_EVIDENCE {
            self.module.code_address(*range)?;
        }
        let expected = self.module.file().image().read_rva(APPEARANCE_TABLE, 16)?;
        decode_appearance(expected, 0)?;
        let mut live = [0; 16];
        self.module
            .memory()
            .read_exact(at(self.module.base(), APPEARANCE_TABLE)?, &mut live)?;
        if expected != live {
            return Err(Error::ImageMismatch {
                rva: APPEARANCE_TABLE,
            });
        }
        Ok(TorrentHookSites {
            appearance_resolver: at(self.module.base(), TORRENT_EVIDENCE[0].rva)?,
            selector_refresh: at(self.module.base(), TORRENT_EVIDENCE[1].rva)?,
            spawn_return: at(self.module.base(), SPAWN_RETURN)?,
            assembly_return: at(self.module.base(), ASSEMBLY_RETURN)?,
        })
    }
}

fn at(base: u64, offset: u32) -> Result<u64> {
    base.checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_four_appearance_table_preserves_full_u8_outside_failure() {
        let bytes: Vec<_> = NATIVE_APPEARANCES
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect();
        for selector in 0..=255 {
            assert_eq!(
                decode_appearance(&bytes, selector).unwrap(),
                NATIVE_APPEARANCES
                    .get(selector as usize)
                    .copied()
                    .unwrap_or(-1)
            );
        }
        let mut changed = bytes.clone();
        changed[15] ^= 1;
        assert!(decode_appearance(&changed, 255).is_err());
        assert!(decode_appearance(&bytes[..15], 0).is_err());
    }
}
