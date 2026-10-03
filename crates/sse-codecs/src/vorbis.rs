//! Dependency-free Vorbis-I decoder used for game menu sounds.

use std::f32::consts::PI;

use crate::ogg;
use sse_core::{Error, Result};

const MAX_CODEBOOK_ENTRIES: usize = 1_000_000;
const MAX_LOOKUP_SCALARS: usize = 4_000_000;

/// Fully decoded interleaved signed 16-bit PCM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcm {
    /// Channel count.
    pub channels: u8,
    /// Sample rate in hertz.
    pub rate: u32,
    /// Interleaved samples.
    pub samples: Vec<i16>,
}

struct Bits<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit: 0 }
    }

    fn read(&mut self, width: u8) -> Result<u32> {
        if width > 32 {
            return Err(Error::damaged("Vorbis bit width exceeds 32"));
        }
        let mut value = 0_u32;
        for shift in 0..width {
            let byte = self
                .data
                .get(self.bit.saturating_div(8))
                .copied()
                .ok_or_else(|| Error::damaged("truncated Vorbis packet"))?;
            let bit = byte.wrapping_shr(u32::try_from(self.bit % 8).unwrap_or_default()) & 1;
            value |= u32::from(bit).wrapping_shl(u32::from(shift));
            self.bit = self
                .bit
                .checked_add(1)
                .ok_or_else(|| Error::damaged("Vorbis bit offset overflow"))?;
        }
        Ok(value)
    }

    fn flag(&mut self) -> Result<bool> {
        Ok(self.read(1)? != 0)
    }
}

fn ilog(value: usize) -> u8 {
    if value == 0 {
        0
    } else {
        u8::try_from(usize::BITS.saturating_sub(value.leading_zeros())).unwrap_or(usize::BITS as u8)
    }
}

fn header<'a>(packet: &'a [u8], kind: u8) -> Result<&'a [u8]> {
    if packet.first() != Some(&kind) || packet.get(1..7) != Some(b"vorbis") {
        return Err(Error::damaged("invalid Vorbis header"));
    }
    packet.get(7..).ok_or_else(|| Error::damaged("short Vorbis header"))
}

fn le32(bytes: &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(
        <[u8; 4]>::try_from(bytes).map_err(|_| Error::damaged("short Vorbis u32"))?,
    ))
}

#[derive(Clone, Copy)]
struct Ident {
    channels: u8,
    rate: u32,
    small: usize,
    large: usize,
}

fn identification(packet: &[u8]) -> Result<Ident> {
    let data = header(packet, 1)?;
    if data.len() < 23 {
        return Err(Error::damaged("short Vorbis identification header"));
    }
    if le32(data.get(..4).ok_or_else(|| Error::damaged("Vorbis version"))?)? != 0 {
        return Err(Error::Refused("unsupported Vorbis version".to_owned()));
    }
    let channels = data.get(4).copied().ok_or_else(|| Error::damaged("Vorbis channels"))?;
    if channels == 0 || channels > 8 {
        return Err(Error::Refused("unsupported Vorbis channel count".to_owned()));
    }
    let rate = le32(data.get(5..9).ok_or_else(|| Error::damaged("Vorbis sample rate"))?)?;
    if rate == 0 {
        return Err(Error::damaged("zero Vorbis sample rate"));
    }
    let packed = data.get(21).copied().ok_or_else(|| Error::damaged("Vorbis block sizes"))?;
    let small = 1_usize
        .checked_shl(u32::from(packed & 15))
        .ok_or_else(|| Error::damaged("Vorbis small block size"))?;
    let large = 1_usize
        .checked_shl(u32::from(packed.wrapping_shr(4)))
        .ok_or_else(|| Error::damaged("Vorbis large block size"))?;
    if small < 64 || large < small || large > 8192 {
        return Err(Error::damaged("invalid Vorbis block sizes"));
    }
    if data.get(22).copied().unwrap_or(0) & 1 == 0 {
        return Err(Error::damaged("Vorbis identification framing bit"));
    }
    Ok(Ident {
        channels,
        rate,
        small,
        large,
    })
}

fn validate_comment(packet: &[u8]) -> Result<()> {
    let data = header(packet, 3)?;
    let mut position = 0_usize;
    let take = |data: &[u8], position: &mut usize| -> Result<usize> {
        let end = position.saturating_add(4);
        let value = usize::try_from(le32(
            data.get(*position..end)
                .ok_or_else(|| Error::damaged("short Vorbis comment"))?,
        )?)
        .map_err(|_| Error::damaged("Vorbis comment length"))?;
        *position = end;
        Ok(value)
    };
    let vendor = take(data, &mut position)?;
    position = position
        .checked_add(vendor)
        .ok_or_else(|| Error::damaged("Vorbis vendor length overflow"))?;
    if position > data.len() {
        return Err(Error::damaged("truncated Vorbis vendor"));
    }
    let comments = take(data, &mut position)?;
    if comments > data.len().saturating_div(4).saturating_add(1) {
        return Err(Error::Refused("Vorbis comment count limit exceeded".to_owned()));
    }
    for _ in 0..comments {
        let length = take(data, &mut position)?;
        position = position
            .checked_add(length)
            .ok_or_else(|| Error::damaged("Vorbis comment length overflow"))?;
        if position > data.len() {
            return Err(Error::damaged("truncated Vorbis comment"));
        }
    }
    if data.get(position).copied().unwrap_or(0) & 1 == 0 {
        return Err(Error::damaged("Vorbis comment framing bit"));
    }
    Ok(())
}

#[derive(Clone, Default)]
struct HuffNode {
    zero: Option<usize>,
    one: Option<usize>,
    symbol: Option<usize>,
}

fn insert_code(nodes: &mut Vec<HuffNode>, node: usize, depth: u8, target: u8, symbol: usize) -> bool {
    if depth == target {
        let Some(slot) = nodes.get_mut(node) else { return false };
        if slot.symbol.is_some() || slot.zero.is_some() || slot.one.is_some() {
            return false;
        }
        slot.symbol = Some(symbol);
        return true;
    }
    if nodes.get(node).is_some_and(|item| item.symbol.is_some()) {
        return false;
    }
    for one in [false, true] {
        let child = if one {
            nodes.get(node).and_then(|item| item.one)
        } else {
            nodes.get(node).and_then(|item| item.zero)
        };
        let child = if let Some(index) = child {
            index
        } else {
            let index = nodes.len();
            nodes.push(HuffNode::default());
            if let Some(parent) = nodes.get_mut(node) {
                if one {
                    parent.one = Some(index);
                } else {
                    parent.zero = Some(index);
                }
            }
            index
        };
        if insert_code(nodes, child, depth.saturating_add(1), target, symbol) {
            return true;
        }
    }
    false
}

#[derive(Clone)]
struct Codebook {
    dimensions: usize,
    tree: Vec<HuffNode>,
    single: Option<usize>,
    vectors: Option<Vec<f32>>,
}

impl Codebook {
    fn scalar(&self, bits: &mut Bits<'_>) -> Result<usize> {
        if let Some(symbol) = self.single {
            let _ = bits.read(1)?;
            return Ok(symbol);
        }
        let mut node = 0_usize;
        for _ in 0..32 {
            let branch = bits.flag()?;
            node = if branch {
                self.tree.get(node).and_then(|item| item.one)
            } else {
                self.tree.get(node).and_then(|item| item.zero)
            }
            .ok_or_else(|| Error::damaged("invalid Vorbis Huffman code"))?;
            if let Some(symbol) = self.tree.get(node).and_then(|item| item.symbol) {
                return Ok(symbol);
            }
        }
        Err(Error::damaged("Vorbis Huffman code exceeds 32 bits"))
    }

    fn vector<'a>(&'a self, bits: &mut Bits<'_>) -> Result<&'a [f32]> {
        let symbol = self.scalar(bits)?;
        let values = self
            .vectors
            .as_ref()
            .ok_or_else(|| Error::damaged("Vorbis scalar-only codebook used as VQ"))?;
        let start = symbol
            .checked_mul(self.dimensions)
            .ok_or_else(|| Error::damaged("Vorbis VQ offset overflow"))?;
        values
            .get(start..start.saturating_add(self.dimensions))
            .ok_or_else(|| Error::damaged("Vorbis VQ vector out of range"))
    }
}

fn float32_unpack(value: u32) -> f32 {
    let mantissa = (value & 0x001f_ffff) as i32;
    let exponent = i32::try_from((value & 0x7fe0_0000).wrapping_shr(21)).unwrap_or_default();
    let signed = if value & 0x8000_0000 != 0 {
        mantissa.saturating_neg()
    } else {
        mantissa
    };
    (signed as f32) * 2_f32.powi(exponent.saturating_sub(788))
}

fn pow_leq(base: usize, exponent: usize, limit: usize) -> bool {
    let mut value = 1_usize;
    for _ in 0..exponent {
        let Some(next) = value.checked_mul(base) else { return false };
        value = next;
        if value > limit {
            return false;
        }
    }
    true
}

fn lookup1_values(entries: usize, dimensions: usize) -> usize {
    if dimensions == 0 || entries == 0 {
        return 0;
    }
    let mut low = 1_usize;
    let mut high = entries.saturating_add(1);
    while low.saturating_add(1) < high {
        let mid = low.saturating_add(high.saturating_sub(low).saturating_div(2));
        if pow_leq(mid, dimensions, entries) {
            low = mid;
        } else {
            high = mid;
        }
    }
    low
}

fn read_codebook(bits: &mut Bits<'_>) -> Result<Codebook> {
    if bits.read(24)? != 0x0056_4342 {
        return Err(Error::damaged("Vorbis codebook sync"));
    }
    let dimensions = usize::try_from(bits.read(16)?).map_err(|_| Error::damaged("Vorbis codebook dimensions"))?;
    let entries = usize::try_from(bits.read(24)?).map_err(|_| Error::damaged("Vorbis codebook entries"))?;
    if dimensions == 0 || entries == 0 || entries > MAX_CODEBOOK_ENTRIES {
        return Err(Error::Refused("Vorbis codebook dimensions/entry limit exceeded".to_owned()));
    }
    let mut lengths = vec![0_u8; entries];
    if bits.flag()? {
        let mut entry = 0_usize;
        let mut length = u8::try_from(bits.read(5)?).unwrap_or(0).saturating_add(1);
        while entry < entries {
            let width = ilog(entries.saturating_sub(entry));
            let count = usize::try_from(bits.read(width)?).map_err(|_| Error::damaged("Vorbis ordered codebook count"))?;
            if count == 0 || entry.saturating_add(count) > entries {
                return Err(Error::damaged("invalid ordered Vorbis codebook"));
            }
            for slot in lengths.iter_mut().skip(entry).take(count) {
                *slot = length;
            }
            entry = entry.saturating_add(count);
            length = length.saturating_add(1);
            if length > 32 && entry < entries {
                return Err(Error::damaged("Vorbis codeword length exceeds 32"));
            }
        }
    } else {
        let sparse = bits.flag()?;
        for slot in &mut lengths {
            if !sparse || bits.flag()? {
                *slot = u8::try_from(bits.read(5)?).unwrap_or(0).saturating_add(1);
            }
        }
    }
    let active: Vec<(usize, u8)> = lengths
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, length)| *length != 0)
        .collect();
    if active.is_empty() {
        return Err(Error::damaged("empty Vorbis codebook"));
    }
    let single = if active.len() == 1 {
        let (symbol, length) = active.first().copied().unwrap_or((0, 0));
        if length != 1 {
            return Err(Error::damaged("invalid single-entry Vorbis codebook"));
        }
        Some(symbol)
    } else {
        None
    };
    let mut tree = vec![HuffNode::default()];
    if single.is_none() {
        for (symbol, length) in active {
            if !insert_code(&mut tree, 0, 0, length, symbol) {
                return Err(Error::damaged("oversubscribed Vorbis codebook"));
            }
        }
    }

    let lookup = bits.read(4)?;
    if lookup > 2 {
        return Err(Error::damaged("reserved Vorbis codebook lookup type"));
    }
    let vectors = if lookup == 0 {
        None
    } else {
        let minimum = float32_unpack(bits.read(32)?);
        let delta = float32_unpack(bits.read(32)?);
        let value_bits = u8::try_from(bits.read(4)?).unwrap_or(0).saturating_add(1);
        let sequence = bits.flag()?;
        let lookup_values = if lookup == 1 {
            lookup1_values(entries, dimensions)
        } else {
            entries
                .checked_mul(dimensions)
                .ok_or_else(|| Error::Refused("Vorbis lookup size overflow".to_owned()))?
        };
        if lookup_values > MAX_LOOKUP_SCALARS {
            return Err(Error::Refused("Vorbis codebook lookup limit exceeded".to_owned()));
        }
        let mut multiplicands = Vec::with_capacity(lookup_values);
        for _ in 0..lookup_values {
            multiplicands.push(bits.read(value_bits)?);
        }
        let scalar_count = entries
            .checked_mul(dimensions)
            .ok_or_else(|| Error::Refused("Vorbis VQ size overflow".to_owned()))?;
        if scalar_count > MAX_LOOKUP_SCALARS {
            return Err(Error::Refused("Vorbis expanded VQ limit exceeded".to_owned()));
        }
        let mut expanded = Vec::with_capacity(scalar_count);
        for entry in 0..entries {
            let mut last = 0_f32;
            let mut divisor = 1_usize;
            for dimension in 0..dimensions {
                let index = if lookup == 1 {
                    entry.saturating_div(divisor) % lookup_values.max(1)
                } else {
                    entry.saturating_mul(dimensions).saturating_add(dimension)
                };
                let multiplicand = multiplicands.get(index).copied().unwrap_or(0) as f32;
                let value = multiplicand * delta + minimum + last;
                expanded.push(value);
                if sequence {
                    last = value;
                }
                if lookup == 1 {
                    divisor = divisor.saturating_mul(lookup_values.max(1));
                }
            }
        }
        Some(expanded)
    };
    Ok(Codebook {
        dimensions,
        tree,
        single,
        vectors,
    })
}

#[derive(Clone)]
struct Floor1 {
    partitions: Vec<usize>,
    class_dimensions: Vec<usize>,
    class_subclasses: Vec<u8>,
    class_masterbooks: Vec<Option<usize>>,
    subclass_books: Vec<Vec<Option<usize>>>,
    multiplier: usize,
    x: Vec<usize>,
}

#[derive(Clone)]
struct Residue {
    kind: u16,
    begin: usize,
    end: usize,
    partition: usize,
    classifications: usize,
    classbook: usize,
    books: Vec<Vec<Option<usize>>>,
}

#[derive(Clone)]
struct Mapping {
    submaps: usize,
    coupling: Vec<(usize, usize)>,
    mux: Vec<usize>,
    floors: Vec<usize>,
    residues: Vec<usize>,
}

#[derive(Clone, Copy)]
struct Mode {
    long: bool,
    mapping: usize,
}

struct Setup {
    books: Vec<Codebook>,
    floors: Vec<Floor1>,
    residues: Vec<Residue>,
    mappings: Vec<Mapping>,
    modes: Vec<Mode>,
}

fn read_floor1(bits: &mut Bits<'_>, books: usize) -> Result<Floor1> {
    let partition_count = usize::try_from(bits.read(5)?).unwrap_or(0);
    let mut partitions = Vec::with_capacity(partition_count);
    let mut maximum_class = 0_usize;
    for _ in 0..partition_count {
        let class = usize::try_from(bits.read(4)?).unwrap_or(0);
        maximum_class = maximum_class.max(class);
        partitions.push(class);
    }
    let class_count = if partitions.is_empty() { 0 } else { maximum_class.saturating_add(1) };
    let mut class_dimensions = Vec::with_capacity(class_count);
    let mut class_subclasses = Vec::with_capacity(class_count);
    let mut class_masterbooks = Vec::with_capacity(class_count);
    let mut subclass_books = Vec::with_capacity(class_count);
    for _ in 0..class_count {
        let dimensions = usize::try_from(bits.read(3)?).unwrap_or(0).saturating_add(1);
        let subclasses = u8::try_from(bits.read(2)?).unwrap_or(0);
        let master = if subclasses == 0 {
            None
        } else {
            let book = usize::try_from(bits.read(8)?).unwrap_or(usize::MAX);
            if book >= books {
                return Err(Error::damaged("Vorbis floor masterbook out of range"));
            }
            Some(book)
        };
        let count = 1_usize.checked_shl(u32::from(subclasses)).unwrap_or(0);
        let mut list = Vec::with_capacity(count);
        for _ in 0..count {
            let raw = bits.read(8)?;
            let book = raw.saturating_sub(1);
            if raw == 0 {
                list.push(None);
            } else {
                let index = usize::try_from(book).unwrap_or(usize::MAX);
                if index >= books {
                    return Err(Error::damaged("Vorbis floor subclass book out of range"));
                }
                list.push(Some(index));
            }
        }
        class_dimensions.push(dimensions);
        class_subclasses.push(subclasses);
        class_masterbooks.push(master);
        subclass_books.push(list);
    }
    let multiplier = usize::try_from(bits.read(2)?).unwrap_or(0).saturating_add(1);
    let range_bits = u8::try_from(bits.read(4)?).unwrap_or(0);
    let maximum_x = 1_usize.checked_shl(u32::from(range_bits)).unwrap_or(0);
    let mut x = vec![0, maximum_x];
    for class in &partitions {
        let dimensions = class_dimensions.get(*class).copied().unwrap_or(0);
        for _ in 0..dimensions {
            x.push(usize::try_from(bits.read(range_bits)?).unwrap_or(0));
        }
    }
    Ok(Floor1 {
        partitions,
        class_dimensions,
        class_subclasses,
        class_masterbooks,
        subclass_books,
        multiplier,
        x,
    })
}

fn setup(packet: &[u8], ident: Ident) -> Result<Setup> {
    let data = header(packet, 5)?;
    let mut bits = Bits::new(data);
    let book_count = usize::try_from(bits.read(8)?).unwrap_or(0).saturating_add(1);
    let mut books = Vec::with_capacity(book_count);
    for _ in 0..book_count {
        books.push(read_codebook(&mut bits)?);
    }
    let time_count = usize::try_from(bits.read(6)?).unwrap_or(0).saturating_add(1);
    for _ in 0..time_count {
        if bits.read(16)? != 0 {
            return Err(Error::damaged("unsupported Vorbis time transform"));
        }
    }
    let floor_count = usize::try_from(bits.read(6)?).unwrap_or(0).saturating_add(1);
    let mut floors = Vec::with_capacity(floor_count);
    for _ in 0..floor_count {
        match bits.read(16)? {
            0 => return Err(Error::Refused("Vorbis floor 0 is unsupported".to_owned())),
            1 => floors.push(read_floor1(&mut bits, books.len())?),
            _ => return Err(Error::damaged("reserved Vorbis floor type")),
        }
    }
    let residue_count = usize::try_from(bits.read(6)?).unwrap_or(0).saturating_add(1);
    let mut residues = Vec::with_capacity(residue_count);
    for _ in 0..residue_count {
        let kind = u16::try_from(bits.read(16)?).unwrap_or(u16::MAX);
        if kind > 2 {
            return Err(Error::damaged("reserved Vorbis residue type"));
        }
        let begin = usize::try_from(bits.read(24)?).unwrap_or(usize::MAX);
        let end = usize::try_from(bits.read(24)?).unwrap_or(usize::MAX);
        let partition = usize::try_from(bits.read(24)?).unwrap_or(usize::MAX).saturating_add(1);
        let classifications = usize::try_from(bits.read(6)?).unwrap_or(0).saturating_add(1);
        let classbook = usize::try_from(bits.read(8)?).unwrap_or(usize::MAX);
        if classbook >= books.len() {
            return Err(Error::damaged("Vorbis residue classbook out of range"));
        }
        let mut cascades = Vec::with_capacity(classifications);
        for _ in 0..classifications {
            let low = bits.read(3)?;
            let high = if bits.flag()? { bits.read(5)? } else { 0 };
            cascades.push(high.wrapping_shl(3) | low);
        }
        let mut residue_books = Vec::with_capacity(classifications);
        for cascade in cascades {
            let mut row = Vec::with_capacity(8);
            for pass in 0..8_u8 {
                if cascade & 1_u32.wrapping_shl(u32::from(pass)) != 0 {
                    let index = usize::try_from(bits.read(8)?).unwrap_or(usize::MAX);
                    if index >= books.len() || books.get(index).is_none_or(|book| book.vectors.is_none()) {
                        return Err(Error::damaged("Vorbis residue VQ book out of range"));
                    }
                    row.push(Some(index));
                } else {
                    row.push(None);
                }
            }
            residue_books.push(row);
        }
        residues.push(Residue {
            kind,
            begin,
            end,
            partition,
            classifications,
            classbook,
            books: residue_books,
        });
    }
    let mapping_count = usize::try_from(bits.read(6)?).unwrap_or(0).saturating_add(1);
    let mut mappings = Vec::with_capacity(mapping_count);
    for _ in 0..mapping_count {
        if bits.read(16)? != 0 {
            return Err(Error::damaged("reserved Vorbis mapping type"));
        }
        let submaps = if bits.flag()? {
            usize::try_from(bits.read(4)?).unwrap_or(0).saturating_add(1)
        } else {
            1
        };
        let coupling_steps = if bits.flag()? {
            usize::try_from(bits.read(8)?).unwrap_or(0).saturating_add(1)
        } else {
            0
        };
        let channel_bits = ilog(usize::from(ident.channels).saturating_sub(1));
        let mut coupling = Vec::with_capacity(coupling_steps);
        for _ in 0..coupling_steps {
            let magnitude = usize::try_from(bits.read(channel_bits)?).unwrap_or(usize::MAX);
            let angle = usize::try_from(bits.read(channel_bits)?).unwrap_or(usize::MAX);
            if magnitude == angle || magnitude >= usize::from(ident.channels) || angle >= usize::from(ident.channels) {
                return Err(Error::damaged("invalid Vorbis channel coupling"));
            }
            coupling.push((magnitude, angle));
        }
        if bits.read(2)? != 0 {
            return Err(Error::damaged("Vorbis mapping reserved bits"));
        }
        let mut mux = vec![0_usize; usize::from(ident.channels)];
        if submaps > 1 {
            for slot in &mut mux {
                let value = usize::try_from(bits.read(4)?).unwrap_or(usize::MAX);
                if value >= submaps {
                    return Err(Error::damaged("Vorbis mapping mux out of range"));
                }
                *slot = value;
            }
        }
        let mut map_floors = Vec::with_capacity(submaps);
        let mut map_residues = Vec::with_capacity(submaps);
        for _ in 0..submaps {
            let _ = bits.read(8)?;
            let floor = usize::try_from(bits.read(8)?).unwrap_or(usize::MAX);
            let residue = usize::try_from(bits.read(8)?).unwrap_or(usize::MAX);
            if floor >= floors.len() || residue >= residues.len() {
                return Err(Error::damaged("Vorbis submap out of range"));
            }
            map_floors.push(floor);
            map_residues.push(residue);
        }
        mappings.push(Mapping {
            submaps,
            coupling,
            mux,
            floors: map_floors,
            residues: map_residues,
        });
    }
    let mode_count = usize::try_from(bits.read(6)?).unwrap_or(0).saturating_add(1);
    let mut modes = Vec::with_capacity(mode_count);
    for _ in 0..mode_count {
        let long = bits.flag()?;
        if bits.read(16)? != 0 || bits.read(16)? != 0 {
            return Err(Error::damaged("unsupported Vorbis mode transform/window"));
        }
        let mapping = usize::try_from(bits.read(8)?).unwrap_or(usize::MAX);
        if mapping >= mappings.len() {
            return Err(Error::damaged("Vorbis mode mapping out of range"));
        }
        modes.push(Mode { long, mapping });
    }
    if !bits.flag()? {
        return Err(Error::damaged("Vorbis setup framing bit"));
    }
    Ok(Setup {
        books,
        floors,
        residues,
        mappings,
        modes,
    })
}


fn low_neighbor(values: &[usize], index: usize) -> Result<usize> {
    let target = values.get(index).copied().ok_or_else(|| Error::damaged("Vorbis floor X"))?;
    let mut best = None;
    for prior in 0..index {
        let value = values.get(prior).copied().unwrap_or(0);
        if value < target && best.is_none_or(|old| value > values.get(old).copied().unwrap_or(0)) {
            best = Some(prior);
        }
    }
    best.ok_or_else(|| Error::damaged("Vorbis floor low neighbor"))
}

fn high_neighbor(values: &[usize], index: usize) -> Result<usize> {
    let target = values.get(index).copied().ok_or_else(|| Error::damaged("Vorbis floor X"))?;
    let mut best = None;
    for prior in 0..index {
        let value = values.get(prior).copied().unwrap_or(usize::MAX);
        if value > target && best.is_none_or(|old| value < values.get(old).copied().unwrap_or(usize::MAX)) {
            best = Some(prior);
        }
    }
    best.ok_or_else(|| Error::damaged("Vorbis floor high neighbor"))
}

fn render_point(x0: usize, y0: i32, x1: usize, y1: i32, x: usize) -> i32 {
    let dy = i64::from(y1).saturating_sub(i64::from(y0));
    let adx = i64::try_from(x1.saturating_sub(x0)).unwrap_or(i64::MAX).max(1);
    let ady = dy.abs();
    let dx = i64::try_from(x.saturating_sub(x0)).unwrap_or(i64::MAX);
    let offset = ady.saturating_mul(dx).saturating_div(adx);
    let base = i64::from(y0);
    i32::try_from(if dy < 0 { base.saturating_sub(offset) } else { base.saturating_add(offset) })
        .unwrap_or(if dy < 0 { i32::MIN } else { i32::MAX })
}

struct FloorPacket {
    y: Vec<i32>,
    active: Vec<bool>,
}

fn decode_floor(floor: &Floor1, books: &[Codebook], bits: &mut Bits<'_>) -> Result<Option<FloorPacket>> {
    if !bits.flag()? {
        return Ok(None);
    }
    let range = match floor.multiplier {
        1 => 256_i32,
        2 => 128_i32,
        3 => 86_i32,
        4 => 64_i32,
        _ => return Err(Error::damaged("Vorbis floor multiplier")),
    };
    let width = ilog(usize::try_from(range.saturating_sub(1)).unwrap_or(0));
    let mut y = vec![
        i32::try_from(bits.read(width)?).unwrap_or(0),
        i32::try_from(bits.read(width)?).unwrap_or(0),
    ];
    for class in &floor.partitions {
        let dimensions = floor.class_dimensions.get(*class).copied().unwrap_or(0);
        let subclasses = floor.class_subclasses.get(*class).copied().unwrap_or(0);
        let mut selector = if subclasses == 0 {
            0_usize
        } else {
            let master = floor
                .class_masterbooks
                .get(*class)
                .copied()
                .flatten()
                .ok_or_else(|| Error::damaged("Vorbis floor masterbook"))?;
            books.get(master).ok_or_else(|| Error::damaged("Vorbis floor masterbook"))?.scalar(bits)?
        };
        let mask = 1_usize
            .checked_shl(u32::from(subclasses))
            .unwrap_or(0)
            .saturating_sub(1);
        for _ in 0..dimensions {
            let book = floor
                .subclass_books
                .get(*class)
                .and_then(|row| row.get(selector & mask))
                .copied()
                .flatten();
            let value = if let Some(index) = book {
                i32::try_from(
                    books
                        .get(index)
                        .ok_or_else(|| Error::damaged("Vorbis floor subclass book"))?
                        .scalar(bits)?,
                )
                .unwrap_or(i32::MAX)
            } else {
                0
            };
            y.push(value);
            selector = selector.wrapping_shr(u32::from(subclasses));
        }
    }
    if y.len() != floor.x.len() {
        return Err(Error::damaged("Vorbis floor value count mismatch"));
    }
    let mut active = vec![true; y.len()];
    for index in 2..y.len() {
        let low = low_neighbor(&floor.x, index)?;
        let high = high_neighbor(&floor.x, index)?;
        let predicted = render_point(
            floor.x.get(low).copied().unwrap_or(0),
            y.get(low).copied().unwrap_or(0),
            floor.x.get(high).copied().unwrap_or(0),
            y.get(high).copied().unwrap_or(0),
            floor.x.get(index).copied().unwrap_or(0),
        );
        let value = y.get(index).copied().unwrap_or(0);
        let high_room = range.saturating_sub(predicted);
        let low_room = predicted;
        let room = 2_i32.saturating_mul(high_room.min(low_room));
        let final_value = if value == 0 {
            if let Some(slot) = active.get_mut(index) {
                *slot = false;
            }
            predicted
        } else if value >= room {
            if high_room > low_room {
                value.saturating_sub(low_room).saturating_add(predicted)
            } else {
                predicted
                    .saturating_sub(value)
                    .saturating_add(high_room)
                    .saturating_sub(1)
            }
        } else if value & 1 != 0 {
            predicted.saturating_sub(value.saturating_add(1).saturating_div(2))
        } else {
            predicted.saturating_add(value.saturating_div(2))
        };
        if let Some(slot) = y.get_mut(index) {
            *slot = final_value;
        }
    }
    Ok(Some(FloorPacket { y, active }))
}

fn floor_amplitude(value: i32) -> f32 {
    let exponent = (value.saturating_sub(255) as f32) * (std::f32::consts::LN_10 / 20.0) * (140.0 / 256.0);
    exponent.exp()
}

fn render_line(x0: usize, y0: i32, x1: usize, y1: i32, curve: &mut [f32]) {
    if x1 <= x0 {
        return;
    }
    let dy = y1.saturating_sub(y0);
    let adx = i32::try_from(x1.saturating_sub(x0)).unwrap_or(i32::MAX).max(1);
    let mut ady = dy.abs();
    let base = dy.saturating_div(adx);
    let sy = if dy < 0 { base.saturating_sub(1) } else { base.saturating_add(1) };
    ady = ady.saturating_sub(base.abs().saturating_mul(adx));
    let mut error = 0_i32;
    let mut y = y0;
    if let Some(slot) = curve.get_mut(x0) {
        *slot = floor_amplitude(y);
    }
    for x in x0.saturating_add(1)..x1.min(curve.len()) {
        error = error.saturating_add(ady);
        if error >= adx {
            error = error.saturating_sub(adx);
            y = y.saturating_add(sy);
        } else {
            y = y.saturating_add(base);
        }
        if let Some(slot) = curve.get_mut(x) {
            *slot = floor_amplitude(y);
        }
    }
}

fn floor_curve(floor: &Floor1, packet: &FloorPacket, n: usize) -> Vec<f32> {
    let mut curve = vec![0_f32; n];
    let mut order: Vec<usize> = (0..floor.x.len()).collect();
    order.sort_by_key(|index| floor.x.get(*index).copied().unwrap_or(usize::MAX));
    let mut lx = 0_usize;
    let mut ly = packet.y.first().copied().unwrap_or(0).saturating_mul(i32::try_from(floor.multiplier).unwrap_or(1));
    let mut hx = 0_usize;
    let mut hy = ly;
    for index in order.into_iter().skip(1) {
        if packet.active.get(index).copied().unwrap_or(false) {
            hx = floor.x.get(index).copied().unwrap_or(n);
            hy = packet
                .y
                .get(index)
                .copied()
                .unwrap_or(0)
                .saturating_mul(i32::try_from(floor.multiplier).unwrap_or(1));
            render_line(lx, ly, hx.min(n), hy, &mut curve);
            lx = hx;
            ly = hy;
        }
    }
    if hx < n {
        for slot in curve.iter_mut().skip(hx) {
            *slot = floor_amplitude(hy);
        }
    }
    curve
}

fn decode_partition(
    output: &mut [f32],
    offset: usize,
    size: usize,
    kind: u16,
    book: &Codebook,
    bits: &mut Bits<'_>,
) -> Result<()> {
    let dimensions = book.dimensions;
    if dimensions == 0 {
        return Err(Error::damaged("zero-dimensional Vorbis residue book"));
    }
    if kind == 0 {
        let step = size.saturating_div(dimensions);
        for i in 0..step {
            let vector = book.vector(bits)?;
            for (dimension, value) in vector.iter().copied().enumerate() {
                let index = offset
                    .saturating_add(i)
                    .saturating_add(dimension.saturating_mul(step));
                if let Some(slot) = output.get_mut(index) {
                    *slot += value;
                }
            }
        }
    } else {
        let mut written = 0_usize;
        while written < size {
            let vector = book.vector(bits)?;
            for value in vector {
                if written >= size {
                    break;
                }
                if let Some(slot) = output.get_mut(offset.saturating_add(written)) {
                    *slot += *value;
                }
                written = written.saturating_add(1);
            }
        }
    }
    Ok(())
}

fn residue_classifications(
    residue: &Residue,
    books: &[Codebook],
    bits: &mut Bits<'_>,
    partitions: usize,
) -> Result<Vec<usize>> {
    let classbook = books
        .get(residue.classbook)
        .ok_or_else(|| Error::damaged("Vorbis residue classbook"))?;
    let words = classbook.dimensions;
    if words == 0 {
        return Err(Error::damaged("zero-dimensional Vorbis residue classbook"));
    }
    let mut result = vec![0_usize; partitions];
    let mut partition = 0_usize;
    while partition < partitions {
        let mut value = classbook.scalar(bits)?;
        let count = words.min(partitions.saturating_sub(partition));
        for reverse in (0..count).rev() {
            let index = partition.saturating_add(reverse);
            if let Some(slot) = result.get_mut(index) {
                *slot = value % residue.classifications;
            }
            value = value.saturating_div(residue.classifications);
        }
        partition = partition.saturating_add(count);
    }
    Ok(result)
}

fn decode_residue_channels(
    residue: &Residue,
    books: &[Codebook],
    bits: &mut Bits<'_>,
    channels: &mut [Vec<f32>],
    skip: &[bool],
    n: usize,
) -> Result<()> {
    let begin = residue.begin.min(n);
    let end = residue.end.min(n);
    if begin >= end || residue.partition == 0 {
        return Ok(());
    }
    let partitions = end.saturating_sub(begin).saturating_div(residue.partition);
    let mut classes = vec![Vec::<usize>::new(); channels.len()];
    for pass in 0..8_usize {
        if pass == 0 {
            for (channel, class) in classes.iter_mut().enumerate() {
                if !skip.get(channel).copied().unwrap_or(true) {
                    *class = residue_classifications(residue, books, bits, partitions)?;
                }
            }
        }
        for partition in 0..partitions {
            for channel in 0..channels.len() {
                if skip.get(channel).copied().unwrap_or(true) {
                    continue;
                }
                let class = classes
                    .get(channel)
                    .and_then(|items| items.get(partition))
                    .copied()
                    .unwrap_or(0);
                let book_index = residue
                    .books
                    .get(class)
                    .and_then(|row| row.get(pass))
                    .copied()
                    .flatten();
                if let Some(book_index) = book_index {
                    let offset = begin.saturating_add(partition.saturating_mul(residue.partition));
                    let output = channels
                        .get_mut(channel)
                        .ok_or_else(|| Error::damaged("Vorbis residue channel"))?;
                    decode_partition(
                        output,
                        offset,
                        residue.partition,
                        residue.kind,
                        books.get(book_index).ok_or_else(|| Error::damaged("Vorbis residue book"))?,
                        bits,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn decode_residue_type2(
    residue: &Residue,
    books: &[Codebook],
    bits: &mut Bits<'_>,
    channels: &mut [Vec<f32>],
    skip: &[bool],
    n: usize,
) -> Result<()> {
    if skip.iter().all(|value| *value) || channels.is_empty() {
        return Ok(());
    }
    let channel_count = channels.len();
    let actual = n
        .checked_mul(channel_count)
        .ok_or_else(|| Error::damaged("Vorbis residue-2 size overflow"))?;
    let begin = residue.begin.min(actual);
    let end = residue.end.min(actual);
    if begin >= end || residue.partition == 0 {
        return Ok(());
    }
    let partitions = end.saturating_sub(begin).saturating_div(residue.partition);
    let classes = residue_classifications(residue, books, bits, partitions)?;
    let mut interleaved = vec![0_f32; actual];
    for pass in 0..8_usize {
        for partition in 0..partitions {
            let class = classes.get(partition).copied().unwrap_or(0);
            let book_index = residue
                .books
                .get(class)
                .and_then(|row| row.get(pass))
                .copied()
                .flatten();
            if let Some(book_index) = book_index {
                let offset = begin.saturating_add(partition.saturating_mul(residue.partition));
                decode_partition(
                    &mut interleaved,
                    offset,
                    residue.partition,
                    1,
                    books.get(book_index).ok_or_else(|| Error::damaged("Vorbis residue-2 book"))?,
                    bits,
                )?;
            }
        }
    }
    for sample in 0..n {
        for channel in 0..channel_count {
            let source = sample.saturating_mul(channel_count).saturating_add(channel);
            let value = interleaved.get(source).copied().unwrap_or(0.0);
            if let Some(slot) = channels.get_mut(channel).and_then(|items| items.get_mut(sample)) {
                *slot = value;
            }
        }
    }
    Ok(())
}
