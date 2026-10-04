//! Deterministic mutations of the PNG decoder and hostile-dimension regression.

#[path = "fuzz_support/mod.rs"]
mod support;

const SEED: &[u8] = include_bytes!("../../../fixtures/fuzz/one-pixel.png");

#[test]
fn one_pixel_png_fixture_decodes() {
    assert!(sse_codecs::png::decode(SEED).is_ok());
}

#[test]
fn png_2000_deterministic_mutations() {
    support::run(SEED, 2_000, 0x0050_4e47_0001, |input| {
        let _ = sse_codecs::png::decode(input);
    });
}

#[test]
#[ignore = "100,000 deterministic PNG mutations"]
fn png_100000_deterministic_mutations() {
    support::run(SEED, 100_000, 0x0050_4e47_0001, |input| {
        let _ = sse_codecs::png::decode(input);
    });
}

#[test]
fn png_rejects_hostile_pixel_count_before_inflating() {
    let mut input = SEED.to_vec();
    let Some(dimensions) = input.get_mut(16..24) else {
        panic!("PNG fuzz fixture is shorter than IHDR");
    };
    let Some(width) = dimensions.get_mut(..4) else {
        panic!("PNG fuzz fixture has a truncated IHDR width");
    };
    width.copy_from_slice(&16_384_u32.to_be_bytes());
    let Some(height) = dimensions.get_mut(4..8) else {
        panic!("PNG fuzz fixture has a truncated IHDR height");
    };
    height.copy_from_slice(&16_384_u32.to_be_bytes());
    let checksum = sse_codecs::crc32::crc32(input.get(12..29).unwrap_or_default());
    let Some(crc) = input.get_mut(29..33) else {
        panic!("PNG fuzz fixture has a truncated IHDR CRC");
    };
    crc.copy_from_slice(&checksum.to_be_bytes());
    let Err(error) = sse_codecs::png::decode(&input) else {
        panic!("PNG decoder accepted a hostile oversized image");
    };
    assert!(error.to_string().contains("pixel count exceeds limit"));
}
