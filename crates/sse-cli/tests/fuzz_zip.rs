//! Deterministic mutations of the bounded ZIP reader.

#[path = "fuzz_support/mod.rs"]
mod support;

const SEED: &[u8] = include_bytes!("../../../fixtures/fuzz/one-file.zip");

#[test]
fn one_file_zip_fixture_reads() {
    assert!(sse_codecs::zip::read(SEED, 1024 * 1024).is_ok());
}

#[test]
fn zip_2000_deterministic_mutations() {
    support::run(SEED, 2_000, 0x005a_4950_0001, |input| {
        let _ = sse_codecs::zip::read(input, 1024 * 1024);
    });
}

#[test]
#[ignore = "100,000 deterministic ZIP mutations"]
fn zip_100000_deterministic_mutations() {
    support::run(SEED, 100_000, 0x005a_4950_0001, |input| {
        let _ = sse_codecs::zip::read(input, 1024 * 1024);
    });
}
