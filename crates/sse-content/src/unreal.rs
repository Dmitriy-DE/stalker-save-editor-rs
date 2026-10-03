//! Read-only Unreal Engine IoStore (.utoc/.ucas) and legacy .pak access.
//!
//! The parser deliberately rejects encrypted containers. Compression is delegated to
//! the repository's zlib and Kraken decoders.

use sse_codecs::{inflate, kraken};
use sse_core::{Error, Result};
use std::collections::BTreeMap;

const IOSTORE_MAGIC: &[u8; 16] = b"-==--==--==--==-";
const PAK_MAGIC: u32 = 0x5A6F12E1;
const MAX_ENTRY_COUNT: usize = 4_000_000;
const MAX_FILE_SIZE: usize = 1_073_741_824;

/// A file advertised by an Unreal container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnrealEntry {
    /// Normalized slash-separated path.
    pub path: String,
    /// Uncompressed byte length.
    pub size: u64,
}

#[derive(Debug, Clone)]
struct IoChunk {
    offset: u64,
    length: u64,
}
#[derive(Debug, Clone)]
struct IoBlock {
    offset: u64,
    uncompressed: usize,
    method: u8,
}

/// Read-only IoStore view over a .utoc/.ucas pair.
pub struct IoStore<'a> {
    ucas: &'a [u8],
    block_size: usize,
    methods: Vec<String>,
    chunks: Vec<IoChunk>,
    blocks: Vec<IoBlock>,
    files: BTreeMap<String, usize>,
}

impl<'a> IoStore<'a> {
    /// Parses an unencrypted IoStore TOC and associates it with its UCAS payload.
    ///
    /// # Errors
    /// Returns Error::Damaged for malformed tables and Error::Refused for encryption.
    pub fn open(utoc: &[u8], ucas: &'a [u8]) -> Result<Self> {
        if utoc.len() < 0x90 || utoc.get(..16) != Some(&IOSTORE_MAGIC[..]) {
            return Err(Error::damaged("invalid IoStore TOC magic/header"));
        }
        let mut r = Reader::new(utoc);
        r.skip(16)?;
        let version = r.u8()?;
        r.skip(3)?;
        let header_size = usize::try_from(r.u32()?).map_err(|_| Error::damaged("IoStore header size"))?;
        if header_size != 0x90 {
            return Err(Error::damaged("unsupported IoStore header size"));
        }
        let entry_count = bounded_count(r.u32()?)?;
        let block_count = bounded_count(r.u32()?)?;
        let block_entry_size = usize::try_from(r.u32()?).map_err(|_| Error::damaged("IoStore block entry size"))?;
        if block_entry_size != 12 {
            return Err(Error::damaged("unsupported IoStore compression block entry size"));
        }
        let method_count = bounded_count(r.u32()?)?;
        let method_name_length = usize::try_from(r.u32()?).map_err(|_| Error::damaged("IoStore method name length"))?;
        let block_size = usize::try_from(r.u32()?).map_err(|_| Error::damaged("IoStore block size"))?;
        if block_size == 0 {
            return Err(Error::damaged("IoStore compression block size is zero"));
        }
        let directory_size = usize::try_from(r.u32()?).map_err(|_| Error::damaged("IoStore directory size"))?;
        let _partition_count = r.u32()?;
        r.skip(8 + 16)?;
        let flags = r.u8()?;
        r.skip(3)?;
        let perfect_hash_count = bounded_count(r.u32()?)?;
        let _partition_size = r.u64()?;
        let overflow_count = bounded_count(r.u32()?)?;
        r.skip(4 + 40)?;
        if r.position() != header_size {
            return Err(Error::damaged("IoStore header accounting mismatch"));
        }
        if flags & 0x02 != 0 {
            return Err(Error::Refused(
                "encrypted IoStore containers are unsupported".to_owned(),
            ));
        }

        r.skip(
            entry_count
                .checked_mul(12)
                .ok_or_else(|| Error::damaged("IoStore chunk-id table overflow"))?,
        )?;
        let mut chunks = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            let packed = r.take(10)?;
            chunks.push(IoChunk {
                offset: be40(packed.get(..5).ok_or_else(|| Error::damaged("IoStore chunk offset"))?)?,
                length: be40(packed.get(5..).ok_or_else(|| Error::damaged("IoStore chunk length"))?)?,
            });
        }
        if version >= 4 {
            r.skip(
                perfect_hash_count
                    .checked_mul(4)
                    .ok_or_else(|| Error::damaged("IoStore perfect hash table overflow"))?,
            )?;
        }
        if version >= 5 {
            r.skip(
                overflow_count
                    .checked_mul(4)
                    .ok_or_else(|| Error::damaged("IoStore overflow table overflow"))?,
            )?;
        }

        let mut blocks = Vec::with_capacity(block_count);
        for _ in 0..block_count {
            let b = r.take(12)?;
            blocks.push(IoBlock {
                offset: le40(b.get(..5).ok_or_else(|| Error::damaged("IoStore block offset"))?)?,
                compressed: usize::try_from(le24(
                    b.get(5..8).ok_or_else(|| Error::damaged("IoStore compressed size"))?,
                )?)
                .map_err(|_| Error::damaged("IoStore compressed size"))?,
                uncompressed: usize::try_from(le24(
                    b.get(8..11)
                        .ok_or_else(|| Error::damaged("IoStore uncompressed size"))?,
                )?)
                .map_err(|_| Error::damaged("IoStore uncompressed size"))?,
                method: *b.get(11).ok_or_else(|| Error::damaged("IoStore compression method"))?,
            });
        }
        let mut methods = Vec::with_capacity(method_count);
        for _ in 0..method_count {
            let raw = r.take(method_name_length)?;
            let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
            let name = std::str::from_utf8(raw.get(..end).unwrap_or_default())
                .map_err(|_| Error::damaged("IoStore compression method is not UTF-8"))?;
            methods.push(name.to_ascii_lowercase());
        }
        if flags & 0x04 != 0 {
            let signature_size = usize::try_from(r.u32()?).map_err(|_| Error::damaged("IoStore signature size"))?;
            r.skip(
                signature_size
                    .checked_mul(2)
                    .ok_or_else(|| Error::damaged("IoStore signature overflow"))?,
            )?;
            r.skip(
                block_count
                    .checked_mul(20)
                    .ok_or_else(|| Error::damaged("IoStore block signature overflow"))?,
            )?;
        }
        let directory = r.take(directory_size)?;
        let files = if flags & 0x08 != 0 {
            parse_directory_index(directory, entry_count)?
        } else {
            BTreeMap::new()
        };
        Ok(Self {
            ucas,
            block_size,
            methods,
            chunks,
            blocks,
            files,
        })
    }

    /// Lists files from the IoStore directory index.
    #[must_use]
    pub fn entries(&self) -> Vec<UnrealEntry> {
        self.files
            .iter()
            .filter_map(|(path, index)| {
                self.chunks.get(*index).map(|c| UnrealEntry {
                    path: path.clone(),
                    size: c.length,
                })
            })
            .collect()
    }

    /// Reads a file by normalized path.
    ///
    /// # Errors
    /// Returns Error::Refused for unknown compression/encryption and Error::Damaged for corrupt ranges.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let key = normalize_path(path);
        let index = *self
            .files
            .get(&key)
            .ok_or_else(|| Error::damaged("IoStore path not found"))?;
        let chunk = self
            .chunks
            .get(index)
            .ok_or_else(|| Error::damaged("IoStore chunk index outside table"))?;
        let wanted =
            usize::try_from(chunk.length).map_err(|_| Error::Refused("IoStore file is too large".to_owned()))?;
        if wanted > MAX_FILE_SIZE {
            return Err(Error::Refused("IoStore file exceeds read limit".to_owned()));
        }
        let first = usize::try_from(
            chunk
                .offset
                .checked_div(u64::try_from(self.block_size).unwrap_or(1))
                .unwrap_or_default(),
        )
        .map_err(|_| Error::damaged("IoStore first block"))?;
        let offset_in_first = usize::try_from(
            chunk
                .offset
                .checked_rem(u64::try_from(self.block_size).unwrap_or(1))
                .unwrap_or_default(),
        )
        .map_err(|_| Error::damaged("IoStore first-block offset"))?;
        let mut out = Vec::with_capacity(wanted);
        let mut block_index = first;
        let mut skip = offset_in_first;
        while out.len() < wanted {
            let block = self
                .blocks
                .get(block_index)
                .ok_or_else(|| Error::damaged("IoStore chunk exceeds compression block table"))?;
            let decoded = self.decode_block(block)?;
            if skip > decoded.len() {
                return Err(Error::damaged("IoStore chunk offset exceeds block"));
            }
            let remaining = wanted.saturating_sub(out.len());
            let available = decoded.len().saturating_sub(skip);
            let take = remaining.min(available);
            out.extend_from_slice(
                decoded
                    .get(skip..skip.saturating_add(take))
                    .ok_or_else(|| Error::damaged("IoStore block slice"))?,
            );
            if take == 0 {
                return Err(Error::damaged("IoStore zero-progress block"));
            }
            skip = 0;
            block_index = block_index
                .checked_add(1)
                .ok_or_else(|| Error::damaged("IoStore block index overflow"))?;
        }
        Ok(out)
    }

    fn decode_block(&self, block: &IoBlock) -> Result<Vec<u8>> {
        let start = usize::try_from(block.offset).map_err(|_| Error::damaged("IoStore physical offset"))?;
        let end = start
            .checked_add(block.compressed)
            .ok_or_else(|| Error::damaged("IoStore physical range overflow"))?;
        let source = self
            .ucas
            .get(start..end)
            .ok_or_else(|| Error::damaged("IoStore block outside UCAS"))?;
        if block.method == 0 {
            if source.len() != block.uncompressed {
                return Err(Error::damaged("IoStore uncompressed block size mismatch"));
            }
            return Ok(source.to_vec());
        }
        let method_index = usize::from(block.method.saturating_sub(1));
        let method = self
            .methods
            .get(method_index)
            .ok_or_else(|| Error::damaged("IoStore compression method index"))?;
        match method.as_str() {
            "zlib" => inflate::inflate_zlib(source, block.uncompressed),
            "oodle" | "kraken" => {
                let mut out = vec![0_u8; block.uncompressed];
                kraken::decompress_into(source, &mut out)?;
                Ok(out)
            }
            _ => Err(Error::Refused(format!(
                "unsupported IoStore compression method: {method}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
struct PakEntry {
    offset: u64,
    compressed: usize,
    uncompressed: usize,
    compression: Option<usize>,
    blocks: Vec<(u64, u64)>,
    encrypted: bool,
    block_size: usize,
    header_size: usize,
}

/// Read-only legacy Unreal .pak reader (versions 3 through 9).
pub struct Pak<'a> {
    data: &'a [u8],
    relative_blocks: bool,
    methods: Vec<String>,
    files: BTreeMap<String, PakEntry>,
}

impl<'a> Pak<'a> {
    /// Parses an unencrypted legacy PAK index.
    ///
    /// # Errors
    /// Returns Error::Damaged for malformed archives and Error::Refused for encryption/new path-hash indices.
    pub fn open(data: &'a [u8]) -> Result<Self> {
        let footer = find_pak_footer(data)?;
        if footer.encrypted {
            return Err(Error::Refused("encrypted PAK indices are unsupported".to_owned()));
        }
        if footer.version >= 10 {
            return Err(Error::Refused(
                "PAK path-hash index versions are outside X32 legacy scope".to_owned(),
            ));
        }
        let start = usize::try_from(footer.index_offset).map_err(|_| Error::damaged("PAK index offset"))?;
        let size = usize::try_from(footer.index_size).map_err(|_| Error::damaged("PAK index size"))?;
        let end = start
            .checked_add(size)
            .ok_or_else(|| Error::damaged("PAK index range overflow"))?;
        let mut r = Reader::new(
            data.get(start..end)
                .ok_or_else(|| Error::damaged("PAK index outside archive"))?,
        );
        let mount = r.fstring()?;
        let count = bounded_count(r.u32()?)?;
        let mut files = BTreeMap::new();
        for _ in 0..count {
            let relative = r.fstring()?;
            let entry = read_pak_entry(&mut r, footer.version)?;
            if entry.encrypted {
                return Err(Error::Refused("encrypted PAK entries are unsupported".to_owned()));
            }
            let path = normalize_path(&format!("{mount}{relative}"));
            files.insert(path, entry);
        }
        Ok(Self {
            data,
            relative_blocks: footer.version >= 5,
            methods: footer.methods,
            files,
        })
    }
    /// Lists indexed PAK files.
    #[must_use]
    pub fn entries(&self) -> Vec<UnrealEntry> {
        self.files
            .iter()
            .map(|(p, e)| UnrealEntry {
                path: p.clone(),
                size: u64::try_from(e.uncompressed).unwrap_or(u64::MAX),
            })
            .collect()
    }
    /// Reads and decompresses a PAK entry.
    ///
    /// # Errors
    /// Returns Error::Damaged for corrupt ranges and Error::Refused for unsupported compression.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let key = normalize_path(path);
        let entry = self
            .files
            .get(&key)
            .or_else(|| self.files.iter().find(|(p, _)| p.ends_with(&key)).map(|(_, e)| e))
            .ok_or_else(|| Error::damaged("PAK path not found"))?;
        if entry.uncompressed > MAX_FILE_SIZE {
            return Err(Error::Refused("PAK file exceeds read limit".to_owned()));
        }
        let data_start = usize::try_from(entry.offset)
            .map_err(|_| Error::damaged("PAK data offset"))?
            .checked_add(entry.header_size)
            .ok_or_else(|| Error::damaged("PAK data start overflow"))?;
        if entry.compression.is_none() {
            let end = data_start
                .checked_add(entry.uncompressed)
                .ok_or_else(|| Error::damaged("PAK file range overflow"))?;
            return Ok(self
                .data
                .get(data_start..end)
                .ok_or_else(|| Error::damaged("PAK file outside archive"))?
                .to_vec());
        }
        let method = self
            .methods
            .get(
                entry
                    .compression
                    .ok_or_else(|| Error::damaged("PAK compression slot"))?,
            )
            .ok_or_else(|| Error::damaged("PAK compression method index"))?;
        let mut out = Vec::with_capacity(entry.uncompressed);
        for (block_start, block_end) in &entry.blocks {
            let (start, end) = if self.relative_blocks {
                let base = usize::try_from(entry.offset).map_err(|_| Error::damaged("PAK entry offset"))?;
                (
                    base.checked_add(usize::try_from(*block_start).map_err(|_| Error::damaged("PAK block start"))?)
                        .ok_or_else(|| Error::damaged("PAK block start overflow"))?,
                    base.checked_add(usize::try_from(*block_end).map_err(|_| Error::damaged("PAK block end"))?)
                        .ok_or_else(|| Error::damaged("PAK block end overflow"))?,
                )
            } else {
                (
                    usize::try_from(*block_start).map_err(|_| Error::damaged("PAK block start"))?,
                    usize::try_from(*block_end).map_err(|_| Error::damaged("PAK block end"))?,
                )
            };
            let source = self
                .data
                .get(start..end)
                .ok_or_else(|| Error::damaged("PAK compressed block outside archive"))?;
            let remaining = entry.uncompressed.saturating_sub(out.len());
            let expected = remaining.min(entry.block_size.max(1));
            let decoded = match method.as_str() {
                "zlib" => inflate::inflate_zlib(source, expected)?,
                "oodle" | "kraken" => {
                    let mut v = vec![0_u8; expected];
                    kraken::decompress_into(source, &mut v)?;
                    v
                }
                _ => return Err(Error::Refused(format!("unsupported PAK compression method: {method}"))),
            };
            out.extend_from_slice(&decoded);
        }
        if out.len() != entry.uncompressed {
            return Err(Error::damaged("PAK decompressed size mismatch"));
        }
        Ok(out)
    }
}

struct PakFooter {
    version: u32,
    index_offset: u64,
    index_size: u64,
    encrypted: bool,
    methods: Vec<String>,
}
fn find_pak_footer(data: &[u8]) -> Result<PakFooter> {
    for version in (3_u32..=9).rev() {
        let size = pak_footer_size(version);
        if data.len() < size {
            continue;
        }
        let mut r = Reader::new(
            data.get(data.len().saturating_sub(size)..)
                .ok_or_else(|| Error::damaged("PAK footer"))?,
        );
        if version >= 7 {
            r.skip(16)?;
        }
        let encrypted = if version >= 4 { r.u8()? != 0 } else { false };
        if r.u32()? != PAK_MAGIC {
            continue;
        }
        let stored = r.u32()?;
        if stored != version {
            continue;
        }
        let index_offset = r.u64()?;
        let index_size = r.u64()?;
        r.skip(20)?;
        if version == 9 {
            let frozen = r.u8()? != 0;
            if frozen {
                return Err(Error::Refused("frozen PAK index is unsupported".to_owned()));
            }
        }
        let mut methods = if version < 8 {
            vec!["zlib".to_owned(), "gzip".to_owned(), "oodle".to_owned()]
        } else {
            Vec::new()
        };
        if version >= 8 {
            let names = if data.len().saturating_sub(r.position()) >= 160 {
                5
            } else {
                4
            };
            for _ in 0..names {
                let raw = r.take(32)?;
                let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
                let name = std::str::from_utf8(raw.get(..end).unwrap_or_default())
                    .map_err(|_| Error::damaged("PAK compression name"))?
                    .to_ascii_lowercase();
                methods.push(name);
            }
        }
        return Ok(PakFooter {
            version,
            index_offset,
            index_size,
            encrypted,
            methods,
        });
    }
    Err(Error::damaged("no supported PAK footer found"))
}
fn pak_footer_size(version: u32) -> usize {
    let mut n = 44_usize;
    if version >= 7 {
        n = n.saturating_add(16);
    }
    if version >= 4 {
        n = n.saturating_add(1);
    }
    if version == 9 {
        n = n.saturating_add(1);
    }
    if version >= 8 {
        n = n.saturating_add(if version == 8 { 128 } else { 160 });
    }
    n
}

fn read_pak_entry(r: &mut Reader<'_>, version: u32) -> Result<PakEntry> {
    let start = r.position();
    let offset = r.u64()?;
    let compressed = usize::try_from(r.u64()?).map_err(|_| Error::damaged("PAK compressed size"))?;
    let uncompressed = usize::try_from(r.u64()?).map_err(|_| Error::damaged("PAK uncompressed size"))?;
    let slot = r.u32()?;
    let compression = if slot == 0 {
        None
    } else {
        Some(usize::try_from(slot.saturating_sub(1)).map_err(|_| Error::damaged("PAK compression slot"))?)
    };
    r.skip(20)?;
    let mut blocks = Vec::new();
    let mut encrypted = false;
    let mut block_size = uncompressed;
    if version >= 3 {
        if compression.is_some() {
            let count = bounded_count(r.u32()?)?;
            for _ in 0..count {
                blocks.push((r.u64()?, r.u64()?));
            }
        }
        encrypted = r.u8()? != 0;
        block_size = usize::try_from(r.u32()?).map_err(|_| Error::damaged("PAK block size"))?;
    }
    let header_size = r.position().saturating_sub(start);
    Ok(PakEntry {
        offset,
        uncompressed,
        compression,
        blocks,
        encrypted,
        block_size,
        header_size,
    })
}

fn parse_directory_index(bytes: &[u8], entry_count: usize) -> Result<BTreeMap<String, usize>> {
    let mut r = Reader::new(bytes);
    let mount = r.fstring()?;
    let dir_count = bounded_count(r.u32()?)?;
    let mut dirs = Vec::with_capacity(dir_count);
    for _ in 0..dir_count {
        dirs.push((r.u32()?, r.u32()?, r.u32()?, r.u32()?));
    }
    let file_count = bounded_count(r.u32()?)?;
    let mut files = Vec::with_capacity(file_count);
    for _ in 0..file_count {
        files.push((r.u32()?, r.u32()?, r.u32()?));
    }
    let name_count = bounded_count(r.u32()?)?;
    let mut names = Vec::with_capacity(name_count);
    for _ in 0..name_count {
        names.push(r.fstring()?);
    }
    let mut out = BTreeMap::new();
    if !dirs.is_empty() {
        walk_directory(0, "", &mount, &dirs, &files, &names, entry_count, &mut out, 0)?;
    }
    Ok(out)
}
#[allow(clippy::too_many_arguments)]
fn walk_directory(
    index: usize,
    parent: &str,
    mount: &str,
    dirs: &[(u32, u32, u32, u32)],
    files: &[(u32, u32, u32)],
    names: &[String],
    entry_count: usize,
    out: &mut BTreeMap<String, usize>,
    depth: usize,
) -> Result<()> {
    if depth > 1024 {
        return Err(Error::damaged("IoStore directory recursion too deep"));
    }
    let &(name, first_child, _next, first_file) = dirs
        .get(index)
        .ok_or_else(|| Error::damaged("IoStore directory index"))?;
    let mut current = parent.to_owned();
    if name != u32::MAX {
        let n = names
            .get(usize::try_from(name).map_err(|_| Error::damaged("IoStore directory name index"))?)
            .ok_or_else(|| Error::damaged("IoStore directory name"))?;
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(n);
    }
    let mut file = first_file;
    let mut guard = 0_usize;
    while file != u32::MAX {
        guard = guard.saturating_add(1);
        if guard > files.len() {
            return Err(Error::damaged("IoStore file list cycle"));
        }
        let &(name, next, user) = files
            .get(usize::try_from(file).map_err(|_| Error::damaged("IoStore file index"))?)
            .ok_or_else(|| Error::damaged("IoStore file entry"))?;
        let n = names
            .get(usize::try_from(name).map_err(|_| Error::damaged("IoStore file name index"))?)
            .ok_or_else(|| Error::damaged("IoStore file name"))?;
        let chunk = usize::try_from(user).map_err(|_| Error::damaged("IoStore chunk user data"))?;
        if chunk >= entry_count {
            return Err(Error::damaged("IoStore directory points outside chunk table"));
        }
        let relative = if current.is_empty() {
            n.clone()
        } else {
            format!("{current}/{n}")
        };
        out.insert(normalize_path(&relative), chunk);
        let full = format!("{mount}{relative}");
        out.insert(normalize_path(&full), chunk);
        file = next;
    }
    let mut child = first_child;
    let mut guard = 0_usize;
    while child != u32::MAX {
        guard = guard.saturating_add(1);
        if guard > dirs.len() {
            return Err(Error::damaged("IoStore directory sibling cycle"));
        }
        let ci = usize::try_from(child).map_err(|_| Error::damaged("IoStore child index"))?;
        walk_directory(
            ci,
            &current,
            mount,
            dirs,
            files,
            names,
            entry_count,
            out,
            depth.saturating_add(1),
        )?;
        child = dirs.get(ci).ok_or_else(|| Error::damaged("IoStore child"))?.2;
    }
    Ok(())
}
fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches('/').to_ascii_lowercase()
}
fn bounded_count(v: u32) -> Result<usize> {
    let n = usize::try_from(v).map_err(|_| Error::damaged("container count conversion"))?;
    if n > MAX_ENTRY_COUNT {
        return Err(Error::Refused("container table is too large".to_owned()));
    }
    Ok(n)
}
fn be40(b: &[u8]) -> Result<u64> {
    if b.len() != 5 {
        return Err(Error::damaged("be40 width"));
    }
    let mut a = [0_u8; 8];
    a.get_mut(3..)
        .ok_or_else(|| Error::damaged("be40 target"))?
        .copy_from_slice(b);
    Ok(u64::from_be_bytes(a))
}
fn le40(b: &[u8]) -> Result<u64> {
    if b.len() != 5 {
        return Err(Error::damaged("le40 width"));
    }
    let mut a = [0_u8; 8];
    a.get_mut(..5)
        .ok_or_else(|| Error::damaged("le40 target"))?
        .copy_from_slice(b);
    Ok(u64::from_le_bytes(a))
}
fn le24(b: &[u8]) -> Result<u32> {
    if b.len() != 3 {
        return Err(Error::damaged("le24 width"));
    }
    Ok(u32::from(*b.first().unwrap_or(&0))
        | u32::from(*b.get(1).unwrap_or(&0)).checked_shl(8).unwrap_or_default()
        | u32::from(*b.get(2).unwrap_or(&0)).checked_shl(16).unwrap_or_default())
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }
    const fn position(&self) -> usize {
        self.at
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .ok_or_else(|| Error::damaged("container range overflow"))?;
        let v = self
            .data
            .get(self.at..end)
            .ok_or_else(|| Error::damaged("truncated container"))?;
        self.at = end;
        Ok(v)
    }
    fn skip(&mut self, n: usize) -> Result<()> {
        let _ = self.take(n)?;
        Ok(())
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(*self.take(1)?.first().ok_or_else(|| Error::damaged("u8"))?)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            <[u8; 4]>::try_from(self.take(4)?).map_err(|_| Error::damaged("u32"))?,
        ))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(
            <[u8; 8]>::try_from(self.take(8)?).map_err(|_| Error::damaged("u64"))?,
        ))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(
            <[u8; 4]>::try_from(self.take(4)?).map_err(|_| Error::damaged("i32"))?,
        ))
    }
    fn fstring(&mut self) -> Result<String> {
        let len = self.i32()?;
        if len == 0 {
            return Ok(String::new());
        }
        if len > 0 {
            let n = usize::try_from(len).map_err(|_| Error::damaged("FString length"))?;
            let raw = self.take(n)?;
            let body = raw
                .get(..raw.len().saturating_sub(1))
                .ok_or_else(|| Error::damaged("FString body"))?;
            return String::from_utf8(body.to_vec()).map_err(|_| Error::damaged("FString UTF-8"));
        }
        let units = usize::try_from(len.unsigned_abs()).map_err(|_| Error::damaged("FString UTF-16 length"))?;
        let raw = self.take(
            units
                .checked_mul(2)
                .ok_or_else(|| Error::damaged("FString UTF-16 size overflow"))?,
        )?;
        let mut u = Vec::with_capacity(units.saturating_sub(1));
        for pair in raw.chunks_exact(2).take(units.saturating_sub(1)) {
            u.push(u16::from_le_bytes(
                <[u8; 2]>::try_from(pair).map_err(|_| Error::damaged("FString UTF-16 unit"))?,
            ));
        }
        String::from_utf16(&u).map_err(|_| Error::damaged("FString UTF-16"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn push_u32(v: &mut Vec<u8>, x: u32) {
        v.extend_from_slice(&x.to_le_bytes());
    }
    fn push_u64(v: &mut Vec<u8>, x: u64) {
        v.extend_from_slice(&x.to_le_bytes());
    }
    fn fstr(v: &mut Vec<u8>, s: &str) {
        push_u32(v, u32::try_from(s.len().saturating_add(1)).unwrap_or_default());
        v.extend_from_slice(s.as_bytes());
        v.push(0);
    }
    fn set_u32(v: &mut [u8], at: usize, x: u32) {
        let end = at.saturating_add(4);
        if let Some(dst) = v.get_mut(at..end) {
            dst.copy_from_slice(&x.to_le_bytes());
        }
    }
    #[test]
    fn synthetic_iostore_directory_and_read() {
        let mut dir = Vec::new();
        fstr(&mut dir, "../../../Game/Content/");
        push_u32(&mut dir, 1);
        for x in [u32::MAX, u32::MAX, u32::MAX, 0] {
            push_u32(&mut dir, x);
        }
        push_u32(&mut dir, 1);
        for x in [0, u32::MAX, 0] {
            push_u32(&mut dir, x);
        }
        push_u32(&mut dir, 1);
        fstr(&mut dir, "hello.txt");
        let mut toc = vec![0_u8; 0x90];
        if let Some(dst) = toc.get_mut(..16) {
            dst.copy_from_slice(IOSTORE_MAGIC);
        }
        if let Some(v) = toc.get_mut(16) {
            *v = 2;
        }
        set_u32(&mut toc, 20, 0x90);
        set_u32(&mut toc, 24, 1);
        set_u32(&mut toc, 28, 1);
        set_u32(&mut toc, 32, 12);
        set_u32(&mut toc, 36, 0);
        set_u32(&mut toc, 40, 32);
        set_u32(&mut toc, 44, 65_536);
        set_u32(&mut toc, 48, u32::try_from(dir.len()).unwrap_or_default());
        set_u32(&mut toc, 52, 1);
        if let Some(v) = toc.get_mut(80) {
            *v = 0x08;
        }
        toc.extend_from_slice(&[0_u8; 12]);
        toc.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 5]);
        toc.extend_from_slice(&[0, 0, 0, 0, 0, 5, 0, 0, 5, 0, 0, 0]);
        toc.extend_from_slice(&dir);
        let store = IoStore::open(&toc, b"hello").unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(store.read_file("hello.txt"), Ok(b"hello".to_vec()));
    }
    #[test]
    fn synthetic_v3_pak_index_and_read() {
        let mut data = Vec::new();
        push_u64(&mut data, 0);
        push_u64(&mut data, 5);
        push_u64(&mut data, 5);
        push_u32(&mut data, 0);
        data.extend_from_slice(&[0_u8; 20]);
        data.push(0);
        push_u32(&mut data, 0);
        data.extend_from_slice(b"hello");
        let index_offset = u64::try_from(data.len()).unwrap_or_default();
        let mut index = Vec::new();
        fstr(&mut index, "../../../Game/Content/");
        push_u32(&mut index, 1);
        fstr(&mut index, "hello.txt");
        push_u64(&mut index, 0);
        push_u64(&mut index, 5);
        push_u64(&mut index, 5);
        push_u32(&mut index, 0);
        index.extend_from_slice(&[0_u8; 20]);
        index.push(0);
        push_u32(&mut index, 0);
        let index_size = u64::try_from(index.len()).unwrap_or_default();
        data.extend_from_slice(&index);
        push_u32(&mut data, PAK_MAGIC);
        push_u32(&mut data, 3);
        push_u64(&mut data, index_offset);
        push_u64(&mut data, index_size);
        data.extend_from_slice(&[0_u8; 20]);
        let pak = Pak::open(&data).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(pak.read_file("hello.txt"), Ok(b"hello".to_vec()));
    }
}
