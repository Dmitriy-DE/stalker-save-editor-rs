//! Tests for save preview extraction and S2 campaign metadata.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use sse_content::dds::RgbaImage;
use sse_content::preview::{parse_campaigns, PreviewCache, Stalker2SlotMeta};
use std::path::PathBuf;
use std::time::SystemTime;

#[test]
fn preview_cache_enforces_bounded_memory_limit() {
    let mut cache = PreviewCache::new(1024 * 1024); // 1 MiB limit
    assert_eq!(cache.max_memory_bytes(), 1024 * 1024);
    assert_eq!(cache.current_memory_bytes(), 0);

    // Each 256x256 image is 256 * 256 * 4 = 262,144 bytes (~256 KiB)
    for i in 0..10 {
        let p = PathBuf::from(format!("/saves/save_{i}.sav"));
        let img = RgbaImage::new(256, 256, vec![0_u8; 256 * 256 * 4]);
        cache.insert(p, img);
        assert!(cache.current_memory_bytes() <= 1024 * 1024);
    }

    // At most 4 images fit in 1 MiB
    assert!(cache.current_memory_bytes() <= 1024 * 1024);
}

#[test]
fn stalker2_slot_meta_slug_formatting() {
    let meta = Stalker2SlotMeta {
        slot_guid: "1234567890ABCDEF1234567890ABCDEF".to_string(),
        region_key: "sid_locations_region_iron_forest_name".to_string(),
        quest_key: "E08_MQ01".to_string(),
        play_hours: 12.5,
        saved_at_utc: SystemTime::now(),
    };

    assert_eq!(meta.region_slug(), "iron_forest");
}

#[test]
fn campaigns_save_record_parser_handles_empty_or_synthetic_input() {
    // Empty input returns empty map safely without panicking
    assert!(parse_campaigns(&[]).is_empty());
    assert!(parse_campaigns(b"invalid header data").is_empty());

    // Build synthetic campaign payload
    let mut data = Vec::new();
    data.extend_from_slice(b"HeaderMarker\0"); // 13 bytes
    data.push(b'x'); // name byte
    data.push(0); // null terminator

    // One record:
    // 4 bytes: id (u32)
    data.extend_from_slice(&1_u32.to_le_bytes());
    // 16 bytes: GUID (four u32s)
    data.extend_from_slice(&0x11223344_u32.to_le_bytes());
    data.extend_from_slice(&0x55667788_u32.to_le_bytes());
    data.extend_from_slice(&0x99AABBCC_u32.to_le_bytes());
    data.extend_from_slice(&0xDDEEFF00_u32.to_le_bytes());
    // 8 bytes: kind + build
    data.extend_from_slice(&0_u64.to_le_bytes());
    // 8 bytes: ticks (after 1970)
    let ticks = 630_000_000_000_000_000_i64;
    data.extend_from_slice(&ticks.to_le_bytes());
    // 4 bytes: play seconds (7200.0s = 2.0 hours)
    data.extend_from_slice(&7200.0_f32.to_le_bytes());

    // String 1 (region): index 0 (new) -> len 6 -> "Region"
    data.extend_from_slice(&0_u16.to_le_bytes());
    data.extend_from_slice(&6_u16.to_le_bytes());
    data.extend_from_slice(b"Region");

    // String 2 (quest): index 1 (new) -> len 5 -> "Quest"
    data.extend_from_slice(&1_u16.to_le_bytes());
    data.extend_from_slice(&5_u16.to_le_bytes());
    data.extend_from_slice(b"Quest");

    // 6 bytes trailer
    data.extend_from_slice(&[0; 6]);
    data.extend_from_slice(b"Achievements");

    let parsed = parse_campaigns(&data);
    assert_eq!(parsed.len(), 1);
    let record = parsed.values().next().unwrap();
    assert_eq!(record.region_key, "Region");
    assert_eq!(record.quest_key, "Quest");
    assert!((record.play_hours - 2.0).abs() < 1e-4);
}
