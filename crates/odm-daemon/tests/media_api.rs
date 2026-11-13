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
        backend: None,
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
        backend: None,
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
        backend: None,
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

fn state_for(dir: &std::path::Path, ytdlp: bool) -> odm_daemon::state::AppState {
    let store = odm_core::store::Store::open_in_memory().unwrap();
    let engine = odm_core::Engine::new(store).unwrap();
    let state =
        odm_daemon::state::AppState::new(engine.clone(), DaemonConfig::for_test(dir, "secret"));
    if ytdlp {
        let backend = std::sync::Arc::new(odm_media::ytdlp::YtDlpBackend::new(
            dir.join("no-such-yt-dlp"),
        ));
        engine.register_backend(backend.clone());
        state.with_ytdlp(backend)
    } else {
        state
    }
}

async fn add_with_backend(
    app: &Router,
    url: &str,
    backend: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let body = serde_json::to_vec(&AddDownloadRequest {
        url: url.to_string(),
        target_dir: None,
        filename: None,
        queue: None,
        category: None,
        options: None,
        backend: backend.map(str::to_string),
    })
    .unwrap();
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/downloads")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

async fn backend_of(app: &Router, id: i64) -> Option<String> {
    let req = axum::http::Request::builder()
        .method("GET")
        .uri(format!("/api/v1/downloads/{id}"))
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
    let detail: DownloadDetailDto = serde_json::from_slice(&bytes).unwrap();
    detail.backend
}

#[tokio::test]
async fn add_with_backend_ytdlp_creates_ytdlp_job() {
    let dir = tempdir().unwrap();
    let app = odm_daemon::server::build_router_with_state(state_for(dir.path(), true));

    let (status, body) =
        add_with_backend(&app, "https://example.com/watch?v=1", Some("ytdlp")).await;

    assert_eq!(status, StatusCode::OK);
    let id = body["id"].as_i64().unwrap();
    assert_eq!(backend_of(&app, id).await.as_deref(), Some("yt-dlp"));
}

#[tokio::test]
async fn ytdlp_without_binary_is_400() {
    let dir = tempdir().unwrap();
    let app = odm_daemon::server::build_router_with_state(state_for(dir.path(), false));

    let (status, body) =
        add_with_backend(&app, "https://example.com/watch?v=1", Some("ytdlp")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "backend_unavailable");
}

#[tokio::test]
async fn unknown_backend_name_is_400() {
    let dir = tempdir().unwrap();
    let app = odm_daemon::server::build_router_with_state(state_for(dir.path(), true));

    let (status, _) = add_with_backend(&app, "https://example.com/a.zip", Some("bogus")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn https_torrent_url_uses_http_backend() {
    let dir = tempdir().unwrap();
    let app = odm_daemon::server::build_router_with_state(state_for(dir.path(), false));

    let (status, body) = add_with_backend(&app, "https://example.com/linux.torrent", None).await;

    assert_eq!(status, StatusCode::OK);
    let id = body["id"].as_i64().unwrap();
    assert_eq!(backend_of(&app, id).await.as_deref(), Some("http"));
}
