//! Linux System V shared memory backed by the four `shm*` syscalls.

use std::ffi::c_void;
use std::io;
use std::ptr::NonNull;

#[cfg(target_arch = "x86_64")]
const SYS_SHMGET: isize = 29;
#[cfg(target_arch = "x86_64")]
const SYS_SHMAT: isize = 30;
#[cfg(target_arch = "x86_64")]
const SYS_SHMCTL: isize = 31;
#[cfg(target_arch = "x86_64")]
const SYS_SHMDT: isize = 67;

#[cfg(target_arch = "aarch64")]
const SYS_SHMGET: isize = 194;
#[cfg(target_arch = "aarch64")]
const SYS_SHMCTL: isize = 195;
#[cfg(target_arch = "aarch64")]
const SYS_SHMAT: isize = 196;
#[cfg(target_arch = "aarch64")]
const SYS_SHMDT: isize = 197;

const IPC_PRIVATE: i32 = 0;
const IPC_CREAT: i32 = 0o1000;
const IPC_RMID: i32 = 0;
const OWNER_READ_WRITE: i32 = 0o600;

unsafe extern "C" {
    fn syscall(number: isize, ...) -> isize;
}

/// A process mapping of a private SysV shared-memory segment.
///
/// The mapping is automatically detached and its kernel segment marked for removal on drop.
pub struct SharedMemory {
    id: i32,
    pointer: NonNull<u8>,
    len: usize,
    removal_requested: bool,
}

impl SharedMemory {
    /// Allocates and attaches a private shared-memory segment with owner-only access.
    ///
    /// # Errors
    /// Returns an I/O error when the size is invalid or a SysV operation fails.
    pub fn new(len: usize) -> io::Result<Self> {
        if len == 0 || len > isize::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid shared-memory length",
            ));
        }

        // SAFETY: The SysV syscall arguments use their documented scalar types.
        let id_result = unsafe { syscall(SYS_SHMGET, IPC_PRIVATE, len, IPC_CREAT | OWNER_READ_WRITE) };
        let id = syscall_value(id_result)?;
        let id = match i32::try_from(id) {
            Ok(id) => id,
            Err(_) => {
                remove_segment(id);
                return Err(io::Error::other("shared-memory id does not fit i32"));
            }
        };

        // SAFETY: A null address asks the kernel to choose a mapping; the returned address is checked below.
        let address = unsafe { syscall(SYS_SHMAT, id, std::ptr::null::<c_void>(), 0_i32) };
        if address == -1 {
            let error = io::Error::last_os_error();
            remove_segment(isize::try_from(id).unwrap_or(-1));
            return Err(error);
        }
        let address = match usize::try_from(address) {
            Ok(address) => address,
            Err(_) => {
                let pointer = std::ptr::with_exposed_provenance::<c_void>(address as usize);
                // SAFETY: `pointer` is reconstructed from the pointer-sized value returned by successful shmat.
                unsafe {
                    let _ = syscall(SYS_SHMDT, pointer);
                }
                remove_segment(isize::try_from(id).unwrap_or(-1));
                return Err(io::Error::other("shared-memory address does not fit usize"));
            }
        };
        let pointer = match NonNull::new(std::ptr::with_exposed_provenance_mut(address)) {
            Some(pointer) => pointer,
            None => {
                // SAFETY: A null shmat result still represents a successful attach at address zero.
                unsafe {
                    let _ = syscall(SYS_SHMDT, std::ptr::null::<c_void>());
                }
                remove_segment(isize::try_from(id).unwrap_or(-1));
                return Err(io::Error::other("shmat returned a null address"));
            }
        };

        Ok(Self {
            id,
            pointer,
            len,
            removal_requested: false,
        })
    }

    /// Returns the kernel identifier used by the MIT-SHM Attach request.
    #[must_use]
    pub const fn id(&self) -> i32 {
        self.id
    }

    /// Returns the mapped byte length.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns whether the mapping has no bytes; allocation rejects empty mappings.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Copies 32-bit pixels to the segment in little-endian byte order.
    ///
    /// # Errors
    /// Returns `InvalidInput` when the requested range exceeds the mapping.
    pub fn write_u32_le(&mut self, offset: usize, pixels: &[u32]) -> io::Result<()> {
        let byte_len = pixels
            .len()
            .checked_mul(4)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "pixel byte count overflow"))?;
        self.check_range(offset, byte_len)?;
        // SAFETY: `check_range` proves that this byte slice is inside the live mapping.
        let bytes = unsafe { std::slice::from_raw_parts_mut(self.pointer.as_ptr().wrapping_add(offset), byte_len) };
        for (chunk, pixel) in bytes.chunks_exact_mut(4).zip(pixels) {
            chunk.copy_from_slice(&pixel.to_le_bytes());
        }
        Ok(())
    }

    /// Copies bytes out of the segment into a caller-owned buffer.
    ///
    /// # Errors
    /// Returns `InvalidInput` when the requested range exceeds the mapping.
    pub fn read_to(&self, offset: usize, out: &mut [u8]) -> io::Result<()> {
        self.check_range(offset, out.len())?;
        // SAFETY: `check_range` proves that this byte slice is inside the live mapping.
        let bytes = unsafe { std::slice::from_raw_parts(self.pointer.as_ptr().wrapping_add(offset), out.len()) };
        out.copy_from_slice(bytes);
        Ok(())
    }

    /// Marks the kernel segment for deletion after its last process detaches.
    ///
    /// This is safe after the X server has successfully attached to the segment.
    ///
    /// # Errors
    /// Returns an I/O error when `shmctl(IPC_RMID)` fails.
    pub fn request_removal(&mut self) -> io::Result<()> {
        if self.removal_requested {
            return Ok(());
        }
        // SAFETY: IPC_RMID ignores the third argument; null is the documented unused value.
        let result = unsafe { syscall(SYS_SHMCTL, self.id, IPC_RMID, std::ptr::null::<c_void>()) };
        syscall_value(result)?;
        self.removal_requested = true;
        Ok(())
    }

    fn check_range(&self, offset: usize, len: usize) -> io::Result<()> {
        let end = offset
            .checked_add(len)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "shared-memory range overflow"))?;
        if end > self.len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "shared-memory range exceeds the mapping",
            ));
        }
        Ok(())
    }
}

impl Drop for SharedMemory {
    fn drop(&mut self) {
        // SAFETY: `pointer` is the live mapping returned by shmat and is detached exactly once here.
        unsafe {
            let _ = syscall(SYS_SHMDT, self.pointer.as_ptr().cast::<c_void>());
        }
        if !self.removal_requested {
            remove_segment(isize::try_from(self.id).unwrap_or(-1));
        }
    }
}

fn syscall_value(value: isize) -> io::Result<isize> {
    if value == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(value)
    }
}

fn remove_segment(id: isize) {
    // SAFETY: IPC_RMID only consumes the integer id; the final pointer argument is unused.
    unsafe {
        let _ = syscall(SYS_SHMCTL, id, IPC_RMID, std::ptr::null::<c_void>());
    }
}

#[cfg(test)]
mod tests {
    use super::SharedMemory;
    use std::io::ErrorKind;

    #[test]
    fn shared_memory_copies_pixels_as_little_endian_bytes() {
        let mut memory = match SharedMemory::new(16) {
            Ok(memory) => memory,
            Err(error) => panic!("allocation failed: {error}"),
        };
        assert_eq!(memory.len(), 16);
        assert!(memory.id() >= 0);
        if let Err(error) = memory.write_u32_le(4, &[0x1122_3344, 0xaabb_ccdd]) {
            panic!("write failed: {error}");
        }
        let mut bytes = [0_u8; 8];
        if let Err(error) = memory.read_to(4, &mut bytes) {
            panic!("read failed: {error}");
        }
        assert_eq!(bytes, [0x44, 0x33, 0x22, 0x11, 0xdd, 0xcc, 0xbb, 0xaa]);
    }

    #[test]
    fn shared_memory_rejects_zero_length() {
        let result = SharedMemory::new(0);
        assert_eq!(result.err().map(|error| error.kind()), Some(ErrorKind::InvalidInput));
    }

    #[test]
    fn shared_memory_rejects_out_of_bounds_access() {
        let mut memory = match SharedMemory::new(8) {
            Ok(memory) => memory,
            Err(error) => panic!("allocation failed: {error}"),
        };
        assert_eq!(
            memory.write_u32_le(5, &[0x1234_5678]).err().map(|error| error.kind()),
            Some(ErrorKind::InvalidInput)
        );
        let mut bytes = [0_u8; 4];
        assert_eq!(
            memory.read_to(5, &mut bytes).err().map(|error| error.kind()),
            Some(ErrorKind::InvalidInput)
        );
    }

    #[test]
    fn shared_memory_can_be_marked_for_kernel_removal() {
        let mut memory = match SharedMemory::new(8) {
            Ok(memory) => memory,
            Err(error) => panic!("allocation failed: {error}"),
        };
        if let Err(error) = memory.request_removal() {
            panic!("removal request failed: {error}");
        }
    }
}
