//! WebSocket events streaming and authentication test.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use futures_util::{SinkExt, StreamExt};
use odm_api::WsAuthRequest;
use odm_core::{Engine, Store};
use odm_daemon::config::DaemonConfig;
use odm_daemon::server::build_router_with_state;
use odm_daemon::state::AppState;
use tempfile::tempdir;
use tokio::net::TcpListener;

#[tokio::test]
async fn websocket_authenticates_and_receives_events() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join("ws.sqlite")).unwrap();
    let engine = Engine::new(store).unwrap();
    let config = DaemonConfig::for_test(dir.path(), "ws-secret");
    let state = AppState::new(engine, config);
    let app = build_router_with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let (mut ws_stream, _) =
        tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/api/v1/events", addr.port()))
            .await
            .unwrap();

    // First message: auth
    let auth = serde_json::to_string(&WsAuthRequest {
        token: "ws-secret".to_string(),
    })
    .unwrap();
    ws_stream
        .send(tokio_tungstenite::tungstenite::Message::Text(auth.into()))
        .await
        .unwrap();

    let resp = ws_stream.next().await.unwrap().unwrap();
    let text = resp.to_text().unwrap();
    assert!(text.contains("authenticated"));
}

async fn ws_connect_with_origin(origin: Option<&str>) -> bool {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join("ws.sqlite")).unwrap();
    let engine = Engine::new(store).unwrap();
    let state = AppState::new(engine, DaemonConfig::for_test(dir.path(), "ws-secret"));
    let app = build_router_with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let mut request = format!("ws://127.0.0.1:{}/api/v1/events", addr.port())
        .into_client_request()
        .unwrap();
    if let Some(origin) = origin {
        request
            .headers_mut()
            .insert("Origin", origin.parse().unwrap());
    }
    tokio_tungstenite::connect_async(request).await.is_ok()
}

#[tokio::test]
async fn ws_rejects_foreign_origin() {
    assert!(!ws_connect_with_origin(Some("http://evil.example")).await);
}

#[tokio::test]
async fn ws_accepts_no_origin_and_loopback_origin() {
    assert!(ws_connect_with_origin(None).await);
    assert!(ws_connect_with_origin(Some("http://127.0.0.1:5555")).await);
}

#[tokio::test]
async fn ws_survives_lagged_receiver() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join("ws.sqlite")).unwrap();
    let engine = Engine::new(store).unwrap();
    let events = engine.events();
    let state = AppState::new(engine, DaemonConfig::for_test(dir.path(), "ws-secret"));
    let app = build_router_with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let (mut ws_stream, _) =
        tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/api/v1/events", addr.port()))
            .await
            .unwrap();
    let auth = serde_json::to_string(&WsAuthRequest {
        token: "ws-secret".to_string(),
    })
    .unwrap();
    ws_stream
        .send(tokio_tungstenite::tungstenite::Message::Text(auth.into()))
        .await
        .unwrap();
    let reply = ws_stream.next().await.unwrap().unwrap();
    assert!(reply.to_text().unwrap().contains("true"));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // The server task gets no chance to run during the burst, so its receiver
    // falls behind the 1024-event channel.
    for bytes in 0..5000 {
        let _ = events.send(odm_core::Event::JobProgress {
            id: odm_core::JobId(1),
            downloaded_bytes: bytes,
            total_bytes: None,
            speed_bps: 0,
        });
    }
    let _ = events.send(odm_core::Event::JobRemoved(odm_core::JobId(4242)));

    let found = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(Ok(message)) = ws_stream.next().await {
            if message.to_text().is_ok_and(|text| text.contains("4242")) {
                return true;
            }
        }
        false
    })
    .await
    .expect("no event arrived in time");
    assert!(found, "the socket closed instead of skipping missed events");
}
