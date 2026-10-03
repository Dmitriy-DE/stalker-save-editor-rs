use sse_core::{Error, Result};

const MAX_ENTROPY_OUTPUT: usize = 0x80000;
const MAX_CODE_BITS: usize = 11;

#[derive(Clone, Copy)]
struct EntropyFrame {
    codec: u8,
    header_bytes: usize,
    source_bytes: usize,
    output_bytes: usize,
}

pub(super) fn decode(source: &[u8], output: &mut [u8], depth: usize) -> Result<usize> {
    if depth > 16 {
        return Err(Error::damaged("independent Kraken entropy nesting limit exceeded"));
    }
    let frame = parse_frame(source, output.len())?;
    if frame.output_bytes != output.len() {
        return Err(Error::damaged("independent entropy output length mismatch"));
    }
    let payload_end = frame
        .header_bytes
        .checked_add(frame.source_bytes)
        .ok_or_else(|| Error::damaged("independent entropy payload end overflow"))?;
    let payload = source
        .get(frame.header_bytes..payload_end)
        .ok_or_else(|| Error::damaged("independent entropy payload is truncated"))?;
    match frame.codec {
        0 => output.copy_from_slice(payload),
        2 => decode_huffman(payload, output, false)?,
        4 => decode_huffman(payload, output, true)?,
        codec => {
            return Err(Error::Refused(format!(
                "independent Kraken entropy codec {codec} is not implemented"
            )))
        }
    }
    frame
        .header_bytes
        .checked_add(frame.source_bytes)
        .ok_or_else(|| Error::damaged("independent entropy consumed size overflow"))
}

pub(super) fn decode_owned(source: &[u8], limit: usize, depth: usize) -> Result<(Vec<u8>, usize)> {
    let frame = parse_frame(source, limit)?;
    if frame.output_bytes > MAX_ENTROPY_OUTPUT {
        return Err(Error::Refused(
            "independent Kraken entropy scratch exceeds 512 KiB".to_owned(),
        ));
    }
    let mut output = vec![0_u8; frame.output_bytes];
    let used = decode(source, &mut output, depth)?;
    Ok((output, used))
}

fn parse_frame(source: &[u8], capacity: usize) -> Result<EntropyFrame> {
    let first = *source
        .first()
        .ok_or_else(|| Error::damaged("missing independent entropy header"))?;
    let codec = (first >> 4) & 7;
    if codec == 0 {
        let (header_bytes, size): (usize, usize) = if first & 0x80 != 0 {
            let pair = source
                .get(..2)
                .ok_or_else(|| Error::damaged("short compact raw entropy header"))?;
            let packed = u16::from_be_bytes(
                pair.try_into()
                    .map_err(|_| Error::damaged("compact raw entropy header size mismatch"))?,
            );
            (2, usize::from(packed & 0x0fff))
        } else {
            let triple = source
                .get(..3)
                .ok_or_else(|| Error::damaged("short extended raw entropy header"))?;
            let first = *triple
                .first()
                .ok_or_else(|| Error::damaged("raw entropy header is empty"))?;
            if first & 0x80 != 0 {
                return Err(Error::damaged("reserved raw entropy header bit is set"));
            }
            let second = *triple
                .get(1)
                .ok_or_else(|| Error::damaged("raw entropy header is short"))?;
            let third = *triple
                .get(2)
                .ok_or_else(|| Error::damaged("raw entropy header is short"))?;
            let packed = u32::from(first) << 16 | u32::from(second) << 8 | u32::from(third);
            (
                3_usize,
                usize::try_from(packed & 0x3ffff).map_err(|_| Error::damaged("raw entropy size conversion failed"))?,
            )
        };
        if size > capacity {
            return Err(Error::damaged("raw entropy output exceeds its bound"));
        }
        let end = header_bytes
            .checked_add(size)
            .ok_or_else(|| Error::damaged("raw entropy frame size overflow"))?;
        if end > source.len() {
            return Err(Error::damaged("raw entropy payload is truncated"));
        }
        return Ok(EntropyFrame {
            codec,
            header_bytes,
            source_bytes: size,
            output_bytes: size,
        });
    }
    if codec > 5 {
        return Err(Error::damaged("invalid independent entropy codec tag"));
    }

    let (header_bytes, source_bytes, output_bytes): (usize, usize, usize) = if first & 0x80 != 0 {
        let header = source
            .get(..3)
            .ok_or_else(|| Error::damaged("short compact entropy header"))?;
        let first = *header
            .first()
            .ok_or_else(|| Error::damaged("compact entropy header is empty"))?;
        let second = *header
            .get(1)
            .ok_or_else(|| Error::damaged("compact entropy header is short"))?;
        let third = *header
            .get(2)
            .ok_or_else(|| Error::damaged("compact entropy header is short"))?;
        let packed = u32::from(first) << 16 | u32::from(second) << 8 | u32::from(third);
        let encoded =
            usize::try_from(packed & 0x3ff).map_err(|_| Error::damaged("entropy input length conversion failed"))?;
        let extra = usize::try_from((packed >> 10) & 0x3ff)
            .map_err(|_| Error::damaged("entropy length delta conversion failed"))?;
        let decoded = encoded
            .checked_add(extra)
            .and_then(|size| size.checked_add(1))
            .ok_or_else(|| Error::damaged("compact entropy output length overflow"))?;
        (3_usize, encoded, decoded)
    } else {
        let header = source
            .get(..5)
            .ok_or_else(|| Error::damaged("short extended entropy header"))?;
        let packed_bytes = header
            .get(1..5)
            .ok_or_else(|| Error::damaged("extended entropy size header is short"))?;
        let packed = u32::from_be_bytes(
            packed_bytes
                .try_into()
                .map_err(|_| Error::damaged("extended entropy header size mismatch"))?,
        );
        let encoded = usize::try_from(packed & 0x3ffff)
            .map_err(|_| Error::damaged("extended entropy input length conversion failed"))?;
        let decoded_bits = ((packed >> 18) | (u32::from(first) << 14)) & 0x3ffff;
        let decoded = usize::try_from(decoded_bits)
            .ok()
            .and_then(|size| size.checked_add(1))
            .ok_or_else(|| Error::damaged("extended entropy output length overflow"))?;
        if encoded >= decoded {
            return Err(Error::damaged("compressed entropy is not smaller than decoded data"));
        }
        (5_usize, encoded, decoded)
    };
    if output_bytes > capacity {
        return Err(Error::damaged("compressed entropy output exceeds its bound"));
    }
    let end = header_bytes
        .checked_add(source_bytes)
        .ok_or_else(|| Error::damaged("compressed entropy frame size overflow"))?;
    if end > source.len() {
        return Err(Error::damaged("compressed entropy payload is truncated"));
    }
    Ok(EntropyFrame {
        codec,
        header_bytes,
        source_bytes,
        output_bytes,
    })
}

#[derive(Clone)]
struct MsbBits<'a> {
    bytes: &'a [u8],
    next: usize,
}

impl<'a> MsbBits<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, next: 0 }
    }

    fn bit(&mut self) -> Result<u32> {
        let byte = *self
            .bytes
            .get(self.next / 8)
            .ok_or_else(|| Error::damaged("independent Huffman header bitstream ended"))?;
        let shift = 7_u8.saturating_sub(u8::try_from(self.next % 8).unwrap_or_default());
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| Error::damaged("independent Huffman header position overflow"))?;
        Ok(u32::from((byte >> shift) & 1))
    }

    fn read(&mut self, count: u8) -> Result<u32> {
        if count > 32 {
            return Err(Error::damaged("independent Huffman field exceeds 32 bits"));
        }
        let mut value = 0_u32;
        for _ in 0..count {
            value = value.checked_shl(1).unwrap_or_default() | self.bit()?;
        }
        Ok(value)
    }

    fn peek_padded(&self, count: u8) -> u32 {
        let mut clone = self.clone();
        let mut value = 0_u32;
        for _ in 0..count {
            value <<= 1;
            if let Ok(bit) = clone.bit() {
                value |= bit;
            }
        }
        value
    }

    fn byte_position(&self) -> usize {
        self.next.saturating_add(7) / 8
    }
}

fn read_fluff(bits: &mut MsbBits<'_>, symbol_count: usize) -> Result<usize> {
    if symbol_count == 256 {
        return Ok(0);
    }
    let missing = 257_usize
        .checked_sub(symbol_count)
        .ok_or_else(|| Error::damaged("invalid independent Huffman symbol count"))?;
    let span = missing
        .min(symbol_count)
        .checked_mul(2)
        .ok_or_else(|| Error::damaged("independent Huffman gap span overflow"))?;
    if span == 0 {
        return Err(Error::damaged("empty independent Huffman gap span"));
    }
    let last = span
        .checked_sub(1)
        .ok_or_else(|| Error::damaged("Huffman gap span underflow"))?;
    let width = usize::BITS.saturating_sub(last.leading_zeros());
    let width = u8::try_from(width).map_err(|_| Error::damaged("Huffman gap bit width conversion failed"))?;
    if width == 0 || width > 31 {
        return Err(Error::damaged("Huffman gap field outside supported width"));
    }
    let value = bits.peek_padded(width);
    let range = 1_usize
        .checked_shl(u32::from(width))
        .ok_or_else(|| Error::damaged("Huffman gap range overflow"))?;
    let excess = range
        .checked_sub(span)
        .ok_or_else(|| Error::damaged("Huffman gap range underflow"))?;
    if usize::try_from(value >> 1).unwrap_or_default() >= excess {
        let result = usize::try_from(value)
            .ok()
            .and_then(|number| number.checked_sub(excess))
            .ok_or_else(|| Error::damaged("Huffman gap value underflow"))?;
        let _ = bits.read(width)?;
        Ok(result)
    } else {
        let result = usize::try_from(value >> 1).unwrap_or_default();
        let short_width = width
            .checked_sub(1)
            .ok_or_else(|| Error::damaged("Huffman short gap width underflow"))?;
        let _ = bits.read(short_width)?;
        Ok(result)
    }
}

fn decode_rice_lengths(bits: &mut MsbBits<'_>, count: usize) -> Result<Vec<u8>> {
    let mut values = Vec::with_capacity(count);
    while values.len() < count {
        let mut zeros = 0_u16;
        loop {
            if bits.bit()? != 0 {
                break;
            }
            zeros = zeros
                .checked_add(1)
                .ok_or_else(|| Error::damaged("independent Rice unary run overflow"))?;
            if zeros > u16::from(u8::MAX) {
                return Err(Error::damaged("independent Rice value exceeds a byte"));
            }
        }
        values.push(u8::try_from(zeros).map_err(|_| Error::damaged("Rice value conversion failed"))?);
    }
    Ok(values)
}

fn decode_huffman_lengths(bits: &mut MsbBits<'_>) -> Result<Vec<u8>> {
    let mode = bits.bit()?;
    if mode == 0 {
        return Err(Error::Refused(
            "legacy Kraken Huffman tables are not implemented".to_owned(),
        ));
    }
    if bits.bit()? != 0 {
        return Err(Error::damaged("reserved independent Huffman table mode"));
    }
    let forced = u8::try_from(bits.read(2)?).map_err(|_| Error::damaged("Huffman forced-bit conversion failed"))?;
    let symbol_count = usize::try_from(bits.read(8)?)
        .map_err(|_| Error::damaged("Huffman symbol count conversion failed"))?
        .saturating_add(1);
    let fluff = read_fluff(bits, symbol_count)?;
    let total = symbol_count
        .checked_add(fluff)
        .ok_or_else(|| Error::damaged("Huffman Rice entry count overflow"))?;
    if total > 512 {
        return Err(Error::damaged("independent Huffman table exceeds 512 Rice entries"));
    }

    let mut packed_lengths = decode_rice_lengths(bits, total)?;
    for length in packed_lengths.iter_mut().take(symbol_count) {
        let suffix = bits.read(forced)?;
        let combined = u32::from(*length).checked_shl(u32::from(forced)).unwrap_or_default() | suffix;
        *length = u8::try_from(combined).map_err(|_| Error::damaged("Huffman packed code length exceeds byte"))?;
    }

    let mut running = 30_i32;
    for length in packed_lengths.iter_mut().take(symbol_count) {
        let raw = i32::from(*length);
        let sign = (raw & 1)
            .checked_neg()
            .ok_or_else(|| Error::damaged("Huffman signed delta overflow"))?;
        let delta = (raw >> 1) ^ sign;
        let decoded = delta.saturating_add(running >> 2).saturating_add(1);
        if !(1..=i32::try_from(MAX_CODE_BITS).unwrap_or_default()).contains(&decoded) {
            return Err(Error::damaged("independent Huffman code length is outside 1..11"));
        }
        *length = u8::try_from(decoded).map_err(|_| Error::damaged("Huffman code length conversion failed"))?;
        running = running.saturating_add(delta);
    }

    let gaps = packed_lengths
        .get(symbol_count..)
        .ok_or_else(|| Error::damaged("independent Huffman gap lengths are missing"))?;
    let ranges = read_symbol_ranges(bits, symbol_count, fluff, gaps)?;
    let mut by_symbol = vec![0_u8; 256];
    let mut length_at = 0_usize;
    for (start, count) in ranges {
        let end = start
            .checked_add(count)
            .ok_or_else(|| Error::damaged("Huffman symbol range overflow"))?;
        if end > by_symbol.len() {
            return Err(Error::damaged("Huffman symbol range exceeds alphabet"));
        }
        for symbol in start..end {
            let length = *packed_lengths
                .get(length_at)
                .ok_or_else(|| Error::damaged("Huffman code length list ended early"))?;
            *by_symbol
                .get_mut(symbol)
                .ok_or_else(|| Error::damaged("Huffman symbol index outside alphabet"))? = length;
            length_at = length_at.saturating_add(1);
        }
    }
    if length_at != symbol_count {
        return Err(Error::damaged("independent Huffman symbols are not fully covered"));
    }
    Ok(by_symbol)
}

fn read_symbol_ranges(
    bits: &mut MsbBits<'_>,
    symbol_count: usize,
    fluff: usize,
    gap_lengths: &[u8],
) -> Result<Vec<(usize, usize)>> {
    let range_count = fluff >> 1;
    let mut range_at = 0_usize;
    let mut symbol_at = 0_usize;
    if fluff & 1 != 0 {
        let width = u32::from(
            *gap_lengths
                .get(range_at)
                .ok_or_else(|| Error::damaged("missing first symbol gap"))?,
        );
        range_at = range_at.saturating_add(1);
        if width >= 8 {
            return Err(Error::damaged("Huffman first gap width is too large"));
        }
        let field_bits = u8::try_from(width + 1).map_err(|_| Error::damaged("Huffman gap width conversion failed"))?;
        let base = 1_usize
            .checked_shl(u32::from(field_bits))
            .unwrap_or_default()
            .saturating_sub(1);
        symbol_at = usize::try_from(bits.read(field_bits)?)
            .map_err(|_| Error::damaged("Huffman first gap conversion failed"))?
            .saturating_add(base);
    }

    let mut ranges = Vec::with_capacity(range_count.saturating_add(1));
    let mut used = 0_usize;
    for _ in 0..range_count {
        let count_width = u32::from(
            *gap_lengths
                .get(range_at)
                .ok_or_else(|| Error::damaged("missing Huffman range count width"))?,
        );
        range_at = range_at.saturating_add(1);
        if count_width >= 9 {
            return Err(Error::damaged("Huffman range count width is too large"));
        }
        let count_bits =
            u8::try_from(count_width).map_err(|_| Error::damaged("Huffman range count conversion failed"))?;
        let count = usize::try_from(bits.read(count_bits)?)
            .map_err(|_| Error::damaged("Huffman range count conversion failed"))?
            .saturating_add(1_usize.checked_shl(count_width).unwrap_or_default());
        let space_width = u32::from(
            *gap_lengths
                .get(range_at)
                .ok_or_else(|| Error::damaged("missing Huffman range gap width"))?,
        );
        range_at = range_at.saturating_add(1);
        if space_width >= 8 {
            return Err(Error::damaged("Huffman range gap width is too large"));
        }
        let space_bits =
            u8::try_from(space_width + 1).map_err(|_| Error::damaged("Huffman range gap conversion failed"))?;
        let space = usize::try_from(bits.read(space_bits)?)
            .map_err(|_| Error::damaged("Huffman range gap conversion failed"))?
            .saturating_add(
                1_usize
                    .checked_shl(u32::from(space_bits))
                    .unwrap_or_default()
                    .saturating_sub(1),
            );
        ranges.push((symbol_at, count));
        used = used
            .checked_add(count)
            .ok_or_else(|| Error::damaged("Huffman used symbol count overflow"))?;
        symbol_at = symbol_at
            .checked_add(count)
            .and_then(|index| index.checked_add(space))
            .ok_or_else(|| Error::damaged("Huffman next symbol position overflow"))?;
    }
    if used >= symbol_count || symbol_at > 256 {
        return Err(Error::damaged("invalid independent Huffman symbol ranges"));
    }
    let final_count = symbol_count
        .checked_sub(used)
        .ok_or_else(|| Error::damaged("Huffman final range count underflow"))?;
    ranges.push((symbol_at, final_count));
    Ok(ranges)
}

struct HuffCodebook {
    decode: Vec<Vec<Option<u8>>>,
    only_symbol: Option<u8>,
}

fn build_codebook(lengths: &[u8]) -> Result<HuffCodebook> {
    let mut buckets: [Vec<u8>; MAX_CODE_BITS + 1] = std::array::from_fn(|_| Vec::new());
    for (symbol, length) in lengths.iter().copied().enumerate() {
        if length == 0 {
            continue;
        }
        let bucket = buckets
            .get_mut(usize::from(length))
            .ok_or_else(|| Error::damaged("Huffman code length exceeds the supported table"))?;
        bucket.push(u8::try_from(symbol).map_err(|_| Error::damaged("Huffman symbol exceeds a byte"))?);
    }
    let active = buckets.iter().map(Vec::len).sum::<usize>();
    if active == 0 {
        return Err(Error::damaged("empty independent Huffman codebook"));
    }
    if active == 1 {
        let symbol = buckets
            .iter()
            .find_map(|bucket| bucket.first().copied())
            .ok_or_else(|| Error::damaged("single Huffman symbol is missing"))?;
        return Ok(HuffCodebook {
            decode: vec![Vec::new(); MAX_CODE_BITS + 1],
            only_symbol: Some(symbol),
        });
    }

    let mut table: Vec<Vec<Option<u8>>> = (0..=MAX_CODE_BITS)
        .map(|length| {
            vec![
                None;
                1_usize
                    .checked_shl(u32::try_from(length).unwrap_or_default())
                    .unwrap_or_default()
            ]
        })
        .collect();
    let mut next_code = 0_u32;
    for length in 1..=MAX_CODE_BITS {
        next_code = next_code
            .checked_shl(1)
            .ok_or_else(|| Error::damaged("canonical Huffman code overflow"))?;
        let limit = 1_u32
            .checked_shl(u32::try_from(length).unwrap_or_default())
            .ok_or_else(|| Error::damaged("canonical Huffman limit overflow"))?;
        let bucket = buckets
            .get(length)
            .ok_or_else(|| Error::damaged("missing canonical Huffman bucket"))?;
        if next_code
            .checked_add(u32::try_from(bucket.len()).map_err(|_| Error::damaged("Huffman bucket too large"))?)
            .is_none_or(|end| end > limit)
        {
            return Err(Error::damaged("oversubscribed independent Huffman code lengths"));
        }
        for symbol in bucket {
            let reversed = reverse_low_bits(next_code, length)?;
            let slot = table
                .get_mut(length)
                .and_then(|slots| slots.get_mut(usize::try_from(reversed).unwrap_or_default()))
                .ok_or_else(|| Error::damaged("canonical Huffman lookup slot outside table"))?;
            *slot = Some(*symbol);
            next_code = next_code.saturating_add(1);
        }
    }
    if next_code != 1_u32 << MAX_CODE_BITS {
        return Err(Error::damaged("independent Huffman tree is incomplete"));
    }
    Ok(HuffCodebook {
        decode: table,
        only_symbol: None,
    })
}

fn reverse_low_bits(value: u32, count: usize) -> Result<u32> {
    let width = u32::try_from(count).map_err(|_| Error::damaged("Huffman bit width conversion failed"))?;
    Ok(value.reverse_bits() >> (32_u32.saturating_sub(width)))
}

trait SymbolBits {
    fn read_symbol_bit(&mut self) -> Result<u32>;
    fn consumed_bytes(&self) -> usize;
}

struct ForwardSymbolBits<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> ForwardSymbolBits<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }
}

impl SymbolBits for ForwardSymbolBits<'_> {
    fn read_symbol_bit(&mut self) -> Result<u32> {
        let byte = *self
            .bytes
            .get(self.position / 8)
            .ok_or_else(|| Error::damaged("forward Huffman data stream ended"))?;
        let bit = (byte >> (self.position % 8)) & 1;
        self.position = self
            .position
            .checked_add(1)
            .ok_or_else(|| Error::damaged("forward Huffman data position overflow"))?;
        Ok(u32::from(bit))
    }

    fn consumed_bytes(&self) -> usize {
        self.position.saturating_add(7) / 8
    }
}

struct BackwardSymbolBits<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> BackwardSymbolBits<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }
}

impl SymbolBits for BackwardSymbolBits<'_> {
    fn read_symbol_bit(&mut self) -> Result<u32> {
        let bytes_from_end = self
            .position
            .checked_div(8)
            .and_then(|whole_bytes| whole_bytes.checked_add(1))
            .ok_or_else(|| Error::damaged("backward Huffman byte count overflow"))?;
        let byte_index = self
            .bytes
            .len()
            .checked_sub(bytes_from_end)
            .ok_or_else(|| Error::damaged("backward Huffman data stream ended"))?;
        let byte = *self
            .bytes
            .get(byte_index)
            .ok_or_else(|| Error::damaged("backward Huffman byte outside source"))?;
        let bit = (byte >> (self.position % 8)) & 1;
        self.position = self
            .position
            .checked_add(1)
            .ok_or_else(|| Error::damaged("backward Huffman data position overflow"))?;
        Ok(u32::from(bit))
    }

    fn consumed_bytes(&self) -> usize {
        self.position.saturating_add(7) / 8
    }
}

fn next_symbol(reader: &mut impl SymbolBits, book: &HuffCodebook) -> Result<u8> {
    if let Some(symbol) = book.only_symbol {
        return Ok(symbol);
    }
    let mut code = 0_usize;
    for length in 1..=MAX_CODE_BITS {
        let bit = usize::try_from(reader.read_symbol_bit()?)
            .map_err(|_| Error::damaged("Huffman data bit conversion failed"))?;
        let shift = length
            .checked_sub(1)
            .ok_or_else(|| Error::damaged("Huffman bit index underflow"))?;
        code |= bit
            .checked_shl(u32::try_from(shift).unwrap_or_default())
            .unwrap_or_default();
        if let Some(Some(symbol)) = book.decode.get(length).and_then(|slots| slots.get(code)) {
            return Ok(*symbol);
        }
    }
    Err(Error::damaged("no independent Huffman code matches input bits"))
}

fn decode_three_streams(first: &[u8], shared: &[u8], output: &mut [u8], book: &HuffCodebook) -> Result<()> {
    let mut forward = ForwardSymbolBits::new(first);
    let mut middle = ForwardSymbolBits::new(shared);
    let mut reverse = BackwardSymbolBits::new(shared);
    let mut destination = 0_usize;
    while destination < output.len() {
        if let Some(slot) = output.get_mut(destination) {
            *slot = next_symbol(&mut forward, book)?;
        }
        destination = destination.saturating_add(1);
        if destination >= output.len() {
            break;
        }
        if let Some(slot) = output.get_mut(destination) {
            *slot = next_symbol(&mut reverse, book)?;
        }
        destination = destination.saturating_add(1);
        if destination >= output.len() {
            break;
        }
        if let Some(slot) = output.get_mut(destination) {
            *slot = next_symbol(&mut middle, book)?;
        }
        destination = destination.saturating_add(1);
    }
    if forward.consumed_bytes() != first.len()
        || middle.consumed_bytes().saturating_add(reverse.consumed_bytes()) != shared.len()
    {
        return Err(Error::damaged(
            "independent Huffman streams did not meet at their split",
        ));
    }
    Ok(())
}

fn decode_huffman(source: &[u8], output: &mut [u8], split_halves: bool) -> Result<()> {
    let mut bits = MsbBits::new(source);
    let lengths = decode_huffman_lengths(&mut bits)?;
    let start = bits.byte_position();
    let book = build_codebook(&lengths)?;
    if let Some(symbol) = book.only_symbol {
        output.fill(symbol);
        return Ok(());
    }
    let streams = source
        .get(start..)
        .ok_or_else(|| Error::damaged("Huffman streams start outside source"))?;
    if !split_halves {
        let split_bytes = streams
            .get(..2)
            .ok_or_else(|| Error::damaged("missing independent Huffman split"))?;
        let split = usize::from(u16::from_le_bytes(
            split_bytes
                .try_into()
                .map_err(|_| Error::damaged("Huffman split header size mismatch"))?,
        ));
        let stream_start = 2_usize;
        let middle = stream_start
            .checked_add(split)
            .ok_or_else(|| Error::damaged("Huffman split position overflow"))?;
        if middle > streams.len() {
            return Err(Error::damaged("independent Huffman split exceeds source"));
        }
        decode_three_streams(
            streams
                .get(stream_start..middle)
                .ok_or_else(|| Error::damaged("Huffman forward stream outside source"))?,
            streams
                .get(middle..)
                .ok_or_else(|| Error::damaged("Huffman shared stream outside source"))?,
            output,
            &book,
        )
    } else {
        let split_header = streams
            .get(..3)
            .ok_or_else(|| Error::damaged("missing Huffman halves split"))?;
        let first_byte = *split_header
            .first()
            .ok_or_else(|| Error::damaged("Huffman half split header is empty"))?;
        let second_byte = *split_header
            .get(1)
            .ok_or_else(|| Error::damaged("Huffman half split header is short"))?;
        let third_byte = *split_header
            .get(2)
            .ok_or_else(|| Error::damaged("Huffman half split header is short"))?;
        let first_size = usize::from(first_byte) | (usize::from(second_byte) << 8) | (usize::from(third_byte) << 16);
        let first_base = 3_usize;
        let second_base = first_base
            .checked_add(first_size)
            .ok_or_else(|| Error::damaged("Huffman half split overflow"))?;
        if second_base > streams.len() {
            return Err(Error::damaged("independent Huffman half split exceeds source"));
        }
        let first_part = streams
            .get(first_base..second_base)
            .ok_or_else(|| Error::damaged("first Huffman half outside source"))?;
        let first_forward_len = read_u16(first_part, 0)?;
        let first_shared_at = 2_usize
            .checked_add(first_forward_len)
            .ok_or_else(|| Error::damaged("first Huffman stream split overflow"))?;
        if first_shared_at > first_part.len() {
            return Err(Error::damaged("first Huffman stream split exceeds its half"));
        }
        let half = output.len().saturating_add(1) / 2;
        decode_three_streams(
            first_part
                .get(2..first_shared_at)
                .ok_or_else(|| Error::damaged("first Huffman forward stream invalid"))?,
            first_part
                .get(first_shared_at..)
                .ok_or_else(|| Error::damaged("first Huffman reverse stream invalid"))?,
            output
                .get_mut(..half)
                .ok_or_else(|| Error::damaged("first Huffman output half invalid"))?,
            &book,
        )?;

        let second_part = streams
            .get(second_base..)
            .ok_or_else(|| Error::damaged("second Huffman half outside source"))?;
        let second_forward_len = read_u16(second_part, 0)?;
        let second_shared_at = 2_usize
            .checked_add(second_forward_len)
            .ok_or_else(|| Error::damaged("second Huffman stream split overflow"))?;
        if second_shared_at > second_part.len() {
            return Err(Error::damaged("second Huffman stream split exceeds its half"));
        }
        decode_three_streams(
            second_part
                .get(2..second_shared_at)
                .ok_or_else(|| Error::damaged("second Huffman forward stream invalid"))?,
            second_part
                .get(second_shared_at..)
                .ok_or_else(|| Error::damaged("second Huffman reverse stream invalid"))?,
            output
                .get_mut(half..)
                .ok_or_else(|| Error::damaged("second Huffman output half invalid"))?,
            &book,
        )
    }
}

fn read_u16(source: &[u8], offset: usize) -> Result<usize> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| Error::damaged("Huffman u16 range overflow"))?;
    let pair = source
        .get(offset..end)
        .ok_or_else(|| Error::damaged("short Huffman u16"))?;
    let value = u16::from_le_bytes(
        pair.try_into()
            .map_err(|_| Error::damaged("Huffman u16 header size mismatch"))?,
    );
    Ok(usize::from(value))
}
