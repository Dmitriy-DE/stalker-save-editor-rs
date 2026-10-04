//! Deterministic mutations of the S2 save parser and hostile-size regression.

#[path = "fuzz_support/mod.rs"]
mod support;

const SEED: &[u8] = include_bytes!("../../../fixtures/synthetic/writer-s2-money/s2-money-source.sav");

#[test]
fn s2_save_2000_deterministic_mutations() {
    support::run(SEED, 2_000, 0x0053_329c_e42a_7401, |input| {
        let _ = sse_s2::S2Save::from_bytes(input);
    });
}

#[test]
#[ignore = "100,000 deterministic S2 mutations"]
fn s2_save_100000_deterministic_mutations() {
    support::run(SEED, 100_000, 0x0053_329c_e42a_7401, |input| {
        let _ = sse_s2::S2Save::from_bytes(input);
    });
}

#[test]
fn s2_container_rejects_a_256_mib_declared_image_before_allocating_it() {
    let mut packed = Vec::new();
    packed.extend_from_slice(&0x1000_0001_u32.to_le_bytes());
    packed.extend_from_slice(&[0x8c, 0x06]);
    let checksum = sse_codecs::crc32::crc32(&packed);
    packed.extend_from_slice(&checksum.to_le_bytes());
    assert!(sse_s2::S2Container::from_bytes(&packed).is_err());
}
