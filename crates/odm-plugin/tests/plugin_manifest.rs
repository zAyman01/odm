#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_plugin::{PluginError, PluginManifest, PluginRole};

#[test]
fn test_parse_valid_plugin_manifest() {
    let toml = r#"
[plugin]
name = "test-resolver"
version = "0.1.0"
description = "Test resolver description"
role = "resolver"
url_patterns = ["*://*.example.com/watch/*", "^https?://mysite\\.com/item/\\d+$"]
command = ["python", "resolver.py"]
"#;

    let manifest = PluginManifest::from_toml_str(toml).unwrap();
    assert_eq!(manifest.name, "test-resolver");
    assert_eq!(manifest.version, "0.1.0");
    assert_eq!(
        manifest.description.as_deref(),
        Some("Test resolver description")
    );
    assert_eq!(manifest.role, PluginRole::Resolver);
    assert_eq!(
        manifest.url_patterns,
        vec![
            "*://*.example.com/watch/*".to_string(),
            "^https?://mysite\\.com/item/\\d+$".to_string()
        ]
    );
    assert_eq!(manifest.command, vec!["python", "resolver.py"]);
}

#[test]
fn test_command_string_and_array_formats() {
    let toml_single = r#"
[plugin]
name = "single-cmd"
version = "1.0.0"
role = "resolver"
url_patterns = ["*://example.com/*"]
command = "resolver.py"
"#;
    let manifest_single = PluginManifest::from_toml_str(toml_single).unwrap();
    assert_eq!(manifest_single.command, vec!["resolver.py"]);
    assert_eq!(manifest_single.description, None);

    let toml_array = r#"
[plugin]
name = "array-cmd"
version = "1.0.0"
role = "resolver"
url_patterns = ["*://example.com/*"]
command = ["node", "resolver.js", "--verbose"]
"#;
    let manifest_array = PluginManifest::from_toml_str(toml_array).unwrap();
    assert_eq!(
        manifest_array.command,
        vec!["node", "resolver.js", "--verbose"]
    );
}

#[test]
fn test_wildcard_glob_pattern_matching() {
    let toml = r#"
[plugin]
name = "wildcard-matcher"
version = "0.1.0"
role = "resolver"
url_patterns = [
    "*://*.example.com/watch/*",
    "*://example.com/watch/*",
    "*.example.org/*"
]
command = "resolver.exe"
"#;

    let manifest = PluginManifest::from_toml_str(toml).unwrap();

    // Matching *://*.example.com/watch/*
    assert!(manifest.matches_url("https://sub.example.com/watch/12345"));
    assert!(manifest.matches_url("http://video.example.com/watch/abc"));
    assert!(manifest.matches_url("https://a.b.example.com/watch/test?q=1"));

    // Rejecting non-matching for *://*.example.com/watch/*
    assert!(!manifest.matches_url("https://sub.example.com/other/12345"));
    assert!(!manifest.matches_url("https://notexample.com/watch/12345"));

    // Matching *://example.com/watch/*
    assert!(manifest.matches_url("https://example.com/watch/12345"));
    assert!(manifest.matches_url("http://example.com/watch/video"));

    // Matching *.example.org/*
    assert!(manifest.matches_url("https://cdn.example.org/files/archive.zip"));
    assert!(manifest.matches_url("http://sub.example.org/item"));

    // Rejecting non-matching
    assert!(!manifest.matches_url("https://example.net/watch/123"));
}

#[test]
fn test_regex_pattern_matching() {
    let toml = r#"
[plugin]
name = "regex-matcher"
version = "0.1.0"
role = "resolver"
url_patterns = [
    "^https?://mysite\\.com/item/\\d+$",
    "^https?://(www\\.)?othersite\\.com/video/[a-z0-9]+$"
]
command = ["python", "resolver.py"]
"#;

    let manifest = PluginManifest::from_toml_str(toml).unwrap();

    // Matching first pattern
    assert!(manifest.matches_url("https://mysite.com/item/12345"));
    assert!(manifest.matches_url("http://mysite.com/item/9"));

    // Rejecting first pattern
    assert!(!manifest.matches_url("https://mysite.com/item/"));
    assert!(!manifest.matches_url("https://mysite.com/item/abc"));
    assert!(!manifest.matches_url("https://mysite.com/item/123/details"));
    assert!(!manifest.matches_url("ftp://mysite.com/item/12345"));

    // Matching second pattern
    assert!(manifest.matches_url("https://othersite.com/video/abc123xyz"));
    assert!(manifest.matches_url("https://www.othersite.com/video/item1"));

    // Rejecting second pattern
    assert!(!manifest.matches_url("https://sub.othersite.com/video/item1"));
    assert!(!manifest.matches_url("https://othersite.com/video/ITEM1"));
}

#[test]
fn test_invalid_role_returns_error() {
    let toml_downloader = r#"
[plugin]
name = "invalid-role"
version = "0.1.0"
role = "downloader"
url_patterns = ["*://example.com/*"]
command = "run.sh"
"#;
    let err = PluginManifest::from_toml_str(toml_downloader).unwrap_err();
    assert!(
        matches!(&err, PluginError::UnsupportedRole(role) if role == "downloader"),
        "expected UnsupportedRole('downloader'), got {err:?}"
    );

    let toml_uploader = r#"
[plugin]
name = "uploader-role"
version = "0.1.0"
role = "uploader"
url_patterns = ["*://example.com/*"]
command = "run.sh"
"#;
    let err = PluginManifest::from_toml_str(toml_uploader).unwrap_err();
    assert!(
        matches!(&err, PluginError::UnsupportedRole(role) if role == "uploader"),
        "expected UnsupportedRole('uploader'), got {err:?}"
    );
}

#[test]
fn test_missing_required_fields_returns_error() {
    // Missing name
    let no_name = r#"
[plugin]
version = "0.1.0"
role = "resolver"
url_patterns = ["*://example.com/*"]
command = "run.sh"
"#;
    assert!(matches!(
        PluginManifest::from_toml_str(no_name).unwrap_err(),
        PluginError::InvalidManifest(_)
    ));

    // Missing command
    let no_command = r#"
[plugin]
name = "test"
version = "0.1.0"
role = "resolver"
url_patterns = ["*://example.com/*"]
"#;
    assert!(matches!(
        PluginManifest::from_toml_str(no_command).unwrap_err(),
        PluginError::InvalidManifest(_)
    ));

    // Missing version
    let no_version = r#"
[plugin]
name = "test"
role = "resolver"
url_patterns = ["*://example.com/*"]
command = "run.sh"
"#;
    assert!(matches!(
        PluginManifest::from_toml_str(no_version).unwrap_err(),
        PluginError::InvalidManifest(_)
    ));

    // Missing role
    let no_role = r#"
[plugin]
name = "test"
version = "0.1.0"
url_patterns = ["*://example.com/*"]
command = "run.sh"
"#;
    assert!(matches!(
        PluginManifest::from_toml_str(no_role).unwrap_err(),
        PluginError::InvalidManifest(_)
    ));

    // Missing [plugin] table entirely
    let no_plugin_table = r#"
name = "test"
version = "0.1.0"
role = "resolver"
url_patterns = ["*://example.com/*"]
command = "run.sh"
"#;
    assert!(matches!(
        PluginManifest::from_toml_str(no_plugin_table).unwrap_err(),
        PluginError::InvalidManifest(_)
    ));

    // Empty command array
    let empty_command_arr = r#"
[plugin]
name = "test"
version = "0.1.0"
role = "resolver"
url_patterns = ["*://example.com/*"]
command = []
"#;
    assert!(matches!(
        PluginManifest::from_toml_str(empty_command_arr).unwrap_err(),
        PluginError::InvalidManifest(_)
    ));

    // Empty command string
    let empty_command_str = r#"
[plugin]
name = "test"
version = "0.1.0"
role = "resolver"
url_patterns = ["*://example.com/*"]
command = ""
"#;
    assert!(matches!(
        PluginManifest::from_toml_str(empty_command_str).unwrap_err(),
        PluginError::InvalidManifest(_)
    ));

    // Invalid regex pattern
    let invalid_regex = r#"
[plugin]
name = "test"
version = "0.1.0"
role = "resolver"
url_patterns = ["^[unclosed-bracket"]
command = "run.sh"
"#;
    assert!(matches!(
        PluginManifest::from_toml_str(invalid_regex).unwrap_err(),
        PluginError::InvalidManifest(_)
    ));
}
