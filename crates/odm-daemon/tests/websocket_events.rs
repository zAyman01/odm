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
