//! Store v3: settings key-value store and cascade job deletion.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_core::{Job, JobOptions, Segment, Source, Store};
use tempfile::tempdir;

#[test]
fn migration_3_creates_settings_and_persists_values() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.sqlite");
    let store = Store::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), 3);
    assert_eq!(store.get_setting("download_dir").unwrap(), None);

    store.set_setting("download_dir", "/downloads").unwrap();
    assert_eq!(
        store.get_setting("download_dir").unwrap().as_deref(),
        Some("/downloads")
    );

    // Overwrite setting
    store.set_setting("download_dir", "/new_downloads").unwrap();
    assert_eq!(
        store.get_setting("download_dir").unwrap().as_deref(),
        Some("/new_downloads")
    );
}

#[test]
fn delete_job_cascades_to_segments() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.sqlite");
    let store = Store::open(&path).unwrap();

    let mut job = Job::new(
        Source::Url("https://example.com/file.bin".to_string()),
        dir.path().to_path_buf(),
        JobOptions::default(),
    );
    store.insert_job(&mut job).unwrap();
    let id = job.id();

    let segments = vec![Segment {
        index: 0,
        start: 0,
        end: 100,
        downloaded: 50,
    }];
    store.replace_segments(id, &segments).unwrap();
    assert_eq!(store.list_segments(id).unwrap().len(), 1);

    store.delete_job(id).unwrap();
    assert!(store.get_job(id).is_err());
    assert_eq!(store.list_segments(id).unwrap().len(), 0);
}

#[test]
fn cannot_delete_default_queue() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.sqlite");
    let store = Store::open(&path).unwrap();

    assert!(store.delete_queue("default").is_err());

    store.create_queue("temporary", 2).unwrap();
    assert!(store.delete_queue("temporary").is_ok());
    assert!(store.queue_by_name("temporary").is_err());
}
