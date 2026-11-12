#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::Router;
use axum::http::StatusCode;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, HOST};
use axum::response::IntoResponse;
use axum::routing::get;
use odm_api::{
    AddDownloadRequest, AddDownloadResponse, DownloadDetailDto, JobOptionsDto, ProbeRequest,
    ProbeResponse, SettingsDto, UpdateSettingsRequest,
};
use odm_daemon::config::DaemonConfig;
use odm_daemon::server::build_router;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tower::ServiceExt;

async fn test_hls_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let app = Router::new().route(
        "/master.m3u8",
        get(|| async {
            let body = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=854x480,URI=\"480p.m3u8\"\n\
480p.m3u8\n\
#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720,URI=\"720p.m3u8\"\n\
720p.m3u8\n";
            (
                [(CONTENT_TYPE, "application/vnd.apple.mpegurl")],
                body.to_string(),
            )
                .into_response()
        }),
    );

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn test_add_download_hls() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "secret");
    let app = build_router(config).unwrap();

    let add_body = serde_json::to_vec(&AddDownloadRequest {
        url: "https://example.com/stream.m3u8".to_string(),
        target_dir: Some(dir.path().to_string_lossy().to_string()),
        filename: None,
        queue: None,
        category: None,
        options: None,
    })
    .unwrap();

    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/downloads")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(add_body))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let add_resp: AddDownloadResponse = serde_json::from_slice(&bytes).unwrap();

    let get_req = axum::http::Request::builder()
        .method("GET")
        .uri(format!("/api/v1/downloads/{}", add_resp.id))
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(axum::body::Body::empty())
        .unwrap();

    let get_resp = app.oneshot(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_bytes = axum::body::to_bytes(get_resp.into_body(), 4096)
        .await
        .unwrap();
    let detail: DownloadDetailDto = serde_json::from_slice(&get_bytes).unwrap();
    assert_eq!(detail.item.backend.as_deref(), Some("hls"));
    assert_eq!(detail.backend.as_deref(), Some("hls"));
}

#[tokio::test]
async fn test_add_download_dash() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "secret");
    let app = build_router(config).unwrap();

    let add_body = serde_json::to_vec(&AddDownloadRequest {
        url: "https://example.com/manifest.mpd".to_string(),
        target_dir: Some(dir.path().to_string_lossy().to_string()),
        filename: None,
        queue: None,
        category: None,
        options: None,
    })
    .unwrap();

    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/downloads")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(add_body))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let add_resp: AddDownloadResponse = serde_json::from_slice(&bytes).unwrap();

    let get_req = axum::http::Request::builder()
        .method("GET")
        .uri(format!("/api/v1/downloads/{}", add_resp.id))
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(axum::body::Body::empty())
        .unwrap();

    let get_resp = app.oneshot(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_bytes = axum::body::to_bytes(get_resp.into_body(), 4096)
        .await
        .unwrap();
    let detail: DownloadDetailDto = serde_json::from_slice(&get_bytes).unwrap();
    assert_eq!(detail.item.backend.as_deref(), Some("dash"));
    assert_eq!(detail.backend.as_deref(), Some("dash"));
}

#[tokio::test]
async fn test_add_download_with_quality() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "secret");
    let app = build_router(config).unwrap();

    let add_body = serde_json::to_vec(&AddDownloadRequest {
        url: "https://example.com/stream.m3u8".to_string(),
        target_dir: Some(dir.path().to_string_lossy().to_string()),
        filename: None,
        queue: None,
        category: None,
        options: Some(JobOptionsDto {
            quality: Some("720p".to_string()),
            ..Default::default()
        }),
    })
    .unwrap();

    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/downloads")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(add_body))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let add_resp: AddDownloadResponse = serde_json::from_slice(&bytes).unwrap();

    let get_req = axum::http::Request::builder()
        .method("GET")
        .uri(format!("/api/v1/downloads/{}", add_resp.id))
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(axum::body::Body::empty())
        .unwrap();

    let get_resp = app.oneshot(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_bytes = axum::body::to_bytes(get_resp.into_body(), 4096)
        .await
        .unwrap();
    let detail: DownloadDetailDto = serde_json::from_slice(&get_bytes).unwrap();
    assert_eq!(detail.options.quality.as_deref(), Some("720p"));
}

#[tokio::test]
async fn test_probe_m3u8_qualities() {
    let server_url = test_hls_server().await;
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "secret");
    let app = build_router(config).unwrap();

    let probe_body = serde_json::to_vec(&ProbeRequest {
        url: format!("{server_url}/master.m3u8"),
        user_agent: None,
    })
    .unwrap();

    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/probe")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(probe_body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let probe: ProbeResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(probe.backend, "hls");
    let qualities = probe.qualities.expect("qualities should be present");
    assert!(!qualities.is_empty());
    assert!(qualities.iter().any(|q| q.id == "720p" || q.id == "480p"));
}

#[tokio::test]
async fn test_settings_ffmpeg_and_ytdlp_path() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "secret");
    let app = build_router(config).unwrap();

    let update_body = serde_json::to_vec(&UpdateSettingsRequest {
        ffmpeg_path: Some("/opt/ffmpeg".to_string()),
        ytdlp_path: Some("/opt/yt-dlp".to_string()),
        ..Default::default()
    })
    .unwrap();

    let patch_req = axum::http::Request::builder()
        .method("PATCH")
        .uri("/api/v1/settings")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(update_body))
        .unwrap();

    let patch_resp = app.clone().oneshot(patch_req).await.unwrap();
    assert_eq!(patch_resp.status(), StatusCode::OK);

    let get_req = axum::http::Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(axum::body::Body::empty())
        .unwrap();

    let get_resp = app.oneshot(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(get_resp.into_body(), 4096)
        .await
        .unwrap();
    let settings: SettingsDto = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(settings.ffmpeg_path, "/opt/ffmpeg");
    assert_eq!(settings.ytdlp_path, "/opt/yt-dlp");
}
