//! Read memory as bytes. Rust references to game-owned objects are never fabricated.
use crate::{Error, Result};

pub trait AddressSpace {
    /// Copies the entire span or reports failure. An empty span performs no access.
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()>;

    fn read_u8(&self, address: u64) -> Result<u8> {
        let mut bytes = [0; 1];
        self.read_exact(address, &mut bytes)?;
        Ok(bytes[0])
    }

    fn read_u32(&self, address: u64) -> Result<u32> {
        let mut bytes = [0; 4];
        self.read_exact(address, &mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn read_u64(&self, address: u64) -> Result<u64> {
        let mut bytes = [0; 8];
        self.read_exact(address, &mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn read_f32(&self, address: u64) -> Result<f32> {
        Ok(f32::from_bits(self.read_u32(address)?))
    }

    /// Reads a signed RIP displacement and resolves it relative to its end.
    fn rip_target(&self, displacement_address: u64, trailing_bytes: u64) -> Result<u64> {
        let delta = self.read_u32(displacement_address)? as i32 as i64;
        let end = displacement_address
            .checked_add(4)
            .and_then(|v| v.checked_add(trailing_bytes))
            .ok_or(Error::AddressOverflow)?;
        end.checked_add_signed(delta).ok_or(Error::AddressOverflow)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SliceMemory<'a> {
    base: u64,
    bytes: &'a [u8],
}

impl<'a> SliceMemory<'a> {
    pub fn new(base: u64, bytes: &'a [u8]) -> Result<Self> {
        base.checked_add(bytes.len() as u64)
            .ok_or(Error::AddressOverflow)?;
        Ok(Self { base, bytes })
    }
}

impl AddressSpace for SliceMemory<'_> {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        if output.is_empty() {
            return Ok(());
        }
        let fail = || Error::OutOfBounds {
            address,
            length: output.len(),
        };
        let offset = usize::try_from(address.checked_sub(self.base).ok_or_else(fail)?)
            .map_err(|_| fail())?;
        let end = offset
            .checked_add(output.len())
            .ok_or(Error::AddressOverflow)?;
        let source = self.bytes.get(offset..end).ok_or_else(fail)?;
        output.copy_from_slice(source);
        Ok(())
    }
}

/// Copies from the current process using Windows' checked read primitive.
///
/// A successful copy does not establish an object's lifetime or an engine phase.
#[cfg(windows)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CurrentProcess;

#[cfg(windows)]
impl AddressSpace for CurrentProcess {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        // SAFETY: GetCurrentProcess has no arguments and returns a process pseudo-handle.
        let process = unsafe { windows_reads::GetCurrentProcess() };
        windows_reads::read_process(process, address, output)
    }
}

/// An owned Windows handle opened with PROCESS_VM_READ only.
/// No write, thread creation, or privilege-adjustment interface is provided.
#[cfg(windows)]
#[derive(Debug)]
pub struct RemoteProcess {
    handle: std::ptr::NonNull<std::ffi::c_void>,
}

#[cfg(windows)]
impl RemoteProcess {
    pub fn open_readonly(pid: u32) -> Result<Self> {
        // SAFETY: OpenProcess takes integer values; this requests read access and no inheritance.
        let handle = unsafe { windows_reads::OpenProcess(0x0010, 0, pid) };
        let handle = std::ptr::NonNull::new(handle).ok_or_else(|| {
            // SAFETY: GetLastError takes no pointers and immediately follows the failed open.
            let code = unsafe { windows_reads::GetLastError() };
            Error::WindowsCall {
                operation: "OpenProcess(PROCESS_VM_READ)",
                code,
            }
        })?;
        Ok(Self { handle })
    }
}

#[cfg(windows)]
impl Drop for RemoteProcess {
    fn drop(&mut self) {
        // SAFETY: This handle is owned, opened successfully, never cloned, and closed once.
        unsafe {
            windows_reads::CloseHandle(self.handle.as_ptr());
        }
    }
}

#[cfg(windows)]
impl AddressSpace for RemoteProcess {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        windows_reads::read_process(self.handle.as_ptr(), address, output)
    }
}

#[cfg(windows)]
mod windows_reads {
    use super::{Error, Result};
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub(super) fn GetCurrentProcess() -> *mut c_void;
        pub(super) fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        pub(super) fn CloseHandle(handle: *mut c_void) -> i32;
        pub(super) fn GetLastError() -> u32;
        fn ReadProcessMemory(
            process: *mut c_void,
            source: *const c_void,
            destination: *mut c_void,
            size: usize,
            read: *mut usize,
        ) -> i32;
    }

    // Private callers provide either a valid owned handle or the current-process pseudo-handle.
    pub(super) fn read_process(
        process: *mut c_void,
        address: u64,
        output: &mut [u8],
    ) -> Result<()> {
        if output.is_empty() {
            return Ok(());
        }
        address
            .checked_add(output.len() as u64)
            .ok_or(Error::AddressOverflow)?;
        let source = usize::try_from(address).map_err(|_| Error::AddressOverflow)?;
        let mut read = 0;
        // SAFETY: Windows validates the source span; destination is our live writable slice.
        let success = unsafe {
            ReadProcessMemory(
                process,
                source as *const c_void,
                output.as_mut_ptr().cast(),
                output.len(),
                &mut read,
            )
        };
        if success == 0 || read != output.len() {
            // SAFETY: GetLastError takes no pointers and is called immediately after the failed read.
            let code = unsafe { GetLastError() };
            return Err(Error::ProcessReadFailed {
                address,
                length: output.len(),
                code,
            });
        }
        Ok(())
    }
}
