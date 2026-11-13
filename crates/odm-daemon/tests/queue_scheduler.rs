//! Queue scheduler integration tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_core::schedule::{Clock, QueueSchedule};
use odm_core::{Engine, Job, JobOptions, JobState, Source, Store};
use odm_daemon::scheduler::Scheduler;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

#[derive(Clone)]
struct MockClock {
    current: Arc<Mutex<(String, String)>>,
}

impl MockClock {
    fn new(day: &str, time: &str) -> Self {
        Self {
            current: Arc::new(Mutex::new((day.to_string(), time.to_string()))),
        }
    }

    fn set(&self, day: &str, time: &str) {
        let mut guard = self.current.lock().unwrap();
        *guard = (day.to_string(), time.to_string());
    }
}

impl Clock for MockClock {
    fn now(&self) -> (String, String) {
        self.current.lock().unwrap().clone()
    }
}

fn sample_job(target_dir: &Path) -> Job {
    Job::new(
        Source::Url("http://127.0.0.1:9/never".to_string()),
        target_dir.to_path_buf(),
        JobOptions::default(),
    )
}

#[test]
fn schedule_evaluation_matches_start_time_and_weekday() {
    let schedule = QueueSchedule::new(
        "08:00",
        Some("17:00".to_string()),
        vec!["mon".to_string(), "wed".to_string(), "fri".to_string()],
    );

    assert!(schedule.should_start("mon", "08:00"));
    assert!(!schedule.should_start("tue", "08:00"));
    assert!(!schedule.should_start("mon", "08:01"));
    assert!(schedule.should_stop("wed", "17:00"));
    assert!(!schedule.should_stop("thu", "17:00"));
}

#[tokio::test]
async fn scheduler_auto_starts_queue_at_start_time() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join("test.sqlite")).unwrap();
    let queue = store
        .create_queue_with_schedule(
            "nightly",
            2,
            Some(&QueueSchedule::new(
                "22:00",
                Some("04:00".to_string()),
                vec!["mon".to_string()],
            )),
        )
        .unwrap();

    let mut job = sample_job(dir.path());
    job.set_queue_id(queue.id);
    store.insert_job(&mut job).unwrap();

    let engine = Engine::new(store).unwrap();
    let clock = Arc::new(MockClock::new("mon", "21:59"));
    let scheduler = Scheduler::new_with_clock(engine.clone(), clock.clone());

    // 21:59 - does not start
    scheduler.tick().await;
    assert_eq!(
        engine.store().get_job(job.id()).unwrap().state(),
        JobState::Queued
    );

    // 22:00 - hits start time, triggers queue run
    clock.set("mon", "22:00");
    scheduler.tick().await;

    // Small yield for background queue spawn
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    // The job should have moved out of Queued (e.g. Probing/Connecting/Failed since mock url is unreachable)
    let state = engine.store().get_job(job.id()).unwrap().state();
    assert_ne!(state, JobState::Queued);
}

#[tokio::test]
async fn scheduler_auto_pauses_running_jobs_at_stop_time() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join("test.sqlite")).unwrap();
    let queue = store
        .create_queue_with_schedule(
            "nightly",
            2,
            Some(&QueueSchedule::new(
                "22:00",
                Some("04:00".to_string()),
                vec!["mon".to_string()],
            )),
        )
        .unwrap();

    let mut job = sample_job(dir.path());
    job.set_queue_id(queue.id);
    store.insert_job(&mut job).unwrap();
    // Simulate job in Downloading state
    job.transition_to(JobState::Probing).unwrap();
    job.transition_to(JobState::Downloading).unwrap();
    store.update_job(&job).unwrap();

    let engine = Engine::new(store).unwrap();
    let clock = Arc::new(MockClock::new("mon", "04:00"));
    let scheduler = Scheduler::new_with_clock(engine.clone(), clock.clone());

    scheduler.tick().await;

    let updated = engine.store().get_job(job.id()).unwrap();
    assert_eq!(updated.state(), JobState::Paused);
}

#[tokio::test]
async fn queued_jobs_start_after_restart() {
    use axum::Router;
    use axum::routing::get;
    use odm_daemon::config::DaemonConfig;
    use odm_daemon::state::AppState;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new().route("/file.bin", get(|| async { vec![7u8; 2048] }));
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join("test.sqlite")).unwrap();
    let mut job = Job::new(
        Source::Url(format!("http://{addr}/file.bin")),
        dir.path().to_path_buf(),
        JobOptions::default(),
    );
    store.insert_job(&mut job).unwrap();

    let engine = Engine::new(store).unwrap();
    let state = AppState::new(engine.clone(), DaemonConfig::for_test(dir.path(), "t"));
    state.resume_queued();

    let mut final_state = JobState::Queued;
    for _ in 0..100 {
        final_state = engine.store().get_job(job.id()).unwrap().state();
        if final_state == JobState::Completed {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(final_state, JobState::Completed);
}

#[tokio::test]
async fn paused_waiting_job_does_not_start_when_slot_frees() {
    use odm_daemon::config::DaemonConfig;
    use odm_daemon::state::AppState;

    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join("test.sqlite")).unwrap();
    let mut job = sample_job(dir.path());
    store.insert_job(&mut job).unwrap();
    let engine = Engine::new(store).unwrap();
    let state = AppState::new(engine.clone(), DaemonConfig::for_test(dir.path(), "t"));

    let slot = state.get_semaphore("busy", 1);
    let held = slot.clone().acquire_owned().await.unwrap();
    state.spawn_download(job.id(), "busy", 1);
    engine.pause(job.id()).unwrap();
    drop(held);

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        engine.store().get_job(job.id()).unwrap().state(),
        JobState::Paused
    );
}

mod settings_take_effect {
    use axum::Router;
    use axum::http::StatusCode;
    use axum::http::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HOST};
    use axum::response::IntoResponse;
    use axum::routing::get;
    use odm_api::{AddDownloadRequest, AddDownloadResponse};
    use odm_core::{Engine, JobId, JobState, Store};
    use odm_daemon::config::DaemonConfig;
    use odm_daemon::server::build_router_with_state;
    use odm_daemon::state::AppState;
    use tempfile::tempdir;
    use tokio::net::TcpListener;
    use tower::ServiceExt;

    fn state_in(dir: &std::path::Path) -> AppState {
        let engine = Engine::new(Store::open_in_memory().unwrap()).unwrap();
        AppState::new(engine, DaemonConfig::for_test(dir, "secret"))
    }

    async fn send(
        app: &Router,
        method: &str,
        uri: &str,
        body: serde_json::Value,
    ) -> (StatusCode, Vec<u8>) {
        let req = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header(HOST, "127.0.0.1")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, bytes.to_vec())
    }

    async fn add(app: &Router, url: String, dir: &std::path::Path, queue: Option<&str>) -> i64 {
        let body = serde_json::to_value(AddDownloadRequest {
            url,
            target_dir: Some(dir.to_string_lossy().to_string()),
            filename: None,
            queue: queue.map(str::to_string),
            category: None,
            options: None,
            backend: None,
        })
        .unwrap();
        let (status, bytes) = send(app, "POST", "/api/v1/downloads", body).await;
        assert_eq!(status, StatusCode::OK);
        serde_json::from_slice::<AddDownloadResponse>(&bytes)
            .unwrap()
            .id
    }

    #[tokio::test]
    async fn default_segments_setting_used() {
        let dir = tempdir().unwrap();
        let state = state_in(dir.path());
        let app = build_router_with_state(state.clone());
        send(
            &app,
            "PATCH",
            "/api/v1/settings",
            serde_json::json!({"default_segments": 3}),
        )
        .await;

        let id = add(&app, "http://127.0.0.1:9/never".into(), dir.path(), None).await;

        let job = state.engine.store().get_job(JobId(id)).unwrap();
        assert_eq!(job.options().segments, Some(3));
    }

    #[tokio::test]
    async fn default_retry_count_setting_used() {
        let dir = tempdir().unwrap();
        let state = state_in(dir.path());
        let app = build_router_with_state(state.clone());
        send(
            &app,
            "PATCH",
            "/api/v1/settings",
            serde_json::json!({"default_retry_count": 2}),
        )
        .await;

        let id = add(&app, "http://127.0.0.1:9/never".into(), dir.path(), None).await;

        let job = state.engine.store().get_job(JobId(id)).unwrap();
        assert_eq!(job.options().retry_count, Some(2));
    }

    #[tokio::test]
    async fn global_limit_setting_slows_download() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let origin = Router::new().route(
            "/slow.bin",
            get(|| async {
                let body = vec![b's'; 128 * 1024];
                (
                    StatusCode::OK,
                    [
                        (CONTENT_TYPE, "application/octet-stream"),
                        (CONTENT_LENGTH, "131072"),
                    ],
                    body,
                )
                    .into_response()
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, origin).await.unwrap();
        });

        let dir = tempdir().unwrap();
        let state = state_in(dir.path());
        let app = build_router_with_state(state.clone());
        send(
            &app,
            "PATCH",
            "/api/v1/settings",
            serde_json::json!({"global_speed_limit": 65536}),
        )
        .await;

        let started = std::time::Instant::now();
        let id = add(&app, format!("http://{addr}/slow.bin"), dir.path(), None).await;
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                let done = state
                    .engine
                    .store()
                    .get_job(JobId(id))
                    .is_ok_and(|job| job.state() == JobState::Completed);
                if done {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("download should complete");

        // 128 KiB at 64 KiB/s cannot finish in under about two seconds.
        assert!(started.elapsed() >= std::time::Duration::from_millis(1500));
    }

    #[tokio::test]
    async fn queue_limit_change_takes_effect_without_restart() {
        let dir = tempdir().unwrap();
        let state = state_in(dir.path());
        state.engine.store().create_queue("bulk", 1).unwrap();
        let semaphore = state.get_semaphore("bulk", 1);
        let app = build_router_with_state(state);
        assert_eq!(semaphore.available_permits(), 1);

        let (status, _) = send(
            &app,
            "PATCH",
            "/api/v1/queues/bulk",
            serde_json::json!({"max_concurrent": 3}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(semaphore.available_permits(), 3);

        send(
            &app,
            "PATCH",
            "/api/v1/queues/bulk",
            serde_json::json!({"max_concurrent": 1}),
        )
        .await;
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while semaphore.available_permits() != 1 {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("a lower limit must retire the extra slots");
    }
}
