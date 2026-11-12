//! API model serialization and contract compliance tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_api::{
    AddDownloadRequest, AddDownloadResponse, DownloadItemDto, ErrorResponse, Event, HealthResponse,
    JobOptionsDto, ListDownloadsResponse, ProbeResponse, SettingsDto, UpdateSettingsRequest,
    WsAuthRequest,
};

#[test]
fn error_response_matches_spec_shape() {
    let err = ErrorResponse::new("invalid_token", "Bearer token is invalid");
    let json = serde_json::to_string(&err).unwrap();
    assert_eq!(
        json,
        r#"{"error":{"code":"invalid_token","message":"Bearer token is invalid"}}"#
    );

    let parsed: ErrorResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.error.code, "invalid_token");
    assert_eq!(parsed.error.message, "Bearer token is invalid");
}

#[test]
fn health_response_roundtrips() {
    let health = HealthResponse {
        version: "0.1.0".to_string(),
        status: "ok".to_string(),
    };
    let json = serde_json::to_string(&health).unwrap();
    assert_eq!(json, r#"{"version":"0.1.0","status":"ok"}"#);
    let parsed: HealthResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.version, "0.1.0");
    assert_eq!(parsed.status, "ok");
}

#[test]
fn probe_response_roundtrips() {
    let probe = ProbeResponse {
        backend: "http".to_string(),
        filename: "test.zip".to_string(),
        size: Some(1024),
        ranges: true,
        etag: Some("abc".to_string()),
        last_modified: None,
        torrent_files: None,
    };
    let json = serde_json::to_string(&probe).unwrap();
    let parsed: ProbeResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.backend, "http");
    assert_eq!(parsed.filename, "test.zip");
    assert!(parsed.ranges);
}

#[test]
fn add_download_request_and_response() {
    let req = AddDownloadRequest {
        url: "https://example.com/file.zip".to_string(),
        target_dir: Some("/downloads".to_string()),
        filename: Some("custom.zip".to_string()),
        queue: Some("bulk".to_string()),
        category: Some("Compressed".to_string()),
        options: Some(JobOptionsDto {
            segments: Some(4),
            speed_limit: Some(1024),
            ..Default::default()
        }),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: AddDownloadRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.url, "https://example.com/file.zip");
    assert_eq!(parsed.options.unwrap().segments, Some(4));

    let resp = AddDownloadResponse { id: 42 };
    let json_resp = serde_json::to_string(&resp).unwrap();
    assert_eq!(json_resp, r#"{"id":42}"#);
}

#[test]
fn download_item_dto_and_list() {
    let item = DownloadItemDto {
        id: 1,
        url: "https://example.com/file.bin".to_string(),
        filename: "file.bin".to_string(),
        state: "downloading".to_string(),
        total_bytes: Some(1024),
        downloaded_bytes: 512,
        progress_pct: 50.0,
        speed_bps: 10240,
        queue: "default".to_string(),
        target_dir: "/tmp".to_string(),
        category: None,
        error: None,
        backend: Some("http".to_string()),
    };
    let list = ListDownloadsResponse {
        downloads: vec![item],
    };
    let json = serde_json::to_string(&list).unwrap();
    let parsed: ListDownloadsResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.downloads.len(), 1);
    assert_eq!(parsed.downloads[0].id, 1);
    assert_eq!(parsed.downloads[0].state, "downloading");
}

#[test]
fn settings_dto_and_update_roundtrips() {
    let settings = SettingsDto {
        download_dir: Some("/home/user/Downloads".to_string()),
        default_segments: 8,
        global_speed_limit: 0,
        default_retry_count: 5,
        seed_ratio: 1.0,
        torrent_listen_port: 6881,
    };
    let json = serde_json::to_string(&settings).unwrap();
    let parsed: SettingsDto = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.default_segments, 8);
    assert_eq!(parsed.seed_ratio, 1.0);
    assert_eq!(parsed.torrent_listen_port, 6881);

    let update = UpdateSettingsRequest {
        download_dir: None,
        default_segments: Some(16),
        global_speed_limit: None,
        default_retry_count: None,
        seed_ratio: Some(2.0),
        torrent_listen_port: Some(6882),
    };
    let json_up = serde_json::to_string(&update).unwrap();
    let parsed_up: UpdateSettingsRequest = serde_json::from_str(&json_up).unwrap();
    assert_eq!(parsed_up.default_segments, Some(16));
}

#[test]
fn event_variants_roundtrip_tagged_shape() {
    let event = Event::JobProgress {
        id: 10,
        downloaded_bytes: 2048,
        total_bytes: Some(4096),
        speed_bps: 512,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"job_progress""#));
    let parsed: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);

    let auth = WsAuthRequest {
        token: "secret123".to_string(),
    };
    let auth_json = serde_json::to_string(&auth).unwrap();
    assert_eq!(auth_json, r#"{"token":"secret123"}"#);
}

#[test]
fn checksum_dto_parse_and_roundtrip() {
    use odm_api::{ChecksumAlgorithm, ChecksumDto};
    use std::str::FromStr;

    let cs = ChecksumDto::from_str(
        "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
    )
    .unwrap();
    assert_eq!(cs.algorithm, ChecksumAlgorithm::Sha256);
    assert_eq!(
        cs.to_string(),
        "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );

    let cs_raw = ChecksumDto::from_str("5eb63bbbe01eeed093cb22bb8f5acdc3").unwrap();
    assert_eq!(cs_raw.algorithm, ChecksumAlgorithm::Md5);

    let json = serde_json::to_string(&cs).unwrap();
    let parsed: ChecksumDto = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, cs);
}
