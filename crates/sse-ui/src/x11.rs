//! Dependency-free X11 core/XKB/MIT-SHM wire codec used by the native UI backend.
//!
//! This module deliberately contains no socket or OS calls. The caller owns the transport and
//! feeds/receives exact protocol byte sequences. All length arithmetic and slicing is checked.

use sse_core::{Error, Result};
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

const SETUP_MAJOR: u16 = 11;
const SETUP_MINOR: u16 = 0;
const CORE_PACKET: usize = 32;
const MAX_AUTH_FIELD: usize = 64 * 1024;
const MAX_SETUP_ITEMS: usize = 1 << 16;
const MAX_PROPERTY_BYTES: usize = 64 * 1024 * 1024;
const INCR_THRESHOLD: usize = 256 * 1024;

/// Minimal synchronous byte transport. Implementations must fill every byte passed to `receive`.
pub trait Transport {
    /// Sends all bytes in `data`.
    fn send(&mut self, data: &[u8]);
    /// Receives exactly `out.len()` bytes into `out`.
    fn receive(&mut self, out: &mut [u8]);
}

/// Byte order used by the connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteOrder {
    /// Least-significant byte first (`'l'`).
    Little,
    /// Most-significant byte first (`'B'`).
    Big,
}

impl ByteOrder {
    fn marker(self) -> u8 {
        match self {
            Self::Little => b'l',
            Self::Big => b'B',
        }
    }
}

/// One `.Xauthority` record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XAuthorityEntry {
    /// Xauthority family value.
    pub family: u16,
    /// Binary address bytes.
    pub address: Vec<u8>,
    /// Display number as stored by Xauthority.
    pub number: Vec<u8>,
    /// Authentication protocol name.
    pub name: Vec<u8>,
    /// Authentication token.
    pub data: Vec<u8>,
}

/// Authentication material for X11 setup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Auth {
    /// Protocol name, normally `MIT-MAGIC-COOKIE-1`.
    pub name: Vec<u8>,
    /// Cookie bytes.
    pub data: Vec<u8>,
}

/// Reads and parses a `.Xauthority` file.
pub fn read_xauthority(path: &Path) -> Result<Vec<XAuthorityEntry>> {
    let data = std::fs::read(path)?;
    parse_xauthority(&data)
}

/// Parses Xauthority records. Xauthority itself is always big-endian.
pub fn parse_xauthority(data: &[u8]) -> Result<Vec<XAuthorityEntry>> {
    let mut rd = Reader::new(data, ByteOrder::Big);
    let mut out = Vec::new();
    while rd.remaining() != 0 {
        let family = rd.u16()?;
        let address = rd.counted_be(MAX_AUTH_FIELD)?;
        let number = rd.counted_be(MAX_AUTH_FIELD)?;
        let name = rd.counted_be(MAX_AUTH_FIELD)?;
        let auth = rd.counted_be(MAX_AUTH_FIELD)?;
        out.push(XAuthorityEntry {
            family,
            address,
            number,
            name,
            data: auth,
        });
        if out.len() > MAX_SETUP_ITEMS {
            return Err(Error::damaged("too many Xauthority entries"));
        }
    }
    Ok(out)
}

/// Finds a MIT-MAGIC-COOKIE-1 entry for `display_number`.
///
/// Family/address matching is intentionally left to the caller when multiple hosts are present;
/// this function picks the first cookie with an empty or matching display number.
pub fn find_mit_cookie(entries: &[XAuthorityEntry], display_number: u16) -> Option<Auth> {
    let display = display_number.to_string();
    entries.iter().find_map(|entry| {
        if entry.name.as_slice() != b"MIT-MAGIC-COOKIE-1" {
            return None;
        }
        if !entry.number.is_empty() && entry.number.as_slice() != display.as_bytes() {
            return None;
        }
        Some(Auth {
            name: entry.name.clone(),
            data: entry.data.clone(),
        })
    })
}

/// Encodes the initial X11 setup request.
pub fn encode_setup(order: ByteOrder, auth: Option<&Auth>) -> Result<Vec<u8>> {
    let empty = Auth {
        name: Vec::new(),
        data: Vec::new(),
    };
    let selected = auth.unwrap_or(&empty);
    let name_len = to_u16(selected.name.len(), "auth name too large")?;
    let data_len = to_u16(selected.data.len(), "auth data too large")?;
    let mut out = Vec::with_capacity(checked_add(
        12,
        checked_add(pad4(selected.name.len()), pad4(selected.data.len()))?,
    )?);
    out.push(order.marker());
    out.push(0);
    push_u16(&mut out, order, SETUP_MAJOR);
    push_u16(&mut out, order, SETUP_MINOR);
    push_u16(&mut out, order, name_len);
    push_u16(&mut out, order, data_len);
    push_u16(&mut out, order, 0);
    push_padded(&mut out, &selected.name)?;
    push_padded(&mut out, &selected.data)?;
    Ok(out)
}

/// Parsed server setup information.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    /// Server protocol major.
    pub protocol_major: u16,
    /// Server protocol minor.
    pub protocol_minor: u16,
    /// Release number.
    pub release_number: u32,
    /// Resource-id base.
    pub resource_id_base: u32,
    /// Resource-id mask.
    pub resource_id_mask: u32,
    /// Motion buffer size.
    pub motion_buffer_size: u32,
    /// Vendor string.
    pub vendor: Vec<u8>,
    /// Maximum request length in 4-byte units before BIG-REQUESTS.
    pub maximum_request_length: u16,
    /// Pixmap format declarations.
    pub pixmap_formats: Vec<PixmapFormat>,
    /// Root screens.
    pub screens: Vec<Screen>,
}

/// Pixmap format record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixmapFormat {
    /// Depth.
    pub depth: u8,
    /// Bits per pixel.
    pub bits_per_pixel: u8,
    /// Scanline padding in bits.
    pub scanline_pad: u8,
}

/// One X11 screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    /// Root window id.
    pub root: u32,
    /// Default colormap.
    pub default_colormap: u32,
    /// White pixel value.
    pub white_pixel: u32,
    /// Black pixel value.
    pub black_pixel: u32,
    /// Current input event mask.
    pub current_input_masks: u32,
    /// Pixel width.
    pub width_pixels: u16,
    /// Pixel height.
    pub height_pixels: u16,
    /// Root visual id.
    pub root_visual: u32,
    /// Backing-store policy.
    pub backing_stores: u8,
    /// Whether save-unders are supported.
    pub save_unders: bool,
    /// Root depth.
    pub root_depth: u8,
    /// Allowed depths.
    pub depths: Vec<Depth>,
}

/// Visual depth block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Depth {
    /// Depth value.
    pub depth: u8,
    /// Visuals at this depth.
    pub visuals: Vec<Visual>,
}

/// X11 visual description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Visual {
    /// Visual id.
    pub id: u32,
    /// Visual class.
    pub class: u8,
    /// Bits per RGB value.
    pub bits_per_rgb: u8,
    /// Colormap entries.
    pub colormap_entries: u16,
    /// Red mask.
    pub red_mask: u32,
    /// Green mask.
    pub green_mask: u32,
    /// Blue mask.
    pub blue_mask: u32,
}

/// Parses a complete setup reply including the fixed 8-byte prefix.
pub fn parse_setup_reply(data: &[u8], order: ByteOrder) -> Result<Setup> {
    let mut rd = Reader::new(data, order);
    let status = rd.u8()?;
    let reason_len = usize::from(rd.u8()?);
    let protocol_major = rd.u16()?;
    let protocol_minor = rd.u16()?;
    let extra_units = usize::from(rd.u16()?);
    let extra_bytes = checked_mul(extra_units, 4)?;
    if rd.remaining() != extra_bytes {
        return Err(Error::damaged("X11 setup reply length mismatch"));
    }
    if status != 1 {
        let reason = rd.take(reason_len.min(rd.remaining()))?;
        return Err(Error::System(format!(
            "X11 setup rejected: {}",
            String::from_utf8_lossy(reason)
        )));
    }
    let release_number = rd.u32()?;
    let resource_id_base = rd.u32()?;
    let resource_id_mask = rd.u32()?;
    let motion_buffer_size = rd.u32()?;
    let vendor_len = usize::from(rd.u16()?);
    let maximum_request_length = rd.u16()?;
    let screen_count = usize::from(rd.u8()?);
    let format_count = usize::from(rd.u8()?);
    let _image_byte_order = rd.u8()?;
    let _bitmap_bit_order = rd.u8()?;
    let _bitmap_scanline_unit = rd.u8()?;
    let _bitmap_scanline_pad = rd.u8()?;
    let _min_keycode = rd.u8()?;
    let _max_keycode = rd.u8()?;
    rd.skip(4)?;
    let vendor = rd.take(vendor_len)?.to_vec();
    rd.skip(padding(vendor_len))?;

    if screen_count > MAX_SETUP_ITEMS || format_count > MAX_SETUP_ITEMS {
        return Err(Error::damaged("X11 setup item count exceeds limit"));
    }
    let mut pixmap_formats = Vec::with_capacity(format_count);
    for _ in 0..format_count {
        let depth = rd.u8()?;
        let bits_per_pixel = rd.u8()?;
        let scanline_pad = rd.u8()?;
        rd.skip(5)?;
        pixmap_formats.push(PixmapFormat {
            depth,
            bits_per_pixel,
            scanline_pad,
        });
    }
    let mut screens = Vec::with_capacity(screen_count);
    for _ in 0..screen_count {
        screens.push(parse_screen(&mut rd)?);
    }
    if rd.remaining() != 0 {
        return Err(Error::damaged("trailing bytes in X11 setup reply"));
    }
    Ok(Setup {
        protocol_major,
        protocol_minor,
        release_number,
        resource_id_base,
        resource_id_mask,
        motion_buffer_size,
        vendor,
        maximum_request_length,
        pixmap_formats,
        screens,
    })
}

fn parse_screen(rd: &mut Reader<'_>) -> Result<Screen> {
    let root = rd.u32()?;
    let default_colormap = rd.u32()?;
    let white_pixel = rd.u32()?;
    let black_pixel = rd.u32()?;
    let current_input_masks = rd.u32()?;
    let width_pixels = rd.u16()?;
    let height_pixels = rd.u16()?;
    let _width_mm = rd.u16()?;
    let _height_mm = rd.u16()?;
    let _min_installed_maps = rd.u16()?;
    let _max_installed_maps = rd.u16()?;
    let root_visual = rd.u32()?;
    let backing_stores = rd.u8()?;
    let save_unders = rd.u8()? != 0;
    let root_depth = rd.u8()?;
    let depth_count = usize::from(rd.u8()?);
    if depth_count > MAX_SETUP_ITEMS {
        return Err(Error::damaged("too many X11 depths"));
    }
    let mut depths = Vec::with_capacity(depth_count);
    for _ in 0..depth_count {
        let depth = rd.u8()?;
        rd.skip(1)?;
        let visual_count = usize::from(rd.u16()?);
        rd.skip(4)?;
        if visual_count > MAX_SETUP_ITEMS {
            return Err(Error::damaged("too many X11 visuals"));
        }
        let mut visuals = Vec::with_capacity(visual_count);
        for _ in 0..visual_count {
            visuals.push(Visual {
                id: rd.u32()?,
                class: rd.u8()?,
                bits_per_rgb: rd.u8()?,
                colormap_entries: rd.u16()?,
                red_mask: rd.u32()?,
                green_mask: rd.u32()?,
                blue_mask: rd.u32()?,
            });
            rd.skip(4)?;
        }
        depths.push(Depth { depth, visuals });
    }
    Ok(Screen {
        root,
        default_colormap,
        white_pixel,
        black_pixel,
        current_input_masks,
        width_pixels,
        height_pixels,
        root_visual,
        backing_stores,
        save_unders,
        root_depth,
        depths,
    })
}

/// X11 atom id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Atom(pub u32);

/// X11 window id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Window(pub u32);

/// X11 drawable id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drawable(pub u32);

/// Graphics-context id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gc(pub u32);

/// Shared-memory segment id used by MIT-SHM.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShmSeg(pub u32);

/// Rectangle used for expose/damage aggregation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    /// X coordinate.
    pub x: i16,
    /// Y coordinate.
    pub y: i16,
    /// Width.
    pub width: u16,
    /// Height.
    pub height: u16,
}

impl Rect {
    fn union(self, other: Self) -> Self {
        let ax0 = i32::from(self.x);
        let ay0 = i32::from(self.y);
        let ax1 = ax0.saturating_add(i32::from(self.width));
        let ay1 = ay0.saturating_add(i32::from(self.height));
        let bx0 = i32::from(other.x);
        let by0 = i32::from(other.y);
        let bx1 = bx0.saturating_add(i32::from(other.width));
        let by1 = by0.saturating_add(i32::from(other.height));
        let x0 = ax0.min(bx0);
        let y0 = ay0.min(by0);
        let x1 = ax1.max(bx1);
        let y1 = ay1.max(by1);
        Self {
            x: clamp_i16(x0),
            y: clamp_i16(y0),
            width: clamp_u16(x1.saturating_sub(x0)),
            height: clamp_u16(y1.saturating_sub(y0)),
        }
    }
}

/// Core event subset used by the window backend.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Event {
    /// One or more Expose packets merged into a damage rectangle.
    Expose {
        window: Window,
        damage: Rect,
        remaining: u16,
    },
    /// Window geometry changed.
    ConfigureNotify {
        window: Window,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    },
    /// Key press/release.
    Key {
        pressed: bool,
        detail: u8,
        state: u16,
        time: u32,
        event: Window,
        x: i16,
        y: i16,
    },
    /// Button press/release.
    Button {
        pressed: bool,
        detail: u8,
        state: u16,
        time: u32,
        event: Window,
        x: i16,
        y: i16,
    },
    /// Pointer motion.
    Motion {
        state: u16,
        time: u32,
        event: Window,
        x: i16,
        y: i16,
    },
    /// Focus change.
    Focus {
        focused: bool,
        window: Window,
        mode: u8,
        detail: u8,
    },
    /// ClientMessage.
    ClientMessage {
        window: Window,
        message_type: Atom,
        format: u8,
        data: [u8; 20],
    },
    /// SelectionRequest.
    SelectionRequest {
        time: u32,
        owner: Window,
        requestor: Window,
        selection: Atom,
        target: Atom,
        property: Atom,
    },
    /// SelectionNotify.
    SelectionNotify {
        time: u32,
        requestor: Window,
        selection: Atom,
        target: Atom,
        property: Atom,
    },
    /// SelectionClear.
    SelectionClear { time: u32, owner: Window, selection: Atom },
    /// PropertyNotify, needed for INCR transfers.
    PropertyNotify {
        window: Window,
        atom: Atom,
        time: u32,
        deleted: bool,
    },
    /// Unknown core/extension event retained as raw bytes.
    Unknown { response_type: u8, bytes: [u8; 32] },
}

/// Decoded X11 error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XError {
    /// Error code.
    pub code: u8,
    /// Human-readable core error name when known.
    pub name: &'static str,
    /// Sequence number.
    pub sequence: u16,
    /// Resource/value involved.
    pub bad_value: u32,
    /// Major opcode.
    pub major_opcode: u8,
    /// Minor opcode.
    pub minor_opcode: u16,
}

/// Names a core protocol error.
#[must_use]
pub fn error_name(code: u8) -> &'static str {
    match code {
        1 => "BadRequest",
        2 => "BadValue",
        3 => "BadWindow",
        4 => "BadPixmap",
        5 => "BadAtom",
        6 => "BadCursor",
        7 => "BadFont",
        8 => "BadMatch",
        9 => "BadDrawable",
        10 => "BadAccess",
        11 => "BadAlloc",
        12 => "BadColor",
        13 => "BadGC",
        14 => "BadIDChoice",
        15 => "BadName",
        16 => "BadLength",
        17 => "BadImplementation",
        _ => "UnknownError",
    }
}

/// Stateful request encoder and reply/event decoder.
pub struct Connection<T: Transport> {
    transport: T,
    order: ByteOrder,
    sequence: u16,
    max_request_units: u32,
    extension_opcodes: BTreeMap<Vec<u8>, u8>,
    expose_pending: BTreeMap<Window, Rect>,
    queued_events: VecDeque<Event>,
}

impl<T: Transport> Connection<T> {
    /// Creates a protocol connection wrapper after setup.
    pub fn new(transport: T, order: ByteOrder, setup: &Setup) -> Self {
        Self {
            transport,
            order,
            sequence: 0,
            max_request_units: u32::from(setup.maximum_request_length),
            extension_opcodes: BTreeMap::new(),
            expose_pending: BTreeMap::new(),
            queued_events: VecDeque::new(),
        }
    }

    /// Returns the transport.
    pub fn into_inner(self) -> T {
        self.transport
    }

    /// Current maximum request length in 4-byte units.
    #[must_use]
    pub fn maximum_request_length(&self) -> u32 {
        self.max_request_units
    }

    /// Registers a discovered extension opcode.
    pub fn set_extension_opcode(&mut self, name: &[u8], opcode: u8) {
        self.extension_opcodes.insert(name.to_vec(), opcode);
    }

    /// Sends a QueryExtension request.
    pub fn query_extension(&mut self, name: &[u8]) -> Result<u16> {
        let name_len = to_u16(name.len(), "extension name too long")?;
        let mut req = request_header(98, 0, checked_add(8, pad4(name.len()))?, self.order)?;
        push_u16(&mut req, self.order, name_len);
        push_u16(&mut req, self.order, 0);
        push_padded(&mut req, name)?;
        self.send_request(req)
    }

    /// Parses a QueryExtension reply and records the opcode when present.
    pub fn parse_query_extension_reply(&mut self, name: &[u8], packet: &[u8]) -> Result<Option<u8>> {
        Ok(self
            .parse_query_extension_reply_details(name, packet)?
            .map(|(opcode, _)| opcode))
    }

    /// Parses a QueryExtension reply and returns its major opcode and first event type.
    pub fn parse_query_extension_reply_details(&mut self, name: &[u8], packet: &[u8]) -> Result<Option<(u8, u8)>> {
        ensure_reply(packet)?;
        let present = *packet
            .get(8)
            .ok_or_else(|| Error::damaged("short QueryExtension reply"))?
            != 0;
        let opcode = *packet
            .get(9)
            .ok_or_else(|| Error::damaged("short QueryExtension reply"))?;
        let first_event = *packet
            .get(10)
            .ok_or_else(|| Error::damaged("short QueryExtension reply"))?;
        if present {
            self.set_extension_opcode(name, opcode);
            Ok(Some((opcode, first_event)))
        } else {
            Ok(None)
        }
    }

    /// Sends GetInputFocus as a reply barrier after an asynchronous extension request.
    pub fn get_input_focus(&mut self) -> Result<u16> {
        self.send_request(request_header(43, 0, 4, self.order)?)
    }

    /// Sends BIG-REQUESTS Enable after QueryExtension discovered its opcode.
    pub fn enable_big_requests(&mut self) -> Result<u16> {
        let opcode = self.extension_opcode(b"BIG-REQUESTS")?;
        self.send_request(request_header(opcode, 0, 4, self.order)?)
    }

    /// Parses BIG-REQUESTS Enable reply and updates the request limit.
    pub fn parse_big_requests_reply(&mut self, packet: &[u8]) -> Result<u32> {
        ensure_reply(packet)?;
        let mut rd = Reader::new(packet, self.order);
        rd.skip(8)?;
        let max = rd.u32()?;
        if max < u32::from(u16::MAX) {
            return Err(Error::damaged("BIG-REQUESTS maximum is implausibly small"));
        }
        self.max_request_units = max;
        Ok(max)
    }

    /// CreateWindow request.
    #[allow(clippy::too_many_arguments)]
    pub fn create_window(
        &mut self,
        depth: u8,
        wid: Window,
        parent: Window,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        border_width: u16,
        class: u16,
        visual: u32,
        values: &[(u32, u32)],
    ) -> Result<u16> {
        let mask = values.iter().fold(0_u32, |acc, (bit, _)| acc | *bit);
        let value_bytes = checked_mul(values.len(), 4)?;
        let mut req = request_header(1, depth, checked_add(32, value_bytes)?, self.order)?;
        push_u32(&mut req, self.order, wid.0);
        push_u32(&mut req, self.order, parent.0);
        push_i16(&mut req, self.order, x);
        push_i16(&mut req, self.order, y);
        push_u16(&mut req, self.order, width);
        push_u16(&mut req, self.order, height);
        push_u16(&mut req, self.order, border_width);
        push_u16(&mut req, self.order, class);
        push_u32(&mut req, self.order, visual);
        push_u32(&mut req, self.order, mask);
        for (_, value) in values {
            push_u32(&mut req, self.order, *value);
        }
        self.send_request(req)
    }

    /// ChangeProperty request for arbitrary byte data.
    pub fn change_property(
        &mut self,
        window: Window,
        property: Atom,
        property_type: Atom,
        format: u8,
        mode: u8,
        data: &[u8],
    ) -> Result<u16> {
        if !matches!(format, 8 | 16 | 32) {
            return Err(Error::damaged("property format must be 8, 16, or 32"));
        }
        if data.len() > MAX_PROPERTY_BYTES {
            return Err(Error::damaged("property is too large"));
        }
        let unit_bytes = usize::from(format / 8);
        if data.len().checked_rem(unit_bytes).unwrap_or_default() != 0 {
            return Err(Error::damaged("property data is not aligned to its format"));
        }
        let element_count = data
            .len()
            .checked_div(unit_bytes)
            .ok_or_else(|| Error::damaged("property unit size is zero"))?;
        let count = u32::try_from(element_count).map_err(|_| Error::damaged("property element count overflow"))?;
        let mut req = request_header(18, mode, checked_add(24, pad4(data.len()))?, self.order)?;
        push_u32(&mut req, self.order, window.0);
        push_u32(&mut req, self.order, property.0);
        push_u32(&mut req, self.order, property_type.0);
        req.push(format);
        req.extend_from_slice(&[0, 0, 0]);
        push_u32(&mut req, self.order, count);
        push_padded(&mut req, data)?;
        self.send_request(req)
    }

    /// Convenience for WM_NAME / _NET_WM_NAME UTF-8 byte properties.
    pub fn set_text_property(
        &mut self,
        window: Window,
        property: Atom,
        property_type: Atom,
        text: &str,
    ) -> Result<u16> {
        self.change_property(window, property, property_type, 8, 0, text.as_bytes())
    }

    /// Sets WM_PROTOCOLS to contain WM_DELETE_WINDOW.
    pub fn set_wm_delete_window(
        &mut self,
        window: Window,
        wm_protocols: Atom,
        wm_delete_window: Atom,
        atom_type: Atom,
    ) -> Result<u16> {
        let mut bytes = Vec::with_capacity(4);
        push_u32(&mut bytes, self.order, wm_delete_window.0);
        self.change_property(window, wm_protocols, atom_type, 32, 0, &bytes)
    }

    /// Sets _NET_WM_ICON from ARGB32 words.
    pub fn set_net_wm_icon(
        &mut self,
        window: Window,
        property: Atom,
        cardinal: Atom,
        width: u32,
        height: u32,
        argb: &[u32],
    ) -> Result<u16> {
        let expected = checked_mul(
            usize::try_from(width).map_err(|_| Error::damaged("icon width overflow"))?,
            usize::try_from(height).map_err(|_| Error::damaged("icon height overflow"))?,
        )?;
        if argb.len() != expected {
            return Err(Error::damaged("icon pixel count mismatch"));
        }
        let mut bytes = Vec::with_capacity(checked_mul(checked_add(expected, 2)?, 4)?);
        push_u32(&mut bytes, self.order, width);
        push_u32(&mut bytes, self.order, height);
        for pixel in argb {
            push_u32(&mut bytes, self.order, *pixel);
        }
        self.change_property(window, property, cardinal, 32, 0, &bytes)
    }

    /// MapWindow request.
    pub fn map_window(&mut self, window: Window) -> Result<u16> {
        let mut req = request_header(8, 0, 8, self.order)?;
        push_u32(&mut req, self.order, window.0);
        self.send_request(req)
    }

    /// ConfigureWindow using caller-provided mask/value pairs.
    pub fn configure_window(&mut self, window: Window, values: &[(u16, u32)]) -> Result<u16> {
        let mask = values.iter().fold(0_u16, |acc, (bit, _)| acc | *bit);
        let mut req = request_header(12, 0, checked_add(12, checked_mul(values.len(), 4)?)?, self.order)?;
        push_u32(&mut req, self.order, window.0);
        push_u16(&mut req, self.order, mask);
        push_u16(&mut req, self.order, 0);
        for (_, value) in values {
            push_u32(&mut req, self.order, *value);
        }
        self.send_request(req)
    }

    /// CreateGC request.
    pub fn create_gc(&mut self, gc: Gc, drawable: Drawable, values: &[(u32, u32)]) -> Result<u16> {
        let mask = values.iter().fold(0_u32, |acc, (bit, _)| acc | *bit);
        let mut req = request_header(55, 0, checked_add(16, checked_mul(values.len(), 4)?)?, self.order)?;
        push_u32(&mut req, self.order, gc.0);
        push_u32(&mut req, self.order, drawable.0);
        push_u32(&mut req, self.order, mask);
        for (_, value) in values {
            push_u32(&mut req, self.order, *value);
        }
        self.send_request(req)
    }

    /// Sends PutImage requests split at the negotiated maximum request size.
    #[allow(clippy::too_many_arguments)]
    pub fn put_image(
        &mut self,
        format: u8,
        drawable: Drawable,
        gc: Gc,
        width: u16,
        height: u16,
        dst_x: i16,
        dst_y: i16,
        left_pad: u8,
        depth: u8,
        bytes_per_row: usize,
        pixels: &[u8],
    ) -> Result<Vec<u16>> {
        let full_bytes = checked_mul(bytes_per_row, usize::from(height))?;
        if pixels.len() != full_bytes {
            return Err(Error::damaged("PutImage pixel size mismatch"));
        }
        let max_bytes = self.max_request_bytes()?;
        if max_bytes <= 24 {
            return Err(Error::damaged("maximum request length too small for PutImage"));
        }
        let payload_max = max_bytes.saturating_sub(24);
        let rows_per = payload_max
            .checked_div(bytes_per_row)
            .ok_or_else(|| Error::damaged("PutImage row width is zero"))?;
        if rows_per == 0 {
            return Err(Error::Refused(
                "one PutImage row exceeds maximum request length".to_owned(),
            ));
        }
        let mut seqs = Vec::new();
        let mut row = 0_usize;
        while row < usize::from(height) {
            let rows = rows_per.min(usize::from(height).saturating_sub(row));
            let payload = checked_mul(rows, bytes_per_row)?;
            let start = checked_mul(row, bytes_per_row)?;
            let end = checked_add(start, payload)?;
            let slice = pixels
                .get(start..end)
                .ok_or_else(|| Error::damaged("PutImage slice out of range"))?;
            let rows_u16 = to_u16(rows, "PutImage chunk height overflow")?;
            let y_offset = i32::try_from(row).map_err(|_| Error::damaged("PutImage row offset overflow"))?;
            let chunk_y = i32::from(dst_y)
                .checked_add(y_offset)
                .ok_or_else(|| Error::damaged("PutImage y overflow"))?;
            let mut req = request_header(72, format, checked_add(24, pad4(payload))?, self.order)?;
            push_u32(&mut req, self.order, drawable.0);
            push_u32(&mut req, self.order, gc.0);
            push_u16(&mut req, self.order, width);
            push_u16(&mut req, self.order, rows_u16);
            push_i16(&mut req, self.order, dst_x);
            push_i16(&mut req, self.order, clamp_i16(chunk_y));
            req.push(left_pad);
            req.push(depth);
            req.extend_from_slice(&[0, 0]);
            push_padded(&mut req, slice)?;
            seqs.push(self.send_request(req)?);
            row = checked_add(row, rows)?;
        }
        Ok(seqs)
    }

    /// MIT-SHM Attach request.
    pub fn shm_attach(&mut self, shmseg: ShmSeg, shmid: u32, read_only: bool) -> Result<u16> {
        let opcode = self.extension_opcode(b"MIT-SHM")?;
        let mut req = request_header(opcode, 1, 16, self.order)?;
        push_u32(&mut req, self.order, shmseg.0);
        push_u32(&mut req, self.order, shmid);
        req.push(u8::from(read_only));
        req.extend_from_slice(&[0, 0, 0]);
        self.send_request(req)
    }

    /// MIT-SHM Detach request.
    pub fn shm_detach(&mut self, shmseg: ShmSeg) -> Result<u16> {
        let opcode = self.extension_opcode(b"MIT-SHM")?;
        let mut req = request_header(opcode, 2, 8, self.order)?;
        push_u32(&mut req, self.order, shmseg.0);
        self.send_request(req)
    }

    /// MIT-SHM PutImage request.
    #[allow(clippy::too_many_arguments)]
    pub fn shm_put_image(
        &mut self,
        drawable: Drawable,
        gc: Gc,
        total_width: u16,
        total_height: u16,
        src_x: u16,
        src_y: u16,
        src_width: u16,
        src_height: u16,
        dst_x: i16,
        dst_y: i16,
        depth: u8,
        format: u8,
        send_event: bool,
        shmseg: ShmSeg,
        offset: u32,
    ) -> Result<u16> {
        let opcode = self.extension_opcode(b"MIT-SHM")?;
        let mut req = request_header(opcode, 3, 40, self.order)?;
        push_u32(&mut req, self.order, drawable.0);
        push_u32(&mut req, self.order, gc.0);
        for value in [total_width, total_height, src_x, src_y, src_width, src_height] {
            push_u16(&mut req, self.order, value);
        }
        push_i16(&mut req, self.order, dst_x);
        push_i16(&mut req, self.order, dst_y);
        req.push(depth);
        req.push(format);
        req.push(u8::from(send_event));
        req.push(0);
        push_u32(&mut req, self.order, shmseg.0);
        push_u32(&mut req, self.order, offset);
        self.send_request(req)
    }

    /// InternAtom request.
    pub fn intern_atom(&mut self, only_if_exists: bool, name: &[u8]) -> Result<u16> {
        let len = to_u16(name.len(), "atom name too long")?;
        let mut req = request_header(
            16,
            u8::from(only_if_exists),
            checked_add(8, pad4(name.len()))?,
            self.order,
        )?;
        push_u16(&mut req, self.order, len);
        push_u16(&mut req, self.order, 0);
        push_padded(&mut req, name)?;
        self.send_request(req)
    }

    /// Parses an InternAtom reply.
    pub fn parse_intern_atom_reply(&self, packet: &[u8]) -> Result<Atom> {
        ensure_reply(packet)?;
        let mut rd = Reader::new(packet, self.order);
        rd.skip(8)?;
        Ok(Atom(rd.u32()?))
    }

    /// GetKeyboardMapping request.
    pub fn get_keyboard_mapping(&mut self, first_keycode: u8, count: u8) -> Result<u16> {
        let mut req = request_header(101, 0, 8, self.order)?;
        req.push(first_keycode);
        req.push(count);
        req.extend_from_slice(&[0, 0]);
        self.send_request(req)
    }

    /// Parses GetKeyboardMapping reply into rows of keysyms.
    pub fn parse_keyboard_mapping_reply(&self, packet: &[u8], keycode_count: usize) -> Result<Vec<Vec<u32>>> {
        ensure_reply(packet)?;
        let per = usize::from(
            *packet
                .get(1)
                .ok_or_else(|| Error::damaged("short keyboard mapping reply"))?,
        );
        let extra_units = read_u32_at(packet, 4, self.order)?;
        let extra = usize::try_from(extra_units)
            .map_err(|_| Error::damaged("keyboard reply length overflow"))?
            .checked_mul(4)
            .ok_or_else(|| Error::damaged("keyboard reply length overflow"))?;
        if packet.len() != checked_add(32, extra)? {
            return Err(Error::damaged("keyboard mapping reply length mismatch"));
        }
        let expected = checked_mul(checked_mul(keycode_count, per)?, 4)?;
        if extra != expected {
            return Err(Error::damaged("keyboard mapping keysym count mismatch"));
        }
        let mut rd = Reader::new(
            packet.get(32..).ok_or_else(|| Error::damaged("short keyboard reply"))?,
            self.order,
        );
        let mut rows = Vec::with_capacity(keycode_count);
        for _ in 0..keycode_count {
            let mut row = Vec::with_capacity(per);
            for _ in 0..per {
                row.push(rd.u32()?);
            }
            rows.push(row);
        }
        Ok(rows)
    }

    /// Sends XKB GetMap for the core keyboard. The extension must have been queried first.
    #[allow(clippy::too_many_arguments)]
    pub fn xkb_get_map(
        &mut self,
        device_spec: u16,
        full: u16,
        partial: u16,
        first_type: u8,
        n_types: u8,
        first_key_sym: u8,
        n_key_syms: u8,
        first_key_action: u8,
        n_key_actions: u8,
    ) -> Result<u16> {
        let opcode = self.extension_opcode(b"XKEYBOARD")?;
        let mut req = request_header(opcode, 8, 28, self.order)?;
        push_u16(&mut req, self.order, device_spec);
        push_u16(&mut req, self.order, full);
        push_u16(&mut req, self.order, partial);
        req.extend_from_slice(&[
            first_type,
            n_types,
            first_key_sym,
            n_key_syms,
            first_key_action,
            n_key_actions,
        ]);
        req.extend_from_slice(&[0; 10]);
        self.send_request(req)
    }

    /// SetSelectionOwner request.
    pub fn set_selection_owner(&mut self, owner: Window, selection: Atom, time: u32) -> Result<u16> {
        let mut req = request_header(22, 0, 16, self.order)?;
        push_u32(&mut req, self.order, owner.0);
        push_u32(&mut req, self.order, selection.0);
        push_u32(&mut req, self.order, time);
        self.send_request(req)
    }

    /// ConvertSelection request.
    pub fn convert_selection(
        &mut self,
        requestor: Window,
        selection: Atom,
        target: Atom,
        property: Atom,
        time: u32,
    ) -> Result<u16> {
        let mut req = request_header(24, 0, 24, self.order)?;
        for value in [requestor.0, selection.0, target.0, property.0, time] {
            push_u32(&mut req, self.order, value);
        }
        self.send_request(req)
    }

    /// SendEvent carrying SelectionNotify.
    pub fn send_selection_notify(
        &mut self,
        requestor: Window,
        selection: Atom,
        target: Atom,
        property: Atom,
        time: u32,
    ) -> Result<u16> {
        let mut event = [0_u8; 32];
        set_u8(&mut event, 0, 31)?;
        write_u32_at(&mut event, 4, self.order, time)?;
        write_u32_at(&mut event, 8, self.order, requestor.0)?;
        write_u32_at(&mut event, 12, self.order, selection.0)?;
        write_u32_at(&mut event, 16, self.order, target.0)?;
        write_u32_at(&mut event, 20, self.order, property.0)?;
        let mut req = request_header(25, 0, 44, self.order)?;
        push_u32(&mut req, self.order, requestor.0);
        push_u32(&mut req, self.order, 0);
        req.extend_from_slice(&event);
        self.send_request(req)
    }

    /// Deletes a property, used by INCR handshakes.
    pub fn delete_property(&mut self, window: Window, property: Atom) -> Result<u16> {
        let mut req = request_header(19, 0, 12, self.order)?;
        push_u32(&mut req, self.order, window.0);
        push_u32(&mut req, self.order, property.0);
        self.send_request(req)
    }

    /// GetProperty request.
    pub fn get_property(
        &mut self,
        delete: bool,
        window: Window,
        property: Atom,
        property_type: Atom,
        offset_words: u32,
        length_words: u32,
    ) -> Result<u16> {
        let mut req = request_header(20, u8::from(delete), 24, self.order)?;
        push_u32(&mut req, self.order, window.0);
        push_u32(&mut req, self.order, property.0);
        push_u32(&mut req, self.order, property_type.0);
        push_u32(&mut req, self.order, offset_words);
        push_u32(&mut req, self.order, length_words);
        self.send_request(req)
    }

    /// Creates a core glyph cursor using CreateGlyphCursor.
    #[allow(clippy::too_many_arguments)]
    pub fn create_glyph_cursor(
        &mut self,
        cid: u32,
        source_font: u32,
        mask_font: u32,
        source_char: u16,
        mask_char: u16,
        foreground: (u16, u16, u16),
        background: (u16, u16, u16),
    ) -> Result<u16> {
        let mut req = request_header(94, 0, 32, self.order)?;
        for value in [cid, source_font, mask_font] {
            push_u32(&mut req, self.order, value);
        }
        push_u16(&mut req, self.order, source_char);
        push_u16(&mut req, self.order, mask_char);
        for value in [
            foreground.0,
            foreground.1,
            foreground.2,
            background.0,
            background.1,
            background.2,
        ] {
            push_u16(&mut req, self.order, value);
        }
        self.send_request(req)
    }

    /// Receives and decodes the next 32-byte core event/error packet.
    pub fn receive_packet(&mut self) -> Result<Packet> {
        let mut packet = [0_u8; CORE_PACKET];
        self.transport.receive(&mut packet);
        self.decode_packet(&packet)
    }

    /// Decodes an already received 32-byte core event/error packet.
    pub fn decode_packet(&mut self, packet: &[u8; 32]) -> Result<Packet> {
        let response = packet
            .first()
            .copied()
            .ok_or_else(|| Error::damaged("empty X11 packet"))?;
        if response == 0 {
            return Ok(Packet::Error(parse_error(packet, self.order)?));
        }
        if response == 1 {
            return Ok(Packet::Reply(packet.to_vec()));
        }
        let event = parse_event(packet, self.order)?;
        if let Event::Expose {
            window,
            damage,
            remaining,
        } = event
        {
            let merged = self
                .expose_pending
                .get(&window)
                .copied()
                .map_or(damage, |old| old.union(damage));
            if remaining == 0 {
                self.expose_pending.remove(&window);
                return Ok(Packet::Event(Event::Expose {
                    window,
                    damage: merged,
                    remaining: 0,
                }));
            }
            self.expose_pending.insert(window, merged);
            return Ok(Packet::Deferred);
        }
        Ok(Packet::Event(event))
    }

    /// Queues an event for caller-side dispatch.
    pub fn queue_event(&mut self, event: Event) {
        self.queued_events.push_back(event);
    }

    /// Pops a queued event.
    pub fn pop_queued_event(&mut self) -> Option<Event> {
        self.queued_events.pop_front()
    }

    fn extension_opcode(&self, name: &[u8]) -> Result<u8> {
        self.extension_opcodes
            .get(name)
            .copied()
            .ok_or_else(|| Error::Refused(format!("X11 extension {} not available", String::from_utf8_lossy(name))))
    }

    fn max_request_bytes(&self) -> Result<usize> {
        let units =
            usize::try_from(self.max_request_units).map_err(|_| Error::damaged("maximum request length overflow"))?;
        checked_mul(units, 4)
    }

    fn send_request(&mut self, req: Vec<u8>) -> Result<u16> {
        if req.len().checked_rem(4).unwrap_or_default() != 0 {
            return Err(Error::damaged("X11 request is not 4-byte aligned"));
        }
        if req.len() > self.max_request_bytes()? {
            return Err(Error::Refused("X11 request exceeds negotiated maximum".to_owned()));
        }
        self.sequence = self.sequence.wrapping_add(1);
        self.transport.send(&req);
        Ok(self.sequence)
    }
}

/// Result of decoding one fixed core packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Packet {
    /// Error packet.
    Error(XError),
    /// A 32-byte reply prefix. Variable reply payload is read separately by the caller.
    Reply(Vec<u8>),
    /// Decoded event.
    Event(Event),
    /// Expose packet held until the final expose in the series arrives.
    Deferred,
}

/// Clipboard send-side transfer, including INCR chunking.
#[derive(Clone, Debug)]
pub struct ClipboardOffer {
    bytes: Vec<u8>,
    cursor: usize,
    chunk_size: usize,
}

impl ClipboardOffer {
    /// Starts a UTF-8 clipboard offer.
    pub fn new(text: &str, max_request_bytes: usize) -> Result<Self> {
        let overhead = 64_usize;
        let chunk_size = max_request_bytes.saturating_sub(overhead).clamp(4096, INCR_THRESHOLD);
        Ok(Self {
            bytes: text.as_bytes().to_vec(),
            cursor: 0,
            chunk_size,
        })
    }

    /// Whether X11 INCR should be used.
    #[must_use]
    pub fn needs_incr(&self) -> bool {
        self.bytes.len() > INCR_THRESHOLD
    }

    /// Total byte length.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the payload is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Returns the next INCR chunk. After the data, one final empty chunk is returned exactly once.
    pub fn next_chunk(&mut self) -> Option<&[u8]> {
        if self.cursor > self.bytes.len() {
            return None;
        }
        if self.cursor == self.bytes.len() {
            self.cursor = self.cursor.saturating_add(1);
            return Some(&[]);
        }
        let end = self.cursor.saturating_add(self.chunk_size).min(self.bytes.len());
        let out = self.bytes.get(self.cursor..end)?;
        self.cursor = end;
        Some(out)
    }
}

/// Clipboard receive-side INCR accumulator.
#[derive(Clone, Debug, Default)]
pub struct ClipboardPaste {
    bytes: Vec<u8>,
    expected: Option<usize>,
    complete: bool,
}

impl ClipboardPaste {
    /// Starts a direct non-INCR paste.
    pub fn direct(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_PROPERTY_BYTES {
            return Err(Error::damaged("clipboard payload exceeds limit"));
        }
        Ok(Self {
            bytes: bytes.to_vec(),
            expected: Some(bytes.len()),
            complete: true,
        })
    }

    /// Starts an INCR paste with the announced byte count.
    pub fn incr(expected: usize) -> Result<Self> {
        if expected > MAX_PROPERTY_BYTES {
            return Err(Error::damaged("clipboard INCR size exceeds limit"));
        }
        Ok(Self {
            bytes: Vec::with_capacity(expected.min(INCR_THRESHOLD)),
            expected: Some(expected),
            complete: false,
        })
    }

    /// Appends one INCR property chunk. Empty data terminates the transfer.
    pub fn push_chunk(&mut self, bytes: &[u8]) -> Result<()> {
        if self.complete {
            return Err(Error::damaged("clipboard transfer already complete"));
        }
        if bytes.is_empty() {
            if let Some(expected) = self.expected {
                if self.bytes.len() != expected {
                    return Err(Error::damaged("clipboard INCR byte count mismatch"));
                }
            }
            self.complete = true;
            return Ok(());
        }
        let new_len = checked_add(self.bytes.len(), bytes.len())?;
        if new_len > MAX_PROPERTY_BYTES {
            return Err(Error::damaged("clipboard payload exceeds limit"));
        }
        if let Some(expected) = self.expected {
            if new_len > expected {
                return Err(Error::damaged("clipboard INCR exceeds announced size"));
            }
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    /// Returns completed UTF-8 text.
    pub fn text(&self) -> Result<Option<String>> {
        if !self.complete {
            return Ok(None);
        }
        String::from_utf8(self.bytes.clone())
            .map(Some)
            .map_err(|_| Error::damaged("clipboard is not valid UTF-8"))
    }
}

/// Maps a keysym to a Unicode scalar where the mapping is character-like.
///
/// Latin-1 follows the X11 identity mapping; Cyrillic/Greek legacy keysyms use the generated
/// tables below; Unicode keysyms (`0x01000000 | scalar`) are accepted directly.
#[must_use]
pub fn keysym_to_char(keysym: u32) -> Option<char> {
    if (0x20..=0x7e).contains(&keysym) || (0xa0..=0xff).contains(&keysym) {
        return char::from_u32(keysym);
    }
    if keysym & 0xff00_0000 == 0x0100_0000 {
        return char::from_u32(keysym & 0x00ff_ffff);
    }
    KEYSYM_TABLE
        .binary_search_by_key(&keysym, |pair| pair.0)
        .ok()
        .and_then(|index| KEYSYM_TABLE.get(index).map(|pair| pair.1))
}

// Generated from the X11 keysym definitions used by xkbcommon/X.Org. Kept sorted by keysym.
static KEYSYM_TABLE: &[(u32, char)] = &[
    (0x06a1, 'ђ'),
    (0x06a2, 'ѓ'),
    (0x06a3, 'ё'),
    (0x06a4, 'є'),
    (0x06a5, 'ѕ'),
    (0x06a6, 'і'),
    (0x06a7, 'ї'),
    (0x06a8, 'ј'),
    (0x06a9, 'љ'),
    (0x06aa, 'њ'),
    (0x06ab, 'ћ'),
    (0x06ac, 'ќ'),
    (0x06ae, 'ў'),
    (0x06af, 'џ'),
    (0x06b0, '№'),
    (0x06b1, 'Ђ'),
    (0x06b2, 'Ѓ'),
    (0x06b3, 'Ё'),
    (0x06b4, 'Є'),
    (0x06b5, 'Ѕ'),
    (0x06b6, 'І'),
    (0x06b7, 'Ї'),
    (0x06b8, 'Ј'),
    (0x06b9, 'Љ'),
    (0x06ba, 'Њ'),
    (0x06bb, 'Ћ'),
    (0x06bc, 'Ќ'),
    (0x06be, 'Ў'),
    (0x06bf, 'Џ'),
    (0x06c0, 'ю'),
    (0x06c1, 'а'),
    (0x06c2, 'б'),
    (0x06c3, 'ц'),
    (0x06c4, 'д'),
    (0x06c5, 'е'),
    (0x06c6, 'ф'),
    (0x06c7, 'г'),
    (0x06c8, 'х'),
    (0x06c9, 'и'),
    (0x06ca, 'й'),
    (0x06cb, 'к'),
    (0x06cc, 'л'),
    (0x06cd, 'м'),
    (0x06ce, 'н'),
    (0x06cf, 'о'),
    (0x06d0, 'п'),
    (0x06d1, 'я'),
    (0x06d2, 'р'),
    (0x06d3, 'с'),
    (0x06d4, 'т'),
    (0x06d5, 'у'),
    (0x06d6, 'ж'),
    (0x06d7, 'в'),
    (0x06d8, 'ь'),
    (0x06d9, 'ы'),
    (0x06da, 'з'),
    (0x06db, 'ш'),
    (0x06dc, 'э'),
    (0x06dd, 'щ'),
    (0x06de, 'ч'),
    (0x06df, 'ъ'),
    (0x06e0, 'Ю'),
    (0x06e1, 'А'),
    (0x06e2, 'Б'),
    (0x06e3, 'Ц'),
    (0x06e4, 'Д'),
    (0x06e5, 'Е'),
    (0x06e6, 'Ф'),
    (0x06e7, 'Г'),
    (0x06e8, 'Х'),
    (0x06e9, 'И'),
    (0x06ea, 'Й'),
    (0x06eb, 'К'),
    (0x06ec, 'Л'),
    (0x06ed, 'М'),
    (0x06ee, 'Н'),
    (0x06ef, 'О'),
    (0x06f0, 'П'),
    (0x06f1, 'Я'),
    (0x06f2, 'Р'),
    (0x06f3, 'С'),
    (0x06f4, 'Т'),
    (0x06f5, 'У'),
    (0x06f6, 'Ж'),
    (0x06f7, 'В'),
    (0x06f8, 'Ь'),
    (0x06f9, 'Ы'),
    (0x06fa, 'З'),
    (0x06fb, 'Ш'),
    (0x06fc, 'Э'),
    (0x06fd, 'Щ'),
    (0x06fe, 'Ч'),
    (0x06ff, 'Ъ'),
    (0x07a1, 'Ά'),
    (0x07a2, 'Έ'),
    (0x07a3, 'Ή'),
    (0x07a4, 'Ί'),
    (0x07a5, 'Ϊ'),
    (0x07a7, 'Ό'),
    (0x07a8, 'Ύ'),
    (0x07a9, 'Ϋ'),
    (0x07ab, 'Ώ'),
    (0x07ae, '΅'),
    (0x07af, '―'),
    (0x07b1, 'ά'),
    (0x07b2, 'έ'),
    (0x07b3, 'ή'),
    (0x07b4, 'ί'),
    (0x07b5, 'ϊ'),
    (0x07b6, 'ΐ'),
    (0x07b7, 'ό'),
    (0x07b8, 'ύ'),
    (0x07b9, 'ϋ'),
    (0x07ba, 'ΰ'),
    (0x07bb, 'ώ'),
    (0x07c1, 'Α'),
    (0x07c2, 'Β'),
    (0x07c3, 'Γ'),
    (0x07c4, 'Δ'),
    (0x07c5, 'Ε'),
    (0x07c6, 'Ζ'),
    (0x07c7, 'Η'),
    (0x07c8, 'Θ'),
    (0x07c9, 'Ι'),
    (0x07ca, 'Κ'),
    (0x07cb, 'Λ'),
    (0x07cc, 'Μ'),
    (0x07cd, 'Ν'),
    (0x07ce, 'Ξ'),
    (0x07cf, 'Ο'),
    (0x07d0, 'Π'),
    (0x07d1, 'Ρ'),
    (0x07d2, 'Σ'),
    (0x07d4, 'Τ'),
    (0x07d5, 'Υ'),
    (0x07d6, 'Φ'),
    (0x07d7, 'Χ'),
    (0x07d8, 'Ψ'),
    (0x07d9, 'Ω'),
    (0x07e1, 'α'),
    (0x07e2, 'β'),
    (0x07e3, 'γ'),
    (0x07e4, 'δ'),
    (0x07e5, 'ε'),
    (0x07e6, 'ζ'),
    (0x07e7, 'η'),
    (0x07e8, 'θ'),
    (0x07e9, 'ι'),
    (0x07ea, 'κ'),
    (0x07eb, 'λ'),
    (0x07ec, 'μ'),
    (0x07ed, 'ν'),
    (0x07ee, 'ξ'),
    (0x07ef, 'ο'),
    (0x07f0, 'π'),
    (0x07f1, 'ρ'),
    (0x07f2, 'σ'),
    (0x07f3, 'ς'),
    (0x07f4, 'τ'),
    (0x07f5, 'υ'),
    (0x07f6, 'φ'),
    (0x07f7, 'χ'),
    (0x07f8, 'ψ'),
    (0x07f9, 'ω'),
    (0xffaa, '*'),
    (0xffab, '+'),
    (0xffac, ','),
    (0xffad, '-'),
    (0xffae, '.'),
    (0xffaf, '/'),
    (0xffb0, '0'),
    (0xffb1, '1'),
    (0xffb2, '2'),
    (0xffb3, '3'),
    (0xffb4, '4'),
    (0xffb5, '5'),
    (0xffb6, '6'),
    (0xffb7, '7'),
    (0xffb8, '8'),
    (0xffb9, '9'),
    (0xffbd, '='),
];

fn parse_event(packet: &[u8; 32], order: ByteOrder) -> Result<Event> {
    let response = packet.first().copied().ok_or_else(|| Error::damaged("empty event"))? & 0x7f;
    match response {
        2 | 3 => Ok(Event::Key {
            pressed: response == 2,
            detail: *packet.get(1).ok_or_else(|| Error::damaged("short key event"))?,
            time: read_u32_at(packet, 4, order)?,
            event: Window(read_u32_at(packet, 12, order)?),
            x: read_i16_at(packet, 24, order)?,
            y: read_i16_at(packet, 26, order)?,
            state: read_u16_at(packet, 28, order)?,
        }),
        4 | 5 => Ok(Event::Button {
            pressed: response == 4,
            detail: *packet.get(1).ok_or_else(|| Error::damaged("short button event"))?,
            time: read_u32_at(packet, 4, order)?,
            event: Window(read_u32_at(packet, 12, order)?),
            x: read_i16_at(packet, 24, order)?,
            y: read_i16_at(packet, 26, order)?,
            state: read_u16_at(packet, 28, order)?,
        }),
        6 => Ok(Event::Motion {
            time: read_u32_at(packet, 4, order)?,
            event: Window(read_u32_at(packet, 12, order)?),
            x: read_i16_at(packet, 24, order)?,
            y: read_i16_at(packet, 26, order)?,
            state: read_u16_at(packet, 28, order)?,
        }),
        9 | 10 => Ok(Event::Focus {
            focused: response == 9,
            detail: *packet.get(1).ok_or_else(|| Error::damaged("short focus event"))?,
            window: Window(read_u32_at(packet, 4, order)?),
            mode: *packet.get(8).ok_or_else(|| Error::damaged("short focus event"))?,
        }),
        12 => Ok(Event::Expose {
            window: Window(read_u32_at(packet, 4, order)?),
            damage: Rect {
                x: read_i16_at(packet, 8, order)?,
                y: read_i16_at(packet, 10, order)?,
                width: read_u16_at(packet, 12, order)?,
                height: read_u16_at(packet, 14, order)?,
            },
            remaining: read_u16_at(packet, 16, order)?,
        }),
        22 => Ok(Event::ConfigureNotify {
            window: Window(read_u32_at(packet, 8, order)?),
            x: read_i16_at(packet, 16, order)?,
            y: read_i16_at(packet, 18, order)?,
            width: read_u16_at(packet, 20, order)?,
            height: read_u16_at(packet, 22, order)?,
        }),
        28 => Ok(Event::PropertyNotify {
            window: Window(read_u32_at(packet, 4, order)?),
            atom: Atom(read_u32_at(packet, 8, order)?),
            time: read_u32_at(packet, 12, order)?,
            deleted: *packet.get(16).ok_or_else(|| Error::damaged("short PropertyNotify"))? == 1,
        }),
        29 => Ok(Event::SelectionClear {
            time: read_u32_at(packet, 4, order)?,
            owner: Window(read_u32_at(packet, 8, order)?),
            selection: Atom(read_u32_at(packet, 12, order)?),
        }),
        30 => Ok(Event::SelectionRequest {
            time: read_u32_at(packet, 4, order)?,
            owner: Window(read_u32_at(packet, 8, order)?),
            requestor: Window(read_u32_at(packet, 12, order)?),
            selection: Atom(read_u32_at(packet, 16, order)?),
            target: Atom(read_u32_at(packet, 20, order)?),
            property: Atom(read_u32_at(packet, 24, order)?),
        }),
        31 => Ok(Event::SelectionNotify {
            time: read_u32_at(packet, 4, order)?,
            requestor: Window(read_u32_at(packet, 8, order)?),
            selection: Atom(read_u32_at(packet, 12, order)?),
            target: Atom(read_u32_at(packet, 16, order)?),
            property: Atom(read_u32_at(packet, 20, order)?),
        }),
        33 => {
            let format = *packet.get(1).ok_or_else(|| Error::damaged("short ClientMessage"))?;
            let mut data = [0_u8; 20];
            let src = packet
                .get(12..32)
                .ok_or_else(|| Error::damaged("short ClientMessage"))?;
            data.copy_from_slice(src);
            Ok(Event::ClientMessage {
                window: Window(read_u32_at(packet, 4, order)?),
                message_type: Atom(read_u32_at(packet, 8, order)?),
                format,
                data,
            })
        }
        _ => Ok(Event::Unknown {
            response_type: response,
            bytes: *packet,
        }),
    }
}

fn parse_error(packet: &[u8; 32], order: ByteOrder) -> Result<XError> {
    let code = *packet.get(1).ok_or_else(|| Error::damaged("short X11 error"))?;
    Ok(XError {
        code,
        name: error_name(code),
        sequence: read_u16_at(packet, 2, order)?,
        bad_value: read_u32_at(packet, 4, order)?,
        minor_opcode: read_u16_at(packet, 8, order)?,
        major_opcode: *packet.get(10).ok_or_else(|| Error::damaged("short X11 error"))?,
    })
}

fn ensure_reply(packet: &[u8]) -> Result<()> {
    if packet.len() < CORE_PACKET || packet.first().copied() != Some(1) {
        return Err(Error::damaged("expected 32-byte X11 reply"));
    }
    Ok(())
}

fn request_header(opcode: u8, data: u8, byte_len: usize, order: ByteOrder) -> Result<Vec<u8>> {
    if byte_len < 4 || byte_len.checked_rem(4).unwrap_or_default() != 0 {
        return Err(Error::damaged("invalid X11 request length"));
    }
    let units = byte_len / 4;
    if let Ok(units_u16) = u16::try_from(units) {
        let mut out = Vec::with_capacity(byte_len);
        out.push(opcode);
        out.push(data);
        push_u16(&mut out, order, units_u16);
        return Ok(out);
    }

    // BIG-REQUESTS inserts a 32-bit request length immediately after a zero 16-bit length.
    // The extended length includes those additional four bytes. Before BIG-REQUESTS is
    // enabled, `Connection::send_request` still rejects this against the setup limit.
    let extended_bytes = checked_add(byte_len, 4)?;
    let extended_units = extended_bytes / 4;
    let extended_units =
        u32::try_from(extended_units).map_err(|_| Error::damaged("BIG-REQUESTS length exceeds u32 units"))?;
    let mut out = Vec::with_capacity(extended_bytes);
    out.push(opcode);
    out.push(data);
    push_u16(&mut out, order, 0);
    push_u32(&mut out, order, extended_units);
    Ok(out)
}

fn push_padded(out: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    out.extend_from_slice(bytes);
    let pad = padding(bytes.len());
    let new_len = checked_add(out.len(), pad)?;
    out.resize(new_len, 0);
    Ok(())
}

fn padding(len: usize) -> usize {
    (4_usize.wrapping_sub(len & 3)) & 3
}
fn pad4(len: usize) -> usize {
    len.saturating_add(padding(len))
}

fn checked_add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or_else(|| Error::damaged("size overflow"))
}
fn checked_mul(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b).ok_or_else(|| Error::damaged("size overflow"))
}
fn to_u16(value: usize, what: &str) -> Result<u16> {
    u16::try_from(value).map_err(|_| Error::damaged(what))
}
fn clamp_i16(value: i32) -> i16 {
    i16::try_from(value).unwrap_or(if value < 0 { i16::MIN } else { i16::MAX })
}
fn clamp_u16(value: i32) -> u16 {
    u16::try_from(value.max(0)).unwrap_or(u16::MAX)
}

fn push_u16(out: &mut Vec<u8>, order: ByteOrder, value: u16) {
    let bytes = match order {
        ByteOrder::Little => value.to_le_bytes(),
        ByteOrder::Big => value.to_be_bytes(),
    };
    out.extend_from_slice(&bytes);
}
fn push_i16(out: &mut Vec<u8>, order: ByteOrder, value: i16) {
    let bytes = match order {
        ByteOrder::Little => value.to_le_bytes(),
        ByteOrder::Big => value.to_be_bytes(),
    };
    out.extend_from_slice(&bytes);
}
fn push_u32(out: &mut Vec<u8>, order: ByteOrder, value: u32) {
    let bytes = match order {
        ByteOrder::Little => value.to_le_bytes(),
        ByteOrder::Big => value.to_be_bytes(),
    };
    out.extend_from_slice(&bytes);
}

fn read_u16_at(data: &[u8], offset: usize, order: ByteOrder) -> Result<u16> {
    let end = checked_add(offset, 2)?;
    let bytes: [u8; 2] = data
        .get(offset..end)
        .ok_or_else(|| Error::damaged("short X11 field"))?
        .try_into()
        .map_err(|_| Error::damaged("short X11 field"))?;
    Ok(match order {
        ByteOrder::Little => u16::from_le_bytes(bytes),
        ByteOrder::Big => u16::from_be_bytes(bytes),
    })
}
fn read_i16_at(data: &[u8], offset: usize, order: ByteOrder) -> Result<i16> {
    let end = checked_add(offset, 2)?;
    let bytes: [u8; 2] = data
        .get(offset..end)
        .ok_or_else(|| Error::damaged("short X11 field"))?
        .try_into()
        .map_err(|_| Error::damaged("short X11 field"))?;
    Ok(match order {
        ByteOrder::Little => i16::from_le_bytes(bytes),
        ByteOrder::Big => i16::from_be_bytes(bytes),
    })
}
fn read_u32_at(data: &[u8], offset: usize, order: ByteOrder) -> Result<u32> {
    let end = checked_add(offset, 4)?;
    let bytes: [u8; 4] = data
        .get(offset..end)
        .ok_or_else(|| Error::damaged("short X11 field"))?
        .try_into()
        .map_err(|_| Error::damaged("short X11 field"))?;
    Ok(match order {
        ByteOrder::Little => u32::from_le_bytes(bytes),
        ByteOrder::Big => u32::from_be_bytes(bytes),
    })
}
fn write_u32_at(data: &mut [u8], offset: usize, order: ByteOrder, value: u32) -> Result<()> {
    let end = checked_add(offset, 4)?;
    let dst = data
        .get_mut(offset..end)
        .ok_or_else(|| Error::damaged("short X11 field"))?;
    let bytes = match order {
        ByteOrder::Little => value.to_le_bytes(),
        ByteOrder::Big => value.to_be_bytes(),
    };
    dst.copy_from_slice(&bytes);
    Ok(())
}
fn set_u8(data: &mut [u8], offset: usize, value: u8) -> Result<()> {
    let slot = data.get_mut(offset).ok_or_else(|| Error::damaged("short X11 field"))?;
    *slot = value;
    Ok(())
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    order: ByteOrder,
}
impl<'a> Reader<'a> {
    fn new(data: &'a [u8], order: ByteOrder) -> Self {
        Self { data, pos: 0, order }
    }
    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }
    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = checked_add(self.pos, len)?;
        let out = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| Error::damaged("truncated X11 packet"))?;
        self.pos = end;
        Ok(out)
    }
    fn skip(&mut self, len: usize) -> Result<()> {
        self.take(len).map(|_| ())
    }
    fn u8(&mut self) -> Result<u8> {
        self.take(1)?
            .first()
            .copied()
            .ok_or_else(|| Error::damaged("truncated X11 byte"))
    }
    fn u16(&mut self) -> Result<u16> {
        let bytes: [u8; 2] = self.take(2)?.try_into().map_err(|_| Error::damaged("short u16"))?;
        Ok(match self.order {
            ByteOrder::Little => u16::from_le_bytes(bytes),
            ByteOrder::Big => u16::from_be_bytes(bytes),
        })
    }
    fn u32(&mut self) -> Result<u32> {
        let bytes: [u8; 4] = self.take(4)?.try_into().map_err(|_| Error::damaged("short u32"))?;
        Ok(match self.order {
            ByteOrder::Little => u32::from_le_bytes(bytes),
            ByteOrder::Big => u32::from_be_bytes(bytes),
        })
    }
    fn counted_be(&mut self, max: usize) -> Result<Vec<u8>> {
        let bytes: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_| Error::damaged("short Xauthority length"))?;
        let len = usize::from(u16::from_be_bytes(bytes));
        if len > max {
            return Err(Error::damaged("Xauthority field exceeds limit"));
        }
        Ok(self.take(len)?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeTransport {
        sent: Vec<Vec<u8>>,
        incoming: VecDeque<u8>,
    }
    impl Transport for FakeTransport {
        fn send(&mut self, data: &[u8]) {
            self.sent.push(data.to_vec());
        }
        fn receive(&mut self, out: &mut [u8]) {
            for slot in out.iter_mut() {
                *slot = self.incoming.pop_front().unwrap_or_default();
            }
        }
    }

    fn minimal_setup() -> Setup {
        Setup {
            protocol_major: 11,
            protocol_minor: 0,
            release_number: 1,
            resource_id_base: 0x200000,
            resource_id_mask: 0x1fffff,
            motion_buffer_size: 0,
            vendor: b"test".to_vec(),
            maximum_request_length: 65535,
            pixmap_formats: Vec::new(),
            screens: Vec::new(),
        }
    }

    #[test]
    fn xauthority_parses_big_endian_fields() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&256_u16.to_be_bytes());
        for field in [
            b"host".as_slice(),
            b"0".as_slice(),
            b"MIT-MAGIC-COOKIE-1".as_slice(),
            &[1, 2, 3, 4],
        ] {
            bytes.extend_from_slice(&u16::try_from(field.len()).unwrap_or_default().to_be_bytes());
            bytes.extend_from_slice(field);
        }
        let entries = match parse_xauthority(&bytes) {
            Ok(v) => v,
            Err(e) => panic!("parse failed: {e}"),
        };
        assert_eq!(entries.len(), 1);
        assert_eq!(find_mit_cookie(&entries, 0).map(|a| a.data), Some(vec![1, 2, 3, 4]));
    }

    #[test]
    fn setup_request_matches_spec_layout() {
        let auth = Auth {
            name: b"MIT-MAGIC-COOKIE-1".to_vec(),
            data: vec![1, 2, 3],
        };
        let bytes = match encode_setup(ByteOrder::Little, Some(&auth)) {
            Ok(v) => v,
            Err(e) => panic!("encode failed: {e}"),
        };
        assert_eq!(bytes.first().copied(), Some(b'l'));
        assert_eq!(read_u16_at(&bytes, 2, ByteOrder::Little), Ok(11));
        assert_eq!(bytes.len() % 4, 0);
    }

    #[test]
    fn query_extension_reply_exposes_opcode_and_first_event() {
        let mut connection = Connection::new(FakeTransport::default(), ByteOrder::Little, &minimal_setup());
        let mut reply = vec![0_u8; 32];
        for (slot, value) in reply.iter_mut().zip([1_u8, 0, 0, 0, 0, 0, 0, 0, 1, 130, 64]) {
            *slot = value;
        }
        assert_eq!(
            connection.parse_query_extension_reply_details(b"MIT-SHM", &reply),
            Ok(Some((130, 64)))
        );
        assert_eq!(connection.extension_opcode(b"MIT-SHM"), Ok(130));
    }

    #[test]
    fn shm_attach_detach_and_input_focus_use_expected_request_codes() {
        let mut connection = Connection::new(FakeTransport::default(), ByteOrder::Little, &minimal_setup());
        connection.set_extension_opcode(b"MIT-SHM", 130);
        assert!(connection.shm_attach(ShmSeg(0x1122_3344), 17, false).is_ok());
        assert!(connection.get_input_focus().is_ok());
        assert!(connection.shm_detach(ShmSeg(0x1122_3344)).is_ok());
        let requests = connection.into_inner().sent;
        assert_eq!(requests.len(), 3);
        assert_eq!(requests.first().and_then(|request| request.first()).copied(), Some(130));
        assert_eq!(requests.get(1).and_then(|request| request.first()).copied(), Some(43));
        assert_eq!(requests.get(2).and_then(|request| request.first()).copied(), Some(130));
        assert_eq!(requests.get(2).and_then(|request| request.get(1)).copied(), Some(2));
    }

    #[test]
    fn request_bytes_are_scriptable() {
        let transport = FakeTransport::default();
        let mut c = Connection::new(transport, ByteOrder::Little, &minimal_setup());
        if let Err(e) = c.map_window(Window(0x11223344)) {
            panic!("map failed: {e}");
        }
        let transport = c.into_inner();
        assert_eq!(
            transport.sent.first().map(Vec::as_slice),
            Some(&[8, 0, 2, 0, 0x44, 0x33, 0x22, 0x11][..])
        );
    }

    #[test]
    fn net_wm_icon_request_contains_cardinal_dimensions_and_argb_word() {
        let mut connection = Connection::new(FakeTransport::default(), ByteOrder::Little, &minimal_setup());
        assert!(connection
            .set_net_wm_icon(Window(9), Atom(12), Atom(6), 1, 1, &[0x0401_0203])
            .is_ok());
        let transport = connection.into_inner();
        let request = transport.sent.first().map(Vec::as_slice).unwrap_or_default();
        assert_eq!(request.first().copied(), Some(18));
        assert_eq!(request.get(24..36), Some(&[1, 0, 0, 0, 1, 0, 0, 0, 3, 2, 1, 4][..]));
    }

    #[test]
    fn expose_packets_merge() {
        let transport = FakeTransport::default();
        let mut c = Connection::new(transport, ByteOrder::Little, &minimal_setup());
        let mut a = [0_u8; 32];
        a[0] = 12;
        a[4..8].copy_from_slice(&7_u32.to_le_bytes());
        a[8..10].copy_from_slice(&1_i16.to_le_bytes());
        a[10..12].copy_from_slice(&2_i16.to_le_bytes());
        a[12..14].copy_from_slice(&10_u16.to_le_bytes());
        a[14..16].copy_from_slice(&10_u16.to_le_bytes());
        a[16..18].copy_from_slice(&1_u16.to_le_bytes());
        assert_eq!(c.decode_packet(&a), Ok(Packet::Deferred));
        let mut b = a;
        b[8..10].copy_from_slice(&8_i16.to_le_bytes());
        b[10..12].copy_from_slice(&9_i16.to_le_bytes());
        b[16..18].copy_from_slice(&0_u16.to_le_bytes());
        let got = match c.decode_packet(&b) {
            Ok(Packet::Event(e)) => e,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(
            got,
            Event::Expose {
                window: Window(7),
                damage: Rect {
                    x: 1,
                    y: 2,
                    width: 17,
                    height: 17
                },
                remaining: 0
            }
        );
    }

    #[test]
    fn clipboard_incr_terminates_with_empty_chunk() {
        let text = "x".repeat(INCR_THRESHOLD.saturating_add(100));
        let mut offer = match ClipboardOffer::new(&text, 64 * 1024) {
            Ok(v) => v,
            Err(e) => panic!("offer: {e}"),
        };
        assert!(offer.needs_incr());
        let mut total = 0usize;
        let mut saw_empty = false;
        while let Some(chunk) = offer.next_chunk() {
            if chunk.is_empty() {
                saw_empty = true;
            } else {
                total = total.saturating_add(chunk.len());
            }
        }
        assert_eq!(total, text.len());
        assert!(saw_empty);
    }

    #[test]
    fn big_request_header_uses_extended_length() {
        let byte_len = 300_000_usize;
        let header = match request_header(72, 2, byte_len, ByteOrder::Little) {
            Ok(value) => value,
            Err(error) => panic!("BIG-REQUESTS header failed: {error:?}"),
        };
        assert_eq!(header.len(), 8);
        assert_eq!(header.first().copied(), Some(72));
        assert_eq!(header.get(1).copied(), Some(2));
        assert_eq!(read_u16_at(&header, 2, ByteOrder::Little), Ok(0));
        let expected_units = u32::try_from(byte_len.saturating_add(4) / 4).unwrap_or_default();
        assert_eq!(read_u32_at(&header, 4, ByteOrder::Little), Ok(expected_units));
    }

    #[test]
    fn keysyms_cover_required_scripts() {
        assert_eq!(keysym_to_char(0x41), Some('A'));
        assert_eq!(keysym_to_char(0x06e1), Some('А'));
        assert_eq!(keysym_to_char(0x07c1), Some('Α'));
        assert_eq!(keysym_to_char(0xffb7), Some('7'));
        assert_eq!(keysym_to_char(0xffbe), None); // F1 is non-character.
        assert_eq!(keysym_to_char(0x0101f642), Some('🙂'));
    }

    #[test]
    fn damaged_packets_never_panic() {
        for len in 0..32usize {
            let data = vec![0_u8; len];
            let _ = parse_setup_reply(&data, ByteOrder::Little);
        }
    }
}
