//! Tests for DASH backend probing, quality querying, segment downloading, pause/resume, cancellation, and DRM detection.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use odm_core::backend::{Backend, ControlToken, JobContext, ProgressSink, RunOutcome};
use odm_core::job::{BackendKind, Job, JobOptions, Source};
use odm_core::limiter::{Limiter, TokenBucket};
use odm_core::store::Store;
use odm_media::dash::DashBackend;
use odm_media::state::DashResumeState;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::Notify;

const SEGMENT_SIZE: usize = 4096;

#[cfg(windows)]
fn make_fake_ffmpeg(dir: &std::path::Path) -> PathBuf {
    let script_path = dir.join("fake_ffmpeg.cmd");
    let script = "@echo off\r\n\
                  set \"last=\"\r\n\
                  for %%x in (%*) do set \"last=%%x\"\r\n\
                  if not \"%~5\"==\"\" (\r\n\
                      copy /b \"%~3\"+\"%~5\" \"%last%\" >nul\r\n\
                  ) else (\r\n\
                      copy /b \"%~3\" \"%last%\" >nul\r\n\
                  )\r\n\
                  exit /b 0\r\n";
    std::fs::write(&script_path, script).unwrap();
    script_path
}

#[cfg(not(windows))]
fn make_fake_ffmpeg(dir: &std::path::Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script_path = dir.join("fake_ffmpeg.sh");
    let script = "#!/bin/bash\n\
                  out=\"${@: -1}\"\n\
                  if [ -n \"$5\" ] && [ \"$5\" != \"-c\" ]; then\n\
                      cat \"$3\" \"$5\" > \"$out\"\n\
                  else\n\
                      cat \"$3\" > \"$out\"\n\
                  fi\n\
                  exit 0\n";
    std::fs::write(&script_path, script).unwrap();
    let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms).unwrap();
    script_path
}

struct ServerState {
    video_init: Vec<u8>,
    video_seg1: Vec<u8>,
    video_seg2: Vec<u8>,
    audio_init: Vec<u8>,
    audio_seg1: Vec<u8>,
    audio_seg2: Vec<u8>,
    video_seg2_requested: Arc<Notify>,
    video_seg2_fetch_count: AtomicUsize,
}

struct TestServer {
    addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
    state: Arc<ServerState>,
}

impl TestServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let state = Arc::new(ServerState {
            video_init: vec![0x11; SEGMENT_SIZE],
            video_seg1: vec![0x22; SEGMENT_SIZE],
            video_seg2: vec![0x33; SEGMENT_SIZE],
            audio_init: vec![0x44; SEGMENT_SIZE],
            audio_seg1: vec![0x55; SEGMENT_SIZE],
            audio_seg2: vec![0x66; SEGMENT_SIZE],
            video_seg2_requested: Arc::new(Notify::new()),
            video_seg2_fetch_count: AtomicUsize::new(0),
        });

        let app = Router::new()
            .route("/manifest.mpd", get(manifest_mpd))
            .route("/drm.mpd", get(drm_mpd))
            .route("/video/init.mp4", get(get_video_init))
            .route("/video/seg-1.m4s", get(get_video_seg1))
            .route("/video/seg-2.m4s", get(get_video_seg2))
            .route("/audio/init.mp4", get(get_audio_init))
            .route("/audio/seg-1.m4s", get(get_audio_seg1))
            .route("/audio/seg-2.m4s", get(get_audio_seg2))
            .with_state(state.clone());

        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self {
            addr,
            handle,
            state,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn manifest_mpd() -> Response {
    let body = r#"<?xml version="1.0" encoding="utf-8"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011"
     profiles="urn:mpeg:dash:profile:isoff-live:2011"
     type="static"
     mediaPresentationDuration="PT20S"
     minBufferTime="PT2S">
  <Period id="0" duration="PT20S">
    <AdaptationSet id="0" contentType="video" mimeType="video/mp4" width="1280" height="720">
      <SegmentTemplate initialization="/video/init.mp4"
                       media="/video/seg-$Number$.m4s"
                       startNumber="1"
                       timescale="1000"
                       duration="10000" />
      <Representation id="720p" bandwidth="2000000" width="1280" height="720" codecs="avc1.64001f" />
    </AdaptationSet>
    <AdaptationSet id="1" contentType="audio" mimeType="audio/mp4">
      <SegmentTemplate initialization="/audio/init.mp4"
                       media="/audio/seg-$Number$.m4s"
                       startNumber="1"
                       timescale="1000"
                       duration="10000" />
      <Representation id="audio" bandwidth="128000" codecs="mp4a.40.2" />
    </AdaptationSet>
  </Period>
</MPD>"#;
    ([(CONTENT_TYPE, "application/dash+xml")], body.to_string()).into_response()
}

async fn drm_mpd() -> Response {
    let body = r#"<?xml version="1.0" encoding="utf-8"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011"
     xmlns:cenc="urn:mpeg:cenc:2013"
     profiles="urn:mpeg:dash:profile:isoff-live:2011"
     type="static"
     mediaPresentationDuration="PT20S"
     minBufferTime="PT2S">
  <Period id="0" duration="PT20S">
    <AdaptationSet id="0" contentType="video" mimeType="video/mp4" width="1280" height="720">
      <ContentProtection schemeIdUri="urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed" />
      <SegmentTemplate initialization="/video/init.mp4"
                       media="/video/seg-$Number$.m4s"
                       startNumber="1"
                       timescale="1000"
                       duration="10000" />
      <Representation id="720p" bandwidth="2000000" width="1280" height="720" codecs="avc1.64001f" />
    </AdaptationSet>
  </Period>
</MPD>"#;
    ([(CONTENT_TYPE, "application/dash+xml")], body.to_string()).into_response()
}

async fn get_video_init(State(state): State<Arc<ServerState>>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
    (
        StatusCode::OK,
        headers,
        Bytes::from(state.video_init.clone()),
    )
        .into_response()
}

async fn get_video_seg1(State(state): State<Arc<ServerState>>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
    (
        StatusCode::OK,
        headers,
        Bytes::from(state.video_seg1.clone()),
    )
        .into_response()
}

async fn get_video_seg2(State(state): State<Arc<ServerState>>) -> Response {
    state.video_seg2_fetch_count.fetch_add(1, Ordering::SeqCst);
    state.video_seg2_requested.notify_waiters();
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
    (
        StatusCode::OK,
        headers,
        Bytes::from(state.video_seg2.clone()),
    )
        .into_response()
}

async fn get_audio_init(State(state): State<Arc<ServerState>>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("audio/mp4"));
    (
        StatusCode::OK,
        headers,
        Bytes::from(state.audio_init.clone()),
    )
        .into_response()
}

async fn get_audio_seg1(State(state): State<Arc<ServerState>>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("audio/mp4"));
    (
        StatusCode::OK,
        headers,
        Bytes::from(state.audio_seg1.clone()),
    )
        .into_response()
}

async fn get_audio_seg2(State(state): State<Arc<ServerState>>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("audio/mp4"));
    (
        StatusCode::OK,
        headers,
        Bytes::from(state.audio_seg2.clone()),
    )
        .into_response()
}

#[tokio::test]
async fn probe_mpd_returns_dash_and_qualities() {
    let server = TestServer::start().await;
    let temp_dir = TempDir::new().unwrap();
    let fake_ffmpeg = make_fake_ffmpeg(temp_dir.path());
    let backend = DashBackend::new(reqwest::Client::new(), Some(fake_ffmpeg));
    let source = Source::Url(server.url("/manifest.mpd"));

    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();

    assert_eq!(probe.backend, BackendKind::Dash);
    assert!(!probe.filename.is_empty());

    let qualities = backend.probe_qualities(&source).await.unwrap();
    assert_eq!(qualities.len(), 1);

    let q720 = qualities
        .iter()
        .find(|q| q.id == "720p" || q.resolution.as_deref() == Some("1280x720"))
        .expect("720p quality rendition must be present");
    assert_eq!(q720.bandwidth, Some(2000000));
}

#[tokio::test]
async fn probe_without_ffmpeg_fails() {
    let server = TestServer::start().await;
    let non_existent = PathBuf::from("/nonexistent/path/to/ffmpeg_abc_123");
    let backend = DashBackend::new(reqwest::Client::new(), Some(non_existent));
    let source = Source::Url(server.url("/manifest.mpd"));

    let err = backend.probe(&source, &JobOptions::default()).await;
    assert!(err.is_err(), "probe must fail when ffmpeg is missing");
    let msg = err.err().unwrap().to_string();
    assert!(
        msg.contains("ffmpeg is required"),
        "error message must mention ffmpeg is required, got: {msg}"
    );
}

#[tokio::test]
async fn probe_drm_protected_fails() {
    let server = TestServer::start().await;
    let temp_dir = TempDir::new().unwrap();
    let fake_ffmpeg = make_fake_ffmpeg(temp_dir.path());
    let backend = DashBackend::new(reqwest::Client::new(), Some(fake_ffmpeg));
    let source = Source::Url(server.url("/drm.mpd"));

    let err = backend.probe(&source, &JobOptions::default()).await;
    assert!(err.is_err(), "probe must fail for DRM protected stream");
    let msg = err.err().unwrap().to_string();
    assert!(
        msg.contains("DRM") || msg.contains("not supported"),
        "error message must mention DRM or not supported, got: {msg}"
    );
}

#[tokio::test]
async fn download_dash_stream_success() {
    let server = TestServer::start().await;
    let temp_dir = TempDir::new().unwrap();
    let fake_ffmpeg = make_fake_ffmpeg(temp_dir.path());
    let backend = DashBackend::new(reqwest::Client::new(), Some(fake_ffmpeg));
    let source = Source::Url(server.url("/manifest.mpd"));

    let mut job = Job::new(
        source.clone(),
        temp_dir.path().to_path_buf(),
        JobOptions::default(),
    );
    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();
    job.apply_probe(probe);

    let store = Store::open_in_memory().unwrap();
    store.insert_job(&mut job).unwrap();

    let (_control_handle, control) = ControlToken::new();
    let (progress, mut progress_rx) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job: job.clone(),
        control,
        progress,
        store: store.clone(),
        limiter,
    };

    let outcome = backend.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Completed);

    let target_path = job.target_path();
    assert!(
        target_path.exists(),
        "Target path {} does not exist",
        target_path.display()
    );

    let downloaded_data = std::fs::read(&target_path).unwrap();
    assert!(
        !downloaded_data.is_empty(),
        "Downloaded output must not be empty"
    );

    // Temp segment files must be cleaned up
    let mut v_tmp = job.part_path().into_os_string();
    v_tmp.push(".v.tmp");
    assert!(!PathBuf::from(v_tmp).exists(), "video tmp must be deleted");

    let mut a_tmp = job.part_path().into_os_string();
    a_tmp.push(".a.tmp");
    assert!(!PathBuf::from(a_tmp).exists(), "audio tmp must be deleted");

    let mut total_reported = 0;
    while let Ok(update) = progress_rx.try_recv() {
        total_reported = update.downloaded_bytes;
    }
    assert!(total_reported > 0, "Progress must have been reported");
}

#[tokio::test]
async fn pause_and_resume_dash_download() {
    let server = TestServer::start().await;
    let temp_dir = TempDir::new().unwrap();
    let fake_ffmpeg = make_fake_ffmpeg(temp_dir.path());
    let backend = Arc::new(DashBackend::new(reqwest::Client::new(), Some(fake_ffmpeg)));
    let source = Source::Url(server.url("/manifest.mpd"));

    let mut job = Job::new(
        source.clone(),
        temp_dir.path().to_path_buf(),
        JobOptions::default(),
    );
    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();
    job.apply_probe(probe);

    let store = Store::open_in_memory().unwrap();
    store.insert_job(&mut job).unwrap();
    let job_id = job.id();

    let (control_handle, control) = ControlToken::new();
    let (progress, _) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job: job.clone(),
        control,
        progress,
        store: store.clone(),
        limiter: limiter.clone(),
    };

    // Trigger pause when video_seg2 is requested
    let seg2_notif = server.state.video_seg2_requested.clone();
    let handle_clone = control_handle.clone();
    tokio::spawn(async move {
        seg2_notif.notified().await;
        handle_clone.pause();
    });

    let backend_clone = backend.clone();
    let outcome = backend_clone.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Paused);

    // Verify resume state saved in store
    let state_json = store
        .get_backend_state(job_id)
        .unwrap()
        .expect("backend state must be present after pause");
    let resume_state: DashResumeState = serde_json::from_str(&state_json).unwrap();
    assert!(
        !resume_state.completed_video_segments.is_empty(),
        "At least one video segment must be completed before pause"
    );

    // Resume download
    let (_resume_handle, resume_control) = ControlToken::new();
    let (resume_progress, _) = ProgressSink::new();
    let mut resumed_job = store.get_job(job_id).unwrap();
    resumed_job.set_backend_state(Some(state_json));

    let resume_ctx = JobContext {
        job: resumed_job.clone(),
        control: resume_control,
        progress: resume_progress,
        store: store.clone(),
        limiter,
    };

    let resume_outcome = backend.run(resume_ctx).await.unwrap();
    assert_eq!(resume_outcome, RunOutcome::Completed);

    let target_path = resumed_job.target_path();
    assert!(
        target_path.exists(),
        "Resumed target path {} must exist",
        target_path.display()
    );
}

#[tokio::test]
async fn cancel_dash_download() {
    let server = TestServer::start().await;
    let temp_dir = TempDir::new().unwrap();
    let fake_ffmpeg = make_fake_ffmpeg(temp_dir.path());
    let backend = DashBackend::new(reqwest::Client::new(), Some(fake_ffmpeg));
    let source = Source::Url(server.url("/manifest.mpd"));

    let mut job = Job::new(
        source.clone(),
        temp_dir.path().to_path_buf(),
        JobOptions::default(),
    );
    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();
    job.apply_probe(probe);

    let store = Store::open_in_memory().unwrap();
    store.insert_job(&mut job).unwrap();

    let (control_handle, control) = ControlToken::new();
    control_handle.cancel();

    let (progress, _) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job,
        control,
        progress,
        store,
        limiter,
    };

    let outcome = backend.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Cancelled);
}

async fn serve(app: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

fn mpd_response(body: String) -> Response {
    ([(CONTENT_TYPE, "application/dash+xml")], body).into_response()
}

/// Probes `url` and returns a backend, a stored job and its store.
async fn prepare(url: &str, dir: &std::path::Path) -> (DashBackend, Job, Store) {
    let backend = DashBackend::new(reqwest::Client::new(), Some(make_fake_ffmpeg(dir)));
    let source = Source::Url(url.to_string());
    let mut job = Job::new(source.clone(), dir.to_path_buf(), JobOptions::default());
    job.apply_probe(
        backend
            .probe(&source, &JobOptions::default())
            .await
            .unwrap(),
    );
    let store = Store::open_in_memory().unwrap();
    store.insert_job(&mut job).unwrap();
    (backend, job, store)
}

async fn execute(backend: &DashBackend, job: &Job, store: Store) -> RunOutcome {
    let (_control_handle, control) = ControlToken::new();
    let (progress, _progress_rx) = ProgressSink::new();
    backend
        .run(JobContext {
            job: job.clone(),
            control,
            progress,
            store,
            limiter: Limiter::new(Arc::new(TokenBucket::new(0)), None),
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn dash_number_template_with_root_duration_downloads_all_segments() {
    let requested = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = requested.clone();
    let addr = serve(
        Router::new()
            .route(
                "/m.mpd",
                get(|| async {
                    // The duration is only on the root element, not on the period.
                    mpd_response(
                        r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT8S">
  <Period>
    <AdaptationSet contentType="video" mimeType="video/mp4" width="640" height="360">
      <Representation id="v" bandwidth="1000">
        <SegmentTemplate media="/s-$Number$.m4s" duration="2" timescale="1" startNumber="1"/>
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>"#
                            .to_string(),
                    )
                }),
            )
            .route(
                "/{name}",
                get(
                    move |axum::extract::Path(name): axum::extract::Path<String>| {
                        let seen = seen.clone();
                        async move {
                            seen.lock().unwrap().push(name);
                            Bytes::from(vec![0x5A; 100])
                        }
                    },
                ),
            ),
    )
    .await;
    let temp_dir = TempDir::new().unwrap();
    let (backend, job, store) = prepare(&format!("http://{addr}/m.mpd"), temp_dir.path()).await;

    let outcome = execute(&backend, &job, store).await;

    assert_eq!(outcome, RunOutcome::Completed);
    let mut names = requested.lock().unwrap().clone();
    names.sort();
    assert_eq!(names, ["s-1.m4s", "s-2.m4s", "s-3.m4s", "s-4.m4s"]);
    assert_eq!(std::fs::read(job.target_path()).unwrap().len(), 400);
}

#[tokio::test]
async fn dash_segment_base_streams_to_disk() {
    const TOTAL: usize = 3 * 1024 * 1024;
    const FIRST: usize = 1024 * 1024;
    let temp_dir = TempDir::new().unwrap();
    let video_tmp = Arc::new(std::sync::Mutex::new(PathBuf::new()));
    let written_early = Arc::new(AtomicBool::new(false));

    // A file server that sends 1 MiB, then waits to see the client write it to
    // disk before it sends the rest. A client that buffers the whole body
    // never writes anything in between.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let file_addr = listener.local_addr().unwrap();
    let (tmp_for_server, early_for_server) = (video_tmp.clone(), written_early.clone());
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        while let Ok((mut socket, _)) = listener.accept().await {
            let (tmp, early) = (tmp_for_server.clone(), early_for_server.clone());
            tokio::spawn(async move {
                let mut request = vec![0_u8; 4096];
                let _ = socket.read(&mut request).await;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {TOTAL}\r\nConnection: close\r\n\r\n"
                );
                socket.write_all(head.as_bytes()).await.unwrap();
                socket.write_all(&vec![7_u8; FIRST]).await.unwrap();
                socket.flush().await.unwrap();
                let path = tmp.lock().unwrap().clone();
                for _ in 0..300 {
                    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > 0) {
                        early.store(true, Ordering::SeqCst);
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                let _ = socket.write_all(&vec![7_u8; TOTAL - FIRST]).await;
            });
        }
    });
    let manifest = format!(
        r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT10S">
  <Period>
    <AdaptationSet contentType="video" mimeType="video/mp4" width="640" height="360">
      <Representation id="v" bandwidth="1000">
        <BaseURL>http://{file_addr}/big.mp4</BaseURL>
        <SegmentBase indexRange="0-100"/>
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>"#
    );
    let addr = serve(Router::new().route(
        "/m.mpd",
        get(move || {
            let manifest = manifest.clone();
            async move { mpd_response(manifest) }
        }),
    ))
    .await;
    let (backend, job, store) = prepare(&format!("http://{addr}/m.mpd"), temp_dir.path()).await;
    let mut tmp_name = job.part_path().into_os_string();
    tmp_name.push(".v.tmp");
    *video_tmp.lock().unwrap() = PathBuf::from(tmp_name);

    let outcome = execute(&backend, &job, store).await;

    assert_eq!(outcome, RunOutcome::Completed);
    assert!(
        written_early.load(Ordering::SeqCst),
        "the segment was held in memory instead of being written as it arrived"
    );
    assert_eq!(std::fs::read(job.target_path()).unwrap().len(), TOTAL);
}

#[tokio::test]
async fn dash_live_manifest_is_rejected() {
    let addr = serve(Router::new().route(
        "/live.mpd",
        get(|| async {
            mpd_response(
                r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="dynamic" minimumUpdatePeriod="PT2S">
  <Period>
    <AdaptationSet contentType="video" mimeType="video/mp4">
      <Representation id="v" bandwidth="1000">
        <SegmentTemplate media="/s-$Number$.m4s" duration="2" timescale="1"/>
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>"#
                    .to_string(),
            )
        }),
    ))
    .await;
    let temp_dir = TempDir::new().unwrap();
    let backend = DashBackend::new(
        reqwest::Client::new(),
        Some(make_fake_ffmpeg(temp_dir.path())),
    );

    let error = backend
        .probe(
            &Source::Url(format!("http://{addr}/live.mpd")),
            &JobOptions::default(),
        )
        .await
        .unwrap_err();

    assert!(
        error.to_string().contains("live streams are not supported"),
        "got: {error}"
    );
}
