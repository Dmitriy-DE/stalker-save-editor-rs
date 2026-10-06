//! App icon raster shared by the native window backends.

#[cfg(all(target_os = "macos", not(test)))]
use sse_core::Result;
#[cfg(any(all(unix, not(target_os = "macos")), test))]
use sse_core::{Error, Result};
use std::sync::OnceLock;

pub(crate) const APP_ICON_SIZE: u32 = 64;
const SUPERSAMPLE: usize = 4;
const BACKGROUND: [u8; 3] = [21, 24, 20];
const ACCENT: [u8; 3] = [214, 166, 45];
#[cfg(test)]
const BRAND_STAR_PATH: &str = "M128 38l18 58 61-12-46 42 40 48-59-18-14 62-14-62-59 18 40-48-46-42 61 12z";
const STAR: [(f64, f64); 12] = [
    (128.0, 38.0),
    (146.0, 96.0),
    (207.0, 84.0),
    (161.0, 126.0),
    (201.0, 174.0),
    (142.0, 156.0),
    (128.0, 218.0),
    (114.0, 156.0),
    (55.0, 174.0),
    (95.0, 126.0),
    (49.0, 84.0),
    (110.0, 96.0),
];

pub(crate) fn app_icon_rgba() -> &'static [u8] {
    static PIXELS: OnceLock<Vec<u8>> = OnceLock::new();
    PIXELS.get_or_init(rasterize_app_icon).as_slice()
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn app_icon_png() -> Result<Vec<u8>> {
    sse_codecs::png_encode::encode_rgba8(APP_ICON_SIZE, APP_ICON_SIZE, app_icon_rgba())
}

#[cfg(any(all(unix, not(target_os = "macos")), test))]
pub(crate) fn rgba_to_argb32(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u32>> {
    let expected = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| Error::Refused("icon dimensions overflow".to_owned()))?;
    if rgba.len() != expected {
        return Err(Error::Refused("icon RGBA byte count mismatch".to_owned()));
    }
    Ok(rgba
        .chunks_exact(4)
        .map(|pixel| {
            u32::from_be_bytes([
                pixel.get(3).copied().unwrap_or_default(),
                pixel.first().copied().unwrap_or_default(),
                pixel.get(1).copied().unwrap_or_default(),
                pixel.get(2).copied().unwrap_or_default(),
            ])
        })
        .collect())
}

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
pub(crate) fn rgba_to_premultiplied_argb32(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u32>> {
    let expected = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| Error::Refused("icon dimensions overflow".to_owned()))?;
    if rgba.len() != expected {
        return Err(Error::Refused("icon RGBA byte count mismatch".to_owned()));
    }
    Ok(rgba
        .chunks_exact(4)
        .map(|pixel| {
            let alpha = u32::from(pixel.get(3).copied().unwrap_or_default());
            let premultiply = |channel: u8| {
                u32::from(channel)
                    .saturating_mul(alpha)
                    .saturating_add(127)
                    .checked_div(255)
                    .unwrap_or_default()
            };
            (alpha << 24)
                | (premultiply(pixel.first().copied().unwrap_or_default()) << 16)
                | (premultiply(pixel.get(1).copied().unwrap_or_default()) << 8)
                | premultiply(pixel.get(2).copied().unwrap_or_default())
        })
        .collect())
}

fn rasterize_app_icon() -> Vec<u8> {
    let side = usize::try_from(APP_ICON_SIZE).unwrap_or_default();
    let pixel_count = side.checked_mul(side).unwrap_or_default();
    let mut rgba = vec![0_u8; pixel_count.checked_mul(4).unwrap_or_default()];
    for y in 0..side {
        for x in 0..side {
            let mut color_sum = [0_u32; 3];
            let mut opaque_samples = 0_u32;
            for sub_y in 0..SUPERSAMPLE {
                for sub_x in 0..SUPERSAMPLE {
                    let x = f64::from(u32::try_from(x).unwrap_or_default())
                        + (f64::from(u32::try_from(sub_x).unwrap_or_default()) + 0.5)
                            / f64::from(u32::try_from(SUPERSAMPLE).unwrap_or(1));
                    let y = f64::from(u32::try_from(y).unwrap_or_default())
                        + (f64::from(u32::try_from(sub_y).unwrap_or_default()) + 0.5)
                            / f64::from(u32::try_from(SUPERSAMPLE).unwrap_or(1));
                    if let Some(sample) = app_icon_sample(x, y) {
                        opaque_samples = opaque_samples.saturating_add(1);
                        for (sum, channel) in color_sum.iter_mut().zip(sample) {
                            *sum = sum.saturating_add(u32::from(channel));
                        }
                    }
                }
            }
            let Some(pixel_index) = y.checked_mul(side).and_then(|row| row.checked_add(x)) else {
                continue;
            };
            let Some(offset) = pixel_index.checked_mul(4) else {
                continue;
            };
            if opaque_samples == 0 {
                continue;
            }
            for (channel, sum) in color_sum.into_iter().enumerate() {
                if let Some(destination) = rgba.get_mut(offset.saturating_add(channel)) {
                    *destination =
                        u8::try_from(sum.checked_div(opaque_samples).unwrap_or_default()).unwrap_or_default();
                }
            }
            if let Some(alpha) = rgba.get_mut(offset.saturating_add(3)) {
                let total_samples = u32::try_from(SUPERSAMPLE.saturating_mul(SUPERSAMPLE)).unwrap_or(1);
                *alpha = u8::try_from(
                    opaque_samples
                        .saturating_mul(255)
                        .checked_div(total_samples)
                        .unwrap_or(u32::MAX),
                )
                .unwrap_or(u8::MAX);
            }
        }
    }
    rgba
}

fn app_icon_sample(x: f64, y: f64) -> Option<[u8; 3]> {
    let scale = 256.0 / f64::from(APP_ICON_SIZE);
    if !inside_rounded_square(x, y, f64::from(APP_ICON_SIZE), 36.0 / scale) {
        return None;
    }
    let svg_x = x * scale;
    let svg_y = y * scale;
    let dx = svg_x - 128.0;
    let dy = svg_y - 128.0;
    let radius = dx.mul_add(dx, dy * dy).sqrt();
    if radius <= 24.0 {
        return Some(BACKGROUND);
    }
    if point_in_polygon(svg_x, svg_y, &STAR) || (72.0..=84.0).contains(&radius) {
        Some(ACCENT)
    } else {
        Some(BACKGROUND)
    }
}

fn inside_rounded_square(x: f64, y: f64, side: f64, radius: f64) -> bool {
    let dx = if x < radius {
        radius - x
    } else if x > side - radius {
        x - (side - radius)
    } else {
        0.0
    };
    let dy = if y < radius {
        radius - y
    } else if y > side - radius {
        y - (side - radius)
    } else {
        0.0
    };
    dx.mul_add(dx, dy * dy) <= radius * radius
}

fn point_in_polygon(x: f64, y: f64, polygon: &[(f64, f64)]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = polygon.len().saturating_sub(1);
    for current in 0..polygon.len() {
        let (Some(&(x1, y1)), Some(&(x2, y2))) = (polygon.get(current), polygon.get(previous)) else {
            return false;
        };
        if (y1 > y) != (y2 > y) {
            let crossing_x = (x2 - x1).mul_add((y - y1) / (y2 - y1), x1);
            if x < crossing_x {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: &[u8], x: usize, y: usize) -> [u8; 4] {
        let side = usize::try_from(APP_ICON_SIZE).unwrap_or_default();
        let Some(offset) = y
            .checked_mul(side)
            .and_then(|row| row.checked_add(x))
            .and_then(|index| index.checked_mul(4))
        else {
            return [0; 4];
        };
        let Some(end) = offset.checked_add(4) else {
            return [0; 4];
        };
        rgba.get(offset..end)
            .and_then(|bytes| bytes.try_into().ok())
            .unwrap_or([0; 4])
    }

    #[test]
    fn app_icon_uses_the_brand_geometry_and_transparent_rounded_corners() {
        let rgba = app_icon_rgba();
        assert_eq!(rgba.len(), 64 * 64 * 4);
        assert_eq!(pixel(rgba, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(rgba, 32, 32), [21, 24, 20, 255]);
        assert_eq!(pixel(rgba, 32, 12), [214, 166, 45, 255]);
        assert_eq!(pixel(rgba, 32, 16), [214, 166, 45, 255]);
    }

    #[test]
    fn app_icon_raster_geometry_matches_the_packaged_svg() {
        let packaged = include_str!("../../../packaging/icons/stalker-save-editor.svg");
        assert!(packaged.contains("viewBox=\"0 0 256 256\""));
        assert!(packaged.contains("fill=\"#151814\""));
        assert!(packaged.contains("stroke=\"#d6a62d\" stroke-width=\"12\""));
        assert!(packaged.contains(BRAND_STAR_PATH));
    }

    #[test]
    fn x11_argb_words_preserve_alpha_and_rgba_channel_order() {
        assert_eq!(rgba_to_argb32(1, 1, &[1, 2, 3, 4]).ok(), Some(vec![0x0401_0203]));
        assert_eq!(
            rgba_to_argb32(1, 1, &[1, 2, 3]).err().map(|e| e.to_string()),
            Some("icon RGBA byte count mismatch".to_owned())
        );
    }

    #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
    #[test]
    fn wayland_argb_words_premultiply_translucent_channels() {
        assert_eq!(
            rgba_to_premultiplied_argb32(1, 1, &[100, 50, 20, 128]).ok(),
            Some(vec![0x8032_190a])
        );
        assert_eq!(
            rgba_to_premultiplied_argb32(1, 1, &[200, 100, 50, 0]).ok(),
            Some(vec![0])
        );
    }

    #[test]
    fn encoded_icon_is_a_valid_rgba_png() {
        let encoded = app_icon_png().unwrap_or_else(|error| panic!("{error}"));
        let decoded = sse_codecs::png::decode(&encoded).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!((decoded.width, decoded.height), (APP_ICON_SIZE, APP_ICON_SIZE));
        assert_eq!(decoded.pixels, app_icon_rgba());
    }
}
