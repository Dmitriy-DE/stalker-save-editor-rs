//! Linux Unix-domain descriptor passing used by dependency-free protocol backends.
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
const SOL_SOCKET:i32=1; const SCM_RIGHTS:i32=1;
#[repr(C)] struct Iovec{base:*const u8,len:usize}
#[repr(C)] struct Msghdr{name:*mut u8,namelen:u32,iov:*mut Iovec,iovlen:usize,control:*mut u8,controllen:usize,flags:i32}
#[repr(C)] struct Cmsghdr{len:usize,level:i32,kind:i32}
unsafe extern "C"{fn sendmsg(fd:i32,message:*const Msghdr,flags:i32)->isize;}
/// Sends one complete message carrying one SCM_RIGHTS descriptor.
pub fn send_fd(stream:&UnixStream,bytes:&[u8],fd:RawFd)->io::Result<()> {
 let mut iov=Iovec{base:bytes.as_ptr(),len:bytes.len()}; let header=std::mem::size_of::<Cmsghdr>(); let fd_size=std::mem::size_of::<RawFd>(); let align=std::mem::size_of::<usize>();
 let unaligned=header.checked_add(fd_size).and_then(|v|v.checked_add(align.saturating_sub(1))).ok_or_else(||io::Error::other("SCM_RIGHTS size overflow"))?; let control_len=unaligned & !align.saturating_sub(1); let mut control=vec![0_u8;control_len]; let hdr=control.as_mut_ptr().cast::<Cmsghdr>();
 // SAFETY: control is sized for cmsghdr plus one RawFd and remains live through sendmsg.
 unsafe{(*hdr).len=header.saturating_add(fd_size);(*hdr).level=SOL_SOCKET;(*hdr).kind=SCM_RIGHTS;std::ptr::write_unaligned(control.as_mut_ptr().add(header).cast::<RawFd>(),fd);}
 let msg=Msghdr{name:std::ptr::null_mut(),namelen:0,iov:&mut iov,iovlen:1,control:control.as_mut_ptr(),controllen:control.len(),flags:0};
 // SAFETY: msg points only at initialized live storage for this call.
 let sent=unsafe{sendmsg(stream.as_raw_fd(),&msg,0)}; if sent<0{return Err(io::Error::last_os_error());} if usize::try_from(sent).ok()!=Some(bytes.len()){return Err(io::Error::new(io::ErrorKind::WriteZero,"short sendmsg"));} Ok(())
}
