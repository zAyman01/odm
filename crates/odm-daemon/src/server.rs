//! HTTP and WebSocket server construction and routing.
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::middleware::from_fn_with_state;
use axum::routing::{get, post};
use odm_core::{Engine, Store};
use odm_media::dash::DashBackend;
use odm_media::hls::HlsBackend;
use odm_media::ytdlp::YtDlpBackend;

use crate::auth::{create_cors_layer, require_bearer_token, validate_host_header};
use crate::config::DaemonConfig;
use crate::routes::*;
use crate::state::AppState;
use crate::webui::{webui_handler, webui_index_handler};
use crate::ws::ws_handler;

/// Locate yt-dlp binary: checks `settings_path` if non-empty, otherwise searches `PATH`.
pub fn find_ytdlp(settings_path: &str) -> Option<PathBuf> {
    let trimmed = settings_path.trim();
    if !trimmed.is_empty() {
        let path = PathBuf::from(trimmed);
        if path.is_file() {
            return Some(path);
        }
        #[cfg(windows)]
        {
            for ext in ["exe", "bat", "cmd"] {
                let with_ext = path.with_extension(ext);
                if with_ext.is_file() {
                    return Some(with_ext);
                }
            }
        }
        return None;
    }

    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join("yt-dlp");
            if candidate.is_file() {
                return Some(candidate);
            }
            #[cfg(windows)]
            {
                for ext in ["exe", "bat", "cmd"] {
                    let candidate_ext = dir.join(format!("yt-dlp.{ext}"));
                    if candidate_ext.is_file() {
                        return Some(candidate_ext);
                    }
                }
            }
        }
    }

    None
}

pub type MediaBackends = (
    Option<Arc<HlsBackend>>,
    Option<Arc<DashBackend>>,
    Option<Arc<YtDlpBackend>>,
);

/// Initializes plugin backend and discovers installed plugins in `<data_dir>/plugins`.
pub fn init_plugin_backend(
    data_dir: &std::path::Path,
    engine: &Engine,
) -> (
    Arc<tokio::sync::RwLock<odm_plugin::PluginRegistry>>,
    Arc<odm_plugin::PluginBackend>,
) {
    let plugins_dir = data_dir.join("plugins");
    let _ = std::fs::create_dir_all(&plugins_dir);
    let mut registry = odm_plugin::PluginRegistry::new(plugins_dir);
    if let Err(e) = registry.discover() {
        tracing::warn!("Failed to discover plugins: {e}");
    }
    let registry = Arc::new(tokio::sync::RwLock::new(registry));
    let backend = Arc::new(odm_plugin::PluginBackend::new(registry.clone()));
    engine.register_backend(backend.clone());
    (registry, backend)
}

/// Initializes media backends (HLS, DASH, yt-dlp) based on settings and registers them with the engine.
pub fn init_media_backends(store: &Store, engine: &Engine) -> MediaBackends {
    let client = odm_media::reqwest::Client::builder()
        .user_agent(concat!("odm/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default();

    let ffmpeg_setting = store
        .get_setting("ffmpeg_path")
        .ok()
        .flatten()
        .unwrap_or_default();
    let ytdlp_setting = store
        .get_setting("ytdlp_path")
        .ok()
        .flatten()
        .unwrap_or_default();

    let ffmpeg_path = odm_media::mux::find_ffmpeg(&ffmpeg_setting);
    let ytdlp_path = find_ytdlp(&ytdlp_setting);

    let hls = Arc::new(HlsBackend::new(client.clone(), ffmpeg_path.clone()));
    engine.register_backend(hls.clone());

    let dash = if let Some(ref ff) = ffmpeg_path {
        let backend = Arc::new(DashBackend::new(client.clone(), Some(ff.clone())));
        engine.register_backend(backend.clone());
        Some(backend)
    } else {
        None
    };

    let ytdlp = if let Some(yt) = ytdlp_path {
        let backend = Arc::new(YtDlpBackend::new(yt));
        engine.register_backend(backend.clone());
        Some(backend)
    } else {
        None
    };

    (Some(hls), dash, ytdlp)
}

/// Builds the Axum router for the daemon given the application state.
pub fn build_router_with_state(state: AppState) -> Router {
    let token = state.config.token.clone();
    let port = crate::auth::configured_port(&state.config.bind_addr);
    let allowed_hosts = state.allowed_hosts.clone();

    let api_routes = Router::new()
        .route("/health", get(health_handler))
        .route("/probe", post(probe_handler))
        .route("/plugins", get(get_plugins_handler))
        .route("/grabber", post(grabber_handler))
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
            "/downloads/{id}/position",
            axum::routing::put(set_position_handler),
        )
        .route(
            "/queues",
            get(list_queues_handler).post(create_queue_handler),
        )
        .route(
            "/queues/{name}",
            axum::routing::patch(update_queue_handler).delete(delete_queue_handler),
        )
        .route("/categories", get(list_categories_handler))
        .route(
            "/categories/{name}",
            axum::routing::put(set_category_handler).delete(delete_category_handler),
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
        .route("/", get(webui_index_handler))
        .route("/{*path}", get(webui_handler))
        .layer(from_fn_with_state(token, require_bearer_token))
        .layer(from_fn_with_state(allowed_hosts, validate_host_header))
        .layer(create_cors_layer(port))
}

/// Builds the Axum router for testing from configuration.
pub fn build_router(config: DaemonConfig) -> Result<Router, Box<dyn std::error::Error>> {
    let store = Store::open_in_memory()?;
    let engine = Engine::new(store.clone())?;
    let (plugins, _) = init_plugin_backend(&config.data_dir, &engine);
    let mut state = AppState::new(engine.clone(), config).with_plugins(plugins);
    let (hls, dash, ytdlp) = init_media_backends(&store, &engine);
    if let Some(h) = hls {
        state = state.with_hls(h);
    }
    if let Some(d) = dash {
        state = state.with_dash(d);
    }
    if let Some(y) = ytdlp {
        state = state.with_ytdlp(y);
    }
    Ok(build_router_with_state(state))
}
