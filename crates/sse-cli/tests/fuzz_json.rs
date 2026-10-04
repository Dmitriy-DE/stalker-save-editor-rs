//! Deterministic mutations of the streaming JSON reader.

#[path = "fuzz_support/mod.rs"]
mod support;

const SEED: &[u8] = include_bytes!("../../../fixtures/golden/steam-vdf/libraryfolders.json");

fn parse(input: &[u8]) {
    let mut reader = sse_codecs::json::Reader::new(input);
    while let Ok(Some(_)) = reader.next_event() {}
}

#[test]
fn json_2000_deterministic_mutations() {
    support::run(SEED, 2_000, 0x004a_534f_4e01, parse);
}

#[test]
#[ignore = "100,000 deterministic JSON mutations"]
fn json_100000_deterministic_mutations() {
    support::run(SEED, 100_000, 0x004a_534f_4e01, parse);
}
