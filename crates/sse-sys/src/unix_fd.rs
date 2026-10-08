//! Linux Unix-domain descriptor passing used by dependency-free protocol backends.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;

const SOL_SOCKET: i32 = 1;
const SCM_RIGHTS: i32 = 1;
const MSG_CMSG_CLOEXEC: i32 = 0x4000_0000;
const MSG_CTRUNC: i32 = 0x08;
const EINTR: i32 = 4;
const CONTROL_ALIGN: usize = 8;

#[repr(C)]
struct Iovec {
    base: *mut u8,
    len: usize,
}

#[repr(C)]
struct Msghdr {
    name: *mut u8,
    namelen: u32,
    iov: *mut Iovec,
    iovlen: usize,
    control: *mut u8,
    controllen: usize,
    flags: i32,
}

#[repr(C)]
struct Cmsghdr {
    len: usize,
    level: i32,
    kind: i32,
}

unsafe extern "C" {
    fn sendmsg(fd: i32, message: *const Msghdr, flags: i32) -> isize;
    fn recvmsg(fd: i32, message: *mut Msghdr, flags: i32) -> isize;
}

/// Sends one complete message carrying one SCM_RIGHTS descriptor.
///
/// # Errors
/// Retries EINTR. Any partial send is treated as a broken connection because ancillary data
/// cannot safely be replayed after only part of the associated byte message was accepted.
pub fn send_fd(stream: &UnixStream, bytes: &[u8], fd: RawFd) -> io::Result<()> {
    if bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SCM_RIGHTS requires at least one payload byte",
        ));
    }
    let mut iov = Iovec {
        base: bytes.as_ptr().cast_mut(),
        len: bytes.len(),
    };
    let data_offset = align_control(std::mem::size_of::<Cmsghdr>())?;
    let data_len = data_offset
        .checked_add(std::mem::size_of::<RawFd>())
        .ok_or_else(|| io::Error::other("SCM_RIGHTS size overflow"))?;
    let control_len = align_control(data_len)?;
    let words = control_len
        .checked_div(CONTROL_ALIGN)
        .ok_or_else(|| io::Error::other("SCM_RIGHTS control size is invalid"))?;
    let mut control = vec![0_u64; words];
    let control_bytes = control.as_mut_ptr().cast::<u8>();
    let header = control_bytes.cast::<Cmsghdr>();
    // SAFETY: Vec<u64> provides 8-byte alignment and enough live storage for cmsghdr + one RawFd.
    unsafe {
        (*header).len = data_len;
        (*header).level = SOL_SOCKET;
        (*header).kind = SCM_RIGHTS;
        std::ptr::write(control_bytes.add(data_offset).cast::<RawFd>(), fd);
    }
    let message = Msghdr {
        name: std::ptr::null_mut(),
        namelen: 0,
        iov: &mut iov,
        iovlen: 1,
        control: control_bytes,
        controllen: control_len,
        flags: 0,
    };
    let sent = loop {
        // SAFETY: message points only at initialized live payload/control storage for this synchronous call.
        let result = unsafe { sendmsg(stream.as_raw_fd(), &message, 0) };
        if result >= 0 {
            break result;
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(EINTR) {
            return Err(error);
        }
    };
    if usize::try_from(sent).ok() != Some(bytes.len()) {
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "partial sendmsg disconnected the descriptor-passing transport",
        ));
    }
    Ok(())
}

/// Receives bytes and at most one SCM_RIGHTS descriptor with close-on-exec set atomically.
///
/// # Errors
/// Retries EINTR, rejects truncated ancillary data and malformed SCM_RIGHTS headers.
pub fn recv_fd(stream: &UnixStream, bytes: &mut [u8]) -> io::Result<(usize, Option<OwnedFd>)> {
    if bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "recv_fd needs payload storage",
        ));
    }
    let data_offset = align_control(std::mem::size_of::<Cmsghdr>())?;
    let data_len = data_offset
        .checked_add(std::mem::size_of::<RawFd>())
        .ok_or_else(|| io::Error::other("SCM_RIGHTS size overflow"))?;
    let control_len = align_control(data_len)?;
    let words = control_len
        .checked_div(CONTROL_ALIGN)
        .ok_or_else(|| io::Error::other("SCM_RIGHTS control size is invalid"))?;
    let mut control = vec![0_u64; words];
    let mut iov = Iovec {
        base: bytes.as_mut_ptr(),
        len: bytes.len(),
    };
    let mut message = Msghdr {
        name: std::ptr::null_mut(),
        namelen: 0,
        iov: &mut iov,
        iovlen: 1,
        control: control.as_mut_ptr().cast(),
        controllen: control_len,
        flags: 0,
    };
    let received = loop {
        // SAFETY: message points at writable live payload/control buffers for this synchronous call.
        let result = unsafe { recvmsg(stream.as_raw_fd(), &mut message, MSG_CMSG_CLOEXEC) };
        if result >= 0 {
            break result;
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(EINTR) {
            return Err(error);
        }
    };
    if message.flags & MSG_CTRUNC != 0 {
        close_received_descriptors(&control, message.controllen);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SCM_RIGHTS control data was truncated",
        ));
    }
    let count = usize::try_from(received).map_err(|_| io::Error::other("negative recvmsg byte count"))?;
    if message.controllen == 0 {
        return Ok((count, None));
    }
    if message.controllen < data_len {
        close_received_descriptors(&control, message.controllen);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SCM_RIGHTS control header is truncated",
        ));
    }
    let header = control.as_ptr().cast::<Cmsghdr>();
    // SAFETY: controllen was checked to contain a complete aligned cmsghdr and one RawFd.
    let (header_len, level, kind) = unsafe { ((*header).len, (*header).level, (*header).kind) };
    if header_len != data_len
        || header_len > message.controllen
        || message.controllen > control_len
        || level != SOL_SOCKET
        || kind != SCM_RIGHTS
    {
        close_received_descriptors(&control, message.controllen);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected Unix ancillary message",
        ));
    }
    // SAFETY: validated cmsghdr length includes one aligned RawFd written by the kernel.
    let raw = unsafe { std::ptr::read(control.as_ptr().cast::<u8>().add(data_offset).cast::<RawFd>()) };
    if raw < 0 {
        close_received_descriptors(&control, message.controllen);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SCM_RIGHTS returned an invalid descriptor",
        ));
    }
    // SAFETY: SCM_RIGHTS transfers ownership of this newly installed descriptor to the receiver.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    Ok((count, Some(fd)))
}

fn close_received_descriptors(control: &[u64], controllen: usize) {
    let Some(capacity) = control.len().checked_mul(std::mem::size_of::<u64>()) else {
        return;
    };
    let available = controllen.min(capacity);
    let Ok(data_offset) = align_control(std::mem::size_of::<Cmsghdr>()) else {
        return;
    };
    let base = control.as_ptr().cast::<u8>();
    let mut offset = 0_usize;

    while let Some(remaining) = available.checked_sub(offset) {
        if remaining < std::mem::size_of::<Cmsghdr>() {
            break;
        }
        // SAFETY: a complete Cmsghdr lies within the initialized control bytes supplied by recvmsg.
        let header = unsafe { base.add(offset).cast::<Cmsghdr>() };
        // SAFETY: recvmsg initialized these fields; unaligned reads avoid relying on header alignment.
        let (header_len, level, kind) = unsafe {
            (
                std::ptr::read_unaligned(std::ptr::addr_of!((*header).len)),
                std::ptr::read_unaligned(std::ptr::addr_of!((*header).level)),
                std::ptr::read_unaligned(std::ptr::addr_of!((*header).kind)),
            )
        };
        if header_len < data_offset {
            break;
        }
        let Some(header_end) = offset.checked_add(header_len) else {
            break;
        };
        let visible_end = header_end.min(available);
        let Some(payload_start) = offset.checked_add(data_offset) else {
            break;
        };
        if level == SOL_SOCKET && kind == SCM_RIGHTS && visible_end >= payload_start {
            let descriptor_size = std::mem::size_of::<RawFd>();
            let Some(payload_size) = visible_end.checked_sub(payload_start) else {
                break;
            };
            let descriptor_count = payload_size.checked_div(descriptor_size).unwrap_or_default();
            for index in 0..descriptor_count {
                let Some(relative_offset) = index.checked_mul(descriptor_size) else {
                    break;
                };
                let Some(descriptor_offset) = payload_start.checked_add(relative_offset) else {
                    break;
                };
                // SAFETY: this complete RawFd lies inside a kernel-written SCM_RIGHTS payload.
                let raw = unsafe { std::ptr::read_unaligned(base.add(descriptor_offset).cast::<RawFd>()) };
                if raw >= 0 {
                    // SAFETY: recvmsg installed this descriptor for the current process; this closes it once.
                    drop(unsafe { OwnedFd::from_raw_fd(raw) });
                }
            }
        }
        let Ok(aligned_len) = align_control(header_len) else {
            break;
        };
        let Some(next) = offset.checked_add(aligned_len) else {
            break;
        };
        if next <= offset {
            break;
        }
        offset = next;
    }
}

fn align_control(value: usize) -> io::Result<usize> {
    let rounded = value
        .checked_add(CONTROL_ALIGN.saturating_sub(1))
        .ok_or_else(|| io::Error::other("SCM_RIGHTS alignment overflow"))?;
    Ok(rounded & !CONTROL_ALIGN.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::{align_control, recv_fd, send_fd, Cmsghdr, Iovec, Msghdr, SCM_RIGHTS, SOL_SOCKET};
    use crate::memfd::MappedFile;
    use std::{
        fs::{self, OpenOptions},
        io,
        os::{
            fd::{AsRawFd, RawFd},
            unix::net::UnixStream,
        },
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

    struct TempFile(PathBuf);

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn descriptors_for(path: &Path) -> io::Result<usize> {
        let mut count = 0_usize;
        for entry in fs::read_dir("/proc/self/fd")? {
            let entry = entry?;
            if fs::read_link(entry.path()).ok().as_deref() == Some(path) {
                count = count
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("descriptor count overflow"))?;
            }
        }
        Ok(count)
    }

    fn send_descriptors(stream: &UnixStream, fd: RawFd, count: usize) -> io::Result<()> {
        let mut payload = *b"fd";
        let mut iov = Iovec {
            base: payload.as_mut_ptr(),
            len: payload.len(),
        };
        let data_offset = align_control(std::mem::size_of::<Cmsghdr>())?;
        let fds_size = std::mem::size_of::<RawFd>()
            .checked_mul(count)
            .ok_or_else(|| io::Error::other("SCM_RIGHTS test size overflow"))?;
        let data_len = data_offset
            .checked_add(fds_size)
            .ok_or_else(|| io::Error::other("SCM_RIGHTS test size overflow"))?;
        let control_len = align_control(data_len)?;
        let words = control_len
            .checked_div(8)
            .ok_or_else(|| io::Error::other("SCM_RIGHTS test control size is invalid"))?;
        let mut control = vec![0_u64; words];
        let control_bytes = control.as_mut_ptr().cast::<u8>();
        // SAFETY: the aligned control buffer has space for one header and `count` RawFd values.
        unsafe {
            let header = control_bytes.cast::<Cmsghdr>();
            (*header).len = data_len;
            (*header).level = SOL_SOCKET;
            (*header).kind = SCM_RIGHTS;
            for index in 0..count {
                let offset = index
                    .checked_mul(std::mem::size_of::<RawFd>())
                    .and_then(|value| data_offset.checked_add(value))
                    .ok_or_else(|| io::Error::other("SCM_RIGHTS test offset overflow"))?;
                std::ptr::write(control_bytes.add(offset).cast::<RawFd>(), fd);
            }
        }
        let message = Msghdr {
            name: std::ptr::null_mut(),
            namelen: 0,
            iov: &mut iov,
            iovlen: 1,
            control: control_bytes,
            controllen: control_len,
            flags: 0,
        };
        loop {
            // SAFETY: message points only at initialized live payload and control storage.
            let sent = unsafe { super::sendmsg(stream.as_raw_fd(), &message, 0) };
            if sent >= 0 {
                if usize::try_from(sent).ok() == Some(payload.len()) {
                    return Ok(());
                }
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "partial test sendmsg"));
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(super::EINTR) {
                return Err(error);
            }
        }
    }

    #[test]
    fn passes_memfd_over_unix_stream_pair() -> std::io::Result<()> {
        let (sender, receiver) = UnixStream::pair()?;
        let mapping = MappedFile::new(4096)?;
        send_fd(&sender, b"fd", mapping.raw_fd())?;

        let mut payload = [0_u8; 8];
        let (received, descriptor) = recv_fd(&receiver, &mut payload)?;
        assert_eq!(payload.get(..received), Some(&b"fd"[..]));
        let file =
            std::fs::File::from(descriptor.ok_or_else(|| std::io::Error::other("SCM_RIGHTS descriptor missing"))?);
        assert_eq!(file.metadata()?.len(), 4096);
        Ok(())
    }

    #[test]
    fn truncated_control_data_closes_every_installed_descriptor() -> io::Result<()> {
        let serial = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("sse-unix-fd-ctrunc-{}-{serial}", std::process::id()));
        let _temp_file = TempFile(path.clone());
        let original = OpenOptions::new().create_new(true).read(true).write(true).open(&path)?;
        assert_eq!(descriptors_for(&path)?, 1);

        let (sender, receiver) = UnixStream::pair()?;
        send_descriptors(&sender, original.as_raw_fd(), 3)?;
        let mut payload = [0_u8; 8];
        let error = match recv_fd(&receiver, &mut payload) {
            Ok(_) => return Err(io::Error::other("truncated descriptors were unexpectedly accepted")),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(descriptors_for(&path)?, 1);
        Ok(())
    }

    #[test]
    fn rejects_second_rights_descriptor_and_closes_it() -> io::Result<()> {
        let serial = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("sse-unix-fd-multiple-{}-{serial}", std::process::id()));
        let _temp_file = TempFile(path.clone());
        let original = OpenOptions::new().create_new(true).read(true).write(true).open(&path)?;
        assert_eq!(descriptors_for(&path)?, 1);

        let (sender, receiver) = UnixStream::pair()?;
        send_descriptors(&sender, original.as_raw_fd(), 2)?;
        let mut payload = [0_u8; 8];
        let error = match recv_fd(&receiver, &mut payload) {
            Ok((_, descriptor)) => {
                drop(descriptor);
                return Err(io::Error::other("multiple descriptors were unexpectedly accepted"));
            }
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(descriptors_for(&path)?, 1);
        Ok(())
    }
}
