#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use odm_core::{BackendKind, Engine, JobOptions, Source, Store};
use tempfile::tempdir;

#[test]
fn test_add_download_selects_backend_based_on_source() {
    let store = Store::open_in_memory().unwrap();
    let engine = Engine::new(store).unwrap();
    let dir = tempdir().unwrap();
    let target_dir = dir.path().to_path_buf();

    // 1. Http URL selects Http backend
    let http_job = engine
        .add_download(
            Source::Url("https://example.com/file.zip".to_string()),
            target_dir.clone(),
            JobOptions::default(),
        )
        .unwrap();
    assert_eq!(http_job.backend(), BackendKind::Http);

    // 2. Magnet URI selects Torrent backend
    let magnet_job = engine
        .add_download(
            Source::Magnet(
                "magnet:?xt=urn:btih:c12fe1c06bba254a9dc9f519b335aa7c1367a88a".to_string(),
            ),
            target_dir.clone(),
            JobOptions::default(),
        )
        .unwrap();
    assert_eq!(magnet_job.backend(), BackendKind::Torrent);

    // 3. TorrentFile path selects Torrent backend
    let torrent_file_job = engine
        .add_download(
            Source::TorrentFile(PathBuf::from("/path/to/sample.torrent")),
            target_dir,
            JobOptions::default(),
        )
        .unwrap();
    assert_eq!(torrent_file_job.backend(), BackendKind::Torrent);
}

use async_trait::async_trait;
use odm_core::ProbeResult;
use odm_core::backend::{Backend, BackendError, JobContext, RunOutcome};
use std::sync::Arc;

struct MockTorrentBackend;

#[async_trait]
impl Backend for MockTorrentBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Torrent
    }

    async fn probe(
        &self,
        _source: &Source,
        _opts: &JobOptions,
    ) -> Result<ProbeResult, BackendError> {
        Ok(ProbeResult {
            backend: BackendKind::Torrent,
            filename: "torrent_file.bin".to_string(),
            total_bytes: Some(1024),
            supports_ranges: true,
            etag: None,
            last_modified: None,
            content_type: None,
        })
    }

    async fn run(&self, ctx: JobContext) -> Result<RunOutcome, BackendError> {
        ctx.progress.report(1024, Some(1024));
        Ok(RunOutcome::Completed)
    }
}

#[tokio::test]
async fn test_register_backend_and_engine_run() {
    let store = Store::open_in_memory().unwrap();
    let engine = Engine::new(store).unwrap();
    let dir = tempdir().unwrap();

    // Register our mock torrent backend
    engine.register_backend(Arc::new(MockTorrentBackend));

    let job = engine
        .add_download(
            Source::Magnet("magnet:?xt=urn:btih:dummy".to_string()),
            dir.path().to_path_buf(),
            JobOptions::default(),
        )
        .unwrap();

    let completed_job = engine.run(job.id()).await.unwrap();
    assert_eq!(completed_job.backend(), BackendKind::Torrent);
    assert_eq!(completed_job.filename(), "torrent_file.bin");
    assert_eq!(completed_job.total_bytes(), Some(1024));
    assert_eq!(completed_job.downloaded_bytes(), 1024);
}
