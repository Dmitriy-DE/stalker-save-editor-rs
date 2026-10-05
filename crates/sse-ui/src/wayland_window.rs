//! Minimal dependency-free Wayland client for the editor surface.
//!
//! It speaks the core Wayland wire protocol over the compositor Unix socket.  The compositor,
//! xdg-shell and wl_shm objects are discovered from wl_registry; pixels live in a memfd mapping.

use crate::event_loop::{Present, Proxy, WindowEvent};
use crate::raster::Rect;
use sse_core::{Error, Result};
use sse_sys::memfd::MappedFile;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::fd::RawFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const WL_DISPLAY: u32 = 1;
const WL_REGISTRY: u32 = 2;
const WL_SHM_FORMAT_XRGB8888: u32 = 1;
const WL_SHM_FORMAT_ARGB8888: u32 = 0;
const SOL_SOCKET: i32 = 1;
const SCM_RIGHTS: i32 = 1;

#[repr(C)]
struct Iovec { base: *const u8, len: usize }
#[repr(C)]
struct Msghdr { name: *mut u8, namelen: u32, iov: *mut Iovec, iovlen: usize, control: *mut u8, controllen: usize, flags: i32 }
#[repr(C)]
struct Cmsghdr { len: usize, level: i32, kind: i32 }
unsafe extern "C" { fn sendmsg(fd: i32, message: *const Msghdr, flags: i32) -> isize; }

#[derive(Clone, Copy, Default)]
struct Globals { compositor: Option<(u32,u32)>, shm: Option<(u32,u32)>, wm_base: Option<(u32,u32)>, seat: Option<(u32,u32)> }

/// Native Wayland presenter. Input support currently uses wl_seat discovery and falls back to X11 when the
/// compositor cannot provide the required core objects.
pub struct WaylandWindow {
    stream: UnixStream,
    surface: u32,
    buffer: u32,
    pool: u32,
    memory: MappedFile,
    size: (u32,u32),
    closed: Arc<Mutex<bool>>,
}

impl WaylandWindow {
    /// Opens a top-level xdg-shell surface on the current Wayland display.
    pub fn open<U: Send + 'static>(title: &str, width: u32, height: u32, proxy: Proxy<U>) -> Result<Self> {
        let mut stream = connect()?;
        send(&mut stream, WL_DISPLAY, 1, &u32s(&[WL_REGISTRY]))?; // get_registry
        send(&mut stream, WL_DISPLAY, 0, &u32s(&[3]))?; // sync callback id 3
        let mut globals = Globals::default();
        read_registry_until_done(&mut stream, 3, &mut globals)?;
        let compositor = bind(&mut stream, globals.compositor, "wl_compositor", 4, 4)?;
        let shm = bind(&mut stream, globals.shm, "wl_shm", 1, 5)?;
        let wm = bind(&mut stream, globals.wm_base, "xdg_wm_base", 1, 6)?;
        if let Some(seat) = globals.seat { let _ = bind(&mut stream, Some(seat), "wl_seat", seat.1.min(7), 7)?; }

        let surface = 8;
        send(&mut stream, compositor, 0, &u32s(&[surface]))?;
        let xdg_surface = 9;
        send(&mut stream, wm, 2, &u32s(&[xdg_surface, surface]))?;
        let toplevel = 10;
        send(&mut stream, xdg_surface, 1, &u32s(&[toplevel]))?;
        send_string(&mut stream, toplevel, 2, title)?;
        send_string(&mut stream, toplevel, 3, "stalker-save-editor")?;
        send(&mut stream, surface, 6, &[])?; // initial commit
        stream.flush().map_err(io)?;

        let byte_len = frame_bytes(width, height)?;
        let memory = MappedFile::new(byte_len).map_err(io)?;
        let pool = 11;
        send_with_fd(&stream, shm, 0, &u32s(&[pool, u32::try_from(byte_len).map_err(|_| Error::Refused("Wayland buffer too large".to_owned()))?]), memory.raw_fd())?;
        let buffer = 12;
        let stride = width.checked_mul(4).ok_or_else(|| Error::Refused("Wayland stride overflow".to_owned()))?;
        send(&mut stream, pool, 0, &u32s(&[buffer, 0, width, height, stride, WL_SHM_FORMAT_XRGB8888]))?;
        stream.flush().map_err(io)?;

        let closed = Arc::new(Mutex::new(false));
        let reader = stream.try_clone().map_err(io)?;
        let reader_closed = Arc::clone(&closed);
        std::thread::Builder::new().name("wayland-events".to_owned()).spawn(move || event_reader(reader, xdg_surface, toplevel, wm, proxy, reader_closed)).map_err(io)?;
        Ok(Self { stream, surface, buffer, pool, memory, size:(width,height), closed })
    }
}

impl Present for WaylandWindow {
    fn present(&mut self, frame: &[u32], stride: usize, width: u32, height: u32, rects: &[Rect]) -> Result<()> {
        if rects.is_empty() { return Ok(()); }
        if (width,height) != self.size || stride != usize::try_from(width).unwrap_or(0) {
            return Err(Error::Refused("Wayland resize requires buffer recreation; using XWayland fallback is recommended".to_owned()));
        }
        self.memory.write_u32_le(frame).map_err(io)?;
        send(&mut self.stream, self.surface, 1, &u32s(&[self.buffer, 0, 0]))?; // attach
        for rect in rects {
            send(&mut self.stream, self.surface, 2, &i32s(&[rect.x, rect.y, i32::try_from(rect.width).unwrap_or(i32::MAX), i32::try_from(rect.height).unwrap_or(i32::MAX)]))?;
        }
        send(&mut self.stream, self.surface, 6, &[])?;
        self.stream.flush().map_err(io)
    }
}

impl Drop for WaylandWindow {
    fn drop(&mut self) {
        let _ = send(&mut self.stream, self.buffer, 0, &[]);
        let _ = send(&mut self.stream, self.pool, 1, &[]);
        let _ = self.closed.lock().map(|mut value| *value = true);
    }
}

fn connect() -> Result<UnixStream> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| Error::Refused("XDG_RUNTIME_DIR is not set".to_owned()))?;
    let display = std::env::var_os("WAYLAND_DISPLAY").ok_or_else(|| Error::Refused("WAYLAND_DISPLAY is not set".to_owned()))?;
    let path = PathBuf::from(runtime).join(display);
    UnixStream::connect(path).map_err(io)
}

fn event_reader<U: Send + 'static>(mut stream: UnixStream, xdg_surface:u32, toplevel:u32, wm:u32, proxy:Proxy<U>, closed:Arc<Mutex<bool>>) {
    loop {
        if closed.lock().map(|v| *v).unwrap_or(true) { return; }
        let Ok((object, opcode, payload)) = read_message(&mut stream) else { let _=proxy.send_window(WindowEvent::Disconnected); return; };
        if object == xdg_surface && opcode == 0 && payload.len() >= 4 {
            if let Some(serial)=read_u32(&payload,0) { let _=send(&mut stream, xdg_surface, 4, &u32s(&[serial])); let _=stream.flush(); }
        } else if object == toplevel && opcode == 1 {
            let _=proxy.send_window(WindowEvent::CloseRequested);
        } else if object == wm && opcode == 0 && payload.len() >= 4 {
            if let Some(serial)=read_u32(&payload,0) { let _=send(&mut stream, wm, 3, &u32s(&[serial])); let _=stream.flush(); }
        }
    }
}

fn bind(stream:&mut UnixStream, global:Option<(u32,u32)>, interface:&str, max_version:u32, id:u32)->Result<u32>{
    let (name,version)=global.ok_or_else(|| Error::Refused(format!("Wayland compositor has no {interface}")))?;
    let version=version.min(max_version);
    let mut body=u32s(&[name]); body.extend_from_slice(&wire_string(interface)); body.extend_from_slice(&u32s(&[version,id]));
    send(stream,WL_REGISTRY,0,&body)?; Ok(id)
}

fn read_registry_until_done(stream:&mut UnixStream, callback:u32, globals:&mut Globals)->Result<()> {
    loop { let (object,opcode,payload)=read_message(stream)?; if object==callback && opcode==0 { return Ok(()); }
        if object==WL_REGISTRY && opcode==0 { if let Some((name,iface,version))=parse_global(&payload) { match iface.as_str(){
            "wl_compositor"=>globals.compositor=Some((name,version)), "wl_shm"=>globals.shm=Some((name,version)), "xdg_wm_base"=>globals.wm_base=Some((name,version)), "wl_seat"=>globals.seat=Some((name,version)), _=>{} } } }
    }
}

fn parse_global(payload:&[u8])->Option<(u32,String,u32)>{ let name=read_u32(payload,0)?; let len=usize::try_from(read_u32(payload,4)?).ok()?; let end=8usize.checked_add(align4(len))?; let raw=payload.get(8..8usize.checked_add(len)?.saturating_sub(1))?; let iface=String::from_utf8(raw.to_vec()).ok()?; Some((name,iface,read_u32(payload,end)?)) }
fn read_message(stream:&mut UnixStream)->Result<(u32,u16,Vec<u8>)>{ let mut head=[0u8;8]; stream.read_exact(&mut head).map_err(io)?; let object=u32::from_ne_bytes(head[..4].try_into().map_err(|_|Error::damaged("Wayland header"))?); let word=u32::from_ne_bytes(head[4..].try_into().map_err(|_|Error::damaged("Wayland header"))?); let size=usize::try_from(word>>16).map_err(|_|Error::damaged("Wayland size"))?; if size<8||size>1024*1024{return Err(Error::damaged("invalid Wayland message size"));} let mut payload=vec![0u8;size-8]; stream.read_exact(&mut payload).map_err(io)?; Ok((object,(word&0xffff) as u16,payload)) }
fn send(stream:&mut UnixStream,object:u32,opcode:u16,payload:&[u8])->Result<()> { let size=8usize.checked_add(payload.len()).ok_or_else(||Error::damaged("Wayland message overflow"))?; let word=(u32::try_from(size).map_err(|_|Error::damaged("Wayland message too large"))?<<16)|u32::from(opcode); stream.write_all(&object.to_ne_bytes()).map_err(io)?; stream.write_all(&word.to_ne_bytes()).map_err(io)?; stream.write_all(payload).map_err(io) }
fn send_string(stream:&mut UnixStream,object:u32,opcode:u16,value:&str)->Result<()> { send(stream,object,opcode,&wire_string(value)) }
fn send_with_fd(stream:&UnixStream,object:u32,opcode:u16,payload:&[u8],fd:RawFd)->Result<()> { let size=8usize.checked_add(payload.len()).ok_or_else(||Error::damaged("Wayland fd message overflow"))?; let word=(u32::try_from(size).map_err(|_|Error::damaged("Wayland message too large"))?<<16)|u32::from(opcode); let mut bytes=Vec::with_capacity(size); bytes.extend_from_slice(&object.to_ne_bytes());bytes.extend_from_slice(&word.to_ne_bytes());bytes.extend_from_slice(payload); send_fd(stream, &bytes, fd).map_err(io) }
fn send_fd(stream:&UnixStream,bytes:&[u8],fd:RawFd)->std::io::Result<()> { let mut iov=Iovec{base:bytes.as_ptr(),len:bytes.len()}; let header_size=std::mem::size_of::<Cmsghdr>(); let align=std::mem::size_of::<usize>(); let control_len=(header_size+std::mem::size_of::<RawFd>()+align-1)&!(align-1); let mut control=vec![0u8;control_len]; let hdr=control.as_mut_ptr().cast::<Cmsghdr>(); unsafe{(*hdr).len=header_size+std::mem::size_of::<RawFd>();(*hdr).level=SOL_SOCKET;(*hdr).kind=SCM_RIGHTS;std::ptr::write_unaligned(control.as_mut_ptr().add(header_size).cast::<RawFd>(),fd);} let msg=Msghdr{name:std::ptr::null_mut(),namelen:0,iov:&mut iov,iovlen:1,control:control.as_mut_ptr(),controllen:control.len(),flags:0}; let sent=unsafe{sendmsg(stream.as_raw_fd(),&msg,0)}; if sent<0{return Err(std::io::Error::last_os_error());} if usize::try_from(sent).ok()!=Some(bytes.len()){return Err(std::io::Error::new(std::io::ErrorKind::WriteZero,"short Wayland sendmsg"));} Ok(()) }
fn wire_string(value:&str)->Vec<u8>{ let len=value.len().saturating_add(1); let mut out=u32s(&[u32::try_from(len).unwrap_or(u32::MAX)]);out.extend_from_slice(value.as_bytes());out.push(0);out.resize(out.len().saturating_add((4-out.len()%4)%4),0);out }
fn u32s(values:&[u32])->Vec<u8>{values.iter().flat_map(|v|v.to_ne_bytes()).collect()}
fn i32s(values:&[i32])->Vec<u8>{values.iter().flat_map(|v|v.to_ne_bytes()).collect()}
fn read_u32(bytes:&[u8],offset:usize)->Option<u32>{Some(u32::from_ne_bytes(bytes.get(offset..offset+4)?.try_into().ok()?))}
fn align4(value:usize)->usize{value.saturating_add(3)&!3}
fn frame_bytes(width:u32,height:u32)->Result<usize>{usize::try_from(width).ok().and_then(|w|usize::try_from(height).ok().and_then(|h|w.checked_mul(h))).and_then(|p|p.checked_mul(4)).filter(|n|*n>0&&*n<=128*1024*1024).ok_or_else(||Error::Refused("Wayland frame size outside limit".to_owned()))}
fn io(error:std::io::Error)->Error{Error::System(format!("Wayland: {error}"))}

#[cfg(test)] mod tests { use super::{parse_global,wire_string}; #[test] fn registry_global_decodes(){let mut p=1u32.to_ne_bytes().to_vec();p.extend_from_slice(&wire_string("wl_compositor"));p.extend_from_slice(&4u32.to_ne_bytes());assert_eq!(parse_global(&p),Some((1,"wl_compositor".to_owned(),4)));} }
