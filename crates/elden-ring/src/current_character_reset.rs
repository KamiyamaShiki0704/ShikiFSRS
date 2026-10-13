//! Explicit native character reset for resident summons. Offline experimental.
use crate::{
    WW_171, Ww171File,
    character_sets::CharacterIdentity,
    character_spatial::{CharacterPlacementObservation, NativePoseInput, placement_plan},
};
use shikifsrs_runtime::{
    Error, Result,
    code_patches::OwnedCodePatches,
    memory::CurrentProcess,
    profile::{CodeRange, OwnedVerifiedFile},
};
use std::{marker::PhantomData, rc::Rc, sync::Arc};

pub(crate) const RESET_EVIDENCE: &[CodeRange] = &[CodeRange {
    rva: 0x3f59c0,
    length: 0x2dc,
}];

#[derive(Debug)]
pub struct NativeCharacterResetCall {
    pub character: CharacterIdentity,
    pub input: NativePoseInput,
    /// Forwarded native fourth byte; neither value proves gameplay readiness.
    pub alternate: bool,
    /// Native AL only. Upper return-register bits are undefined.
    pub returned_byte: u8,
    /// The attempt occurred even when identity observation fails.
    pub postflight: Result<CharacterIdentity>,
}

pub struct CurrentCharacterReset {
    file: OwnedVerifiedFile,
    patches: Option<Arc<OwnedCodePatches>>,
    base: u64,
    epoch: u64,
    _thread: PhantomData<Rc<()>>,
}
impl CurrentCharacterReset {
    /// # Safety
    /// Caller pins the entire character/controller/module/resource/model/event/task/world
    /// graph, native globals, allocator and transitive callees in an exclusive valid reset
    /// phase. All cached poses and writable native state are initialized; no reader,
    /// teardown, address reuse, competing reset or module unload overlaps an operation.
    /// Binding and copied observations acquire none of these guarantees.
    pub unsafe fn bind(
        bytes: Arc<[u8]>,
        base: u64,
        epoch: u64,
        patches: Option<Arc<OwnedCodePatches>>,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(Error::InvalidProfile("zero character reset epoch"));
        }
        let file = OwnedVerifiedFile::verify(bytes, WW_171)?;
        let image = Ww171File::from_verified(file.borrow()?)?;
        let objects = image.bind_objects_with_patches(&CurrentProcess, base, patches.as_deref())?;
        objects.module.code_address(RESET_EVIDENCE[0])?;
        Ok(Self {
            file,
            patches,
            base,
            epoch,
            _thread: PhantomData,
        })
    }
    /// Invoke the complete independently decoded native reset once. It resets numerous
    /// event/controller/model/entry fields and invokes additional native callbacks; this
    /// is not equivalent to an HP store, visibility request, resurrection or a barrier.
    /// # Safety
    /// Bind's complete writable graph/phase contract holds. Caller owns this particular
    /// reset, valid position/Euler conventions and resources, reconciles every attempted
    /// operation after failure, and separately restores HP/policy/presentation. Only
    /// non-primary characters may be reset here. No automatic retry or Drop cleanup.
    pub unsafe fn execute(
        &self,
        expected: CharacterPlacementObservation,
        input: NativePoseInput,
        alternate: bool,
    ) -> Result<NativeCharacterResetCall> {
        let image = Ww171File::from_verified(self.file.borrow()?)?;
        let objects =
            image.bind_objects_with_patches(&CurrentProcess, self.base, self.patches.as_deref())?;
        let target = expected.motion.spatial.module.character;
        let angles = placement_plan(
            objects.character_placement(target)?,
            expected,
            self.epoch,
            input,
        )?;
        let primary = objects.primary_player_identity(self.epoch)?;
        if primary.is_none() || primary == Some(target) {
            return Err(Error::NativeOperationRejected(
                "character reset excludes absent/primary player",
            ));
        }
        let entry = objects.module.code_address(RESET_EVIDENCE[0])?;
        // SAFETY: Exact RCX character/RDX aligned position/R8 aligned Euler/R9B byte/AL ABI.
        let function: Reset = unsafe { std::mem::transmute(entry as usize) };
        if objects.primary_player_identity(self.epoch)? != primary {
            return Err(Error::NativeOperationRejected(
                "primary changed before character reset",
            ));
        }
        // SAFETY: Caller retains the complete native graph and exclusive phase through observation.
        Ok(unsafe {
            invoke(function, target, input, angles, alternate, || {
                objects
                    .character_identity(target.address, self.epoch)?
                    .ok_or(Error::InvalidProfile("character lost after native reset"))
            })
        })
    }
}
#[repr(C, align(16))]
struct Vector {
    values: [f32; 4],
}
type Reset = unsafe extern "system" fn(u64, *const Vector, *const Vector, u8) -> u8;
unsafe fn invoke(
    function: Reset,
    target: CharacterIdentity,
    input: NativePoseInput,
    angles: [u32; 4],
    alternate: bool,
    observe: impl FnOnce() -> Result<CharacterIdentity>,
) -> NativeCharacterResetCall {
    let position = Vector {
        values: input.position_bits.map(f32::from_bits),
    };
    let rotation = Vector {
        values: angles.map(f32::from_bits),
    };
    // SAFETY: Original aligned owned buffers remain live for the complete single native call.
    let returned_byte =
        unsafe { function(target.address, &position, &rotation, u8::from(alternate)) };
    NativeCharacterResetCall {
        character: target,
        input,
        alternate,
        returned_byte,
        postflight: observe().and_then(|after| {
            if after == target {
                Ok(after)
            } else {
                Err(Error::InvalidProfile("character reset identity changed"))
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[derive(Default)]
    struct Calls {
        count: usize,
        target: u64,
        position: [u32; 4],
        angles: [u32; 4],
        byte: u8,
    }
    thread_local! { static CALLS: RefCell<Calls> = RefCell::new(Calls::default()); }
    unsafe extern "system" fn record(
        target: u64,
        position: *const Vector,
        angles: *const Vector,
        byte: u8,
    ) -> u8 {
        assert_eq!(position as usize % 16, 0);
        assert_eq!(angles as usize % 16, 0);
        assert_ne!(position, angles);
        // SAFETY: invoke owns both original initialized test buffers, no game memory is accessed.
        let (position, angles) = unsafe {
            (
                (*position).values.map(f32::to_bits),
                (*angles).values.map(f32::to_bits),
            )
        };
        CALLS.with(|c| {
            let mut c = c.borrow_mut();
            c.count += 1;
            c.target = target;
            c.position = position;
            c.angles = angles;
            c.byte = byte;
        });
        0x81
    }
    #[test]
    fn reset_actual_abi_preserves_aligned_vectors_fourth_byte_and_failed_attempt() {
        CALLS.with(|c| *c.borrow_mut() = Calls::default());
        let target = CharacterIdentity {
            address: 0x1000,
            world_address: 0x2000,
            native_identifier: 7,
            epoch: 3,
        };
        let input = NativePoseInput {
            position_bits: [2.0, -3.0, 4.0, 0.0].map(f32::to_bits),
            angle_bits: [0.0, 1.5, -0.5].map(f32::to_bits),
        };
        let angles = [
            input.angle_bits[0],
            input.angle_bits[1],
            input.angle_bits[2],
            0,
        ];
        for mode in 0..3 {
            // SAFETY: Original owned mock ABI only; no engine or process operations.
            let receipt = unsafe {
                invoke(record, target, input, angles, mode != 0, || match mode {
                    0 => Ok(target),
                    1 => Err(Error::InvalidProfile("postflight unavailable")),
                    _ => Ok(CharacterIdentity { epoch: 4, ..target }),
                })
            };
            assert_eq!(receipt.returned_byte, 0x81);
            assert_eq!(receipt.postflight.is_ok(), mode == 0);
            CALLS.with(|c| {
                let c = c.borrow();
                assert_eq!(c.count, mode + 1);
                assert_eq!(c.target, target.address);
                assert_eq!(c.position, input.position_bits);
                assert_eq!(c.angles, angles);
                assert_eq!(c.byte, u8::from(mode != 0));
            });
            drop(receipt);
            CALLS.with(|c| assert_eq!(c.borrow().count, mode + 1));
        }
    }
}
