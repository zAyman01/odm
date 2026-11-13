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
    pub allowed_hosts: crate::auth::AllowedHosts,
    pub http_client: Client,
    semaphores: Arc<Mutex<HashMap<String, QueueSlots>>>,
}

/// The slots of one queue and the limit they were sized for.
struct QueueSlots {
    semaphore: Arc<Semaphore>,
    limit: u32,
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

        let state = Self {
            engine,
            config,
            torrent: None,
            hls: None,
            dash: None,
            ytdlp: None,
            plugins,
            allowed_hosts: Arc::default(),
            http_client,
            semaphores: Arc::new(Mutex::new(HashMap::new())),
        };
        state.apply_global_speed_limit();
        state.reload_allowed_hosts();
        state
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
            .or_insert_with(|| QueueSlots {
                semaphore: Arc::new(Semaphore::new(max_concurrent.max(1) as usize)),
                limit: max_concurrent.max(1),
            })
            .semaphore
            .clone()
    }

    /// Resizes a queue's slots. A higher limit adds slots at once; a lower one
    /// retires slots as running jobs finish, and never stops a running job.
    pub fn set_queue_limit(&self, queue_name: &str, max_concurrent: u32) {
        let new_limit = max_concurrent.max(1);
        let mut map = self.semaphores.lock().unwrap_or_else(|p| p.into_inner());
        let Some(slots) = map.get_mut(queue_name) else {
            return;
        };
        let old_limit = std::mem::replace(&mut slots.limit, new_limit);
        if new_limit > old_limit {
            slots
                .semaphore
                .add_permits((new_limit - old_limit) as usize);
        } else if new_limit < old_limit {
            let semaphore = slots.semaphore.clone();
            let surplus = old_limit - new_limit;
            tokio::spawn(async move {
                if let Ok(permits) = semaphore.acquire_many_owned(surplus).await {
                    permits.forget();
                }
            });
        }
    }

    /// Reloads the host allow-list from the stored `allowed_hosts` setting.
    pub fn reload_allowed_hosts(&self) {
        let setting = self
            .engine
            .store()
            .get_setting("allowed_hosts")
            .ok()
            .flatten()
            .unwrap_or_default();
        *self
            .allowed_hosts
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            crate::auth::parse_allowed_hosts(&setting);
    }

    /// Applies the stored `global_speed_limit` setting to the running engine.
    pub fn apply_global_speed_limit(&self) {
        let limit = self
            .engine
            .store()
            .get_setting("global_speed_limit")
            .ok()
            .flatten()
            .and_then(|value| value.parse::<u64>().ok());
        self.engine.set_global_speed_limit(limit);
    }

    /// Starts every job that is still `Queued`, for example after a daemon restart.
    pub fn resume_queued(&self) {
        let store = self.engine.store();
        let (Ok(jobs), Ok(queues)) = (store.list_jobs(), store.list_queues()) else {
            return;
        };
        for job in jobs
            .iter()
            .filter(|job| job.state() == odm_core::JobState::Queued)
        {
            let (name, limit) = queues
                .iter()
                .find(|queue| queue.id == job.queue_id())
                .map_or(("default", 3), |queue| {
                    (queue.name.as_str(), queue.max_concurrent)
                });
            self.spawn_download(job.id(), name, limit);
        }
    }

    pub fn spawn_download(&self, id: JobId, queue_name: &str, max_concurrent: u32) {
        let sem = self.get_semaphore(queue_name, max_concurrent);
        let engine = self.engine.clone();
        let is_queued = move |engine: &odm_core::Engine| {
            engine
                .store()
                .get_job(id)
                .is_ok_and(|job| job.state() == odm_core::JobState::Queued)
        };
        // A job that waits for a slot as `Queued` may be paused or cancelled meanwhile.
        let waits_as_queued = is_queued(&engine);
        tokio::spawn(async move {
            let permit = sem.acquire().await;
            if !waits_as_queued || is_queued(&engine) {
                run_releasing_slot_when_seeding(&engine, id, permit).await;
            }
        });
    }
}

/// Runs the job and frees its queue slot as soon as it starts seeding, since
/// seeding uploads only and must not block the queued downloads behind it.
async fn run_releasing_slot_when_seeding(
    engine: &odm_core::Engine,
    id: JobId,
    permit: Result<tokio::sync::SemaphorePermit<'_>, tokio::sync::AcquireError>,
) {
    let mut events = engine.subscribe();
    let run = engine.run(id);
    tokio::pin!(run);
    let mut permit = permit.ok();
    while permit.is_some() {
        tokio::select! {
            _ = &mut run => return,
            event = events.recv() => match event {
                Ok(odm_core::Event::JobStateChanged { id: changed, state: odm_core::JobState::Seeding })
                    if changed == id => permit = None,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                _ => {}
            },
        }
    }
    let _ = run.await;
}
