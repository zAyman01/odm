//! HTTP and WebSocket server construction.
use axum::middleware::from_fn;
use axum::middleware::from_fn_with_state;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use odm_api::{HealthResponse, ListDownloadsResponse};

use crate::auth::{create_cors_layer, require_bearer_token, validate_host_header};
use crate::config::DaemonConfig;

async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        status: "ok".to_string(),
    })
}

async fn downloads_handler() -> impl IntoResponse {
    Json(ListDownloadsResponse { downloads: vec![] })
}

/// Builds the Axum router for the daemon with security middleware.
pub fn build_router(config: DaemonConfig) -> Router {
    let token = config.token.clone();

    let api_routes = Router::new()
        .route("/health", get(health_handler))
        .route("/downloads", get(downloads_handler));

    Router::new()
        .nest("/api/v1", api_routes)
        .route("/health", get(health_handler))
        .layer(from_fn_with_state(token, require_bearer_token))
        .layer(from_fn(validate_host_header))
        .layer(create_cors_layer())
}
