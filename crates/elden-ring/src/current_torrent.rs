//! Experimental single-byte saved selector writes; no native refresh or hook installation.
use crate::{
    WW_171, Ww171File,
    torrent::{SELECTOR_OFFSET, TorrentSelectorObservation},
};
use shikifsrs_runtime::{
    Error, Result, code_patches::OwnedCodePatches, memory::CurrentProcess,
    profile::OwnedVerifiedFile,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TorrentSelectorWrite {
    Applied,
    Unchanged,
    Superseded,
    Unavailable,
}

/// Thread confined. Does not pin the saved state or acquire a game lock.
pub struct CurrentTorrentSelector {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentTorrentSelector {
    /// Verify exact file and loaded evidence, without installing code or writing state.
    /// # Safety
    /// base identifies this process's current ER module; caller holds module/SDK, live
    /// manager and initialized PlayerGameData allocation with writable +960 byte, and an
    /// exclusive permitted saved-state mutation phase. No game write, callback, teardown,
    /// address reuse/ABA, alias, unload or concurrent code patch overlaps any operation.
    /// A manifest additionally requires its owner's transitive hook contracts. Epoch and
    /// copied addresses do not acquire any guard. Runtime behavior remains unaccepted.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero torrent selector epoch"));
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
    pub fn observe(&self) -> Result<Option<TorrentSelectorObservation>> {
        let file = Ww171File::from_verified(self.file.borrow()?)?;
        file.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .torrent_selector(self.epoch)
    }
    /// Ordered event flags independently of whether saved PlayerGameData is ready.
    /// Preserves consumer selection publication during saved-mirror startup absence.
    pub fn flags(
        &self,
        ids: &[u32],
        node_limit: u32,
    ) -> Result<Option<Vec<crate::event_flags::EventFlagObservation>>> {
        crate::event_flags::validate_flag_batch(ids, node_limit)?;
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .event_flags(ids, node_limit)
    }
    /// Freshly bind and copy the saved mirror and bounded ordered event flags. Slot selection,
    /// extended mappings and hook routing remain the caller's original catalog policy.
    pub fn observe_flags(
        &self,
        ids: &[u32],
        node_limit: u32,
    ) -> Result<Option<crate::torrent::TorrentFlagStateObservation>> {
        crate::event_flags::validate_flag_batch(ids, node_limit)?;
        Ww171File::from_verified(self.file.borrow()?)?
            .bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?
            .torrent_flag_state(self.epoch, ids, node_limit)
    }

    /// Single conditional store; caller explicitly maps experimental selections to native0..3.
    /// # Safety
    /// bind's exclusive writable live allocation contract holds for observation and store.
    /// This is not atomic compare-exchange and does not call refresh or regenerate a mount.
    pub unsafe fn compare_replace(
        &self,
        expected: TorrentSelectorObservation,
        selector: u8,
    ) -> Result<TorrentSelectorWrite> {
        if selector > 3 || expected.selector > 3 {
            return Err(Error::InvalidProfile(
                "torrent store accepts only native selectors0..3",
            ));
        }
        if expected.module_base != self.base || expected.epoch != self.epoch {
            return Ok(TorrentSelectorWrite::Unavailable);
        }
        match destination(self.observe()?, expected, selector)? {
            Destination::Stop(outcome) => Ok(outcome),
            Destination::Store(at) => {
                // SAFETY: Caller pins writable one-byte extent under exclusive mutation phase.
                // Exactly one byte store; no fallible operation follows the side effect.
                unsafe {
                    (at as *mut u8).write(selector);
                }
                Ok(TorrentSelectorWrite::Applied)
            }
        }
    }
    /// # Safety
    /// Reestablish bind's contract after a known lifetime transition; epoch is not a guard.
    pub unsafe fn advance_epoch(&mut self, epoch: u64) -> Result<()> {
        if epoch <= self.epoch {
            return Err(Error::InvalidProfile("torrent selector epoch must advance"));
        }
        self.epoch = epoch;
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Destination {
    Stop(TorrentSelectorWrite),
    Store(usize),
}
fn destination(
    current: Option<TorrentSelectorObservation>,
    expected: TorrentSelectorObservation,
    selector: u8,
) -> Result<Destination> {
    if selector > 3 || expected.selector > 3 || expected.epoch == 0 {
        return Err(Error::InvalidProfile("invalid torrent selector snapshot"));
    }
    let Some(now) = current else {
        return Ok(Destination::Stop(TorrentSelectorWrite::Unavailable));
    };
    if (
        now.module_base,
        now.manager_address,
        now.player_data_address,
        now.epoch,
    ) != (
        expected.module_base,
        expected.manager_address,
        expected.player_data_address,
        expected.epoch,
    ) {
        return Ok(Destination::Stop(TorrentSelectorWrite::Unavailable));
    }
    if now.selector != expected.selector {
        return Ok(Destination::Stop(TorrentSelectorWrite::Superseded));
    }
    if now.selector == selector {
        return Ok(Destination::Stop(TorrentSelectorWrite::Unchanged));
    }
    if now.player_data_address == 0 || !now.player_data_address.is_multiple_of(8) {
        return Err(Error::InvalidProfile("invalid player data destination"));
    }
    let at = now
        .player_data_address
        .checked_add(u64::from(SELECTOR_OFFSET))
        .ok_or(Error::AddressOverflow)?;
    Ok(Destination::Store(
        usize::try_from(at).map_err(|_| Error::AddressOverflow)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> TorrentSelectorObservation {
        TorrentSelectorObservation {
            module_base: 0x100000,
            manager_address: 0x200000,
            player_data_address: 0x300000,
            epoch: 3,
            selector: 1,
        }
    }
    #[test]
    fn selector_store_plan_preserves_foreign_values_replaced_state_and_noop() {
        let expected = snapshot();
        assert_eq!(
            destination(Some(expected), expected, 2).unwrap(),
            Destination::Store(0x300960)
        );
        assert_eq!(
            destination(Some(expected), expected, 1).unwrap(),
            Destination::Stop(TorrentSelectorWrite::Unchanged)
        );
        assert_eq!(
            destination(None, expected, 2).unwrap(),
            Destination::Stop(TorrentSelectorWrite::Unavailable)
        );
        let mut now = expected;
        now.selector = 3;
        assert_eq!(
            destination(Some(now), expected, 2).unwrap(),
            Destination::Stop(TorrentSelectorWrite::Superseded)
        );
        for field in 0..4 {
            let mut now = expected;
            match field {
                0 => now.module_base += 8,
                1 => now.manager_address += 8,
                2 => now.player_data_address += 8,
                _ => now.epoch += 1,
            };
            assert_eq!(
                destination(Some(now), expected, 2).unwrap(),
                Destination::Stop(TorrentSelectorWrite::Unavailable)
            );
        }
    }
    #[test]
    fn selector_store_plan_refuses_extensions_zero_epoch_unaligned_and_overflow() {
        let expected = snapshot();
        assert!(destination(Some(expected), expected, 4).is_err());
        for bad in [
            TorrentSelectorObservation {
                selector: 4,
                ..expected
            },
            TorrentSelectorObservation {
                epoch: 0,
                ..expected
            },
            TorrentSelectorObservation {
                player_data_address: 0,
                ..expected
            },
            TorrentSelectorObservation {
                player_data_address: 1,
                ..expected
            },
            TorrentSelectorObservation {
                player_data_address: u64::MAX - 7,
                ..expected
            },
        ] {
            assert!(destination(Some(bad), bad, 2).is_err());
        }
    }
}
