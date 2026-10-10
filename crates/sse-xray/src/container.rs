//! Bounded X-Ray container and raw chunk index.

use sse_core::{Cursor, Error, Result, SaveBuffer};

const SIGNATURE: u32 = u32::MAX;
const MAXIMUM_UNPACKED_SIZE: usize = sse_core::limits::MAXIMUM_UNPACKED_BYTES;
const MAXIMUM_CHUNKS: usize = 65_536;

/// The decompressed X-Ray image and its chunk offsets.
#[derive(Debug)]
pub struct Container {
    version: u32,
    image: SaveBuffer,
    chunks: Vec<Chunk>,
}

/// A chunk's type and payload range in the raw image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    /// X-Ray chunk type.
    pub kind: u32,
    /// Byte offset of the payload in the raw image.
    pub offset: usize,
    /// Payload length in bytes.
    pub length: usize,
}

impl Container {
    /// Decompresses a supported container and indexes its chunks without copying the image.
    pub fn read(packed: &[u8]) -> Result<Self> {
        let mut reader = Cursor::new(packed);
        if reader.remaining() < 12 {
            return Err(Error::damaged("X-Ray header is shorter than 12 bytes"));
        }
        if reader.u32()? != SIGNATURE {
            return Err(Error::damaged("invalid X-Ray container signature"));
        }
        let version = reader.u32()?;
        if ![3, 5, 6].contains(&version) {
            return Err(Error::damaged(format!("unsupported X-Ray container version {version}")));
        }
        let unpacked_u32 = reader.u32()?;
        let unpacked_size = usize::try_from(unpacked_u32)
            .map_err(|_| Error::damaged("X-Ray unpacked size does not fit this platform"))?;
        if unpacked_size == 0 || unpacked_size > MAXIMUM_UNPACKED_SIZE {
            return Err(Error::damaged(format!("invalid X-Ray unpacked size {unpacked_size}")));
        }
        let compressed = reader.take(reader.remaining())?;
        let raw = sse_codecs::lzo1x::decompress(compressed, unpacked_size)
            .map_err(|error| Error::damaged(format!("invalid X-Ray LZO payload: {error}")))?;
        let image = SaveBuffer::from_vec(raw);
        let chunks = parse_chunks(image.as_slice())?;
        Ok(Self { version, image, chunks })
    }

    /// Container format version.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// The decompressed image.
    #[must_use]
    pub fn image(&self) -> &[u8] {
        self.image.as_slice()
    }

    /// Parsed chunks in original order.
    #[must_use]
    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    /// Returns the payload bytes for a chunk from the shared image.
    pub fn chunk_bytes(&self, chunk: Chunk) -> Result<&[u8]> {
        let end = chunk
            .offset
            .checked_add(chunk.length)
            .ok_or_else(|| Error::damaged("X-Ray chunk range overflow"))?;
        self.image
            .as_slice()
            .get(chunk.offset..end)
            .ok_or_else(|| Error::damaged("X-Ray chunk range is outside the image"))
    }

    pub(crate) fn repack(&self, raw: &[u8]) -> Result<SaveBuffer> {
        if raw.is_empty() || raw.len() > MAXIMUM_UNPACKED_SIZE {
            return Err(Error::Refused(format!("invalid X-Ray image size {}", raw.len())));
        }
        let unpacked_size = u32::try_from(raw.len())
            .map_err(|_| Error::Refused("X-Ray image size does not fit the container header".to_owned()))?;
        let compressed = sse_codecs::lzo1x::compress(raw);
        let packed_size = 12_usize
            .checked_add(compressed.len())
            .ok_or_else(|| Error::Refused("packed X-Ray size overflow".to_owned()))?;
        let mut packed = Vec::with_capacity(packed_size);
        packed.extend_from_slice(&SIGNATURE.to_le_bytes());
        packed.extend_from_slice(&self.version.to_le_bytes());
        packed.extend_from_slice(&unpacked_size.to_le_bytes());
        packed.extend_from_slice(&compressed);
        Ok(SaveBuffer::from_vec(packed))
    }

    #[cfg(test)]
    pub(crate) fn rebuild_chunk_payloads(&self, raw: &[u8], replacements: &[(u32, &[u8])]) -> Result<Vec<u8>> {
        let mut found = vec![false; replacements.len()];
        let mut capacity = raw.len();
        let mut output = Vec::new();
        for chunk in &self.chunks {
            let end = chunk
                .offset
                .checked_add(chunk.length)
                .ok_or_else(|| Error::damaged("X-Ray chunk range overflow"))?;
            let _payload = raw
                .get(chunk.offset..end)
                .ok_or_else(|| Error::damaged("X-Ray chunk is outside the current image"))?;
            if let Some((index, (_, replacement))) = replacements
                .iter()
                .enumerate()
                .find(|(_, (kind, _))| *kind == chunk.kind)
            {
                let already_found = found
                    .get(index)
                    .copied()
                    .ok_or_else(|| Error::damaged("X-Ray replacement index is outside its table"))?;
                if already_found {
                    return Err(Error::Refused(format!(
                        "X-Ray chunk type {} occurs more than once",
                        chunk.kind
                    )));
                }
                let found_slot = found
                    .get_mut(index)
                    .ok_or_else(|| Error::damaged("X-Ray replacement index is outside its table"))?;
                *found_slot = true;
                capacity = capacity
                    .checked_sub(chunk.length)
                    .and_then(|value| value.checked_add(replacement.len()))
                    .ok_or_else(|| Error::Refused("X-Ray rebuilt image size overflow".to_owned()))?;
            }
        }
        for ((kind, _), is_found) in replacements.iter().zip(found) {
            if !is_found {
                return Err(Error::damaged(format!("missing X-Ray chunk type {kind}")));
            }
        }
        output.reserve(capacity);
        for chunk in &self.chunks {
            let end = chunk
                .offset
                .checked_add(chunk.length)
                .ok_or_else(|| Error::damaged("X-Ray chunk range overflow"))?;
            let payload = raw
                .get(chunk.offset..end)
                .ok_or_else(|| Error::damaged("X-Ray chunk is outside the current image"))?;
            let payload = replacements
                .iter()
                .find(|(kind, _)| *kind == chunk.kind)
                .map_or(payload, |(_, replacement)| *replacement);
            let payload_length = u32::try_from(payload.len())
                .map_err(|_| Error::Refused("X-Ray chunk exceeds its u32 length field".to_owned()))?;
            output.extend_from_slice(&chunk.kind.to_le_bytes());
            output.extend_from_slice(&payload_length.to_le_bytes());
            output.extend_from_slice(payload);
        }
        Ok(output)
    }

    /// Replaces chunk payloads in the existing image buffer, without building a second image.
    pub(crate) fn rebuild_chunk_payloads_in_place(
        &self,
        raw: &mut Vec<u8>,
        replacements: &[(u32, &[u8])],
    ) -> Result<()> {
        let mut found = vec![false; replacements.len()];
        let mut final_size = raw.len();
        for chunk in &self.chunks {
            let end = chunk
                .offset
                .checked_add(chunk.length)
                .ok_or_else(|| Error::damaged("X-Ray chunk range overflow"))?;
            raw.get(chunk.offset..end)
                .ok_or_else(|| Error::damaged("X-Ray chunk is outside the current image"))?;
            let header_start = chunk
                .offset
                .checked_sub(8)
                .ok_or_else(|| Error::damaged("X-Ray chunk header offset underflow"))?;
            let header = raw
                .get(header_start..chunk.offset)
                .ok_or_else(|| Error::damaged("X-Ray chunk header is outside the current image"))?;
            let header_kind = header
                .get(..4)
                .ok_or_else(|| Error::damaged("X-Ray chunk type is outside the current image"))?;
            let header_length = header
                .get(4..)
                .ok_or_else(|| Error::damaged("X-Ray chunk length is outside the current image"))?;
            let indexed_length = u32::try_from(chunk.length)
                .map_err(|_| Error::damaged("X-Ray chunk length does not fit its header"))?;
            if header_kind != chunk.kind.to_le_bytes() || header_length != indexed_length.to_le_bytes() {
                return Err(Error::damaged("X-Ray chunk index does not match the current image"));
            }
            if let Some((index, (_, replacement))) = replacements
                .iter()
                .enumerate()
                .find(|(_, (kind, _))| *kind == chunk.kind)
            {
                let already_found = found
                    .get(index)
                    .copied()
                    .ok_or_else(|| Error::damaged("X-Ray replacement index is outside its table"))?;
                if already_found {
                    return Err(Error::Refused(format!(
                        "X-Ray chunk type {} occurs more than once",
                        chunk.kind
                    )));
                }
                let found_slot = found
                    .get_mut(index)
                    .ok_or_else(|| Error::damaged("X-Ray replacement index is outside its table"))?;
                *found_slot = true;
                u32::try_from(replacement.len())
                    .map_err(|_| Error::Refused("X-Ray chunk exceeds its u32 length field".to_owned()))?;
                final_size = final_size
                    .checked_sub(chunk.length)
                    .and_then(|size| size.checked_add(replacement.len()))
                    .ok_or_else(|| Error::Refused("X-Ray rebuilt image size overflow".to_owned()))?;
            }
        }
        for ((kind, _), is_found) in replacements.iter().zip(found) {
            if !is_found {
                return Err(Error::damaged(format!("missing X-Ray chunk type {kind}")));
            }
        }
        if final_size == 0 || final_size > MAXIMUM_UNPACKED_SIZE {
            return Err(Error::Refused(format!("invalid rebuilt X-Ray image size {final_size}")));
        }
        raw.try_reserve_exact(final_size.saturating_sub(raw.len()))
            .map_err(|_| Error::Refused("unable to reserve space for rebuilt X-Ray image".to_owned()))?;

        // Work from the end so each chunk's original offset stays valid while later chunks move.
        for chunk in self.chunks.iter().rev() {
            let Some((_, replacement)) = replacements.iter().find(|(kind, _)| *kind == chunk.kind) else {
                continue;
            };
            let old_end = chunk
                .offset
                .checked_add(chunk.length)
                .ok_or_else(|| Error::damaged("X-Ray chunk range overflow"))?;
            let new_end = chunk
                .offset
                .checked_add(replacement.len())
                .ok_or_else(|| Error::Refused("X-Ray replacement range overflow".to_owned()))?;
            let old_image_len = raw.len();
            if replacement.len() > chunk.length {
                let growth = replacement
                    .len()
                    .checked_sub(chunk.length)
                    .ok_or_else(|| Error::Refused("X-Ray replacement growth underflow".to_owned()))?;
                let expanded_len = old_image_len
                    .checked_add(growth)
                    .ok_or_else(|| Error::Refused("X-Ray rebuilt image size overflow".to_owned()))?;
                raw.resize(expanded_len, 0);
                raw.copy_within(old_end..old_image_len, new_end);
            } else if replacement.len() < chunk.length {
                let shrinkage = chunk
                    .length
                    .checked_sub(replacement.len())
                    .ok_or_else(|| Error::Refused("X-Ray replacement shrinkage underflow".to_owned()))?;
                raw.copy_within(old_end..old_image_len, new_end);
                let shortened_len = old_image_len
                    .checked_sub(shrinkage)
                    .ok_or_else(|| Error::Refused("X-Ray rebuilt image size underflow".to_owned()))?;
                raw.truncate(shortened_len);
            }
            raw.get_mut(chunk.offset..new_end)
                .ok_or_else(|| Error::damaged("X-Ray replacement is outside the rebuilt image"))?
                .copy_from_slice(replacement);
            let header_start = chunk
                .offset
                .checked_sub(4)
                .ok_or_else(|| Error::damaged("X-Ray chunk length offset underflow"))?;
            let header_end = header_start
                .checked_add(4)
                .ok_or_else(|| Error::damaged("X-Ray chunk length range overflow"))?;
            raw.get_mut(header_start..header_end)
                .ok_or_else(|| Error::damaged("X-Ray chunk length is outside the rebuilt image"))?
                .copy_from_slice(
                    &u32::try_from(replacement.len())
                        .map_err(|_| Error::Refused("X-Ray chunk exceeds its u32 length field".to_owned()))?
                        .to_le_bytes(),
                );
        }
        Ok(())
    }
}

fn parse_chunks(raw: &[u8]) -> Result<Vec<Chunk>> {
    let mut reader = Cursor::new(raw);
    let mut chunks = Vec::new();
    while reader.remaining() > 0 {
        if chunks.len() >= MAXIMUM_CHUNKS {
            return Err(Error::damaged(format!("X-Ray chunk count exceeds {MAXIMUM_CHUNKS}")));
        }
        if reader.remaining() < 8 {
            return Err(Error::damaged(format!(
                "truncated X-Ray chunk header at {}",
                reader.position()
            )));
        }
        let kind = reader.u32()?;
        let length_u32 = reader.u32()?;
        let length =
            usize::try_from(length_u32).map_err(|_| Error::damaged("X-Ray chunk size does not fit this platform"))?;
        if length > reader.remaining() {
            return Err(Error::damaged(format!("X-Ray chunk type {kind} exceeds the image")));
        }
        let offset = reader.position();
        reader.skip(length)?;
        chunks.push(Chunk { kind, offset, length });
    }
    if chunks.is_empty() {
        return Err(Error::damaged("X-Ray image contains no chunks"));
    }
    Ok(chunks)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::{Container, MAXIMUM_CHUNKS};
    use sse_core::Error;

    #[test]
    fn rejects_a_chunk_length_beyond_the_decompressed_image() {
        let source = include_bytes!("../../../fixtures/synthetic/xray-soc.raw");
        let mut raw = source.to_vec();
        let Some(size) = raw.get_mut(4..8) else {
            panic!("first chunk header expected")
        };
        size.copy_from_slice(&u32::MAX.to_le_bytes());
        let packed = wrap(3, &raw);
        assert!(matches!(Container::read(&packed), Err(Error::Damaged(_))));
    }

    #[test]
    fn rejects_an_unbounded_table_of_empty_chunks() {
        let mut raw = Vec::with_capacity((MAXIMUM_CHUNKS + 1) * 8);
        for _ in 0..=MAXIMUM_CHUNKS {
            raw.extend_from_slice(&0_u32.to_le_bytes());
            raw.extend_from_slice(&0_u32.to_le_bytes());
        }
        let packed = wrap(3, &raw);
        assert!(matches!(Container::read(&packed), Err(Error::Damaged(_))));
    }

    #[test]
    fn in_place_chunk_rebuild_matches_the_reference_for_growth_and_shrinkage() -> sse_core::Result<()> {
        let raw = chunk(1, b"abc", &chunk(2, b"defg", &chunk(3, b"hi", &[])));
        let packed = wrap(3, &raw);
        let container = Container::read(&packed)?;
        let replacements: [(u32, &[u8]); 2] = [(1, b"longer payload"), (3, b"x")];
        let expected = container.rebuild_chunk_payloads(&raw, &replacements)?;
        let mut actual = raw.clone();
        container.rebuild_chunk_payloads_in_place(&mut actual, &replacements)?;
        assert_eq!(actual, expected);
        assert_eq!(Container::read(&wrap(3, &actual))?.image(), actual);
        Ok(())
    }

    #[test]
    fn in_place_chunk_rebuild_rejects_missing_replacements_before_mutating() -> sse_core::Result<()> {
        let raw = chunk(1, b"payload", &[]);
        let packed = wrap(3, &raw);
        let container = Container::read(&packed)?;
        let mut actual = raw.clone();
        assert!(container
            .rebuild_chunk_payloads_in_place(&mut actual, &[(9, b"missing")])
            .is_err());
        assert_eq!(actual, raw);
        Ok(())
    }

    #[test]
    fn every_xray_packed_fixture_decompresses_to_its_raw_pair() {
        let pairs: [(&[u8], &[u8]); 7] = [
            (
                include_bytes!("../../../fixtures/synthetic/xray-soc.sav"),
                include_bytes!("../../../fixtures/synthetic/xray-soc.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-clear-sky.sav"),
                include_bytes!("../../../fixtures/synthetic/xray-clear-sky.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-call-of-pripyat.sav"),
                include_bytes!("../../../fixtures/synthetic/xray-call-of-pripyat.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-soc-ee.sav"),
                include_bytes!("../../../fixtures/synthetic/xray-soc-ee.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-clear-sky-ee.sav"),
                include_bytes!("../../../fixtures/synthetic/xray-clear-sky-ee.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-call-of-pripyat-ee.sav"),
                include_bytes!("../../../fixtures/synthetic/xray-call-of-pripyat-ee.raw"),
            ),
            (
                include_bytes!("../../../fixtures/synthetic/xray-call-of-pripyat-base-item.sav"),
                include_bytes!("../../../fixtures/synthetic/xray-call-of-pripyat-base-item.raw"),
            ),
        ];
        for (packed, expected_raw) in pairs {
            let parsed = Container::read(packed);
            assert!(parsed.is_ok(), "fixture container failed: {parsed:?}");
            let Ok(container) = parsed else { continue };
            assert_eq!(container.image(), expected_raw);
        }
    }

    #[test]
    fn fixed_seed_mutations_stay_bounded_and_repeatable() {
        let source = include_bytes!("../../../fixtures/synthetic/xray-soc.sav");
        for seed in 0_u64..256 {
            let mut mutated = source.to_vec();
            let length = mutated.len();
            let position = usize::try_from(seed.checked_mul(97).unwrap_or_default())
                .unwrap_or_default()
                .checked_rem(length)
                .unwrap_or_default();
            if let Some(byte) = mutated.get_mut(position) {
                *byte ^= u8::try_from(seed % 256).unwrap_or_default().max(1);
            }
            if let Ok(container) = Container::read(&mutated) {
                assert!(container.image().len() <= 256 * 1024 * 1024);
            }
        }
    }

    fn wrap(version: u32, raw: &[u8]) -> Vec<u8> {
        let compressed = sse_codecs::lzo1x::compress(raw);
        let mut result = Vec::with_capacity(12_usize.saturating_add(compressed.len()));
        result.extend_from_slice(&u32::MAX.to_le_bytes());
        result.extend_from_slice(&version.to_le_bytes());
        result.extend_from_slice(&u32::try_from(raw.len()).unwrap_or_default().to_le_bytes());
        result.extend_from_slice(&compressed);
        result
    }

    fn chunk(kind: u32, payload: &[u8], tail: &[u8]) -> Vec<u8> {
        let mut result = Vec::new();
        result.extend_from_slice(&kind.to_le_bytes());
        result.extend_from_slice(&u32::try_from(payload.len()).unwrap_or_default().to_le_bytes());
        result.extend_from_slice(payload);
        result.extend_from_slice(tail);
        result
    }
}
