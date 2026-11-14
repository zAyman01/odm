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

mod queue_order {
    use super::sample_job;
    use axum::http::StatusCode;
    use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, HOST};
    use odm_core::{Engine, Event, JobId, JobState, Store};
    use odm_daemon::config::DaemonConfig;
    use odm_daemon::server::build_router_with_state;
    use odm_daemon::state::AppState;
    use tempfile::tempdir;
    use tower::ServiceExt;

    async fn send(
        app: &axum::Router,
        method: &str,
        uri: &str,
        body: serde_json::Value,
    ) -> StatusCode {
        let req = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header(HOST, "127.0.0.1")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        app.clone().oneshot(req).await.unwrap().status()
    }

    fn queue_changed_names(rx: &mut tokio::sync::broadcast::Receiver<Event>) -> Vec<String> {
        let mut names = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let Event::QueueChanged(name) = event {
                names.push(name);
            }
        }
        names
    }

    #[tokio::test]
    async fn position_route_reorders_and_emits_queue_changed() {
        let dir = tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let mut first = sample_job(dir.path());
        let mut second = sample_job(dir.path());
        store.insert_job(&mut first).unwrap();
        store.insert_job(&mut second).unwrap();
        let engine = Engine::new(store.clone()).unwrap();
        let mut events = engine.subscribe();
        let state = AppState::new(engine, DaemonConfig::for_test(dir.path(), "secret"));
        let app = build_router_with_state(state);

        let status = send(
            &app,
            "PUT",
            &format!("/api/v1/downloads/{}/position", second.id().0),
            serde_json::json!({"position": 0}),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(store.get_job(second.id()).unwrap().position(), 0);
        assert_eq!(queue_changed_names(&mut events), vec!["default"]);
        let missing = send(
            &app,
            "PUT",
            "/api/v1/downloads/999/position",
            serde_json::json!({"position": 1}),
        )
        .await;
        assert_eq!(missing, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn queue_limit_change_and_delete_emit_queue_changed() {
        let dir = tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        store.create_queue("bulk", 1).unwrap();
        let engine = Engine::new(store).unwrap();
        let mut events = engine.subscribe();
        let state = AppState::new(engine, DaemonConfig::for_test(dir.path(), "secret"));
        let app = build_router_with_state(state);

        send(
            &app,
            "PATCH",
            "/api/v1/queues/bulk",
            serde_json::json!({"max_concurrent": 2}),
        )
        .await;
        assert_eq!(queue_changed_names(&mut events), vec!["bulk"]);

        let status = send(&app, "DELETE", "/api/v1/queues/bulk", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(queue_changed_names(&mut events), vec!["bulk"]);
    }

    #[tokio::test]
    async fn queued_jobs_start_in_position_order() {
        let dir = tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let mut ids = Vec::new();
        for position in [2, 0, 1] {
            let mut job = sample_job(dir.path());
            store.insert_job(&mut job).unwrap();
            store.set_job_position(job.id(), position).unwrap();
            ids.push(job.id());
        }
        let engine = Engine::new(store).unwrap();
        let mut events = engine.subscribe();
        let state = AppState::new(engine, DaemonConfig::for_test(dir.path(), "secret"));

        // One slot, held while all three jobs wait, so only the order decides.
        let held = state
            .get_semaphore("default", 1)
            .acquire_owned()
            .await
            .unwrap();
        for id in &ids {
            state.spawn_download(*id, "default", 1);
        }
        drop(held);

        let mut started: Vec<JobId> = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            while started.len() < 3 {
                if let Ok(Event::JobStateChanged { id, state }) = events.recv().await
                    && state != JobState::Queued
                    && !started.contains(&id)
                {
                    started.push(id);
                }
            }
        })
        .await
        .expect("all three jobs should start");

        // Jobs were added at positions 2, 0, 1, so they start 2nd, 3rd, 1st.
        assert_eq!(started, vec![ids[1], ids[2], ids[0]]);
    }
}

mod schedule_windows {
    use super::{MockClock, sample_job};
    use odm_core::schedule::QueueSchedule;
    use odm_core::{Engine, JobState, Store};
    use odm_daemon::config::DaemonConfig;
    use odm_daemon::scheduler::Scheduler;
    use odm_daemon::state::AppState;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn night_schedule() -> QueueSchedule {
        QueueSchedule::new("22:00", Some("04:00".to_string()), vec!["mon".to_string()])
    }

    #[test]
    fn window_covers_the_night_that_starts_on_a_listed_day() {
        let schedule = night_schedule();

        assert!(schedule.is_active_at("mon", "22:00"));
        assert!(schedule.is_active_at("mon", "23:59"));
        assert!(schedule.is_active_at("tue", "03:59"));
        assert!(!schedule.is_active_at("tue", "04:00"));
        assert!(!schedule.is_active_at("mon", "12:00"));
        assert!(!schedule.is_active_at("tue", "23:00"));
        assert!(!schedule.is_active_at("mon", "03:00"));

        // The stop of a night that starts on Monday falls on Tuesday morning.
        assert!(schedule.should_stop("tue", "04:00"));
        assert!(!schedule.should_stop("mon", "04:00"));
    }

    #[test]
    fn same_day_window_and_open_ended_schedule() {
        let day = QueueSchedule::new("08:00", Some("17:00".to_string()), vec![]);
        assert!(day.is_active_at("wed", "08:00"));
        assert!(day.is_active_at("wed", "16:59"));
        assert!(!day.is_active_at("wed", "17:00"));
        assert!(!day.is_active_at("wed", "07:59"));

        let open = QueueSchedule::new("08:00", None, vec!["fri".to_string()]);
        assert!(open.is_active_at("fri", "23:30"));
        assert!(!open.is_active_at("fri", "07:00"));
        assert!(!open.is_active_at("sat", "09:00"));
    }

    fn state_with_queue(dir: &std::path::Path, clock: &Arc<MockClock>) -> (AppState, i64) {
        let store = Store::open_in_memory().unwrap();
        let queue = store
            .create_queue_with_schedule("nightly", 1, Some(&night_schedule()))
            .unwrap();
        let engine = Engine::new(store).unwrap();
        let state =
            AppState::new(engine, DaemonConfig::for_test(dir, "secret")).with_clock(clock.clone());
        (state, queue.id)
    }

    #[tokio::test]
    async fn job_added_outside_window_stays_queued() {
        let dir = tempdir().unwrap();
        let clock = Arc::new(MockClock::new("mon", "12:00"));
        let (state, queue_id) = state_with_queue(dir.path(), &clock);
        let mut job = sample_job(dir.path());
        job.set_queue_id(queue_id);
        state.engine.store().insert_job(&mut job).unwrap();

        state.spawn_download(job.id(), "nightly", 1);
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        assert_eq!(
            state.engine.store().get_job(job.id()).unwrap().state(),
            JobState::Queued
        );
    }

    #[tokio::test]
    async fn job_added_inside_window_starts() {
        let dir = tempdir().unwrap();
        let clock = Arc::new(MockClock::new("mon", "23:00"));
        let (state, queue_id) = state_with_queue(dir.path(), &clock);
        let mut job = sample_job(dir.path());
        job.set_queue_id(queue_id);
        state.engine.store().insert_job(&mut job).unwrap();

        state.spawn_download(job.id(), "nightly", 1);
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        assert_ne!(
            state.engine.store().get_job(job.id()).unwrap().state(),
            JobState::Queued
        );
    }

    #[tokio::test]
    async fn at_stop_time_waiting_jobs_do_not_start_when_running_ones_pause() {
        let dir = tempdir().unwrap();
        let clock = Arc::new(MockClock::new("mon", "23:00"));
        let (state, queue_id) = state_with_queue(dir.path(), &clock);
        let mut waiting = sample_job(dir.path());
        waiting.set_queue_id(queue_id);
        state.engine.store().insert_job(&mut waiting).unwrap();

        // The only slot is busy, as if a running job held it.
        let busy = state
            .get_semaphore("nightly", 1)
            .acquire_owned()
            .await
            .unwrap();
        state.spawn_download(waiting.id(), "nightly", 1);
        clock.set("tue", "04:00");
        drop(busy);
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        assert_eq!(
            state.engine.store().get_job(waiting.id()).unwrap().state(),
            JobState::Queued
        );
    }

    #[tokio::test]
    async fn scheduler_resumes_only_the_jobs_it_paused() {
        let dir = tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let queue = store
            .create_queue_with_schedule("nightly", 2, Some(&night_schedule()))
            .unwrap();
        let mut by_schedule = sample_job(dir.path());
        let mut by_user = sample_job(dir.path());
        for job in [&mut by_schedule, &mut by_user] {
            job.set_queue_id(queue.id);
            store.insert_job(job).unwrap();
        }
        let engine = Engine::new(store).unwrap();
        by_schedule.transition_to(JobState::Probing).unwrap();
        by_schedule.transition_to(JobState::Downloading).unwrap();
        engine.store().update_job(&by_schedule).unwrap();
        by_user.transition_to(JobState::Paused).unwrap();
        engine.store().update_job(&by_user).unwrap();

        let clock = Arc::new(MockClock::new("tue", "04:00"));
        let scheduler = Scheduler::new_with_clock(engine.clone(), clock.clone());
        scheduler.tick().await;
        assert_eq!(
            engine.store().get_job(by_schedule.id()).unwrap().state(),
            JobState::Paused
        );

        clock.set("mon", "22:00");
        scheduler.tick().await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        assert_ne!(
            engine.store().get_job(by_schedule.id()).unwrap().state(),
            JobState::Paused
        );
        assert_eq!(
            engine.store().get_job(by_user.id()).unwrap().state(),
            JobState::Paused
        );
    }
}
