#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use odm_core::backend::{
    Backend, BackendError, ControlToken, JobContext, ProgressSink, RunOutcome,
};
use odm_core::job::{BackendKind, Job, JobOptions, Source};
use odm_core::limiter::{Limiter, TokenBucket};
use odm_core::store::Store;
use odm_media::state::YtDlpResumeState;
use odm_media::ytdlp::YtDlpBackend;
use tempfile::TempDir;

fn build_fake_ytdlp(dir: &Path) -> PathBuf {
    let source_path = dir.join("fake_ytdlp_source.rs");
    let out_binary = if cfg!(windows) {
        dir.join("fake_ytdlp.exe")
    } else {
        dir.join("fake_ytdlp")
    };
    let code = r##"
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let joined = args.join(" ");

    // Probe mode: -J is present
    if joined.contains("-J") {
        if joined.contains("bad-url") {
            eprintln!("ERROR: Video unavailable");
            std::process::exit(1);
        }
        println!("{}", r#"{"title": "Test Video", "id": "123", "ext": "mp4", "filesize": 10485760, "formats": [{"format_id": "22", "resolution": "1280x720", "ext": "mp4", "tbr": 1500, "vcodec": "avc1", "acodec": "mp4a"}]}"#);
        std::process::exit(0);
    }

    // Download mode: find -o <path>
    for i in 0..args.len() {
        if args[i] == "-o" && i + 1 < args.len() {
            let path = &args[i + 1];
            let _ = std::fs::write(path, b"dummy video content");
        }
    }

    if joined.contains("hang") {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    if joined.contains("malformed") {
        println!("garbage line 1");
        println!("garbage line 2");
        std::process::exit(0);
    }

    if joined.contains("pause") || joined.contains("cancel") {
        println!("download: 50.0% 10485760 2097152");
        let _ = std::io::stdout().flush();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    println!("download: 50.0% 10485760 2097152");
    let _ = std::io::stdout().flush();
    std::thread::sleep(std::time::Duration::from_millis(10));
    println!("download: 100.0% 10485760 2097152");
    let _ = std::io::stdout().flush();
}
"##;
    std::fs::write(&source_path, code).unwrap();
    let status = std::process::Command::new("rustc")
        .arg(&source_path)
        .arg("-o")
        .arg(&out_binary)
        .status()
        .expect("failed to compile fake_ytdlp");
    assert!(status.success());
    out_binary
}

#[tokio::test]
async fn test_probe_success() {
    let temp_dir = TempDir::new().unwrap();
    let fake_ytdlp = build_fake_ytdlp(temp_dir.path());
    let backend = YtDlpBackend::new(fake_ytdlp);
    let source = Source::Url("https://example.com/watch?v=123".to_string());

    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();

    assert_eq!(probe.backend, BackendKind::YtDlp);
    assert_eq!(probe.filename, "Test Video.mp4");
    assert_eq!(probe.total_bytes, Some(10485760));

    let qualities = backend.probe_qualities(&source).await.unwrap();
    assert_eq!(qualities.len(), 1);
    assert_eq!(qualities[0].id, "22");
    assert_eq!(qualities[0].resolution.as_deref(), Some("1280x720"));
}

#[tokio::test]
async fn test_probe_not_found() {
    let non_existent = PathBuf::from("/nonexistent/path/to/ytdlp_xyz_98765");
    let backend = YtDlpBackend::new(non_existent);
    let source = Source::Url("https://example.com/watch?v=123".to_string());

    let err = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap_err();
    match err {
        BackendError::YtDlp(msg) => {
            assert!(
                msg.contains("yt-dlp not found"),
                "expected 'yt-dlp not found', got {msg}"
            );
        }
        other => panic!("expected BackendError::YtDlp, got {other:?}"),
    }
}

#[tokio::test]
async fn test_probe_failure_exit_code() {
    let temp_dir = TempDir::new().unwrap();
    let fake_ytdlp = build_fake_ytdlp(temp_dir.path());
    let backend = YtDlpBackend::new(fake_ytdlp);
    let source = Source::Url("https://example.com/bad-url".to_string());

    let err = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap_err();
    match err {
        BackendError::YtDlp(msg) => {
            assert!(
                msg.contains("Video unavailable"),
                "expected 'Video unavailable', got {msg}"
            );
        }
        other => panic!("expected BackendError::YtDlp, got {other:?}"),
    }
}

#[tokio::test]
async fn test_download_success() {
    let temp_dir = TempDir::new().unwrap();
    let fake_ytdlp = build_fake_ytdlp(temp_dir.path());
    let backend = YtDlpBackend::new(fake_ytdlp);
    let source = Source::Url("https://example.com/watch?v=123".to_string());

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
        store,
        limiter,
    };

    let outcome = backend.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Completed);

    let target_path = job.target_path();
    assert!(target_path.exists(), "Target file must exist");
    let content = std::fs::read(&target_path).unwrap();
    assert_eq!(content, b"dummy video content");

    let mut total_reported = 0;
    while let Ok(update) = progress_rx.try_recv() {
        total_reported = update.downloaded_bytes;
    }
    assert!(total_reported > 0, "Progress must have been reported");
}

#[tokio::test]
async fn test_pause_download() {
    let temp_dir = TempDir::new().unwrap();
    let fake_ytdlp = build_fake_ytdlp(temp_dir.path());
    let backend = Arc::new(YtDlpBackend::new(fake_ytdlp));
    let source = Source::Url("https://example.com/pause".to_string());

    let mut job = Job::new(
        source.clone(),
        temp_dir.path().to_path_buf(),
        JobOptions {
            quality: Some("22".to_string()),
            ..JobOptions::default()
        },
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
    let (progress, mut progress_rx) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job: job.clone(),
        control,
        progress,
        store: store.clone(),
        limiter,
    };

    // Pause after progress update received
    let handle_clone = control_handle.clone();
    tokio::spawn(async move {
        let _ = progress_rx.recv().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        handle_clone.pause();
    });

    let outcome = backend.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Paused);

    let state_str = store.get_backend_state(job_id).unwrap().unwrap();
    let state: YtDlpResumeState = serde_json::from_str(&state_str).unwrap();
    assert_eq!(state.format_id.as_deref(), Some("22"));
}

#[tokio::test]
async fn test_cancel_download() {
    let temp_dir = TempDir::new().unwrap();
    let fake_ytdlp = build_fake_ytdlp(temp_dir.path());
    let backend = Arc::new(YtDlpBackend::new(fake_ytdlp));
    let source = Source::Url("https://example.com/cancel".to_string());

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
    let (progress, mut progress_rx) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job: job.clone(),
        control,
        progress,
        store,
        limiter,
    };

    let handle_clone = control_handle.clone();
    tokio::spawn(async move {
        let _ = progress_rx.recv().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        handle_clone.cancel();
    });

    let outcome = backend.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Cancelled);
}

#[tokio::test]
async fn test_malformed_output_handled_gracefully() {
    let temp_dir = TempDir::new().unwrap();
    let fake_ytdlp = build_fake_ytdlp(temp_dir.path());
    let backend = YtDlpBackend::new(fake_ytdlp);
    let source = Source::Url("https://example.com/malformed".to_string());

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
    assert_eq!(outcome, RunOutcome::Completed);
}

#[tokio::test]
async fn test_hang_terminated_on_cancel() {
    let temp_dir = TempDir::new().unwrap();
    let fake_ytdlp = build_fake_ytdlp(temp_dir.path());
    let backend = Arc::new(YtDlpBackend::new(fake_ytdlp));
    let source = Source::Url("https://example.com/hang".to_string());

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
    let (progress, _) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job,
        control,
        progress,
        store,
        limiter,
    };

    let handle_clone = control_handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        handle_clone.cancel();
    });

    let outcome = backend.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Cancelled);
}

#[cfg(windows)]
#[tokio::test]
async fn test_windows_cmd_script_support() {
    let temp_dir = TempDir::new().unwrap();
    let fake_ytdlp_exe = build_fake_ytdlp(temp_dir.path());
    let cmd_script = temp_dir.path().join("fake_ytdlp.cmd");
    std::fs::write(
        &cmd_script,
        format!("@echo off\r\n\"{}\" %*\r\n", fake_ytdlp_exe.display()),
    )
    .unwrap();

    let backend = YtDlpBackend::new(cmd_script);
    let source = Source::Url("https://example.com/watch?v=123".to_string());

    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();
    assert_eq!(probe.backend, BackendKind::YtDlp);
    assert_eq!(probe.filename, "Test Video.mp4");
}
