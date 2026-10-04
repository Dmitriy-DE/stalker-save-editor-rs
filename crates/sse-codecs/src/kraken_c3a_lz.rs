use sse_core::{Error, Result};

use super::kraken_c3a_entropy::decode_owned;

const MAX_EXTENDED_LENGTHS: usize = 512;

#[derive(Debug)]
struct LzStreams {
    literals: Vec<u8>,
    commands: Vec<u8>,
    offsets: Vec<i32>,
    lengths: Vec<u32>,
}

pub(super) fn decode_lz_chunk(mode: u8, source: &[u8], output: &mut [u8], start: usize, end: usize) -> Result<()> {
    if mode != 1 {
        return Err(Error::Refused("independent Kraken LZ mode is not supported".to_owned()));
    }
    let output_capacity = end
        .checked_sub(start)
        .ok_or_else(|| Error::damaged("Kraken LZ output range is reversed"))?;
    let mut source_at = 0_usize;
    let mut destination = start;
    if start == 0 {
        let seed_end = start
            .checked_add(8)
            .ok_or_else(|| Error::damaged("Kraken LZ seed range overflow"))?;
        let seed = source
            .get(..8)
            .ok_or_else(|| Error::damaged("Kraken LZ seed is truncated"))?;
        output
            .get_mut(start..seed_end)
            .ok_or_else(|| Error::damaged("Kraken LZ seed exceeds output"))?
            .copy_from_slice(seed);
        destination = seed_end;
        source_at = 8;
    }
    if source.get(source_at).is_some_and(|byte| byte & 0x80 != 0) {
        return Err(Error::Refused(
            "independent Kraken excess-byte LZ mode is unsupported".to_owned(),
        ));
    }

    let (literals, used) = decode_owned(
        source
            .get(source_at..)
            .ok_or_else(|| Error::damaged("Kraken literal table begins outside payload"))?,
        output_capacity,
        1,
    )?;
    source_at = source_at
        .checked_add(used)
        .ok_or_else(|| Error::damaged("Kraken literal table position overflow"))?;
    let (commands, used) = decode_owned(
        source
            .get(source_at..)
            .ok_or_else(|| Error::damaged("Kraken command table begins outside payload"))?,
        output_capacity,
        1,
    )?;
    source_at = source_at
        .checked_add(used)
        .ok_or_else(|| Error::damaged("Kraken command table position overflow"))?;

    let mut scale = 0_u8;
    let mut extra_offsets = None;
    let packed_offsets;
    if source.get(source_at).is_some_and(|byte| byte & 0x80 != 0) {
        scale = source
            .get(source_at)
            .copied()
            .and_then(|byte| byte.checked_sub(127))
            .ok_or_else(|| Error::damaged("Kraken offset scale is invalid"))?;
        source_at = source_at
            .checked_add(1)
            .ok_or_else(|| Error::damaged("Kraken offset scale position overflow"))?;
        let (offsets, used) = decode_owned(
            source
                .get(source_at..)
                .ok_or_else(|| Error::damaged("Kraken packed offsets begin outside payload"))?,
            commands.len(),
            1,
        )?;
        source_at = source_at
            .checked_add(used)
            .ok_or_else(|| Error::damaged("Kraken packed offsets position overflow"))?;
        if scale != 1 {
            let (extra, used) = decode_owned(
                source
                    .get(source_at..)
                    .ok_or_else(|| Error::damaged("Kraken extra offsets begin outside payload"))?,
                offsets.len(),
                1,
            )?;
            if extra.len() != offsets.len() {
                return Err(Error::damaged("Kraken extra-offset count does not match offsets"));
            }
            source_at = source_at
                .checked_add(used)
                .ok_or_else(|| Error::damaged("Kraken extra-offset position overflow"))?;
            extra_offsets = Some(extra);
        }
        packed_offsets = offsets;
    } else {
        let (offsets, used) = decode_owned(
            source
                .get(source_at..)
                .ok_or_else(|| Error::damaged("Kraken packed offsets begin outside payload"))?,
            commands.len(),
            1,
        )?;
        source_at = source_at
            .checked_add(used)
            .ok_or_else(|| Error::damaged("Kraken packed offsets position overflow"))?;
        packed_offsets = offsets;
    }

    let (packed_lengths, used) = decode_owned(
        source
            .get(source_at..)
            .ok_or_else(|| Error::damaged("Kraken packed lengths begin outside payload"))?,
        output_capacity / 4,
        1,
    )?;
    source_at = source_at
        .checked_add(used)
        .ok_or_else(|| Error::damaged("Kraken packed lengths position overflow"))?;
    if source_at > source.len() {
        return Err(Error::damaged("Kraken offset bitstream begins beyond input"));
    }
    let (offsets, lengths) = expand_offsets(
        source.get(source_at..).unwrap_or_default(),
        &packed_offsets,
        extra_offsets.as_deref(),
        scale,
        &packed_lengths,
    )?;
    let streams = LzStreams {
        literals,
        commands,
        offsets,
        lengths,
    };
    process_commands(streams, output, end, destination)
}

struct OffsetBits<'a> {
    bytes: &'a [u8],
    position: i64,
    bits: u32,
    bit_position: i32,
    backwards: bool,
}

impl<'a> OffsetBits<'a> {
    fn forward(bytes: &'a [u8]) -> Result<Self> {
        let mut reader = Self {
            bytes,
            position: 0,
            bits: 0,
            bit_position: 24,
            backwards: false,
        };
        reader.refill()?;
        Ok(reader)
    }

    fn backward(bytes: &'a [u8]) -> Result<Self> {
        let position = i64::try_from(bytes.len()).map_err(|_| Error::damaged("offset stream too large"))?;
        let mut reader = Self {
            bytes,
            position,
            bits: 0,
            bit_position: 24,
            backwards: true,
        };
        reader.refill()?;
        Ok(reader)
    }

    fn refill(&mut self) -> Result<()> {
        if self.bit_position > 24 {
            return Err(Error::damaged("offset bit accumulator is overfull"));
        }
        while self.bit_position > 0 {
            let byte = if self.backwards {
                self.position = self
                    .position
                    .checked_sub(1)
                    .ok_or_else(|| Error::damaged("reverse offset cursor underflow"))?;
                usize::try_from(self.position)
                    .ok()
                    .and_then(|index| self.bytes.get(index))
                    .copied()
                    .unwrap_or_default()
            } else {
                let byte = usize::try_from(self.position)
                    .ok()
                    .and_then(|index| self.bytes.get(index))
                    .copied()
                    .unwrap_or_default();
                self.position = self
                    .position
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("forward offset cursor overflow"))?;
                byte
            };
            let shift =
                u32::try_from(self.bit_position).map_err(|_| Error::damaged("offset shift conversion failed"))?;
            self.bits |= u32::from(byte)
                .checked_shl(shift)
                .ok_or_else(|| Error::damaged("offset accumulator shift overflow"))?;
            self.bit_position = self
                .bit_position
                .checked_sub(8)
                .ok_or_else(|| Error::damaged("offset refill position underflow"))?;
        }
        Ok(())
    }

    fn read(&mut self, count: u32) -> Result<u32> {
        if count > 24 {
            return Err(Error::damaged("offset bit read exceeds 24 bits"));
        }
        if count == 0 {
            return Ok(0);
        }
        let shift = 32_u32
            .checked_sub(count)
            .ok_or_else(|| Error::damaged("offset read shift underflow"))?;
        let value = self.bits >> shift;
        self.bits = self
            .bits
            .checked_shl(count)
            .ok_or_else(|| Error::damaged("offset accumulator shift overflow"))?;
        self.bit_position = self
            .bit_position
            .checked_add(i32::try_from(count).map_err(|_| Error::damaged("offset bit count conversion failed"))?)
            .ok_or_else(|| Error::damaged("offset bit position overflow"))?;
        Ok(value)
    }

    fn read_wide(&mut self, count: u32) -> Result<u32> {
        if count <= 24 {
            let value = self.read(count)?;
            self.refill()?;
            return Ok(value);
        }
        if count > 31 {
            return Err(Error::damaged("wide offset field exceeds 31 bits"));
        }
        let tail_bits = count
            .checked_sub(24)
            .ok_or_else(|| Error::damaged("wide offset tail width underflow"))?;
        let high = self
            .read(24)?
            .checked_shl(tail_bits)
            .ok_or_else(|| Error::damaged("wide offset high part overflow"))?;
        self.refill()?;
        let low = self.read(tail_bits)?;
        self.refill()?;
        Ok(high | low)
    }

    fn distance(&mut self, symbol: u8) -> Result<u32> {
        let width = if symbol < 0xf0 {
            u32::from(symbol >> 4).saturating_add(4)
        } else {
            u32::from(symbol.saturating_sub(0xf0)).saturating_add(4)
        };
        if width >= 31 {
            return Err(Error::damaged("Kraken match-distance code is too wide"));
        }
        let rotated = (self.bits | 1).rotate_left(width);
        self.bit_position = self
            .bit_position
            .checked_add(i32::try_from(width).map_err(|_| Error::damaged("distance bit count conversion failed"))?)
            .ok_or_else(|| Error::damaged("distance bit position overflow"))?;
        let mask = (2_u32
            .checked_shl(width)
            .ok_or_else(|| Error::damaged("distance mask overflow"))?)
        .saturating_sub(1);
        self.bits = rotated & !mask;
        let mut distance = if symbol < 0xf0 {
            (rotated & mask)
                .checked_shl(4)
                .ok_or_else(|| Error::damaged("distance prefix overflow"))?
                .saturating_add(u32::from(symbol & 0x0f))
                .checked_sub(248)
                .ok_or_else(|| Error::damaged("distance prefix underflow"))?
        } else {
            8_322_816_u32.saturating_add(
                (rotated & mask)
                    .checked_shl(12)
                    .ok_or_else(|| Error::damaged("long distance prefix overflow"))?,
            )
        };
        self.refill()?;
        if symbol >= 0xf0 {
            distance = distance
                .checked_add(self.read(12)?)
                .ok_or_else(|| Error::damaged("long distance extension overflow"))?;
            self.refill()?;
        }
        Ok(distance)
    }

    fn extended_length(&mut self) -> Result<u32> {
        let zeros = self.bits.leading_zeros();
        if zeros > 12 {
            return Err(Error::damaged("Kraken extended length prefix is too long"));
        }
        self.bit_position = self
            .bit_position
            .checked_add(i32::try_from(zeros).map_err(|_| Error::damaged("length prefix conversion failed"))?)
            .ok_or_else(|| Error::damaged("length bit position overflow"))?;
        self.bits = self.bits.checked_shl(zeros).unwrap_or_default();
        self.refill()?;
        let width = zeros.saturating_add(7);
        let length = self
            .read(width)?
            .checked_sub(64)
            .ok_or_else(|| Error::damaged("extended length code underflow"))?;
        self.refill()?;
        Ok(length)
    }

    fn effective_position(&self) -> Result<i64> {
        let pending = i64::from((24_i32.saturating_sub(self.bit_position)).max(0) >> 3);
        if self.backwards {
            self.position
                .checked_add(pending)
                .ok_or_else(|| Error::damaged("reverse offset end overflow"))
        } else {
            self.position
                .checked_sub(pending)
                .ok_or_else(|| Error::damaged("forward offset end underflow"))
        }
    }
}

fn expand_offsets(
    source: &[u8],
    packed: &[u8],
    extra: Option<&[u8]>,
    scale: u8,
    packed_lengths: &[u8],
) -> Result<(Vec<i32>, Vec<u32>)> {
    let mut forward = OffsetBits::forward(source)?;
    let mut backward = OffsetBits::backward(source)?;
    if backward.bits < 0x2000 {
        return Err(Error::damaged("reverse Kraken offset stream is too short"));
    }
    let zeros = backward.bits.leading_zeros();
    if zeros > 31 {
        return Err(Error::damaged("invalid extended-length count prefix"));
    }
    backward.bit_position = backward
        .bit_position
        .checked_add(i32::try_from(zeros).map_err(|_| Error::damaged("length-count prefix conversion failed"))?)
        .ok_or_else(|| Error::damaged("length-count prefix position overflow"))?;
    backward.bits = backward.bits.checked_shl(zeros).unwrap_or_default();
    backward.refill()?;
    let width = zeros.saturating_add(1);
    let extended_count = backward
        .read(width)?
        .checked_sub(1)
        .ok_or_else(|| Error::damaged("extended-length count underflow"))?;
    backward.refill()?;
    let extended_count =
        usize::try_from(extended_count).map_err(|_| Error::damaged("extended-length count conversion failed"))?;
    if extended_count > MAX_EXTENDED_LENGTHS {
        return Err(Error::damaged("too many Kraken extended lengths"));
    }

    let mut offsets = Vec::with_capacity(packed.len());
    for (index, code) in packed.iter().copied().enumerate() {
        let reader = if index % 2 == 0 { &mut forward } else { &mut backward };
        let value = if scale == 0 {
            let distance = reader.distance(code)?;
            i32::try_from(distance)
                .ok()
                .and_then(i32::checked_neg)
                .ok_or_else(|| Error::damaged("Kraken distance does not fit a signed offset"))?
        } else {
            let width = u32::from(code >> 3);
            if width > 26 {
                return Err(Error::damaged("scaled Kraken distance width exceeds 26"));
            }
            let prefix = u32::from(8_u8.saturating_add(code & 7))
                .checked_shl(width)
                .ok_or_else(|| Error::damaged("scaled Kraken distance prefix overflow"))?;
            let distance = prefix | reader.read_wide(width)?;
            let base = 8_i64.saturating_sub(i64::from(distance));
            let scaled = if scale == 1 {
                base
            } else {
                let low = i64::from(
                    extra
                        .and_then(|values| values.get(index))
                        .copied()
                        .ok_or_else(|| Error::damaged("missing scaled Kraken distance suffix"))?,
                );
                base.saturating_mul(i64::from(scale)).saturating_sub(low)
            };
            i32::try_from(scaled).map_err(|_| Error::damaged("scaled Kraken distance exceeds i32"))?
        };
        offsets.push(value);
    }

    let mut extensions = Vec::with_capacity(extended_count);
    for index in 0..extended_count {
        let reader = if index % 2 == 0 { &mut forward } else { &mut backward };
        extensions.push(reader.extended_length()?);
    }
    if forward.effective_position()? != backward.effective_position()? {
        return Err(Error::damaged("forward and reverse Kraken offset streams do not meet"));
    }

    let mut extension_at = 0_usize;
    let mut lengths = Vec::with_capacity(packed_lengths.len());
    for code in packed_lengths {
        let base = if *code == u8::MAX {
            let value = *extensions
                .get(extension_at)
                .ok_or_else(|| Error::damaged("missing Kraken extended match length"))?;
            extension_at = extension_at.saturating_add(1);
            value
                .checked_add(255)
                .ok_or_else(|| Error::damaged("Kraken extended match length overflow"))?
        } else {
            u32::from(*code)
        };
        lengths.push(
            base.checked_add(3)
                .ok_or_else(|| Error::damaged("Kraken match length overflow"))?,
        );
    }
    if extension_at != extensions.len() {
        return Err(Error::damaged("unused Kraken extended match lengths"));
    }
    Ok((offsets, lengths))
}

fn process_commands(streams: LzStreams, output: &mut [u8], end: usize, mut destination: usize) -> Result<()> {
    let mut literal_at = 0_usize;
    let mut offset_at = 0_usize;
    let mut length_at = 0_usize;
    let mut recent = [0_i32; 7];
    recent[3..6].fill(-8);
    for command in &streams.commands {
        let mut literal_count = usize::from(command & 3);
        let offset_kind = usize::from(command >> 6);
        let short_match = usize::from((command >> 2) & 0x0f);
        if literal_count == 3 {
            literal_count = usize::try_from(
                *streams
                    .lengths
                    .get(length_at)
                    .ok_or_else(|| Error::damaged("missing extended Kraken literal count"))?,
            )
            .map_err(|_| Error::damaged("Kraken literal count conversion failed"))?;
            length_at = length_at.saturating_add(1);
        }

        let fresh_offset = *streams.offsets.get(offset_at).unwrap_or(&0);
        recent[6] = fresh_offset;
        copy_literals(
            &streams.literals,
            &mut literal_at,
            output,
            &mut destination,
            end,
            literal_count,
        )?;
        let selected_index = offset_kind.saturating_add(3);
        let offset = *recent
            .get(selected_index)
            .ok_or_else(|| Error::damaged("Kraken recent offset index outside queue"))?;
        if offset_kind.saturating_add(3) >= 4 {
            for slot in (4_usize..=offset_kind.saturating_add(3)).rev() {
                let previous = slot
                    .checked_sub(1)
                    .and_then(|index| recent.get(index).copied())
                    .ok_or_else(|| Error::damaged("Kraken recent-offset source slot missing"))?;
                *recent
                    .get_mut(slot)
                    .ok_or_else(|| Error::damaged("Kraken recent-offset destination slot missing"))? = previous;
            }
        }
        recent[3] = offset;
        if offset_kind == 3 {
            offset_at = offset_at.saturating_add(1);
        }

        let match_length = if short_match != 15 {
            short_match.saturating_add(2)
        } else {
            let extra = usize::try_from(
                *streams
                    .lengths
                    .get(length_at)
                    .ok_or_else(|| Error::damaged("missing extended Kraken match length"))?,
            )
            .map_err(|_| Error::damaged("Kraken match length conversion failed"))?;
            length_at = length_at.saturating_add(1);
            14_usize
                .checked_add(extra)
                .ok_or_else(|| Error::damaged("Kraken match length overflow"))?
        };
        copy_match(output, &mut destination, end, 0, offset, match_length)?;
    }

    if offset_at != streams.offsets.len() || length_at != streams.lengths.len() {
        return Err(Error::damaged(
            "Kraken LZ offset or length table was not fully consumed",
        ));
    }
    if streams.literals.len().saturating_sub(literal_at) != end.saturating_sub(destination) {
        return Err(Error::damaged(
            "Kraken final literal tail does not match output remainder",
        ));
    }
    let tail = end.saturating_sub(destination);
    copy_literals(&streams.literals, &mut literal_at, output, &mut destination, end, tail)?;
    if destination != end || literal_at != streams.literals.len() {
        return Err(Error::damaged("Kraken LZ command stream did not fill destination"));
    }
    Ok(())
}

fn copy_literals(
    literals: &[u8],
    literal_at: &mut usize,
    output: &mut [u8],
    destination: &mut usize,
    end: usize,
    count: usize,
) -> Result<()> {
    let source_end = literal_at
        .checked_add(count)
        .ok_or_else(|| Error::damaged("Kraken literal source end overflow"))?;
    let output_end = destination
        .checked_add(count)
        .ok_or_else(|| Error::damaged("Kraken literal output end overflow"))?;
    if output_end > end {
        return Err(Error::damaged("Kraken literal run exceeds its output block"));
    }
    let source = literals
        .get(*literal_at..source_end)
        .ok_or_else(|| Error::damaged("Kraken literal source is short"))?;
    output
        .get_mut(*destination..output_end)
        .ok_or_else(|| Error::damaged("Kraken literal destination is out of bounds"))?
        .copy_from_slice(source);
    *literal_at = source_end;
    *destination = output_end;
    Ok(())
}

fn copy_match(
    output: &mut [u8],
    destination: &mut usize,
    end: usize,
    history_start: usize,
    offset: i32,
    count: usize,
) -> Result<()> {
    let output_end = destination
        .checked_add(count)
        .ok_or_else(|| Error::damaged("Kraken match output end overflow"))?;
    if output_end > end {
        return Err(Error::damaged("Kraken match crosses its output block"));
    }
    for _ in 0..count {
        let source = i64::try_from(*destination)
            .map_err(|_| Error::damaged("Kraken match destination conversion failed"))?
            .checked_add(i64::from(offset))
            .ok_or_else(|| Error::damaged("Kraken match source overflow"))?;
        let source = usize::try_from(source).map_err(|_| Error::damaged("Kraken match points before history"))?;
        if source < history_start || source >= *destination {
            return Err(Error::damaged("Kraken match reference is outside decoded history"));
        }
        let byte = *output
            .get(source)
            .ok_or_else(|| Error::damaged("Kraken match source outside output"))?;
        *output
            .get_mut(*destination)
            .ok_or_else(|| Error::damaged("Kraken match destination outside output"))? = byte;
        *destination = destination
            .checked_add(1)
            .ok_or_else(|| Error::damaged("Kraken match destination overflow"))?;
    }
    Ok(())
}
