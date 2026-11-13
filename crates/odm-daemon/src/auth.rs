//! Authentication, host validation, and CORS security layers.
use axum::Json;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use odm_api::ErrorResponse;
use tower_http::cors::{Any, CorsLayer};

/// Middleware verifying that the Host header matches loopback.
pub async fn validate_host_header(
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let host_header = req
        .headers()
        .get(header::HOST)
        .and_then(|val| val.to_str().ok());

    let is_valid = if let Some(host) = host_header {
        let domain = host
            .split(':')
            .next()
            .unwrap_or(host)
            .trim_matches(['[', ']']);
        domain == "127.0.0.1" || domain == "localhost" || domain == "::1"
    } else {
        false
    };

    if !is_valid {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(
                "invalid_host",
                "Host header is not allowed",
            )),
        ));
    }

    Ok(next.run(req).await)
}

/// Middleware verifying the Authorization header carries `Bearer <token>` on protected routes.
pub async fn require_bearer_token(
    State(expected_token): State<String>,
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let path = req.uri().path();
    if path == "/api/v1/health"
        || path == "/health"
        || path == "/api/v1/events"
        || path == "/events"
        || !path.starts_with("/api/")
    {
        return Ok(next.run(req).await);
    }

    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|val| val.to_str().ok());

    let is_authenticated = match auth_header {
        Some(val) if val.starts_with("Bearer ") => {
            let token = val.trim_start_matches("Bearer ").trim();
            token == expected_token
        }
        _ => false,
    };

    if !is_authenticated {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse::new(
                "unauthorized",
                "Missing or invalid authorization token",
            )),
        ));
    }

    Ok(next.run(req).await)
}

/// Creates the CORS layer permitting loopback and browser extension origins.
pub fn create_cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::predicate(|origin, _| {
            let s = origin.to_str().unwrap_or("");
            s.starts_with("chrome-extension://")
                || s.starts_with("moz-extension://")
                || s.starts_with("http://127.0.0.1:")
                || s.starts_with("http://localhost:")
        }))
        .allow_methods(Any)
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}
