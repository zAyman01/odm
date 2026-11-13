//! Engine event bus and live progress streaming.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::http::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use axum::response::IntoResponse;
use axum::routing::get;
use odm_core::{Engine, Event, JobOptions, JobState, Store};
use tempfile::tempdir;
use tokio::net::TcpListener;

async fn test_server() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_clone = hits.clone();

    let payload: Vec<u8> = vec![b'x'; 256 * 1024];

    let app = Router::new().route(
        "/events_file.bin",
        get(move |req: Request| {
            let hits = hits_clone.clone();
            let payload = payload.clone();
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

    (format!("http://{addr}/events_file.bin"), hits)
}

#[tokio::test]
async fn engine_emits_added_state_and_removed_events() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join("store.sqlite")).unwrap();
    let engine = Engine::new(store).unwrap();

    let mut rx = engine.subscribe();
    let (url, _) = test_server().await;

    let job = engine
        .add(&url, dir.path().to_path_buf(), JobOptions::default())
        .unwrap();
    let id = job.id();

    // Event 1: JobAdded
    let event = rx.recv().await.unwrap();
    assert_eq!(event, Event::JobAdded(id));

    // Run to completion
    let finished_job = engine.run(id).await.unwrap();
    assert_eq!(finished_job.state(), JobState::Completed);

    // Collect subsequent events
    let mut states = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let Event::JobStateChanged { id: ev_id, state } = ev
            && ev_id == id
        {
            states.push(state);
        }
    }
    assert!(states.contains(&JobState::Downloading));
    assert!(states.contains(&JobState::Completed));

    // Remove job and file
    engine.remove(id, true).unwrap();
    let removed_event = rx.recv().await.unwrap();
    assert_eq!(removed_event, Event::JobRemoved(id));
}

#[tokio::test]
async fn pause_queued_job_moves_it_to_paused() {
    let store = Store::open_in_memory().unwrap();
    let engine = Engine::new(store.clone()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let job = engine
        .add(
            "http://127.0.0.1:1/file.bin",
            dir.path().to_path_buf(),
            JobOptions::default(),
        )
        .unwrap();
    let mut rx = engine.subscribe();

    engine.pause(job.id()).unwrap();

    assert_eq!(store.get_job(job.id()).unwrap().state(), JobState::Paused);
    let mut saw_paused = false;
    while let Ok(event) = rx.try_recv() {
        if matches!(
            event,
            Event::JobStateChanged {
                state: JobState::Paused,
                ..
            }
        ) {
            saw_paused = true;
        }
    }
    assert!(saw_paused, "no JobStateChanged{{Paused}} event");
}

#[tokio::test]
async fn user_filename_gets_conflict_suffix() {
    let (url, _hits) = test_server().await;
    let store = Store::open_in_memory().unwrap();
    let engine = Engine::new(store.clone()).unwrap();
    let dir = tempdir().unwrap();

    let mut names = Vec::new();
    for _ in 0..2 {
        let mut job = engine
            .add(&url, dir.path().to_path_buf(), JobOptions::default())
            .unwrap();
        job.set_filename("a.bin");
        store.update_job(&job).unwrap();
        let done = engine.run(job.id()).await.unwrap();
        names.push(done.filename().to_string());
    }

    assert_eq!(names, vec!["a.bin".to_string(), "a (1).bin".to_string()]);
}
