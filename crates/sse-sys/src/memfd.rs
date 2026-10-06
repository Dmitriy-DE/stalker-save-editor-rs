//! Linux anonymous file-backed mapping used by Wayland wl_shm buffers.

use std::ffi::{c_char, c_int, c_void};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::ptr::NonNull;

const PROT_READ: c_int = 1;
const PROT_WRITE: c_int = 2;
const MAP_SHARED: c_int = 1;
const MFD_CLOEXEC: u32 = 1;
const MFD_ALLOW_SEALING: u32 = 2;
const F_ADD_SEALS: c_int = 1033;
const F_SEAL_SHRINK: c_int = 0x0002;

unsafe extern "C" {
    fn memfd_create(name: *const c_char, flags: u32) -> c_int;
    fn ftruncate(fd: c_int, length: i64) -> c_int;
    fn fcntl(fd: c_int, command: c_int, ...) -> c_int;
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
        if len == 0 || i64::try_from(len).is_err() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid mapping length"));
        }
        let name = b"sse-wayland\0";
        // SAFETY: name is NUL terminated and flags are defined by Linux memfd_create.
        let raw = unsafe { memfd_create(name.as_ptr().cast(), MFD_CLOEXEC | MFD_ALLOW_SEALING) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful memfd_create returns one owned descriptor.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let length =
            i64::try_from(len).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "mapping too large"))?;
        // SAFETY: descriptor is live and length is validated.
        if unsafe { ftruncate(fd.as_raw_fd(), length) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: descriptor is live and was created with MFD_ALLOW_SEALING; the seal prevents later shrinking.
        if unsafe { fcntl(fd.as_raw_fd(), F_ADD_SEALS, F_SEAL_SHRINK) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: kernel chooses the address; result is checked against MAP_FAILED below.
        let address = unsafe {
            mmap(
                std::ptr::null_mut(),
                len,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        if address as isize == -1 {
            return Err(io::Error::last_os_error());
        }
        let pointer = NonNull::new(address.cast()).ok_or_else(|| io::Error::other("mmap returned null"))?;
        Ok(Self { fd, pointer, len })
    }

    /// Descriptor passed to wl_shm.create_pool with SCM_RIGHTS.
    #[must_use]
    pub fn raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// Copies a native little-endian ARGB frame into the mapping.
    pub fn write_u32_le(&mut self, pixels: &[u32]) -> io::Result<()> {
        let bytes = pixels
            .len()
            .checked_mul(4)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "pixel size overflow"))?;
        if bytes > self.len {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "frame exceeds mapping"));
        }
        let target = self.pointer.as_ptr();
        let mut offset = 0_usize;
        for pixel in pixels {
            let pixel_bytes = pixel.to_le_bytes();
            let end = offset
                .checked_add(pixel_bytes.len())
                .filter(|end| *end <= self.len)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "frame exceeds mapping"))?;
            // SAFETY: checked total length bounds every four-byte offset in this loop, the mapping is live,
            // and the stack byte array does not overlap the mapped destination. No Rust mutable slice aliases it.
            unsafe {
                std::ptr::copy_nonoverlapping(pixel_bytes.as_ptr(), target.add(offset), pixel_bytes.len());
            }
            offset = end;
        }
        Ok(())
    }
}

impl Drop for MappedFile {
    fn drop(&mut self) {
        // SAFETY: this is the mapping returned by mmap and is unmapped exactly once.
        unsafe {
            let _ = munmap(self.pointer.as_ptr().cast(), self.len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fcntl, MappedFile, F_SEAL_SHRINK};
    use std::os::fd::AsRawFd;

    const F_GET_SEALS: std::ffi::c_int = 1034;

    #[test]
    fn memfd_is_sealed_against_shrinking() -> std::io::Result<()> {
        let mapping = MappedFile::new(16)?;
        // SAFETY: the mapping owns a live memfd descriptor and F_GET_SEALS does not mutate it.
        let seals = unsafe { fcntl(mapping.fd.as_raw_fd(), F_GET_SEALS) };
        assert!(seals >= 0, "F_GET_SEALS failed: {}", std::io::Error::last_os_error());
        assert_ne!(seals & F_SEAL_SHRINK, 0);
        Ok(())
    }

    #[test]
    fn writes_little_endian_pixels_inside_the_mapping() -> std::io::Result<()> {
        let mut mapping = MappedFile::new(8)?;
        mapping.write_u32_le(&[0x1234_5678])?;
        // SAFETY: the mapping is live, the inspected range is within its 8-byte length, and no mutable reference exists.
        let bytes = unsafe { std::slice::from_raw_parts(mapping.pointer.as_ptr(), mapping.len) };
        assert_eq!(bytes, &[0x78, 0x56, 0x34, 0x12, 0, 0, 0, 0]);
        Ok(())
    }
}
