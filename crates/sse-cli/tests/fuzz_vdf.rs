//! Deterministic mutations of the Valve VDF reader.

#[path = "fuzz_support/mod.rs"]
mod support;

const SEED: &[u8] = include_bytes!("../../../fixtures/synthetic/steam-vdf/libraryfolders.vdf");

#[test]
fn vdf_2000_deterministic_mutations() {
    support::run(SEED, 2_000, 0x0056_4446_0001, |input| {
        if let Ok(text) = std::str::from_utf8(input) {
            let _ = sse_codecs::vdf::parse(text);
        }
    });
}

#[test]
#[ignore = "100,000 deterministic VDF mutations"]
fn vdf_100000_deterministic_mutations() {
    support::run(SEED, 100_000, 0x0056_4446_0001, |input| {
        if let Ok(text) = std::str::from_utf8(input) {
            let _ = sse_codecs::vdf::parse(text);
        }
    });
}
