//! Seven exact native marker slots, copied without acquiring UI ownership.
use crate::objects::{GameObjectCatalog, ObjectKind, ObjectObservation};
use shikifsrs_runtime::{Error, Result, memory::AddressSpace};

const FRONTEND_GLOBAL: u32 = 0x3d6f8f0;
const VALUES_OFFSET: u32 = 0x80;
const DISPLAY_OFFSET: u32 = 0x5c60;
const DISPLAY_STRIDE: u32 = 0xb0;
const DISPLAY_VISIBLE: u32 = 0x10;
const DISPLAY_HANDLE: u32 = 0x98;
const DISPLAY_ELAPSED: u32 = 0x8c;
const TAG_OFFSET: u32 = 0x1e88;
const TAG_STRIDE: u32 = 0x128;
const TAG_HANDLE: u32 = 8;
const TAG_POSITION: u32 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayMarker {
    pub address: u64,
    pub identifier: u64,
    pub visible: u8,
    /// Native float bits at offsets 0/4, not tag's signed integer pixels.
    pub screen_bits: [u32; 2],
    /// Native 774fd0 accumulates its input delta here. No clock or freshness policy.
    pub elapsed_bits: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TagMarker {
    pub address: u64,
    pub identifier: u64,
    /// Visible, update-position, offscreen. Unknown bytes remain uncoerced.
    pub flags: [u8; 3],
    /// 774fd0 converts display floats with CVTTSS2SI; 8c33f0 consumes signed words.
    pub screen_pixels: [i32; 2],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrontendMarkerObservation {
    pub module_base: u64,
    pub epoch: u64,
    pub frontend: ObjectObservation,
    pub values_address: u64,
    pub displays: [DisplayMarker; 7],
    pub tags: [TagMarker; 7],
}
fn at(address: u64, offset: u32) -> Result<u64> {
    address
        .checked_add(u64::from(offset))
        .ok_or(Error::AddressOverflow)
}
fn pair(memory: &impl AddressSpace, address: u64) -> Result<[u32; 2]> {
    let mut bytes = [0; 8];
    memory.read_exact(address, &mut bytes)?;
    Ok(std::array::from_fn(|i| {
        u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
impl<M: AddressSpace> GameObjectCatalog<'_, '_, M> {
    /// Existing complete FRIENDLY_HUD_EVIDENCE gates constructor, publisher,
    /// converter and renderer. Read only selected marker bytes, twice, then
    /// recheck the exact typed singleton. No strings or pointer-backed UI data.
    pub fn frontend_markers(&self, epoch: u64) -> Result<Option<FrontendMarkerObservation>> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero frontend marker lifetime epoch"));
        }
        let Some(frontend) = self.singleton(FRONTEND_GLOBAL, ObjectKind::Frontend)? else {
            if self
                .singleton(FRONTEND_GLOBAL, ObjectKind::Frontend)?
                .is_some()
            {
                return Err(Error::InvalidProfile(
                    "frontend appeared during marker copy",
                ));
            }
            return Ok(None);
        };
        let copy = || -> Result<FrontendMarkerObservation> {
            let memory = self.module.memory();
            let values_address = at(frontend.address, VALUES_OFFSET)?;
            let mut displays = Vec::with_capacity(7);
            let mut tags = Vec::with_capacity(7);
            for index in 0..7 {
                let address = at(frontend.address, DISPLAY_OFFSET + index * DISPLAY_STRIDE)?;
                displays.push(DisplayMarker {
                    address,
                    identifier: memory.read_u64(at(address, DISPLAY_HANDLE)?)?,
                    visible: memory.read_u8(at(address, DISPLAY_VISIBLE)?)?,
                    screen_bits: pair(memory, address)?,
                    elapsed_bits: memory.read_u32(at(address, DISPLAY_ELAPSED)?)?,
                });
                let address = at(values_address, TAG_OFFSET + index * TAG_STRIDE)?;
                let mut flags = [0; 3];
                memory.read_exact(address, &mut flags)?;
                tags.push(TagMarker {
                    address,
                    identifier: memory.read_u64(at(address, TAG_HANDLE)?)?,
                    flags,
                    screen_pixels: pair(memory, at(address, TAG_POSITION)?)?
                        .map(|word| word as i32),
                });
            }
            Ok(FrontendMarkerObservation {
                module_base: self.module.base(),
                epoch,
                frontend,
                values_address,
                displays: displays
                    .try_into()
                    .map_err(|_| Error::InvalidProfile("display marker count"))?,
                tags: tags
                    .try_into()
                    .map_err(|_| Error::InvalidProfile("tag marker count"))?,
            })
        };
        let first = copy()?;
        if copy()? != first
            || self.singleton(FRONTEND_GLOBAL, ObjectKind::Frontend)? != Some(frontend)
        {
            return Err(Error::InvalidProfile(
                "frontend markers changed during copy",
            ));
        }
        Ok(Some(first))
    }
}
