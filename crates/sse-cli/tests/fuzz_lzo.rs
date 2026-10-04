//! Deterministic mutations of the LZO1X decoder.

#[path = "fuzz_support/mod.rs"]
mod support;

const PACKED: &[u8] = include_bytes!("../../../fixtures/synthetic/lzo1x-literal-repeated.lzo");
const IMAGE: &[u8] = include_bytes!("../../../fixtures/synthetic/lzo1x-literal-repeated.raw");

#[test]
fn lzo_2000_deterministic_mutations() {
    support::run(PACKED, 2_000, 0x004c_5a4f_0101, |input| {
        let _ = sse_codecs::lzo1x::decompress(input, IMAGE.len());
    });
}

#[test]
#[ignore = "100,000 deterministic LZO mutations"]
fn lzo_100000_deterministic_mutations() {
    support::run(PACKED, 100_000, 0x004c_5a4f_0101, |input| {
        let _ = sse_codecs::lzo1x::decompress(input, IMAGE.len());
    });
}
