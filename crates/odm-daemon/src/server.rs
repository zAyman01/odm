//! HTTP and WebSocket server construction and routing.
use axum::Router;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::{get, post};
use odm_core::{Engine, Store};

use crate::auth::{create_cors_layer, require_bearer_token, validate_host_header};
use crate::config::DaemonConfig;
use crate::routes::*;
use crate::state::AppState;
use crate::ws::ws_handler;

/// Builds the Axum router for the daemon given the application state.
pub fn build_router_with_state(state: AppState) -> Router {
    let token = state.config.token.clone();

    let api_routes = Router::new()
        .route("/health", get(health_handler))
        .route("/probe", post(probe_handler))
        .route(
            "/downloads",
            get(list_downloads_handler).post(add_download_handler),
        )
        .route(
            "/downloads/{id}",
            get(get_download_handler).delete(delete_download_handler),
        )
        .route("/downloads/{id}/pause", post(pause_download_handler))
        .route("/downloads/{id}/resume", post(resume_download_handler))
        .route("/downloads/{id}/cancel", post(cancel_download_handler))
        .route("/downloads/{id}/retry", post(retry_download_handler))
        .route(
            "/queues",
            get(list_queues_handler).post(create_queue_handler),
        )
        .route(
            "/queues/{name}",
            axum::routing::patch(update_queue_handler).delete(delete_queue_handler),
        )
        .route(
            "/settings",
            get(get_settings_handler).patch(update_settings_handler),
        )
        .route("/events", get(ws_handler))
        .with_state(state);

    Router::new()
        .nest("/api/v1", api_routes)
        .route("/health", get(health_handler))
        .layer(from_fn_with_state(token, require_bearer_token))
        .layer(from_fn(validate_host_header))
        .layer(create_cors_layer())
}

/// Builds the Axum router for testing from configuration.
pub fn build_router(config: DaemonConfig) -> Result<Router, Box<dyn std::error::Error>> {
    let store = Store::open_in_memory()?;
    let engine = Engine::new(store)?;
    let state = AppState::new(engine, config);
    Ok(build_router_with_state(state))
}
