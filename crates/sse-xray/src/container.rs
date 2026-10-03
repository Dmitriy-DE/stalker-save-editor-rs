//! Bounded X-Ray container and raw chunk index.

use sse_core::{Cursor, Error, Result, SaveBuffer};

const SIGNATURE: u32 = u32::MAX;
const MAXIMUM_UNPACKED_SIZE: usize = 512 * 1024 * 1024;
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
                assert!(container.image().len() <= 512 * 1024 * 1024);
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
}
