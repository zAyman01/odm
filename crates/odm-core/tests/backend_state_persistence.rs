#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_core::{Job, JobOptions, Source, Store};
use tempfile::tempdir;

#[test]
fn test_migration_applies_cleanly_on_fresh_db() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let store = Store::open(&db_path).unwrap();

    let version = store.schema_version().unwrap();
    assert!(
        version >= 6,
        "schema version should be at least 6, got {version}"
    );
}

#[test]
fn test_backend_state_persistence_methods() {
    let store = Store::open_in_memory().unwrap();
    let dir = tempdir().unwrap();

    let mut job = Job::new(
        Source::Url("http://example.com/file.bin".to_string()),
        dir.path().to_path_buf(),
        JobOptions::default(),
    );
    store.insert_job(&mut job).unwrap();

    // Initially backend state should be None
    let initial_state = store.get_backend_state(job.id()).unwrap();
    assert_eq!(initial_state, None);

    // Set backend state
    let sample_json = r#"{"torrent_id":42}"#;
    store.set_backend_state(job.id(), sample_json).unwrap();

    // Retrieve backend state
    let retrieved_state = store.get_backend_state(job.id()).unwrap();
    assert_eq!(retrieved_state.as_deref(), Some(sample_json));

    // Also verify get_job retrieves it
    let loaded_job = store.get_job(job.id()).unwrap();
    assert_eq!(loaded_job.backend_state(), Some(sample_json));
}

#[test]
fn test_job_options_selected_files_and_seed_ratio_roundtrip() {
    let store = Store::open_in_memory().unwrap();
    let dir = tempdir().unwrap();

    let options = JobOptions {
        selected_files: Some(vec![0, 2, 5]),
        seed_ratio: Some(1.5),
        ..Default::default()
    };

    let mut job = Job::new(
        Source::Magnet("magnet:?xt=urn:btih:dummy".to_string()),
        dir.path().to_path_buf(),
        options.clone(),
    );
    store.insert_job(&mut job).unwrap();

    let loaded = store.get_job(job.id()).unwrap();
    assert_eq!(loaded.options().selected_files, Some(vec![0, 2, 5]));
    assert_eq!(loaded.options().seed_ratio, Some(1.5));
}

#[test]
fn update_job_does_not_clobber_backend_state() {
    let store = Store::open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    let mut job = Job::new(
        Source::Url("http://example.com/file.bin".to_string()),
        dir.path().to_path_buf(),
        JobOptions::default(),
    );
    store.insert_job(&mut job).unwrap();
    let stale = store.get_job(job.id()).unwrap();

    store.set_backend_state(job.id(), r#"{"x":1}"#).unwrap();
    store.update_job(&stale).unwrap();

    assert_eq!(
        store.get_backend_state(job.id()).unwrap().as_deref(),
        Some(r#"{"x":1}"#)
    );
}
