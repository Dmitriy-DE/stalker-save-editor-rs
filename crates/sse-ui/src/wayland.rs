//! Pure-`std` Wayland client-protocol marshalling plus a deliberately small XKB text-keymap parser.
//!
//! This module does not open sockets, map shared memory or own operating-system file descriptors. The caller's
//! transport provides byte delivery and represents each passed file descriptor as an opaque `u32` handle. All
//! protocol sizes, object identifiers and cursor movement are bounds checked before they are committed.

use sse_core::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};

const HEADER_BYTES: usize = 8;
const MAX_MESSAGE_BYTES: usize = u16::MAX as usize;
const MAX_STRING_BYTES: usize = 1 << 20;
const MAX_ARRAY_BYTES: usize = 1 << 24;
const MAX_OBJECTS: usize = 1 << 20;
const SERVER_ID_START: u32 = 0xff00_0000;

/// A Wayland object identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(u32);

impl ObjectId {
    /// The display object's fixed identifier.
    pub const DISPLAY: Self = Self(1);

    /// Creates a non-zero object identifier.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for zero.
    pub fn new(raw: u32) -> Result<Self> {
        if raw == 0 {
            Err(Error::damaged("Wayland object id 0 is reserved for null"))
        } else {
            Ok(Self(raw))
        }
    }

    /// Returns the wire value.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Raw 24.8 Wayland fixed-point value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fixed(i32);

impl Fixed {
    /// Creates a fixed-point value from its exact wire representation.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    /// Returns the exact wire representation.
    #[must_use]
    pub const fn raw(self) -> i32 {
        self.0
    }
}

/// One fully framed Wayland message plus its ancillary descriptor handles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Destination object.
    pub object: ObjectId,
    /// Request or event opcode.
    pub opcode: u16,
    /// Complete wire bytes, including the eight-byte Wayland header.
    pub bytes: Vec<u8>,
    /// Opaque descriptor handles carried beside the byte stream.
    pub fds: Vec<u32>,
}

impl Message {
    /// Decodes and validates a complete single Wayland message.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for a short, misaligned or size-mismatched packet.
    pub fn decode(bytes: &[u8], fds: &[u32]) -> Result<Self> {
        if bytes.len() < HEADER_BYTES {
            return Err(Error::damaged("Wayland message is shorter than its header"));
        }
        let object_raw = read_u32_ne(bytes, 0)?;
        let word = read_u32_ne(bytes, 4)?;
        let size_u32 = word >> 16;
        let opcode_u32 = word & 0xffff;
        let size = usize::try_from(size_u32).map_err(|_| Error::damaged("Wayland message size does not fit usize"))?;
        if size != bytes.len() || size < HEADER_BYTES || size.checked_rem(4) != Some(0) {
            return Err(Error::damaged("Wayland message size/header mismatch"));
        }
        let opcode = u16::try_from(opcode_u32).map_err(|_| Error::damaged("Wayland opcode does not fit u16"))?;
        Ok(Self {
            object: ObjectId::new(object_raw)?,
            opcode,
            bytes: bytes.to_vec(),
            fds: fds.to_vec(),
        })
    }

    /// Returns a reader positioned at the first argument.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] if the message header is not valid anymore.
    pub fn arguments(&self) -> Result<WireReader<'_>> {
        let decoded = Self::decode(&self.bytes, &self.fds)?;
        if decoded.object != self.object || decoded.opcode != self.opcode {
            return Err(Error::damaged("Wayland message metadata disagrees with its header"));
        }
        Ok(WireReader::new(
            self.bytes
                .get(HEADER_BYTES..)
                .ok_or_else(|| Error::damaged("Wayland argument slice missing"))?,
            &self.fds,
        ))
    }
}

/// Byte transport used by the protocol marshaller.
///
/// The real Unix socket and descriptor passing live outside `sse-ui`; descriptors are opaque `u32` handles here.
pub trait Transport {
    /// Sends one complete framed message with zero or more descriptor handles.
    ///
    /// # Errors
    /// Implementations return their transport failure as [`Error`].
    fn send(&mut self, bytes: &[u8], fds: &[u32]) -> Result<()>;

    /// Receives one complete framed message into `buf` and appends its descriptor handles to `fds`.
    ///
    /// The returned byte count must not exceed `buf.len()`.
    ///
    /// # Errors
    /// Implementations return their transport failure as [`Error`].
    fn receive(&mut self, buf: &mut [u8], fds: &mut Vec<u32>) -> Result<usize>;
}

/// Builder for one Wayland request/event payload.
#[derive(Debug)]
pub struct WireWriter {
    object: ObjectId,
    opcode: u16,
    payload: Vec<u8>,
    fds: Vec<u32>,
}

impl WireWriter {
    /// Starts a message for `object` and `opcode`.
    #[must_use]
    pub fn new(object: ObjectId, opcode: u16) -> Self {
        Self {
            object,
            opcode,
            payload: Vec::new(),
            fds: Vec::new(),
        }
    }

    /// Appends a signed 32-bit integer.
    pub fn int(&mut self, value: i32) {
        self.payload.extend_from_slice(&value.to_ne_bytes());
    }

    /// Appends an unsigned 32-bit integer.
    pub fn uint(&mut self, value: u32) {
        self.payload.extend_from_slice(&value.to_ne_bytes());
    }

    /// Appends a 24.8 fixed-point value.
    pub fn fixed(&mut self, value: Fixed) {
        self.int(value.raw());
    }

    /// Appends a nullable object reference; `None` is encoded as object id zero.
    pub fn object(&mut self, value: Option<ObjectId>) {
        self.uint(value.map_or(0, ObjectId::raw));
    }

    /// Appends a newly allocated object identifier.
    pub fn new_id(&mut self, value: ObjectId) {
        self.uint(value.raw());
    }

    /// Appends one passed descriptor handle. File descriptors occupy no bytes in Wayland messages.
    pub fn fd(&mut self, handle: u32) {
        self.fds.push(handle);
    }

    /// Appends a UTF-8 Wayland string including the required trailing zero and four-byte padding.
    ///
    /// # Errors
    /// Returns [`Error::Refused`] when the string exceeds the hard message/string limits.
    pub fn string(&mut self, value: &str) -> Result<()> {
        let content = value.as_bytes();
        if content.len() > MAX_STRING_BYTES || content.contains(&0) {
            return Err(Error::Refused("Wayland string is too large or contains NUL".to_owned()));
        }
        let wire_len = content
            .len()
            .checked_add(1)
            .ok_or_else(|| Error::Refused("Wayland string length overflow".to_owned()))?;
        self.uint(u32::try_from(wire_len).map_err(|_| Error::Refused("Wayland string length exceeds u32".to_owned()))?);
        self.payload.extend_from_slice(content);
        self.payload.push(0);
        pad_vec_4(&mut self.payload)?;
        Ok(())
    }

    /// Appends a raw Wayland array and four-byte padding.
    ///
    /// # Errors
    /// Returns [`Error::Refused`] when the array exceeds configured limits.
    pub fn array(&mut self, value: &[u8]) -> Result<()> {
        if value.len() > MAX_ARRAY_BYTES {
            return Err(Error::Refused("Wayland array exceeds the hard limit".to_owned()));
        }
        self.uint(
            u32::try_from(value.len()).map_err(|_| Error::Refused("Wayland array length exceeds u32".to_owned()))?,
        );
        self.payload.extend_from_slice(value);
        pad_vec_4(&mut self.payload)?;
        Ok(())
    }

    /// Finishes the message, writes the native-endian Wayland header and enforces the 16-bit wire size.
    ///
    /// # Errors
    /// Returns [`Error::Refused`] when the message cannot be represented by Wayland's 16-bit size field.
    pub fn finish(self) -> Result<Message> {
        let size = HEADER_BYTES
            .checked_add(self.payload.len())
            .ok_or_else(|| Error::Refused("Wayland message size overflow".to_owned()))?;
        if size > MAX_MESSAGE_BYTES || size.checked_rem(4) != Some(0) {
            return Err(Error::Refused(
                "Wayland message exceeds size or alignment limits".to_owned(),
            ));
        }
        let size_u16 =
            u16::try_from(size).map_err(|_| Error::Refused("Wayland message size exceeds u16".to_owned()))?;
        let size_word = u32::from(size_u16)
            .checked_shl(16)
            .ok_or_else(|| Error::Refused("Wayland size header shift overflow".to_owned()))?;
        let word = size_word | u32::from(self.opcode);
        let mut bytes = Vec::with_capacity(size);
        bytes.extend_from_slice(&self.object.raw().to_ne_bytes());
        bytes.extend_from_slice(&word.to_ne_bytes());
        bytes.extend_from_slice(&self.payload);
        Ok(Message {
            object: self.object,
            opcode: self.opcode,
            bytes,
            fds: self.fds,
        })
    }
}

/// Bounds-checked argument reader for one message payload.
#[derive(Debug, Clone)]
pub struct WireReader<'a> {
    bytes: &'a [u8],
    position: usize,
    fds: &'a [u32],
    fd_position: usize,
}

impl<'a> WireReader<'a> {
    /// Starts at the first payload byte.
    #[must_use]
    pub const fn new(bytes: &'a [u8], fds: &'a [u32]) -> Self {
        Self {
            bytes,
            position: 0,
            fds,
            fd_position: 0,
        }
    }

    /// Number of unread payload bytes.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    /// Number of unread descriptor handles.
    #[must_use]
    pub fn remaining_fds(&self) -> usize {
        self.fds.len().saturating_sub(self.fd_position)
    }

    /// Reads a signed integer.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] on truncated input.
    pub fn int(&mut self) -> Result<i32> {
        Ok(i32::from_ne_bytes(self.fixed_array()?))
    }

    /// Reads an unsigned integer.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] on truncated input.
    pub fn uint(&mut self) -> Result<u32> {
        Ok(u32::from_ne_bytes(self.fixed_array()?))
    }

    /// Reads a fixed-point value.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] on truncated input.
    pub fn fixed(&mut self) -> Result<Fixed> {
        self.int().map(Fixed::from_raw)
    }

    /// Reads a nullable object reference.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] on truncated input or an invalid non-zero id.
    pub fn object(&mut self) -> Result<Option<ObjectId>> {
        let raw = self.uint()?;
        if raw == 0 {
            Ok(None)
        } else {
            ObjectId::new(raw).map(Some)
        }
    }

    /// Reads a non-null new object id.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] on zero or truncated input.
    pub fn new_id(&mut self) -> Result<ObjectId> {
        ObjectId::new(self.uint()?)
    }

    /// Consumes one opaque descriptor handle.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] when the ancillary descriptor list is exhausted.
    pub fn fd(&mut self) -> Result<u32> {
        let value = self
            .fds
            .get(self.fd_position)
            .copied()
            .ok_or_else(|| Error::damaged("Wayland fd argument is missing"))?;
        self.fd_position = self
            .fd_position
            .checked_add(1)
            .ok_or_else(|| Error::damaged("Wayland fd cursor overflow"))?;
        Ok(value)
    }

    /// Reads a UTF-8 Wayland string and skips its padding.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for bad length, missing NUL, invalid UTF-8 or truncated padding.
    pub fn string(&mut self) -> Result<String> {
        let length =
            usize::try_from(self.uint()?).map_err(|_| Error::damaged("Wayland string length conversion failed"))?;
        if length == 0 || length > MAX_STRING_BYTES {
            return Err(Error::damaged("Wayland string length is invalid"));
        }
        let raw = self.take(length)?;
        if raw.last().copied() != Some(0) {
            return Err(Error::damaged("Wayland string has no trailing NUL"));
        }
        let content_end = length
            .checked_sub(1)
            .ok_or_else(|| Error::damaged("Wayland string underflow"))?;
        let content = raw
            .get(..content_end)
            .ok_or_else(|| Error::damaged("Wayland string content range invalid"))?;
        let text = std::str::from_utf8(content)
            .map_err(|_| Error::damaged("Wayland string is not UTF-8"))?
            .to_owned();
        let padded = align4(length)?;
        self.skip(padded.saturating_sub(length))?;
        Ok(text)
    }

    /// Reads a raw Wayland array and skips four-byte padding.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for excessive or truncated arrays.
    pub fn array(&mut self) -> Result<Vec<u8>> {
        let length =
            usize::try_from(self.uint()?).map_err(|_| Error::damaged("Wayland array length conversion failed"))?;
        if length > MAX_ARRAY_BYTES {
            return Err(Error::damaged("Wayland array exceeds hard limit"));
        }
        let result = self.take(length)?.to_vec();
        let padded = align4(length)?;
        self.skip(padded.saturating_sub(length))?;
        Ok(result)
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(length)
            .ok_or_else(|| Error::damaged("Wayland reader position overflow"))?;
        let slice = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| Error::damaged("Wayland payload is truncated"))?;
        self.position = end;
        Ok(slice)
    }

    fn skip(&mut self, length: usize) -> Result<()> {
        self.take(length).map(|_| ())
    }

    fn fixed_array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let raw = self.take(N)?;
        <[u8; N]>::try_from(raw).map_err(|_| Error::damaged("Wayland integer is truncated"))
    }
}

/// Wayland interfaces understood by this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Interface {
    /// Core display object.
    WlDisplay,
    /// Global registry.
    WlRegistry,
    /// Compositor factory.
    WlCompositor,
    /// Surface object.
    WlSurface,
    /// Callback object.
    WlCallback,
    /// Shared-memory global.
    WlShm,
    /// Shared-memory pool.
    WlShmPool,
    /// Buffer object.
    WlBuffer,
    /// Input seat.
    WlSeat,
    /// Keyboard object.
    WlKeyboard,
    /// Pointer object.
    WlPointer,
    /// Data-device manager.
    WlDataDeviceManager,
    /// Data device.
    WlDataDevice,
    /// Data source.
    WlDataSource,
    /// Data offer.
    WlDataOffer,
    /// xdg-shell base.
    XdgWmBase,
    /// xdg surface.
    XdgSurface,
    /// xdg toplevel.
    XdgToplevel,
    /// xdg-decoration manager.
    ZxdgDecorationManagerV1,
    /// xdg toplevel decoration.
    ZxdgToplevelDecorationV1,
    /// Viewporter global.
    WpViewporter,
    /// Per-surface viewport object.
    WpViewport,
    /// Fractional-scale manager.
    WpFractionalScaleManagerV1,
    /// Fractional-scale object.
    WpFractionalScaleV1,
}

impl Interface {
    /// Protocol interface name used by `wl_registry.bind`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::WlDisplay => "wl_display",
            Self::WlRegistry => "wl_registry",
            Self::WlCompositor => "wl_compositor",
            Self::WlSurface => "wl_surface",
            Self::WlCallback => "wl_callback",
            Self::WlShm => "wl_shm",
            Self::WlShmPool => "wl_shm_pool",
            Self::WlBuffer => "wl_buffer",
            Self::WlSeat => "wl_seat",
            Self::WlKeyboard => "wl_keyboard",
            Self::WlPointer => "wl_pointer",
            Self::WlDataDeviceManager => "wl_data_device_manager",
            Self::WlDataDevice => "wl_data_device",
            Self::WlDataSource => "wl_data_source",
            Self::WlDataOffer => "wl_data_offer",
            Self::XdgWmBase => "xdg_wm_base",
            Self::XdgSurface => "xdg_surface",
            Self::XdgToplevel => "xdg_toplevel",
            Self::ZxdgDecorationManagerV1 => "zxdg_decoration_manager_v1",
            Self::ZxdgToplevelDecorationV1 => "zxdg_toplevel_decoration_v1",
            Self::WpViewporter => "wp_viewporter",
            Self::WpViewport => "wp_viewport",
            Self::WpFractionalScaleManagerV1 => "wp_fractional_scale_manager_v1",
            Self::WpFractionalScaleV1 => "wp_fractional_scale_v1",
        }
    }

    /// Highest version whose requests/events are encoded by this module.
    #[must_use]
    pub const fn supported_version(self) -> u32 {
        match self {
            Self::WlDisplay | Self::WlRegistry | Self::WlCallback | Self::WlBuffer => 1,
            Self::WlCompositor => 6,
            Self::WlSurface => 6,
            Self::WlShm | Self::WlShmPool => 2,
            Self::WlSeat => 9,
            Self::WlKeyboard => 9,
            Self::WlPointer => 9,
            Self::WlDataDeviceManager => 3,
            Self::WlDataDevice | Self::WlDataSource | Self::WlDataOffer => 3,
            Self::XdgWmBase => 6,
            Self::XdgSurface => 6,
            Self::XdgToplevel => 6,
            Self::ZxdgDecorationManagerV1 | Self::ZxdgToplevelDecorationV1 => 1,
            Self::WpViewporter | Self::WpViewport => 1,
            Self::WpFractionalScaleManagerV1 | Self::WpFractionalScaleV1 => 1,
        }
    }
}

/// Metadata tracked for one live protocol object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectInfo {
    /// Interface implemented by the object.
    pub interface: Interface,
    /// Bound protocol version.
    pub version: u32,
}

/// Client-side object-id allocator and interface table.
#[derive(Debug, Clone)]
pub struct ObjectTable {
    next: u32,
    recycled: BTreeSet<u32>,
    objects: BTreeMap<ObjectId, ObjectInfo>,
}

impl Default for ObjectTable {
    fn default() -> Self {
        let mut objects = BTreeMap::new();
        objects.insert(
            ObjectId::DISPLAY,
            ObjectInfo {
                interface: Interface::WlDisplay,
                version: 1,
            },
        );
        Self {
            next: 2,
            recycled: BTreeSet::new(),
            objects,
        }
    }
}

impl ObjectTable {
    /// Allocates and registers a client object id.
    ///
    /// # Errors
    /// Returns [`Error::Refused`] if the object cap or client-id range is exhausted.
    pub fn allocate(&mut self, interface: Interface, version: u32) -> Result<ObjectId> {
        if self.objects.len() >= MAX_OBJECTS {
            return Err(Error::Refused("Wayland object table reached its hard limit".to_owned()));
        }
        let raw = if let Some(value) = self.recycled.pop_first() {
            value
        } else {
            if self.next >= SERVER_ID_START {
                return Err(Error::Refused("Wayland client object id space exhausted".to_owned()));
            }
            let value = self.next;
            self.next = self
                .next
                .checked_add(1)
                .ok_or_else(|| Error::Refused("Wayland object id counter overflow".to_owned()))?;
            value
        };
        let id = ObjectId::new(raw)?;
        let actual_version = version.min(interface.supported_version());
        self.objects.insert(
            id,
            ObjectInfo {
                interface,
                version: actual_version,
            },
        );
        Ok(id)
    }

    /// Registers a server-created object id such as `wl_data_offer`.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for duplicates or a client-range id.
    pub fn register_server(&mut self, id: ObjectId, interface: Interface, version: u32) -> Result<()> {
        if id.raw() < SERVER_ID_START {
            return Err(Error::damaged(
                "server-created Wayland object is inside client id range",
            ));
        }
        if self.objects.contains_key(&id) {
            return Err(Error::damaged("Wayland server reused a live object id"));
        }
        self.objects.insert(
            id,
            ObjectInfo {
                interface,
                version: version.min(interface.supported_version()),
            },
        );
        Ok(())
    }

    /// Removes local metadata for an object that has been destroyed.
    ///
    /// Client ids are deliberately not recycled here: Wayland permits reuse only after
    /// `wl_display.delete_id` confirms that the server has dropped every reference.
    pub fn release(&mut self, id: ObjectId) {
        if id != ObjectId::DISPLAY {
            let _ = self.objects.remove(&id);
        }
    }

    /// Handles `wl_display.delete_id` and makes a client id available for reuse.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] when the server reports a reserved or server-side id.
    pub fn confirm_delete_id(&mut self, raw: u32) -> Result<()> {
        if raw <= ObjectId::DISPLAY.raw() || raw >= SERVER_ID_START {
            return Err(Error::damaged("wl_display.delete_id contains an invalid client id"));
        }
        let id = ObjectId::new(raw)?;
        let _ = self.objects.remove(&id);
        self.recycled.insert(raw);
        Ok(())
    }

    /// Returns metadata for a live object.
    #[must_use]
    pub fn get(&self, id: ObjectId) -> Option<ObjectInfo> {
        self.objects.get(&id).copied()
    }

    /// Ensures that `id` is live and implements `interface`.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for unknown or wrong-interface objects.
    pub fn require(&self, id: ObjectId, interface: Interface) -> Result<ObjectInfo> {
        let info = self
            .get(id)
            .ok_or_else(|| Error::damaged(format!("unknown Wayland object {}", id.raw())))?;
        if info.interface != interface {
            return Err(Error::damaged(format!(
                "Wayland object {} is {}, expected {}",
                id.raw(),
                info.interface.name(),
                interface.name()
            )));
        }
        Ok(info)
    }
}

/// Marshals requests, tracks object ids and delegates byte delivery to a [`Transport`].
#[derive(Debug)]
pub struct Client<T> {
    transport: T,
    objects: ObjectTable,
}

impl<T: Transport> Client<T> {
    /// Creates a client with only the fixed `wl_display` object registered.
    #[must_use]
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            objects: ObjectTable::default(),
        }
    }

    /// Returns the live object table.
    #[must_use]
    pub const fn objects(&self) -> &ObjectTable {
        &self.objects
    }

    /// Returns a mutable reference to the transport, useful for test/script transports.
    #[must_use]
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Sends `wl_display.sync` and returns the callback object id.
    ///
    /// # Errors
    /// Returns allocation, marshalling or transport errors.
    pub fn display_sync(&mut self) -> Result<ObjectId> {
        let id = self.objects.allocate(Interface::WlCallback, 1)?;
        let mut writer = WireWriter::new(ObjectId::DISPLAY, 0);
        writer.new_id(id);
        self.send(writer.finish()?)?;
        Ok(id)
    }

    /// Alias for [`Self::display_sync`].
    pub fn sync(&mut self) -> Result<ObjectId> {
        self.display_sync()
    }

    /// Sends `wl_display.get_registry` and returns the allocated registry object id.
    ///
    /// # Errors
    /// Returns allocation, marshalling or transport errors.
    pub fn get_registry(&mut self) -> Result<ObjectId> {
        let id = self.objects.allocate(Interface::WlRegistry, 1)?;
        let mut writer = WireWriter::new(ObjectId::DISPLAY, 1);
        writer.new_id(id);
        self.send(writer.finish()?)?;
        Ok(id)
    }

    /// Binds one advertised global through `wl_registry.bind`.
    ///
    /// `advertised_version` is clamped to the implementation's supported version.
    ///
    /// # Errors
    /// Returns errors for a bad registry object, allocation, marshalling or transport failure.
    pub fn bind_global(
        &mut self,
        registry: ObjectId,
        name: u32,
        interface: Interface,
        advertised_version: u32,
    ) -> Result<ObjectId> {
        self.objects.require(registry, Interface::WlRegistry)?;
        let version = advertised_version.min(interface.supported_version());
        let id = self.objects.allocate(interface, version)?;
        let mut writer = WireWriter::new(registry, 0);
        writer.uint(name);
        writer.string(interface.name())?;
        writer.uint(version);
        writer.new_id(id);
        self.send(writer.finish()?)?;
        Ok(id)
    }

    /// Creates a `wl_surface` from a compositor.
    pub fn create_surface(&mut self, compositor: ObjectId) -> Result<ObjectId> {
        let compositor_info = self.objects.require(compositor, Interface::WlCompositor)?;
        let id = self.objects.allocate(Interface::WlSurface, compositor_info.version)?;
        let mut writer = WireWriter::new(compositor, 0);
        writer.new_id(id);
        self.send(writer.finish()?)?;
        Ok(id)
    }

    /// Sends `wl_surface.attach`.
    pub fn surface_attach(&mut self, surface: ObjectId, buffer: Option<ObjectId>, x: i32, y: i32) -> Result<()> {
        self.objects.require(surface, Interface::WlSurface)?;
        if let Some(id) = buffer {
            self.objects.require(id, Interface::WlBuffer)?;
        }
        let mut writer = WireWriter::new(surface, 1);
        writer.object(buffer);
        writer.int(x);
        writer.int(y);
        self.send(writer.finish()?)
    }

    /// Sends `wl_surface.damage_buffer`.
    pub fn surface_damage_buffer(&mut self, surface: ObjectId, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        let info = self.objects.require(surface, Interface::WlSurface)?;
        if info.version < 4 {
            return Err(Error::Refused("wl_surface.damage_buffer requires version 4".to_owned()));
        }
        let mut writer = WireWriter::new(surface, 9);
        writer.int(x);
        writer.int(y);
        writer.int(width);
        writer.int(height);
        self.send(writer.finish()?)
    }

    /// Requests a frame callback from a surface.
    pub fn surface_frame(&mut self, surface: ObjectId) -> Result<ObjectId> {
        self.objects.require(surface, Interface::WlSurface)?;
        let callback = self.objects.allocate(Interface::WlCallback, 1)?;
        let mut writer = WireWriter::new(surface, 3);
        writer.new_id(callback);
        self.send(writer.finish()?)?;
        Ok(callback)
    }

    /// Commits pending surface state.
    pub fn surface_commit(&mut self, surface: ObjectId) -> Result<()> {
        self.objects.require(surface, Interface::WlSurface)?;
        self.send(WireWriter::new(surface, 6).finish()?)
    }

    /// Sets integer buffer scale on a surface.
    pub fn surface_set_buffer_scale(&mut self, surface: ObjectId, scale: i32) -> Result<()> {
        let info = self.objects.require(surface, Interface::WlSurface)?;
        if info.version < 3 || scale <= 0 {
            return Err(Error::Refused(
                "wl_surface buffer scale requires v3 and a positive value".to_owned(),
            ));
        }
        let mut writer = WireWriter::new(surface, 8);
        writer.int(scale);
        self.send(writer.finish()?)
    }

    /// Creates a `wp_viewport` object for a surface.
    pub fn viewporter_get_viewport(&mut self, viewporter: ObjectId, surface: ObjectId) -> Result<ObjectId> {
        self.objects.require(viewporter, Interface::WpViewporter)?;
        self.objects.require(surface, Interface::WlSurface)?;
        let id = self.objects.allocate(Interface::WpViewport, 1)?;
        let mut writer = WireWriter::new(viewporter, 1);
        writer.new_id(id);
        writer.object(Some(surface));
        self.send(writer.finish()?)?;
        Ok(id)
    }

    /// Sets or clears the source rectangle of a `wp_viewport`.
    ///
    /// The protocol uses raw 24.8 fixed values; four `-1.0` values clear the source rectangle.
    pub fn viewport_set_source(
        &mut self,
        viewport: ObjectId,
        x: Fixed,
        y: Fixed,
        width: Fixed,
        height: Fixed,
    ) -> Result<()> {
        self.objects.require(viewport, Interface::WpViewport)?;
        let mut writer = WireWriter::new(viewport, 1);
        writer.fixed(x);
        writer.fixed(y);
        writer.fixed(width);
        writer.fixed(height);
        self.send(writer.finish()?)
    }

    /// Sets or clears the destination size of a `wp_viewport`.
    ///
    /// A width and height of `-1` clear the destination size.
    pub fn viewport_set_destination(&mut self, viewport: ObjectId, width: i32, height: i32) -> Result<()> {
        self.objects.require(viewport, Interface::WpViewport)?;
        let mut writer = WireWriter::new(viewport, 2);
        writer.int(width);
        writer.int(height);
        self.send(writer.finish()?)
    }

    /// Creates a `wp_fractional_scale_v1` object for a surface.
    pub fn fractional_scale(&mut self, manager: ObjectId, surface: ObjectId) -> Result<ObjectId> {
        self.objects.require(manager, Interface::WpFractionalScaleManagerV1)?;
        self.objects.require(surface, Interface::WlSurface)?;
        let id = self.objects.allocate(Interface::WpFractionalScaleV1, 1)?;
        let mut writer = WireWriter::new(manager, 1);
        writer.new_id(id);
        writer.object(Some(surface));
        self.send(writer.finish()?)?;
        Ok(id)
    }

    /// Creates a `wl_shm_pool`; `fd` is an opaque descriptor handle understood by the transport.
    pub fn shm_create_pool(&mut self, shm: ObjectId, fd: u32, size: i32) -> Result<ObjectId> {
        let shm_info = self.objects.require(shm, Interface::WlShm)?;
        if size <= 0 {
            return Err(Error::Refused("wl_shm pool size must be positive".to_owned()));
        }
        let pool = self.objects.allocate(Interface::WlShmPool, shm_info.version)?;
        let mut writer = WireWriter::new(shm, 0);
        writer.new_id(pool);
        writer.fd(fd);
        writer.int(size);
        self.send(writer.finish()?)?;
        Ok(pool)
    }

    /// Creates a `wl_buffer` from a shared-memory pool.
    pub fn shm_pool_create_buffer(
        &mut self,
        pool: ObjectId,
        offset: i32,
        width: i32,
        height: i32,
        stride: i32,
        format: u32,
    ) -> Result<ObjectId> {
        self.objects.require(pool, Interface::WlShmPool)?;
        if offset < 0 || width <= 0 || height <= 0 || stride <= 0 {
            return Err(Error::Refused("invalid wl_shm buffer geometry".to_owned()));
        }
        let buffer = self.objects.allocate(Interface::WlBuffer, 1)?;
        let mut writer = WireWriter::new(pool, 0);
        writer.new_id(buffer);
        writer.int(offset);
        writer.int(width);
        writer.int(height);
        writer.int(stride);
        writer.uint(format);
        self.send(writer.finish()?)?;
        Ok(buffer)
    }

    /// Resizes a shared-memory pool.
    pub fn shm_pool_resize(&mut self, pool: ObjectId, size: i32) -> Result<()> {
        self.objects.require(pool, Interface::WlShmPool)?;
        if size <= 0 {
            return Err(Error::Refused("wl_shm pool size must be positive".to_owned()));
        }
        let mut writer = WireWriter::new(pool, 2);
        writer.int(size);
        self.send(writer.finish()?)
    }

    /// Destroys a shared-memory pool object locally after sending the protocol destructor.
    pub fn shm_pool_destroy(&mut self, pool: ObjectId) -> Result<()> {
        self.objects.require(pool, Interface::WlShmPool)?;
        self.send(WireWriter::new(pool, 1).finish()?)?;
        self.objects.release(pool);
        Ok(())
    }

    /// Destroys a `wl_buffer` locally after sending the protocol destructor.
    pub fn buffer_destroy(&mut self, buffer: ObjectId) -> Result<()> {
        self.objects.require(buffer, Interface::WlBuffer)?;
        self.send(WireWriter::new(buffer, 0).finish()?)?;
        self.objects.release(buffer);
        Ok(())
    }

    /// Responds to an `xdg_wm_base.ping`.
    pub fn xdg_pong(&mut self, wm_base: ObjectId, serial: u32) -> Result<()> {
        self.objects.require(wm_base, Interface::XdgWmBase)?;
        let mut writer = WireWriter::new(wm_base, 3);
        writer.uint(serial);
        self.send(writer.finish()?)
    }

    /// Creates an `xdg_surface` for a `wl_surface`.
    pub fn xdg_get_surface(&mut self, wm_base: ObjectId, surface: ObjectId) -> Result<ObjectId> {
        let wm_info = self.objects.require(wm_base, Interface::XdgWmBase)?;
        self.objects.require(surface, Interface::WlSurface)?;
        let id = self.objects.allocate(Interface::XdgSurface, wm_info.version)?;
        let mut writer = WireWriter::new(wm_base, 2);
        writer.new_id(id);
        writer.object(Some(surface));
        self.send(writer.finish()?)?;
        Ok(id)
    }

    /// Creates an `xdg_toplevel` role for an `xdg_surface`.
    pub fn xdg_get_toplevel(&mut self, xdg_surface: ObjectId) -> Result<ObjectId> {
        let surface_info = self.objects.require(xdg_surface, Interface::XdgSurface)?;
        let id = self.objects.allocate(Interface::XdgToplevel, surface_info.version)?;
        let mut writer = WireWriter::new(xdg_surface, 1);
        writer.new_id(id);
        self.send(writer.finish()?)?;
        Ok(id)
    }

    /// Acknowledges an `xdg_surface.configure` serial.
    pub fn xdg_ack_configure(&mut self, xdg_surface: ObjectId, serial: u32) -> Result<()> {
        self.objects.require(xdg_surface, Interface::XdgSurface)?;
        let mut writer = WireWriter::new(xdg_surface, 4);
        writer.uint(serial);
        self.send(writer.finish()?)
    }

    /// Sets an xdg-toplevel UTF-8 title.
    pub fn xdg_toplevel_set_title(&mut self, toplevel: ObjectId, title: &str) -> Result<()> {
        self.objects.require(toplevel, Interface::XdgToplevel)?;
        let mut writer = WireWriter::new(toplevel, 2);
        writer.string(title)?;
        self.send(writer.finish()?)
    }

    /// Sets the stable xdg-toplevel application identifier.
    pub fn xdg_toplevel_set_app_id(&mut self, toplevel: ObjectId, app_id: &str) -> Result<()> {
        self.objects.require(toplevel, Interface::XdgToplevel)?;
        let mut writer = WireWriter::new(toplevel, 3);
        writer.string(app_id)?;
        self.send(writer.finish()?)
    }

    /// Sets the minimum xdg-toplevel size.
    pub fn xdg_toplevel_set_min_size(&mut self, toplevel: ObjectId, width: i32, height: i32) -> Result<()> {
        self.objects.require(toplevel, Interface::XdgToplevel)?;
        if width < 0 || height < 0 {
            return Err(Error::Refused("xdg minimum size cannot be negative".to_owned()));
        }
        let mut writer = WireWriter::new(toplevel, 8);
        writer.int(width);
        writer.int(height);
        self.send(writer.finish()?)
    }

    /// Creates server-side decoration control for an xdg toplevel.
    pub fn decoration_for_toplevel(&mut self, manager: ObjectId, toplevel: ObjectId) -> Result<ObjectId> {
        self.objects.require(manager, Interface::ZxdgDecorationManagerV1)?;
        self.objects.require(toplevel, Interface::XdgToplevel)?;
        let id = self.objects.allocate(Interface::ZxdgToplevelDecorationV1, 1)?;
        let mut writer = WireWriter::new(manager, 1);
        writer.new_id(id);
        writer.object(Some(toplevel));
        self.send(writer.finish()?)?;
        Ok(id)
    }

    /// Requests client-side (`1`) or server-side (`2`) decoration mode.
    pub fn decoration_set_mode(&mut self, decoration: ObjectId, mode: u32) -> Result<()> {
        self.objects.require(decoration, Interface::ZxdgToplevelDecorationV1)?;
        if mode != 1 && mode != 2 {
            return Err(Error::Refused("unknown xdg-decoration mode".to_owned()));
        }
        let mut writer = WireWriter::new(decoration, 1);
        writer.uint(mode);
        self.send(writer.finish()?)
    }

    /// Requests server-side decorations for an xdg toplevel decoration.
    pub fn decoration_set_server_side(&mut self, decoration: ObjectId) -> Result<()> {
        self.decoration_set_mode(decoration, 2)
    }

    /// Creates a keyboard for a seat.
    pub fn seat_get_keyboard(&mut self, seat: ObjectId) -> Result<ObjectId> {
        let seat_info = self.objects.require(seat, Interface::WlSeat)?;
        let id = self.objects.allocate(Interface::WlKeyboard, seat_info.version)?;
        let mut writer = WireWriter::new(seat, 1);
        writer.new_id(id);
        self.send(writer.finish()?)?;
        Ok(id)
    }

    /// Creates a pointer for a seat.
    pub fn seat_get_pointer(&mut self, seat: ObjectId) -> Result<ObjectId> {
        let seat_info = self.objects.require(seat, Interface::WlSeat)?;
        let id = self.objects.allocate(Interface::WlPointer, seat_info.version)?;
        let mut writer = WireWriter::new(seat, 0);
        writer.new_id(id);
        self.send(writer.finish()?)?;
        Ok(id)
    }

    /// Creates a `wl_data_source` used to own clipboard text.
    pub fn data_create_source(&mut self, manager: ObjectId) -> Result<ObjectId> {
        let manager_info = self.objects.require(manager, Interface::WlDataDeviceManager)?;
        let id = self.objects.allocate(Interface::WlDataSource, manager_info.version)?;
        let mut writer = WireWriter::new(manager, 0);
        writer.new_id(id);
        self.send(writer.finish()?)?;
        Ok(id)
    }

    /// Creates a seat-bound data device.
    pub fn data_get_device(&mut self, manager: ObjectId, seat: ObjectId) -> Result<ObjectId> {
        let manager_info = self.objects.require(manager, Interface::WlDataDeviceManager)?;
        self.objects.require(seat, Interface::WlSeat)?;
        let id = self.objects.allocate(Interface::WlDataDevice, manager_info.version)?;
        let mut writer = WireWriter::new(manager, 1);
        writer.new_id(id);
        writer.object(Some(seat));
        self.send(writer.finish()?)?;
        Ok(id)
    }

    /// Offers one MIME type from a clipboard data source.
    pub fn data_source_offer(&mut self, source: ObjectId, mime: &str) -> Result<()> {
        self.objects.require(source, Interface::WlDataSource)?;
        let mut writer = WireWriter::new(source, 0);
        writer.string(mime)?;
        self.send(writer.finish()?)
    }

    /// Makes `source` the current clipboard selection, or clears the selection with `None`.
    pub fn data_device_set_selection(&mut self, device: ObjectId, source: Option<ObjectId>, serial: u32) -> Result<()> {
        self.objects.require(device, Interface::WlDataDevice)?;
        if let Some(id) = source {
            self.objects.require(id, Interface::WlDataSource)?;
        }
        let mut writer = WireWriter::new(device, 1);
        writer.object(source);
        writer.uint(serial);
        self.send(writer.finish()?)
    }

    /// Requests clipboard data from an offer into the passed descriptor handle.
    pub fn data_offer_receive(&mut self, offer: ObjectId, mime: &str, fd: u32) -> Result<()> {
        self.objects.require(offer, Interface::WlDataOffer)?;
        let mut writer = WireWriter::new(offer, 1);
        writer.string(mime)?;
        writer.fd(fd);
        self.send(writer.finish()?)
    }

    /// Receives, decodes and dispatches one event according to the destination object's interface.
    pub fn receive_event(&mut self) -> Result<Event> {
        let mut bytes = vec![0_u8; MAX_MESSAGE_BYTES];
        let mut fds = Vec::new();
        let used = self.transport.receive(&mut bytes, &mut fds)?;
        let packet = bytes
            .get(..used)
            .ok_or_else(|| Error::damaged("Wayland transport returned an oversized byte count"))?;
        let message = Message::decode(packet, &fds)?;
        let info = self
            .objects
            .get(message.object)
            .ok_or_else(|| Error::damaged("event targets an unknown Wayland object"))?;
        let event = decode_event(info.interface, &message)?;
        match event {
            Event::DisplayDeleteId(raw) => {
                self.objects.confirm_delete_id(raw)?;
                Ok(Event::DisplayDeleteId(raw))
            }
            Event::DataOfferCreated { id } => {
                self.objects.register_server(id, Interface::WlDataOffer, info.version)?;
                Ok(Event::DataOfferCreated { id })
            }
            other => Ok(other),
        }
    }

    fn send(&mut self, message: Message) -> Result<()> {
        self.transport.send(&message.bytes, &message.fds)
    }
}

/// Decoded events needed by the window/input/clipboard layers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Fatal `wl_display.error` event.
    DisplayError {
        /// Object blamed by the compositor.
        object: ObjectId,
        /// Protocol-defined error code.
        code: u32,
        /// Human-readable compositor message.
        message: String,
    },
    /// `wl_display.delete_id`; the id may now be recycled by the client.
    DisplayDeleteId(u32),
    /// `wl_registry.global`.
    RegistryGlobal {
        /// Registry numeric global name.
        name: u32,
        /// Protocol interface name.
        interface: String,
        /// Advertised version.
        version: u32,
    },
    /// `wl_registry.global_remove`.
    RegistryGlobalRemove(u32),
    /// `wl_callback.done`.
    CallbackDone(u32),
    /// `wl_shm.format`.
    ShmFormat(u32),
    /// `wl_buffer.release`.
    BufferRelease,
    /// Preferred fractional scale in 120ths.
    FractionalScalePreferred(u32),
    /// `xdg_wm_base.ping`.
    XdgPing(u32),
    /// `xdg_surface.configure`.
    XdgSurfaceConfigure(u32),
    /// `xdg_toplevel.configure`.
    XdgToplevelConfigure {
        /// Suggested width.
        width: i32,
        /// Suggested height.
        height: i32,
        /// Raw array of `xdg_toplevel_state` u32 values.
        states: Vec<u32>,
    },
    /// `xdg_toplevel.close`.
    XdgToplevelClose,
    /// `zxdg_toplevel_decoration_v1.configure`.
    DecorationConfigure(u32),
    /// `wl_seat.capabilities`.
    SeatCapabilities(u32),
    /// `wl_seat.name`.
    SeatName(String),
    /// `wl_keyboard.keymap`.
    KeyboardKeymap {
        /// Keymap format (`1` is XKB v1 text).
        format: u32,
        /// Opaque keymap fd handle.
        fd: u32,
        /// Byte length available through the fd.
        size: u32,
    },
    /// `wl_keyboard.enter`.
    KeyboardEnter {
        /// Input serial.
        serial: u32,
        /// Focused surface.
        surface: ObjectId,
        /// Already-held evdev key numbers.
        keys: Vec<u32>,
    },
    /// `wl_keyboard.leave`.
    KeyboardLeave {
        /// Input serial.
        serial: u32,
        /// Surface losing focus.
        surface: ObjectId,
    },
    /// `wl_keyboard.key`.
    KeyboardKey {
        /// Input serial.
        serial: u32,
        /// Timestamp in compositor units.
        time: u32,
        /// Evdev key number (XKB keycode is normally this value plus eight).
        key: u32,
        /// `0` released, `1` pressed.
        state: u32,
    },
    /// `wl_keyboard.modifiers`.
    KeyboardModifiers {
        /// Input serial.
        serial: u32,
        /// Depressed XKB modifier mask.
        depressed: u32,
        /// Latched XKB modifier mask.
        latched: u32,
        /// Locked XKB modifier mask.
        locked: u32,
        /// Effective group/layout index.
        group: u32,
    },
    /// `wl_keyboard.repeat_info`.
    KeyboardRepeatInfo {
        /// Repeats per second; zero disables repeat.
        rate: i32,
        /// Delay before repeating in milliseconds.
        delay: i32,
    },
    /// `wl_pointer.enter`.
    PointerEnter {
        /// Input serial.
        serial: u32,
        /// Surface entered by the pointer.
        surface: ObjectId,
        /// Surface-local x coordinate.
        x: Fixed,
        /// Surface-local y coordinate.
        y: Fixed,
    },
    /// `wl_pointer.leave`.
    PointerLeave {
        /// Input serial.
        serial: u32,
        /// Surface left by the pointer.
        surface: ObjectId,
    },
    /// `wl_pointer.motion`.
    PointerMotion {
        /// Timestamp.
        time: u32,
        /// Surface-local x coordinate.
        x: Fixed,
        /// Surface-local y coordinate.
        y: Fixed,
    },
    /// `wl_pointer.button`.
    PointerButton {
        /// Input serial.
        serial: u32,
        /// Timestamp.
        time: u32,
        /// Linux input button number.
        button: u32,
        /// `0` released, `1` pressed.
        state: u32,
    },
    /// `wl_pointer.axis`.
    PointerAxis {
        /// Timestamp.
        time: u32,
        /// Axis number.
        axis: u32,
        /// Fixed-point scroll amount.
        value: Fixed,
    },
    /// Marks the end of one logical pointer event group.
    PointerFrame,
    /// A compositor-created `wl_data_offer` object.
    DataOfferCreated {
        /// Server object id.
        id: ObjectId,
    },
    /// MIME type advertised by a data offer.
    DataOfferMime(String),
    /// Current clipboard selection offer, or `None` when cleared.
    DataSelection(Option<ObjectId>),
    /// The compositor asks our data source to write one MIME type to an fd.
    DataSourceSend {
        /// Requested MIME type.
        mime: String,
        /// Opaque descriptor handle to write.
        fd: u32,
    },
    /// Our clipboard data source was cancelled.
    DataSourceCancelled,
}

/// Decodes one event using the destination object's interface.
///
/// # Errors
/// Returns [`Error::Damaged`] for an unknown opcode or malformed argument sequence.
pub fn decode_event(interface: Interface, message: &Message) -> Result<Event> {
    let mut reader = message.arguments()?;
    let event = match (interface, message.opcode) {
        (Interface::WlDisplay, 0) => Event::DisplayError {
            object: reader
                .object()?
                .ok_or_else(|| Error::damaged("wl_display.error has a null object"))?,
            code: reader.uint()?,
            message: reader.string()?,
        },
        (Interface::WlDisplay, 1) => Event::DisplayDeleteId(reader.uint()?),
        (Interface::WlRegistry, 0) => Event::RegistryGlobal {
            name: reader.uint()?,
            interface: reader.string()?,
            version: reader.uint()?,
        },
        (Interface::WlRegistry, 1) => Event::RegistryGlobalRemove(reader.uint()?),
        (Interface::WlCallback, 0) => Event::CallbackDone(reader.uint()?),
        (Interface::WlShm, 0) => Event::ShmFormat(reader.uint()?),
        (Interface::WlBuffer, 0) => Event::BufferRelease,
        (Interface::WpFractionalScaleV1, 0) => Event::FractionalScalePreferred(reader.uint()?),
        (Interface::XdgWmBase, 0) => Event::XdgPing(reader.uint()?),
        (Interface::XdgSurface, 0) => Event::XdgSurfaceConfigure(reader.uint()?),
        (Interface::XdgToplevel, 0) => {
            let width = reader.int()?;
            let height = reader.int()?;
            let raw = reader.array()?;
            Event::XdgToplevelConfigure {
                width,
                height,
                states: array_u32(&raw)?,
            }
        }
        (Interface::XdgToplevel, 1) => Event::XdgToplevelClose,
        (Interface::ZxdgToplevelDecorationV1, 0) => Event::DecorationConfigure(reader.uint()?),
        (Interface::WlSeat, 0) => Event::SeatCapabilities(reader.uint()?),
        (Interface::WlSeat, 1) => Event::SeatName(reader.string()?),
        (Interface::WlKeyboard, 0) => Event::KeyboardKeymap {
            format: reader.uint()?,
            fd: reader.fd()?,
            size: reader.uint()?,
        },
        (Interface::WlKeyboard, 1) => {
            let serial = reader.uint()?;
            let surface = reader
                .object()?
                .ok_or_else(|| Error::damaged("keyboard enter has null surface"))?;
            let keys = array_u32(&reader.array()?)?;
            Event::KeyboardEnter { serial, surface, keys }
        }
        (Interface::WlKeyboard, 2) => Event::KeyboardLeave {
            serial: reader.uint()?,
            surface: reader
                .object()?
                .ok_or_else(|| Error::damaged("keyboard leave has null surface"))?,
        },
        (Interface::WlKeyboard, 3) => Event::KeyboardKey {
            serial: reader.uint()?,
            time: reader.uint()?,
            key: reader.uint()?,
            state: reader.uint()?,
        },
        (Interface::WlKeyboard, 4) => Event::KeyboardModifiers {
            serial: reader.uint()?,
            depressed: reader.uint()?,
            latched: reader.uint()?,
            locked: reader.uint()?,
            group: reader.uint()?,
        },
        (Interface::WlKeyboard, 5) => Event::KeyboardRepeatInfo {
            rate: reader.int()?,
            delay: reader.int()?,
        },
        (Interface::WlPointer, 0) => Event::PointerEnter {
            serial: reader.uint()?,
            surface: reader
                .object()?
                .ok_or_else(|| Error::damaged("pointer enter has null surface"))?,
            x: reader.fixed()?,
            y: reader.fixed()?,
        },
        (Interface::WlPointer, 1) => Event::PointerLeave {
            serial: reader.uint()?,
            surface: reader
                .object()?
                .ok_or_else(|| Error::damaged("pointer leave has null surface"))?,
        },
        (Interface::WlPointer, 2) => Event::PointerMotion {
            time: reader.uint()?,
            x: reader.fixed()?,
            y: reader.fixed()?,
        },
        (Interface::WlPointer, 3) => Event::PointerButton {
            serial: reader.uint()?,
            time: reader.uint()?,
            button: reader.uint()?,
            state: reader.uint()?,
        },
        (Interface::WlPointer, 4) => Event::PointerAxis {
            time: reader.uint()?,
            axis: reader.uint()?,
            value: reader.fixed()?,
        },
        (Interface::WlPointer, 5) => Event::PointerFrame,
        (Interface::WlDataDevice, 0) => Event::DataOfferCreated { id: reader.new_id()? },
        (Interface::WlDataDevice, 5) => Event::DataSelection(reader.object()?),
        (Interface::WlDataOffer, 0) => Event::DataOfferMime(reader.string()?),
        (Interface::WlDataSource, 1) => Event::DataSourceSend {
            mime: reader.string()?,
            fd: reader.fd()?,
        },
        (Interface::WlDataSource, 2) => Event::DataSourceCancelled,
        _ => {
            return Err(Error::damaged(format!(
                "unsupported {} event opcode {}",
                interface.name(),
                message.opcode
            )))
        }
    };
    if reader.remaining() != 0 || reader.remaining_fds() != 0 {
        return Err(Error::damaged("Wayland event has trailing arguments"));
    }
    Ok(event)
}

/// Effective keyboard modifiers consumed by [`XkbKeymap::keysym`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    /// Shift is active.
    pub shift: bool,
    /// Caps/shift lock is active.
    pub lock: bool,
    /// ISO level-three modifier is active.
    pub level3: bool,
    /// Effective layout group (`0` US, `1` Russian in the reproduced fixture).
    pub group: u32,
}

impl KeyModifiers {
    /// Converts the conventional XKB core modifier masks carried by `wl_keyboard.modifiers`.
    ///
    /// `Shift` is core bit 0, `Lock` bit 1 and the usual `ISO_Level3_Shift` mapping is `Mod5`
    /// (core bit 7). The effective group is supplied separately by Wayland.
    #[must_use]
    pub const fn from_xkb_masks(depressed: u32, latched: u32, locked: u32, group: u32) -> Self {
        let active = depressed | latched | locked;
        Self {
            shift: active & 0x01 != 0,
            lock: active & 0x02 != 0,
            level3: active & 0x80 != 0,
            group,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct XkbType {
    modifiers: u8,
    maps: BTreeMap<u8, usize>,
}

#[derive(Debug, Clone, Default)]
struct XkbGroup {
    type_name: Option<String>,
    levels: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
struct XkbKey {
    groups: Vec<XkbGroup>,
}

/// Parsed subset of an XKB v1 text keymap sufficient for ordinary US/Russian desktop text input.
#[derive(Debug, Clone, Default)]
pub struct XkbKeymap {
    keys: BTreeMap<u32, XkbKey>,
    types: BTreeMap<String, XkbType>,
    /// Whether the compatibility section contains a group-switch action such as `LockGroup`.
    pub has_group_switch_compat: bool,
}

impl XkbKeymap {
    /// Parses `xkb_keycodes`, `xkb_types`, `xkb_symbols` and enough `xkb_compat` to recognise layout switching.
    ///
    /// This is intentionally not a general XKB compiler. It consumes the canonical text form compositors send
    /// through `wl_keyboard.keymap`: named keycodes, type maps to `LevelN`, symbol lists per group and group-lock
    /// compatibility actions.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for missing mandatory sections, malformed keycodes/types/symbols or excessive
    /// input.
    pub fn parse(text: &str) -> Result<Self> {
        if text.len() > MAX_ARRAY_BYTES {
            return Err(Error::Refused("XKB text keymap exceeds hard limit".to_owned()));
        }
        let keycodes_block = section_block(text, "xkb_keycodes")?;
        let types_block = section_block(text, "xkb_types")?;
        let symbols_block = section_block(text, "xkb_symbols")?;
        let compat_block = section_block(text, "xkb_compat")?;

        let keycodes = parse_keycodes(keycodes_block)?;
        let types = parse_types(types_block)?;
        let keys = parse_symbols(symbols_block, &keycodes)?;
        if keys.is_empty() {
            return Err(Error::damaged("XKB symbols section contains no keys"));
        }
        Ok(Self {
            keys,
            types,
            has_group_switch_compat: compat_block.contains("LockGroup")
                || compat_block.contains("LatchGroup")
                || compat_block.contains("SetGroup"),
        })
    }

    /// Resolves an XKB keycode plus effective modifiers/group to a keysym.
    #[must_use]
    pub fn keysym(&self, keycode: u32, modifiers: KeyModifiers) -> Option<u32> {
        let key = self.keys.get(&keycode)?;
        let group_len_u32 = u32::try_from(key.groups.len()).ok()?;
        if group_len_u32 == 0 {
            return None;
        }
        let group_index_u32 = modifiers.group.checked_rem(group_len_u32)?;
        let group_index = usize::try_from(group_index_u32).ok()?;
        let group = key.groups.get(group_index)?;
        let level = self.level_for_group(group, modifiers);
        group
            .levels
            .get(level)
            .copied()
            .or_else(|| group.levels.first().copied())
    }

    /// Resolves a key directly to a Unicode character when the keysym represents text.
    #[must_use]
    pub fn character(&self, keycode: u32, modifiers: KeyModifiers) -> Option<char> {
        self.keysym(keycode, modifiers).and_then(keysym_to_char)
    }

    /// Resolves the evdev key number carried by `wl_keyboard.key` to Unicode.
    ///
    /// The Wayland keyboard protocol reports evdev numbers, while XKB text keymaps use keycodes that are eight
    /// larger for the standard evdev keycode set.
    #[must_use]
    pub fn character_from_evdev(&self, key: u32, modifiers: KeyModifiers) -> Option<char> {
        key.checked_add(8)
            .and_then(|keycode| self.character(keycode, modifiers))
    }

    fn level_for_group(&self, group: &XkbGroup, modifiers: KeyModifiers) -> usize {
        if let Some(name) = group.type_name.as_ref() {
            if let Some(rule) = self.types.get(name) {
                let active = modifier_bits(modifiers) & rule.modifiers;
                if let Some(level) = rule.maps.get(&active) {
                    return *level;
                }
            }
        }
        match group.levels.len() {
            0 | 1 => 0,
            2 => {
                let alphabetic = group
                    .levels
                    .first()
                    .copied()
                    .and_then(keysym_to_char)
                    .zip(group.levels.get(1).copied().and_then(keysym_to_char))
                    .is_some_and(|(a, b)| a.to_lowercase().to_string() == b.to_lowercase().to_string());
                if alphabetic {
                    usize::from(modifiers.shift != modifiers.lock)
                } else {
                    usize::from(modifiers.shift)
                }
            }
            _ => {
                let high = if modifiers.level3 { 2_usize } else { 0_usize };
                high.checked_add(usize::from(modifiers.shift)).unwrap_or_default()
            }
        }
    }
}

/// Converts a supported X11/XKB keysym to Unicode.
///
/// Latin-1 keysyms and `0x01000000 | Unicode` are handled generically; the legacy Cyrillic block used by
/// `xkeyboard-config` is mapped explicitly.
#[must_use]
pub fn keysym_to_char(keysym: u32) -> Option<char> {
    if (0x20..=0x7e).contains(&keysym) || (0xa0..=0xff).contains(&keysym) {
        return char::from_u32(keysym);
    }
    if keysym & 0xff00_0000 == 0x0100_0000 {
        return char::from_u32(keysym & 0x00ff_ffff);
    }
    keypad_to_char(keysym)
        .or_else(|| legacy_cyrillic_to_char(keysym))
        .or_else(|| legacy_greek_to_char(keysym))
}

fn modifier_bits(modifiers: KeyModifiers) -> u8 {
    let mut bits = 0_u8;
    if modifiers.shift {
        bits |= 1;
    }
    if modifiers.lock {
        bits |= 2;
    }
    if modifiers.level3 {
        bits |= 4;
    }
    bits
}

fn parse_keycodes(block: &str) -> Result<BTreeMap<String, u32>> {
    let mut result = BTreeMap::new();
    for statement in block.split(';') {
        let Some(open) = statement.find('<') else {
            continue;
        };
        let after_open = open
            .checked_add(1)
            .ok_or_else(|| Error::damaged("XKB keycode offset overflow"))?;
        let Some(relative_close) = statement.get(after_open..).and_then(|rest| rest.find('>')) else {
            continue;
        };
        let close = after_open
            .checked_add(relative_close)
            .ok_or_else(|| Error::damaged("XKB keycode close offset overflow"))?;
        let name = statement
            .get(after_open..close)
            .ok_or_else(|| Error::damaged("XKB keycode name range invalid"))?
            .trim();
        let Some(eq) = statement.find('=') else {
            continue;
        };
        let number = statement
            .get(eq.saturating_add(1)..)
            .ok_or_else(|| Error::damaged("XKB keycode value range invalid"))?
            .trim()
            .parse::<u32>()
            .map_err(|_| Error::damaged("invalid numeric XKB keycode"))?;
        result.insert(name.to_owned(), number);
    }
    if result.is_empty() {
        Err(Error::damaged("XKB keycodes section contains no assignments"))
    } else {
        Ok(result)
    }
}

fn parse_types(block: &str) -> Result<BTreeMap<String, XkbType>> {
    let mut result = BTreeMap::new();
    let mut cursor = 0_usize;
    while let Some(relative) = block.get(cursor..).and_then(|rest| rest.find("type")) {
        let start = cursor
            .checked_add(relative)
            .ok_or_else(|| Error::damaged("XKB type cursor overflow"))?;
        let Some(quote_rel) = block.get(start..).and_then(|rest| rest.find('"')) else {
            break;
        };
        let quote = start
            .checked_add(quote_rel)
            .ok_or_else(|| Error::damaged("XKB type quote overflow"))?;
        let name_start = quote
            .checked_add(1)
            .ok_or_else(|| Error::damaged("XKB type name offset overflow"))?;
        let Some(name_end_rel) = block.get(name_start..).and_then(|rest| rest.find('"')) else {
            return Err(Error::damaged("unterminated XKB type name"));
        };
        let name_end = name_start
            .checked_add(name_end_rel)
            .ok_or_else(|| Error::damaged("XKB type name end overflow"))?;
        let name = block
            .get(name_start..name_end)
            .ok_or_else(|| Error::damaged("XKB type name range invalid"))?
            .to_owned();
        let Some(open_rel) = block.get(name_end..).and_then(|rest| rest.find('{')) else {
            return Err(Error::damaged("XKB type has no block"));
        };
        let open = name_end
            .checked_add(open_rel)
            .ok_or_else(|| Error::damaged("XKB type block offset overflow"))?;
        let close = matching_brace(block, open)?;
        let body_start = open
            .checked_add(1)
            .ok_or_else(|| Error::damaged("XKB type body offset overflow"))?;
        let body = block
            .get(body_start..close)
            .ok_or_else(|| Error::damaged("XKB type body range invalid"))?;
        result.insert(name, parse_type_body(body)?);
        cursor = close
            .checked_add(1)
            .ok_or_else(|| Error::damaged("XKB type cursor advance overflow"))?;
    }
    Ok(result)
}

fn parse_type_body(body: &str) -> Result<XkbType> {
    let mut rule = XkbType::default();
    rule.maps.insert(0, 0);
    for statement in body.split(';') {
        let trimmed = statement.trim();
        if let Some(value) = trimmed.strip_prefix("modifiers") {
            let Some(eq) = value.find('=') else {
                continue;
            };
            let mods = value.get(eq.saturating_add(1)..).unwrap_or_default();
            rule.modifiers = parse_modifier_names(mods);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("map[") {
            let Some(close) = rest.find(']') else {
                return Err(Error::damaged("XKB type map has no closing bracket"));
            };
            let names = rest.get(..close).unwrap_or_default();
            let Some(eq) = rest.find('=') else {
                continue;
            };
            let level_text = rest.get(eq.saturating_add(1)..).unwrap_or_default().trim();
            let level_number = level_text
                .strip_prefix("Level")
                .ok_or_else(|| Error::damaged("XKB type map target is not LevelN"))?
                .trim()
                .parse::<usize>()
                .map_err(|_| Error::damaged("invalid XKB LevelN value"))?;
            let level = level_number
                .checked_sub(1)
                .ok_or_else(|| Error::damaged("XKB levels start at one"))?;
            rule.maps.insert(parse_modifier_names(names), level);
        }
    }
    Ok(rule)
}

fn parse_modifier_names(text: &str) -> u8 {
    let mut bits = 0_u8;
    for name in text.split('+').map(str::trim) {
        match name {
            "Shift" => bits |= 1,
            "Lock" => bits |= 2,
            "LevelThree" | "Mod5" | "ISO_Level3_Shift" => bits |= 4,
            _ => {}
        }
    }
    bits
}

fn parse_symbols(block: &str, keycodes: &BTreeMap<String, u32>) -> Result<BTreeMap<u32, XkbKey>> {
    let mut result = BTreeMap::new();
    let mut cursor = 0_usize;
    while let Some(relative) = block.get(cursor..).and_then(|rest| rest.find("key")) {
        let start = cursor
            .checked_add(relative)
            .ok_or_else(|| Error::damaged("XKB symbol cursor overflow"))?;
        let Some(angle_rel) = block.get(start..).and_then(|rest| rest.find('<')) else {
            break;
        };
        let angle = start
            .checked_add(angle_rel)
            .ok_or_else(|| Error::damaged("XKB symbol key offset overflow"))?;
        let name_start = angle
            .checked_add(1)
            .ok_or_else(|| Error::damaged("XKB symbol key name overflow"))?;
        let Some(close_angle_rel) = block.get(name_start..).and_then(|rest| rest.find('>')) else {
            return Err(Error::damaged("unterminated XKB symbol key name"));
        };
        let close_angle = name_start
            .checked_add(close_angle_rel)
            .ok_or_else(|| Error::damaged("XKB symbol key end overflow"))?;
        let name = block
            .get(name_start..close_angle)
            .ok_or_else(|| Error::damaged("XKB symbol key name range invalid"))?;
        let Some(open_rel) = block.get(close_angle..).and_then(|rest| rest.find('{')) else {
            return Err(Error::damaged("XKB key symbols have no block"));
        };
        let open = close_angle
            .checked_add(open_rel)
            .ok_or_else(|| Error::damaged("XKB symbol block offset overflow"))?;
        let close = matching_brace(block, open)?;
        let body_start = open
            .checked_add(1)
            .ok_or_else(|| Error::damaged("XKB symbol body offset overflow"))?;
        let body = block
            .get(body_start..close)
            .ok_or_else(|| Error::damaged("XKB symbol body range invalid"))?;
        if let Some(code) = keycodes.get(name).copied() {
            result.insert(code, parse_key_body(body)?);
        }
        cursor = close
            .checked_add(1)
            .ok_or_else(|| Error::damaged("XKB symbol cursor advance overflow"))?;
    }
    Ok(result)
}

fn parse_key_body(body: &str) -> Result<XkbKey> {
    let mut groups: Vec<XkbGroup> = Vec::new();
    for group_number in 1_usize..=4_usize {
        let marker = format!("symbols[Group{group_number}]");
        if let Some(position) = body.find(&marker) {
            let after = position
                .checked_add(marker.len())
                .ok_or_else(|| Error::damaged("XKB symbol group marker overflow"))?;
            if let Some(list) = bracket_list_after(body, after)? {
                ensure_group(&mut groups, group_number)?;
                if let Some(group) = groups.get_mut(group_number.saturating_sub(1)) {
                    group.levels = parse_keysym_list(list)?;
                }
            }
        }
        let type_marker = format!("type[Group{group_number}]");
        if let Some(position) = body.find(&type_marker) {
            let after = position
                .checked_add(type_marker.len())
                .ok_or_else(|| Error::damaged("XKB type group marker overflow"))?;
            if let Some(name) = quoted_after(body, after)? {
                ensure_group(&mut groups, group_number)?;
                if let Some(group) = groups.get_mut(group_number.saturating_sub(1)) {
                    group.type_name = Some(name.to_owned());
                }
            }
        }
    }

    if groups.iter().all(|group| group.levels.is_empty()) {
        let lists = top_level_bracket_lists(body)?;
        for (offset, list) in lists.iter().enumerate() {
            let group_number = offset
                .checked_add(1)
                .ok_or_else(|| Error::damaged("XKB inferred group number overflow"))?;
            ensure_group(&mut groups, group_number)?;
            if let Some(group) = groups.get_mut(offset) {
                group.levels = parse_keysym_list(list)?;
            }
        }
    }

    while groups
        .last()
        .is_some_and(|group| group.levels.is_empty() && group.type_name.is_none())
    {
        let _ = groups.pop();
    }
    Ok(XkbKey { groups })
}

fn ensure_group(groups: &mut Vec<XkbGroup>, group_number: usize) -> Result<()> {
    if group_number == 0 || group_number > 4 {
        return Err(Error::damaged("XKB group number outside 1..4"));
    }
    while groups.len() < group_number {
        groups.push(XkbGroup::default());
    }
    Ok(())
}

fn parse_keysym_list(list: &str) -> Result<Vec<u32>> {
    let mut result = Vec::new();
    for raw in list.split(',') {
        let name = raw.trim();
        if name.is_empty() || name == "NoSymbol" {
            continue;
        }
        result.push(keysym_from_name(name)?);
    }
    Ok(result)
}

fn keysym_from_name(name: &str) -> Result<u32> {
    if let Some(hex) = name.strip_prefix('U') {
        if hex.len() >= 4 && hex.len() <= 8 && hex.chars().all(|value| value.is_ascii_hexdigit()) {
            let code = u32::from_str_radix(hex, 16).map_err(|_| Error::damaged("invalid Unicode XKB keysym"))?;
            return 0x0100_0000_u32
                .checked_add(code)
                .ok_or_else(|| Error::damaged("Unicode keysym overflow"));
        }
    }
    if name.chars().count() == 1 {
        let character = name
            .chars()
            .next()
            .ok_or_else(|| Error::damaged("empty XKB keysym name"))?;
        return Ok(u32::from(character));
    }
    let value = match name {
        "space" => 0x20,
        "exclam" => 0x21,
        "quotedbl" => 0x22,
        "numbersign" => 0x23,
        "dollar" => 0x24,
        "percent" => 0x25,
        "ampersand" => 0x26,
        "apostrophe" => 0x27,
        "parenleft" => 0x28,
        "parenright" => 0x29,
        "asterisk" => 0x2a,
        "plus" => 0x2b,
        "comma" => 0x2c,
        "minus" => 0x2d,
        "period" => 0x2e,
        "slash" => 0x2f,
        "colon" => 0x3a,
        "semicolon" => 0x3b,
        "less" => 0x3c,
        "equal" => 0x3d,
        "greater" => 0x3e,
        "question" => 0x3f,
        "at" => 0x40,
        "bracketleft" => 0x5b,
        "backslash" => 0x5c,
        "bracketright" => 0x5d,
        "asciicircum" => 0x5e,
        "underscore" => 0x5f,
        "grave" => 0x60,
        "braceleft" => 0x7b,
        "bar" => 0x7c,
        "braceright" => 0x7d,
        "asciitilde" => 0x7e,
        "ISO_Level3_Shift" => 0xfe03,
        "ISO_Next_Group" => 0xfe08,
        "Cyrillic_io" => 0x06a3,
        "Cyrillic_IO" => 0x06b3,
        "Cyrillic_yu" => 0x06c0,
        "Cyrillic_a" => 0x06c1,
        "Cyrillic_be" => 0x06c2,
        "Cyrillic_tse" => 0x06c3,
        "Cyrillic_de" => 0x06c4,
        "Cyrillic_ie" => 0x06c5,
        "Cyrillic_ef" => 0x06c6,
        "Cyrillic_ghe" => 0x06c7,
        "Cyrillic_ha" => 0x06c8,
        "Cyrillic_i" => 0x06c9,
        "Cyrillic_shorti" => 0x06ca,
        "Cyrillic_ka" => 0x06cb,
        "Cyrillic_el" => 0x06cc,
        "Cyrillic_em" => 0x06cd,
        "Cyrillic_en" => 0x06ce,
        "Cyrillic_o" => 0x06cf,
        "Cyrillic_pe" => 0x06d0,
        "Cyrillic_ya" => 0x06d1,
        "Cyrillic_er" => 0x06d2,
        "Cyrillic_es" => 0x06d3,
        "Cyrillic_te" => 0x06d4,
        "Cyrillic_u" => 0x06d5,
        "Cyrillic_zhe" => 0x06d6,
        "Cyrillic_ve" => 0x06d7,
        "Cyrillic_softsign" => 0x06d8,
        "Cyrillic_yeru" => 0x06d9,
        "Cyrillic_ze" => 0x06da,
        "Cyrillic_sha" => 0x06db,
        "Cyrillic_e" => 0x06dc,
        "Cyrillic_shcha" => 0x06dd,
        "Cyrillic_che" => 0x06de,
        "Cyrillic_hardsign" => 0x06df,
        "Cyrillic_YU" => 0x06e0,
        "Cyrillic_A" => 0x06e1,
        "Cyrillic_BE" => 0x06e2,
        "Cyrillic_TSE" => 0x06e3,
        "Cyrillic_DE" => 0x06e4,
        "Cyrillic_IE" => 0x06e5,
        "Cyrillic_EF" => 0x06e6,
        "Cyrillic_GHE" => 0x06e7,
        "Cyrillic_HA" => 0x06e8,
        "Cyrillic_I" => 0x06e9,
        "Cyrillic_SHORTI" => 0x06ea,
        "Cyrillic_KA" => 0x06eb,
        "Cyrillic_EL" => 0x06ec,
        "Cyrillic_EM" => 0x06ed,
        "Cyrillic_EN" => 0x06ee,
        "Cyrillic_O" => 0x06ef,
        "Cyrillic_PE" => 0x06f0,
        "Cyrillic_YA" => 0x06f1,
        "Cyrillic_ER" => 0x06f2,
        "Cyrillic_ES" => 0x06f3,
        "Cyrillic_TE" => 0x06f4,
        "Cyrillic_U" => 0x06f5,
        "Cyrillic_ZHE" => 0x06f6,
        "Cyrillic_VE" => 0x06f7,
        "Cyrillic_SOFTSIGN" => 0x06f8,
        "Cyrillic_YERU" => 0x06f9,
        "Cyrillic_ZE" => 0x06fa,
        "Cyrillic_SHA" => 0x06fb,
        "Cyrillic_E" => 0x06fc,
        "Cyrillic_SHCHA" => 0x06fd,
        "Cyrillic_CHE" => 0x06fe,
        "Cyrillic_HARDSIGN" => 0x06ff,
        _ => {
            return greek_keysym_from_name(name)
                .or_else(|| keypad_keysym_from_name(name))
                .ok_or_else(|| Error::damaged(format!("unsupported XKB keysym name {name}")));
        }
    };
    Ok(value)
}

fn keypad_keysym_from_name(name: &str) -> Option<u32> {
    match name {
        "KP_Space" => Some(0xff80),
        "KP_Tab" => Some(0xff89),
        "KP_Enter" => Some(0xff8d),
        "KP_Multiply" => Some(0xffaa),
        "KP_Add" => Some(0xffab),
        "KP_Separator" => Some(0xffac),
        "KP_Subtract" => Some(0xffad),
        "KP_Decimal" => Some(0xffae),
        "KP_Divide" => Some(0xffaf),
        "KP_0" => Some(0xffb0),
        "KP_1" => Some(0xffb1),
        "KP_2" => Some(0xffb2),
        "KP_3" => Some(0xffb3),
        "KP_4" => Some(0xffb4),
        "KP_5" => Some(0xffb5),
        "KP_6" => Some(0xffb6),
        "KP_7" => Some(0xffb7),
        "KP_8" => Some(0xffb8),
        "KP_9" => Some(0xffb9),
        "KP_Equal" => Some(0xffbd),
        _ => None,
    }
}

fn greek_keysym_from_name(name: &str) -> Option<u32> {
    match name {
        "Greek_ALPHAaccent" => Some(0x07a1),
        "Greek_EPSILONaccent" => Some(0x07a2),
        "Greek_ETAaccent" => Some(0x07a3),
        "Greek_IOTAaccent" => Some(0x07a4),
        "Greek_IOTAdieresis" | "Greek_IOTAdiaeresis" => Some(0x07a5),
        "Greek_OMICRONaccent" => Some(0x07a7),
        "Greek_UPSILONaccent" => Some(0x07a8),
        "Greek_UPSILONdieresis" => Some(0x07a9),
        "Greek_OMEGAaccent" => Some(0x07ab),
        "Greek_accentdieresis" => Some(0x07ae),
        "Greek_horizbar" => Some(0x07af),
        "Greek_alphaaccent" => Some(0x07b1),
        "Greek_epsilonaccent" => Some(0x07b2),
        "Greek_etaaccent" => Some(0x07b3),
        "Greek_iotaaccent" => Some(0x07b4),
        "Greek_iotadieresis" => Some(0x07b5),
        "Greek_iotaaccentdieresis" => Some(0x07b6),
        "Greek_omicronaccent" => Some(0x07b7),
        "Greek_upsilonaccent" => Some(0x07b8),
        "Greek_upsilondieresis" => Some(0x07b9),
        "Greek_upsilonaccentdieresis" => Some(0x07ba),
        "Greek_omegaaccent" => Some(0x07bb),
        "Greek_ALPHA" => Some(0x07c1),
        "Greek_BETA" => Some(0x07c2),
        "Greek_GAMMA" => Some(0x07c3),
        "Greek_DELTA" => Some(0x07c4),
        "Greek_EPSILON" => Some(0x07c5),
        "Greek_ZETA" => Some(0x07c6),
        "Greek_ETA" => Some(0x07c7),
        "Greek_THETA" => Some(0x07c8),
        "Greek_IOTA" => Some(0x07c9),
        "Greek_KAPPA" => Some(0x07ca),
        "Greek_LAMDA" | "Greek_LAMBDA" => Some(0x07cb),
        "Greek_MU" => Some(0x07cc),
        "Greek_NU" => Some(0x07cd),
        "Greek_XI" => Some(0x07ce),
        "Greek_OMICRON" => Some(0x07cf),
        "Greek_PI" => Some(0x07d0),
        "Greek_RHO" => Some(0x07d1),
        "Greek_SIGMA" => Some(0x07d2),
        "Greek_TAU" => Some(0x07d4),
        "Greek_UPSILON" => Some(0x07d5),
        "Greek_PHI" => Some(0x07d6),
        "Greek_CHI" => Some(0x07d7),
        "Greek_PSI" => Some(0x07d8),
        "Greek_OMEGA" => Some(0x07d9),
        "Greek_alpha" => Some(0x07e1),
        "Greek_beta" => Some(0x07e2),
        "Greek_gamma" => Some(0x07e3),
        "Greek_delta" => Some(0x07e4),
        "Greek_epsilon" => Some(0x07e5),
        "Greek_zeta" => Some(0x07e6),
        "Greek_eta" => Some(0x07e7),
        "Greek_theta" => Some(0x07e8),
        "Greek_iota" => Some(0x07e9),
        "Greek_kappa" => Some(0x07ea),
        "Greek_lamda" | "Greek_lambda" => Some(0x07eb),
        "Greek_mu" => Some(0x07ec),
        "Greek_nu" => Some(0x07ed),
        "Greek_xi" => Some(0x07ee),
        "Greek_omicron" => Some(0x07ef),
        "Greek_pi" => Some(0x07f0),
        "Greek_rho" => Some(0x07f1),
        "Greek_sigma" => Some(0x07f2),
        "Greek_finalsmallsigma" => Some(0x07f3),
        "Greek_tau" => Some(0x07f4),
        "Greek_upsilon" => Some(0x07f5),
        "Greek_phi" => Some(0x07f6),
        "Greek_chi" => Some(0x07f7),
        "Greek_psi" => Some(0x07f8),
        "Greek_omega" => Some(0x07f9),
        _ => None,
    }
}

fn keypad_to_char(keysym: u32) -> Option<char> {
    match keysym {
        0xff80 => Some(' '),
        0xff89 => Some('\t'),
        0xff8d => Some('\n'),
        0xffaa => Some('*'),
        0xffab => Some('+'),
        0xffac => Some(','),
        0xffad => Some('-'),
        0xffae => Some('.'),
        0xffaf => Some('/'),
        0xffb0 => Some('0'),
        0xffb1 => Some('1'),
        0xffb2 => Some('2'),
        0xffb3 => Some('3'),
        0xffb4 => Some('4'),
        0xffb5 => Some('5'),
        0xffb6 => Some('6'),
        0xffb7 => Some('7'),
        0xffb8 => Some('8'),
        0xffb9 => Some('9'),
        0xffbd => Some('='),
        _ => None,
    }
}

fn legacy_greek_to_char(keysym: u32) -> Option<char> {
    match keysym {
        0x07a1 => Some('Ά'),
        0x07a2 => Some('Έ'),
        0x07a3 => Some('Ή'),
        0x07a4 => Some('Ί'),
        0x07a5 => Some('Ϊ'),
        0x07a7 => Some('Ό'),
        0x07a8 => Some('Ύ'),
        0x07a9 => Some('Ϋ'),
        0x07ab => Some('Ώ'),
        0x07ae => Some('΅'),
        0x07af => Some('―'),
        0x07b1 => Some('ά'),
        0x07b2 => Some('έ'),
        0x07b3 => Some('ή'),
        0x07b4 => Some('ί'),
        0x07b5 => Some('ϊ'),
        0x07b6 => Some('ΐ'),
        0x07b7 => Some('ό'),
        0x07b8 => Some('ύ'),
        0x07b9 => Some('ϋ'),
        0x07ba => Some('ΰ'),
        0x07bb => Some('ώ'),
        0x07c1 => Some('Α'),
        0x07c2 => Some('Β'),
        0x07c3 => Some('Γ'),
        0x07c4 => Some('Δ'),
        0x07c5 => Some('Ε'),
        0x07c6 => Some('Ζ'),
        0x07c7 => Some('Η'),
        0x07c8 => Some('Θ'),
        0x07c9 => Some('Ι'),
        0x07ca => Some('Κ'),
        0x07cb => Some('Λ'),
        0x07cc => Some('Μ'),
        0x07cd => Some('Ν'),
        0x07ce => Some('Ξ'),
        0x07cf => Some('Ο'),
        0x07d0 => Some('Π'),
        0x07d1 => Some('Ρ'),
        0x07d2 => Some('Σ'),
        0x07d4 => Some('Τ'),
        0x07d5 => Some('Υ'),
        0x07d6 => Some('Φ'),
        0x07d7 => Some('Χ'),
        0x07d8 => Some('Ψ'),
        0x07d9 => Some('Ω'),
        0x07e1 => Some('α'),
        0x07e2 => Some('β'),
        0x07e3 => Some('γ'),
        0x07e4 => Some('δ'),
        0x07e5 => Some('ε'),
        0x07e6 => Some('ζ'),
        0x07e7 => Some('η'),
        0x07e8 => Some('θ'),
        0x07e9 => Some('ι'),
        0x07ea => Some('κ'),
        0x07eb => Some('λ'),
        0x07ec => Some('μ'),
        0x07ed => Some('ν'),
        0x07ee => Some('ξ'),
        0x07ef => Some('ο'),
        0x07f0 => Some('π'),
        0x07f1 => Some('ρ'),
        0x07f2 => Some('σ'),
        0x07f3 => Some('ς'),
        0x07f4 => Some('τ'),
        0x07f5 => Some('υ'),
        0x07f6 => Some('φ'),
        0x07f7 => Some('χ'),
        0x07f8 => Some('ψ'),
        0x07f9 => Some('ω'),
        _ => None,
    }
}

fn legacy_cyrillic_to_char(keysym: u32) -> Option<char> {
    let character = match keysym {
        0x06a3 => 'ё',
        0x06b3 => 'Ё',
        0x06c0 => 'ю',
        0x06c1 => 'а',
        0x06c2 => 'б',
        0x06c3 => 'ц',
        0x06c4 => 'д',
        0x06c5 => 'е',
        0x06c6 => 'ф',
        0x06c7 => 'г',
        0x06c8 => 'х',
        0x06c9 => 'и',
        0x06ca => 'й',
        0x06cb => 'к',
        0x06cc => 'л',
        0x06cd => 'м',
        0x06ce => 'н',
        0x06cf => 'о',
        0x06d0 => 'п',
        0x06d1 => 'я',
        0x06d2 => 'р',
        0x06d3 => 'с',
        0x06d4 => 'т',
        0x06d5 => 'у',
        0x06d6 => 'ж',
        0x06d7 => 'в',
        0x06d8 => 'ь',
        0x06d9 => 'ы',
        0x06da => 'з',
        0x06db => 'ш',
        0x06dc => 'э',
        0x06dd => 'щ',
        0x06de => 'ч',
        0x06df => 'ъ',
        0x06e0 => 'Ю',
        0x06e1 => 'А',
        0x06e2 => 'Б',
        0x06e3 => 'Ц',
        0x06e4 => 'Д',
        0x06e5 => 'Е',
        0x06e6 => 'Ф',
        0x06e7 => 'Г',
        0x06e8 => 'Х',
        0x06e9 => 'И',
        0x06ea => 'Й',
        0x06eb => 'К',
        0x06ec => 'Л',
        0x06ed => 'М',
        0x06ee => 'Н',
        0x06ef => 'О',
        0x06f0 => 'П',
        0x06f1 => 'Я',
        0x06f2 => 'Р',
        0x06f3 => 'С',
        0x06f4 => 'Т',
        0x06f5 => 'У',
        0x06f6 => 'Ж',
        0x06f7 => 'В',
        0x06f8 => 'Ь',
        0x06f9 => 'Ы',
        0x06fa => 'З',
        0x06fb => 'Ш',
        0x06fc => 'Э',
        0x06fd => 'Щ',
        0x06fe => 'Ч',
        0x06ff => 'Ъ',
        _ => return None,
    };
    Some(character)
}

fn section_block<'a>(text: &'a str, name: &str) -> Result<&'a str> {
    let start = text
        .find(name)
        .ok_or_else(|| Error::damaged(format!("XKB section {name} is missing")))?;
    let open_rel = text
        .get(start..)
        .and_then(|rest| rest.find('{'))
        .ok_or_else(|| Error::damaged(format!("XKB section {name} has no opening brace")))?;
    let open = start
        .checked_add(open_rel)
        .ok_or_else(|| Error::damaged("XKB section offset overflow"))?;
    let close = matching_brace(text, open)?;
    let body_start = open
        .checked_add(1)
        .ok_or_else(|| Error::damaged("XKB section body offset overflow"))?;
    text.get(body_start..close)
        .ok_or_else(|| Error::damaged("XKB section body range invalid"))
}

fn matching_brace(text: &str, open: usize) -> Result<usize> {
    if text.as_bytes().get(open).copied() != Some(b'{') {
        return Err(Error::damaged("brace scanner did not start on an opening brace"));
    }
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, byte) in text
        .as_bytes()
        .get(open..)
        .ok_or_else(|| Error::damaged("brace scan range invalid"))?
        .iter()
        .copied()
        .enumerate()
    {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        if byte == b'"' {
            in_string = true;
        } else if byte == b'{' {
            depth = depth
                .checked_add(1)
                .ok_or_else(|| Error::damaged("XKB brace depth overflow"))?;
        } else if byte == b'}' {
            depth = depth
                .checked_sub(1)
                .ok_or_else(|| Error::damaged("XKB brace depth underflow"))?;
            if depth == 0 {
                return open
                    .checked_add(offset)
                    .ok_or_else(|| Error::damaged("XKB closing brace offset overflow"));
            }
        }
    }
    Err(Error::damaged("unterminated XKB brace block"))
}

fn bracket_list_after(text: &str, offset: usize) -> Result<Option<&str>> {
    let Some(relative) = text.get(offset..).and_then(|rest| rest.find('[')) else {
        return Ok(None);
    };
    let open = offset
        .checked_add(relative)
        .ok_or_else(|| Error::damaged("XKB list offset overflow"))?;
    let content_start = open
        .checked_add(1)
        .ok_or_else(|| Error::damaged("XKB list start overflow"))?;
    let Some(close_rel) = text.get(content_start..).and_then(|rest| rest.find(']')) else {
        return Err(Error::damaged("unterminated XKB symbol list"));
    };
    let close = content_start
        .checked_add(close_rel)
        .ok_or_else(|| Error::damaged("XKB list end overflow"))?;
    Ok(text.get(content_start..close))
}

fn quoted_after(text: &str, offset: usize) -> Result<Option<&str>> {
    let Some(first_rel) = text.get(offset..).and_then(|rest| rest.find('"')) else {
        return Ok(None);
    };
    let first = offset
        .checked_add(first_rel)
        .ok_or_else(|| Error::damaged("XKB quote offset overflow"))?;
    let content = first
        .checked_add(1)
        .ok_or_else(|| Error::damaged("XKB quoted content offset overflow"))?;
    let Some(second_rel) = text.get(content..).and_then(|rest| rest.find('"')) else {
        return Err(Error::damaged("unterminated XKB quoted string"));
    };
    let second = content
        .checked_add(second_rel)
        .ok_or_else(|| Error::damaged("XKB quote end overflow"))?;
    Ok(text.get(content..second))
}

fn top_level_bracket_lists(text: &str) -> Result<Vec<&str>> {
    let mut result = Vec::new();
    let mut cursor = 0_usize;
    while let Some(relative) = text.get(cursor..).and_then(|rest| rest.find('[')) {
        let open = cursor
            .checked_add(relative)
            .ok_or_else(|| Error::damaged("XKB bracket-list cursor overflow"))?;
        let content = open
            .checked_add(1)
            .ok_or_else(|| Error::damaged("XKB bracket-list content overflow"))?;
        let Some(close_rel) = text.get(content..).and_then(|rest| rest.find(']')) else {
            return Err(Error::damaged("unterminated XKB bracket list"));
        };
        let close = content
            .checked_add(close_rel)
            .ok_or_else(|| Error::damaged("XKB bracket-list close overflow"))?;
        result.push(
            text.get(content..close)
                .ok_or_else(|| Error::damaged("XKB bracket-list range invalid"))?,
        );
        cursor = close
            .checked_add(1)
            .ok_or_else(|| Error::damaged("XKB bracket-list advance overflow"))?;
    }
    Ok(result)
}

fn align4(length: usize) -> Result<usize> {
    length
        .checked_add(3)
        .and_then(|value| value.checked_div(4))
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| Error::damaged("four-byte alignment overflow"))
}

fn pad_vec_4(bytes: &mut Vec<u8>) -> Result<()> {
    let target = align4(bytes.len())?;
    if target > MAX_MESSAGE_BYTES {
        return Err(Error::Refused("Wayland message padding exceeds wire limit".to_owned()));
    }
    bytes.resize(target, 0);
    Ok(())
}

fn read_u32_ne(bytes: &[u8], offset: usize) -> Result<u32> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| Error::damaged("Wayland u32 offset overflow"))?;
    let raw = bytes
        .get(offset..end)
        .ok_or_else(|| Error::damaged("Wayland u32 is truncated"))?;
    let array = <[u8; 4]>::try_from(raw).map_err(|_| Error::damaged("Wayland u32 width mismatch"))?;
    Ok(u32::from_ne_bytes(array))
}

fn array_u32(bytes: &[u8]) -> Result<Vec<u32>> {
    if bytes.len().checked_rem(4) != Some(0) {
        return Err(Error::damaged("Wayland u32 array is not four-byte aligned"));
    }
    let mut result = Vec::with_capacity(bytes.len().checked_div(4).unwrap_or_default());
    for chunk in bytes.chunks_exact(4) {
        let array = <[u8; 4]>::try_from(chunk).map_err(|_| Error::damaged("Wayland array chunk width mismatch"))?;
        result.push(u32::from_ne_bytes(array));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{
        decode_event, keysym_to_char, Client, Event, Fixed, Interface, KeyModifiers, Message, ObjectId, Transport,
        WireReader, WireWriter, XkbKeymap,
    };
    use sse_core::{Error, Result};
    use std::collections::VecDeque;

    #[derive(Debug, Default)]
    struct ScriptTransport {
        sent: Vec<(Vec<u8>, Vec<u32>)>,
        incoming: VecDeque<(Vec<u8>, Vec<u32>)>,
    }

    impl Transport for ScriptTransport {
        fn send(&mut self, bytes: &[u8], fds: &[u32]) -> Result<()> {
            self.sent.push((bytes.to_vec(), fds.to_vec()));
            Ok(())
        }

        fn receive(&mut self, buf: &mut [u8], fds: &mut Vec<u32>) -> Result<usize> {
            let (bytes, handles) = self
                .incoming
                .pop_front()
                .ok_or_else(|| Error::System("script transport is empty".to_owned()))?;
            let target = buf
                .get_mut(..bytes.len())
                .ok_or_else(|| Error::System("script receive buffer is too small".to_owned()))?;
            target.copy_from_slice(&bytes);
            fds.extend_from_slice(&handles);
            Ok(bytes.len())
        }
    }

    fn framed(object: u32, opcode: u16, payload: &[u8]) -> Vec<u8> {
        let size = 8_usize.saturating_add(payload.len());
        let size_u16 = u16::try_from(size).unwrap_or_default();
        let word = u32::from(size_u16).checked_shl(16).unwrap_or_default() | u32::from(opcode);
        let mut result = Vec::new();
        result.extend_from_slice(&object.to_ne_bytes());
        result.extend_from_slice(&word.to_ne_bytes());
        result.extend_from_slice(payload);
        result
    }

    #[test]
    fn wire_round_trip_covers_all_argument_shapes() {
        let object = ObjectId::new(7).unwrap_or(ObjectId::DISPLAY);
        let child = ObjectId::new(9).unwrap_or(ObjectId::DISPLAY);
        let mut writer = WireWriter::new(object, 5);
        writer.int(-7);
        writer.uint(42);
        writer.fixed(Fixed::from_raw(384));
        writer.object(Some(child));
        writer.new_id(child);
        assert_eq!(writer.string("тест"), Ok(()));
        assert_eq!(writer.array(&[1, 2, 3]), Ok(()));
        writer.fd(99);
        let message = writer.finish();
        assert!(message.is_ok());
        let message = message.unwrap_or_else(|_| unreachable!());
        assert_eq!(Message::decode(&message.bytes, &message.fds), Ok(message.clone()));
        let mut reader = message.arguments().unwrap_or_else(|_| unreachable!());
        assert_eq!(reader.int(), Ok(-7));
        assert_eq!(reader.uint(), Ok(42));
        assert_eq!(reader.fixed(), Ok(Fixed::from_raw(384)));
        assert_eq!(reader.object(), Ok(Some(child)));
        assert_eq!(reader.new_id(), Ok(child));
        assert_eq!(reader.string(), Ok("тест".to_owned()));
        assert_eq!(reader.array(), Ok(vec![1, 2, 3]));
        assert_eq!(reader.fd(), Ok(99));
        assert_eq!(reader.remaining(), 0);
        assert_eq!(reader.remaining_fds(), 0);
    }

    #[test]
    fn hand_built_registry_global_event_decodes() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&17_u32.to_ne_bytes());
        let text = b"wl_compositor\0";
        payload.extend_from_slice(&u32::try_from(text.len()).unwrap_or_default().to_ne_bytes());
        payload.extend_from_slice(text);
        while payload.len().checked_rem(4) != Some(0) {
            payload.push(0);
        }
        payload.extend_from_slice(&6_u32.to_ne_bytes());
        let bytes = framed(2, 0, &payload);
        let message = Message::decode(&bytes, &[]).unwrap_or_else(|_| unreachable!());
        assert_eq!(
            decode_event(Interface::WlRegistry, &message),
            Ok(Event::RegistryGlobal {
                name: 17,
                interface: "wl_compositor".to_owned(),
                version: 6,
            })
        );
    }

    #[test]
    fn client_marshals_registry_surface_frame_and_shm_fd() {
        let transport = ScriptTransport::default();
        let mut client = Client::new(transport);
        let registry = client.get_registry().unwrap_or_else(|_| unreachable!());
        let compositor = client
            .bind_global(registry, 12, Interface::WlCompositor, 99)
            .unwrap_or_else(|_| unreachable!());
        let surface = client.create_surface(compositor).unwrap_or_else(|_| unreachable!());
        let _callback = client.surface_frame(surface).unwrap_or_else(|_| unreachable!());
        let shm = client
            .bind_global(registry, 13, Interface::WlShm, 1)
            .unwrap_or_else(|_| unreachable!());
        let _pool = client.shm_create_pool(shm, 77, 4096).unwrap_or_else(|_| unreachable!());
        let sent = &client.transport_mut().sent;
        assert_eq!(sent.len(), 6);
        assert_eq!(
            sent.last().map(|entry| entry.1.as_slice()),
            Some(std::slice::from_ref(&77_u32))
        );
    }

    #[test]
    fn keyboard_keymap_event_consumes_fd_out_of_band() {
        let keyboard = ObjectId::new(15).unwrap_or(ObjectId::DISPLAY);
        let mut payload = Vec::new();
        payload.extend_from_slice(&1_u32.to_ne_bytes());
        payload.extend_from_slice(&1234_u32.to_ne_bytes());
        let bytes = framed(keyboard.raw(), 0, &payload);
        let message = Message::decode(&bytes, &[55]).unwrap_or_else(|_| unreachable!());
        assert_eq!(
            decode_event(Interface::WlKeyboard, &message),
            Ok(Event::KeyboardKeymap {
                format: 1,
                fd: 55,
                size: 1234,
            })
        );
    }

    #[test]
    fn malformed_lengths_and_fd_counts_are_rejected() {
        let mut writer = WireWriter::new(ObjectId::DISPLAY, 0);
        writer.uint(1);
        let message = writer.finish().unwrap_or_else(|_| unreachable!());
        let mut broken = message.bytes.clone();
        if let Some(byte) = broken.get_mut(6) {
            *byte = 0xff;
        }
        assert!(Message::decode(&broken, &[]).is_err());
        let mut reader = WireReader::new(&[], &[]);
        assert!(reader.fd().is_err());
    }

    // Reproduced from memory rather than copied from a machine. The section names, keycode numbers, type syntax,
    // Group1/Group2 symbol structure and common us/ru symbols follow the canonical xkeyboard-config output, but
    // comments, includes, virtual-modifier boilerplate and many non-text keys are intentionally omitted.
    const US_RU_KEYMAP: &str = r#"
        xkb_keymap {
          xkb_keycodes "evdev" {
            <TLDE> = 49; <AE01> = 10; <AE02> = 11;
            <AD01> = 24; <AD02> = 25; <AD03> = 26; <AD04> = 27;
            <AC01> = 38; <AC02> = 39; <AB01> = 52; <SPCE> = 65;
          };
          xkb_types "complete" {
            type "TWO_LEVEL" { modifiers = Shift; map[Shift] = Level2; };
            type "ALPHABETIC" {
              modifiers = Shift+Lock;
              map[Shift] = Level2;
              map[Lock] = Level2;
              map[Shift+Lock] = Level1;
            };
            type "FOUR_LEVEL" {
              modifiers = Shift+LevelThree;
              map[Shift] = Level2;
              map[LevelThree] = Level3;
              map[Shift+LevelThree] = Level4;
            };
          };
          xkb_compat "complete" {
            interpret ISO_Next_Group+AnyOfOrNone(all) { action= LockGroup(group=+1); };
          };
          xkb_symbols "pc+us+ru" {
            key <TLDE> { type[Group1]="FOUR_LEVEL", symbols[Group1]=[ grave, asciitilde, U0060, U007E ],
                         type[Group2]="FOUR_LEVEL", symbols[Group2]=[ Cyrillic_io, Cyrillic_IO, U0060, U007E ] };
            key <AE01> { [ 1, exclam ], [ 1, exclam ] };
            key <AE02> { [ 2, at ], [ 2, quotedbl ] };
            key <AD01> { type[Group1]="ALPHABETIC", symbols[Group1]=[ q, Q ],
                         type[Group2]="ALPHABETIC", symbols[Group2]=[ Cyrillic_shorti, Cyrillic_SHORTI ] };
            key <AD02> { type[Group1]="ALPHABETIC", symbols[Group1]=[ w, W ],
                         type[Group2]="ALPHABETIC", symbols[Group2]=[ Cyrillic_tse, Cyrillic_TSE ] };
            key <AD03> { type[Group1]="ALPHABETIC", symbols[Group1]=[ e, E ],
                         type[Group2]="ALPHABETIC", symbols[Group2]=[ Cyrillic_u, Cyrillic_U ] };
            key <AD04> { type[Group1]="ALPHABETIC", symbols[Group1]=[ r, R ],
                         type[Group2]="ALPHABETIC", symbols[Group2]=[ Cyrillic_ka, Cyrillic_KA ] };
            key <AC01> { type[Group1]="ALPHABETIC", symbols[Group1]=[ a, A ],
                         type[Group2]="ALPHABETIC", symbols[Group2]=[ Cyrillic_ef, Cyrillic_EF ] };
            key <AC02> { type[Group1]="ALPHABETIC", symbols[Group1]=[ s, S ],
                         type[Group2]="ALPHABETIC", symbols[Group2]=[ Cyrillic_yeru, Cyrillic_YERU ] };
            key <AB01> { type[Group1]="ALPHABETIC", symbols[Group1]=[ z, Z ],
                         type[Group2]="ALPHABETIC", symbols[Group2]=[ Cyrillic_ya, Cyrillic_YA ] };
            key <SPCE> { [ space ], [ space ] };
          };
        };
    "#;

    #[test]
    fn reproduced_us_ru_keymap_maps_shift_lock_level3_and_group() {
        let map = XkbKeymap::parse(US_RU_KEYMAP).unwrap_or_else(|_| unreachable!());
        assert!(map.has_group_switch_compat);
        assert_eq!(map.character(24, KeyModifiers::default()), Some('q'));
        assert_eq!(
            map.character(
                24,
                KeyModifiers {
                    shift: true,
                    ..KeyModifiers::default()
                }
            ),
            Some('Q')
        );
        assert_eq!(
            map.character(
                24,
                KeyModifiers {
                    lock: true,
                    ..KeyModifiers::default()
                }
            ),
            Some('Q')
        );
        assert_eq!(
            map.character(
                24,
                KeyModifiers {
                    shift: true,
                    lock: true,
                    ..KeyModifiers::default()
                }
            ),
            Some('q')
        );
        assert_eq!(
            map.character(
                24,
                KeyModifiers {
                    group: 1,
                    ..KeyModifiers::default()
                }
            ),
            Some('й')
        );
        assert_eq!(
            map.character(
                24,
                KeyModifiers {
                    shift: true,
                    group: 1,
                    ..KeyModifiers::default()
                }
            ),
            Some('Й')
        );
        assert_eq!(
            map.character(
                49,
                KeyModifiers {
                    level3: true,
                    ..KeyModifiers::default()
                }
            ),
            Some('`')
        );
        assert_eq!(
            map.character(
                49,
                KeyModifiers {
                    shift: true,
                    level3: true,
                    ..KeyModifiers::default()
                }
            ),
            Some('~')
        );
    }

    #[test]
    fn cyrillic_keysym_table_covers_reference_letters() {
        let cases = [
            (0x06a3, 'ё'),
            (0x06b3, 'Ё'),
            (0x06c1, 'а'),
            (0x06ca, 'й'),
            (0x06d1, 'я'),
            (0x06db, 'ш'),
            (0x06e1, 'А'),
            (0x06ea, 'Й'),
            (0x06f1, 'Я'),
            (0x06fb, 'Ш'),
        ];
        for (keysym, character) in cases {
            assert_eq!(keysym_to_char(keysym), Some(character));
        }
    }

    #[test]
    fn clipboard_requests_are_marshaled_without_fd_bytes() {
        let transport = ScriptTransport::default();
        let mut client = Client::new(transport);
        let registry = client.get_registry().unwrap_or_else(|_| unreachable!());
        let manager = client
            .bind_global(registry, 3, Interface::WlDataDeviceManager, 3)
            .unwrap_or_else(|_| unreachable!());
        let seat = client
            .bind_global(registry, 4, Interface::WlSeat, 9)
            .unwrap_or_else(|_| unreachable!());
        let source = client.data_create_source(manager).unwrap_or_else(|_| unreachable!());
        let device = client.data_get_device(manager, seat).unwrap_or_else(|_| unreachable!());
        assert_eq!(client.data_source_offer(source, "text/plain;charset=utf-8"), Ok(()));
        assert_eq!(client.data_device_set_selection(device, Some(source), 77), Ok(()));
        assert!(client.transport_mut().sent.len() >= 7);
    }
    #[test]
    fn client_ids_are_reused_only_after_display_delete_id() {
        let mut table = super::ObjectTable::default();
        let first = table
            .allocate(Interface::WlRegistry, 1)
            .unwrap_or_else(|_| unreachable!());
        table.release(first);
        let second = table
            .allocate(Interface::WlRegistry, 1)
            .unwrap_or_else(|_| unreachable!());
        assert_ne!(first, second);
        assert_eq!(table.confirm_delete_id(first.raw()), Ok(()));
        let recycled = table
            .allocate(Interface::WlRegistry, 1)
            .unwrap_or_else(|_| unreachable!());
        assert_eq!(recycled, first);
    }

    #[test]
    fn xkb_core_masks_and_evdev_offset_map_text() {
        let map = XkbKeymap::parse(US_RU_KEYMAP).unwrap_or_else(|_| unreachable!());
        let upper = KeyModifiers::from_xkb_masks(0x01, 0, 0, 0);
        assert_eq!(map.character_from_evdev(16, upper), Some('Q'));
        let russian = KeyModifiers::from_xkb_masks(0, 0, 0, 1);
        assert_eq!(map.character_from_evdev(16, russian), Some('й'));
        let level3 = KeyModifiers::from_xkb_masks(0x80, 0, 0, 0);
        assert_eq!(map.character_from_evdev(41, level3), Some('`'));
    }

    #[test]
    fn malformed_wire_lengths_are_errors() {
        assert!(Message::decode(&[0; 7], &[]).is_err());

        let mut too_small = Vec::new();
        too_small.extend_from_slice(&1_u32.to_ne_bytes());
        too_small.extend_from_slice(&(4_u32.checked_shl(16).unwrap_or_default()).to_ne_bytes());
        assert!(Message::decode(&too_small, &[]).is_err());

        let mut missing_nul = Vec::new();
        missing_nul.extend_from_slice(&4_u32.to_ne_bytes());
        missing_nul.extend_from_slice(b"abcd");
        let mut string_reader = WireReader::new(&missing_nul, &[]);
        assert!(string_reader.string().is_err());

        let mut long_array = Vec::new();
        long_array.extend_from_slice(&8_u32.to_ne_bytes());
        long_array.extend_from_slice(&[1, 2, 3, 4]);
        let mut array_reader = WireReader::new(&long_array, &[]);
        assert!(array_reader.array().is_err());
    }

    #[test]
    fn extended_requests_cover_sync_viewporter_app_id_shm_and_server_decorations() {
        let transport = ScriptTransport::default();
        let mut client = Client::new(transport);
        let sync = client.display_sync();
        assert!(sync.is_ok());

        let registry = client.get_registry().unwrap_or_else(|_| unreachable!());
        let compositor = client
            .bind_global(registry, 1, Interface::WlCompositor, 6)
            .unwrap_or_else(|_| unreachable!());
        let fractional_manager = client
            .bind_global(registry, 2, Interface::WpFractionalScaleManagerV1, 1)
            .unwrap_or_else(|_| unreachable!());
        let viewporter = client
            .bind_global(registry, 3, Interface::WpViewporter, 1)
            .unwrap_or_else(|_| unreachable!());
        let shm = client
            .bind_global(registry, 4, Interface::WlShm, 2)
            .unwrap_or_else(|_| unreachable!());
        let wm_base = client
            .bind_global(registry, 5, Interface::XdgWmBase, 6)
            .unwrap_or_else(|_| unreachable!());
        let decoration_manager = client
            .bind_global(registry, 6, Interface::ZxdgDecorationManagerV1, 1)
            .unwrap_or_else(|_| unreachable!());

        let surface = client.create_surface(compositor).unwrap_or_else(|_| unreachable!());
        assert_eq!(client.surface_attach(surface, None, 0, 0), Ok(()));
        assert_eq!(client.surface_damage_buffer(surface, 1, 2, 3, 4), Ok(()));
        assert!(client.surface_frame(surface).is_ok());
        assert_eq!(client.surface_set_buffer_scale(surface, 2), Ok(()));
        assert_eq!(client.surface_commit(surface), Ok(()));

        assert!(client.fractional_scale(fractional_manager, surface).is_ok());
        let viewport = client
            .viewporter_get_viewport(viewporter, surface)
            .unwrap_or_else(|_| unreachable!());
        assert_eq!(
            client.viewport_set_source(
                viewport,
                Fixed::from_raw(0),
                Fixed::from_raw(0),
                Fixed::from_raw(2560),
                Fixed::from_raw(1280),
            ),
            Ok(())
        );
        assert_eq!(client.viewport_set_destination(viewport, 10, 5), Ok(()));

        let pool = client.shm_create_pool(shm, 91, 16_384).unwrap_or_else(|_| unreachable!());
        let buffer = client
            .shm_pool_create_buffer(pool, 0, 64, 64, 256, 0)
            .unwrap_or_else(|_| unreachable!());
        assert_eq!(client.shm_pool_resize(pool, 32_768), Ok(()));
        assert_eq!(client.surface_attach(surface, Some(buffer), 0, 0), Ok(()));

        let xdg_surface = client.xdg_get_surface(wm_base, surface).unwrap_or_else(|_| unreachable!());
        let toplevel = client.xdg_get_toplevel(xdg_surface).unwrap_or_else(|_| unreachable!());
        assert_eq!(client.xdg_ack_configure(xdg_surface, 77), Ok(()));
        assert_eq!(client.xdg_toplevel_set_title(toplevel, "SSE"), Ok(()));
        assert_eq!(client.xdg_toplevel_set_app_id(toplevel, "sse"), Ok(()));
        assert_eq!(client.xdg_toplevel_set_min_size(toplevel, 640, 480), Ok(()));
        assert_eq!(client.xdg_pong(wm_base, 88), Ok(()));

        let decoration = client
            .decoration_for_toplevel(decoration_manager, toplevel)
            .unwrap_or_else(|_| unreachable!());
        assert_eq!(client.decoration_set_server_side(decoration), Ok(()));

        let sent = &client.transport_mut().sent;
        assert!(sent.iter().any(|(bytes, _)| {
            Message::decode(bytes, &[])
                .is_ok_and(|message| message.object == ObjectId::DISPLAY && message.opcode == 0)
        }));
        assert!(sent.iter().any(|(bytes, fds)| fds.as_slice() == [91] && Message::decode(bytes, fds).is_ok()));
    }

    #[test]
    fn hand_built_extended_events_decode() {
        let buffer_release = Message::decode(&framed(30, 0, &[]), &[]).unwrap_or_else(|_| unreachable!());
        assert_eq!(decode_event(Interface::WlBuffer, &buffer_release), Ok(Event::BufferRelease));

        let pointer_frame = Message::decode(&framed(31, 5, &[]), &[]).unwrap_or_else(|_| unreachable!());
        assert_eq!(decode_event(Interface::WlPointer, &pointer_frame), Ok(Event::PointerFrame));

        let scale_payload = 150_u32.to_ne_bytes();
        let scale = Message::decode(&framed(32, 0, &scale_payload), &[]).unwrap_or_else(|_| unreachable!());
        assert_eq!(
            decode_event(Interface::WpFractionalScaleV1, &scale),
            Ok(Event::FractionalScalePreferred(150))
        );

        let close = Message::decode(&framed(33, 1, &[]), &[]).unwrap_or_else(|_| unreachable!());
        assert_eq!(decode_event(Interface::XdgToplevel, &close), Ok(Event::XdgToplevelClose));
    }

    #[test]
    fn greek_and_keypad_keysyms_map_to_text() {
        assert_eq!(keysym_to_char(0x07c1), Some('Α'));
        assert_eq!(keysym_to_char(0x07f9), Some('ω'));
        assert_eq!(keysym_to_char(0xffb0), Some('0'));
        assert_eq!(keysym_to_char(0xffb9), Some('9'));
        assert_eq!(keysym_to_char(0xffab), Some('+'));
        assert_eq!(keysym_to_char(0xff8d), Some('\n'));
    }

}
