//! X-Ray game archive reader (`.db*`, `.xdb*`, `.xrp`).
//!
//! Reads archives with positional reads (`ReadAt`) without loading whole files
//! into memory or memory-mapping. Verifies chunk sizes, limits, and per-file CRC32.

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use sse_core::{Error, Result};

use crate::crc32::crc32;
use crate::encoding::decode_archive_name;

/// Largest allowed size of an archive header (64 MiB).
pub const MAXIMUM_HEADER_SIZE: usize = 64 * 1024 * 1024;

/// Largest allowed size of an extracted file from an archive (512 MiB).
pub const MAXIMUM_ENTRY_SIZE: u32 = 512 * 1024 * 1024;

const COMPRESSED_CHUNK_FLAG: u32 = 0x8000_0000;

/// Header decoder function type: takes compressed header bytes and returns candidate decoded header tables.
pub type HeaderDecoder = Arc<dyn Fn(&[u8]) -> Result<Vec<Vec<u8>>> + Send + Sync>;

/// Entry decompressor function type: takes compressed entry bytes and expected uncompressed size.
pub type EntryDecoder = Arc<dyn Fn(&[u8], usize) -> Result<Vec<u8>> + Send + Sync>;

/// Trait for positional reads without moving an internal file pointer.
pub trait ReadAt: Send + Sync {
    /// Returns the length of the readable resource in bytes.
    fn len(&self) -> u64;

    /// Returns `true` if the resource is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Reads exact number of bytes into `buf` starting at `offset`.
    ///
    /// # Errors
    /// Returns [`Error::System`] on I/O error or [`Error::Damaged`] on unexpected EOF or range errors.
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()>;
}

impl ReadAt for [u8] {
    fn len(&self) -> u64 {
        self.len() as u64
    }

    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()> {
        let offset = usize::try_from(offset).map_err(|_| Error::damaged("offset out of range"))?;
        let end = offset
            .checked_add(buf.len())
            .ok_or_else(|| Error::damaged("offset overflow"))?;
        let slice = self
            .get(offset..end)
            .ok_or_else(|| Error::damaged("read past end of slice"))?;
        buf.copy_from_slice(slice);
        Ok(())
    }
}

impl ReadAt for Vec<u8> {
    fn len(&self) -> u64 {
        self.len() as u64
    }

    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()> {
        self.as_slice().read_exact_at(buf, offset)
    }
}

impl<T: ReadAt + ?Sized> ReadAt for &T {
    fn len(&self) -> u64 {
        (**self).len()
    }

    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()> {
        (**self).read_exact_at(buf, offset)
    }
}

impl<T: ReadAt + ?Sized> ReadAt for Box<T> {
    fn len(&self) -> u64 {
        (**self).len()
    }

    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()> {
        (**self).read_exact_at(buf, offset)
    }
}

impl<T: ReadAt + ?Sized> ReadAt for Arc<T> {
    fn len(&self) -> u64 {
        (**self).len()
    }

    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()> {
        (**self).read_exact_at(buf, offset)
    }
}

impl ReadAt for File {
    fn len(&self) -> u64 {
        self.metadata().map(|m| m.len()).unwrap_or(0)
    }

    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            FileExt::read_exact_at(self, buf, offset).map_err(|e| {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    Error::damaged("unexpected end of file")
                } else {
                    Error::System(e.to_string())
                }
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            let mut current_offset = offset;
            let mut remaining = buf;
            while !remaining.is_empty() {
                let n =
                    FileExt::seek_read(self, remaining, current_offset).map_err(|e| Error::System(e.to_string()))?;
                if n == 0 {
                    return Err(Error::damaged("unexpected end of file"));
                }
                let n_u64 = u64::try_from(n).map_err(|_| Error::damaged("read size conversion error"))?;
                current_offset = current_offset
                    .checked_add(n_u64)
                    .ok_or_else(|| Error::damaged("offset overflow"))?;
                remaining = match remaining.get_mut(n..) {
                    Some(rest) => rest,
                    None => return Err(Error::damaged("slice indexing error")),
                };
            }
            Ok(())
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (buf, offset);
            Err(Error::damaged("unsupported platform for positional file reads"))
        }
    }
}

/// An entry in an X-Ray archive table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XRayArchiveEntry {
    /// Normalized path relative to archive root (using forward slashes).
    pub name: String,
    /// Expected size of decompressed data.
    pub uncompressed_size: u32,
    /// Size of stored data in the archive.
    pub compressed_size: u32,
    /// IEEE CRC32 checksum of uncompressed data (0 if not validated).
    pub crc32: u32,
    /// Offset of entry data from the start of the archive.
    pub offset: u32,
}

/// An opened X-Ray archive backed by a positional reader.
pub struct XRayArchive {
    reader: Box<dyn ReadAt>,
    entries: Vec<XRayArchiveEntry>,
    by_name: HashMap<String, usize>,
    by_name_lower: HashMap<String, usize>,
    entry_decoder: Option<EntryDecoder>,
}

impl XRayArchive {
    /// Opens an archive from a filesystem path.
    ///
    /// # Errors
    /// Returns [`Error::System`] on I/O error or [`Error::Damaged`] on malformed archive.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_decoder(path, None, None)
    }

    /// Opens an archive from a filesystem path with custom header and entry decoders.
    ///
    /// # Errors
    /// Returns [`Error::System`] on I/O error or [`Error::Damaged`] on malformed archive.
    pub fn open_with_decoder(
        path: impl AsRef<Path>,
        header_decoder: Option<HeaderDecoder>,
        entry_decoder: Option<EntryDecoder>,
    ) -> Result<Self> {
        let file = File::open(path.as_ref()).map_err(|e| Error::System(e.to_string()))?;
        Self::from_reader(Box::new(file), header_decoder, entry_decoder)
    }

    /// Opens an archive from a filesystem path using a pre-parsed entries table.
    ///
    /// # Errors
    /// Returns [`Error::System`] on I/O error.
    pub fn open_with_entries(
        path: impl AsRef<Path>,
        entries: Vec<XRayArchiveEntry>,
        entry_decoder: Option<EntryDecoder>,
    ) -> Result<Self> {
        let file = File::open(path.as_ref()).map_err(|e| Error::System(e.to_string()))?;
        Self::from_reader_with_entries(Box::new(file), entries, entry_decoder)
    }

    /// Opens an archive from any positional reader.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] on malformed archive.
    pub fn from_reader(
        reader: Box<dyn ReadAt>,
        header_decoder: Option<HeaderDecoder>,
        entry_decoder: Option<EntryDecoder>,
    ) -> Result<Self> {
        let entries = read_entries(reader.as_ref(), header_decoder)?;
        Self::from_reader_with_entries(reader, entries, entry_decoder)
    }

    /// Opens an archive from a positional reader with pre-parsed entries.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] on invalid entries.
    pub fn from_reader_with_entries(
        reader: Box<dyn ReadAt>,
        entries: Vec<XRayArchiveEntry>,
        entry_decoder: Option<EntryDecoder>,
    ) -> Result<Self> {
        let mut by_name = HashMap::with_capacity(entries.len());
        let mut by_name_lower = HashMap::with_capacity(entries.len());

        for (idx, entry) in entries.iter().enumerate() {
            let norm = normalize_name(&entry.name);
            let lower = norm.to_ascii_lowercase();
            by_name.insert(norm, idx);
            by_name_lower.insert(lower, idx);
        }

        Ok(Self {
            reader,
            entries,
            by_name,
            by_name_lower,
            entry_decoder,
        })
    }

    /// Opens an archive from an in-memory byte buffer.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] on malformed archive.
    pub fn from_vec(
        bytes: Vec<u8>,
        header_decoder: Option<HeaderDecoder>,
        entry_decoder: Option<EntryDecoder>,
    ) -> Result<Self> {
        Self::from_reader(Box::new(bytes), header_decoder, entry_decoder)
    }

    /// Opens an archive from a borrowed byte slice by copying into an owned buffer.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] on malformed archive.
    pub fn from_slice(
        bytes: &[u8],
        header_decoder: Option<HeaderDecoder>,
        entry_decoder: Option<EntryDecoder>,
    ) -> Result<Self> {
        Self::from_vec(bytes.to_vec(), header_decoder, entry_decoder)
    }

    /// Entries in this archive.
    #[must_use]
    pub fn entries(&self) -> &[XRayArchiveEntry] {
        &self.entries
    }

    /// Checks if a file exists in this archive.
    #[must_use]
    pub fn contains_file(&self, name: &str) -> bool {
        let normalized = normalize_name(name);
        if self.by_name.contains_key(&normalized) {
            return true;
        }
        let lower = normalized.to_ascii_lowercase();
        self.by_name_lower.contains_key(&lower)
    }

    /// Reads and decompresses a file from the archive by path.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] if entry is not found, exceeds limits, fails CRC, or has corrupt data.
    pub fn read_file(&self, name: &str) -> Result<Vec<u8>> {
        if name.trim().is_empty() {
            return Err(Error::damaged("archive entry name cannot be empty"));
        }

        let normalized = normalize_name(name);
        let entry_idx = self
            .by_name
            .get(&normalized)
            .copied()
            .or_else(|| {
                let lower = normalized.to_ascii_lowercase();
                self.by_name_lower.get(&lower).copied()
            })
            .ok_or_else(|| Error::damaged(format!("X-Ray archive entry '{name}' was not found.")))?;

        let entry = self
            .entries
            .get(entry_idx)
            .ok_or_else(|| Error::damaged("invalid entry index"))?;

        if entry.offset == 0 {
            return Err(Error::damaged(format!("entry '{}' has no data offset", entry.name)));
        }

        if entry.uncompressed_size > MAXIMUM_ENTRY_SIZE || entry.compressed_size > MAXIMUM_ENTRY_SIZE {
            return Err(Error::damaged(format!(
                "entry '{}' exceeds the supported size limit",
                entry.name
            )));
        }

        let comp_len =
            usize::try_from(entry.compressed_size).map_err(|_| Error::damaged("compressed size too large"))?;
        let mut stored = vec![0u8; comp_len];
        self.reader.read_exact_at(&mut stored, u64::from(entry.offset))?;

        let data = if entry.compressed_size == entry.uncompressed_size {
            stored
        } else if let Some(decoder) = &self.entry_decoder {
            let uncomp_size =
                usize::try_from(entry.uncompressed_size).map_err(|_| Error::damaged("uncompressed size too large"))?;
            decoder(&stored, uncomp_size)?
        } else {
            return Err(Error::damaged(format!(
                "entry '{}' has compressed stream but no LZO decoder is available",
                entry.name
            )));
        };

        if u32::try_from(data.len()).ok() != Some(entry.uncompressed_size) {
            return Err(Error::damaged(format!(
                "entry '{}' has an unexpected uncompressed size",
                entry.name
            )));
        }

        if entry.crc32 != 0 {
            let actual_crc = crc32(&data);
            if actual_crc != entry.crc32 {
                return Err(Error::damaged(format!("entry '{}' failed its CRC32 check", entry.name)));
            }
        }

        Ok(data)
    }
}

fn normalize_name(name: &str) -> String {
    name.replace('\\', "/").trim_start_matches('/').to_string()
}

fn read_entries(reader: &dyn ReadAt, header_decoder: Option<HeaderDecoder>) -> Result<Vec<XRayArchiveEntry>> {
    let mut header_data: Option<Vec<u8>> = None;
    let mut header_compressed = false;
    let mut data_start: Option<usize> = None;
    let mut data_end: usize = 0;
    let mut position = 0u64;
    let total_len = reader.len();

    while position < total_len {
        if position.checked_add(8).is_none_or(|end| end > total_len) {
            return Err(Error::damaged("truncated archive chunk header"));
        }

        let mut header_slice = [0u8; 8];
        reader.read_exact_at(&mut header_slice, position)?;

        let b0: [u8; 4] = header_slice
            .get(0..4)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| Error::damaged("chunk header slice error"))?;
        let b1: [u8; 4] = header_slice
            .get(4..8)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| Error::damaged("chunk header slice error"))?;
        let chunk_type = u32::from_le_bytes(b0);
        let chunk_size = u32::from_le_bytes(b1);

        let body_offset = position
            .checked_add(8)
            .ok_or_else(|| Error::damaged("offset overflow"))?;
        let chunk_size_u64 = u64::from(chunk_size);
        let body_end = body_offset
            .checked_add(chunk_size_u64)
            .ok_or_else(|| Error::damaged("offset overflow"))?;

        if body_end > total_len {
            return Err(Error::damaged("archive chunk exceeds the input stream"));
        }

        let base_type = chunk_type & !COMPRESSED_CHUNK_FLAG;
        if base_type == 0 {
            let body_offset_usize =
                usize::try_from(body_offset).map_err(|_| Error::damaged("offset exceeds addressable space"))?;
            let body_end_usize =
                usize::try_from(body_end).map_err(|_| Error::damaged("offset exceeds addressable space"))?;
            if data_start.is_none() {
                data_start = Some(body_offset_usize);
            }
            data_end = data_end.max(body_end_usize);
        } else if base_type == 1 {
            if header_data.is_some() {
                return Err(Error::damaged("archive contains multiple file-table headers"));
            }
            let chunk_size_usize = usize::try_from(chunk_size).map_err(|_| Error::damaged("chunk size too large"))?;
            if chunk_size_usize > MAXIMUM_HEADER_SIZE {
                return Err(Error::damaged("file-table header exceeds the size limit"));
            }
            let mut chunk_body = vec![0u8; chunk_size_usize];
            reader.read_exact_at(&mut chunk_body, body_offset)?;
            header_data = Some(chunk_body);
            header_compressed = (chunk_type & COMPRESSED_CHUNK_FLAG) != 0;
        }

        position = body_end;
    }

    let header_bytes = header_data.ok_or_else(|| Error::damaged("archive has no file-table header or data chunk"))?;
    let start_off = data_start.ok_or_else(|| Error::damaged("archive has no file-table header or data chunk"))?;
    if data_end == 0 {
        return Err(Error::damaged("archive has no file-table header or data chunk"));
    }

    let candidate_headers: Vec<Vec<u8>> = if !header_compressed {
        vec![header_bytes]
    } else if let Some(decoder) = header_decoder {
        decoder(&header_bytes)?
    } else {
        return Err(Error::damaged("archive header is compressed but no decoder provided"));
    };

    for candidate in &candidate_headers {
        if let Some(entries) = try_parse_entries(candidate, start_off, data_end) {
            return Ok(entries);
        }
    }

    Err(Error::damaged("archive has no verified file-table header variant"))
}

fn try_parse_entries(header: &[u8], data_start: usize, data_end: usize) -> Option<Vec<XRayArchiveEntry>> {
    let mut entries = Vec::new();
    let mut position = 0usize;

    while position < header.len() {
        let remaining = header.len().saturating_sub(position);
        if remaining < 14 {
            return None;
        }

        let slice14 = header.get(position..position.checked_add(14)?)?;
        let name_size = u16::from_le_bytes(slice14.get(0..2)?.try_into().ok()?);
        let uncompressed_size = u32::from_le_bytes(slice14.get(2..6)?.try_into().ok()?);
        let compressed_size = u32::from_le_bytes(slice14.get(6..10)?.try_into().ok()?);
        let crc = u32::from_le_bytes(slice14.get(10..14)?.try_into().ok()?);

        position = position.checked_add(14)?;

        let name_length = usize::from(name_size.checked_sub(16)?);
        if name_length > header.len().saturating_sub(position) {
            return None;
        }

        let name_bytes = header.get(position..position.checked_add(name_length)?)?;
        let name = decode_archive_name(name_bytes);
        position = position.checked_add(name_length)?;

        let offset_slice = header.get(position..position.checked_add(4)?)?;
        let offset = u32::from_le_bytes(offset_slice.try_into().ok()?);
        position = position.checked_add(4)?;

        if offset != 0 {
            let offset_usize = usize::try_from(offset).ok()?;
            let comp_size_usize = usize::try_from(compressed_size).ok()?;
            let entry_end = offset_usize.checked_add(comp_size_usize)?;
            if offset_usize < data_start || entry_end > data_end {
                return None;
            }
        }

        entries.push(XRayArchiveEntry {
            name,
            uncompressed_size,
            compressed_size,
            crc32: crc,
            offset,
        });
    }

    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}
