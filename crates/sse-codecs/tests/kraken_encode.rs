//! Round-trip and compression-ratio checks for the safe Kraken encoder.

use sse_codecs::{kraken, kraken_encode};
use std::fs;
use std::path::{Path, PathBuf};

struct Vector {
    name: String,
    raw: String,
    size: usize,
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("kraken")
}

fn string_field(block: &str, field: &str) -> Option<String> {
    let marker = format!("\"{field}\"");
    let field = block.get(block.find(&marker)?.checked_add(marker.len())?..)?;
    let value = field.get(field.find(':')?.checked_add(1)?..)?.trim_start();
    let value = value.strip_prefix('"')?;
    Some(value.get(..value.find('"')?)?.to_owned())
}

fn usize_field(block: &str, field: &str) -> Option<usize> {
    let marker = format!("\"{field}\"");
    let field = block.get(block.find(&marker)?.checked_add(marker.len())?..)?;
    let value = field.get(field.find(':')?.checked_add(1)?..)?.trim_start();
    let end = value
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(value.len());
    value.get(..end)?.parse().ok()
}

fn vectors() -> Vec<Vector> {
    let manifest = fs::read_to_string(fixture_root().join("manifest.json"))
        .unwrap_or_else(|error| panic!("read Kraken manifest: {error}"));
    manifest
        .split('{')
        .skip(1)
        .filter_map(|block| {
            let block = block.split('}').next()?;
            Some(Vector {
                name: string_field(block, "name")?,
                raw: string_field(block, "raw")?,
                size: usize_field(block, "size")?,
            })
        })
        .collect()
}

#[test]
fn all_reference_vectors_round_trip_through_the_encoder() {
    let root = fixture_root();
    let vectors = vectors();
    assert_eq!(vectors.len(), 23, "manifest must contain all reference vectors");

    for vector in vectors {
        let packed = fs::read(root.join(format!("{}.kraken", vector.name)))
            .unwrap_or_else(|error| panic!("read {}: {error}", vector.name));
        let mut original = vec![0; vector.size];
        kraken::decompress_into(&packed, &mut original)
            .unwrap_or_else(|error| panic!("decode {}: {error:?}", vector.name));
        let raw = fs::read(root.join(&vector.raw)).unwrap_or_else(|error| panic!("read raw {}: {error}", vector.raw));
        assert_eq!(original, raw, "{} source vector differs from raw fixture", vector.name);
        let encoded = kraken_encode::compress(&original);
        let level_four = vector
            .name
            .rsplit_once("-l")
            .map(|(base, _)| format!("{base}-l4.kraken"))
            .unwrap_or_else(|| panic!("{} has no compression level", vector.name));
        let reference = fs::read(root.join(level_four))
            .unwrap_or_else(|error| panic!("read level-4 reference for {}: {error}", vector.name));
        assert!(
            encoded.len().saturating_mul(100) <= reference.len().saturating_mul(110),
            "{} encoder output {} bytes exceeds level-4 reference {} bytes by over 10%",
            vector.name,
            encoded.len(),
            reference.len()
        );
        let mut decoded = vec![0; original.len()];
        kraken::decompress_into(&encoded, &mut decoded)
            .unwrap_or_else(|error| panic!("round-trip decode {}: {error:?}", vector.name));
        assert_eq!(decoded, original, "{} round-trip changed bytes", vector.name);
    }
}

#[test]
fn deterministic_random_and_repetitive_inputs_round_trip() {
    let mut state = 0x91e1_0da5_u32;
    for case in 0..2_000_usize {
        let scale = case % 21;
        let length = (1_usize << scale).min(1 << 20);
        let mut input = Vec::with_capacity(length);
        for index in 0..length {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            input.push(if case % 2 == 0 {
                u8::try_from(index % (case % 31 + 1))
                    .unwrap_or_else(|error| panic!("repetitive byte conversion: {error}"))
            } else {
                u8::try_from(state & 0xff).unwrap_or_else(|error| panic!("random byte conversion: {error}"))
            });
        }

        let encoded = kraken_encode::compress(&input);
        let mut decoded = vec![0; input.len()];
        kraken::decompress_into(&encoded, &mut decoded).unwrap_or_else(|error| {
            panic!(
                "case {case}, {} bytes: {error:?}; encoded_len={}",
                input.len(),
                encoded.len()
            )
        });
        assert_eq!(decoded, input, "case {case}, {} bytes", input.len());
    }
}

#[test]
#[ignore = "local release-mode throughput measurement"]
fn fixture_encoder_throughput_exceeds_target() {
    let root = fixture_root();
    let names = [
        "text-like.raw",
        "save-like-small.raw",
        "save-like-multi-block.raw",
        "random.raw",
    ];
    let inputs = names
        .iter()
        .map(|name| fs::read(root.join(name)).unwrap_or_else(|error| panic!("read {name}: {error}")))
        .collect::<Vec<_>>();
    let mut total_bytes = 0_u128;
    let started = std::time::Instant::now();
    for (name, input) in names.iter().zip(&inputs) {
        let fixture_started = std::time::Instant::now();
        let mut fixture_bytes = 0_u128;
        for _ in 0..16 {
            let encoded = kraken_encode::compress(std::hint::black_box(input.as_slice()));
            fixture_bytes = fixture_bytes.saturating_add(u128::try_from(input.len()).unwrap_or_default());
            std::hint::black_box(encoded);
        }
        let fixture_nanos = fixture_started.elapsed().as_nanos().max(1);
        let fixture_rate = fixture_bytes
            .saturating_mul(1_000_000_000)
            .checked_div(fixture_nanos)
            .unwrap_or_default()
            .checked_div(1_048_576)
            .unwrap_or_default();
        eprintln!("{name}: {fixture_rate} MiB/s");
        total_bytes = total_bytes.saturating_add(fixture_bytes);
    }
    let elapsed_nanos = started.elapsed().as_nanos().max(1);
    let throughput_mib = total_bytes
        .saturating_mul(1_000_000_000)
        .checked_div(elapsed_nanos)
        .unwrap_or_default()
        .checked_div(1_048_576)
        .unwrap_or_default();
    eprintln!("fixture encoder: {throughput_mib} MiB/s over {total_bytes} input bytes");
    assert!(throughput_mib >= 50, "encoder throughput was below 50 MiB/s");
}

#[test]
#[ignore = "local release-mode compressed-size report"]
fn fixture_encoder_size_report() {
    let root = fixture_root();
    let mut seen_raw = std::collections::BTreeSet::new();
    let mut total_raw = 0_usize;
    let mut total_encoded = 0_usize;
    let mut total_reference = 0_usize;
    for vector in vectors() {
        if !seen_raw.insert(vector.raw.clone()) {
            continue;
        }
        let raw = fs::read(root.join(&vector.raw)).unwrap_or_else(|error| panic!("read {}: {error}", vector.raw));
        let encoded = kraken_encode::compress(&raw);
        let level_four = vector
            .name
            .rsplit_once("-l")
            .map(|(base, _)| format!("{base}-l4.kraken"))
            .unwrap_or_else(|| panic!("{} has no compression level", vector.name));
        let reference = fs::read(root.join(&level_four)).unwrap_or_else(|error| panic!("read {level_four}: {error}"));
        let reference_percent = encoded
            .len()
            .saturating_mul(100)
            .checked_div(reference.len().max(1))
            .unwrap_or_default();
        eprintln!(
            "{}: encoded={} B, raw={} B, level4={} B, encoded/level4={} %",
            vector.raw,
            encoded.len(),
            raw.len(),
            reference.len(),
            reference_percent
        );
        total_raw = total_raw.saturating_add(raw.len());
        total_encoded = total_encoded.saturating_add(encoded.len());
        total_reference = total_reference.saturating_add(reference.len());
    }
    eprintln!(
        "total: encoded={} B / raw={} B; encoded={} B / level4={} B",
        total_encoded, total_raw, total_encoded, total_reference
    );
}
