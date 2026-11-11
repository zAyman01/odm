//! SQLite persistence tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use odm_core::{Job, JobId, JobOptions, JobState, Source, Store, StoreError};
use tempfile::tempdir;

fn sample_job(target_dir: &Path) -> Job {
    Job::new(
        Source::Url("https://example.com/file.bin".to_string()),
        target_dir.to_path_buf(),
        JobOptions {
            headers: vec![("X-Test".to_string(), "yes".to_string())],
            user_agent: Some("odm-test".to_string()),
            referer: None,
            cookies: None,
            ..JobOptions::default()
        },
    )
}

#[test]
fn insert_and_get_round_trip() {
    let store = Store::open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    let mut job = sample_job(dir.path());

    store.insert_job(&mut job).unwrap();
    assert!(job.id().0 > 0);

    let loaded = store.get_job(job.id()).unwrap();
    assert_eq!(loaded.id(), job.id());
    assert_eq!(loaded.source(), job.source());
    assert_eq!(loaded.target_dir(), job.target_dir());
    assert_eq!(loaded.state(), JobState::Queued);
    assert_eq!(loaded.options().user_agent.as_deref(), Some("odm-test"));
    assert_eq!(loaded.options().headers.len(), 1);
}

#[test]
fn update_persists_state_error_and_progress() {
    let store = Store::open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    let mut job = sample_job(dir.path());
    store.insert_job(&mut job).unwrap();

    job.transition_to(JobState::Probing).unwrap();
    job.set_downloaded_bytes(42);
    job.set_error("boom", true);
    store.update_job(&job).unwrap();

    let loaded = store.get_job(job.id()).unwrap();
    assert_eq!(loaded.state(), JobState::Probing);
    assert_eq!(loaded.downloaded_bytes(), 42);
    let error = loaded.error().unwrap();
    assert_eq!(error.message, "boom");
    assert!(error.retryable);
}

#[test]
fn missing_job_is_reported() {
    let store = Store::open_in_memory().unwrap();
    match store.get_job(JobId(999)) {
        Err(StoreError::JobNotFound(id)) => assert_eq!(id, JobId(999)),
        other => panic!("expected JobNotFound, got {other:?}"),
    }
}

#[test]
fn migrations_are_idempotent() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("odm.sqlite");

    {
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 3);
    }
    {
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 3);
    }
}

#[test]
fn list_jobs_returns_inserted_jobs_in_id_order() {
    let store = Store::open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    let mut first = sample_job(dir.path());
    let mut second = sample_job(dir.path());
    store.insert_job(&mut first).unwrap();
    store.insert_job(&mut second).unwrap();

    let jobs = store.list_jobs().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0].id(), first.id());
    assert_eq!(jobs[1].id(), second.id());
}
