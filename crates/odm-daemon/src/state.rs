//! Shared daemon application state.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use odm_core::{Engine, JobId};
use tokio::sync::Semaphore;

use crate::config::DaemonConfig;

#[derive(Clone)]
pub struct AppState {
    pub engine: Engine,
    pub config: DaemonConfig,
    semaphores: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
}

impl AppState {
    pub fn new(engine: Engine, config: DaemonConfig) -> Self {
        Self {
            engine,
            config,
            semaphores: Arc::new(Mutex::new(HashMap::new())),
        }
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
