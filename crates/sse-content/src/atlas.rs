//! Binary icon atlas format with LZO-compressed pages and on-demand decoding.
//!
//! Features:
//! - Duplicate deduplication by pixel hash
//! - 16-bit RGBA4444 packed pages for compact distribution (<= 3.5 MiB)
//! - Single-page on-demand decoding with LZO1X
//! - Sub-rectangle icon cropping

use crate::dds::RgbaImage;
use std::collections::HashMap;

const ATLAS_MAGIC: &[u8; 4] = b"SSIA";
const ATLAS_VERSION: u32 = 1;

/// Format of pixel data stored inside an atlas page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PageFormat {
    /// 32-bit RGBA8888 (4 bytes per pixel).
    Rgba8888 = 0,
    /// 16-bit RGBA4444 (2 bytes per pixel).
    Rgba4444 = 1,
}

impl PageFormat {
    /// Converts raw byte identifier into a `PageFormat`.
    #[must_use]
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::Rgba8888),
            1 => Some(Self::Rgba4444),
            _ => None,
        }
    }
}

/// Metadata describing the position of an icon within the atlas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlasEntry {
    /// Relative icon identifier (e.g. "xray/wpn_ak74.png", "s2/A012A.png").
    pub name: String,
    /// Index of the page containing this icon.
    pub page_index: u16,
    /// X coordinate on the page in pixels.
    pub x: u16,
    /// Y coordinate on the page in pixels.
    pub y: u16,
    /// Width of the icon in pixels.
    pub width: u16,
    /// Height of the icon in pixels.
    pub height: u16,
}

#[derive(Debug, Clone)]
struct PageHeader {
    width: u16,
    height: u16,
    format: PageFormat,
    uncompressed_size: u32,
    compressed_size: u32,
    data_offset: usize,
}

/// A decoded icon atlas supporting on-demand single-page decompression.
#[derive(Debug, Clone)]
pub struct IconAtlas<'a> {
    raw: &'a [u8],
    entries: HashMap<String, AtlasEntry>,
    pages: Vec<PageHeader>,
}

impl<'a> IconAtlas<'a> {
    /// Parses an icon atlas from binary data.
    #[must_use]
    pub fn parse(raw: &'a [u8]) -> Option<Self> {
        if raw.len() < 10 || raw.get(..4)? != ATLAS_MAGIC {
            return None;
        }

        let version = read_u32_le(raw, 4)?;
        if version != ATLAS_VERSION {
            return None;
        }

        let page_count = usize::from(read_u16_le(raw, 8)?);
        let entry_count_u32 = read_u32_le(raw, 10)?;
        let entry_count = usize::try_from(entry_count_u32).ok()?;

        let mut offset = 14_usize;
        let mut entries = HashMap::with_capacity(entry_count);

        for _ in 0..entry_count {
            let name = read_string(raw, &mut offset)?;
            let page_index = read_u16_le_offset(raw, &mut offset)?;
            let x = read_u16_le_offset(raw, &mut offset)?;
            let y = read_u16_le_offset(raw, &mut offset)?;
            let width = read_u16_le_offset(raw, &mut offset)?;
            let height = read_u16_le_offset(raw, &mut offset)?;

            entries.insert(
                name.clone(),
                AtlasEntry {
                    name,
                    page_index,
                    x,
                    y,
                    width,
                    height,
                },
            );
        }

        let mut pages = Vec::with_capacity(page_count);
        for _ in 0..page_count {
            let width = read_u16_le_offset(raw, &mut offset)?;
            let height = read_u16_le_offset(raw, &mut offset)?;
            let format_u8 = *raw.get(offset)?;
            offset = offset.checked_add(1)?;
            let format = PageFormat::from_u8(format_u8)?;
            let uncompressed_size = read_u32_le_offset(raw, &mut offset)?;
            let compressed_size = read_u32_le_offset(raw, &mut offset)?;
            let compressed_len = usize::try_from(compressed_size).ok()?;

            let data_offset = offset;
            offset = offset.checked_add(compressed_len)?;
            if offset > raw.len() {
                return None;
            }

            pages.push(PageHeader {
                width,
                height,
                format,
                uncompressed_size,
                compressed_size,
                data_offset,
            });
        }

        Some(Self { raw, entries, pages })
    }

    /// Number of icon entries registered in the atlas.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Number of pages in the atlas.
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Looks up icon entry metadata by name.
    #[must_use]
    pub fn get_entry(&self, name: &str) -> Option<&AtlasEntry> {
        self.entries.get(name)
    }

    /// Decompresses and decodes a single atlas page to RGBA8888.
    #[must_use]
    pub fn decode_page(&self, page_index: usize) -> Option<RgbaImage> {
        let page = self.pages.get(page_index)?;
        let compressed_end = page
            .data_offset
            .checked_add(usize::try_from(page.compressed_size).ok()?)?;
        let compressed = self.raw.get(page.data_offset..compressed_end)?;

        let expected_size = usize::try_from(page.uncompressed_size).ok()?;
        let decompressed = sse_codecs::lzo1x::decompress(compressed, expected_size).ok()?;

        let width = usize::from(page.width);
        let height = usize::from(page.height);

        match page.format {
            PageFormat::Rgba8888 => Some(RgbaImage::new(width, height, decompressed)),
            PageFormat::Rgba4444 => {
                let pixel_count = width.checked_mul(height)?;
                let total_rgba_bytes = pixel_count.checked_mul(4)?;
                let mut pixels = Vec::with_capacity(total_rgba_bytes);

                for chunk in decompressed.chunks_exact(2) {
                    let Ok(arr) = <[u8; 2]>::try_from(chunk) else {
                        continue;
                    };
                    let u16_val = u16::from_le_bytes(arr);
                    let r4 = (u16_val >> 12) & 0xF;
                    let g4 = (u16_val >> 8) & 0xF;
                    let b4 = (u16_val >> 4) & 0xF;
                    let a4 = u16_val & 0xF;

                    // Expand 4-bit to 8-bit: (x << 4) | x
                    let r8 = u8::try_from((r4 << 4) | r4).unwrap_or(0);
                    let g8 = u8::try_from((g4 << 4) | g4).unwrap_or(0);
                    let b8 = u8::try_from((b4 << 4) | b4).unwrap_or(0);
                    let a8 = u8::try_from((a4 << 4) | a4).unwrap_or(0);

                    pixels.push(r8);
                    pixels.push(g8);
                    pixels.push(b8);
                    pixels.push(a8);
                }

                Some(RgbaImage::new(width, height, pixels))
            }
        }
    }

    /// Crops and returns an icon image using a previously decoded page.
    #[must_use]
    pub fn crop_icon(&self, entry: &AtlasEntry, decoded_page: &RgbaImage) -> Option<RgbaImage> {
        decoded_page.crop(
            usize::from(entry.x),
            usize::from(entry.y),
            usize::from(entry.width),
            usize::from(entry.height),
        )
    }
}

/// Packs images into atlas pages using a shelf packing algorithm.
pub struct AtlasBuilder {
    page_width: usize,
    page_height: usize,
    format: PageFormat,
}

impl AtlasBuilder {
    /// Creates a builder with given page size and output format.
    #[must_use]
    pub fn new(page_width: usize, page_height: usize, format: PageFormat) -> Self {
        Self {
            page_width,
            page_height,
            format,
        }
    }

    /// Default builder targeting 2048x2048 pages with RGBA4444 format.
    #[must_use]
    pub fn default_2048() -> Self {
        Self::new(2048, 2048, PageFormat::Rgba4444)
    }

    /// Builds a packed icon atlas from named images, removing duplicates by pixel hash.
    #[must_use]
    pub fn build(&self, mut images: Vec<(String, RgbaImage)>) -> Vec<u8> {
        // Sort by height descending for optimal shelf packing
        images.sort_by(|a, b| b.1.height.cmp(&a.1.height).then_with(|| a.0.cmp(&b.0)));

        let mut hash_map: HashMap<u64, (u16, u16, u16, u16, u16)> = HashMap::new();
        let mut entries: Vec<AtlasEntry> = Vec::new();
        let mut pages: Vec<RgbaImage> = Vec::new();

        let mut current_page_idx = 0_usize;
        let mut current_x = 0_usize;
        let mut current_y = 0_usize;
        let mut shelf_h = 0_usize;

        let initial_page_bytes = self.page_width.saturating_mul(self.page_height).saturating_mul(4);
        pages.push(RgbaImage::new(
            self.page_width,
            self.page_height,
            vec![0_u8; initial_page_bytes],
        ));

        for (name, img) in &images {
            if img.width == 0 || img.height == 0 || img.width > self.page_width || img.height > self.page_height {
                continue;
            }

            let hash = compute_pixel_hash(&img.pixels);
            if let Some(&(p_idx, x, y, w, h)) = hash_map.get(&hash) {
                entries.push(AtlasEntry {
                    name: name.clone(),
                    page_index: p_idx,
                    x,
                    y,
                    width: w,
                    height: h,
                });
                continue;
            }

            // Check if item fits on current shelf
            if current_x.saturating_add(img.width) > self.page_width {
                // Next shelf
                current_y = current_y.saturating_add(shelf_h);
                current_x = 0;
                shelf_h = 0;
            }

            // Check if item fits on current page
            if current_y.saturating_add(img.height) > self.page_height {
                // Next page
                let next_page_bytes = self.page_width.saturating_mul(self.page_height).saturating_mul(4);
                pages.push(RgbaImage::new(
                    self.page_width,
                    self.page_height,
                    vec![0_u8; next_page_bytes],
                ));
                current_page_idx = current_page_idx.saturating_add(1);
                current_x = 0;
                current_y = 0;
                shelf_h = 0;
            }

            // Blit image onto current page
            if let Some(target_page) = pages.get_mut(current_page_idx) {
                blit_image(target_page, img, current_x, current_y);
            }

            let p_idx_u16 = u16::try_from(current_page_idx).unwrap_or(u16::MAX);
            let x_u16 = u16::try_from(current_x).unwrap_or(u16::MAX);
            let y_u16 = u16::try_from(current_y).unwrap_or(u16::MAX);
            let w_u16 = u16::try_from(img.width).unwrap_or(u16::MAX);
            let h_u16 = u16::try_from(img.height).unwrap_or(u16::MAX);

            hash_map.insert(hash, (p_idx_u16, x_u16, y_u16, w_u16, h_u16));
            entries.push(AtlasEntry {
                name: name.clone(),
                page_index: p_idx_u16,
                x: x_u16,
                y: y_u16,
                width: w_u16,
                height: h_u16,
            });

            current_x = current_x.saturating_add(img.width);
            shelf_h = shelf_h.max(img.height);
        }

        // Sort entries by name for stable output
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        // Encode binary atlas
        let mut output = Vec::new();
        output.extend_from_slice(ATLAS_MAGIC);
        output.extend_from_slice(&ATLAS_VERSION.to_le_bytes());

        let page_count_u16 = u16::try_from(pages.len()).unwrap_or(u16::MAX);
        output.extend_from_slice(&page_count_u16.to_le_bytes());

        let entry_count_u32 = u32::try_from(entries.len()).unwrap_or(u32::MAX);
        output.extend_from_slice(&entry_count_u32.to_le_bytes());

        for entry in &entries {
            write_string(&mut output, &entry.name);
            output.extend_from_slice(&entry.page_index.to_le_bytes());
            output.extend_from_slice(&entry.x.to_le_bytes());
            output.extend_from_slice(&entry.y.to_le_bytes());
            output.extend_from_slice(&entry.width.to_le_bytes());
            output.extend_from_slice(&entry.height.to_le_bytes());
        }

        for page in &pages {
            let width_u16 = u16::try_from(page.width).unwrap_or(u16::MAX);
            let height_u16 = u16::try_from(page.height).unwrap_or(u16::MAX);
            output.extend_from_slice(&width_u16.to_le_bytes());
            output.extend_from_slice(&height_u16.to_le_bytes());
            output.push(self.format as u8);

            let (uncompressed_data, uncompressed_size) = match self.format {
                PageFormat::Rgba8888 => {
                    let sz = u32::try_from(page.pixels.len()).unwrap_or(u32::MAX);
                    (page.pixels.clone(), sz)
                }
                PageFormat::Rgba4444 => {
                    let mut data = Vec::with_capacity(page.pixels.len().checked_div(2).unwrap_or(0));
                    for chunk in page.pixels.chunks_exact(4) {
                        let r = chunk.first().copied().unwrap_or(0) >> 4;
                        let g = chunk.get(1).copied().unwrap_or(0) >> 4;
                        let b = chunk.get(2).copied().unwrap_or(0) >> 4;
                        let a = chunk.get(3).copied().unwrap_or(0) >> 4;
                        let val = (u16::from(r) << 12) | (u16::from(g) << 8) | (u16::from(b) << 4) | u16::from(a);
                        data.extend_from_slice(&val.to_le_bytes());
                    }
                    let sz = u32::try_from(data.len()).unwrap_or(u32::MAX);
                    (data, sz)
                }
            };

            let compressed = sse_codecs::lzo1x::compress_fast(&uncompressed_data);
            let compressed_size = u32::try_from(compressed.len()).unwrap_or(u32::MAX);

            output.extend_from_slice(&uncompressed_size.to_le_bytes());
            output.extend_from_slice(&compressed_size.to_le_bytes());
            output.extend_from_slice(&compressed);
        }

        output
    }
}

fn blit_image(target: &mut RgbaImage, src: &RgbaImage, dst_x: usize, dst_y: usize) {
    for row in 0..src.height {
        let src_start = row.saturating_mul(src.width).saturating_mul(4);
        let src_end = src_start.saturating_add(src.width.saturating_mul(4));
        if let Some(src_slice) = src.pixels.get(src_start..src_end) {
            let target_y = dst_y.saturating_add(row);
            let target_start = target_y
                .saturating_mul(target.width)
                .saturating_add(dst_x)
                .saturating_mul(4);
            let target_end = target_start.saturating_add(src.width.saturating_mul(4));
            if let Some(target_slice) = target.pixels.get_mut(target_start..target_end) {
                target_slice.copy_from_slice(src_slice);
            }
        }
    }
}

fn compute_pixel_hash(pixels: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0100_0000_01b3;
    let mut hash = FNV_OFFSET_BASIS;
    for &b in pixels {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn write_string(output: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = u16::try_from(bytes.len()).unwrap_or(u16::MAX);
    output.extend_from_slice(&len.to_le_bytes());
    output.extend_from_slice(bytes);
}

fn read_string(bytes: &[u8], offset: &mut usize) -> Option<String> {
    let len_slice = bytes.get(*offset..offset.checked_add(2)?)?;
    let len = u16::from_le_bytes(len_slice.try_into().ok()?);
    *offset = offset.checked_add(2)?;

    let str_len = usize::from(len);
    let str_bytes = bytes.get(*offset..offset.checked_add(str_len)?)?;
    *offset = offset.checked_add(str_len)?;

    String::from_utf8(str_bytes.to_vec()).ok()
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes(slice.try_into().ok()?))
}

fn read_u16_le_offset(bytes: &[u8], offset: &mut usize) -> Option<u16> {
    let slice = bytes.get(*offset..offset.checked_add(2)?)?;
    *offset = offset.checked_add(2)?;
    Some(u16::from_le_bytes(slice.try_into().ok()?))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}

fn read_u32_le_offset(bytes: &[u8], offset: &mut usize) -> Option<u32> {
    let slice = bytes.get(*offset..offset.checked_add(4)?)?;
    *offset = offset.checked_add(4)?;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}
The requested file reference is not currently visible. Use files.search or files.list to rediscover the file, then retry with a returned ref_id or file_id.