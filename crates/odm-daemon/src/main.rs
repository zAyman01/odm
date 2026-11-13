//! The odm background service: REST and WebSocket API over the download engine.
#![forbid(unsafe_code)]

use std::path::PathBuf;

use clap::Parser;
use odm_core::{Engine, Store};
use odm_daemon::config::DaemonConfig;
use odm_daemon::server::build_router_with_state;
use odm_daemon::state::AppState;

#[derive(Parser, Debug)]
#[command(name = "odm-daemon", about = "odm background service", version)]
struct Args {
    #[arg(long, help = "Address to bind to (e.g. 127.0.0.1:6820)")]
    bind: Option<String>,

    #[arg(long, help = "Path to configuration directory")]
    config_dir: Option<PathBuf>,

    #[arg(long, help = "Path to data directory")]
    data_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    tracing_subscriber::fmt::init();

    let config = DaemonConfig::load_or_init(args.config_dir, args.data_dir, args.bind)?;
    let db_path = config.data_dir.join("odm.sqlite");
    let store = Store::open(&db_path)?;
    let engine = Engine::new(store.clone())?;

    let torrent_listen_port = store
        .get_setting("torrent_listen_port")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let seed_ratio = store
        .get_setting("seed_ratio")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    let torrent_dir = config.data_dir.join("torrents");
    let _ = std::fs::create_dir_all(&torrent_dir);
    let torrent = match odm_torrent::TorrentBackend::new(torrent_dir, torrent_listen_port).await {
        Ok(mut t) => {
            t.set_default_seed_ratio(seed_ratio);
            let t = std::sync::Arc::new(t);
            engine.register_backend(t.clone());
            Some(t)
        }
        Err(e) => {
            tracing::warn!("Failed to initialize torrent backend: {e}");
            None
        }
    };

    let scheduler = std::sync::Arc::new(odm_daemon::scheduler::Scheduler::new(engine.clone()));
    let _scheduler_handle = scheduler.start();
    let mut state = AppState::new(engine.clone(), config.clone());
    let (plugins, _) = odm_daemon::server::init_plugin_backend(&config.data_dir, &engine);
    state = state.with_plugins(plugins);
    if let Some(t) = torrent {
        state = state.with_torrent(t);
    }
    let (hls, dash, ytdlp) = odm_daemon::server::init_media_backends(&store, &engine);
    if let Some(h) = hls {
        state = state.with_hls(h);
    }
    if let Some(d) = dash {
        state = state.with_dash(d);
    }
    if let Some(y) = ytdlp {
        state = state.with_ytdlp(y);
    }

    state.resume_queued();
    let app = build_router_with_state(state);

    let listener = match tokio::net::TcpListener::bind(&config.bind_addr).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("Failed to bind to {}: {}", config.bind_addr, err);
            std::process::exit(1);
        }
    };

    println!("odm-daemon listening on http://{}", config.bind_addr);
    axum::serve(listener, app).await?;

    Ok(())
}
