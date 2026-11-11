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

    let config = DaemonConfig::load_or_init(args.config_dir, args.data_dir, args.bind);
    let db_path = config.data_dir.join("odm.sqlite");
    let store = Store::open(&db_path)?;
    let engine = Engine::new(store)?;
    let scheduler = std::sync::Arc::new(odm_daemon::scheduler::Scheduler::new(engine.clone()));
    let _scheduler_handle = scheduler.start();
    let state = AppState::new(engine, config.clone());

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
