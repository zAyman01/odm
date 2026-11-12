#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_api::{JobOptionsDto, ProbeResponse, QualityDto, SettingsDto, UpdateSettingsRequest};

#[test]
fn quality_dto_roundtrip() {
    let q = QualityDto {
        id: "1080p".to_string(),
        resolution: Some("1920x1080".to_string()),
        bandwidth: Some(5_000_000),
        codecs: Some("avc1.640028,mp4a.40.2".to_string()),
        label: "1080p (5.0 Mbps)".to_string(),
    };
    let json = serde_json::to_string(&q).unwrap();
    let parsed: QualityDto = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, q);

    // Test optional fields omitted when None
    let q_min = QualityDto {
        id: "audio".to_string(),
        resolution: None,
        bandwidth: None,
        codecs: None,
        label: "Audio only".to_string(),
    };
    let json_min = serde_json::to_string(&q_min).unwrap();
    assert!(!json_min.contains("resolution"));
    assert!(!json_min.contains("bandwidth"));
    assert!(!json_min.contains("codecs"));
    let parsed_min: QualityDto = serde_json::from_str(&json_min).unwrap();
    assert_eq!(parsed_min, q_min);
}

#[test]
fn probe_response_with_qualities_roundtrip() {
    let resp = ProbeResponse {
        backend: "hls".to_string(),
        filename: "video.mp4".to_string(),
        size: Some(1024),
        ranges: false,
        etag: None,
        last_modified: None,
        torrent_files: None,
        qualities: Some(vec![QualityDto {
            id: "720p".to_string(),
            resolution: Some("1280x720".to_string()),
            bandwidth: Some(2_500_000),
            codecs: None,
            label: "720p".to_string(),
        }]),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: ProbeResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.qualities.as_ref().unwrap().len(), 1);
    assert_eq!(parsed.qualities.as_ref().unwrap()[0].id, "720p");

    // Deserialization with omitted qualities field defaults to None
    let json_no_qualities = r#"{"backend":"http","filename":"test.bin","size":100,"ranges":true}"#;
    let parsed_no_q: ProbeResponse = serde_json::from_str(json_no_qualities).unwrap();
    assert_eq!(parsed_no_q.qualities, None);
}

#[test]
fn job_options_with_quality_roundtrip() {
    let opts = JobOptionsDto {
        quality: Some("1080p".to_string()),
        ..Default::default()
    };
    let json = serde_json::to_string(&opts).unwrap();
    assert!(json.contains(r#""quality":"1080p""#));
    let parsed: JobOptionsDto = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.quality, Some("1080p".to_string()));

    // When None, quality should be skipped in serialization and default to None in deserialization
    let opts_default = JobOptionsDto::default();
    let json_default = serde_json::to_string(&opts_default).unwrap();
    assert!(!json_default.contains("quality"));
    let parsed_default: JobOptionsDto = serde_json::from_str(&json_default).unwrap();
    assert_eq!(parsed_default.quality, None);
}

#[test]
fn settings_dto_with_media_paths_roundtrip() {
    let settings = SettingsDto {
        download_dir: Some("/downloads".to_string()),
        default_segments: 4,
        global_speed_limit: 0,
        default_retry_count: 3,
        seed_ratio: 1.0,
        torrent_listen_port: 6881,
        ffmpeg_path: "C:\\bin\\ffmpeg.exe".to_string(),
        ytdlp_path: "C:\\bin\\yt-dlp.exe".to_string(),
    };
    let json = serde_json::to_string(&settings).unwrap();
    let parsed: SettingsDto = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.ffmpeg_path, "C:\\bin\\ffmpeg.exe");
    assert_eq!(parsed.ytdlp_path, "C:\\bin\\yt-dlp.exe");

    // Deserialization with missing ffmpeg_path/ytdlp_path defaults to empty string
    let json_legacy = r#"{
        "default_segments": 4,
        "global_speed_limit": 0,
        "default_retry_count": 3
    }"#;
    let parsed_legacy: SettingsDto = serde_json::from_str(json_legacy).unwrap();
    assert_eq!(parsed_legacy.ffmpeg_path, "");
    assert_eq!(parsed_legacy.ytdlp_path, "");
}

#[test]
fn update_settings_request_with_media_paths_roundtrip() {
    let update = UpdateSettingsRequest {
        ffmpeg_path: Some("ffmpeg".to_string()),
        ytdlp_path: Some("yt-dlp".to_string()),
        ..Default::default()
    };
    let json = serde_json::to_string(&update).unwrap();
    assert!(json.contains(r#""ffmpeg_path":"ffmpeg""#));
    assert!(json.contains(r#""ytdlp_path":"yt-dlp""#));
    let parsed: UpdateSettingsRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.ffmpeg_path, Some("ffmpeg".to_string()));
    assert_eq!(parsed.ytdlp_path, Some("yt-dlp".to_string()));

    let empty_update = UpdateSettingsRequest::default();
    let json_empty = serde_json::to_string(&empty_update).unwrap();
    assert!(!json_empty.contains("ffmpeg_path"));
    assert!(!json_empty.contains("ytdlp_path"));
    let parsed_empty: UpdateSettingsRequest = serde_json::from_str(&json_empty).unwrap();
    assert_eq!(parsed_empty.ffmpeg_path, None);
    assert_eq!(parsed_empty.ytdlp_path, None);
}
