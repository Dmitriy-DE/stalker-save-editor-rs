//! Deterministic mutations of the TrueType/OpenType parser.

#[path = "fuzz_support/mod.rs"]
mod support;

const SEED: &[u8] = include_bytes!("../../../fixtures/fonts/LiberationSansNarrow-Regular.ttf");

#[test]
fn ttf_2000_deterministic_mutations() {
    support::run(SEED, 2_000, 0x0054_5446_0001, |input| {
        let _ = sse_codecs::font::Font::parse(input, 0);
    });
}

#[test]
#[ignore = "100,000 deterministic TTF mutations"]
fn ttf_100000_deterministic_mutations() {
    support::run(SEED, 100_000, 0x0054_5446_0001, |input| {
        let _ = sse_codecs::font::Font::parse(input, 0);
    });
}
