//! Authentication, host validation, and CORS security layers.
use axum::Json;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use odm_api::ErrorResponse;
use std::sync::{Arc, RwLock};
use tower_http::cors::{Any, CorsLayer};

/// Extra host names the daemon accepts besides loopback; shared so a settings change applies at once.
pub type AllowedHosts = Arc<RwLock<Vec<String>>>;

/// Turns the comma-separated `allowed_hosts` setting into lower-case host names.
pub fn parse_allowed_hosts(setting: &str) -> Vec<String> {
    setting
        .split(',')
        .map(|host| host.trim().to_ascii_lowercase())
        .filter(|host| !host.is_empty())
        .collect()
}

/// Returns the host name of a Host header value, without port or IPv6 brackets.
fn host_name(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    host.split(':').next().unwrap_or(host)
}

/// Middleware verifying that the Host header is loopback or an allowed host name.
pub async fn validate_host_header(
    State(allowed): State<AllowedHosts>,
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let host_header = req
        .headers()
        .get(header::HOST)
        .and_then(|val| val.to_str().ok());

    let is_valid = host_header.is_some_and(|host| {
        let domain = host_name(host).to_ascii_lowercase();
        matches!(domain.as_str(), "127.0.0.1" | "localhost" | "::1")
            || allowed
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .contains(&domain)
    });

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

/// Returns true when `provided` is the configured token. An empty configured token
/// never matches, so a blank token file cannot grant access.
pub fn token_matches(expected: &str, provided: &str) -> bool {
    !expected.is_empty() && expected == provided
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
            token_matches(&expected_token, token)
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

/// Returns the port of a `host:port` bind address, or 0 when it has none (any port).
pub fn configured_port(bind_addr: &str) -> u16 {
    bind_addr
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse().ok())
        .unwrap_or(0)
}

/// Returns true for the daemon's own web UI origin (loopback on `port`; any loopback
/// port when `port` is 0) and for browser extension origins. Extension ids differ per
/// install, and the Bearer token still guards every API call.
pub fn origin_allowed(origin: &str, port: u16) -> bool {
    if origin.starts_with("chrome-extension://") || origin.starts_with("moz-extension://") {
        return true;
    }
    let Some((host, origin_port)) = origin
        .strip_prefix("http://")
        .and_then(|rest| rest.rsplit_once(':'))
    else {
        return false;
    };
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
        && origin_port
            .parse::<u16>()
            .is_ok_and(|origin_port| port == 0 || origin_port == port)
}

/// Creates the CORS layer permitting the daemon's own origin and browser extensions.
pub fn create_cors_layer(port: u16) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::predicate(
            move |origin, _| {
                origin
                    .to_str()
                    .is_ok_and(|origin| origin_allowed(origin, port))
            },
        ))
        .allow_methods(Any)
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}
