#![allow(clippy::expect_used, missing_docs)]

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn developer_binary_owns_screenshot_modes() {
    let output = Command::new(env!("CARGO_BIN_EXE_sse-ui-dev"))
        .arg("--screenshot")
        .output()
        .expect("developer binary should start");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("usage: --screenshot"), "unexpected error: {stderr}");
}

#[test]
fn developer_binary_stages_enhanced_edition_packages() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("sse-ui-dev-ee-{}-{nonce}", std::process::id()));
    let output_dir = root.join("packages/clear-sky");
    let data_dir = root.join("app-data");
    std::fs::create_dir_all(&data_dir).expect("isolated data directory should be created");
    let output = Command::new(env!("CARGO_BIN_EXE_sse-ui-dev"))
        .args(["--package-ee", "cs"])
        .arg(&output_dir)
        .env("STALKER_SAVE_EDITOR_DATA", &data_dir)
        .output()
        .expect("developer binary should start");

    assert!(
        output.status.success(),
        "package staging failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output_dir.join("save_editor_companion_cs.xrp").is_file());
    let descriptor = std::fs::read_to_string(output_dir.join("desc.json")).expect("descriptor exists");
    assert!(descriptor.contains("\"game\": \"cs\""));
    assert!(descriptor.contains("\"published_file_id\": 0"));
    assert!(descriptor.contains(&format!("\"version\": \"{}\"", env!("CARGO_PKG_VERSION"))));
    assert!(data_dir.is_dir(), "dev binary should use the isolated data directory");
    std::fs::remove_dir_all(root).expect("temporary package should be removed");
}

#[test]
fn developer_binary_rejects_unknown_enhanced_edition_game() {
    let output = Command::new(env!("CARGO_BIN_EXE_sse-ui-dev"))
        .args(["--package-ee", "s2", "/tmp/sse-ui-dev-invalid-ee"])
        .output()
        .expect("developer binary should start");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--package-ee expects soc, cs, or cop"),
        "unexpected error: {stderr}"
    );
}
