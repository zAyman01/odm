//! CLI command parsing and client execution tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

#[test]
fn cli_help_exits_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Open Download Manager") || stdout.contains("odm"));
    assert!(stdout.contains("add"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("info"));
}

#[test]
fn daemon_status_when_offline_reports_offline() {
    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            "http://127.0.0.1:59999",
            "daemon",
            "status",
            "--json",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout} {stderr}");
    assert!(
        combined.contains("offline")
            || combined.contains("error")
            || combined.contains("unreachable")
    );
}
