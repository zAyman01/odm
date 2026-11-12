#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_api::{GrabbedFileDto, GrabberRequest, GrabberResponse, PluginDto, PluginListResponse};

#[test]
fn grabber_request_roundtrip_full() {
    let req = GrabberRequest {
        url: "https://example.com/gallery".to_string(),
        depth: Some(2),
        extensions: Some(vec!["jpg".to_string(), "png".to_string()]),
        pattern: Some(r".*\.jpg".to_string()),
        respect_robots: Some(true),
        max_pages: Some(50),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: GrabberRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn grabber_request_roundtrip_minimal_and_defaults() {
    let req_min = GrabberRequest {
        url: "https://example.com".to_string(),
        depth: None,
        extensions: None,
        pattern: None,
        respect_robots: None,
        max_pages: None,
    };
    let json_min = serde_json::to_string(&req_min).unwrap();
    assert!(!json_min.contains("depth"));
    assert!(!json_min.contains("extensions"));
    assert!(!json_min.contains("pattern"));
    assert!(!json_min.contains("respect_robots"));
    assert!(!json_min.contains("max_pages"));

    let parsed_min: GrabberRequest = serde_json::from_str(&json_min).unwrap();
    assert_eq!(parsed_min, req_min);

    // Deserialization with only url field defaults missing optional fields to None
    let raw = r#"{"url":"https://example.com"}"#;
    let parsed_raw: GrabberRequest = serde_json::from_str(raw).unwrap();
    assert_eq!(parsed_raw, req_min);
}

#[test]
fn grabbed_file_dto_roundtrip() {
    let file = GrabbedFileDto {
        url: "https://example.com/img1.jpg".to_string(),
        filename: "img1.jpg".to_string(),
        found_on: "https://example.com/gallery".to_string(),
        depth: 1,
    };
    let json = serde_json::to_string(&file).unwrap();
    let parsed: GrabbedFileDto = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, file);
}

#[test]
fn grabber_response_roundtrip() {
    let resp = GrabberResponse {
        files: vec![
            GrabbedFileDto {
                url: "https://example.com/img1.jpg".to_string(),
                filename: "img1.jpg".to_string(),
                found_on: "https://example.com/gallery".to_string(),
                depth: 1,
            },
            GrabbedFileDto {
                url: "https://example.com/img2.png".to_string(),
                filename: "img2.png".to_string(),
                found_on: "https://example.com/gallery".to_string(),
                depth: 2,
            },
        ],
        pages_crawled: 5,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: GrabberResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

#[test]
fn plugin_dto_and_list_response_roundtrip() {
    let plugin = PluginDto {
        name: "yt-dlp".to_string(),
        version: "1.0.0".to_string(),
        description: Some("YouTube downloader plugin".to_string()),
        role: "resolver".to_string(),
        url_patterns: vec!["^https?://.*youtube\\.com/.*$".to_string()],
        command: vec!["yt-dlp".to_string(), "--dump-json".to_string()],
    };
    let json = serde_json::to_string(&plugin).unwrap();
    let parsed: PluginDto = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, plugin);

    // Description None omitted when serialized
    let plugin_no_desc = PluginDto {
        name: "simple-plugin".to_string(),
        version: "0.1.0".to_string(),
        description: None,
        role: "resolver".to_string(),
        url_patterns: vec![],
        command: vec!["plugin.sh".to_string()],
    };
    let json_no_desc = serde_json::to_string(&plugin_no_desc).unwrap();
    assert!(!json_no_desc.contains("description"));
    let parsed_no_desc: PluginDto = serde_json::from_str(&json_no_desc).unwrap();
    assert_eq!(parsed_no_desc, plugin_no_desc);

    // PluginListResponse roundtrip
    let resp = PluginListResponse {
        plugins: vec![plugin, plugin_no_desc],
    };
    let json_resp = serde_json::to_string(&resp).unwrap();
    let parsed_resp: PluginListResponse = serde_json::from_str(&json_resp).unwrap();
    assert_eq!(parsed_resp, resp);
}
