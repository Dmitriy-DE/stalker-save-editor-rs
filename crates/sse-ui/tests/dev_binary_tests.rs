#![allow(clippy::expect_used, missing_docs)]

use std::process::Command;

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
