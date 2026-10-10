//! Software raster primitives for `sse-ui`.
//!
//! The destination is a premultiplied BGRA8 surface stored as `u32`: on little-endian targets the
//! in-memory byte order of `0xAARRGGBB` is B, G, R, A. Colours are composited in sRGB byte space.
//! This is deliberate: the UI palette, glyph masks and decoded thumbnails are stored in sRGB, and
//! byte-space compositing keeps the hot loop integer-only and deterministic. Linear-light blending
//! would need transfer-function conversion for every touched pixel and is a poor trade for this
//! software UI; image resampling still operates on premultiplied channels, so transparent edges do
//! not acquire colour fringes.
//!
//! Rounded-rectangle antialiasing is analytic. A pixel starts as its exact rectangle intersection;
//! each rounded corner subtracts the part of its corner square outside the quarter circle. The
//! circle/rectangle intersection is integrated from the closed-form circle-segment primitive, so
//! there is no supersampling. Image downscaling uses exact source-pixel overlap weights.

use sse_core::{Error, Result};

const MAX_SOLID_PIXEL_SCAN: usize = 262_144;

// This const generator is the one place where direct indexing is needed: both counters are
// initialized to zero and loop strictly below the corresponding 256-element dimension.
#[allow(clippy::indexing_slicing)]
const fn make_scale_lut() -> [[u8; 256]; 256] {
    let mut table = [[0_u8; 256]; 256];
    let mut scale = 0_usize;
    while scale < 256 {
        let mut channel = 0_usize;
        while channel < 256 {
            let product = channel.saturating_mul(scale).saturating_add(127);
            let rounded = product.saturating_add(product >> 8).saturating_add(1);
            // The loop bounds and `(channel * scale + 127) / 255` keep this result in 0..=255.
            #[allow(clippy::cast_possible_truncation)]
            {
                table[scale][channel] = (rounded >> 8) as u8;
            }
            channel = channel.saturating_add(1);
        }
        scale = scale.saturating_add(1);
    }
    table
}

static SCALE_LUT: [[u8; 256]; 256] = make_scale_lut();
static ZERO_SCALE: [u8; 256] = [0; 256];
const MAX_COORDINATE: u32 = 2_147_483_647;

/// An integer rectangle in surface coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    /// Left edge in pixels.
    pub x: i32,
    /// Top edge in pixels.
    pub y: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl Rect {
    /// Creates a rectangle.
    #[must_use]
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    fn right(self) -> i64 {
        i64::from(self.x).saturating_add(i64::from(self.width))
    }

    fn bottom(self) -> i64 {
        i64::from(self.y).saturating_add(i64::from(self.height))
    }
}

/// Circular radius for every rounded-rectangle corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radii {
    /// Top-left radius.
    pub top_left: f64,
    /// Top-right radius.
    pub top_right: f64,
    /// Bottom-right radius.
    pub bottom_right: f64,
    /// Bottom-left radius.
    pub bottom_left: f64,
}

impl Radii {
    /// No rounded corners.
    pub const ZERO: Self = Self {
        top_left: 0.0,
        top_right: 0.0,
        bottom_right: 0.0,
        bottom_left: 0.0,
    };

    /// Uses the same radius on all corners.
    #[must_use]
    pub const fn all(radius: f64) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    fn normalised(self, width: f64, height: f64) -> Self {
        let mut radii = Self {
            top_left: finite_non_negative(self.top_left),
            top_right: finite_non_negative(self.top_right),
            bottom_right: finite_non_negative(self.bottom_right),
            bottom_left: finite_non_negative(self.bottom_left),
        };
        let top = radii.top_left + radii.top_right;
        let bottom = radii.bottom_left + radii.bottom_right;
        let left = radii.top_left + radii.bottom_left;
        let right = radii.top_right + radii.bottom_right;
        let mut scale = 1.0_f64;
        if top > width && top > 0.0 {
            scale = scale.min(width / top);
        }
        if bottom > width && bottom > 0.0 {
            scale = scale.min(width / bottom);
        }
        if left > height && left > 0.0 {
            scale = scale.min(height / left);
        }
        if right > height && right > 0.0 {
            scale = scale.min(height / right);
        }
        radii.top_left *= scale;
        radii.top_right *= scale;
        radii.bottom_right *= scale;
        radii.bottom_left *= scale;
        radii
    }
}

/// A premultiplied BGRA8 colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    b: u8,
    g: u8,
    r: u8,
    a: u8,
}

impl Color {
    /// Constructs a colour from straight (unpremultiplied) RGBA channels.
    #[must_use]
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            b: premultiply(b, a),
            g: premultiply(g, a),
            r: premultiply(r, a),
            a,
        }
    }

    /// Constructs a colour from already-premultiplied BGRA channels.
    #[must_use]
    pub const fn premultiplied_bgra(b: u8, g: u8, r: u8, a: u8) -> Self {
        Self { b, g, r, a }
    }

    /// Encodes the colour as `0xAARRGGBB`.
    #[must_use]
    pub fn to_u32(self) -> u32 {
        u32::from(self.b)
            | u32::from(self.g).wrapping_shl(8)
            | u32::from(self.r).wrapping_shl(16)
            | u32::from(self.a).wrapping_shl(24)
    }

    /// Decodes a premultiplied `0xAARRGGBB` pixel.
    #[must_use]
    pub fn from_u32(pixel: u32) -> Self {
        Self {
            b: byte(pixel),
            g: byte(pixel.wrapping_shr(8)),
            r: byte(pixel.wrapping_shr(16)),
            a: byte(pixel.wrapping_shr(24)),
        }
    }
}

/// Axis for a two-colour linear gradient.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientAxis {
    /// Colour changes from the left edge to the right edge.
    Horizontal,
    /// Colour changes from the top edge to the bottom edge.
    Vertical,
}

/// A two-colour gradient.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gradient {
    /// Colour at the start of the axis.
    pub start: Color,
    /// Colour at the end of the axis.
    pub end: Color,
    /// Direction of interpolation.
    pub axis: GradientAxis,
}

/// Image resampling filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFilter {
    /// Bilinear interpolation of premultiplied channels.
    Bilinear,
    /// Exact source-pixel area coverage. Best for thumbnails and other downscales.
    Box,
}

/// A borrowed premultiplied BGRA image.
#[derive(Clone, Copy, Debug)]
pub struct ImageRef<'a> {
    pixels: &'a [u32],
    width: u32,
    height: u32,
    stride: usize,
    solid_pixel: Option<u32>,
    opaque: bool,
}

impl<'a> ImageRef<'a> {
    /// Validates and borrows an image.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for an invalid stride or a buffer shorter than its dimensions.
    pub fn new(pixels: &'a [u32], width: u32, height: u32, stride: usize) -> Result<Self> {
        validate_plane(pixels.len(), width, height, stride, "image")?;
        let solid_pixel = detect_solid_pixel(pixels, width, height, stride);
        let opaque = detect_opaque(pixels, width, height, stride);
        Ok(Self {
            pixels,
            width,
            height,
            stride,
            solid_pixel,
            opaque,
        })
    }

    /// Whether every pixel has full alpha.
    #[must_use]
    pub const fn is_opaque(self) -> bool {
        self.opaque
    }

    fn pixel(self, x: u32, y: u32) -> u32 {
        plane_offset(x, y, self.stride)
            .and_then(|offset| self.pixels.get(offset).copied())
            .unwrap_or_default()
    }
}

/// A borrowed 8-bit coverage mask.
#[derive(Clone, Copy, Debug)]
pub struct MaskRef<'a> {
    pixels: &'a [u8],
    width: u32,
    height: u32,
    stride: usize,
}

impl<'a> MaskRef<'a> {
    /// Validates and borrows a mask.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for an invalid stride or a buffer shorter than its dimensions.
    pub fn new(pixels: &'a [u8], width: u32, height: u32, stride: usize) -> Result<Self> {
        validate_plane(pixels.len(), width, height, stride, "mask")?;
        Ok(Self {
            pixels,
            width,
            height,
            stride,
        })
    }
}

/// A clipped mutable drawing surface.
pub struct Surface<'a> {
    pixels: &'a mut [u32],
    width: u32,
    height: u32,
    stride: usize,
    clip: Rect,
}

impl<'a> Surface<'a> {
    /// Validates a BGRA surface and intersects `clip` with its bounds.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for an invalid stride, oversized dimensions or a short buffer.
    pub fn new(pixels: &'a mut [u32], width: u32, height: u32, stride: usize, clip: Rect) -> Result<Self> {
        if width > MAX_COORDINATE || height > MAX_COORDINATE {
            return Err(Error::damaged("raster surface dimensions exceed i32 coordinates"));
        }
        validate_plane(pixels.len(), width, height, stride, "surface")?;
        let clip = clip_to_surface(clip, width, height);
        Ok(Self {
            pixels,
            width,
            height,
            stride,
            clip,
        })
    }

    /// Returns the surface dimensions.
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Returns the current clip rectangle after intersection with the surface.
    #[must_use]
    pub const fn clip(&self) -> Rect {
        self.clip
    }

    /// Replaces the active clip with the intersection of `clip` and the surface bounds.
    /// Returns the previous clip so callers can restore nested viewport clipping.
    pub fn replace_clip(&mut self, clip: Rect) -> Rect {
        let previous = self.clip;
        self.clip = clip_to_surface(clip, self.width, self.height);
        previous
    }

    /// Solid rounded-rectangle fill with analytic edge coverage.
    pub fn fill_rect(&mut self, rect: Rect, radii: Radii, color: Color) {
        let bounds = intersect_rect(rect, self.clip);
        if bounds.width == 0 || bounds.height == 0 || color.a == 0 {
            return;
        }
        let rounded = radii.normalised(f64::from(rect.width), f64::from(rect.height));
        if rounded == Radii::ZERO {
            self.fill_solid_rect(bounds, color.to_u32());
            return;
        }
        let pixel = color.to_u32();
        self.paint_shape(bounds, rect, rounded, Some(pixel), |_, _| pixel);
    }

    /// Draws an inward border of any finite non-negative width.
    pub fn border(&mut self, rect: Rect, radii: Radii, width: f64, color: Color) {
        if !width.is_finite() || width <= 0.0 || color.a == 0 {
            return;
        }
        let bounds = intersect_rect(rect, self.clip);
        if bounds.width == 0 || bounds.height == 0 {
            return;
        }
        let outer = rect_to_f64(rect);
        let outer_radii = radii.normalised(outer.width(), outer.height());
        let inner = outer.inset(width);
        let inner_radii = Radii {
            top_left: (outer_radii.top_left - width).max(0.0),
            top_right: (outer_radii.top_right - width).max(0.0),
            bottom_right: (outer_radii.bottom_right - width).max(0.0),
            bottom_left: (outer_radii.bottom_left - width).max(0.0),
        }
        .normalised(inner.width().max(0.0), inner.height().max(0.0));

        let source = color.to_u32();
        let top_depth = border_depth(width, outer_radii.top_left, outer_radii.top_right);
        let bottom_depth = border_depth(width, outer_radii.bottom_left, outer_radii.bottom_right);
        let left_depth = border_depth(width, outer_radii.top_left, outer_radii.bottom_left);
        let right_depth = border_depth(width, outer_radii.top_right, outer_radii.bottom_right);
        let top_end = rect
            .y
            .saturating_add(i32::try_from(top_depth.min(rect.height)).unwrap_or(i32::MAX));
        let bottom_start = rect.bottom().saturating_sub(i64::from(bottom_depth.min(rect.height)));
        let middle_top = i64::from(top_end).min(rect.bottom());
        let middle_bottom = bottom_start.max(middle_top).min(rect.bottom());

        let paint = |x: i32, y: i32, pixel: &mut u32| {
            let outer_coverage = rounded_coverage(outer, outer_radii, x, y);
            let inner_coverage = if inner.is_empty() {
                0.0
            } else {
                rounded_coverage(inner, inner_radii, x, y)
            };
            let coverage = coverage_byte((outer_coverage - inner_coverage).clamp(0.0, 1.0));
            if coverage != 0 {
                *pixel = blend_covered(*pixel, source, coverage);
            }
        };

        let top_bounds = intersect_rect(
            Rect::new(rect.x, rect.y, rect.width, top_depth.min(rect.height)),
            bounds,
        );
        self.for_each_pixel(top_bounds, paint);

        let bottom_height = u32::try_from(rect.bottom().saturating_sub(middle_bottom))
            .unwrap_or_default()
            .min(rect.height);
        let bottom_y = i32::try_from(middle_bottom).unwrap_or(rect.y);
        let bottom_bounds = intersect_rect(Rect::new(rect.x, bottom_y, rect.width, bottom_height), bounds);
        self.for_each_pixel(bottom_bounds, paint);

        let middle_height = u32::try_from(middle_bottom.saturating_sub(middle_top)).unwrap_or_default();
        if middle_height != 0 {
            let middle_y = i32::try_from(middle_top).unwrap_or(rect.y);
            let left_bounds = intersect_rect(
                Rect::new(rect.x, middle_y, left_depth.min(rect.width), middle_height),
                bounds,
            );
            self.for_each_pixel(left_bounds, paint);
            let right_width = right_depth.min(rect.width);
            let right_x = i32::try_from(rect.right().saturating_sub(i64::from(right_width))).unwrap_or(rect.x);
            let right_bounds = intersect_rect(Rect::new(right_x, middle_y, right_width, middle_height), bounds);
            self.for_each_pixel(right_bounds, paint);
        }
    }

    /// Blits an 8-bit coverage mask using `color`.
    pub fn blit_mask(&mut self, mask: MaskRef<'_>, x: i32, y: i32, color: Color) {
        if color.a == 0 || mask.width == 0 || mask.height == 0 {
            return;
        }
        let destination = Rect::new(x, y, mask.width, mask.height);
        let bounds = intersect_rect(destination, self.clip);
        let source = color.to_u32();
        if color.b == 0 && color.g == 0 && color.r == 0 {
            self.blit_black_mask(mask, destination, bounds, color.a);
            return;
        }
        self.blit_coverage_mask(mask, destination, bounds, source);
    }

    /// Scales and blits a premultiplied BGRA image into `destination`.
    ///
    /// [`ImageFilter::Box`] computes exact source-pixel overlap and is intended for downscaling;
    /// [`ImageFilter::Bilinear`] is the faster general scaler.
    /// Draws an opaque image of the destination's size one-to-one by copying rows; any other image goes through
    /// [`Surface::blit_image`] with bilinear filtering.
    pub fn blit_opaque_copy(&mut self, image: ImageRef<'_>, destination: Rect) {
        if !image.opaque || image.width != destination.width || image.height != destination.height {
            self.blit_image(image, destination, ImageFilter::Bilinear);
            return;
        }
        let bounds = intersect_rect(destination, self.clip);
        let Some((start_y, end_y)) = rect_y_range(bounds) else {
            return;
        };
        let (Ok(width), Ok(start_x)) = (usize::try_from(bounds.width), usize::try_from(bounds.x)) else {
            return;
        };
        let source_x = usize::try_from(coordinate_offset(bounds.x, destination.x)).unwrap_or(0);
        for y in start_y..end_y {
            let source_y =
                usize::try_from(coordinate_offset(i32::try_from(y).unwrap_or(i32::MAX), destination.y)).unwrap_or(0);
            let Some(source) = source_y
                .checked_mul(image.stride)
                .and_then(|row| row.checked_add(source_x))
                .and_then(|start| image.pixels.get(start..start.checked_add(width)?))
            else {
                continue;
            };
            let Some(target) = y
                .checked_mul(self.stride)
                .and_then(|row| row.checked_add(start_x))
                .and_then(|start| self.pixels.get_mut(start..start.checked_add(width)?))
            else {
                continue;
            };
            target.copy_from_slice(source);
        }
    }

    /// Draws an image with the given filter.
    pub fn blit_image(&mut self, image: ImageRef<'_>, destination: Rect, filter: ImageFilter) {
        if image.width == 0 || image.height == 0 || destination.width == 0 || destination.height == 0 {
            return;
        }
        let bounds = intersect_rect(destination, self.clip);
        if let Some(source) = image.solid_pixel {
            self.fill_solid_rect(bounds, source);
            return;
        }
        match filter {
            ImageFilter::Bilinear => self.for_each_pixel(bounds, |surface_x, surface_y, pixel| {
                let destination_x = coordinate_offset(surface_x, destination.x);
                let destination_y = coordinate_offset(surface_y, destination.y);
                let source = sample_bilinear(
                    image,
                    destination_x,
                    destination_y,
                    destination.width,
                    destination.height,
                );
                *pixel = blend_covered(*pixel, source, u8::MAX);
            }),
            ImageFilter::Box => self.blit_box_image(image, destination, bounds),
        }
    }

    /// Applies a black dimming overlay to the current clip.
    pub fn dim(&mut self, alpha: u8) {
        if alpha == 0 {
            return;
        }
        let Some((start_y, end_y)) = rect_y_range(self.clip) else {
            return;
        };
        let Some(start_x) = usize::try_from(self.clip.x).ok() else {
            return;
        };
        let Some(row_width) = usize::try_from(self.clip.width).ok() else {
            return;
        };
        let Some(end_x) = start_x.checked_add(row_width) else {
            return;
        };
        let inverse_alpha = u8::MAX.saturating_sub(alpha);
        let row_count = end_y.saturating_sub(start_y);
        let pixel_count = row_count.saturating_mul(row_width);
        let workers = raster_worker_count(pixel_count);
        for_each_row_mut_parallel(
            self.pixels,
            usize::try_from(self.width).unwrap_or_default(),
            self.stride,
            start_y,
            end_y,
            workers,
            |_, full_row| {
                let Some(row) = full_row.get_mut(start_x..end_x) else {
                    return;
                };
                let mut chunks = row.chunks_exact_mut(16);
                for chunk in &mut chunks {
                    dim_chunk_arithmetic(chunk, alpha, inverse_alpha);
                }
                dim_chunk_arithmetic(chunks.into_remainder(), alpha, inverse_alpha);
            },
        );
    }

    fn fill_solid_rect(&mut self, bounds: Rect, source: u32) {
        if pixel_alpha(source) == u8::MAX {
            self.fill_opaque(bounds, source);
            return;
        }
        let covered = CoveredSource::new(source, u8::MAX);
        self.for_each_pixel(bounds, |_, _, pixel| {
            *pixel = blend_prepared(*pixel, covered);
        });
    }

    fn blit_coverage_mask(&mut self, mask: MaskRef<'_>, destination: Rect, bounds: Rect, source: u32) {
        let Some((start_y, end_y)) = rect_y_range(bounds) else {
            return;
        };
        let Some(destination_x) = usize::try_from(bounds.x).ok() else {
            return;
        };
        let Some(row_width) = usize::try_from(bounds.width).ok() else {
            return;
        };
        let Some(destination_end_x) = destination_x.checked_add(row_width) else {
            return;
        };
        let source_x = coordinate_offset(bounds.x, destination.x);
        let Some(source_start_x) = usize::try_from(source_x).ok() else {
            return;
        };
        let Some(source_end_x) = source_start_x.checked_add(row_width) else {
            return;
        };
        let mut surface_y = bounds.y;
        for y in start_y..end_y {
            let source_y = coordinate_offset(surface_y, destination.y);
            let Some(source_row_start) = usize::try_from(source_y)
                .ok()
                .and_then(|row| row.checked_mul(mask.stride))
            else {
                return;
            };
            let Some(source_start) = source_row_start.checked_add(source_start_x) else {
                return;
            };
            let Some(source_end) = source_row_start.checked_add(source_end_x) else {
                return;
            };
            let Some(source_row) = mask.pixels.get(source_start..source_end) else {
                return;
            };
            let Some(destination_row_start) = y.checked_mul(self.stride) else {
                return;
            };
            let Some(destination_start) = destination_row_start.checked_add(destination_x) else {
                return;
            };
            let Some(destination_end) = destination_row_start.checked_add(destination_end_x) else {
                return;
            };
            let Some(destination_row) = self.pixels.get_mut(destination_start..destination_end) else {
                return;
            };
            let mut destination_chunks = destination_row.chunks_exact_mut(8);
            let mut source_chunks = source_row.chunks_exact(8);
            for (destination_chunk, source_chunk) in destination_chunks.by_ref().zip(source_chunks.by_ref()) {
                for (destination_pixel, coverage) in destination_chunk.iter_mut().zip(source_chunk) {
                    *destination_pixel = blend_covered(*destination_pixel, source, *coverage);
                }
            }
            for (destination_pixel, coverage) in destination_chunks
                .into_remainder()
                .iter_mut()
                .zip(source_chunks.remainder())
            {
                *destination_pixel = blend_covered(*destination_pixel, source, *coverage);
            }
            surface_y = surface_y.saturating_add(1);
        }
    }

    fn blit_black_mask(&mut self, mask: MaskRef<'_>, destination: Rect, bounds: Rect, alpha: u8) {
        let Some((start_y, end_y)) = rect_y_range(bounds) else {
            return;
        };
        let Some(destination_x) = usize::try_from(bounds.x).ok() else {
            return;
        };
        let Some(row_width) = usize::try_from(bounds.width).ok() else {
            return;
        };
        let Some(destination_end_x) = destination_x.checked_add(row_width) else {
            return;
        };
        let source_x = coordinate_offset(bounds.x, destination.x);
        let Some(source_start_x) = usize::try_from(source_x).ok() else {
            return;
        };
        let Some(source_end_x) = source_start_x.checked_add(row_width) else {
            return;
        };
        let mut inverse_by_coverage = [0_u8; 256];
        for (coverage, inverse_alpha) in inverse_by_coverage.iter_mut().enumerate() {
            let source_alpha = scale_byte(scale_row(alpha), u8::try_from(coverage).unwrap_or_default());
            *inverse_alpha = u8::MAX.saturating_sub(source_alpha);
        }
        let full_coverage_scale = scale_row(u8::MAX.saturating_sub(alpha));
        let mut surface_y = bounds.y;
        for y in start_y..end_y {
            let source_y = coordinate_offset(surface_y, destination.y);
            let Some(source_row_start) = usize::try_from(source_y)
                .ok()
                .and_then(|row| row.checked_mul(mask.stride))
            else {
                return;
            };
            let Some(source_start) = source_row_start.checked_add(source_start_x) else {
                return;
            };
            let Some(source_end) = source_row_start.checked_add(source_end_x) else {
                return;
            };
            let Some(source_row) = mask.pixels.get(source_start..source_end) else {
                return;
            };
            let Some(destination_row_start) = y.checked_mul(self.stride) else {
                return;
            };
            let Some(destination_start) = destination_row_start.checked_add(destination_x) else {
                return;
            };
            let Some(destination_end) = destination_row_start.checked_add(destination_end_x) else {
                return;
            };
            let Some(destination_row) = self.pixels.get_mut(destination_start..destination_end) else {
                return;
            };
            blend_black_mask_row(
                destination_row,
                source_row,
                alpha,
                &inverse_by_coverage,
                full_coverage_scale,
            );
            surface_y = surface_y.saturating_add(1);
        }
    }

    fn blit_box_image(&mut self, image: ImageRef<'_>, destination: Rect, bounds: Rect) {
        let Some((start_y, end_y)) = rect_y_range(bounds) else {
            return;
        };
        let Some(start_x) = usize::try_from(bounds.x).ok() else {
            return;
        };
        let Some(row_width) = usize::try_from(bounds.width).ok() else {
            return;
        };
        let Some(end_x) = start_x.checked_add(row_width) else {
            return;
        };
        let first_destination_x = coordinate_offset(bounds.x, destination.x);
        let denominator = u128::from(image.width).saturating_mul(u128::from(image.height));
        let use_u64 = denominator <= u128::from(u64::MAX / 255);
        let stride = self.stride;

        for block_start in (0..row_width).step_by(8) {
            let block_width = row_width.saturating_sub(block_start).min(8);
            let mut columns = [BoxAxisWeights::default(); 8];
            for (column_index, column) in columns.iter_mut().take(block_width).enumerate() {
                let position = first_destination_x
                    .saturating_add(u32::try_from(block_start.saturating_add(column_index)).unwrap_or_default());
                *column = BoxAxisWeights::new(position, image.width, destination.width);
            }

            for y in start_y..end_y {
                let Some(surface_y) = i32::try_from(y).ok() else {
                    return;
                };
                let destination_y = coordinate_offset(surface_y, destination.y);
                let rows = BoxAxisWeights::new(destination_y, image.height, destination.height);
                let Some(row_start) = y.checked_mul(stride) else {
                    return;
                };
                let Some(start) = row_start.checked_add(start_x) else {
                    return;
                };
                let Some(end) = row_start.checked_add(end_x) else {
                    return;
                };
                let Some(row) = self.pixels.get_mut(start..end) else {
                    return;
                };
                let Some(block) = row.get_mut(block_start..block_start.saturating_add(block_width)) else {
                    return;
                };
                for (destination, column) in block.iter_mut().zip(columns.iter().take(block_width)) {
                    let source = if use_u64 {
                        sample_box_u64(image, *column, rows, denominator)
                    } else {
                        sample_box_u128(image, *column, rows, denominator)
                    };
                    *destination = blend_covered(*destination, source, u8::MAX);
                }
            }
        }
    }

    fn paint_shape(
        &mut self,
        bounds: Rect,
        rect: Rect,
        radii: Radii,
        solid_pixel: Option<u32>,
        mut colour_at: impl FnMut(i32, i32) -> u32,
    ) {
        let shape = rect_to_f64(rect);
        let Some((start_y, end_y)) = rect_y_range(bounds) else {
            return;
        };
        let Some(start_x) = usize::try_from(bounds.x).ok() else {
            return;
        };
        let Some(row_width) = usize::try_from(bounds.width).ok() else {
            return;
        };
        let Some(end_x) = start_x.checked_add(row_width) else {
            return;
        };
        let clip_left = i64::from(bounds.x);
        let clip_right = bounds.right();
        let mut surface_y = bounds.y;
        for y in start_y..end_y {
            let Some(row_start) = y.checked_mul(self.stride) else {
                return;
            };
            let Some(start) = row_start.checked_add(start_x) else {
                return;
            };
            let Some(end) = row_start.checked_add(end_x) else {
                return;
            };
            let Some(row) = self.pixels.get_mut(start..end) else {
                return;
            };
            let (full_start, full_end) = shape_row_full_span(rect, radii, surface_y);
            let full_start = full_start.clamp(clip_left, clip_right);
            let full_end = full_end.clamp(clip_left, clip_right);
            if full_start >= full_end {
                paint_exact_segment(row, bounds.x, surface_y, shape, radii, &mut colour_at);
            } else {
                let left_width = usize::try_from(full_start.saturating_sub(clip_left)).unwrap_or_default();
                let middle_width = usize::try_from(full_end.saturating_sub(full_start)).unwrap_or_default();
                let (left, remaining) = row.split_at_mut(left_width);
                let (middle, right) = remaining.split_at_mut(middle_width);
                paint_exact_segment(left, bounds.x, surface_y, shape, radii, &mut colour_at);
                if let Some(source) = solid_pixel {
                    if pixel_alpha(source) == u8::MAX {
                        middle.fill(source);
                    } else {
                        let source = CoveredSource::new(source, u8::MAX);
                        for pixel in middle {
                            *pixel = blend_prepared(*pixel, source);
                        }
                    }
                } else {
                    paint_full_segment(
                        middle,
                        i32::try_from(full_start).unwrap_or(bounds.x),
                        surface_y,
                        &mut colour_at,
                    );
                }
                paint_exact_segment(
                    right,
                    i32::try_from(full_end).unwrap_or(bounds.x),
                    surface_y,
                    shape,
                    radii,
                    &mut colour_at,
                );
            }
            surface_y = surface_y.saturating_add(1);
        }
    }

    fn fill_opaque(&mut self, bounds: Rect, pixel: u32) {
        let Some((start_y, end_y)) = rect_y_range(bounds) else {
            return;
        };
        let Some(start_x) = usize::try_from(bounds.x).ok() else {
            return;
        };
        let Some(width) = usize::try_from(bounds.width).ok() else {
            return;
        };
        let Some(end_x) = start_x.checked_add(width) else {
            return;
        };
        for y in start_y..end_y {
            let Some(row_start) = y.checked_mul(self.stride) else {
                return;
            };
            let Some(start) = row_start.checked_add(start_x) else {
                return;
            };
            let Some(end) = row_start.checked_add(end_x) else {
                return;
            };
            if let Some(row) = self.pixels.get_mut(start..end) {
                row.fill(pixel);
            }
        }
    }

    fn for_each_pixel(&mut self, bounds: Rect, mut function: impl FnMut(i32, i32, &mut u32)) {
        let Some((start_y, end_y)) = rect_y_range(bounds) else {
            return;
        };
        let Some(start_x) = usize::try_from(bounds.x).ok() else {
            return;
        };
        let Some(width) = usize::try_from(bounds.width).ok() else {
            return;
        };
        let Some(end_x) = start_x.checked_add(width) else {
            return;
        };
        let mut surface_y = bounds.y;
        for y in start_y..end_y {
            let Some(row_start) = y.checked_mul(self.stride) else {
                return;
            };
            let Some(start) = row_start.checked_add(start_x) else {
                return;
            };
            let Some(end) = row_start.checked_add(end_x) else {
                return;
            };
            let Some(row) = self.pixels.get_mut(start..end) else {
                return;
            };
            for_each_chunked(row, bounds.x, surface_y, &mut function);
            surface_y = surface_y.saturating_add(1);
        }
    }
}

fn raster_worker_count(pixel_count: usize) -> usize {
    if pixel_count < 524_288 {
        return 1;
    }
    std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
        .min(4)
}

fn for_each_row_mut_parallel(
    pixels: &mut [u32],
    surface_width: usize,
    stride: usize,
    start_y: usize,
    end_y: usize,
    workers: usize,
    function: impl Fn(usize, &mut [u32]) + Sync,
) {
    if surface_width == 0 || stride == 0 || start_y >= end_y {
        return;
    }
    let row_count = end_y.saturating_sub(start_y);
    let Some(region_start) = start_y.checked_mul(stride) else {
        return;
    };
    let Some(last_row) = end_y.checked_sub(1) else {
        return;
    };
    let Some(region_end) = last_row
        .checked_mul(stride)
        .and_then(|offset| offset.checked_add(surface_width))
    else {
        return;
    };
    let Some(rows) = pixels.get_mut(region_start..region_end) else {
        return;
    };

    let workers = workers.max(1).min(row_count);
    if workers == 1 {
        for (row_offset, row) in rows.chunks_mut(stride).enumerate() {
            function(start_y.saturating_add(row_offset), row);
        }
        return;
    }

    let rows_per_worker = row_count
        .checked_add(workers.saturating_sub(1))
        .unwrap_or(row_count)
        .checked_div(workers)
        .unwrap_or(1);
    let Some(elements_per_worker) = stride.checked_mul(rows_per_worker) else {
        return;
    };
    if elements_per_worker == 0 {
        return;
    }
    std::thread::scope(|scope| {
        let mut blocks = rows.chunks_mut(elements_per_worker);
        let Some(block_count) = row_count
            .checked_add(rows_per_worker.saturating_sub(1))
            .and_then(|count| count.checked_div(rows_per_worker))
        else {
            return;
        };
        let spawned_blocks = block_count.saturating_sub(1);
        for block_index in 0..spawned_blocks {
            let Some(block) = blocks.next() else {
                return;
            };
            let first_row = start_y.saturating_add(block_index.saturating_mul(rows_per_worker));
            let function = &function;
            scope.spawn(move || {
                for (row_offset, row) in block.chunks_mut(stride).enumerate() {
                    function(first_row.saturating_add(row_offset), row);
                }
            });
        }
        let first_row = start_y.saturating_add(spawned_blocks.saturating_mul(rows_per_worker));
        for (row_offset, row) in blocks.flat_map(|block| block.chunks_mut(stride).enumerate()) {
            function(first_row.saturating_add(row_offset), row);
        }
    });
}

#[derive(Clone, Copy, Debug)]
struct FRect {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl FRect {
    fn width(self) -> f64 {
        (self.right - self.left).max(0.0)
    }

    fn height(self) -> f64 {
        (self.bottom - self.top).max(0.0)
    }

    fn inset(self, amount: f64) -> Self {
        Self {
            left: self.left + amount,
            top: self.top + amount,
            right: self.right - amount,
            bottom: self.bottom - amount,
        }
    }

    fn is_empty(self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }
}

#[derive(Clone, Copy)]
enum Corner {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
}

fn validate_plane(length: usize, width: u32, height: u32, stride: usize, name: &str) -> Result<()> {
    let width_usize = usize::try_from(width).map_err(|_| Error::damaged(format!("{name} width does not fit usize")))?;
    let height_usize =
        usize::try_from(height).map_err(|_| Error::damaged(format!("{name} height does not fit usize")))?;
    if stride < width_usize {
        return Err(Error::damaged(format!(
            "{name} stride {stride} is smaller than width {width}"
        )));
    }
    if width == 0 || height == 0 {
        return Ok(());
    }
    let last_row = height_usize.saturating_sub(1);
    let required = last_row
        .checked_mul(stride)
        .and_then(|offset| offset.checked_add(width_usize))
        .ok_or_else(|| Error::damaged(format!("{name} dimensions overflow")))?;
    if length < required {
        return Err(Error::damaged(format!(
            "{name} buffer has {length} pixels, needs {required}"
        )));
    }
    Ok(())
}

fn detect_opaque(pixels: &[u32], width: u32, height: u32, stride: usize) -> bool {
    let (Ok(columns), Ok(rows)) = (usize::try_from(width), usize::try_from(height)) else {
        return false;
    };
    (0..rows).all(|row| {
        row.checked_mul(stride)
            .and_then(|start| pixels.get(start..start.checked_add(columns)?))
            .is_some_and(|line| line.iter().all(|pixel| pixel >> 24 == 0xFF))
    })
}

fn detect_solid_pixel(pixels: &[u32], width: u32, height: u32, stride: usize) -> Option<u32> {
    let pixel_count = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?;
    if pixel_count == 0 || pixel_count > MAX_SOLID_PIXEL_SCAN {
        return None;
    }
    let first = pixels.first().copied()?;
    for y in 0..usize::try_from(height).ok()? {
        let row_start = y.checked_mul(stride)?;
        let row_end = row_start.checked_add(usize::try_from(width).ok()?)?;
        if !pixels.get(row_start..row_end)?.iter().all(|pixel| *pixel == first) {
            return None;
        }
    }
    Some(first)
}

fn plane_offset(x: u32, y: u32, stride: usize) -> Option<usize> {
    usize::try_from(y)
        .ok()?
        .checked_mul(stride)?
        .checked_add(usize::try_from(x).ok()?)
}

fn clip_to_surface(rect: Rect, width: u32, height: u32) -> Rect {
    let left = i64::from(rect.x).max(0);
    let top = i64::from(rect.y).max(0);
    let right = rect.right().min(i64::from(width)).max(left);
    let bottom = rect.bottom().min(i64::from(height)).max(top);
    let x = i32::try_from(left).unwrap_or_default();
    let y = i32::try_from(top).unwrap_or_default();
    let clipped_width = u32::try_from(right.saturating_sub(left)).unwrap_or_default();
    let clipped_height = u32::try_from(bottom.saturating_sub(top)).unwrap_or_default();
    Rect::new(x, y, clipped_width, clipped_height)
}

fn intersect_rect(a: Rect, b: Rect) -> Rect {
    let left = i64::from(a.x).max(i64::from(b.x));
    let top = i64::from(a.y).max(i64::from(b.y));
    let right = a.right().min(b.right()).max(left);
    let bottom = a.bottom().min(b.bottom()).max(top);
    Rect::new(
        i32::try_from(left).unwrap_or_default(),
        i32::try_from(top).unwrap_or_default(),
        u32::try_from(right.saturating_sub(left)).unwrap_or_default(),
        u32::try_from(bottom.saturating_sub(top)).unwrap_or_default(),
    )
}

fn rect_y_range(rect: Rect) -> Option<(usize, usize)> {
    let start = usize::try_from(rect.y).ok()?;
    let height = usize::try_from(rect.height).ok()?;
    let end = start.checked_add(height)?;
    Some((start, end))
}

fn rect_to_f64(rect: Rect) -> FRect {
    FRect {
        left: f64::from(rect.x),
        top: f64::from(rect.y),
        right: f64::from(rect.x) + f64::from(rect.width),
        bottom: f64::from(rect.y) + f64::from(rect.height),
    }
}

fn finite_non_negative(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn coordinate_offset(value: i32, origin: i32) -> u32 {
    u32::try_from(i64::from(value).saturating_sub(i64::from(origin))).unwrap_or_default()
}

fn for_each_chunked(row: &mut [u32], start_x: i32, y: i32, function: &mut impl FnMut(i32, i32, &mut u32)) {
    let mut x = start_x;
    let mut chunks = row.chunks_exact_mut(8);
    for chunk in &mut chunks {
        for pixel in chunk.iter_mut() {
            function(x, y, pixel);
            x = x.saturating_add(1);
        }
    }
    for pixel in chunks.into_remainder().iter_mut() {
        function(x, y, pixel);
        x = x.saturating_add(1);
    }
}

fn premultiply(channel: u8, alpha: u8) -> u8 {
    scale_byte(scale_row(alpha), channel)
}

#[inline(always)]
fn scale_row(scale: u8) -> &'static [u8; 256] {
    SCALE_LUT.get(usize::from(scale)).unwrap_or(&ZERO_SCALE)
}

#[inline(always)]
fn scale_byte(scale: &[u8; 256], channel: u8) -> u8 {
    scale.get(usize::from(channel)).copied().unwrap_or_default()
}

fn byte(value: u32) -> u8 {
    u8::try_from(value & 0xFF).unwrap_or_default()
}

fn pixel_alpha(pixel: u32) -> u8 {
    byte(pixel.wrapping_shr(24))
}

fn pack_pixel(b: u8, g: u8, r: u8, a: u8) -> u32 {
    u32::from(b) | u32::from(g).wrapping_shl(8) | u32::from(r).wrapping_shl(16) | u32::from(a).wrapping_shl(24)
}

fn scale_packed(pixel: u32, scale: u8) -> u32 {
    let scale = scale_row(scale);
    pack_pixel(
        scale_byte(scale, byte(pixel)),
        scale_byte(scale, byte(pixel.wrapping_shr(8))),
        scale_byte(scale, byte(pixel.wrapping_shr(16))),
        scale_byte(scale, pixel_alpha(pixel)),
    )
}

#[inline(always)]
fn dim_pixel_lut(pixel: &mut u32, alpha: u8, scale: &[u8; 256]) {
    let original = *pixel;
    let original_alpha = pixel_alpha(original);
    let blue = scale_byte(scale, byte(original));
    let green = scale_byte(scale, byte(original.wrapping_shr(8)));
    let red = scale_byte(scale, byte(original.wrapping_shr(16)));
    let result_alpha = if original_alpha == u8::MAX {
        u8::MAX
    } else {
        alpha.saturating_add(scale_byte(scale, original_alpha))
    };
    *pixel = pack_pixel(blue, green, red, result_alpha);
}

#[inline(always)]
fn dim_chunk_lut(chunk: &mut [u32], alpha: u8, scale: &[u8; 256]) -> bool {
    let Some(first) = chunk.first().copied() else {
        return true;
    };
    if chunk.iter().all(|pixel| *pixel == first) {
        let mut dimmed = first;
        dim_pixel_lut(&mut dimmed, alpha, scale);
        chunk.fill(dimmed);
        true
    } else {
        false
    }
}

fn blend_black_mask_row(
    destination: &mut [u32],
    source: &[u8],
    alpha: u8,
    inverse_by_coverage: &[u8; 256],
    full_coverage_scale: &[u8; 256],
) {
    let mut destination_chunks = destination.chunks_exact_mut(8);
    let mut source_chunks = source.chunks_exact(8);
    for (destination_chunk, source_chunk) in destination_chunks.by_ref().zip(source_chunks.by_ref()) {
        if source_chunk.iter().all(|coverage| *coverage == 0) {
            continue;
        }
        if source_chunk.iter().all(|coverage| *coverage == u8::MAX) {
            if !dim_chunk_lut(destination_chunk, alpha, full_coverage_scale) {
                for destination_pixel in destination_chunk {
                    dim_pixel_lut(destination_pixel, alpha, full_coverage_scale);
                }
            }
            continue;
        }
        for (destination_pixel, coverage) in destination_chunk.iter_mut().zip(source_chunk) {
            blend_black_covered(destination_pixel, *coverage, inverse_by_coverage);
        }
    }
    for (destination_pixel, coverage) in destination_chunks
        .into_remainder()
        .iter_mut()
        .zip(source_chunks.remainder())
    {
        if *coverage == u8::MAX {
            dim_pixel_lut(destination_pixel, alpha, full_coverage_scale);
        } else if *coverage != 0 {
            blend_black_covered(destination_pixel, *coverage, inverse_by_coverage);
        }
    }
}

#[inline(always)]
fn dim_pixel_arithmetic(pixel: &mut u32, alpha: u8, inverse_alpha: u8) {
    let attenuated = scale_packed_arithmetic(*pixel, inverse_alpha);
    // Scaled alpha is at most `inverse_alpha`; `alpha + inverse_alpha` is 255.
    let result_alpha = alpha.wrapping_add(pixel_alpha(attenuated));
    *pixel = (attenuated & 0x00ff_ffff) | u32::from(result_alpha).wrapping_shl(24);
}

fn dim_chunk_arithmetic(chunk: &mut [u32], alpha: u8, inverse_alpha: u8) {
    let Some(first) = chunk.first().copied() else {
        return;
    };
    if chunk.iter().all(|pixel| *pixel == first) {
        let mut dimmed = first;
        dim_pixel_arithmetic(&mut dimmed, alpha, inverse_alpha);
        chunk.fill(dimmed);
    } else {
        for pixel in chunk {
            dim_pixel_arithmetic(pixel, alpha, inverse_alpha);
        }
    }
}

#[cfg(test)]
#[inline(always)]
fn scale_byte_arithmetic(channel: u8, scale: u8) -> u8 {
    let product = u32::from(channel).saturating_mul(u32::from(scale)).saturating_add(127);
    let rounded = product.saturating_add(product.wrapping_shr(8)).saturating_add(1);
    u8::try_from(rounded.wrapping_shr(8)).unwrap_or_default()
}

#[inline(always)]
fn scale_packed_arithmetic(pixel: u32, scale: u8) -> u32 {
    let scale = u32::from(scale);
    let blue_red = pixel & 0x00ff_00ff;
    let green_alpha = pixel.wrapping_shr(8) & 0x00ff_00ff;
    let scale_pair = |pair: u32| {
        // `pair` has two byte values in 16-bit lanes. The product, rounding bias,
        // correction, and final bias stay below `u32::MAX` for every u8 scale.
        let product = pair.wrapping_mul(scale).wrapping_add(0x007f_007f);
        let correction = product.wrapping_shr(8) & 0x00ff_00ff;
        product
            .wrapping_add(correction)
            .wrapping_add(0x0001_0001)
            .wrapping_shr(8)
            & 0x00ff_00ff
    };
    scale_pair(blue_red) | scale_pair(green_alpha).wrapping_shl(8)
}

#[inline(always)]
fn blend_black_covered(destination: &mut u32, coverage: u8, inverse_by_coverage: &[u8; 256]) {
    let inverse_alpha = inverse_by_coverage
        .get(usize::from(coverage))
        .copied()
        .unwrap_or_default();
    if inverse_alpha == u8::MAX {
        return;
    }
    let original = *destination;
    let scale = scale_row(inverse_alpha);
    let blue = scale_byte(scale, byte(original));
    let green = scale_byte(scale, byte(original.wrapping_shr(8)));
    let red = scale_byte(scale, byte(original.wrapping_shr(16)));
    let original_alpha = pixel_alpha(original);
    let result_alpha = if original_alpha == u8::MAX {
        u8::MAX
    } else {
        let source_alpha = u8::MAX.saturating_sub(inverse_alpha);
        source_alpha.saturating_add(scale_byte(scale, original_alpha))
    };
    *destination = pack_pixel(blue, green, red, result_alpha);
}

#[derive(Clone, Copy, Debug, Default)]
struct CoveredSource {
    pixel: u32,
    inverse_alpha: u8,
}

impl CoveredSource {
    fn new(source: u32, coverage: u8) -> Self {
        let pixel = scale_packed(source, coverage);
        Self {
            pixel,
            inverse_alpha: u8::MAX.saturating_sub(pixel_alpha(pixel)),
        }
    }
}

fn blend_prepared(destination: u32, source: CoveredSource) -> u32 {
    if source.inverse_alpha == u8::MAX {
        return destination;
    }
    if source.inverse_alpha == 0 {
        return source.pixel;
    }
    if pixel_alpha(destination) == u8::MAX {
        let scale = scale_row(source.inverse_alpha);
        let blue = scale_byte(scale, byte(destination));
        let green = scale_byte(scale, byte(destination.wrapping_shr(8)));
        let red = scale_byte(scale, byte(destination.wrapping_shr(16)));
        return pack_pixel(
            byte(source.pixel).saturating_add(blue),
            byte(source.pixel.wrapping_shr(8)).saturating_add(green),
            byte(source.pixel.wrapping_shr(16)).saturating_add(red),
            u8::MAX,
        );
    }
    add_packed_saturating(source.pixel, scale_packed(destination, source.inverse_alpha))
}

fn add_packed_saturating(left: u32, right: u32) -> u32 {
    let blue_red = (left & 0x00FF_00FF).wrapping_add(right & 0x00FF_00FF);
    let green_alpha = (left.wrapping_shr(8) & 0x00FF_00FF).wrapping_add(right.wrapping_shr(8) & 0x00FF_00FF);
    let blue_red_overflow = (blue_red & 0x0100_0100).wrapping_shr(8).wrapping_mul(0xFF);
    let green_alpha_overflow = (green_alpha & 0x0100_0100).wrapping_shr(8).wrapping_mul(0xFF);
    (blue_red & 0x00FF_00FF) | blue_red_overflow | ((green_alpha & 0x00FF_00FF) | green_alpha_overflow).wrapping_shl(8)
}

fn blend_covered(destination: u32, source: u32, coverage: u8) -> u32 {
    if coverage == 0 {
        return destination;
    }
    if coverage == u8::MAX {
        if pixel_alpha(source) == u8::MAX {
            return source;
        }
        return blend_prepared(
            destination,
            CoveredSource {
                pixel: source,
                inverse_alpha: u8::MAX.saturating_sub(pixel_alpha(source)),
            },
        );
    }
    blend_prepared(destination, CoveredSource::new(source, coverage))
}

fn border_depth(width: f64, first_radius: f64, second_radius: f64) -> u32 {
    let extent = width.max(first_radius).max(second_radius).ceil();
    if !extent.is_finite() || extent <= 0.0 {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        extent.min(f64::from(u32::MAX)) as u32
    }
}

fn rounded_extent(radius: f64) -> i64 {
    if radius <= 0.0 {
        0
    } else {
        // `Radii::normalised` caps each corner at the rectangle dimensions, which are `u32`.
        #[allow(clippy::cast_possible_truncation)]
        let extent = radius.ceil() as u32;
        i64::from(extent)
    }
}

fn shape_row_full_span(rect: Rect, radii: Radii, y: i32) -> (i64, i64) {
    let left = i64::from(rect.x);
    let right = rect.right();
    let top = i64::from(rect.y);
    let bottom = rect.bottom();
    let row = i64::from(y);
    let top_left = rounded_extent(radii.top_left);
    let top_right = rounded_extent(radii.top_right);
    let bottom_right = rounded_extent(radii.bottom_right);
    let bottom_left = rounded_extent(radii.bottom_left);
    let mut full_left = left;
    let mut full_right = right;
    if row < top.saturating_add(top_left) {
        full_left = full_left.max(left.saturating_add(top_left));
    }
    if row < top.saturating_add(top_right) {
        full_right = full_right.min(right.saturating_sub(top_right));
    }
    if row >= bottom.saturating_sub(bottom_right) {
        full_right = full_right.min(right.saturating_sub(bottom_right));
    }
    if row >= bottom.saturating_sub(bottom_left) {
        full_left = full_left.max(left.saturating_add(bottom_left));
    }
    (full_left, full_right)
}

fn paint_exact_segment(
    row: &mut [u32],
    start_x: i32,
    y: i32,
    shape: FRect,
    radii: Radii,
    colour_at: &mut impl FnMut(i32, i32) -> u32,
) {
    for_each_chunked(row, start_x, y, &mut |x, y, pixel| {
        let coverage = coverage_byte(rounded_coverage(shape, radii, x, y));
        if coverage != 0 {
            *pixel = blend_covered(*pixel, colour_at(x, y), coverage);
        }
    });
}

fn paint_full_segment(row: &mut [u32], start_x: i32, y: i32, colour_at: &mut impl FnMut(i32, i32) -> u32) {
    for_each_chunked(row, start_x, y, &mut |x, y, pixel| {
        *pixel = blend_covered(*pixel, colour_at(x, y), u8::MAX);
    });
}

fn rounded_coverage(rect: FRect, radii: Radii, pixel_x: i32, pixel_y: i32) -> f64 {
    if rect.is_empty() {
        return 0.0;
    }
    let pixel_left = f64::from(pixel_x);
    let pixel_top = f64::from(pixel_y);
    let pixel_right = pixel_left + 1.0;
    let pixel_bottom = pixel_top + 1.0;
    let intersection_left = rect.left.max(pixel_left);
    let intersection_top = rect.top.max(pixel_top);
    let intersection_right = rect.right.min(pixel_right);
    let intersection_bottom = rect.bottom.min(pixel_bottom);
    if intersection_right <= intersection_left || intersection_bottom <= intersection_top {
        return 0.0;
    }
    let base = (intersection_right - intersection_left) * (intersection_bottom - intersection_top);
    let loss = corner_loss(rect, radii.top_left, pixel_x, pixel_y, Corner::TopLeft)
        + corner_loss(rect, radii.top_right, pixel_x, pixel_y, Corner::TopRight)
        + corner_loss(rect, radii.bottom_right, pixel_x, pixel_y, Corner::BottomRight)
        + corner_loss(rect, radii.bottom_left, pixel_x, pixel_y, Corner::BottomLeft);
    (base - loss).clamp(0.0, 1.0)
}

fn corner_loss(rect: FRect, radius: f64, pixel_x: i32, pixel_y: i32, corner: Corner) -> f64 {
    if radius <= 0.0 {
        return 0.0;
    }
    let pixel_left = f64::from(pixel_x);
    let pixel_top = f64::from(pixel_y);
    let pixel_right = pixel_left + 1.0;
    let pixel_bottom = pixel_top + 1.0;

    let (square_left, square_top, square_right, square_bottom, center_x, center_y) = match corner {
        Corner::TopLeft => (
            rect.left,
            rect.top,
            rect.left + radius,
            rect.top + radius,
            rect.left + radius,
            rect.top + radius,
        ),
        Corner::TopRight => (
            rect.right - radius,
            rect.top,
            rect.right,
            rect.top + radius,
            rect.right - radius,
            rect.top + radius,
        ),
        Corner::BottomRight => (
            rect.right - radius,
            rect.bottom - radius,
            rect.right,
            rect.bottom,
            rect.right - radius,
            rect.bottom - radius,
        ),
        Corner::BottomLeft => (
            rect.left,
            rect.bottom - radius,
            rect.left + radius,
            rect.bottom,
            rect.left + radius,
            rect.bottom - radius,
        ),
    };

    let left = square_left.max(pixel_left);
    let top = square_top.max(pixel_top);
    let right = square_right.min(pixel_right);
    let bottom = square_bottom.min(pixel_bottom);
    if right <= left || bottom <= top {
        return 0.0;
    }

    let (u0, u1) = match corner {
        Corner::TopLeft | Corner::BottomLeft => (center_x - right, center_x - left),
        Corner::TopRight | Corner::BottomRight => (left - center_x, right - center_x),
    };
    let (v0, v1) = match corner {
        Corner::TopLeft | Corner::TopRight => (center_y - bottom, center_y - top),
        Corner::BottomRight | Corner::BottomLeft => (top - center_y, bottom - center_y),
    };
    let square_area = (right - left) * (bottom - top);
    let inside_circle = quarter_disk_rect_area(u0, u1, v0, v1, radius);
    (square_area - inside_circle).clamp(0.0, square_area)
}

fn quarter_disk_rect_area(u0: f64, u1: f64, v0: f64, v1: f64, radius: f64) -> f64 {
    if radius <= 0.0 || u1 <= u0 || v1 <= v0 || u0 >= radius || v0 >= radius {
        return 0.0;
    }
    let left = u0.clamp(0.0, radius);
    let right = u1.clamp(0.0, radius);
    if right <= left {
        return 0.0;
    }
    let lower = v0.clamp(0.0, radius);
    let upper = v1.clamp(0.0, radius);
    if upper <= lower {
        return 0.0;
    }

    let full_until = circle_x_at_y(radius, upper);
    let nonzero_until = circle_x_at_y(radius, lower);
    let full_right = right.min(full_until);
    let full_area = if full_right > left {
        (full_right - left) * (upper - lower)
    } else {
        0.0
    };

    let partial_left = left.max(full_until);
    let partial_right = right.min(nonzero_until);
    let partial_area = if partial_right > partial_left {
        circle_primitive(partial_right, radius)
            - circle_primitive(partial_left, radius)
            - lower * (partial_right - partial_left)
    } else {
        0.0
    };
    (full_area + partial_area).max(0.0)
}

fn circle_x_at_y(radius: f64, y: f64) -> f64 {
    let square = radius.mul_add(radius, -(y * y));
    square.max(0.0).sqrt()
}

fn circle_primitive(x: f64, radius: f64) -> f64 {
    if radius <= 0.0 {
        return 0.0;
    }
    let x = x.clamp(0.0, radius);
    let root = radius.mul_add(radius, -(x * x)).max(0.0).sqrt();
    0.5 * (x.mul_add(root, radius * radius * (x / radius).asin()))
}

fn coverage_byte(coverage: f64) -> u8 {
    if !coverage.is_finite() || coverage <= 0.0 {
        return 0;
    }
    if coverage >= 1.0 {
        return u8::MAX;
    }
    let target = coverage * 255.0;
    let mut low = 0_u16;
    let mut high = 255_u16;
    while low < high {
        let span = high.saturating_sub(low);
        let middle = low.saturating_add(span.saturating_add(1).checked_div(2).unwrap_or_default());
        let threshold = f64::from(middle) - 0.5;
        if target >= threshold {
            low = middle;
        } else {
            high = middle.saturating_sub(1);
        }
    }
    u8::try_from(low).unwrap_or(u8::MAX)
}

fn sample_bilinear(
    image: ImageRef<'_>,
    destination_x: u32,
    destination_y: u32,
    destination_width: u32,
    destination_height: u32,
) -> u32 {
    let (x0, x1, x_fraction, x_denominator) = linear_coordinate(destination_x, destination_width, image.width);
    let (y0, y1, y_fraction, y_denominator) = linear_coordinate(destination_y, destination_height, image.height);
    let left = x_denominator.saturating_sub(x_fraction);
    let top = y_denominator.saturating_sub(y_fraction);
    let weights = (
        u128::from(left).saturating_mul(u128::from(top)),
        u128::from(x_fraction).saturating_mul(u128::from(top)),
        u128::from(left).saturating_mul(u128::from(y_fraction)),
        u128::from(x_fraction).saturating_mul(u128::from(y_fraction)),
    );
    let denominator = u128::from(x_denominator).saturating_mul(u128::from(y_denominator));
    weighted_four(
        image.pixel(x0, y0),
        image.pixel(x1, y0),
        image.pixel(x0, y1),
        image.pixel(x1, y1),
        weights,
        denominator.max(1),
    )
}

fn linear_coordinate(position: u32, destination: u32, source: u32) -> (u32, u32, u32, u32) {
    if source <= 1 || destination <= 1 {
        return (0, 0, 0, 1);
    }
    let denominator = destination.saturating_sub(1);
    let source_span = source.saturating_sub(1);
    let numerator = u64::from(position).saturating_mul(u64::from(source_span));
    let denominator_u64 = u64::from(denominator);
    let base = numerator.checked_div(denominator_u64).unwrap_or_default();
    let fraction = numerator.checked_rem(denominator_u64).unwrap_or_default();
    let x0 = u32::try_from(base).unwrap_or_default().min(source_span);
    let x1 = x0.saturating_add(1).min(source_span);
    (x0, x1, u32::try_from(fraction).unwrap_or_default(), denominator)
}

fn weighted_four(p00: u32, p10: u32, p01: u32, p11: u32, weights: (u128, u128, u128, u128), denominator: u128) -> u32 {
    pack_pixel(
        weighted_channel((byte(p00), byte(p10), byte(p01), byte(p11)), weights, denominator),
        weighted_channel(
            (
                byte(p00.wrapping_shr(8)),
                byte(p10.wrapping_shr(8)),
                byte(p01.wrapping_shr(8)),
                byte(p11.wrapping_shr(8)),
            ),
            weights,
            denominator,
        ),
        weighted_channel(
            (
                byte(p00.wrapping_shr(16)),
                byte(p10.wrapping_shr(16)),
                byte(p01.wrapping_shr(16)),
                byte(p11.wrapping_shr(16)),
            ),
            weights,
            denominator,
        ),
        weighted_channel(
            (pixel_alpha(p00), pixel_alpha(p10), pixel_alpha(p01), pixel_alpha(p11)),
            weights,
            denominator,
        ),
    )
}

fn weighted_channel(channels: (u8, u8, u8, u8), weights: (u128, u128, u128, u128), denominator: u128) -> u8 {
    let sum = u128::from(channels.0)
        .saturating_mul(weights.0)
        .saturating_add(u128::from(channels.1).saturating_mul(weights.1))
        .saturating_add(u128::from(channels.2).saturating_mul(weights.2))
        .saturating_add(u128::from(channels.3).saturating_mul(weights.3));
    rounded_div_u128(sum, denominator)
}

fn rounded_div_u128(value: u128, denominator: u128) -> u8 {
    if denominator == 0 {
        return 0;
    }
    let rounded = value.saturating_add(denominator.checked_div(2).unwrap_or_default());
    let quotient = rounded.checked_div(denominator).unwrap_or_default().min(255);
    u8::try_from(quotient).unwrap_or(u8::MAX)
}

#[derive(Clone, Copy, Debug, Default)]
struct BoxAxisWeights {
    start: u32,
    end: u32,
    first: u64,
    last: u64,
    scale: u64,
}

impl BoxAxisWeights {
    fn new(position: u32, source: u32, destination: u32) -> Self {
        let scale = u64::from(destination.max(1));
        let left = u64::from(position).saturating_mul(u64::from(source));
        let right = u64::from(position.saturating_add(1)).saturating_mul(u64::from(source));
        let start = left.checked_div(scale).unwrap_or_default().min(u64::from(source));
        let end = ceil_div_u64(right, scale).min(u64::from(source));
        let first_pixel_right = start.saturating_add(1).saturating_mul(scale);
        let first = right
            .min(first_pixel_right)
            .saturating_sub(left.max(start.saturating_mul(scale)));
        let last_pixel_left = end.saturating_sub(1).saturating_mul(scale);
        let last = right.saturating_sub(left.max(last_pixel_left));
        Self {
            start: u32::try_from(start).unwrap_or_default(),
            end: u32::try_from(end).unwrap_or_default(),
            first,
            last,
            scale,
        }
    }

    fn weight(self, position: u32) -> u64 {
        if self.end <= self.start {
            0
        } else if self.end.saturating_sub(self.start) == 1 || position == self.start {
            self.first
        } else if position.saturating_add(1) == self.end {
            self.last
        } else {
            self.scale
        }
    }
}

fn for_each_box_sample(
    image: ImageRef<'_>,
    columns: BoxAxisWeights,
    rows: BoxAxisWeights,
    mut function: impl FnMut(u32, u128),
) {
    for source_y in rows.start..rows.end {
        let overlap_y = rows.weight(source_y);
        for source_x in columns.start..columns.end {
            let overlap_x = columns.weight(source_x);
            let weight = u128::from(overlap_x).saturating_mul(u128::from(overlap_y));
            if weight != 0 {
                function(image.pixel(source_x, source_y), weight);
            }
        }
    }
}

fn sample_box_u64(image: ImageRef<'_>, columns: BoxAxisWeights, rows: BoxAxisWeights, denominator: u128) -> u32 {
    let mut blue = 0_u64;
    let mut green = 0_u64;
    let mut red = 0_u64;
    let mut alpha = 0_u64;
    for_each_box_sample(image, columns, rows, |pixel, weight| {
        let weight = u64::try_from(weight).unwrap_or(u64::MAX);
        blue = blue.saturating_add(u64::from(byte(pixel)).saturating_mul(weight));
        green = green.saturating_add(u64::from(byte(pixel.wrapping_shr(8))).saturating_mul(weight));
        red = red.saturating_add(u64::from(byte(pixel.wrapping_shr(16))).saturating_mul(weight));
        alpha = alpha.saturating_add(u64::from(pixel_alpha(pixel)).saturating_mul(weight));
    });
    let denominator = u64::try_from(denominator).unwrap_or(u64::MAX);
    pack_pixel(
        rounded_div_u64(blue, denominator),
        rounded_div_u64(green, denominator),
        rounded_div_u64(red, denominator),
        rounded_div_u64(alpha, denominator),
    )
}

fn sample_box_u128(image: ImageRef<'_>, columns: BoxAxisWeights, rows: BoxAxisWeights, denominator: u128) -> u32 {
    let mut blue = 0_u128;
    let mut green = 0_u128;
    let mut red = 0_u128;
    let mut alpha = 0_u128;
    for_each_box_sample(image, columns, rows, |pixel, weight| {
        blue = blue.saturating_add(u128::from(byte(pixel)).saturating_mul(weight));
        green = green.saturating_add(u128::from(byte(pixel.wrapping_shr(8))).saturating_mul(weight));
        red = red.saturating_add(u128::from(byte(pixel.wrapping_shr(16))).saturating_mul(weight));
        alpha = alpha.saturating_add(u128::from(pixel_alpha(pixel)).saturating_mul(weight));
    });
    pack_pixel(
        rounded_div_u128(blue, denominator),
        rounded_div_u128(green, denominator),
        rounded_div_u128(red, denominator),
        rounded_div_u128(alpha, denominator),
    )
}

fn rounded_div_u64(value: u64, denominator: u64) -> u8 {
    if denominator == 0 {
        return 0;
    }
    let rounded = value.saturating_add(denominator.checked_div(2).unwrap_or_default());
    let quotient = rounded.checked_div(denominator).unwrap_or_default().min(255);
    u8::try_from(quotient).unwrap_or(u8::MAX)
}

fn ceil_div_u64(value: u64, divisor: u64) -> u64 {
    if divisor == 0 {
        return 0;
    }
    let quotient = value.checked_div(divisor).unwrap_or_default();
    if value.checked_rem(divisor).unwrap_or_default() == 0 {
        quotient
    } else {
        quotient.saturating_add(1)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        blend_black_covered, blend_covered, ceil_div_u64, coverage_byte, pack_pixel, rounded_coverage,
        rounded_div_u128, sample_box_u128, sample_box_u64, scale_byte_arithmetic, scale_packed, BoxAxisWeights, Color,
        FRect, ImageFilter, ImageRef, Radii, Rect, Surface,
    };
    use sse_core::Result;
    use std::f64::consts::PI;

    fn reference_blend(destination: u32, source: u32, coverage: u8) -> u32 {
        let source = Color::from_u32(source);
        let source = Color::premultiplied_bgra(
            reference_scale_byte(source.b, coverage),
            reference_scale_byte(source.g, coverage),
            reference_scale_byte(source.r, coverage),
            reference_scale_byte(source.a, coverage),
        );
        if source.a == 0 {
            return destination;
        }
        if source.a == u8::MAX {
            return source.to_u32();
        }
        let destination = Color::from_u32(destination);
        let inverse_alpha = u8::MAX.saturating_sub(source.a);
        Color::premultiplied_bgra(
            source
                .b
                .saturating_add(reference_scale_byte(destination.b, inverse_alpha)),
            source
                .g
                .saturating_add(reference_scale_byte(destination.g, inverse_alpha)),
            source
                .r
                .saturating_add(reference_scale_byte(destination.r, inverse_alpha)),
            source
                .a
                .saturating_add(reference_scale_byte(destination.a, inverse_alpha)),
        )
        .to_u32()
    }

    fn reference_scale_byte(channel: u8, scale: u8) -> u8 {
        let product = u16::from(channel).saturating_mul(u16::from(scale));
        u8::try_from(product.saturating_add(127).checked_div(255).unwrap_or_default()).unwrap_or(u8::MAX)
    }

    fn reference_sample_box(
        image: ImageRef<'_>,
        destination_x: u32,
        destination_y: u32,
        destination_width: u32,
        destination_height: u32,
    ) -> u32 {
        let x_left = u64::from(destination_x).saturating_mul(u64::from(image.width));
        let x_right = u64::from(destination_x.saturating_add(1)).saturating_mul(u64::from(image.width));
        let y_top = u64::from(destination_y).saturating_mul(u64::from(image.height));
        let y_bottom = u64::from(destination_y.saturating_add(1)).saturating_mul(u64::from(image.height));
        let x_scale = u64::from(destination_width.max(1));
        let y_scale = u64::from(destination_height.max(1));
        let source_x_start = x_left.checked_div(x_scale).unwrap_or_default();
        let source_x_end = ceil_div_u64(x_right, x_scale).min(u64::from(image.width));
        let source_y_start = y_top.checked_div(y_scale).unwrap_or_default();
        let source_y_end = ceil_div_u64(y_bottom, y_scale).min(u64::from(image.height));
        let mut blue = 0_u128;
        let mut green = 0_u128;
        let mut red = 0_u128;
        let mut alpha = 0_u128;
        let mut total = 0_u128;
        for source_y in source_y_start..source_y_end {
            let pixel_top = source_y.saturating_mul(y_scale);
            let pixel_bottom = source_y.saturating_add(1).saturating_mul(y_scale);
            let overlap_y = pixel_bottom.min(y_bottom).saturating_sub(pixel_top.max(y_top));
            for source_x in source_x_start..source_x_end {
                let pixel_left = source_x.saturating_mul(x_scale);
                let pixel_right = source_x.saturating_add(1).saturating_mul(x_scale);
                let overlap_x = pixel_right.min(x_right).saturating_sub(pixel_left.max(x_left));
                let weight = u128::from(overlap_x).saturating_mul(u128::from(overlap_y));
                if weight == 0 {
                    continue;
                }
                let colour = Color::from_u32(image.pixel(
                    u32::try_from(source_x).unwrap_or_default(),
                    u32::try_from(source_y).unwrap_or_default(),
                ));
                blue = blue.saturating_add(u128::from(colour.b).saturating_mul(weight));
                green = green.saturating_add(u128::from(colour.g).saturating_mul(weight));
                red = red.saturating_add(u128::from(colour.r).saturating_mul(weight));
                alpha = alpha.saturating_add(u128::from(colour.a).saturating_mul(weight));
                total = total.saturating_add(weight);
            }
        }
        Color::premultiplied_bgra(
            rounded_div_u128(blue, total),
            rounded_div_u128(green, total),
            rounded_div_u128(red, total),
            rounded_div_u128(alpha, total),
        )
        .to_u32()
    }

    #[test]
    fn rounded_rectangle_matches_analytic_area() {
        let width = 23_u32;
        let height = 17_u32;
        let radius = 4.75_f64;
        let rect = FRect {
            left: 0.0,
            top: 0.0,
            right: f64::from(width),
            bottom: f64::from(height),
        };
        let radii = Radii::all(radius).normalised(rect.width(), rect.height());
        let mut measured = 0.0_f64;
        let height_i32 = i32::try_from(height).unwrap_or_default();
        let width_i32 = i32::try_from(width).unwrap_or_default();
        for y in 0..height_i32 {
            for x in 0..width_i32 {
                measured += rounded_coverage(rect, radii, x, y);
            }
        }
        let square_loss = radius * radius;
        let circle_gain = PI * radius * radius;
        let expected = f64::from(width) * f64::from(height) - 4.0 * square_loss + circle_gain;
        assert!((measured - expected).abs() <= 1.0 / 255.0);
    }

    #[test]
    fn clipping_holds_at_all_four_edges() -> Result<()> {
        let untouched = Color::rgba(10, 20, 30, 255).to_u32();
        let painted = Color::rgba(220, 30, 40, 255).to_u32();
        let mut pixels = vec![untouched; 16];
        {
            let mut surface = Surface::new(&mut pixels, 4, 4, 4, Rect::new(1, 1, 2, 2))?;
            surface.fill_rect(
                Rect::new(-100, -100, 500, 500),
                Radii::ZERO,
                Color::rgba(220, 30, 40, 255),
            );
        }
        for (position, pixel) in pixels.iter().copied().enumerate() {
            let x = position.checked_rem(4).unwrap_or_default();
            let y = position.checked_div(4).unwrap_or_default();
            let inside = (1..3).contains(&x) && (1..3).contains(&y);
            assert_eq!(pixel, if inside { painted } else { untouched });
        }
        Ok(())
    }

    #[test]
    fn parallel_row_chunks_match_single_worker_with_padding() {
        for (height, workers) in [(1_usize, 4_usize), (6, 2), (11, 4), (19, 6)] {
            let width = 5_usize;
            let stride = 8_usize;
            let length = stride.saturating_mul(height);
            let start_y = usize::from(height > 2).saturating_mul(2);
            let end_y = height.saturating_sub(1);
            let initial = vec![u32::MAX; length];
            let mut single_worker = initial.clone();
            let mut parallel = initial;
            super::for_each_row_mut_parallel(
                &mut single_worker,
                width,
                stride,
                start_y,
                end_y,
                1,
                |row_index, row| {
                    if let Some(pixels) = row.get_mut(..width) {
                        pixels.fill(u32::try_from(row_index).unwrap_or_default());
                    }
                },
            );
            super::for_each_row_mut_parallel(
                &mut parallel,
                width,
                stride,
                start_y,
                end_y,
                workers,
                |row_index, row| {
                    if let Some(pixels) = row.get_mut(..width) {
                        pixels.fill(u32::try_from(row_index).unwrap_or_default());
                    }
                },
            );
            assert_eq!(parallel, single_worker, "height={height}, workers={workers}");
        }
    }

    #[test]
    fn blending_identities_hold() -> Result<()> {
        let original = Color::rgba(30, 60, 90, 255).to_u32();
        let replacement = Color::rgba(200, 100, 50, 255).to_u32();
        let mut pixels = vec![original];
        {
            let mut surface = Surface::new(&mut pixels, 1, 1, 1, Rect::new(0, 0, 1, 1))?;
            surface.fill_rect(Rect::new(0, 0, 1, 1), Radii::ZERO, Color::rgba(1, 2, 3, 0));
        }
        assert_eq!(pixels.first().copied(), Some(original));
        {
            let mut surface = Surface::new(&mut pixels, 1, 1, 1, Rect::new(0, 0, 1, 1))?;
            surface.fill_rect(Rect::new(0, 0, 1, 1), Radii::ZERO, Color::rgba(200, 100, 50, 255));
        }
        assert_eq!(pixels.first().copied(), Some(replacement));
        Ok(())
    }

    #[test]
    fn rounded_fill_interior_path_matches_exact_per_pixel_reference() -> Result<()> {
        let width = 37_u32;
        let height = 29_u32;
        let mut initial: Vec<u32> = (0_u32..width.saturating_mul(height))
            .map(|index| {
                let b = u8::try_from(index.wrapping_mul(29) & 0xff).unwrap_or_default();
                let g = u8::try_from(index.wrapping_mul(11) & 0xff).unwrap_or_default();
                let r = u8::try_from(index.wrapping_mul(7) & 0xff).unwrap_or_default();
                let a = u8::try_from(index.wrapping_mul(17) & 0xff).unwrap_or_default();
                Color::premultiplied_bgra(b, g, r, a).to_u32()
            })
            .collect();
        let mut actual = initial.clone();
        let rect = Rect::new(3, 2, 31, 23);
        let clip = Rect::new(4, 3, 28, 21);
        let radii = Radii {
            top_left: 8.25,
            top_right: 3.5,
            bottom_right: 10.75,
            bottom_left: 2.25,
        };
        let source = Color::rgba(197, 103, 241, 187).to_u32();
        {
            let mut surface = Surface::new(
                &mut actual,
                width,
                height,
                usize::try_from(width).unwrap_or_default(),
                clip,
            )?;
            surface.fill_rect(rect, radii, Color::rgba(197, 103, 241, 187));
        }
        let shape = super::rect_to_f64(rect);
        let radii = radii.normalised(f64::from(rect.width), f64::from(rect.height));
        for y in clip.y..i32::try_from(clip.bottom()).unwrap_or_default() {
            for x in clip.x..i32::try_from(clip.right()).unwrap_or_default() {
                let coverage = coverage_byte(rounded_coverage(shape, radii, x, y));
                let offset = usize::try_from(y)
                    .unwrap_or_default()
                    .saturating_mul(usize::try_from(width).unwrap_or_default())
                    .saturating_add(usize::try_from(x).unwrap_or_default());
                let Some(expected) = initial.get_mut(offset) else {
                    continue;
                };
                if coverage != 0 {
                    *expected = reference_blend(*expected, source, coverage);
                }
            }
        }
        assert_eq!(actual, initial);
        Ok(())
    }

    #[test]
    fn packed_scaling_and_blending_match_channel_reference() {
        for channel in 0_u16..=255 {
            let channel = u8::try_from(channel).unwrap_or_default();
            let pixel = pack_pixel(channel, channel, channel, channel);
            for scale in 0_u16..=255 {
                let scale = u8::try_from(scale).unwrap_or_default();
                assert_eq!(
                    scale_byte_arithmetic(channel, scale),
                    reference_scale_byte(channel, scale)
                );
                let expected = pack_pixel(
                    reference_scale_byte(channel, scale),
                    reference_scale_byte(channel, scale),
                    reference_scale_byte(channel, scale),
                    reference_scale_byte(channel, scale),
                );
                assert_eq!(scale_packed(pixel, scale), expected);
            }
        }

        for index in 0_u32..4_096_u32 {
            let destination = index.wrapping_mul(0x9e37_79b9).rotate_left(13);
            let source = index.wrapping_mul(0x85eb_ca6b).rotate_right(7);
            let coverage = u8::try_from(index.wrapping_mul(37) & 0xff).unwrap_or_default();
            assert_eq!(
                blend_covered(destination, source, coverage),
                reference_blend(destination, source, coverage)
            );
        }
    }

    #[test]
    fn packed_integer_scaling_matches_the_byte_lookup_for_every_channel() {
        for scale in 0_u16..=255_u16 {
            let scale = u8::try_from(scale).unwrap_or_default();
            for channel in 0_u16..=255_u16 {
                let channel = u8::try_from(channel).unwrap_or_default();
                let pixel = pack_pixel(channel, channel, channel, channel);
                assert_eq!(
                    super::scale_packed_arithmetic(pixel, scale),
                    super::scale_packed(pixel, scale),
                    "channel={channel}, scale={scale}"
                );
            }
        }
    }

    #[test]
    fn black_mask_blend_matches_generic_source_over() {
        let mut inverse_by_coverage = [0_u8; 256];
        for (coverage, inverse_alpha) in inverse_by_coverage.iter_mut().enumerate() {
            let source_alpha = super::scale_byte(super::scale_row(110), u8::try_from(coverage).unwrap_or_default());
            *inverse_alpha = u8::MAX.saturating_sub(source_alpha);
        }
        let source = Color::rgba(0, 0, 0, 110).to_u32();
        for index in 0_u32..65_536_u32 {
            let coverage = u8::try_from(index & 0xff).unwrap_or_default();
            let destination = index.wrapping_mul(0x9e37_79b9).rotate_left(13);
            let mut actual = destination;
            blend_black_covered(&mut actual, coverage, &inverse_by_coverage);
            assert_eq!(actual, reference_blend(destination, source, coverage));
        }
    }

    #[test]
    fn box_filter_column_weights_match_exact_overlap_reference() -> Result<()> {
        let source_width = 13_u32;
        let source_height = 11_u32;
        let stride = 16_usize;
        let mut pixels =
            vec![0xfeed_beef_u32; stride.saturating_mul(usize::try_from(source_height).unwrap_or_default())];
        for y in 0_u32..source_height {
            for x in 0_u32..source_width {
                let red = u8::try_from(x.saturating_mul(19).saturating_add(y.saturating_mul(7))).unwrap_or_default();
                let green = u8::try_from(x.saturating_mul(3).saturating_add(y.saturating_mul(23))).unwrap_or_default();
                let blue = u8::try_from(x.saturating_mul(11).saturating_add(y.saturating_mul(13))).unwrap_or_default();
                let alpha = u8::try_from(x.saturating_mul(17).saturating_add(y.saturating_mul(9))).unwrap_or_default();
                let offset = usize::try_from(y)
                    .unwrap_or_default()
                    .saturating_mul(stride)
                    .saturating_add(usize::try_from(x).unwrap_or_default());
                if let Some(pixel) = pixels.get_mut(offset) {
                    *pixel = Color::rgba(red, green, blue, alpha).to_u32();
                }
            }
        }
        let image = ImageRef::new(&pixels, source_width, source_height, stride)?;
        let denominator = u128::from(source_width).saturating_mul(u128::from(source_height));
        for (destination_width, destination_height) in [(7_u32, 5_u32), (3, 9), (17, 14), (1, 1), (13, 11)] {
            for y in 0_u32..destination_height {
                for x in 0_u32..destination_width {
                    let columns = BoxAxisWeights::new(x, source_width, destination_width);
                    let rows = BoxAxisWeights::new(y, source_height, destination_height);
                    let actual = if denominator <= u128::from(u64::MAX / 256) {
                        sample_box_u64(image, columns, rows, denominator)
                    } else {
                        sample_box_u128(image, columns, rows, denominator)
                    };
                    assert_eq!(
                        actual,
                        reference_sample_box(image, x, y, destination_width, destination_height),
                        "destination ({x}, {y}) at {destination_width}x{destination_height}"
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn exact_box_downscale_averages_source_pixels() -> Result<()> {
        let source = vec![
            Color::rgba(0, 0, 0, 255).to_u32(),
            Color::rgba(100, 0, 0, 255).to_u32(),
            Color::rgba(0, 100, 0, 255).to_u32(),
            Color::rgba(100, 100, 0, 255).to_u32(),
        ];
        let image = ImageRef::new(&source, 2, 2, 2)?;
        let mut destination = vec![0_u32];
        {
            let mut surface = Surface::new(&mut destination, 1, 1, 1, Rect::new(0, 0, 1, 1))?;
            surface.blit_image(image, Rect::new(0, 0, 1, 1), ImageFilter::Box);
        }
        assert_eq!(destination.first().copied(), Some(Color::rgba(50, 50, 0, 255).to_u32()));
        Ok(())
    }
}
