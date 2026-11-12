//! Shared daemon application state.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use odm_core::{Engine, JobId};
use odm_media::dash::DashBackend;
use odm_media::hls::HlsBackend;
use odm_media::reqwest::Client;
use odm_media::ytdlp::YtDlpBackend;
use odm_plugin::{PluginBackend, PluginRegistry};
use odm_torrent::TorrentBackend;
use tokio::sync::{RwLock, Semaphore};

use crate::config::DaemonConfig;

#[derive(Clone)]
pub struct AppState {
    pub engine: Engine,
    pub config: DaemonConfig,
    pub torrent: Option<Arc<TorrentBackend>>,
    pub hls: Option<Arc<HlsBackend>>,
    pub dash: Option<Arc<DashBackend>>,
    pub ytdlp: Option<Arc<YtDlpBackend>>,
    pub plugins: Arc<RwLock<PluginRegistry>>,
    pub http_client: Client,
    semaphores: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
}

impl AppState {
    pub fn new(engine: Engine, config: DaemonConfig) -> Self {
        let plugins_dir = config.data_dir.join("plugins");
        let _ = std::fs::create_dir_all(&plugins_dir);
        let mut registry = PluginRegistry::new(plugins_dir);
        let _ = registry.discover();
        let plugins = Arc::new(RwLock::new(registry));
        let plugin_backend = Arc::new(PluginBackend::new(plugins.clone()));
        engine.register_backend(plugin_backend);

        let http_client = Client::builder()
            .user_agent(concat!("odm/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();

        Self {
            engine,
            config,
            torrent: None,
            hls: None,
            dash: None,
            ytdlp: None,
            plugins,
            http_client,
            semaphores: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_http_client(mut self, client: Client) -> Self {
        self.http_client = client;
        self
    }

    pub fn with_plugins(mut self, plugins: Arc<RwLock<PluginRegistry>>) -> Self {
        self.plugins = plugins;
        self
    }

    pub fn with_torrent(mut self, torrent: Arc<TorrentBackend>) -> Self {
        self.torrent = Some(torrent);
        self
    }

    pub fn with_hls(mut self, hls: Arc<HlsBackend>) -> Self {
        self.hls = Some(hls);
        self
    }

    pub fn with_dash(mut self, dash: Arc<DashBackend>) -> Self {
        self.dash = Some(dash);
        self
    }

    pub fn with_ytdlp(mut self, ytdlp: Arc<YtDlpBackend>) -> Self {
        self.ytdlp = Some(ytdlp);
        self
    }

    pub fn get_semaphore(&self, queue_name: &str, max_concurrent: u32) -> Arc<Semaphore> {
        let mut map = self.semaphores.lock().unwrap_or_else(|p| p.into_inner());
        map.entry(queue_name.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(max_concurrent.max(1) as usize)))
            .clone()
    }

    pub fn spawn_download(&self, id: JobId, queue_name: &str, max_concurrent: u32) {
        let sem = self.get_semaphore(queue_name, max_concurrent);
        let engine = self.engine.clone();
        tokio::spawn(async move {
            let permit = sem.acquire().await;
            let _ = engine.run(id).await;
            drop(permit);
        });
    }
}
