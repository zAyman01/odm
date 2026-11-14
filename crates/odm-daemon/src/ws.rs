//! WebSocket endpoint for streaming daemon events.
use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use odm_api::{Event, WsAuthRequest, WsAuthResponse};
use odm_core::Event as CoreEvent;
use tokio::sync::broadcast;

use crate::routes::job_to_dto;
use crate::state::AppState;

pub async fn ws_handler(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    // Browsers always send Origin on WebSocket handshakes; CLI clients send none.
    if let Some(origin) = headers.get(header::ORIGIN) {
        let port = crate::auth::configured_port(&state.config.bind_addr);
        if !origin
            .to_str()
            .is_ok_and(|origin| crate::auth::origin_allowed(origin, port))
        {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state))
        .into_response()
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    // 1. Wait for first message: authentication
    let auth_msg = tokio::time::timeout(Duration::from_secs(5), socket.next()).await;
    let authenticated = match auth_msg {
        Ok(Some(Ok(Message::Text(text)))) => {
            if let Ok(req) = serde_json::from_str::<WsAuthRequest>(&text) {
                crate::auth::token_matches(&state.config.token, &req.token)
            } else {
                false
            }
        }
        _ => false,
    };

    if !authenticated {
        let _ = socket
            .send(Message::Text(
                serde_json::to_string(&WsAuthResponse {
                    authenticated: false,
                })
                .unwrap_or_default()
                .into(),
            ))
            .await;
        let _ = socket.close().await;
        return;
    }

    // Send success
    let _ = socket
        .send(Message::Text(
            serde_json::to_string(&WsAuthResponse {
                authenticated: true,
            })
            .unwrap_or_default()
            .into(),
        ))
        .await;

    // 2. Stream engine events
    let mut rx = state.engine.subscribe();
    loop {
        let core_event = match rx.recv().await {
            Ok(event) => event,
            // Slow clients miss events instead of losing the connection.
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        };
        let api_event = match core_event {
            CoreEvent::JobAdded(id) => {
                if let Ok(job) = state.engine.store().get_job(id) {
                    Event::JobAdded(job_to_dto(&job, state.engine.store()))
                } else {
                    continue;
                }
            }
            CoreEvent::JobStateChanged { id, state: st } => Event::JobStateChanged {
                id: id.0,
                state: format!("{st:?}").to_lowercase(),
            },
            CoreEvent::JobProgress {
                id,
                downloaded_bytes,
                total_bytes,
                speed_bps,
            } => Event::JobProgress {
                id: id.0,
                downloaded_bytes,
                total_bytes,
                speed_bps,
            },
            CoreEvent::JobRemoved(id) => Event::JobRemoved(id.0),
            CoreEvent::QueueChanged(name) => {
                if let Ok(q) = state.engine.store().queue_by_name(&name) {
                    Event::QueueChanged(odm_api::QueueDto {
                        id: q.id,
                        name: q.name,
                        max_concurrent: q.max_concurrent,
                        schedule_start: q.schedule.as_ref().map(|s| s.start_time.clone()),
                        schedule_stop: q.schedule.as_ref().and_then(|s| s.stop_time.clone()),
                        schedule_days: q.schedule.map(|s| s.days),
                    })
                } else {
                    continue;
                }
            }
        };

        if let Ok(json) = serde_json::to_string(&api_event)
            && socket.send(Message::Text(json.into())).await.is_err()
        {
            break;
        }
    }
}
