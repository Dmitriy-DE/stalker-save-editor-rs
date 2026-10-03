//! Differential contract tests for the independent Kraken implementation.

use std::fs;
use std::path::{Path, PathBuf};

use sse_codecs::{kraken, kraken_c3a};
use sse_core::Error;

struct Fixture {
    name: String,
    raw: String,
    size: usize,
}

fn quoted_value(body: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\"");
    let start = body.find(&marker)?.checked_add(marker.len())?;
    let tail = body.get(start..)?.trim_start();
    let tail = tail.strip_prefix(':')?.trim_start();
    let tail = tail.strip_prefix('"')?;
    let end = tail.find('"')?;
    Some(tail.get(..end)?.to_owned())
}

fn fixture_manifest(text: &str) -> Vec<Fixture> {
    text.split('{')
        .skip(1)
        .filter_map(|block| {
            let body = block.split('}').next()?;
            let name = quoted_value(body, "name")?;
            let raw = quoted_value(body, "raw")?;
            let size = body
                .split_once("\"size\"")?
                .1
                .split_once(':')?
                .1
                .trim_start()
                .split(|ch: char| !ch.is_ascii_digit())
                .next()?
                .parse()
                .ok()?;
            Some(Fixture { name, raw, size })
        })
        .collect()
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/kraken")
}

fn packed_fixture(name: &str) -> Vec<u8> {
    fs::read(fixture_root().join(format!("{name}.kraken")))
        .unwrap_or_else(|error| panic!("read packed {name}: {error}"))
}

#[test]
fn independent_decoder_matches_all_23_manifest_vectors_and_the_main_decoder() {
    let root = fixture_root();
    let manifest =
        fs::read_to_string(root.join("manifest.json")).unwrap_or_else(|error| panic!("read Kraken manifest: {error}"));
    let fixtures = fixture_manifest(&manifest);
    assert_eq!(fixtures.len(), 23, "expected every checked-in Kraken reference vector");

    for fixture in fixtures {
        let packed = fs::read(root.join(format!("{}.kraken", fixture.name)))
            .unwrap_or_else(|error| panic!("read packed {}: {error}", fixture.name));
        let raw = fs::read(root.join(&fixture.raw)).unwrap_or_else(|error| panic!("read raw {}: {error}", fixture.raw));
        assert_eq!(raw.len(), fixture.size, "{} raw size", fixture.name);

        let mut primary = vec![0_u8; fixture.size];
        kraken::decompress_into(&packed, &mut primary)
            .unwrap_or_else(|error| panic!("main decode {}: {error:?}", fixture.name));

        let mut independent = vec![0_u8; fixture.size];
        kraken_c3a::decompress_into(&packed, &mut independent)
            .unwrap_or_else(|error| panic!("independent decode {}: {error:?}", fixture.name));
        assert_eq!(independent, raw, "independent output differs for {}", fixture.name);
        assert_eq!(independent, primary, "decoders disagree for {}", fixture.name);
    }
}

#[test]
fn every_text_vector_truncation_is_rejected() {
    let packed = packed_fixture("text-like-l1");
    for length in 0..packed.len() {
        let prefix = packed.get(..length).unwrap_or_default();
        let mut output = vec![0_u8; 300_000];
        assert!(
            matches!(kraken_c3a::decompress_into(prefix, &mut output), Err(Error::Damaged(_))),
            "accepted truncated stream prefix of {length} bytes"
        );
    }
}

#[test]
fn flipped_block_header_bits_are_rejected() {
    let original = packed_fixture("save-like-small-l1");
    for (offset, mask) in [(0, 0x10), (1, 0x01)] {
        let mut changed = original.clone();
        let byte = changed
            .get_mut(offset)
            .unwrap_or_else(|| panic!("missing header byte {offset}"));
        *byte ^= mask;
        let mut output = vec![0_u8; 70_000];
        assert!(matches!(
            kraken_c3a::decompress_into(&changed, &mut output),
            Err(Error::Damaged(_))
        ));
    }
}

#[test]
fn hostile_quantum_length_is_rejected_before_large_allocation() {
    let hostile = [0x0c, 0x06, 0x03, 0xff, 0xfe];
    let mut output = vec![0_u8; 70_000];
    assert!(matches!(
        kraken_c3a::decompress_into(&hostile, &mut output),
        Err(Error::Damaged(_))
    ));
}

#[test]
fn fixed_seed_text_vector_mutations_never_panic() {
    let packed = packed_fixture("text-like-l1");
    let mut state = 0x6d2b_79f5_u32;
    let mut output = vec![0_u8; 300_000];
    for _ in 0..128 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let offset = usize::try_from(state).unwrap_or_default() % packed.len();
        state = state.rotate_left(11).wrapping_add(0xa5a5_5a5a);
        let mask = 1_u8.checked_shl(state & 7).unwrap_or_default();
        let mut changed = packed.clone();
        let byte = changed
            .get_mut(offset)
            .unwrap_or_else(|| panic!("mutation offset {offset} is outside stream"));
        *byte ^= mask;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            kraken_c3a::decompress_into(&changed, &mut output)
        }));
        assert!(result.is_ok(), "mutation at byte {offset} panicked");
    }
}

#[test]
#[ignore = "manual Release timing for all independent Kraken reference vectors"]
fn release_manifest_decoder_throughput_measurement() {
    let root = fixture_root();
    let manifest =
        fs::read_to_string(root.join("manifest.json")).unwrap_or_else(|error| panic!("read Kraken manifest: {error}"));
    let fixtures = fixture_manifest(&manifest);
    let packed: Vec<Vec<u8>> = fixtures
        .iter()
        .map(|fixture| {
            fs::read(root.join(format!("{}.kraken", fixture.name)))
                .unwrap_or_else(|error| panic!("read packed {}: {error}", fixture.name))
        })
        .collect();
    let mut outputs: Vec<Vec<u8>> = fixtures.iter().map(|fixture| vec![0; fixture.size]).collect();
    let mut primary_outputs: Vec<Vec<u8>> = fixtures.iter().map(|fixture| vec![0; fixture.size]).collect();
    let rounds = 3_usize;
    let start = std::time::Instant::now();
    for _ in 0..rounds {
        for (source, output) in packed.iter().zip(outputs.iter_mut()) {
            kraken_c3a::decompress_into(std::hint::black_box(source), std::hint::black_box(output))
                .unwrap_or_else(|error| panic!("independent decoder failed during timing: {error:?}"));
        }
    }
    let independent_elapsed = start.elapsed();
    let start = std::time::Instant::now();
    for _ in 0..rounds {
        for (source, output) in packed.iter().zip(primary_outputs.iter_mut()) {
            kraken::decompress_into(std::hint::black_box(source), std::hint::black_box(output))
                .unwrap_or_else(|error| panic!("main decoder failed during timing: {error:?}"));
        }
    }
    let primary_elapsed = start.elapsed();
    let decodes = fixtures.len().saturating_mul(rounds);
    eprintln!(
        "C3a: {decodes} fixture decodes in {:.3} ms ({:.3} ms/decode); main: {:.3} ms ({:.3} ms/decode)",
        independent_elapsed.as_secs_f64() * 1000.0,
        independent_elapsed.as_secs_f64() * 1000.0 / decodes as f64,
        primary_elapsed.as_secs_f64() * 1000.0,
        primary_elapsed.as_secs_f64() * 1000.0 / decodes as f64
    );
}
