//! Checksum verification unit and lifecycle tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use tempfile::NamedTempFile;

use odm_core::checksum::{Checksum, ChecksumAlgorithm, verify_file_checksum};

#[tokio::test]
async fn verify_sha256_matches_known_digest() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"hello world").unwrap();
    file.flush().unwrap();

    let checksum = Checksum {
        algorithm: ChecksumAlgorithm::Sha256,
        expected: "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9".to_string(),
    };

    assert!(verify_file_checksum(file.path(), &checksum).await.is_ok());
}

#[tokio::test]
async fn verify_sha1_matches_known_digest() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"hello world").unwrap();
    file.flush().unwrap();

    let checksum = Checksum {
        algorithm: ChecksumAlgorithm::Sha1,
        expected: "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed".to_string(),
    };

    assert!(verify_file_checksum(file.path(), &checksum).await.is_ok());
}

#[tokio::test]
async fn verify_md5_matches_known_digest() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"hello world").unwrap();
    file.flush().unwrap();

    let checksum = Checksum {
        algorithm: ChecksumAlgorithm::Md5,
        expected: "5eb63bbbe01eeed093cb22bb8f5acdc3".to_string(),
    };

    assert!(verify_file_checksum(file.path(), &checksum).await.is_ok());
}

#[tokio::test]
async fn verify_mismatch_returns_error() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"hello world").unwrap();
    file.flush().unwrap();

    let checksum = Checksum {
        algorithm: ChecksumAlgorithm::Sha256,
        expected: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
    };

    let result = verify_file_checksum(file.path(), &checksum).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("checksum mismatch"));
}

use axum::Router;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::http::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use axum::response::IntoResponse;
use axum::routing::get;
use odm_core::{Engine, Event, JobOptions, JobState, Store};
use tempfile::tempdir;
use tokio::net::TcpListener;

async fn test_server(payload: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let app = Router::new().route(
        "/file.bin",
        get(move |req: Request| async move {
            let range = req
                .headers()
                .get(RANGE)
                .and_then(|val| val.to_str().ok())
                .and_then(|val| val.strip_prefix("bytes="))
                .and_then(|val| val.split_once('-'))
                .and_then(|(s, e)| Some((s.parse::<usize>().ok()?, e.parse::<usize>().ok()?)));

            if let Some((start, end)) = range {
                let end = end.min(payload.len().saturating_sub(1));
                let slice = payload[start..=end].to_vec();
                (
                    StatusCode::PARTIAL_CONTENT,
                    [
                        (CONTENT_TYPE, "application/octet-stream"),
                        (
                            CONTENT_RANGE,
                            format!("bytes {start}-{end}/{}", payload.len()).leak() as &str,
                        ),
                        (CONTENT_LENGTH, slice.len().to_string().leak() as &str),
                        (ACCEPT_RANGES, "bytes"),
                    ],
                    slice,
                )
                    .into_response()
            } else {
                (
                    StatusCode::OK,
                    [
                        (CONTENT_TYPE, "application/octet-stream"),
                        (CONTENT_LENGTH, payload.len().to_string().leak() as &str),
                        (ACCEPT_RANGES, "bytes"),
                    ],
                    payload,
                )
                    .into_response()
            }
        }),
    );

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{addr}/file.bin")
}

#[tokio::test]
async fn engine_verifies_checksum_and_transitions_verifying_to_completed() {
    let payload = b"hello from engine checksum test";
    let url = test_server(payload).await;

    let dir = tempdir().unwrap();
    let db_path = dir.path().join("store.db");
    let store = Store::open(&db_path).unwrap();
    let engine = Engine::new(store).unwrap();
    let mut rx = engine.subscribe();

    // sha256 of b"hello from engine checksum test"
    // can be verified via sha2
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    hasher.update(payload);
    let expected_hash = format!("{:x}", hasher.finalize());

    let job = engine
        .add(
            &url,
            dir.path().to_path_buf(),
            JobOptions {
                checksum: Some(Checksum {
                    algorithm: ChecksumAlgorithm::Sha256,
                    expected: expected_hash,
                }),
                ..Default::default()
            },
        )
        .unwrap();

    let run_handle = tokio::spawn({
        let engine = engine.clone();
        let id = job.id();
        async move { engine.run(id).await }
    });

    let mut saw_verifying = false;
    let mut saw_completed = false;

    while let Ok(event) = rx.recv().await {
        if let Event::JobStateChanged { id, state } = event
            && id == job.id()
        {
            if state == JobState::Verifying {
                saw_verifying = true;
            }
            if state == JobState::Completed {
                saw_completed = true;
                break;
            }
        }
    }

    let completed_job = run_handle.await.unwrap().unwrap();
    assert_eq!(completed_job.state(), JobState::Completed);
    assert!(saw_verifying);
    assert!(saw_completed);
}

#[tokio::test]
async fn engine_checksum_mismatch_fails_job() {
    let payload = b"corrupted payload data";
    let url = test_server(payload).await;

    let dir = tempdir().unwrap();
    let db_path = dir.path().join("store.db");
    let store = Store::open(&db_path).unwrap();
    let engine = Engine::new(store).unwrap();
    let mut rx = engine.subscribe();

    let job = engine
        .add(
            &url,
            dir.path().to_path_buf(),
            JobOptions {
                checksum: Some(Checksum {
                    algorithm: ChecksumAlgorithm::Sha256,
                    expected: "0000000000000000000000000000000000000000000000000000000000000000"
                        .to_string(),
                }),
                ..Default::default()
            },
        )
        .unwrap();

    let run_handle = tokio::spawn({
        let engine = engine.clone();
        let id = job.id();
        async move { engine.run(id).await }
    });

    let mut saw_verifying = false;
    let mut saw_failed = false;

    while let Ok(event) = rx.recv().await {
        if let Event::JobStateChanged { id, state } = event
            && id == job.id()
        {
            if state == JobState::Verifying {
                saw_verifying = true;
            }
            if state == JobState::Failed {
                saw_failed = true;
                break;
            }
        }
    }

    let run_result = run_handle.await.unwrap();
    assert!(run_result.is_err());
    assert!(saw_verifying);
    assert!(saw_failed);

    let failed_job = engine.store().get_job(job.id()).unwrap();
    assert_eq!(failed_job.state(), JobState::Failed);
    let err_info = failed_job.error().unwrap();
    assert!(err_info.message.contains("checksum mismatch"));
    assert!(!err_info.retryable);
}
