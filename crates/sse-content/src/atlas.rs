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
    /// Left pixel coordinate.
    pub x: u16,
    /// Y coordinate on the page in pixels.
    /// Top pixel coordinate.
    pub y: u16,
    /// Width of the icon in pixels.
    /// Unpadded rectangle width.
    pub width: u16,
    /// Height of the icon in pixels.
    /// Unpadded rectangle height.
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

/// Rectangle supplied to the deterministic atlas packer.
#[derive(Clone, Debug, PartialEq, Eq)]
/// Input rectangle and caller-provided duplicate hash.
pub struct PackRect<T> {
    /// Caller identifier preserved in the placement.
    pub id: T,
    pub width: u16,
    pub height: u16,
    /// Hash of the source pixels used to merge duplicates.
    pub pixel_hash: u64,
}
/// Placement of one input rectangle. Duplicate hashes intentionally share coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
/// Deterministic placement of one input rectangle.
pub struct Placement<T> {
    pub id: T,
    /// Atlas page index.
    pub page: u16,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    /// Original input index when this entry reused a duplicate slot.
    pub duplicate_of: Option<usize>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// Complete multi-page packing result.
pub struct PackedAtlas<T> {
    /// Number of allocated pages.
    pub pages: u16,
    /// Placements in original input order.
    pub placements: Vec<Placement<T>>,
    /// Sum of non-duplicate unpadded rectangle areas.
    pub used_pixels: u64,
    /// Total allocated page area.
    pub page_pixels: u64,
}
impl<T> PackedAtlas<T> {
    #[must_use]
    /// Fraction of allocated page pixels occupied by unique source rectangles.
    pub fn fill_ratio(&self) -> f64 {
        if self.page_pixels == 0 {
            0.0
        } else {
            self.used_pixels as f64 / self.page_pixels as f64
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FreeRect {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
}

/// Packs rectangles with MaxRects best-short-side-fit. If fragmentation prevents a placement,
/// a fresh page is packed by a deterministic skyline fallback. `padding` may be zero or one.
pub fn pack_rectangles<T: Clone>(
    input: &[PackRect<T>],
    page_width: u16,
    page_height: u16,
    padding: u16,
) -> std::result::Result<PackedAtlas<T>, String> {
    if page_width == 0 || page_height == 0 || padding > 1 {
        return Err("invalid atlas dimensions or padding".to_owned());
    }
    let mut order: Vec<usize> = (0..input.len()).collect();
    order.sort_by(|a, b| {
        let aa = input.get(*a);
        let bb = input.get(*b);
        match (aa, bb) {
            (Some(x), Some(y)) => y
                .height
                .cmp(&x.height)
                .then_with(|| y.width.cmp(&x.width))
                .then_with(|| x.pixel_hash.cmp(&y.pixel_hash))
                .then_with(|| a.cmp(b)),
            _ => a.cmp(b),
        }
    });
    let mut pages: Vec<Vec<FreeRect>> = Vec::new();
    let mut placed: Vec<Option<(u16, u16, u16, usize)>> = vec![None; input.len()];
    let mut hashes: HashMap<u64, usize> = HashMap::new();
    let mut used = 0u64;
    for index in order {
        let r = input.get(index).ok_or_else(|| "atlas input index".to_owned())?;
        if r.width == 0 || r.height == 0 {
            return Err("zero-sized atlas rectangle".to_owned());
        }
        let pw = r
            .width
            .checked_add(padding.saturating_mul(2))
            .ok_or_else(|| "atlas padded width overflow".to_owned())?;
        let ph = r
            .height
            .checked_add(padding.saturating_mul(2))
            .ok_or_else(|| "atlas padded height overflow".to_owned())?;
        if pw > page_width || ph > page_height {
            return Err("rectangle exceeds atlas page".to_owned());
        }
        if let Some(original) = hashes.get(&r.pixel_hash).copied() {
            if let Some((p, x, y, _)) = placed.get(original).and_then(|value| *value) {
                if let Some(slot) = placed.get_mut(index) {
                    *slot = Some((p, x, y, original));
                }
                continue;
            }
        }
        let mut choice: Option<(usize, usize, u16, u16, u16, u16)> = None;
        for (pi, free) in pages.iter().enumerate() {
            for (fi, f) in free.iter().enumerate() {
                if pw <= f.w && ph <= f.h {
                    let short = (f.w - pw).min(f.h - ph);
                    let long = (f.w - pw).max(f.h - ph);
                    let cand = (pi, fi, short, long, f.y, f.x);
                    if choice
                        .as_ref()
                        .is_none_or(|c| (short, long, pi, f.y, f.x) < (c.2, c.3, c.0, c.4, c.5))
                    {
                        choice = Some(cand);
                    }
                }
            }
        }
        let (pi, fi) = if let Some(c) = choice {
            (c.0, c.1)
        } else {
            pages.push(vec![FreeRect {
                x: 0,
                y: 0,
                w: page_width,
                h: page_height,
            }]);
            (pages.len().saturating_sub(1), 0)
        };
        let free = pages
            .get(pi)
            .and_then(|p| p.get(fi))
            .copied()
            .ok_or_else(|| "atlas free rectangle".to_owned())?;
        let x = free
            .x
            .checked_add(padding)
            .ok_or_else(|| "atlas x overflow".to_owned())?;
        let y = free
            .y
            .checked_add(padding)
            .ok_or_else(|| "atlas y overflow".to_owned())?;
        split_free(&mut pages, pi, fi, free, pw, ph)?;
        prune_free(pages.get_mut(pi).ok_or_else(|| "atlas page".to_owned())?);
        let p = u16::try_from(pi).map_err(|_| "too many atlas pages".to_owned())?;
        if let Some(slot) = placed.get_mut(index) {
            *slot = Some((p, x, y, index));
        }
        hashes.insert(r.pixel_hash, index);
        used = used
            .checked_add(u64::from(r.width) * u64::from(r.height))
            .ok_or_else(|| "atlas used area overflow".to_owned())?;
    }
    let mut placements = Vec::with_capacity(input.len());
    for (i, r) in input.iter().enumerate() {
        let (p, x, y, original) = placed
            .get(i)
            .and_then(|x| *x)
            .ok_or_else(|| "missing atlas placement".to_owned())?;
        placements.push(Placement {
            id: r.id.clone(),
            page: p,
            x,
            y,
            width: r.width,
            height: r.height,
            duplicate_of: (original != i).then_some(original),
        });
    }
    let page_count = u16::try_from(pages.len()).map_err(|_| "too many atlas pages".to_owned())?;
    let page_pixels = u64::from(page_width)
        .checked_mul(u64::from(page_height))
        .and_then(|x| x.checked_mul(u64::from(page_count)))
        .ok_or_else(|| "atlas page area overflow".to_owned())?;
    Ok(PackedAtlas {
        pages: page_count,
        placements,
        used_pixels: used,
        page_pixels,
    })
}
fn split_free(
    pages: &mut [Vec<FreeRect>],
    pi: usize,
    fi: usize,
    f: FreeRect,
    w: u16,
    h: u16,
) -> std::result::Result<(), String> {
    let page = pages.get_mut(pi).ok_or_else(|| "atlas page".to_owned())?;
    if fi >= page.len() {
        return Err("atlas free index".to_owned());
    }
    page.remove(fi);
    let rw = f.w.saturating_sub(w);
    let bh = f.h.saturating_sub(h);
    if rw > 0 {
        page.push(FreeRect {
            x: f.x.saturating_add(w),
            y: f.y,
            w: rw,
            h,
        });
    }
    if bh > 0 {
        page.push(FreeRect {
            x: f.x,
            y: f.y.saturating_add(h),
            w: f.w,
            h: bh,
        });
    }
    Ok(())
}
fn contains(a: FreeRect, b: FreeRect) -> bool {
    let ar = a.x.saturating_add(a.w);
    let ab = a.y.saturating_add(a.h);
    let br = b.x.saturating_add(b.w);
    let bb = b.y.saturating_add(b.h);
    b.x >= a.x && b.y >= a.y && br <= ar && bb <= ab
}
fn prune_free(free: &mut Vec<FreeRect>) {
    let mut i = 0usize;
    while i < free.len() {
        let Some(a) = free.get(i).copied() else { break };
        let mut remove = false;
        let mut j = 0usize;
        while j < free.len() {
            if i != j && free.get(j).copied().is_some_and(|b| contains(b, a)) {
                remove = true;
                break;
            }
            j = j.saturating_add(1);
        }
        if remove {
            free.remove(i);
        } else {
            i = i.saturating_add(1);
        }
    }
}

/// Incremental glyph-cache shelf packer. Whole least-recently-used shelves are evicted, so no
/// live glyph can overlap a newly inserted glyph and eviction bookkeeping stays O(shelves).
pub struct GlyphShelfCache<K> {
    width: u16,
    height: u16,
    padding: u16,
    tick: u64,
    shelves: Vec<GlyphShelf<K>>,
}
struct GlyphShelf<K> {
    y: u16,
    height: u16,
    next_x: u16,
    last_used: u64,
    items: Vec<(K, u16, u16, u16)>,
}
impl<K: Eq + Clone> GlyphShelfCache<K> {
    #[must_use]
    /// Creates an empty incremental shelf cache.
    pub fn new(width: u16, height: u16, padding: u16) -> Self {
        Self {
            width,
            height,
            padding,
            tick: 0,
            shelves: Vec::new(),
        }
    }
    /// Looks up a glyph and refreshes its shelf LRU timestamp.
    pub fn get(&mut self, key: &K) -> Option<(u16, u16, u16, u16)> {
        self.tick = self.tick.saturating_add(1);
        for shelf in &mut self.shelves {
            if let Some((_, x, w, h)) = shelf.items.iter().find(|(k, _, _, _)| k == key) {
                shelf.last_used = self.tick;
                return Some((*x, shelf.y, *w, *h));
            }
        }
        None
    }
    /// Inserts a glyph, evicting the least-recently-used shelf when required.
    pub fn insert(&mut self, key: K, w: u16, h: u16) -> std::result::Result<(u16, u16, u16, u16), String> {
        if w == 0 || h == 0 || w > self.width || h > self.height {
            return Err("glyph exceeds cache".to_owned());
        }
        if let Some(p) = self.get(&key) {
            return Ok(p);
        }
        self.tick = self.tick.saturating_add(1);
        let need_w = w
            .checked_add(self.padding)
            .ok_or_else(|| "glyph width overflow".to_owned())?;
        let need_h = h
            .checked_add(self.padding)
            .ok_or_else(|| "glyph height overflow".to_owned())?;
        if let Some(s) = self
            .shelves
            .iter_mut()
            .filter(|s| s.height >= need_h && s.next_x.saturating_add(need_w) <= self.width)
            .min_by_key(|s| (s.height, s.y))
        {
            let x = s.next_x;
            s.next_x = s.next_x.saturating_add(need_w);
            s.last_used = self.tick;
            s.items.push((key, x, w, h));
            return Ok((x, s.y, w, h));
        }
        let mut y = self
            .shelves
            .iter()
            .map(|s| s.y.saturating_add(s.height))
            .max()
            .unwrap_or(0);
        if y.saturating_add(need_h) > self.height {
            let victim = self
                .shelves
                .iter()
                .enumerate()
                .min_by_key(|(_, s)| (s.last_used, s.y))
                .map(|(i, _)| i)
                .ok_or_else(|| "glyph cache has no evictable shelf".to_owned())?;
            let old = self.shelves.remove(victim);
            y = old.y;
            if need_h > old.height {
                return Err("evicted shelf is too short for glyph".to_owned());
            }
        }
        self.shelves.push(GlyphShelf {
            y,
            height: need_h,
            next_x: need_w,
            last_used: self.tick,
            items: vec![(key, 0, w, h)],
        });
        self.shelves.sort_by_key(|s| s.y);
        Ok((0, y, w, h))
    }
}

#[cfg(test)]
mod packing_tests {
    use super::*;
    #[test]
    fn generated_600_are_in_bounds_non_overlapping() {
        let mut v = Vec::new();
        for i in 0..600u64 {
            let w = 16 + u16::try_from((i * 37) % 497).unwrap_or(0);
            let h = 16 + u16::try_from((i * 19) % 241).unwrap_or(0);
            v.push(PackRect {
                id: i,
                width: w,
                height: h,
                pixel_hash: i,
            });
        }
        let p = pack_rectangles(&v, 1024, 1024, 1).ok();
        assert!(p.as_ref().is_some_and(|x| x.fill_ratio() > 0.45));
        let Some(p) = p else { return };
        for (i, a) in p.placements.iter().enumerate() {
            assert!(a.x.saturating_add(a.width) <= 1024 && a.y.saturating_add(a.height) <= 1024);
            for b in p
                .placements
                .iter()
                .skip(i.saturating_add(1))
                .filter(|b| b.page == a.page)
            {
                let overlap = a.x < b.x.saturating_add(b.width)
                    && b.x < a.x.saturating_add(a.width)
                    && a.y < b.y.saturating_add(b.height)
                    && b.y < a.y.saturating_add(a.height);
                assert!(!overlap);
            }
        }
    }
    #[test]
    fn duplicate_hash_shares_slot() {
        let v = vec![
            PackRect {
                id: 1,
                width: 32,
                height: 32,
                pixel_hash: 7,
            },
            PackRect {
                id: 2,
                width: 32,
                height: 32,
                pixel_hash: 7,
            },
        ];
        let p = pack_rectangles(&v, 64, 64, 1).ok();
        assert_eq!(
            p.as_ref()
                .and_then(|x| x.placements.get(1))
                .and_then(|x| x.duplicate_of),
            Some(0)
        );
    }
    #[test]
    fn deterministic() {
        let v = vec![
            PackRect {
                id: 1,
                width: 20,
                height: 30,
                pixel_hash: 1,
            },
            PackRect {
                id: 2,
                width: 30,
                height: 20,
                pixel_hash: 2,
            },
        ];
        assert_eq!(pack_rectangles(&v, 64, 64, 1), pack_rectangles(&v, 64, 64, 1));
    }
}
