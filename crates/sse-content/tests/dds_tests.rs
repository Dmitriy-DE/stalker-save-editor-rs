//! DDS image decoder tests.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_content::dds::DdsImage;
use sse_core::Error;

fn solid_dxt1(width: usize, height: usize, transparent: bool) -> Vec<u8> {
    let blocks = width.div_ceil(4) * height.div_ceil(4);
    let mut data = vec![0u8; 128 + blocks * 8];
    data[0..4].copy_from_slice(b"DDS ");
    data[4..8].copy_from_slice(&124u32.to_le_bytes());
    data[12..16].copy_from_slice(&(height as u32).to_le_bytes());
    data[16..20].copy_from_slice(&(width as u32).to_le_bytes());
    data[80..84].copy_from_slice(&0x4u32.to_le_bytes());
    data[84..88].copy_from_slice(b"DXT1");

    for block in 0..blocks {
        let offset = 128 + block * 8;
        if transparent {
            // c0 <= c1 selects 3-color mode; index 3 is transparent black.
            data[offset..offset + 2].copy_from_slice(&0x0000u16.to_le_bytes());
            data[offset + 2..offset + 4].copy_from_slice(&0xF800u16.to_le_bytes());
            data[offset + 4..offset + 8].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        } else {
            data[offset..offset + 2].copy_from_slice(&0xF800u16.to_le_bytes());
            data[offset + 2..offset + 4].copy_from_slice(&0x0000u16.to_le_bytes());
            data[offset + 4..offset + 8].copy_from_slice(&0x0000_0000u32.to_le_bytes());
        }
    }

    data
}

fn uncompressed_dds(width: usize, height: usize, pitch: u32, red_mask: u32) -> Vec<u8> {
    let mut data = vec![0u8; 128 + width * height * 4];
    data[0..4].copy_from_slice(b"DDS ");
    data[12..16].copy_from_slice(&(height as u32).to_le_bytes());
    data[16..20].copy_from_slice(&(width as u32).to_le_bytes());
    data[20..24].copy_from_slice(&pitch.to_le_bytes());
    data[80..84].copy_from_slice(&0x40u32.to_le_bytes());
    data[88..92].copy_from_slice(&32u32.to_le_bytes());
    data[92..96].copy_from_slice(&red_mask.to_le_bytes());
    data[96..100].copy_from_slice(&0x0000_FF00u32.to_le_bytes());
    data[100..104].copy_from_slice(&0x0000_00FFu32.to_le_bytes());
    data
}

#[test]
fn decodes_dxt1_including_transparency() {
    let transparent_img = DdsImage::decode(&solid_dxt1(4, 4, true)).unwrap();
    assert_eq!(transparent_img.pixels[3], 0);

    let opaque_img = DdsImage::decode(&solid_dxt1(4, 4, false)).unwrap();
    assert_eq!(opaque_img.pixels[3], 255);
    assert_eq!(opaque_img.pixels[0], 255); // Red
}

#[test]
fn a_dds_with_an_impossible_pitch_is_invalid_data() {
    let dds1 = uncompressed_dds(4, 4, 0x8000_0000, 0x00FF_0000);
    assert!(matches!(DdsImage::decode(&dds1), Err(Error::Damaged(_))));

    let dds2 = uncompressed_dds(4, 4, 3, 0x00FF_0000);
    assert!(matches!(DdsImage::decode(&dds2), Err(Error::Damaged(_))));
}

#[test]
fn a_dds_with_a_32_bit_channel_mask_is_invalid_data_not_a_division_by_zero() {
    assert!(matches!(
        DdsImage::decode(&uncompressed_dds(4, 4, 16, 0xFFFF_FFFF)),
        Err(Error::Damaged(_))
    ));
    assert!(matches!(
        DdsImage::decode(&uncompressed_dds(4, 4, 16, 0x00FF_00FF)),
        Err(Error::Damaged(_))
    ));
    let valid = DdsImage::decode(&uncompressed_dds(4, 4, 16, 0x00FF_0000)).unwrap();
    assert_eq!(valid.width, 4);
}

#[test]
fn rejects_truncated_dds() {
    let data = solid_dxt1(8, 8, false);
    let truncated = &data[..data.len() - 4];
    assert!(matches!(DdsImage::decode(truncated), Err(Error::Damaged(_))));
    assert!(matches!(DdsImage::decode(b"NOTADDS"), Err(Error::Damaged(_))));
}

#[test]
fn crops_decoded_image_correctly() {
    let img = DdsImage::decode(&solid_dxt1(8, 8, false)).unwrap();
    let crop = img.crop(2, 2, 4, 4).unwrap();
    assert_eq!(crop.width, 4);
    assert_eq!(crop.height, 4);
    assert_eq!(crop.pixels.len(), 4 * 4 * 4);

    assert!(img.crop(8, 8, 4, 4).is_none());
    assert!(img.crop(0, 0, 0, 4).is_none());
}
