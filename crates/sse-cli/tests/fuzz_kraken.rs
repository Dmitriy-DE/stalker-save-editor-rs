//! Deterministic mutations of the Kraken vector decoder.

#[path = "fuzz_support/mod.rs"]
mod support;

const PACKED: &[u8] = include_bytes!("../../../fixtures/kraken/save-like-small-l1.kraken");
const IMAGE: &[u8] = include_bytes!("../../../fixtures/kraken/save-like-small.raw");

fn parser() -> impl FnMut(&[u8]) + Send + 'static {
    let mut output = vec![0_u8; IMAGE.len()];
    move |input| {
        output.fill(0);
        let _ = sse_codecs::kraken::decompress_into(input, &mut output);
    }
}

#[test]
fn kraken_vector_2000_deterministic_mutations() {
    support::run(PACKED, 2_000, 0x004b_5241_4b45_4e01, parser());
}

#[test]
#[ignore = "100,000 deterministic Kraken mutations"]
fn kraken_vector_100000_deterministic_mutations() {
    support::run(PACKED, 100_000, 0x004b_5241_4b45_4e01, parser());
}
