//! Deterministic mutations of the LZHUF decoder.

#[path = "fuzz_support/mod.rs"]
mod support;

const SEED: &[u8] = include_bytes!("../../../fixtures/fuzz/empty.lzhuf");

#[test]
fn empty_lzhuf_fixture_decodes() {
    assert!(matches!(sse_codecs::lzhuf::decode(SEED), Ok(output) if output.is_empty()));
}

#[test]
fn lzhuf_2000_deterministic_mutations() {
    support::run(SEED, 2_000, 0x4c5a_4855_4601, |input| {
        let _ = sse_codecs::lzhuf::decode(input);
    });
}

#[test]
#[ignore = "100,000 deterministic LZHUF mutations"]
fn lzhuf_100000_deterministic_mutations() {
    support::run(SEED, 100_000, 0x4c5a_4855_4601, |input| {
        let _ = sse_codecs::lzhuf::decode(input);
    });
}
