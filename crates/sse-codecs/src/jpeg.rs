//! Baseline JPEG decoder (ITU-T T.81, Huffman, 8-bit): one or three components, 4:4:4, 4:2:2 and 4:2:0
//! sampling, restart markers. Progressive, arithmetic-coded and 12-bit streams are refused.
//!
//! The inverse DCT and the chroma upsampling use the same integer arithmetic as libjpeg's default path
//! (`islow` IDCT, triangular "fancy" upsampling, fixed-point YCbCr conversion), so the output matches the
//! reference decoder within rounding. Every length and offset is checked before it is used.

use sse_core::{Error, Result};

const MAX_DIMENSION: u32 = 8192;
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21,
    28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54,
    47, 55, 62, 63,
];

/// Decoded picture: one byte per channel, rows top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// 1 for greyscale, 3 for red, green, blue.
    pub channels: u8,
    /// `width * height * channels` bytes.
    pub pixels: Vec<u8>,
}

/// Decodes a baseline JPEG file.
///
/// # Errors
/// Returns [`Error::Damaged`] for malformed or truncated streams and [`Error::Refused`] for JPEG processes
/// this decoder does not implement.
pub fn decode(data: &[u8]) -> Result<Image> {
    if data.get(..2) != Some(&[0xFF, 0xD8][..]) {
        return Err(Error::damaged("JPEG does not start with SOI"));
    }
    let mut position = 2_usize;
    let mut quant: [Option<[i64; 64]>; 4] = [None; 4];
    let mut dc: [Option<Huffman>; 4] = [None, None, None, None];
    let mut ac: [Option<Huffman>; 4] = [None, None, None, None];
    let mut frame: Option<Frame> = None;
    let mut restart_interval = 0_usize;
    loop {
        let marker = next_marker(data, &mut position)?;
        match marker {
            0xD9 => return Err(Error::damaged("JPEG ends before a scan")),
            0xC0 | 0xC1 => {
                let payload = segment(data, &mut position)?;
                frame = Some(parse_frame(payload)?);
            }
            0xC2..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                return Err(Error::Refused(
                    "JPEG process is not baseline or extended Huffman".to_owned(),
                ));
            }
            0xC4 => {
                let payload = segment(data, &mut position)?;
                parse_huffman(payload, &mut dc, &mut ac)?;
            }
            0xDB => {
                let payload = segment(data, &mut position)?;
                parse_quant(payload, &mut quant)?;
            }
            0xDD => {
                let payload = segment(data, &mut position)?;
                let [high, low] = <[u8; 2]>::try_from(payload).map_err(|_| Error::damaged("short DRI segment"))?;
                restart_interval = usize::from(u16::from_be_bytes([high, low]));
            }
            0xDA => {
                let payload = segment(data, &mut position)?;
                let frame = frame
                    .as_ref()
                    .ok_or_else(|| Error::damaged("JPEG scan before frame header"))?;
                let scan = parse_scan(payload, frame)?;
                let mut bits = Bits {
                    data,
                    position,
                    acc: 0,
                    left: 0,
                };
                let planes = decode_scan(&mut bits, frame, &scan, &quant, &dc, &ac, restart_interval)?;
                return Ok(to_image(frame, &planes));
            }
            0xE0..=0xEF | 0xFE => {
                segment(data, &mut position)?;
            }
            _ => return Err(Error::damaged("unexpected JPEG marker")),
        }
    }
}

fn next_marker(data: &[u8], position: &mut usize) -> Result<u8> {
    loop {
        let byte = *data
            .get(*position)
            .ok_or_else(|| Error::damaged("JPEG ends inside its markers"))?;
        *position = position
            .checked_add(1)
            .ok_or_else(|| Error::damaged("JPEG position overflow"))?;
        if byte != 0xFF {
            continue;
        }
        let marker = *data
            .get(*position)
            .ok_or_else(|| Error::damaged("JPEG ends inside its markers"))?;
        *position = position
            .checked_add(1)
            .ok_or_else(|| Error::damaged("JPEG position overflow"))?;
        if marker != 0x00 && marker != 0xFF {
            return Ok(marker);
        }
    }
}

fn segment<'a>(data: &'a [u8], position: &mut usize) -> Result<&'a [u8]> {
    let length_bytes = data
        .get(*position..position.saturating_add(2))
        .ok_or_else(|| Error::damaged("JPEG segment length is outside input"))?;
    let [high, low] = <[u8; 2]>::try_from(length_bytes).map_err(|_| Error::damaged("short segment length"))?;
    let length = usize::from(u16::from_be_bytes([high, low]));
    if length < 2 {
        return Err(Error::damaged("JPEG segment length below 2"));
    }
    let start = position
        .checked_add(2)
        .ok_or_else(|| Error::damaged("JPEG position overflow"))?;
    let end = position
        .checked_add(length)
        .ok_or_else(|| Error::damaged("JPEG position overflow"))?;
    let payload = data
        .get(start..end)
        .ok_or_else(|| Error::damaged("JPEG segment exceeds input"))?;
    *position = end;
    Ok(payload)
}

#[derive(Clone, Copy, Debug)]
struct Component {
    id: u8,
    h: usize,
    v: usize,
    quant: usize,
}

#[derive(Clone, Debug)]
struct Frame {
    width: usize,
    height: usize,
    components: Vec<Component>,
    h_max: usize,
    v_max: usize,
}

fn parse_frame(payload: &[u8]) -> Result<Frame> {
    let header = payload.get(..6).ok_or_else(|| Error::damaged("short SOF header"))?;
    if header.first().copied() != Some(8) {
        return Err(Error::Refused("only 8-bit JPEG samples are supported".to_owned()));
    }
    let height = u32::from(be16(header.get(1..3))?);
    let width = u32::from(be16(header.get(3..5))?);
    let count = usize::from(header.get(5).copied().unwrap_or(0));
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(Error::Refused("JPEG dimensions are outside 1..=8192".to_owned()));
    }
    if count != 1 && count != 3 {
        return Err(Error::Refused("JPEG must have one or three components".to_owned()));
    }
    let mut components = Vec::with_capacity(count);
    for index in 0..count {
        let at = 6_usize.saturating_add(index.saturating_mul(3));
        let record = payload
            .get(at..at.saturating_add(3))
            .ok_or_else(|| Error::damaged("short SOF component record"))?;
        let [id, sampling, quant] = <[u8; 3]>::try_from(record).map_err(|_| Error::damaged("short component"))?;
        let h = usize::from(sampling >> 4);
        let v = usize::from(sampling & 0x0F);
        if !(1..=2).contains(&h) || !(1..=2).contains(&v) || quant > 3 {
            return Err(Error::Refused("unsupported JPEG sampling factors".to_owned()));
        }
        components.push(Component {
            id,
            h,
            v,
            quant: usize::from(quant),
        });
    }
    let h_max = components.iter().map(|c| c.h).max().unwrap_or(1);
    let v_max = components.iter().map(|c| c.v).max().unwrap_or(1);
    for component in &components {
        let (ratio_x, ratio_y) = (h_max.checked_div(component.h), v_max.checked_div(component.v));
        if !matches!((ratio_x, ratio_y), (Some(1 | 2), Some(1 | 2)))
            || h_max.checked_rem(component.h) != Some(0)
            || v_max.checked_rem(component.v) != Some(0)
        {
            return Err(Error::Refused("unsupported JPEG chroma sampling".to_owned()));
        }
    }
    let width = usize::try_from(width).map_err(|_| Error::damaged("JPEG width does not fit"))?;
    let height = usize::try_from(height).map_err(|_| Error::damaged("JPEG height does not fit"))?;
    Ok(Frame {
        width,
        height,
        components,
        h_max,
        v_max,
    })
}

fn be16(bytes: Option<&[u8]>) -> Result<u16> {
    let [high, low] = <[u8; 2]>::try_from(bytes.ok_or_else(|| Error::damaged("short 16-bit field"))?)
        .map_err(|_| Error::damaged("short 16-bit field"))?;
    Ok(u16::from_be_bytes([high, low]))
}

#[derive(Clone, Debug)]
struct Huffman {
    maxcode: [i32; 17],
    valptr: [i32; 17],
    mincode: [i32; 17],
    values: Vec<u8>,
}

impl Huffman {
    fn build(counts: &[u8], values: Vec<u8>) -> Result<Self> {
        let mut maxcode = [-1_i32; 17];
        let mut valptr = [0_i32; 17];
        let mut mincode = [0_i32; 17];
        let mut code = 0_i32;
        let mut index = 0_i32;
        for (length, count) in counts.iter().enumerate() {
            let len = length.saturating_add(1);
            let count = i32::from(*count);
            if count > 0 {
                if let Some(slot) = valptr.get_mut(len) {
                    *slot = index;
                }
                if let Some(slot) = mincode.get_mut(len) {
                    *slot = code;
                }
                code = code
                    .checked_add(count)
                    .ok_or_else(|| Error::damaged("Huffman code overflow"))?;
                index = index
                    .checked_add(count)
                    .ok_or_else(|| Error::damaged("Huffman index overflow"))?;
                if let Some(slot) = maxcode.get_mut(len) {
                    *slot = code.wrapping_sub(1);
                }
            }
            if code > (1_i32 << len) {
                return Err(Error::damaged("Huffman code lengths overflow"));
            }
            code = code.wrapping_shl(1);
        }
        Ok(Self {
            maxcode,
            valptr,
            mincode,
            values,
        })
    }

    fn decode(&self, bits: &mut Bits<'_>) -> Result<u8> {
        let mut code = 0_i32;
        for len in 1..=16_usize {
            code = code.wrapping_shl(1) | i32::from(bits.bit()?);
            let max = self.maxcode.get(len).copied().unwrap_or(-1);
            if max >= 0 && code <= max {
                let shift = code.wrapping_sub(self.mincode.get(len).copied().unwrap_or(0));
                let index = self.valptr.get(len).copied().unwrap_or(0).wrapping_add(shift);
                let index = usize::try_from(index).map_err(|_| Error::damaged("Huffman index is negative"))?;
                return self
                    .values
                    .get(index)
                    .copied()
                    .ok_or_else(|| Error::damaged("Huffman code points past its values"));
            }
        }
        Err(Error::damaged("Huffman code is longer than 16 bits"))
    }
}

fn parse_huffman(payload: &[u8], dc: &mut [Option<Huffman>; 4], ac: &mut [Option<Huffman>; 4]) -> Result<()> {
    let mut at = 0_usize;
    while at < payload.len() {
        let class_slot = *payload.get(at).ok_or_else(|| Error::damaged("short DHT table"))?;
        let counts = payload
            .get(at.saturating_add(1)..at.saturating_add(17))
            .ok_or_else(|| Error::damaged("short DHT counts"))?;
        let total: usize = counts.iter().map(|count| usize::from(*count)).sum();
        if total > 256 {
            return Err(Error::damaged("DHT lists more than 256 values"));
        }
        let values = payload
            .get(at.saturating_add(17)..at.saturating_add(17).saturating_add(total))
            .ok_or_else(|| Error::damaged("short DHT values"))?
            .to_vec();
        let table = Huffman::build(counts, values)?;
        let slot = usize::from(class_slot & 0x0F);
        if slot > 3 {
            return Err(Error::damaged("DHT table index above 3"));
        }
        if class_slot >> 4 == 0 {
            if let Some(entry) = dc.get_mut(slot) {
                *entry = Some(table);
            }
        } else if let Some(entry) = ac.get_mut(slot) {
            *entry = Some(table);
        } else {
            return Err(Error::damaged("DHT class above 1"));
        }
        at = at.saturating_add(17).saturating_add(total);
    }
    Ok(())
}

fn parse_quant(payload: &[u8], quant: &mut [Option<[i64; 64]>; 4]) -> Result<()> {
    let mut at = 0_usize;
    while at < payload.len() {
        let precision_slot = *payload.get(at).ok_or_else(|| Error::damaged("short DQT table"))?;
        let wide = precision_slot >> 4 == 1;
        let slot = usize::from(precision_slot & 0x0F);
        let size = if wide { 128 } else { 64 };
        let body = payload
            .get(at.saturating_add(1)..at.saturating_add(1).saturating_add(size))
            .ok_or_else(|| Error::damaged("short DQT values"))?;
        let mut table = [0_i64; 64];
        for (index, natural) in ZIGZAG.iter().enumerate() {
            let value = if wide {
                let pair = body
                    .get(index.saturating_mul(2)..index.saturating_mul(2).saturating_add(2))
                    .ok_or_else(|| Error::damaged("short DQT values"))?;
                i64::from(u16::from_be_bytes(
                    <[u8; 2]>::try_from(pair).map_err(|_| Error::damaged("DQT pair"))?,
                ))
            } else {
                i64::from(*body.get(index).ok_or_else(|| Error::damaged("short DQT values"))?)
            };
            if let Some(entry) = table.get_mut(*natural) {
                *entry = value;
            }
        }
        if slot > 3 {
            return Err(Error::damaged("DQT table index above 3"));
        }
        if let Some(entry) = quant.get_mut(slot) {
            *entry = Some(table);
        }
        at = at.saturating_add(1).saturating_add(size);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct ScanComponent {
    index: usize,
    dc: usize,
    ac: usize,
}

struct Scan {
    components: Vec<ScanComponent>,
}

fn parse_scan(payload: &[u8], frame: &Frame) -> Result<Scan> {
    let count = usize::from(*payload.first().ok_or_else(|| Error::damaged("short SOS header"))?);
    if count != frame.components.len() {
        return Err(Error::Refused(
            "JPEG scans must cover every component at once".to_owned(),
        ));
    }
    let mut components = Vec::with_capacity(count);
    for index in 0..count {
        let at = 1_usize.saturating_add(index.saturating_mul(2));
        let record = payload
            .get(at..at.saturating_add(2))
            .ok_or_else(|| Error::damaged("short SOS component record"))?;
        let [id, tables] = <[u8; 2]>::try_from(record).map_err(|_| Error::damaged("SOS record"))?;
        let component = frame
            .components
            .iter()
            .position(|candidate| candidate.id == id)
            .ok_or_else(|| Error::damaged("SOS names an unknown component"))?;
        let dc = usize::from(tables >> 4);
        let ac = usize::from(tables & 0x0F);
        if dc > 3 || ac > 3 {
            return Err(Error::damaged("SOS table index above 3"));
        }
        components.push(ScanComponent {
            index: component,
            dc,
            ac,
        });
    }
    let spectral = payload
        .get(
            1_usize.saturating_add(count.saturating_mul(2))
                ..1_usize.saturating_add(count.saturating_mul(2)).saturating_add(3),
        )
        .ok_or_else(|| Error::damaged("short SOS spectral selection"))?;
    if spectral != [0, 63, 0] {
        return Err(Error::Refused("baseline JPEG scans use the full spectrum".to_owned()));
    }
    Ok(Scan { components })
}

struct Bits<'a> {
    data: &'a [u8],
    position: usize,
    acc: u32,
    left: u32,
}

impl Bits<'_> {
    fn bit(&mut self) -> Result<u8> {
        if self.left == 0 {
            self.fill();
        }
        self.left = self.left.saturating_sub(1);
        Ok(u8::from(self.acc.checked_shr(self.left).unwrap_or(0) & 1 == 1))
    }

    fn fill(&mut self) {
        let mut byte = 0_u8;
        match self.data.get(self.position).copied() {
            Some(0xFF) => match self.data.get(self.position.saturating_add(1)).copied() {
                Some(0x00) => {
                    byte = 0xFF;
                    self.position = self.position.saturating_add(2);
                }
                Some(_) => {}
                None => {}
            },
            Some(value) => {
                byte = value;
                self.position = self.position.saturating_add(1);
            }
            None => {}
        }
        self.acc = self.acc.wrapping_shl(8) | u32::from(byte);
        self.left = 8;
    }

    fn receive(&mut self, size: u32) -> Result<i64> {
        let mut value = 0_i64;
        for _ in 0..size {
            value = value.wrapping_shl(1) | i64::from(self.bit()?);
        }
        Ok(value)
    }

    fn restart(&mut self, expected: u8) -> Result<()> {
        self.acc = 0;
        self.left = 0;
        while self.position.saturating_add(1) < self.data.len() {
            if self.data.get(self.position).copied() == Some(0xFF) {
                let marker = self.data.get(self.position.saturating_add(1)).copied().unwrap_or(0);
                if marker == 0xD0_u8.wrapping_add(expected) {
                    self.position = self.position.saturating_add(2);
                    return Ok(());
                }
                if marker != 0xFF && marker != 0x00 {
                    return Err(Error::damaged("restart marker out of sequence"));
                }
            }
            self.position = self.position.saturating_add(1);
        }
        Err(Error::damaged("missing restart marker"))
    }
}

fn receive_extend(bits: &mut Bits<'_>, size: u32) -> Result<i64> {
    if size == 0 {
        return Ok(0);
    }
    let value = bits.receive(size)?;
    let threshold = 1_i64.wrapping_shl(size.saturating_sub(1));
    Ok(if value < threshold {
        value.wrapping_sub(1_i64.wrapping_shl(size)).wrapping_add(1)
    } else {
        value
    })
}

fn decode_block(
    bits: &mut Bits<'_>,
    dc: &Huffman,
    ac: &Huffman,
    quant: &[i64; 64],
    predictor: &mut i64,
    out: &mut [i64; 64],
) -> Result<()> {
    *out = [0; 64];
    let size = u32::from(dc.decode(bits)?);
    if size > 11 {
        return Err(Error::damaged("DC difference category above 11"));
    }
    *predictor = predictor.wrapping_add(receive_extend(bits, size)?);
    if let Some(slot) = out.first_mut() {
        *slot = predictor.wrapping_mul(quant.first().copied().unwrap_or(1));
    }
    let mut k = 1_usize;
    while k < 64 {
        let symbol = ac.decode(bits)?;
        let run = usize::from(symbol >> 4);
        let size = u32::from(symbol & 0x0F);
        if size == 0 {
            if run == 15 {
                k = k.saturating_add(16);
                continue;
            }
            break;
        }
        k = k.saturating_add(run);
        if k > 63 {
            return Err(Error::damaged("AC run passes the end of the block"));
        }
        let value = receive_extend(bits, size)?;
        let natural = ZIGZAG.get(k).copied().unwrap_or(0);
        if let Some(slot) = out.get_mut(natural) {
            *slot = value.wrapping_mul(quant.get(natural).copied().unwrap_or(1));
        }
        k = k.saturating_add(1);
    }
    Ok(())
}

fn descale(value: i64, shift: u32) -> i64 {
    value
        .wrapping_add(1_i64.wrapping_shl(shift.saturating_sub(1)))
        .checked_shr(shift)
        .unwrap_or(0)
}

const CONST_BITS: u32 = 13;
const PASS1_BITS: u32 = 2;
const FIX_0_298631336: i64 = 2446;
const FIX_0_390180644: i64 = 3196;
const FIX_0_541196100: i64 = 4433;
const FIX_0_765366865: i64 = 6270;
const FIX_0_899976223: i64 = 7373;
const FIX_1_175875602: i64 = 9633;
const FIX_1_501321110: i64 = 12299;
const FIX_1_847759065: i64 = 15137;
const FIX_1_961570560: i64 = 16069;
const FIX_2_053119869: i64 = 16819;
const FIX_2_562915447: i64 = 20995;
const FIX_3_072711026: i64 = 25172;

fn idct_1d(input: [i64; 8], shift: u32) -> [i64; 8] {
    let [in0, in1, in2, in3, in4, in5, in6, in7] = input;
    let z1 = in2.wrapping_add(in6).wrapping_mul(FIX_0_541196100);
    let tmp2 = z1.wrapping_add(in6.wrapping_mul(-FIX_1_847759065));
    let tmp3 = z1.wrapping_add(in2.wrapping_mul(FIX_0_765366865));
    let tmp0 = in0.wrapping_add(in4).wrapping_shl(CONST_BITS);
    let tmp1 = in0.wrapping_sub(in4).wrapping_shl(CONST_BITS);
    let tmp10 = tmp0.wrapping_add(tmp3);
    let tmp13 = tmp0.wrapping_sub(tmp3);
    let tmp11 = tmp1.wrapping_add(tmp2);
    let tmp12 = tmp1.wrapping_sub(tmp2);

    let mut t0 = in7;
    let mut t1 = in5;
    let mut t2 = in3;
    let mut t3 = in1;
    let z1 = t0.wrapping_add(t3);
    let z2 = t1.wrapping_add(t2);
    let z3 = t0.wrapping_add(t2);
    let z4 = t1.wrapping_add(t3);
    let z5 = z3.wrapping_add(z4).wrapping_mul(FIX_1_175875602);
    t0 = t0.wrapping_mul(FIX_0_298631336);
    t1 = t1.wrapping_mul(FIX_2_053119869);
    t2 = t2.wrapping_mul(FIX_3_072711026);
    t3 = t3.wrapping_mul(FIX_1_501321110);
    let z1 = z1.wrapping_mul(-FIX_0_899976223);
    let z2 = z2.wrapping_mul(-FIX_2_562915447);
    let z3 = z3.wrapping_mul(-FIX_1_961570560).wrapping_add(z5);
    let z4 = z4.wrapping_mul(-FIX_0_390180644).wrapping_add(z5);
    t0 = t0.wrapping_add(z1).wrapping_add(z3);
    t1 = t1.wrapping_add(z2).wrapping_add(z4);
    t2 = t2.wrapping_add(z2).wrapping_add(z3);
    t3 = t3.wrapping_add(z1).wrapping_add(z4);

    [
        descale(tmp10.wrapping_add(t3), shift),
        descale(tmp11.wrapping_add(t2), shift),
        descale(tmp12.wrapping_add(t1), shift),
        descale(tmp13.wrapping_add(t0), shift),
        descale(tmp13.wrapping_sub(t0), shift),
        descale(tmp12.wrapping_sub(t1), shift),
        descale(tmp11.wrapping_sub(t2), shift),
        descale(tmp10.wrapping_sub(t3), shift),
    ]
}

fn idct_block(coefficients: &[i64; 64]) -> [u8; 64] {
    let mut workspace = [0_i64; 64];
    for column in 0..8 {
        let input: [i64; 8] = std::array::from_fn(|row| {
            coefficients
                .get(row.saturating_mul(8).saturating_add(column))
                .copied()
                .unwrap_or(0)
        });
        let output = idct_1d(input, CONST_BITS - PASS1_BITS);
        for (row, value) in output.into_iter().enumerate() {
            if let Some(slot) = workspace.get_mut(row.saturating_mul(8).saturating_add(column)) {
                *slot = value;
            }
        }
    }
    let mut pixels = [0_u8; 64];
    for row in 0_usize..8 {
        let input: [i64; 8] = std::array::from_fn(|column| {
            workspace
                .get(row.saturating_mul(8).saturating_add(column))
                .copied()
                .unwrap_or(0)
        });
        let output = idct_1d(input, CONST_BITS + PASS1_BITS + 3);
        for (column, value) in output.into_iter().enumerate() {
            if let Some(slot) = pixels.get_mut(row.saturating_mul(8).saturating_add(column)) {
                *slot = u8::try_from(value.wrapping_add(128).clamp(0, 255)).unwrap_or(0);
            }
        }
    }
    pixels
}

struct Plane {
    width: usize,
    height: usize,
    samples: Vec<u8>,
}

impl Plane {
    fn new(width: usize, height: usize) -> Result<Self> {
        let count = width
            .checked_mul(height)
            .ok_or_else(|| Error::damaged("JPEG plane size overflow"))?;
        Ok(Self {
            width,
            height,
            samples: vec![0; count],
        })
    }

    fn sample(&self, x: usize, y: usize) -> i64 {
        let x = x.min(self.width.saturating_sub(1));
        let y = y.min(self.height.saturating_sub(1));
        self.samples
            .get(y.saturating_mul(self.width).saturating_add(x))
            .map_or(0, |value| i64::from(*value))
    }
}

fn decode_scan(
    bits: &mut Bits<'_>,
    frame: &Frame,
    scan: &Scan,
    quant: &[Option<[i64; 64]>; 4],
    dc: &[Option<Huffman>; 4],
    ac: &[Option<Huffman>; 4],
    restart_interval: usize,
) -> Result<Vec<Plane>> {
    let single = frame.components.len() == 1;
    let (h_max, v_max) = if single { (1, 1) } else { (frame.h_max, frame.v_max) };
    let mcus_x = frame.width.div_ceil(8_usize.saturating_mul(h_max));
    let mcus_y = frame.height.div_ceil(8_usize.saturating_mul(v_max));
    let mut planes = Vec::with_capacity(frame.components.len());
    for component in &frame.components {
        let (h, v) = if single { (1, 1) } else { (component.h, component.v) };
        let blocks_x = mcus_x
            .checked_mul(h)
            .ok_or_else(|| Error::damaged("block count overflow"))?;
        let blocks_y = mcus_y
            .checked_mul(v)
            .ok_or_else(|| Error::damaged("block count overflow"))?;
        planes.push(Plane::new(blocks_x.saturating_mul(8), blocks_y.saturating_mul(8))?);
    }
    let mut predictors = vec![0_i64; scan.components.len()];
    let total = mcus_x
        .checked_mul(mcus_y)
        .ok_or_else(|| Error::damaged("MCU count overflow"))?;
    let mut block = [0_i64; 64];
    for mcu in 0..total {
        if restart_interval > 0 && mcu > 0 && mcu.checked_rem(restart_interval) == Some(0) {
            let expected = u8::try_from(
                mcu.checked_div(restart_interval)
                    .unwrap_or(1)
                    .saturating_sub(1)
                    .checked_rem(8)
                    .unwrap_or(0),
            )
            .unwrap_or(0);
            bits.restart(expected)?;
            predictors.iter_mut().for_each(|value| *value = 0);
        }
        let (mx, my) = (
            mcu.checked_rem(mcus_x).unwrap_or(0),
            mcu.checked_div(mcus_x).unwrap_or(0),
        );
        for (slot, scan_component) in scan.components.iter().enumerate() {
            let component = frame
                .components
                .get(scan_component.index)
                .ok_or_else(|| Error::damaged("scan component index"))?;
            let (h, v) = if single { (1, 1) } else { (component.h, component.v) };
            let table_dc = dc
                .get(scan_component.dc)
                .and_then(Option::as_ref)
                .ok_or_else(|| Error::damaged("scan uses a missing DC table"))?;
            let table_ac = ac
                .get(scan_component.ac)
                .and_then(Option::as_ref)
                .ok_or_else(|| Error::damaged("scan uses a missing AC table"))?;
            let table_quant = quant
                .get(component.quant)
                .and_then(|table| table.as_ref())
                .ok_or_else(|| Error::damaged("component uses a missing quantisation table"))?;
            let plane = planes
                .get_mut(scan_component.index)
                .ok_or_else(|| Error::damaged("plane index"))?;
            let stride = plane.width;
            for by in 0..v {
                for bx in 0..h {
                    let predictor = predictors
                        .get_mut(slot)
                        .ok_or_else(|| Error::damaged("predictor index"))?;
                    decode_block(bits, table_dc, table_ac, table_quant, predictor, &mut block)?;
                    let pixels = idct_block(&block);
                    let x0 = mx.saturating_mul(h).saturating_add(bx).saturating_mul(8);
                    let y0 = my.saturating_mul(v).saturating_add(by).saturating_mul(8);
                    for row in 0_usize..8 {
                        for column in 0_usize..8 {
                            if let Some(value) = pixels.get(row.saturating_mul(8).saturating_add(column)) {
                                if let Some(slot) = plane.samples.get_mut(
                                    y0.saturating_add(row)
                                        .saturating_mul(stride)
                                        .saturating_add(x0)
                                        .saturating_add(column),
                                ) {
                                    *slot = *value;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(planes)
}

fn to_image(frame: &Frame, planes: &[Plane]) -> Image {
    let width = frame.width;
    let height = frame.height;
    let full: Vec<Plane> = frame
        .components
        .iter()
        .zip(planes)
        .map(|(component, plane)| {
            if frame.components.len() == 1 {
                return crop(plane, width, height);
            }
            upsample(plane, component, frame, width, height)
        })
        .collect();
    let channels = full.len();
    let mut pixels = Vec::with_capacity(width.saturating_mul(height).saturating_mul(channels));
    if channels == 1 {
        for plane in &full {
            pixels.extend(plane.samples.iter().copied());
        }
    } else {
        let (luma, cb, cr) = match (full.first(), full.get(1), full.get(2)) {
            (Some(y), Some(cb), Some(cr)) => (y, cb, cr),
            _ => {
                return Image {
                    width: 0,
                    height: 0,
                    channels: 1,
                    pixels,
                }
            }
        };
        for index in 0..width.saturating_mul(height) {
            let y = i64::from(luma.samples.get(index).copied().unwrap_or(0));
            let cb = i64::from(cb.samples.get(index).copied().unwrap_or(128)).wrapping_sub(128);
            let cr = i64::from(cr.samples.get(index).copied().unwrap_or(128)).wrapping_sub(128);
            let r = y.wrapping_add(91_881_i64.wrapping_mul(cr).wrapping_add(32_768) >> 16);
            let g = y.wrapping_add(
                32_768_i64
                    .wrapping_sub(22_554_i64.wrapping_mul(cb))
                    .wrapping_sub(46_802_i64.wrapping_mul(cr))
                    >> 16,
            );
            let b = y.wrapping_add(116_130_i64.wrapping_mul(cb).wrapping_add(32_768) >> 16);
            pixels.extend([clamp8(r), clamp8(g), clamp8(b)]);
        }
    }
    Image {
        width: u32::try_from(width).unwrap_or(0),
        height: u32::try_from(height).unwrap_or(0),
        channels: u8::try_from(channels).unwrap_or(1),
        pixels,
    }
}

fn clamp8(value: i64) -> u8 {
    u8::try_from(value.clamp(0, 255)).unwrap_or(0)
}

fn crop(plane: &Plane, width: usize, height: usize) -> Plane {
    let mut samples = Vec::with_capacity(width.saturating_mul(height));
    for y in 0..height {
        for x in 0..width {
            samples.push(u8::try_from(plane.sample(x, y)).unwrap_or(0));
        }
    }
    Plane { width, height, samples }
}

fn upsample(plane: &Plane, component: &Component, frame: &Frame, width: usize, height: usize) -> Plane {
    let factor_x = frame.h_max.checked_div(component.h).unwrap_or(1);
    let factor_y = frame.v_max.checked_div(component.v).unwrap_or(1);
    let real_width = width.saturating_mul(component.h).div_ceil(frame.h_max).max(1);
    let real_height = height.saturating_mul(component.v).div_ceil(frame.v_max).max(1);
    let source = |x: usize, y: usize| {
        plane.sample(
            x.min(real_width.saturating_sub(1)),
            y.min(real_height.saturating_sub(1)),
        )
    };
    let mut samples = Vec::with_capacity(width.saturating_mul(height));
    for y in 0..height {
        for x in 0..width {
            let value = match (factor_x, factor_y) {
                (1, 1) => source(x, y),
                (2, 1) => h2v1(&source, x / 2, x % 2 == 1, y, real_width),
                (2, 2) => h2v2(&source, x / 2, x % 2 == 1, y, real_width, real_height),
                _ => source(
                    x.checked_div(factor_x).unwrap_or(0),
                    y.checked_div(factor_y).unwrap_or(0),
                ),
            };
            samples.push(u8::try_from(value.clamp(0, 255)).unwrap_or(0));
        }
    }
    Plane { width, height, samples }
}

fn h2v1(source: &impl Fn(usize, usize) -> i64, column: usize, odd: bool, y: usize, width: usize) -> i64 {
    let this = source(column, y).wrapping_mul(3);
    if !odd {
        if column == 0 {
            return source(0, y);
        }
        return this.wrapping_add(source(column.saturating_sub(1), y)).wrapping_add(1) >> 2;
    }
    if column.saturating_add(1) >= width {
        return source(column, y);
    }
    this.wrapping_add(source(column.saturating_add(1), y)).wrapping_add(2) >> 2
}

fn h2v2(source: &impl Fn(usize, usize) -> i64, column: usize, odd: bool, y: usize, width: usize, height: usize) -> i64 {
    let row = y / 2;
    let neighbour = if y % 2 == 1 {
        row.saturating_add(1).min(height.saturating_sub(1))
    } else {
        row.saturating_sub(1)
    };
    let sums = |x: usize| source(x, row).wrapping_mul(3).wrapping_add(source(x, neighbour));
    let this = sums(column);
    if !odd {
        if column == 0 {
            return this.wrapping_mul(4).wrapping_add(8) >> 4;
        }
        return this
            .wrapping_mul(3)
            .wrapping_add(sums(column.saturating_sub(1)))
            .wrapping_add(8)
            >> 4;
    }
    if column.saturating_add(1) >= width {
        return this.wrapping_mul(4).wrapping_add(7) >> 4;
    }
    this.wrapping_mul(3)
        .wrapping_add(sums(column.saturating_add(1)))
        .wrapping_add(7)
        >> 4
}

#[cfg(test)]
mod tests {
    use super::{decode, Error};

    const GRAY: &[u8] = include_bytes!("../../../fixtures/synthetic/jpeg/gray.jpg");
    const GRAY_REF: &[u8] = include_bytes!("../../../fixtures/synthetic/jpeg/gray.png");
    const YUV444: &[u8] = include_bytes!("../../../fixtures/synthetic/jpeg/yuv444.jpg");
    const YUV444_REF: &[u8] = include_bytes!("../../../fixtures/synthetic/jpeg/yuv444.png");
    const YUV422: &[u8] = include_bytes!("../../../fixtures/synthetic/jpeg/yuv422.jpg");
    const YUV422_REF: &[u8] = include_bytes!("../../../fixtures/synthetic/jpeg/yuv422.png");
    const YUV420: &[u8] = include_bytes!("../../../fixtures/synthetic/jpeg/yuv420.jpg");
    const YUV420_REF: &[u8] = include_bytes!("../../../fixtures/synthetic/jpeg/yuv420.png");
    const RESTART: &[u8] = include_bytes!("../../../fixtures/synthetic/jpeg/restart420.jpg");
    const RESTART_REF: &[u8] = include_bytes!("../../../fixtures/synthetic/jpeg/restart420.png");
    const ODD: &[u8] = include_bytes!("../../../fixtures/synthetic/jpeg/odd17x13.jpg");
    const ODD_REF: &[u8] = include_bytes!("../../../fixtures/synthetic/jpeg/odd17x13.png");

    fn assert_matches_reference(name: &str, jpeg: &[u8], reference: &[u8]) {
        let image = decode(jpeg).unwrap_or_else(|error| panic!("{name}: {error:?}"));
        let expected = crate::png::decode(reference).unwrap_or_else(|error| panic!("{name}: {error:?}"));
        assert_eq!(image.width, expected.width, "{name} width");
        assert_eq!(image.height, expected.height, "{name} height");
        let channels = usize::from(image.channels);
        for (index, (ours, theirs)) in image.pixels.chunks(channels).zip(expected.pixels.chunks(4)).enumerate() {
            for (channel, value) in ours.iter().enumerate() {
                let want = theirs.get(channel).copied().unwrap_or(0);
                assert!(
                    value.abs_diff(want) <= 3,
                    "{name}: pixel {index} channel {channel} is {value}, reference {want}"
                );
            }
        }
    }

    #[test]
    fn greyscale_matches_reference() {
        assert_matches_reference("gray", GRAY, GRAY_REF);
    }

    #[test]
    fn full_chroma_matches_reference() {
        assert_matches_reference("yuv444", YUV444, YUV444_REF);
    }

    #[test]
    fn horizontal_half_chroma_matches_reference() {
        assert_matches_reference("yuv422", YUV422, YUV422_REF);
    }

    #[test]
    fn quarter_chroma_matches_reference() {
        assert_matches_reference("yuv420", YUV420, YUV420_REF);
    }

    #[test]
    fn restart_markers_match_reference() {
        assert_matches_reference("restart420", RESTART, RESTART_REF);
    }

    #[test]
    fn odd_size_matches_reference() {
        assert_matches_reference("odd17x13", ODD, ODD_REF);
    }

    #[test]
    fn progressive_streams_are_refused() {
        let mut bytes = YUV420.to_vec();
        let position = bytes
            .windows(2)
            .position(|pair| pair == [0xFF, 0xC0])
            .unwrap_or_else(|| panic!("fixture has no SOF0"));
        if let Some(marker) = bytes.get_mut(position + 1) {
            *marker = 0xC2;
        }
        assert!(matches!(decode(&bytes), Err(Error::Refused(_))));
    }

    #[test]
    fn oversized_dimensions_are_refused() {
        let mut bytes = GRAY.to_vec();
        let position = bytes
            .windows(2)
            .position(|pair| pair == [0xFF, 0xC0])
            .unwrap_or_else(|| panic!("fixture has no SOF0"));
        if let Some(width) = bytes.get_mut(position + 7..position + 9) {
            width.copy_from_slice(&[0x40, 0x01]);
        }
        assert!(decode(&bytes).is_err());
    }

    #[test]
    fn truncated_inputs_never_panic() {
        for length in 0..YUV420.len() {
            let prefix = YUV420.get(..length).unwrap_or_default();
            let _ = decode(prefix);
        }
    }

    #[test]
    fn corrupted_copies_never_panic() {
        let mut state: u32 = 0x9E37_79B9;
        for _ in 0..2000 {
            let mut bytes = YUV420.to_vec();
            for _ in 0..3 {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let position = usize::try_from(state).unwrap_or(0) % bytes.len();
                if let Some(byte) = bytes.get_mut(position) {
                    *byte ^= u8::try_from(state >> 24).unwrap_or(1);
                }
            }
            let _ = decode(&bytes);
        }
    }
}
