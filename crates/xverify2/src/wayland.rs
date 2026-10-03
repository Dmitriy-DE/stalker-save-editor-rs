//! Pure-`std` Wayland client-side wire marshalling and the small XKB subset used by the application.

use sse_core::{Error, Result};
use std::collections::{HashMap, HashSet};

const HEADER_BYTES: usize = 8;
const MAX_MESSAGE_BYTES: usize = u16::MAX as usize;
const CLIENT_ID_MAX: u32 = 0xfeff_ffff;
const MAX_STRING_BYTES: usize = 1 << 20;
const MAX_ARRAY_BYTES: usize = 16 << 20;

/// An opaque file-descriptor handle transported by `sse-sys`.
pub type FdHandle = u32;

/// Minimal transport contract. Socket and real descriptor ownership live outside this module.
pub trait Transport {
    /// Sends one complete Wayland message and the file descriptors attached to it.
    fn send(&mut self, bytes: &[u8], fds: &[FdHandle]) -> Result<()>;
    /// Receives bytes and descriptor handles. Returning zero means no complete transport data is available yet.
    fn receive(&mut self, bytes: &mut Vec<u8>, fds: &mut Vec<FdHandle>) -> Result<usize>;
}

/// A Wayland protocol object id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectId(u32);

impl ObjectId {
    /// Creates an object id after checking that zero is not used.
    pub fn new(value: u32) -> Result<Self> {
        if value == 0 { return Err(Error::damaged("Wayland object id 0 is invalid")); }
        Ok(Self(value))
    }
    /// Numeric wire value.
    #[must_use]
    pub const fn get(self) -> u32 { self.0 }
}

/// 24.8 Wayland fixed-point value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fixed(i32);

impl Fixed {
    /// Creates a fixed value from its raw 24.8 representation.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self { Self(raw) }
    /// Raw 24.8 representation.
    #[must_use]
    pub const fn raw(self) -> i32 { self.0 }
}

/// Known interfaces whose events this module can decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interface {
    /// `wl_display`.
    Display,
    /// `wl_registry`.
    Registry,
    /// `wl_compositor`.
    Compositor,
    /// `wl_surface`.
    Surface,
    /// `wl_callback`.
    Callback,
    /// `wl_shm`.
    Shm,
    /// `wl_shm_pool`.
    ShmPool,
    /// `wl_buffer`.
    Buffer,
    /// `xdg_wm_base`.
    XdgWmBase,
    /// `xdg_surface`.
    XdgSurface,
    /// `xdg_toplevel`.
    XdgToplevel,
    /// `zxdg_decoration_manager_v1`.
    DecorationManager,
    /// `zxdg_toplevel_decoration_v1`.
    ToplevelDecoration,
    /// `wp_fractional_scale_manager_v1`.
    FractionalScaleManager,
    /// `wp_fractional_scale_v1`.
    FractionalScale,
    /// `wl_seat`.
    Seat,
    /// `wl_keyboard`.
    Keyboard,
    /// `wl_pointer`.
    Pointer,
    /// `wl_data_device_manager`.
    DataDeviceManager,
    /// `wl_data_device`.
    DataDevice,
    /// `wl_data_offer`.
    DataOffer,
    /// `wl_data_source`.
    DataSource,
}

/// A decoded wire message before interface-specific event interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Sender object.
    pub object: ObjectId,
    /// Event or request opcode.
    pub opcode: u16,
    /// Message payload excluding the eight-byte header.
    pub payload: Vec<u8>,
    /// Descriptor handles delivered with the message.
    pub fds: Vec<FdHandle>,
}

/// Events used by the UI/runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Registry advertised a global.
    RegistryGlobal { name: u32, interface: String, version: u32 },
    /// Registry removed a global.
    RegistryGlobalRemove { name: u32 },
    /// Callback completed.
    CallbackDone { callback_data: u32 },
    /// `xdg_wm_base.ping`.
    XdgPing { serial: u32 },
    /// `xdg_surface.configure`.
    XdgSurfaceConfigure { serial: u32 },
    /// `xdg_toplevel.configure`.
    XdgToplevelConfigure { width: i32, height: i32, states: Vec<u8> },
    /// Compositor requested that the toplevel close.
    XdgToplevelClose,
    /// Fractional scale preference in units of 1/120.
    FractionalScalePreferred { scale_120: u32 },
    /// Seat capabilities bitset.
    SeatCapabilities { capabilities: u32 },
    /// Seat name.
    SeatName { name: String },
    /// Keyboard keymap fd and byte size.
    KeyboardKeymap { format: u32, fd: FdHandle, size: u32 },
    /// Keyboard focus entered a surface.
    KeyboardEnter { serial: u32, surface: ObjectId, keys: Vec<u8> },
    /// Keyboard focus left a surface.
    KeyboardLeave { serial: u32, surface: ObjectId },
    /// Key press or release.
    KeyboardKey { serial: u32, time: u32, key: u32, state: u32 },
    /// Keyboard modifier masks and group.
    KeyboardModifiers { serial: u32, depressed: u32, latched: u32, locked: u32, group: u32 },
    /// Keyboard repeat information.
    KeyboardRepeatInfo { rate: i32, delay: i32 },
    /// Pointer entered a surface.
    PointerEnter { serial: u32, surface: ObjectId, x: Fixed, y: Fixed },
    /// Pointer moved.
    PointerMotion { time: u32, x: Fixed, y: Fixed },
    /// Pointer button changed.
    PointerButton { serial: u32, time: u32, button: u32, state: u32 },
    /// Pointer axis changed.
    PointerAxis { time: u32, axis: u32, value: Fixed },
    /// Data device announced an offer object.
    DataOffer { offer: ObjectId },
    /// Clipboard selection changed.
    DataSelection { offer: Option<ObjectId> },
    /// A data offer advertised a MIME type.
    DataOfferMime { mime: String },
    /// Event not interpreted by the high-level subset.
    Unknown(Message),
}

/// Allocates client-created Wayland object ids and permits explicit release.
#[derive(Debug, Clone)]
pub struct ObjectIds {
    next: u32,
    recycled: Vec<u32>,
    live: HashSet<u32>,
}

impl Default for ObjectIds {
    fn default() -> Self { Self { next: 2, recycled: Vec::new(), live: HashSet::new() } }
}

impl ObjectIds {
    /// Allocates a new live client id.
    pub fn allocate(&mut self) -> Result<ObjectId> {
        let value = if let Some(value) = self.recycled.pop() {
            value
        } else {
            if self.next > CLIENT_ID_MAX { return Err(Error::Refused("Wayland client object id space exhausted".to_owned())); }
            let value = self.next;
            self.next = self.next.checked_add(1).ok_or_else(|| Error::damaged("Wayland object id overflow"))?;
            value
        };
        self.live.insert(value);
        ObjectId::new(value)
    }

    /// Releases a client id after a protocol delete/destructor path.
    pub fn release(&mut self, id: ObjectId) {
        if self.live.remove(&id.get()) { self.recycled.push(id.get()); }
    }
}

/// One encoded request before transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedRequest {
    /// Complete header plus payload.
    pub bytes: Vec<u8>,
    /// Opaque descriptor handles in protocol order.
    pub fds: Vec<FdHandle>,
}

/// Wayland argument encoder.
#[derive(Debug)]
pub struct Encoder {
    object: ObjectId,
    opcode: u16,
    payload: Vec<u8>,
    fds: Vec<FdHandle>,
}

impl Encoder {
    /// Starts one request.
    #[must_use]
    pub fn new(object: ObjectId, opcode: u16) -> Self { Self { object, opcode, payload: Vec::new(), fds: Vec::new() } }
    /// Appends a signed integer.
    pub fn int(&mut self, value: i32) { self.payload.extend_from_slice(&value.to_ne_bytes()); }
    /// Appends an unsigned integer.
    pub fn uint(&mut self, value: u32) { self.payload.extend_from_slice(&value.to_ne_bytes()); }
    /// Appends a fixed-point value.
    pub fn fixed(&mut self, value: Fixed) { self.int(value.raw()); }
    /// Appends an object; `None` is the protocol null object.
    pub fn object(&mut self, value: Option<ObjectId>) { self.uint(value.map_or(0, ObjectId::get)); }
    /// Appends a newly allocated object id.
    pub fn new_id(&mut self, value: ObjectId) { self.uint(value.get()); }
    /// Appends a UTF-8 string with terminating NUL and four-byte padding.
    pub fn string(&mut self, value: &str) -> Result<()> {
        if value.as_bytes().contains(&0) { return Err(Error::damaged("Wayland strings cannot contain NUL")); }
        let length = value.len().checked_add(1).ok_or_else(|| Error::damaged("Wayland string length overflow"))?;
        if length > MAX_STRING_BYTES { return Err(Error::Refused("Wayland string exceeds configured limit".to_owned())); }
        self.uint(u32::try_from(length).map_err(|_| Error::damaged("Wayland string length does not fit u32"))?);
        self.payload.extend_from_slice(value.as_bytes());
        self.payload.push(0);
        pad_four(&mut self.payload)?;
        Ok(())
    }
    /// Appends an arbitrary byte array and four-byte padding.
    pub fn array(&mut self, value: &[u8]) -> Result<()> {
        if value.len() > MAX_ARRAY_BYTES { return Err(Error::Refused("Wayland array exceeds configured limit".to_owned())); }
        self.uint(u32::try_from(value.len()).map_err(|_| Error::damaged("Wayland array length does not fit u32"))?);
        self.payload.extend_from_slice(value);
        pad_four(&mut self.payload)?;
        Ok(())
    }
    /// Attaches an opaque descriptor handle; descriptors do not consume wire bytes.
    pub fn fd(&mut self, fd: FdHandle) { self.fds.push(fd); }
    /// Finalises the request header.
    pub fn finish(self) -> Result<EncodedRequest> {
        let size = HEADER_BYTES.checked_add(self.payload.len()).ok_or_else(|| Error::damaged("Wayland message size overflow"))?;
        if size > MAX_MESSAGE_BYTES || size.rem_euclid(4) != 0 { return Err(Error::Refused("Wayland request exceeds 16-bit wire size".to_owned())); }
        let size_u32 = u32::try_from(size).map_err(|_| Error::damaged("Wayland message size conversion failed"))?;
        let word = size_u32.checked_shl(16).unwrap_or_default() | u32::from(self.opcode);
        let mut bytes = Vec::with_capacity(size);
        bytes.extend_from_slice(&self.object.get().to_ne_bytes());
        bytes.extend_from_slice(&word.to_ne_bytes());
        bytes.extend_from_slice(&self.payload);
        Ok(EncodedRequest { bytes, fds: self.fds })
    }
}

/// Decoder over one Wayland message payload.
#[derive(Debug, Clone)]
pub struct Decoder<'a> {
    bytes: &'a [u8],
    at: usize,
    fds: &'a [FdHandle],
    fd_at: usize,
}

impl<'a> Decoder<'a> {
    /// Creates a decoder over payload bytes and attached descriptors.
    #[must_use]
    pub fn new(bytes: &'a [u8], fds: &'a [FdHandle]) -> Self { Self { bytes, at: 0, fds, fd_at: 0 } }
    /// Reads an `int`.
    pub fn int(&mut self) -> Result<i32> { Ok(i32::from_ne_bytes(self.take_array()?)) }
    /// Reads a `uint`.
    pub fn uint(&mut self) -> Result<u32> { Ok(u32::from_ne_bytes(self.take_array()?)) }
    /// Reads a fixed-point value.
    pub fn fixed(&mut self) -> Result<Fixed> { self.int().map(Fixed::from_raw) }
    /// Reads an object id; zero is returned as `None`.
    pub fn object(&mut self) -> Result<Option<ObjectId>> { let value=self.uint()?; if value==0 { Ok(None) } else { ObjectId::new(value).map(Some) } }
    /// Reads a new id.
    pub fn new_id(&mut self) -> Result<ObjectId> { ObjectId::new(self.uint()?) }
    /// Reads a UTF-8 protocol string.
    pub fn string(&mut self) -> Result<String> {
        let length = usize::try_from(self.uint()?).map_err(|_| Error::damaged("Wayland string length conversion failed"))?;
        if length == 0 || length > MAX_STRING_BYTES { return Err(Error::damaged("invalid Wayland string length")); }
        let bytes = self.take(length)?;
        if bytes.last().copied() != Some(0) { return Err(Error::damaged("Wayland string lacks NUL terminator")); }
        let text = std::str::from_utf8(bytes.get(..length.saturating_sub(1)).unwrap_or_default()).map_err(|error| Error::damaged(error.to_string()))?;
        self.skip_padding(length)?;
        Ok(text.to_owned())
    }
    /// Reads a byte array.
    pub fn array(&mut self) -> Result<Vec<u8>> {
        let length = usize::try_from(self.uint()?).map_err(|_| Error::damaged("Wayland array length conversion failed"))?;
        if length > MAX_ARRAY_BYTES { return Err(Error::damaged("Wayland array exceeds configured limit")); }
        let value = self.take(length)?.to_vec();
        self.skip_padding(length)?;
        Ok(value)
    }
    /// Reads one attached descriptor handle.
    pub fn fd(&mut self) -> Result<FdHandle> {
        let value = self.fds.get(self.fd_at).copied().ok_or_else(|| Error::damaged("Wayland message is missing an fd"))?;
        self.fd_at = self.fd_at.checked_add(1).ok_or_else(|| Error::damaged("Wayland fd index overflow"))?;
        Ok(value)
    }
    /// Whether all payload bytes and descriptor handles were consumed.
    #[must_use]
    pub fn finished(&self) -> bool { self.at == self.bytes.len() && self.fd_at == self.fds.len() }

    fn take_fixed<const N: usize>(&mut self) -> Result<&'a [u8]> { let end=self.at.checked_add(N).ok_or_else(|| Error::damaged("Wayland decode range overflow"))?; let value=self.bytes.get(self.at..end).ok_or_else(|| Error::damaged("truncated Wayland message"))?; self.at=end; Ok(value) }
    fn take_array<const N: usize>(&mut self) -> Result<[u8; N]> { <[u8; N]>::try_from(self.take_fixed::<N>()?).map_err(|_| Error::damaged("Wayland scalar width mismatch")) }
    fn take_dynamic(&mut self, length: usize) -> Result<&'a [u8]> { let end=self.at.checked_add(length).ok_or_else(|| Error::damaged("Wayland dynamic range overflow"))?; let value=self.bytes.get(self.at..end).ok_or_else(|| Error::damaged("truncated Wayland dynamic field"))?; self.at=end; Ok(value) }
    fn take(&mut self, length: usize) -> Result<&'a [u8]> { self.take_dynamic(length) }
    fn skip_padding(&mut self, length: usize) -> Result<()> { let padding=padding_four(length); let bytes=self.take_dynamic(padding)?; if bytes.iter().any(|byte| *byte!=0) { return Err(Error::damaged("non-zero Wayland padding")); } Ok(()) }
}

/// Stateful request marshaller for the required core and desktop protocols.
pub struct Client<T: Transport> {
    transport: T,
    ids: ObjectIds,
    interfaces: HashMap<ObjectId, Interface>,
}

impl<T: Transport> Client<T> {
    /// Creates a client with `wl_display` registered as object 1.
    pub fn new(transport: T) -> Result<Self> {
        let display=ObjectId::new(1)?;
        let mut interfaces=HashMap::new(); interfaces.insert(display,Interface::Display);
        Ok(Self{transport,ids:ObjectIds::default(),interfaces})
    }
    /// Returns mutable access to the transport for integration code.
    pub fn transport_mut(&mut self) -> &mut T { &mut self.transport }
    /// Allocates and sends `wl_display.get_registry`.
    pub fn get_registry(&mut self) -> Result<ObjectId> { let id=self.create(Interface::Registry)?; self.send_new(ObjectId::new(1)?,1,id,|_|Ok(()))?; Ok(id) }
    /// Binds an advertised registry global.
    pub fn registry_bind(&mut self, registry:ObjectId, name:u32, interface:&str, version:u32, kind:Interface) -> Result<ObjectId> { let id=self.create(kind)?; self.send_new(registry,0,id,|e|{e.uint(name); e.string(interface)?; e.uint(version); Ok(())})?; Ok(id) }
    /// Creates `wl_compositor.create_surface`.
    pub fn compositor_create_surface(&mut self, compositor:ObjectId) -> Result<ObjectId> { let id=self.create(Interface::Surface)?; self.send_new(compositor,0,id,|_|Ok(()))?; Ok(id) }
    /// `wl_surface.attach`.
    pub fn surface_attach(&mut self,surface:ObjectId,buffer:Option<ObjectId>,x:i32,y:i32)->Result<()> { self.send(surface,1,|e|{e.object(buffer);e.int(x);e.int(y);Ok(())}) }
    /// `wl_surface.damage_buffer`.
    pub fn surface_damage_buffer(&mut self,surface:ObjectId,x:i32,y:i32,width:i32,height:i32)->Result<()> { self.send(surface,9,|e|{e.int(x);e.int(y);e.int(width);e.int(height);Ok(())}) }
    /// Requests a frame callback.
    pub fn surface_frame(&mut self,surface:ObjectId)->Result<ObjectId>{let id=self.create(Interface::Callback)?;self.send_new(surface,3,id,|_|Ok(()))?;Ok(id)}
    /// `wl_surface.commit`.
    pub fn surface_commit(&mut self,surface:ObjectId)->Result<()> { self.send(surface,6,|_|Ok(())) }
    /// `wl_surface.set_buffer_scale`.
    pub fn surface_set_buffer_scale(&mut self,surface:ObjectId,scale:i32)->Result<()> { if scale<1{return Err(Error::damaged("buffer scale must be positive"));} self.send(surface,8,|e|{e.int(scale);Ok(())}) }
    /// Creates `wp_fractional_scale_v1` for a surface.
    pub fn fractional_scale_get(&mut self,manager:ObjectId,surface:ObjectId)->Result<ObjectId>{let id=self.create(Interface::FractionalScale)?;self.send_new(manager,1,id,|e|{e.object(Some(surface));Ok(())})?;Ok(id)}
    /// Creates a `wl_shm_pool` from an opaque fd handle.
    pub fn shm_create_pool(&mut self,shm:ObjectId,fd:FdHandle,size:i32)->Result<ObjectId>{if size<0{return Err(Error::damaged("negative shm pool size"));}let id=self.create(Interface::ShmPool)?;self.send_new(shm,0,id,|e|{e.fd(fd);e.int(size);Ok(())})?;Ok(id)}
    /// Creates a `wl_buffer` from a shared-memory pool.
    pub fn shm_pool_create_buffer(&mut self,pool:ObjectId,offset:i32,width:i32,height:i32,stride:i32,format:u32)->Result<ObjectId>{let id=self.create(Interface::Buffer)?;self.send_new(pool,0,id,|e|{e.int(offset);e.int(width);e.int(height);e.int(stride);e.uint(format);Ok(())})?;Ok(id)}
    /// Resizes a `wl_shm_pool`.
    pub fn shm_pool_resize(&mut self,pool:ObjectId,size:i32)->Result<()> { self.send(pool,2,|e|{e.int(size);Ok(())}) }
    /// Responds to `xdg_wm_base.ping`.
    pub fn xdg_pong(&mut self,wm:ObjectId,serial:u32)->Result<()> { self.send(wm,3,|e|{e.uint(serial);Ok(())}) }
    /// Creates an `xdg_surface`.
    pub fn xdg_get_surface(&mut self,wm:ObjectId,surface:ObjectId)->Result<ObjectId>{let id=self.create(Interface::XdgSurface)?;self.send_new(wm,2,id,|e|{e.object(Some(surface));Ok(())})?;Ok(id)}
    /// Creates an `xdg_toplevel`.
    pub fn xdg_surface_get_toplevel(&mut self,xdg_surface:ObjectId)->Result<ObjectId>{let id=self.create(Interface::XdgToplevel)?;self.send_new(xdg_surface,1,id,|_|Ok(()))?;Ok(id)}
    /// Acknowledges an xdg configure serial.
    pub fn xdg_surface_ack_configure(&mut self,xdg_surface:ObjectId,serial:u32)->Result<()> { self.send(xdg_surface,4,|e|{e.uint(serial);Ok(())}) }
    /// Sets an xdg toplevel title.
    pub fn xdg_toplevel_set_title(&mut self,toplevel:ObjectId,title:&str)->Result<()> { self.send(toplevel,2,|e|e.string(title)) }
    /// Sets minimum content dimensions.
    pub fn xdg_toplevel_set_min_size(&mut self,toplevel:ObjectId,width:i32,height:i32)->Result<()> { self.send(toplevel,8,|e|{e.int(width);e.int(height);Ok(())}) }
    /// Requests a toplevel decoration object.
    pub fn decoration_get_toplevel(&mut self,manager:ObjectId,toplevel:ObjectId)->Result<ObjectId>{let id=self.create(Interface::ToplevelDecoration)?;self.send_new(manager,1,id,|e|{e.object(Some(toplevel));Ok(())})?;Ok(id)}
    /// Requests server-side decorations (`mode = 2`).
    pub fn decoration_set_server_side(&mut self,decoration:ObjectId)->Result<()> { self.send(decoration,1,|e|{e.uint(2);Ok(())}) }
    /// Gets a keyboard object from `wl_seat`.
    pub fn seat_get_keyboard(&mut self,seat:ObjectId)->Result<ObjectId>{let id=self.create(Interface::Keyboard)?;self.send_new(seat,1,id,|_|Ok(()))?;Ok(id)}
    /// Gets a pointer object from `wl_seat`.
    pub fn seat_get_pointer(&mut self,seat:ObjectId)->Result<ObjectId>{let id=self.create(Interface::Pointer)?;self.send_new(seat,0,id,|_|Ok(()))?;Ok(id)}
    /// Creates a clipboard data device for a seat.
    pub fn data_device_get(&mut self,manager:ObjectId,seat:ObjectId)->Result<ObjectId>{let id=self.create(Interface::DataDevice)?;self.send_new(manager,1,id,|e|{e.object(Some(seat));Ok(())})?;Ok(id)}
    /// Creates a data source for clipboard ownership.
    pub fn data_source_create(&mut self,manager:ObjectId)->Result<ObjectId>{let id=self.create(Interface::DataSource)?;self.send_new(manager,0,id,|_|Ok(()))?;Ok(id)}
    /// Advertises a text MIME type from a clipboard source.
    pub fn data_source_offer_text(&mut self,source:ObjectId,mime:&str)->Result<()> { self.send(source,0,|e|e.string(mime)) }
    /// Sets the clipboard selection.
    pub fn data_device_set_selection(&mut self,device:ObjectId,source:Option<ObjectId>,serial:u32)->Result<()> { self.send(device,1,|e|{e.object(source);e.uint(serial);Ok(())}) }
    /// Requests offered clipboard data to be written to an fd.
    pub fn data_offer_receive(&mut self,offer:ObjectId,mime:&str,fd:FdHandle)->Result<()> { self.send(offer,1,|e|{e.string(mime)?;e.fd(fd);Ok(())}) }

    /// Decodes an event using the interface registered for `message.object`.
    pub fn decode_event(&mut self, message: Message) -> Result<Event> {
        let Some(interface)=self.interfaces.get(&message.object).copied() else { return Ok(Event::Unknown(message)); };
        let mut d=Decoder::new(&message.payload,&message.fds);
        let event=match (interface,message.opcode) {
            (Interface::Registry,0)=>Event::RegistryGlobal{name:d.uint()?,interface:d.string()?,version:d.uint()?},
            (Interface::Registry,1)=>Event::RegistryGlobalRemove{name:d.uint()?},
            (Interface::Callback,0)=>{let data=d.uint()?;self.ids.release(message.object);self.interfaces.remove(&message.object);Event::CallbackDone{callback_data:data}},
            (Interface::XdgWmBase,0)=>Event::XdgPing{serial:d.uint()?},
            (Interface::XdgSurface,0)=>Event::XdgSurfaceConfigure{serial:d.uint()?},
            (Interface::XdgToplevel,0)=>Event::XdgToplevelConfigure{width:d.int()?,height:d.int()?,states:d.array()?},
            (Interface::XdgToplevel,1)=>Event::XdgToplevelClose,
            (Interface::FractionalScale,0)=>Event::FractionalScalePreferred{scale_120:d.uint()?},
            (Interface::Seat,0)=>Event::SeatCapabilities{capabilities:d.uint()?},
            (Interface::Seat,1)=>Event::SeatName{name:d.string()?},
            (Interface::Keyboard,0)=>Event::KeyboardKeymap{format:d.uint()?,fd:d.fd()?,size:d.uint()?},
            (Interface::Keyboard,1)=>Event::KeyboardEnter{serial:d.uint()?,surface:d.object()?.ok_or_else(||Error::damaged("keyboard enter has null surface"))?,keys:d.array()?},
            (Interface::Keyboard,2)=>Event::KeyboardLeave{serial:d.uint()?,surface:d.object()?.ok_or_else(||Error::damaged("keyboard leave has null surface"))?},
            (Interface::Keyboard,3)=>Event::KeyboardKey{serial:d.uint()?,time:d.uint()?,key:d.uint()?,state:d.uint()?},
            (Interface::Keyboard,4)=>Event::KeyboardModifiers{serial:d.uint()?,depressed:d.uint()?,latched:d.uint()?,locked:d.uint()?,group:d.uint()?},
            (Interface::Keyboard,5)=>Event::KeyboardRepeatInfo{rate:d.int()?,delay:d.int()?},
            (Interface::Pointer,0)=>Event::PointerEnter{serial:d.uint()?,surface:d.object()?.ok_or_else(||Error::damaged("pointer enter has null surface"))?,x:d.fixed()?,y:d.fixed()?},
            (Interface::Pointer,2)=>Event::PointerMotion{time:d.uint()?,x:d.fixed()?,y:d.fixed()?},
            (Interface::Pointer,3)=>Event::PointerButton{serial:d.uint()?,time:d.uint()?,button:d.uint()?,state:d.uint()?},
            (Interface::Pointer,4)=>Event::PointerAxis{time:d.uint()?,axis:d.uint()?,value:d.fixed()?},
            (Interface::DataDevice,0)=>{let offer=d.new_id()?;self.interfaces.insert(offer,Interface::DataOffer);Event::DataOffer{offer}},
            (Interface::DataDevice,5)=>Event::DataSelection{offer:d.object()?},
            (Interface::DataOffer,0)=>Event::DataOfferMime{mime:d.string()?},
            _=>return Ok(Event::Unknown(message)),
        };
        if !d.finished(){return Err(Error::damaged("Wayland event has trailing payload or descriptors"));}
        Ok(event)
    }

    fn create(&mut self, interface:Interface)->Result<ObjectId>{let id=self.ids.allocate()?;self.interfaces.insert(id,interface);Ok(id)}
    fn send<F>(&mut self, object:ObjectId, opcode:u16, build:F)->Result<()> where F:FnOnce(&mut Encoder)->Result<()> { let mut e=Encoder::new(object,opcode);build(&mut e)?;let request=e.finish()?;self.transport.send(&request.bytes,&request.fds) }
    fn send_new<F>(&mut self,object:ObjectId,opcode:u16,new_id:ObjectId,build:F)->Result<()> where F:FnOnce(&mut Encoder)->Result<()> { self.send(object,opcode,|e|{e.new_id(new_id);build(e)}) }
}

/// Decodes one complete Wayland message from bytes already separated from the stream.
pub fn decode_message(bytes:&[u8],fds:Vec<FdHandle>)->Result<Message>{
    if bytes.len()<HEADER_BYTES{return Err(Error::damaged("Wayland message shorter than header"));}
    let object=ObjectId::new(u32::from_ne_bytes(<[u8;4]>::try_from(bytes.get(..4).unwrap_or_default()).map_err(|_|Error::damaged("short Wayland object header"))?))?;
    let word=u32::from_ne_bytes(<[u8;4]>::try_from(bytes.get(4..8).unwrap_or_default()).map_err(|_|Error::damaged("short Wayland size header"))?);
    let opcode=u16::try_from(word&0xffff).map_err(|_|Error::damaged("Wayland opcode conversion failed"))?;
    let size=usize::try_from(word>>16).map_err(|_|Error::damaged("Wayland size conversion failed"))?;
    if size!=bytes.len()||size<HEADER_BYTES||size.rem_euclid(4)!=0{return Err(Error::damaged("Wayland message size/header mismatch"));}
    Ok(Message{object,opcode,payload:bytes.get(HEADER_BYTES..).unwrap_or_default().to_vec(),fds})
}

fn pad_four(bytes:&mut Vec<u8>)->Result<()> { let padding=padding_four(bytes.len()); let new_len=bytes.len().checked_add(padding).ok_or_else(||Error::damaged("Wayland padding overflow"))?; bytes.resize(new_len,0); Ok(()) }
fn padding_four(length:usize)->usize { 4_usize.saturating_sub(length.rem_euclid(4)).rem_euclid(4) }

/// Active XKB modifiers relevant to the supported keymap subset.
#[derive(Debug,Clone,Copy,Default,PartialEq,Eq)]
pub struct Modifiers {
    /// Shift modifier.
    pub shift: bool,
    /// Caps Lock modifier.
    pub lock: bool,
    /// ISO level-3 modifier (often AltGr).
    pub level3: bool,
    /// Active keyboard group; group 0 is US, group 1 is Russian in the reproduced fixture.
    pub group: u32,
}

#[derive(Debug,Clone)]
struct KeyDef { groups:Vec<Vec<String>> }

/// Parsed XKB text keymap sufficient for `us,ru` text input.
#[derive(Debug,Clone,Default)]
pub struct XkbKeymap {
    keycodes:HashMap<u32,String>,
    keys:HashMap<String,KeyDef>,
    has_types:bool,
    has_compat:bool,
}

impl XkbKeymap {
    /// Parses an XKB text keymap containing `xkb_keycodes`, `xkb_types`, `xkb_compatibility`, and `xkb_symbols`.
    pub fn parse(text:&str)->Result<Self>{
        let mut map=Self::default();
        map.has_types=text.contains("xkb_types"); map.has_compat=text.contains("xkb_compat");
        for line in text.lines().map(str::trim) {
            if line.starts_with('<')&&line.contains('=')&&!line.starts_with("key ") {
                if let Some((name,value))=parse_keycode_line(line)? { map.keycodes.insert(value,name); }
            }
            if line.starts_with("key ") {
                if let Some((name,groups))=parse_symbol_line(line)? { map.keys.insert(name,KeyDef{groups}); }
            }
        }
        if map.keycodes.is_empty()||map.keys.is_empty(){return Err(Error::damaged("XKB keymap lacks keycodes or symbols"));}
        Ok(map)
    }
    /// Resolves an XKB keycode and modifier state to a keysym name.
    pub fn keysym(&self,keycode:u32,mods:Modifiers)->Option<&str>{
        let name=self.keycodes.get(&keycode)?; let def=self.keys.get(name)?;
        let group=usize::try_from(mods.group).ok().unwrap_or_default().min(def.groups.len().saturating_sub(1));
        let levels=def.groups.get(group)?;
        let mut level=if mods.level3 {2_usize}else{0_usize};
        if mods.shift {level=level.saturating_add(1);}
        let base=levels.get(level).or_else(||levels.get(level.rem_euclid(2))).or_else(||levels.first())?;
        if mods.lock && !mods.shift && is_alpha_keysym(base) { levels.get(1).map(String::as_str).or(Some(base.as_str())) } else if mods.lock&&mods.shift&&is_alpha_keysym(base) { levels.first().map(String::as_str).or(Some(base.as_str())) } else { Some(base.as_str()) }
    }
    /// Resolves a keycode directly to a Unicode character when the keysym is textual.
    pub fn character(&self,keycode:u32,mods:Modifiers)->Option<char>{keysym_to_char(self.keysym(keycode,mods)?) }
    /// Whether the keymap contained an `xkb_types` section.
    #[must_use] pub const fn has_types(&self)->bool{self.has_types}
    /// Whether the keymap contained an `xkb_compatibility` section.
    #[must_use] pub const fn has_compat(&self)->bool{self.has_compat}
}

/// Converts the keysym names used by ordinary Latin and Russian text keys to Unicode.
#[must_use]
pub fn keysym_to_char(name:&str)->Option<char>{
    if let Some(hex)=name.strip_prefix('U') { if (4..=8).contains(&hex.len()) { if let Ok(value)=u32::from_str_radix(hex,16){if let Some(ch)=char::from_u32(value){return Some(ch);}} } }
    if name.len()==1{return name.chars().next();}
    match name {
        "space"=>Some(' '),"Tab"=>Some('\t'),"Return"=>Some('\n'),"minus"=>Some('-'),"underscore"=>Some('_'),"equal"=>Some('='),"plus"=>Some('+'),"bracketleft"=>Some('['),"braceleft"=>Some('{'),"bracketright"=>Some(']'),"braceright"=>Some('}'),"semicolon"=>Some(';'),"colon"=>Some(':'),"apostrophe"=>Some('\''),"quotedbl"=>Some('"'),"grave"=>Some('`'),"asciitilde"=>Some('~'),"backslash"=>Some('\\'),"bar"=>Some('|'),"comma"=>Some(','),"less"=>Some('<'),"period"=>Some('.'),"greater"=>Some('>'),"slash"=>Some('/'),"question"=>Some('?'),
        "Cyrillic_shorti"=>Some('й'),"Cyrillic_SHORTI"=>Some('Й'),"Cyrillic_tse"=>Some('ц'),"Cyrillic_TSE"=>Some('Ц'),"Cyrillic_u"=>Some('у'),"Cyrillic_U"=>Some('У'),"Cyrillic_ka"=>Some('к'),"Cyrillic_KA"=>Some('К'),"Cyrillic_ie"=>Some('е'),"Cyrillic_IE"=>Some('Е'),"Cyrillic_en"=>Some('н'),"Cyrillic_EN"=>Some('Н'),"Cyrillic_ghe"=>Some('г'),"Cyrillic_GHE"=>Some('Г'),"Cyrillic_sha"=>Some('ш'),"Cyrillic_SHA"=>Some('Ш'),"Cyrillic_shcha"=>Some('щ'),"Cyrillic_SHCHA"=>Some('Щ'),"Cyrillic_ze"=>Some('з'),"Cyrillic_ZE"=>Some('З'),"Cyrillic_ha"=>Some('х'),"Cyrillic_HA"=>Some('Х'),"Cyrillic_hardsign"=>Some('ъ'),"Cyrillic_HARDSIGN"=>Some('Ъ'),"Cyrillic_ef"=>Some('ф'),"Cyrillic_EF"=>Some('Ф'),"Cyrillic_yeru"=>Some('ы'),"Cyrillic_YERU"=>Some('Ы'),"Cyrillic_ve"=>Some('в'),"Cyrillic_VE"=>Some('В'),"Cyrillic_a"=>Some('а'),"Cyrillic_A"=>Some('А'),"Cyrillic_pe"=>Some('п'),"Cyrillic_PE"=>Some('П'),"Cyrillic_er"=>Some('р'),"Cyrillic_ER"=>Some('Р'),"Cyrillic_o"=>Some('о'),"Cyrillic_O"=>Some('О'),"Cyrillic_el"=>Some('л'),"Cyrillic_EL"=>Some('Л'),"Cyrillic_de"=>Some('д'),"Cyrillic_DE"=>Some('Д'),"Cyrillic_zhe"=>Some('ж'),"Cyrillic_ZHE"=>Some('Ж'),"Cyrillic_e"=>Some('э'),"Cyrillic_E"=>Some('Э'),"Cyrillic_ya"=>Some('я'),"Cyrillic_YA"=>Some('Я'),"Cyrillic_che"=>Some('ч'),"Cyrillic_CHE"=>Some('Ч'),"Cyrillic_es"=>Some('с'),"Cyrillic_ES"=>Some('С'),"Cyrillic_em"=>Some('м'),"Cyrillic_EM"=>Some('М'),"Cyrillic_i"=>Some('и'),"Cyrillic_I"=>Some('И'),"Cyrillic_te"=>Some('т'),"Cyrillic_TE"=>Some('Т'),"Cyrillic_softsign"=>Some('ь'),"Cyrillic_SOFTSIGN"=>Some('Ь'),"Cyrillic_be"=>Some('б'),"Cyrillic_BE"=>Some('Б'),"Cyrillic_yu"=>Some('ю'),"Cyrillic_YU"=>Some('Ю'),"Cyrillic_io"=>Some('ё'),"Cyrillic_IO"=>Some('Ё'),
        _=>None,
    }
}

fn is_alpha_keysym(name:&str)->bool { keysym_to_char(name).is_some_and(char::is_alphabetic) }
fn parse_keycode_line(line:&str)->Result<Option<(String,u32)>>{let Some(close)=line.find('>') else{return Ok(None)};let Some(open)=line.find('<') else{return Ok(None)};let name=line.get(open.saturating_add(1)..close).unwrap_or_default().trim();let Some(eq)=line.find('=') else{return Ok(None)};let number=line.get(eq.saturating_add(1)..).unwrap_or_default().trim().trim_end_matches(';').trim();let value=number.parse::<u32>().map_err(|_|Error::damaged("invalid XKB keycode assignment"))?;Ok(Some((name.to_owned(),value)))}
fn parse_symbol_line(line:&str)->Result<Option<(String,Vec<Vec<String>>)>>{let Some(open)=line.find('<') else{return Ok(None)};let Some(close_rel)=line.get(open..).and_then(|v|v.find('>')) else{return Ok(None)};let close=open.checked_add(close_rel).ok_or_else(||Error::damaged("XKB symbol name overflow"))?;let name=line.get(open.saturating_add(1)..close).unwrap_or_default().trim().to_owned();let Some(brace)=line.find('{') else{return Ok(None)};let Some(end)=line.rfind('}') else{return Ok(None)};let body=line.get(brace.saturating_add(1)..end).unwrap_or_default();let mut groups=Vec::new();let mut rest=body;while let Some(start)=rest.find('['){let after=rest.get(start.saturating_add(1)..).unwrap_or_default();let Some(stop)=after.find(']') else{return Err(Error::damaged("unterminated XKB symbol group"))};let inside=after.get(..stop).unwrap_or_default();let levels=inside.split(',').map(str::trim).filter(|v|!v.is_empty()).map(ToOwned::to_owned).collect::<Vec<_>>();groups.push(levels);rest=after.get(stop.saturating_add(1)..).unwrap_or_default();}if groups.is_empty(){return Ok(None)}Ok(Some((name,groups)))}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)] struct MemoryTransport{sent:Vec<(Vec<u8>,Vec<FdHandle>)>}
    impl Transport for MemoryTransport{fn send(&mut self,bytes:&[u8],fds:&[FdHandle])->Result<()>{self.sent.push((bytes.to_vec(),fds.to_vec()));Ok(())}fn receive(&mut self,_:&mut Vec<u8>,_:&mut Vec<FdHandle>)->Result<usize>{Ok(0)}}

    #[test]
    fn hand_built_wire_message_round_trips(){let object=ObjectId::new(7).unwrap_or_else(|e|panic!("{e}"));let mut e=Encoder::new(object,3);e.int(-4);e.uint(9);e.fixed(Fixed::from_raw(384));e.string("hi").unwrap_or_else(|er|panic!("{er}"));e.array(&[1,2,3]).unwrap_or_else(|er|panic!("{er}"));e.fd(55);let r=e.finish().unwrap_or_else(|er|panic!("{er}"));assert_eq!(r.fds,[55]);let m=decode_message(&r.bytes,r.fds.clone()).unwrap_or_else(|er|panic!("{er}"));assert_eq!(m.object,object);assert_eq!(m.opcode,3);let mut d=Decoder::new(&m.payload,&m.fds);assert_eq!(d.int(),Ok(-4));assert_eq!(d.uint(),Ok(9));assert_eq!(d.fixed(),Ok(Fixed::from_raw(384)));assert_eq!(d.string(),Ok("hi".to_owned()));assert_eq!(d.array(),Ok(vec![1,2,3]));assert_eq!(d.fd(),Ok(55));assert!(d.finished());}

    #[test]
    fn request_header_is_wayland_layout(){let object=ObjectId::new(1).unwrap_or_else(|e|panic!("{e}"));let mut e=Encoder::new(object,1);e.new_id(ObjectId::new(2).unwrap_or_else(|er|panic!("{er}")));let r=e.finish().unwrap_or_else(|er|panic!("{er}"));assert_eq!(r.bytes.len(),12);assert_eq!(r.bytes.get(..4),Some(&1_u32.to_ne_bytes()[..]));let word=12_u32.checked_shl(16).unwrap_or_default()|1;assert_eq!(r.bytes.get(4..8),Some(&word.to_ne_bytes()[..]));assert_eq!(r.bytes.get(8..12),Some(&2_u32.to_ne_bytes()[..]));}

    const US_RU:&str=r#"
xkb_keymap {
 xkb_keycodes "evdev" {
   <AD01> = 24; <AD02> = 25; <AC01> = 38; <AB01> = 52;
 };
 xkb_types "complete" { virtual_modifiers LevelThree; };
 xkb_compatibility "complete" { interpret ISO_Level3_Shift+AnyOf(all) { action= SetMods(modifiers=LevelThree); }; };
 xkb_symbols "pc+us+ru:2" {
   key <AD01> { [ q, Q ], [ Cyrillic_shorti, Cyrillic_SHORTI ] };
   key <AD02> { [ w, W ], [ Cyrillic_tse, Cyrillic_TSE ] };
   key <AC01> { [ a, A ], [ Cyrillic_ef, Cyrillic_EF ] };
   key <AB01> { [ z, Z ], [ Cyrillic_ya, Cyrillic_YA ] };
 };
};
"#;

    #[test]
    fn xkb_us_ru_fixture_maps_groups_and_shift(){let map=XkbKeymap::parse(US_RU).unwrap_or_else(|e|panic!("{e}"));assert!(map.has_types());assert!(map.has_compat());assert_eq!(map.character(24,Modifiers::default()),Some('q'));assert_eq!(map.character(24,Modifiers{shift:true,..Modifiers::default()}),Some('Q'));assert_eq!(map.character(24,Modifiers{group:1,..Modifiers::default()}),Some('й'));assert_eq!(map.character(24,Modifiers{shift:true,group:1,..Modifiers::default()}),Some('Й'));assert_eq!(map.character(38,Modifiers{group:1,..Modifiers::default()}),Some('ф'));}
}
