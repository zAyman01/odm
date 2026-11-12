#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use odm_media::mux::{MuxError, find_ffmpeg, mux_audio_video, mux_to_mp4};

#[cfg(windows)]
fn make_fake_ffmpeg(dir: &std::path::Path, exit_code: i32) -> PathBuf {
    let script_path = dir.join(format!("fake_ffmpeg_{exit_code}.cmd"));
    std::fs::write(
        &script_path,
        format!("@echo off\r\nexit /b {exit_code}\r\n"),
    )
    .unwrap();
    script_path
}

#[cfg(not(windows))]
fn make_fake_ffmpeg(dir: &std::path::Path, exit_code: i32) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script_path = dir.join(format!("fake_ffmpeg_{exit_code}.sh"));
    std::fs::write(&script_path, format!("#!/bin/sh\nexit {exit_code}\n")).unwrap();
    let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms).unwrap();
    script_path
}

#[test]
fn test_find_ffmpeg_empty_settings() {
    let res = find_ffmpeg("");
    if let Some(path) = res {
        assert!(path.exists());
    }
}

#[test]
fn test_find_ffmpeg_nonexistent_path() {
    let res = find_ffmpeg("/nonexistent/path/to/ffmpeg_binary_xyz_12345");
    assert!(res.is_none());
}

#[test]
fn test_find_ffmpeg_explicit_existing_path() {
    let temp_dir = tempfile::tempdir().unwrap();
    let fake_binary = temp_dir.path().join("ffmpeg_custom");
    std::fs::write(&fake_binary, b"fake ffmpeg").unwrap();
    let res = find_ffmpeg(fake_binary.to_str().unwrap());
    assert_eq!(res, Some(fake_binary));
}

#[tokio::test]
async fn test_mux_to_mp4_success() {
    let temp_dir = tempfile::tempdir().unwrap();
    let fake_ffmpeg = make_fake_ffmpeg(temp_dir.path(), 0);
    let input = temp_dir.path().join("input.ts");
    let output = temp_dir.path().join("output.mp4");
    std::fs::write(&input, b"dummy ts content").unwrap();

    let res = mux_to_mp4(&input, &output, &fake_ffmpeg).await;
    assert!(res.is_ok(), "expected Ok, got {res:?}");
}

#[tokio::test]
async fn test_mux_to_mp4_failure_exit_status() {
    let temp_dir = tempfile::tempdir().unwrap();
    let fake_ffmpeg = make_fake_ffmpeg(temp_dir.path(), 1);
    let input = temp_dir.path().join("input.ts");
    let output = temp_dir.path().join("output.mp4");
    std::fs::write(&input, b"dummy ts content").unwrap();

    let res = mux_to_mp4(&input, &output, &fake_ffmpeg).await;
    match res {
        Err(MuxError::ExitStatus(1)) => (),
        other => panic!("expected MuxError::ExitStatus(1), got {other:?}"),
    }
}

#[tokio::test]
async fn test_mux_audio_video_success() {
    let temp_dir = tempfile::tempdir().unwrap();
    let fake_ffmpeg = make_fake_ffmpeg(temp_dir.path(), 0);
    let video = temp_dir.path().join("video.m4v");
    let audio = temp_dir.path().join("audio.m4a");
    let output = temp_dir.path().join("output.mp4");
    std::fs::write(&video, b"dummy video content").unwrap();
    std::fs::write(&audio, b"dummy audio content").unwrap();

    let res = mux_audio_video(&video, &audio, &output, &fake_ffmpeg).await;
    assert!(res.is_ok(), "expected Ok, got {res:?}");
}
