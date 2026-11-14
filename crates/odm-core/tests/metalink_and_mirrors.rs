//! Tests for Metalink parsing and multi-mirror failover.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::http::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use axum::response::IntoResponse;
use axum::routing::get;
use odm_core::checksum::ChecksumAlgorithm;
use odm_core::metalink::parse_metalink;
use odm_core::{Engine, JobOptions, JobState, Store};
use tempfile::tempdir;
use tokio::net::TcpListener;

#[test]
fn parse_metalink_v3_xml() {
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<metalink version="3.0" xmlns="http://www.metalinker.org/">
  <files>
    <file name="example.zip">
      <size>1048576</size>
      <verification>
        <hash type="sha256">b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9</hash>
      </verification>
      <resources>
        <url type="http" preference="100">https://primary.example.com/example.zip</url>
        <url type="http" preference="50">https://mirror.example.com/example.zip</url>
      </resources>
    </file>
  </files>
</metalink>"#;

    let spec = parse_metalink(xml).unwrap().remove(0);
    assert_eq!(spec.filename, "example.zip");
    assert_eq!(spec.size, Some(1048576));
    assert_eq!(
        spec.urls,
        vec![
            "https://primary.example.com/example.zip",
            "https://mirror.example.com/example.zip"
        ]
    );
    let cs = spec.checksum.unwrap();
    assert_eq!(cs.algorithm, ChecksumAlgorithm::Sha256);
    assert_eq!(
        cs.expected,
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
}

#[test]
fn parse_meta4_v4_xml() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="package.tar.gz">
    <size>2048</size>
    <hash type="sha-256">2aae6c35c94fcfb415dbe95f408b9ce91ee846ed000000000000000000000000</hash>
    <url priority="1">https://mirror1.example.org/package.tar.gz</url>
    <url priority="2">https://mirror2.example.org/package.tar.gz</url>
  </file>
</metalink>"#;

    let spec = parse_metalink(xml).unwrap().remove(0);
    assert_eq!(spec.filename, "package.tar.gz");
    assert_eq!(spec.size, Some(2048));
    assert_eq!(
        spec.urls,
        vec![
            "https://mirror1.example.org/package.tar.gz",
            "https://mirror2.example.org/package.tar.gz"
        ]
    );
    let cs = spec.checksum.unwrap();
    assert_eq!(cs.algorithm, ChecksumAlgorithm::Sha256);
}

#[test]
fn parse_invalid_xml_returns_error() {
    let xml = "<not-metalink></not-metalink>";
    let result = parse_metalink(xml);
    assert!(result.is_err());
}

async fn faulty_primary_server(payload_len: usize, get_status: StatusCode) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let app = Router::new().route(
        "/file.bin",
        get(move |req: Request| async move {
            // Allow HEAD probe, but fail on GET
            if req.method() == axum::http::Method::HEAD {
                (
                    StatusCode::OK,
                    [
                        (CONTENT_TYPE, "application/octet-stream"),
                        (CONTENT_LENGTH, payload_len.to_string().leak() as &str),
                        (ACCEPT_RANGES, "bytes"),
                    ],
                    "",
                )
                    .into_response()
            } else {
                (get_status, "Primary failed").into_response()
            }
        }),
    );

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{addr}/file.bin")
}

async fn working_mirror_server(payload: &'static [u8]) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_clone = hits.clone();

    let app = Router::new().route(
        "/file.bin",
        get(move |req: Request| {
            let hits = hits_clone.clone();
            async move {
                hits.fetch_add(1, Ordering::SeqCst);
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
            }
        }),
    );

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("http://{addr}/file.bin"), hits)
}

#[tokio::test]
async fn multi_mirror_download_fails_over_to_working_mirror() {
    let payload = b"hello world mirror failover payload!";
    let primary_url = faulty_primary_server(payload.len(), StatusCode::INTERNAL_SERVER_ERROR).await;
    let (mirror_url, mirror_hits) = working_mirror_server(payload).await;

    let dir = tempdir().unwrap();
    let db_path = dir.path().join("store.db");
    let store = Store::open(&db_path).unwrap();
    let engine = Engine::new(store).unwrap();

    let job = engine
        .add(
            &primary_url,
            dir.path().to_path_buf(),
            JobOptions {
                mirrors: vec![mirror_url],
                ..Default::default()
            },
        )
        .unwrap();

    let completed_job = engine.run(job.id()).await.unwrap();
    assert_eq!(completed_job.state(), JobState::Completed);
    assert_eq!(completed_job.downloaded_bytes(), payload.len() as u64);

    let content = std::fs::read(completed_job.target_path()).unwrap();
    assert_eq!(content, payload);
    assert!(mirror_hits.load(Ordering::SeqCst) > 0);
}

#[tokio::test]
async fn mirror_returning_404_falls_through_to_next_mirror() {
    let payload = b"hello world mirror failover payload!";
    let primary_url = faulty_primary_server(payload.len(), StatusCode::NOT_FOUND).await;
    let (mirror_url, mirror_hits) = working_mirror_server(payload).await;

    let dir = tempdir().unwrap();
    let engine = Engine::new(Store::open(dir.path().join("store.db")).unwrap()).unwrap();
    let job = engine
        .add(
            &primary_url,
            dir.path().to_path_buf(),
            JobOptions {
                mirrors: vec![mirror_url],
                ..Default::default()
            },
        )
        .unwrap();

    let completed_job = engine.run(job.id()).await.unwrap();

    assert_eq!(completed_job.state(), JobState::Completed);
    assert_eq!(std::fs::read(completed_job.target_path()).unwrap(), payload);
    assert!(mirror_hits.load(Ordering::SeqCst) > 0);
}

#[test]
fn metalink_with_two_files_yields_two_specs() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="one.bin">
    <size>10</size>
    <hash type="sha-256">1111111111111111111111111111111111111111111111111111111111111111</hash>
    <url>https://one.example.com/one.bin</url>
  </file>
  <file name="two.bin">
    <size>20</size>
    <hash type="md5">22222222222222222222222222222222</hash>
    <url priority="2">https://mirror.example.com/two.bin</url>
    <url priority="1">https://two.example.com/two.bin</url>
  </file>
</metalink>"#;

    let specs = parse_metalink(xml).unwrap();

    assert_eq!(specs.len(), 2);
    assert_eq!(specs[0].filename, "one.bin");
    assert_eq!(specs[0].size, Some(10));
    assert_eq!(specs[0].urls, vec!["https://one.example.com/one.bin"]);
    assert_eq!(
        specs[0].checksum.as_ref().unwrap().algorithm,
        ChecksumAlgorithm::Sha256
    );
    assert_eq!(specs[1].filename, "two.bin");
    assert_eq!(specs[1].size, Some(20));
    assert_eq!(
        specs[1].urls,
        vec![
            "https://two.example.com/two.bin",
            "https://mirror.example.com/two.bin"
        ]
    );
    assert_eq!(
        specs[1].checksum.as_ref().unwrap().algorithm,
        ChecksumAlgorithm::Md5
    );
}

#[test]
fn ampersand_entity_in_url_is_unescaped() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="a.bin"><url>https://example.com/a?x=1&amp;y=2</url></file>
</metalink>"#;

    let specs = parse_metalink(xml).unwrap();

    assert_eq!(specs[0].urls, vec!["https://example.com/a?x=1&y=2"]);
}
