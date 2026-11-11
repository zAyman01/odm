//! Store v2: segments, queues and crash recovery.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use odm_core::{Job, JobOptions, JobState, Segment, Source, Store};
use tempfile::tempdir;

fn sample_job(target_dir: &Path) -> Job {
    Job::new(
        Source::Url("https://example.com/file.bin".to_string()),
        target_dir.to_path_buf(),
        JobOptions::default(),
    )
}

#[test]
fn schema_version_is_two_and_idempotent() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("odm.sqlite");

    {
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 2);
    }
    {
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 2);
    }
}

#[test]
fn segments_round_trip_and_clear() {
    let store = Store::open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    let mut job = sample_job(dir.path());
    store.insert_job(&mut job).unwrap();

    let segments = [
        Segment {
            index: 0,
            start: 0,
            end: 9,
            downloaded: 5,
        },
        Segment {
            index: 1,
            start: 10,
            end: 19,
            downloaded: 0,
        },
    ];
    store.replace_segments(job.id(), &segments).unwrap();
    assert_eq!(store.list_segments(job.id()).unwrap(), segments);

    store.replace_segments(job.id(), &segments[..1]).unwrap();
    assert_eq!(store.list_segments(job.id()).unwrap(), &segments[..1]);

    store.clear_segments(job.id()).unwrap();
    assert!(store.list_segments(job.id()).unwrap().is_empty());
}

#[test]
fn default_queue_exists_and_can_be_configured() {
    let store = Store::open_in_memory().unwrap();

    let queues = store.list_queues().unwrap();
    assert_eq!(queues.len(), 1);
    assert_eq!(queues[0].name, "default");
    assert_eq!(queues[0].max_concurrent, 3);

    let night = store.create_queue("night", 5).unwrap();
    assert_eq!(store.queue_by_name("night").unwrap(), night);

    store.set_queue_limit(night.id, 2).unwrap();
    assert_eq!(store.queue_by_name("night").unwrap().max_concurrent, 2);
}

#[test]
fn jobs_are_ordered_in_their_queue() {
    let store = Store::open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    let mut first = sample_job(dir.path());
    let mut second = sample_job(dir.path());
    let mut third = sample_job(dir.path());
    store.insert_job(&mut first).unwrap();
    store.insert_job(&mut second).unwrap();
    store.insert_job(&mut third).unwrap();

    let ids: Vec<_> = store
        .queue_jobs(1)
        .unwrap()
        .into_iter()
        .map(|job| job.id())
        .collect();
    assert_eq!(ids, [first.id(), second.id(), third.id()]);

    store.set_job_position(third.id(), 0).unwrap();
    let ids: Vec<_> = store
        .queue_jobs(1)
        .unwrap()
        .into_iter()
        .map(|job| job.id())
        .collect();
    assert_eq!(ids, [third.id(), first.id(), second.id()]);
}

#[test]
fn recovery_pauses_interrupted_jobs() {
    let store = Store::open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    let mut interrupted = sample_job(dir.path());
    let mut queued = sample_job(dir.path());
    store.insert_job(&mut interrupted).unwrap();
    store.insert_job(&mut queued).unwrap();

    interrupted.transition_to(JobState::Probing).unwrap();
    interrupted.transition_to(JobState::Downloading).unwrap();
    store.update_job(&interrupted).unwrap();

    assert_eq!(store.recover_interrupted().unwrap(), 1);
    assert_eq!(
        store.get_job(interrupted.id()).unwrap().state(),
        JobState::Paused
    );
    assert_eq!(
        store.get_job(queued.id()).unwrap().state(),
        JobState::Queued
    );
    assert_eq!(store.recover_interrupted().unwrap(), 0);
}
