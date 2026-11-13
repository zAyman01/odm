//! State machine tests for the job lifecycle.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_core::{Job, JobOptions, JobState, Source};

const QUEUED: &[JobState] = &[JobState::Probing, JobState::Paused, JobState::Cancelled];
const PROBING: &[JobState] = &[
    JobState::Downloading,
    JobState::Failed,
    JobState::Paused,
    JobState::Cancelled,
];
const DOWNLOADING: &[JobState] = &[
    JobState::Paused,
    JobState::Verifying,
    JobState::Seeding,
    JobState::Completed,
    JobState::Failed,
    JobState::Cancelled,
];
const PAUSED: &[JobState] = &[JobState::Queued, JobState::Cancelled];
const VERIFYING: &[JobState] = &[JobState::Completed, JobState::Failed];
const SEEDING: &[JobState] = &[JobState::Completed, JobState::Paused, JobState::Cancelled];
const FAILED: &[JobState] = &[JobState::Queued, JobState::Cancelled];

fn allowed(from: JobState) -> &'static [JobState] {
    match from {
        JobState::Queued => QUEUED,
        JobState::Probing => PROBING,
        JobState::Downloading => DOWNLOADING,
        JobState::Paused => PAUSED,
        JobState::Verifying => VERIFYING,
        JobState::Seeding => SEEDING,
        JobState::Failed => FAILED,
        JobState::Completed | JobState::Cancelled => &[],
    }
}

fn new_job() -> Job {
    Job::new(
        Source::Url("https://example.com/file.bin".to_string()),
        std::env::temp_dir(),
        JobOptions::default(),
    )
}

#[test]
fn state_machine_matches_the_spec_table() {
    for from in JobState::ALL {
        for to in JobState::ALL {
            assert_eq!(
                from.can_transition_to(to),
                allowed(from).contains(&to),
                "{from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn transition_to_updates_the_job() {
    let mut job = new_job();
    assert_eq!(job.state(), JobState::Queued);

    job.transition_to(JobState::Probing).unwrap();
    assert_eq!(job.state(), JobState::Probing);

    job.transition_to(JobState::Downloading).unwrap();
    assert_eq!(job.state(), JobState::Downloading);
}

#[test]
fn invalid_transition_is_rejected_and_state_is_unchanged() {
    let mut job = new_job();
    assert!(job.transition_to(JobState::Completed).is_err());
    assert_eq!(job.state(), JobState::Queued);
}

#[test]
fn completed_and_cancelled_are_final() {
    for terminal in [JobState::Completed, JobState::Cancelled] {
        for next in JobState::ALL {
            assert!(
                !terminal.can_transition_to(next),
                "{terminal:?} -> {next:?}"
            );
        }
    }
}

#[test]
fn job_set_filename_sanitises() {
    let mut job = Job::new(
        Source::Url("http://example.com/x".to_string()),
        std::path::PathBuf::from("."),
        JobOptions::default(),
    );
    job.set_filename("../../evil.exe");
    assert_eq!(job.filename(), "evil.exe");
    job.set_filename("..");
    assert_eq!(job.filename(), "download");
}

#[test]
fn apply_probe_sanitises() {
    let mut job = Job::new(
        Source::Url("http://example.com/x".to_string()),
        std::path::PathBuf::from("."),
        JobOptions::default(),
    );
    job.apply_probe(odm_core::ProbeResult {
        backend: odm_core::BackendKind::Http,
        filename: r"C:\Windows\system32\nul.txt".to_string(),
        total_bytes: None,
        supports_ranges: false,
        etag: None,
        last_modified: None,
        content_type: None,
    });
    assert_eq!(job.filename(), "_nul.txt");
}
