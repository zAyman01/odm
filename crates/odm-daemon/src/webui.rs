//! Embedded web UI static file serving.
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../webui/"]
struct WebUiAssets;

pub async fn webui_handler(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    serve_file(&path)
}

pub async fn webui_index_handler() -> impl IntoResponse {
    serve_file("index.html")
}

fn serve_file(path: &str) -> Response {
    let clean_path = path.trim_start_matches('/');
    match WebUiAssets::get(clean_path) {
        Some(content) => {
            let mime = mime_guess::from_path(clean_path)
                .first_or_text_plain()
                .to_string();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime)],
                content.data.to_vec(),
            )
                .into_response()
        }
        None => {
            // SPA fallback: serve index.html for unknown paths
            match WebUiAssets::get("index.html") {
                Some(content) => (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/html".to_string())],
                    content.data.to_vec(),
                )
                    .into_response(),
                None => StatusCode::NOT_FOUND.into_response(),
            }
        }
    }
}
