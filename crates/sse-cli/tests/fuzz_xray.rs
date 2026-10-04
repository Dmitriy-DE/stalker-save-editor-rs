//! Deterministic mutations of the X-Ray save parser and hostile-size regression.

#[path = "fuzz_support/mod.rs"]
mod support;

const SEED: &[u8] = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");

#[test]
fn xray_save_2000_deterministic_mutations() {
    support::run(SEED, 2_000, 0x0058_425a_9151_d001, |input| {
        let _ = sse_xray::Save::read(input);
    });
}

#[test]
#[ignore = "100,000 deterministic X-Ray mutations"]
fn xray_save_100000_deterministic_mutations() {
    support::run(SEED, 100_000, 0x0058_425a_9151_d001, |input| {
        let _ = sse_xray::Save::read(input);
    });
}

#[test]
fn xray_container_rejects_a_256_mib_declared_image_before_allocating_it() {
    let mut packed = Vec::new();
    packed.extend_from_slice(&u32::MAX.to_le_bytes());
    packed.extend_from_slice(&6_u32.to_le_bytes());
    packed.extend_from_slice(&0x1000_0001_u32.to_le_bytes());
    assert!(sse_xray::container::Container::read(&packed).is_err());
}
