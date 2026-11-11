//! Desktop clipboard extraction and notification triggering tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_gui::desktop::{extract_valid_download_url, should_notify_state_change};

#[test]
fn extracts_http_and_https_urls_only() {
    assert_eq!(
        extract_valid_download_url("https://releases.ubuntu.com/24.04/ubuntu-desktop.iso"),
        Some("https://releases.ubuntu.com/24.04/ubuntu-desktop.iso".to_string())
    );
    assert_eq!(
        extract_valid_download_url("http://speedtest.tele2.net/10MB.zip"),
        Some("http://speedtest.tele2.net/10MB.zip".to_string())
    );
    assert_eq!(
        extract_valid_download_url("   https://example.com/file.tar.gz  \n"),
        Some("https://example.com/file.tar.gz".to_string())
    );

    // Rejects non-http(s) schemes or invalid text
    assert_eq!(
        extract_valid_download_url("ftp://ftp.example.com/file.iso"),
        None
    );
    assert_eq!(
        extract_valid_download_url("file:///C:/Users/file.txt"),
        None
    );
    assert_eq!(extract_valid_download_url("not a url at all"), None);
    assert_eq!(extract_valid_download_url("http://"), None);
}

#[test]
fn notifies_on_completion_or_failure_only() {
    assert!(should_notify_state_change("completed"));
    assert!(should_notify_state_change("failed"));
    assert!(!should_notify_state_change("downloading"));
    assert!(!should_notify_state_change("paused"));
    assert!(!should_notify_state_change("pending"));
}
