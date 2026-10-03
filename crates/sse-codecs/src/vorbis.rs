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
        u8::try_from(usize::BITS.saturating_sub(value.leading_zeros())).unwrap_or(u8::MAX)
    }
}

fn header(packet: &[u8], kind: u8) -> Result<&[u8]> {
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
    let packed = data
        .get(21)
        .copied()
        .ok_or_else(|| Error::damaged("Vorbis block sizes"))?;
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
    full: bool,
}

fn insert_code(nodes: &mut Vec<HuffNode>, node: usize, depth: u8, target: u8, symbol: usize) -> bool {
    if nodes.get(node).is_none_or(|item| item.full) {
        return false;
    }
    if depth == target {
        let Some(slot) = nodes.get_mut(node) else {
            return false;
        };
        if slot.symbol.is_some() || slot.zero.is_some() || slot.one.is_some() {
            return false;
        }
        slot.symbol = Some(symbol);
        slot.full = true;
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
            let zero_full = nodes
                .get(node)
                .and_then(|item| item.zero)
                .and_then(|index| nodes.get(index))
                .is_some_and(|item| item.full);
            let one_full = nodes
                .get(node)
                .and_then(|item| item.one)
                .and_then(|index| nodes.get(index))
                .is_some_and(|item| item.full);
            if let Some(parent) = nodes.get_mut(node) {
                parent.full = zero_full && one_full;
            }
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
        let Some(next) = value.checked_mul(base) else {
            return false;
        };
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
        return Err(Error::Refused(
            "Vorbis codebook dimensions/entry limit exceeded".to_owned(),
        ));
    }
    let mut lengths = vec![0_u8; entries];
    if bits.flag()? {
        let mut entry = 0_usize;
        let mut length = u8::try_from(bits.read(5)?).unwrap_or(0).saturating_add(1);
        while entry < entries {
            let width = ilog(entries.saturating_sub(entry));
            let count =
                usize::try_from(bits.read(width)?).map_err(|_| Error::damaged("Vorbis ordered codebook count"))?;
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
                    entry
                        .checked_div(divisor)
                        .unwrap_or(0)
                        .checked_rem(lookup_values.max(1))
                        .unwrap_or(0)
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
    let class_count = if partitions.is_empty() {
        0
    } else {
        maximum_class.saturating_add(1)
    };
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
    let target = values
        .get(index)
        .copied()
        .ok_or_else(|| Error::damaged("Vorbis floor X"))?;
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
    let target = values
        .get(index)
        .copied()
        .ok_or_else(|| Error::damaged("Vorbis floor X"))?;
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
    let offset = ady.saturating_mul(dx).checked_div(adx).unwrap_or(0);
    let base = i64::from(y0);
    i32::try_from(if dy < 0 {
        base.saturating_sub(offset)
    } else {
        base.saturating_add(offset)
    })
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
            books
                .get(master)
                .ok_or_else(|| Error::damaged("Vorbis floor masterbook"))?
                .scalar(bits)?
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

const FLOOR1_INVERSE_DB: [f32; 256] = [
    1.0649863e-07_f32, 1.1341951e-07_f32, 1.2079015e-07_f32, 1.2863978e-07_f32,
    1.3699951e-07_f32, 1.4590251e-07_f32, 1.5538408e-07_f32, 1.6548181e-07_f32,
    1.7623575e-07_f32, 1.8768855e-07_f32, 1.9988561e-07_f32, 2.1287530e-07_f32,
    2.2670913e-07_f32, 2.4144197e-07_f32, 2.5713223e-07_f32, 2.7384213e-07_f32,
    2.9163793e-07_f32, 3.1059021e-07_f32, 3.3077411e-07_f32, 3.5226968e-07_f32,
    3.7516214e-07_f32, 3.9954229e-07_f32, 4.2550680e-07_f32, 4.5315863e-07_f32,
    4.8260743e-07_f32, 5.1396998e-07_f32, 5.4737065e-07_f32, 5.8294187e-07_f32,
    6.2082472e-07_f32, 6.6116941e-07_f32, 7.0413592e-07_f32, 7.4989464e-07_f32,
    7.9862701e-07_f32, 8.5052630e-07_f32, 9.0579828e-07_f32, 9.6466216e-07_f32,
    1.0273513e-06_f32, 1.0941144e-06_f32, 1.1652161e-06_f32, 1.2409384e-06_f32,
    1.3215816e-06_f32, 1.4074654e-06_f32, 1.4989305e-06_f32, 1.5963394e-06_f32,
    1.7000785e-06_f32, 1.8105592e-06_f32, 1.9282195e-06_f32, 2.0535261e-06_f32,
    2.1869758e-06_f32, 2.3290978e-06_f32, 2.4804557e-06_f32, 2.6416497e-06_f32,
    2.8133190e-06_f32, 2.9961443e-06_f32, 3.1908506e-06_f32, 3.3982101e-06_f32,
    3.6190449e-06_f32, 3.8542308e-06_f32, 4.1047004e-06_f32, 4.3714470e-06_f32,
    4.6555282e-06_f32, 4.9580707e-06_f32, 5.2802740e-06_f32, 5.6234160e-06_f32,
    5.9888572e-06_f32, 6.3780469e-06_f32, 6.7925283e-06_f32, 7.2339451e-06_f32,
    7.7040476e-06_f32, 8.2047000e-06_f32, 8.7378876e-06_f32, 9.3057248e-06_f32,
    9.9104632e-06_f32, 1.0554501e-05_f32, 1.1240392e-05_f32, 1.1970856e-05_f32,
    1.2748789e-05_f32, 1.3577278e-05_f32, 1.4459606e-05_f32, 1.5399272e-05_f32,
    1.6400004e-05_f32, 1.7465768e-05_f32, 1.8600792e-05_f32, 1.9809576e-05_f32,
    2.1096914e-05_f32, 2.2467911e-05_f32, 2.3928002e-05_f32, 2.5482978e-05_f32,
    2.7139006e-05_f32, 2.8902651e-05_f32, 3.0780908e-05_f32, 3.2781225e-05_f32,
    3.4911534e-05_f32, 3.7180282e-05_f32, 3.9596466e-05_f32, 4.2169667e-05_f32,
    4.4910090e-05_f32, 4.7828601e-05_f32, 5.0936773e-05_f32, 5.4246931e-05_f32,
    5.7772202e-05_f32, 6.1526565e-05_f32, 6.5524908e-05_f32, 6.9783085e-05_f32,
    7.4317983e-05_f32, 7.9147585e-05_f32, 8.4291040e-05_f32, 8.9768747e-05_f32,
    9.5602426e-05_f32, 0.00010181521_f32, 0.00010843174_f32, 0.00011547824_f32,
    0.00012298267_f32, 0.00013097477_f32, 0.00013948625_f32, 0.00014855085_f32,
    0.00015820453_f32, 0.00016848555_f32, 0.00017943469_f32, 0.00019109536_f32,
    0.00020351382_f32, 0.00021673929_f32, 0.00023082423_f32, 0.00024582449_f32,
    0.00026179955_f32, 0.00027881276_f32, 0.00029693158_f32, 0.00031622787_f32,
    0.00033677814_f32, 0.00035866388_f32, 0.00038197188_f32, 0.00040679456_f32,
    0.00043323036_f32, 0.00046138411_f32, 0.00049136745_f32, 0.00052329927_f32,
    0.00055730621_f32, 0.00059352311_f32, 0.00063209358_f32, 0.00067317058_f32,
    0.00071691700_f32, 0.00076350630_f32, 0.00081312324_f32, 0.00086596457_f32,
    0.00092223983_f32, 0.00098217216_f32, 0.0010459992_f32, 0.0011139742_f32,
    0.0011863665_f32, 0.0012634633_f32, 0.0013455702_f32, 0.0014330129_f32,
    0.0015261382_f32, 0.0016253153_f32, 0.0017309374_f32, 0.0018434235_f32,
    0.0019632195_f32, 0.0020908006_f32, 0.0022266726_f32, 0.0023713743_f32,
    0.0025254795_f32, 0.0026895994_f32, 0.0028643847_f32, 0.0030505286_f32,
    0.0032487691_f32, 0.0034598925_f32, 0.0036847358_f32, 0.0039241906_f32,
    0.0041792066_f32, 0.0044507950_f32, 0.0047400328_f32, 0.0050480668_f32,
    0.0053761186_f32, 0.0057254891_f32, 0.0060975636_f32, 0.0064938176_f32,
    0.0069158225_f32, 0.0073652516_f32, 0.0078438871_f32, 0.0083536271_f32,
    0.0088964928_f32, 0.009474637_f32, 0.010090352_f32, 0.010746080_f32,
    0.011444421_f32, 0.012188144_f32, 0.012980198_f32, 0.013823725_f32,
    0.014722068_f32, 0.015678791_f32, 0.016697687_f32, 0.017782797_f32,
    0.018938423_f32, 0.020169149_f32, 0.021479854_f32, 0.022875735_f32,
    0.024362330_f32, 0.025945531_f32, 0.027631618_f32, 0.029427276_f32,
    0.031339626_f32, 0.033376252_f32, 0.035545228_f32, 0.037855157_f32,
    0.040315199_f32, 0.042935108_f32, 0.045725273_f32, 0.048696758_f32,
    0.051861348_f32, 0.055231591_f32, 0.058820850_f32, 0.062643361_f32,
    0.066714279_f32, 0.071049749_f32, 0.075666962_f32, 0.080584227_f32,
    0.085821044_f32, 0.091398179_f32, 0.097337747_f32, 0.10366330_f32,
    0.11039993_f32, 0.11757434_f32, 0.12521498_f32, 0.13335215_f32,
    0.14201813_f32, 0.15124727_f32, 0.16107617_f32, 0.17154380_f32,
    0.18269168_f32, 0.19456402_f32, 0.20720788_f32, 0.22067342_f32,
    0.23501402_f32, 0.25028656_f32, 0.26655159_f32, 0.28387361_f32,
    0.30232132_f32, 0.32196786_f32, 0.34289114_f32, 0.36517414_f32,
    0.38890521_f32, 0.41417847_f32, 0.44109412_f32, 0.46975890_f32,
    0.50028648_f32, 0.53279791_f32, 0.56742212_f32, 0.60429640_f32,
    0.64356699_f32, 0.68538959_f32, 0.72993007_f32, 0.77736504_f32,
    0.82788260_f32, 0.88168307_f32, 0.9389798_f32, 1.0_f32,
];

fn floor_amplitude(value: i32) -> f32 {
    let index = usize::try_from(value & 255).unwrap_or_default();
    FLOOR1_INVERSE_DB.get(index).copied().unwrap_or(0.0)
}

fn render_line(x0: usize, y0: i32, x1: usize, y1: i32, curve: &mut [f32]) {
    if x1 <= x0 {
        return;
    }
    let dy = y1.saturating_sub(y0);
    let adx = i32::try_from(x1.saturating_sub(x0)).unwrap_or(i32::MAX).max(1);
    let mut ady = dy.abs();
    let base = dy.checked_div(adx).unwrap_or(0);
    let sy = if dy < 0 {
        base.saturating_sub(1)
    } else {
        base.saturating_add(1)
    };
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
    let mut ly = packet
        .y
        .first()
        .copied()
        .unwrap_or(0)
        .saturating_mul(i32::try_from(floor.multiplier).unwrap_or(1));
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
        let step = size.checked_div(dimensions).unwrap_or(0);
        for i in 0..step {
            let vector = book.vector(bits)?;
            for (dimension, value) in vector.iter().copied().enumerate() {
                let index = offset.saturating_add(i).saturating_add(dimension.saturating_mul(step));
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
    let partitions = end.saturating_sub(begin).checked_div(residue.partition).unwrap_or(0);
    let classbook = books
        .get(residue.classbook)
        .ok_or_else(|| Error::damaged("Vorbis residue classbook"))?;
    let words = classbook.dimensions;
    if words == 0 {
        return Err(Error::damaged("zero-dimensional Vorbis residue classbook"));
    }
    let mut classes = vec![vec![0_usize; partitions]; channels.len()];
    for pass in 0..8_usize {
        let mut partition = 0_usize;
        while partition < partitions {
            let count = words.min(partitions.saturating_sub(partition));
            if pass == 0 {
                for channel in 0..channels.len() {
                    if skip.get(channel).copied().unwrap_or(true) {
                        continue;
                    }
                    let mut value = classbook.scalar(bits)?;
                    for reverse in (0..count).rev() {
                        let index = partition.saturating_add(reverse);
                        if let Some(slot) = classes.get_mut(channel).and_then(|row| row.get_mut(index)) {
                            *slot = value.checked_rem(residue.classifications).unwrap_or(0);
                        }
                        value = value.checked_div(residue.classifications).unwrap_or(0);
                    }
                }
            }
            for offset_partition in 0..count {
                let absolute = partition.saturating_add(offset_partition);
                for channel in 0..channels.len() {
                    if skip.get(channel).copied().unwrap_or(true) {
                        continue;
                    }
                    let class = classes
                        .get(channel)
                        .and_then(|row| row.get(absolute))
                        .copied()
                        .unwrap_or(0);
                    let book_index = residue
                        .books
                        .get(class)
                        .and_then(|row| row.get(pass))
                        .copied()
                        .flatten();
                    if let Some(book_index) = book_index {
                        let output = channels
                            .get_mut(channel)
                            .ok_or_else(|| Error::damaged("Vorbis residue channel"))?;
                        decode_partition(
                            output,
                            begin.saturating_add(absolute.saturating_mul(residue.partition)),
                            residue.partition,
                            residue.kind,
                            books
                                .get(book_index)
                                .ok_or_else(|| Error::damaged("Vorbis residue book"))?,
                            bits,
                        )?;
                    }
                }
            }
            partition = partition.saturating_add(count);
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
    let partitions = end.saturating_sub(begin).checked_div(residue.partition).unwrap_or(0);
    let classbook = books
        .get(residue.classbook)
        .ok_or_else(|| Error::damaged("Vorbis residue-2 classbook"))?;
    let words = classbook.dimensions;
    if words == 0 {
        return Err(Error::damaged("zero-dimensional Vorbis residue-2 classbook"));
    }
    let mut classes = vec![0_usize; partitions];
    let mut interleaved = vec![0_f32; actual];
    for pass in 0..8_usize {
        let mut partition = 0_usize;
        while partition < partitions {
            let count = words.min(partitions.saturating_sub(partition));
            if pass == 0 {
                let mut value = classbook.scalar(bits)?;
                for reverse in (0..count).rev() {
                    let index = partition.saturating_add(reverse);
                    if let Some(slot) = classes.get_mut(index) {
                        *slot = value.checked_rem(residue.classifications).unwrap_or(0);
                    }
                    value = value.checked_div(residue.classifications).unwrap_or(0);
                }
            }
            for offset_partition in 0..count {
                let absolute = partition.saturating_add(offset_partition);
                let class = classes.get(absolute).copied().unwrap_or(0);
                let book_index = residue
                    .books
                    .get(class)
                    .and_then(|row| row.get(pass))
                    .copied()
                    .flatten();
                if let Some(book_index) = book_index {
                    decode_partition(
                        &mut interleaved,
                        begin.saturating_add(absolute.saturating_mul(residue.partition)),
                        residue.partition,
                        1,
                        books
                            .get(book_index)
                            .ok_or_else(|| Error::damaged("Vorbis residue-2 book"))?,
                        bits,
                    )?;
                }
            }
            partition = partition.saturating_add(count);
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

#[derive(Clone, Copy, Default)]
struct Complex {
    re: f32,
    im: f32,
}

impl Complex {
    fn mul(self, other: Self) -> Self {
        Self {
            re: self.re * other.re - self.im * other.im,
            im: self.re * other.im + self.im * other.re,
        }
    }

    fn add(self, other: Self) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }

    fn sub(self, other: Self) -> Self {
        Self {
            re: self.re - other.re,
            im: self.im - other.im,
        }
    }
}

fn fft(values: &mut [Complex]) -> Result<()> {
    let n = values.len();
    if !n.is_power_of_two() || n == 0 {
        return Err(Error::damaged("Vorbis FFT size is not a power of two"));
    }
    let mut j = 0_usize;
    for i in 1..n {
        let mut bit = n.wrapping_shr(1);
        while j & bit != 0 {
            j ^= bit;
            bit = bit.wrapping_shr(1);
        }
        j ^= bit;
        if i < j {
            values.swap(i, j);
        }
    }
    let mut length = 2_usize;
    while length <= n {
        let angle = -2.0 * PI / (length as f32);
        let root = Complex {
            re: angle.cos(),
            im: angle.sin(),
        };
        let half = length.saturating_div(2);
        let mut base = 0_usize;
        while base < n {
            let mut factor = Complex { re: 1.0, im: 0.0 };
            for offset in 0..half {
                let left_index = base.saturating_add(offset);
                let right_index = left_index.saturating_add(half);
                let left = values.get(left_index).copied().unwrap_or_default();
                let right = values.get(right_index).copied().unwrap_or_default().mul(factor);
                if let Some(slot) = values.get_mut(left_index) {
                    *slot = left.add(right);
                }
                if let Some(slot) = values.get_mut(right_index) {
                    *slot = left.sub(right);
                }
                factor = factor.mul(root);
            }
            base = base.saturating_add(length);
        }
        length = length.saturating_mul(2);
    }
    Ok(())
}

fn dct4(input: &[f32]) -> Result<Vec<f32>> {
    let n = input.len();
    if n == 0 || !n.is_power_of_two() {
        return Err(Error::damaged("Vorbis DCT-IV size"));
    }
    let fft_len = n
        .checked_mul(2)
        .ok_or_else(|| Error::damaged("Vorbis DCT-IV size overflow"))?;
    let mut work = vec![Complex::default(); fft_len];
    for (index, value) in input.iter().copied().enumerate() {
        let angle = -PI * (index as f32) / (2.0 * n as f32);
        if let Some(slot) = work.get_mut(index) {
            *slot = Complex {
                re: value * angle.cos(),
                im: value * angle.sin(),
            };
        }
    }
    fft(&mut work)?;
    let mut output = Vec::with_capacity(n);
    for index in 0..n {
        let angle = -PI * ((index.saturating_mul(2).saturating_add(1)) as f32) / (4.0 * n as f32);
        let rotation = Complex {
            re: angle.cos(),
            im: angle.sin(),
        };
        output.push(work.get(index).copied().unwrap_or_default().mul(rotation).re);
    }
    Ok(output)
}

fn imdct(spectrum: &[f32]) -> Result<Vec<f32>> {
    let m = spectrum.len();
    if m == 0 || m % 2 != 0 {
        return Err(Error::damaged("Vorbis IMDCT spectrum size"));
    }
    let transformed = dct4(spectrum)?;
    let quarter = m.saturating_div(2);
    let mut output = Vec::with_capacity(m.saturating_mul(2));
    for index in quarter..m {
        output.push(transformed.get(index).copied().unwrap_or(0.0));
    }
    for index in (0..m).rev() {
        output.push(-transformed.get(index).copied().unwrap_or(0.0));
    }
    for index in 0..quarter {
        output.push(-transformed.get(index).copied().unwrap_or(0.0));
    }
    Ok(output)
}

fn window(n: usize, small: usize, long: bool, previous_long: bool, next_long: bool) -> Vec<f32> {
    let center = n.saturating_div(2);
    let (left_start, left_end, left_n) = if long && !previous_long {
        (
            n.saturating_div(4).saturating_sub(small.saturating_div(4)),
            n.saturating_div(4).saturating_add(small.saturating_div(4)),
            small.saturating_div(2),
        )
    } else {
        (0, center, center)
    };
    let three_quarters = n.saturating_mul(3).saturating_div(4);
    let (right_start, right_end, right_n) = if long && !next_long {
        (
            three_quarters.saturating_sub(small.saturating_div(4)),
            three_quarters.saturating_add(small.saturating_div(4)),
            small.saturating_div(2),
        )
    } else {
        (center, n, center)
    };
    let mut result = vec![0_f32; n];
    for index in left_start..left_end {
        let phase = ((index.saturating_sub(left_start)) as f32 + 0.5) / (left_n.max(1) as f32) * (PI / 2.0);
        let sine = phase.sin();
        if let Some(slot) = result.get_mut(index) {
            *slot = (PI / 2.0 * sine * sine).sin();
        }
    }
    for slot in result.iter_mut().take(right_start).skip(left_end) {
        *slot = 1.0;
    }
    for index in right_start..right_end.min(n) {
        let phase =
            ((index.saturating_sub(right_start)) as f32 + 0.5) / (right_n.max(1) as f32) * (PI / 2.0) + PI / 2.0;
        let sine = phase.sin();
        if let Some(slot) = result.get_mut(index) {
            *slot = (PI / 2.0 * sine * sine).sin();
        }
    }
    result
}

struct AudioBlock {
    channels: Vec<Vec<f32>>,
    size: usize,
}

fn decode_audio(packet: &[u8], ident: Ident, setup: &Setup) -> Result<AudioBlock> {
    let mut bits = Bits::new(packet);
    if bits.flag()? {
        return Err(Error::damaged("Vorbis header packet in audio stream"));
    }
    let mode_bits = ilog(setup.modes.len().saturating_sub(1));
    let mode_index = usize::try_from(bits.read(mode_bits)?).unwrap_or(usize::MAX);
    let mode = setup
        .modes
        .get(mode_index)
        .copied()
        .ok_or_else(|| Error::damaged("Vorbis mode out of range"))?;
    let n = if mode.long { ident.large } else { ident.small };
    let previous_long = if mode.long { bits.flag()? } else { false };
    let next_long = if mode.long { bits.flag()? } else { false };
    let mapping = setup
        .mappings
        .get(mode.mapping)
        .ok_or_else(|| Error::damaged("Vorbis mapping out of range"))?;

    let channel_count = usize::from(ident.channels);
    let mut floors = Vec::with_capacity(channel_count);
    let mut no_residue = Vec::with_capacity(channel_count);
    for channel in 0..channel_count {
        let submap = mapping.mux.get(channel).copied().unwrap_or(0);
        let floor_index = mapping
            .floors
            .get(submap)
            .copied()
            .ok_or_else(|| Error::damaged("Vorbis floor mapping"))?;
        let decoded = decode_floor(
            setup
                .floors
                .get(floor_index)
                .ok_or_else(|| Error::damaged("Vorbis floor"))?,
            &setup.books,
            &mut bits,
        )?;
        no_residue.push(decoded.is_none());
        floors.push(decoded);
    }
    for (magnitude, angle) in &mapping.coupling {
        if !no_residue.get(*magnitude).copied().unwrap_or(true) || !no_residue.get(*angle).copied().unwrap_or(true) {
            if let Some(slot) = no_residue.get_mut(*magnitude) {
                *slot = false;
            }
            if let Some(slot) = no_residue.get_mut(*angle) {
                *slot = false;
            }
        }
    }

    let spectral_len = n.saturating_div(2);
    let mut spectra = vec![vec![0_f32; spectral_len]; channel_count];
    for submap in 0..mapping.submaps {
        let selected: Vec<usize> = (0..channel_count)
            .filter(|channel| mapping.mux.get(*channel).copied().unwrap_or(0) == submap)
            .collect();
        if selected.is_empty() {
            continue;
        }
        let residue_index = mapping
            .residues
            .get(submap)
            .copied()
            .ok_or_else(|| Error::damaged("Vorbis residue mapping"))?;
        let residue = setup
            .residues
            .get(residue_index)
            .ok_or_else(|| Error::damaged("Vorbis residue"))?;
        let mut temporary = vec![vec![0_f32; spectral_len]; selected.len()];
        let temporary_skip: Vec<bool> = selected
            .iter()
            .map(|channel| no_residue.get(*channel).copied().unwrap_or(true))
            .collect();
        if residue.kind == 2 {
            decode_residue_type2(
                residue,
                &setup.books,
                &mut bits,
                &mut temporary,
                &temporary_skip,
                spectral_len,
            )?;
        } else {
            decode_residue_channels(
                residue,
                &setup.books,
                &mut bits,
                &mut temporary,
                &temporary_skip,
                spectral_len,
            )?;
        }
        for (local, channel) in selected.iter().copied().enumerate() {
            if let (Some(source), Some(target)) = (temporary.get(local), spectra.get_mut(channel)) {
                target.copy_from_slice(source);
            }
        }
    }

    for (magnitude, angle) in mapping.coupling.iter().copied().rev() {
        for sample in 0..spectral_len {
            let m = spectra
                .get(magnitude)
                .and_then(|values| values.get(sample))
                .copied()
                .unwrap_or(0.0);
            let a = spectra
                .get(angle)
                .and_then(|values| values.get(sample))
                .copied()
                .unwrap_or(0.0);
            let (new_m, new_a) = if m > 0.0 {
                if a > 0.0 {
                    (m, m - a)
                } else {
                    (m + a, m)
                }
            } else if a > 0.0 {
                (m, m + a)
            } else {
                (m - a, m)
            };
            if let Some(slot) = spectra.get_mut(magnitude).and_then(|values| values.get_mut(sample)) {
                *slot = new_m;
            }
            if let Some(slot) = spectra.get_mut(angle).and_then(|values| values.get_mut(sample)) {
                *slot = new_a;
            }
        }
    }

    let win = window(n, ident.small, mode.long, previous_long, next_long);
    let mut channels = Vec::with_capacity(channel_count);
    for channel in 0..channel_count {
        if let Some(floor_packet) = floors.get(channel).and_then(Option::as_ref) {
            let submap = mapping.mux.get(channel).copied().unwrap_or(0);
            let floor_index = mapping.floors.get(submap).copied().unwrap_or(0);
            let curve = floor_curve(
                setup
                    .floors
                    .get(floor_index)
                    .ok_or_else(|| Error::damaged("Vorbis floor synthesis"))?,
                floor_packet,
                spectral_len,
            );
            if let Some(spectrum) = spectra.get_mut(channel) {
                for sample in 0..spectral_len {
                    if let Some(slot) = spectrum.get_mut(sample) {
                        *slot *= curve.get(sample).copied().unwrap_or(0.0);
                    }
                }
            }
        } else if let Some(spectrum) = spectra.get_mut(channel) {
            spectrum.fill(0.0);
        }
        let mut time = imdct(spectra.get(channel).ok_or_else(|| Error::damaged("Vorbis spectrum"))?)?;
        for (sample, factor) in time.iter_mut().zip(win.iter().copied()) {
            *sample *= factor;
        }
        channels.push(time);
    }
    Ok(AudioBlock { channels, size: n })
}

fn overlap(previous: &AudioBlock, current: &AudioBlock, output: &mut Vec<f32>) {
    let count = previous
        .size
        .saturating_div(4)
        .saturating_add(current.size.saturating_div(4));
    let channels = previous.channels.len().min(current.channels.len());
    let current_shift = current
        .size
        .saturating_div(4)
        .saturating_sub(previous.size.saturating_div(4));
    for frame in 0..count {
        for channel in 0..channels {
            let previous_index = previous.size.saturating_div(2).saturating_add(frame);
            let previous_value = previous
                .channels
                .get(channel)
                .and_then(|values| values.get(previous_index))
                .copied()
                .unwrap_or(0.0);
            let signed_current = (frame as isize)
                .saturating_add(current.size.saturating_div(4) as isize)
                .saturating_sub(previous.size.saturating_div(4) as isize);
            let current_value = if signed_current < 0 {
                0.0
            } else {
                current
                    .channels
                    .get(channel)
                    .and_then(|values| values.get(usize::try_from(signed_current).unwrap_or(usize::MAX)))
                    .copied()
                    .unwrap_or(0.0)
            };
            let _ = current_shift;
            output.push(previous_value + current_value);
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
fn to_i16(value: f32) -> i16 {
    let scaled = (value * 32768.0).round().clamp(-32768.0, 32767.0);
    scaled as i16
}

/// Decodes a complete single-stream Ogg/Vorbis-I file to interleaved PCM.
///
/// Floor type 1, residue types 0/1/2, mapping type 0, channel coupling and
/// both Vorbis block sizes are supported. Floor type 0 is explicitly refused.
pub fn decode(input: &[u8]) -> Result<Pcm> {
    let packets = ogg::packets(input)?;
    if packets.len() < 4 {
        return Err(Error::damaged("Vorbis stream has no audio packets"));
    }
    let ident = identification(
        &packets
            .first()
            .ok_or_else(|| Error::damaged("missing Vorbis identification"))?
            .data,
    )?;
    validate_comment(
        &packets
            .get(1)
            .ok_or_else(|| Error::damaged("missing Vorbis comments"))?
            .data,
    )?;
    let setup = setup(
        &packets
            .get(2)
            .ok_or_else(|| Error::damaged("missing Vorbis setup"))?
            .data,
        ident,
    )?;

    let mut previous = None;
    let mut pcm = Vec::<f32>::new();
    let mut initial_center = None;
    let mut final_granule = None;
    for packet in packets.iter().skip(3) {
        let block = decode_audio(&packet.data, ident, &setup)?;
        if initial_center.is_none() {
            initial_center = Some(block.size.saturating_div(2));
        }
        if let Some(old) = previous.as_ref() {
            overlap(old, &block, &mut pcm);
        }
        previous = Some(block);
        if let Some(granule) = packet.granule {
            final_granule = Some(granule);
        }
    }

    if let (Some(granule), Some(center)) = (final_granule, initial_center) {
        let frames = usize::try_from(granule).unwrap_or(usize::MAX).saturating_sub(center);
        let samples = frames
            .checked_mul(usize::from(ident.channels))
            .ok_or_else(|| Error::damaged("Vorbis PCM size overflow"))?;
        pcm.truncate(samples);
    }
    Ok(Pcm {
        channels: ident.channels,
        rate: ident.rate,
        samples: pcm.into_iter().map(to_i16).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(text: &str) -> Vec<u8> {
        fn value(byte: u8) -> Option<u8> {
            match byte {
                b'A'..=b'Z' => Some(byte.saturating_sub(b'A')),
                b'a'..=b'z' => Some(byte.saturating_sub(b'a').saturating_add(26)),
                b'0'..=b'9' => Some(byte.saturating_sub(b'0').saturating_add(52)),
                b'+' => Some(62),
                b'/' => Some(63),
                _ => None,
            }
        }
        let clean: Vec<u8> = text.bytes().filter(|byte| !byte.is_ascii_whitespace()).collect();
        let mut out = Vec::new();
        for chunk in clean.chunks(4) {
            let a = chunk.first().copied().and_then(value).unwrap_or(0);
            let b = chunk.get(1).copied().and_then(value).unwrap_or(0);
            let c = chunk.get(2).copied().and_then(value);
            let d = chunk.get(3).copied().and_then(value);
            out.push(a.wrapping_shl(2) | b.wrapping_shr(4));
            if let Some(c) = c {
                out.push(b.wrapping_shl(4) | c.wrapping_shr(2));
                if let Some(d) = d {
                    out.push(c.wrapping_shl(6) | d);
                }
            }
        }
        out
    }

    fn fnv64(samples: &[i16]) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for sample in samples {
            for byte in sample.to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        hash
    }

    #[test]
    fn menu_decline_matches_reference_pcm() {
        let data = b64(include_str!("../tests/data/menu_decline.ogg.b64"));
        let pcm = decode(&data).unwrap_or_else(|error| panic!("decode menu_decline: {error:?}"));
        assert_eq!(pcm.channels, 1);
        assert_eq!(pcm.rate, 48_000);
        assert_eq!(pcm.samples.len(), 23_348);
        assert_eq!(fnv64(&pcm.samples), 0x2528_3d14_5393_c466);
    }

    #[test]
    fn menu_select_stages_are_decodable() {
        let data = b64(include_str!("../tests/data/menu_select.ogg.b64"));
        let packets = ogg::packets(&data).unwrap_or_else(|error| panic!("ogg: {error:?}"));
        let ident = identification(&packets.first().unwrap_or_else(|| panic!("ident packet")).data)
            .unwrap_or_else(|error| panic!("ident: {error:?}"));
        validate_comment(&packets.get(1).unwrap_or_else(|| panic!("comment packet")).data)
            .unwrap_or_else(|error| panic!("comment: {error:?}"));
        let setup = setup(&packets.get(2).unwrap_or_else(|| panic!("setup packet")).data, ident)
            .unwrap_or_else(|error| panic!("setup: {error:?}"));
        for (index, packet) in packets.iter().skip(3).enumerate() {
            let _ = decode_audio(&packet.data, ident, &setup)
                .unwrap_or_else(|error| panic!("audio packet {index}: {error:?}"));
        }
    }

    #[test]
    fn menu_select_matches_reference_pcm() {
        let data = b64(include_str!("../tests/data/menu_select.ogg.b64"));
        let pcm = decode(&data).unwrap_or_else(|error| panic!("decode menu_select: {error:?}"));
        assert_eq!(pcm.channels, 1);
        assert_eq!(pcm.rate, 48_000);
        assert_eq!(pcm.samples.len(), 23_487);
        eprintln!(
            "x10 select first64={:?} min={:?} max={:?}",
            pcm.samples.get(..64).unwrap_or(&pcm.samples),
            pcm.samples.iter().min(),
            pcm.samples.iter().max()
        );
        assert_eq!(fnv64(&pcm.samples), 0x388f_979d_1a42_93bb);
    }

    #[test]
    fn menu_switch_matches_reference_pcm() {
        let data = b64(include_str!("../tests/data/menu_switch.ogg.b64"));
        let pcm = decode(&data).unwrap_or_else(|error| panic!("decode menu_switch: {error:?}"));
        assert_eq!(pcm.channels, 1);
        assert_eq!(pcm.rate, 48_000);
        assert_eq!(pcm.samples.len(), 23_348);
        assert_eq!(fnv64(&pcm.samples), 0x2d39_9db9_d105_17a4);
    }

    #[test]
    fn lookup1_examples() {
        assert_eq!(lookup1_values(625, 4), 5);
        assert_eq!(lookup1_values(16, 2), 4);
    }

    #[test]
    fn dct4_matches_direct_small_case() {
        let input = [1.0_f32, -0.5, 0.25, 2.0];
        let fast = dct4(&input).unwrap_or_default();
        for k in 0..input.len() {
            let mut direct = 0.0_f32;
            for (n, value) in input.iter().copied().enumerate() {
                direct += value * (PI / input.len() as f32 * (n as f32 + 0.5) * (k as f32 + 0.5)).cos();
            }
            assert!((fast.get(k).copied().unwrap_or(0.0) - direct).abs() < 0.0001);
        }
    }
}
