//! Linux anonymous file-backed mapping used by Wayland wl_shm buffers.

use std::ffi::{c_char, c_int, c_void};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::ptr::NonNull;

const PROT_READ: c_int = 1;
const PROT_WRITE: c_int = 2;
const MAP_SHARED: c_int = 1;
const MFD_CLOEXEC: u32 = 1;

unsafe extern "C" {
    fn memfd_create(name: *const c_char, flags: u32) -> c_int;
    fn ftruncate(fd: c_int, length: i64) -> c_int;
    fn mmap(address: *mut c_void, length: usize, prot: c_int, flags: c_int, fd: c_int, offset: i64) -> *mut c_void;
    fn munmap(address: *mut c_void, length: usize) -> c_int;
}

/// Anonymous file descriptor plus a writable shared mapping.
pub struct MappedFile {
    fd: OwnedFd,
    pointer: NonNull<u8>,
    len: usize,
}

impl MappedFile {
    /// Creates an owner-private anonymous file and maps it read/write.
    pub fn new(len: usize) -> io::Result<Self> {
        if len == 0 || len > i64::MAX as usize { return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid mapping length")); }
        let name = b"sse-wayland\0";
        // SAFETY: name is NUL terminated and flags are defined by Linux memfd_create.
        let raw = unsafe { memfd_create(name.as_ptr().cast(), MFD_CLOEXEC) };
        if raw < 0 { return Err(io::Error::last_os_error()); }
        // SAFETY: successful memfd_create returns one owned descriptor.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let length = i64::try_from(len).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "mapping too large"))?;
        // SAFETY: descriptor is live and length is validated.
        if unsafe { ftruncate(fd.as_raw_fd(), length) } != 0 { return Err(io::Error::last_os_error()); }
        // SAFETY: kernel chooses the address; result is checked against MAP_FAILED below.
        let address = unsafe { mmap(std::ptr::null_mut(), len, PROT_READ | PROT_WRITE, MAP_SHARED, fd.as_raw_fd(), 0) };
        if address as isize == -1 { return Err(io::Error::last_os_error()); }
        let pointer = NonNull::new(address.cast()).ok_or_else(|| io::Error::other("mmap returned null"))?;
        Ok(Self { fd, pointer, len })
    }

    /// Descriptor passed to wl_shm.create_pool with SCM_RIGHTS.
    #[must_use]
    pub fn raw_fd(&self) -> RawFd { self.fd.as_raw_fd() }

    /// Copies a native little-endian ARGB frame into the mapping.
    pub fn write_u32_le(&mut self, pixels: &[u32]) -> io::Result<()> {
        let bytes = pixels.len().checked_mul(4).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "pixel size overflow"))?;
        if bytes > self.len { return Err(io::Error::new(io::ErrorKind::InvalidInput, "frame exceeds mapping")); }
        // SAFETY: mapping is live and bounds checked above.
        let target = unsafe { std::slice::from_raw_parts_mut(self.pointer.as_ptr(), bytes) };
        for (chunk, pixel) in target.chunks_exact_mut(4).zip(pixels) { chunk.copy_from_slice(&pixel.to_le_bytes()); }
        Ok(())
    }
}

impl Drop for MappedFile {
    fn drop(&mut self) {
        // SAFETY: this is the mapping returned by mmap and is unmapped exactly once.
        unsafe { let _ = munmap(self.pointer.as_ptr().cast(), self.len); }
    }
}
