use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum MuxError {
    #[error("ffmpeg not found")]
    NotFound,
    #[error("ffmpeg exited with status {0}")]
    ExitStatus(i32),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Locate ffmpeg binary: checks `settings_path` if non-empty, otherwise searches `PATH`.
pub fn find_ffmpeg(settings_path: &str) -> Option<PathBuf> {
    let trimmed = settings_path.trim();
    if !trimmed.is_empty() {
        let path = PathBuf::from(trimmed);
        if path.is_file() {
            return Some(path);
        }
        #[cfg(windows)]
        {
            let with_exe = path.with_extension("exe");
            if with_exe.is_file() {
                return Some(with_exe);
            }
        }
        return None;
    }

    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join("ffmpeg");
            if candidate.is_file() {
                return Some(candidate);
            }
            #[cfg(windows)]
            {
                let candidate_exe = dir.join("ffmpeg.exe");
                if candidate_exe.is_file() {
                    return Some(candidate_exe);
                }
            }
        }
    }

    None
}

/// Builds the command for an external tool. `Command` itself launches `.bat` and `.cmd`
/// wrappers and escapes their arguments, so no `cmd.exe /C` wrapper is used.
fn build_command(ffmpeg: &Path) -> Command {
    Command::new(ffmpeg)
}

/// Mux a single input (e.g. .ts) into an MP4 container copying streams without re-encoding.
pub async fn mux_to_mp4(input: &Path, output: &Path, ffmpeg: &Path) -> Result<(), MuxError> {
    if !ffmpeg.exists() {
        return Err(MuxError::NotFound);
    }

    let mut cmd = build_command(ffmpeg);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .arg("-y")
        .arg("-i")
        .arg(input)
        .arg("-c")
        .arg("copy")
        .arg(output);

    let status = cmd.status().await?;
    if status.success() {
        Ok(())
    } else {
        Err(MuxError::ExitStatus(status.code().unwrap_or(-1)))
    }
}

/// Mux separate video and audio tracks into a single output file copying streams.
pub async fn mux_audio_video(
    video: &Path,
    audio: &Path,
    output: &Path,
    ffmpeg: &Path,
) -> Result<(), MuxError> {
    if !ffmpeg.exists() {
        return Err(MuxError::NotFound);
    }

    let mut cmd = build_command(ffmpeg);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .arg("-y")
        .arg("-i")
        .arg(video)
        .arg("-i")
        .arg(audio)
        .arg("-c")
        .arg("copy")
        .arg(output);

    let status = cmd.status().await?;
    if status.success() {
        Ok(())
    } else {
        Err(MuxError::ExitStatus(status.code().unwrap_or(-1)))
    }
}
