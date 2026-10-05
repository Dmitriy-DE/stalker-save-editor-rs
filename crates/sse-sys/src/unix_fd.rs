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
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SCM_RIGHTS control header is truncated",
        ));
    }
    let header = control.as_ptr().cast::<Cmsghdr>();
    // SAFETY: controllen was checked to contain a complete aligned cmsghdr and one RawFd.
    let (header_len, level, kind) = unsafe { ((*header).len, (*header).level, (*header).kind) };
    if header_len < data_len || header_len > message.controllen || level != SOL_SOCKET || kind != SCM_RIGHTS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected Unix ancillary message",
        ));
    }
    // SAFETY: validated cmsghdr length includes one aligned RawFd written by the kernel.
    let raw = unsafe { std::ptr::read(control.as_ptr().cast::<u8>().add(data_offset).cast::<RawFd>()) };
    if raw < 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SCM_RIGHTS returned an invalid descriptor",
        ));
    }
    // SAFETY: SCM_RIGHTS transfers ownership of this newly installed descriptor to the receiver.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    Ok((count, Some(fd)))
}

fn align_control(value: usize) -> io::Result<usize> {
    let rounded = value
        .checked_add(CONTROL_ALIGN.saturating_sub(1))
        .ok_or_else(|| io::Error::other("SCM_RIGHTS alignment overflow"))?;
    Ok(rounded & !CONTROL_ALIGN.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::{recv_fd, send_fd};
    use crate::memfd::MappedFile;
    use std::os::unix::net::UnixStream;

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
}
