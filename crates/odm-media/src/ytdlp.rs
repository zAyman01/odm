//! yt-dlp backend for odm.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use odm_core::backend::{Backend, BackendError, JobContext, RunOutcome};
use odm_core::job::{BackendKind, JobOptions, ProbeResult, Source};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

use crate::quality::QualityInfo;
use crate::state::YtDlpResumeState;

/// yt-dlp backend for downloading media streams via the `yt-dlp` executable.
#[derive(Debug, Clone)]
pub struct YtDlpBackend {
    ytdlp_path: PathBuf,
}

impl YtDlpBackend {
    /// Creates a new `YtDlpBackend` pointing to the `yt-dlp` executable or wrapper script.
    pub fn new(ytdlp_path: PathBuf) -> Self {
        Self { ytdlp_path }
    }

    /// Queries selectable qualities from `yt-dlp -J <url>`.
    pub async fn probe_qualities(&self, source: &Source) -> Result<Vec<QualityInfo>, BackendError> {
        let url = source.url().ok_or(BackendError::Unsupported)?;
        let json = self.fetch_info(url, None).await?;

        let mut qualities = Vec::new();
        if let Some(formats) = json.get("formats").and_then(|v| v.as_array()) {
            for fmt in formats {
                let id = fmt
                    .get("format_id")
                    .and_then(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .or_else(|| v.as_i64().map(|n| n.to_string()))
                    })
                    .unwrap_or_default();
                if id.is_empty() {
                    continue;
                }

                let resolution = fmt
                    .get("resolution")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .or_else(|| {
                        let w = fmt.get("width").and_then(|v| v.as_u64());
                        let h = fmt.get("height").and_then(|v| v.as_u64());
                        match (w, h) {
                            (Some(width), Some(height)) => Some(format!("{width}x{height}")),
                            _ => None,
                        }
                    });

                let tbr = fmt
                    .get("tbr")
                    .and_then(|v| v.as_f64().or_else(|| v.as_u64().map(|n| n as f64)));
                let bandwidth = fmt
                    .get("bandwidth")
                    .and_then(|v| v.as_u64())
                    .or_else(|| tbr.map(|t| (t * 1000.0) as u64));

                let vcodec = fmt
                    .get("vcodec")
                    .and_then(|v| v.as_str())
                    .filter(|s| *s != "none");
                let acodec = fmt
                    .get("acodec")
                    .and_then(|v| v.as_str())
                    .filter(|s| *s != "none");
                let codecs = match (vcodec, acodec) {
                    (Some(v), Some(a)) => Some(format!("{v}, {a}")),
                    (Some(v), None) => Some(v.to_string()),
                    (None, Some(a)) => Some(a.to_string()),
                    (None, None) => None,
                };

                let ext = fmt.get("ext").and_then(|v| v.as_str());
                let note = fmt.get("format_note").and_then(|v| v.as_str());

                let base_label = if let Some(n) = note {
                    n.to_string()
                } else if let Some(ref res) = resolution {
                    res.clone()
                } else {
                    id.clone()
                };

                let label = match (ext, tbr) {
                    (Some(e), Some(kbps)) => format!("{base_label} - {} kbps ({e})", kbps as u64),
                    (Some(e), None) => format!("{base_label} ({e})"),
                    (None, Some(kbps)) => format!("{base_label} - {} kbps", kbps as u64),
                    (None, None) => base_label,
                };

                qualities.push(QualityInfo {
                    id,
                    resolution,
                    bandwidth,
                    codecs,
                    label,
                });
            }
        }

        Ok(qualities)
    }

    async fn fetch_info(
        &self,
        url: &str,
        opts: Option<&JobOptions>,
    ) -> Result<serde_json::Value, BackendError> {
        if !self.ytdlp_path.exists() {
            return Err(BackendError::YtDlp(
                "yt-dlp not found. Install it from https://github.com/yt-dlp/yt-dlp".to_string(),
            ));
        }

        let mut cmd = build_command(&self.ytdlp_path);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .arg("-J")
            .arg("--no-warnings");

        if let Some(opts) = opts {
            if let Some(ua) = &opts.user_agent {
                cmd.args(["--user-agent", ua]);
            }
            if let Some(referer) = &opts.referer {
                cmd.args(["--referer", referer]);
            }
            if let Some(cookies) = &opts.cookies {
                cmd.args(["--add-header", &format!("Cookie: {cookies}")]);
            }
            for (name, val) in &opts.headers {
                cmd.args(["--add-header", &format!("{name}: {val}")]);
            }
        }

        cmd.arg(url);

        let output = cmd.output().await.map_err(BackendError::Io)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = if stderr.trim().is_empty() {
                format!("yt-dlp exited with status {}", output.status)
            } else {
                stderr.trim().to_string()
            };
            return Err(BackendError::YtDlp(msg));
        }

        serde_json::from_slice(&output.stdout)
            .map_err(|e| BackendError::YtDlp(format!("invalid JSON from yt-dlp: {e}")))
    }
}

#[async_trait]
impl Backend for YtDlpBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::YtDlp
    }

    async fn probe(&self, source: &Source, opts: &JobOptions) -> Result<ProbeResult, BackendError> {
        let url = source.url().ok_or(BackendError::Unsupported)?;
        let json = self.fetch_info(url, Some(opts)).await?;

        let title = json
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("video");
        let ext = json.get("ext").and_then(|v| v.as_str()).unwrap_or("");
        let filename = if !ext.is_empty() && !title.ends_with(&format!(".{ext}")) {
            format!("{title}.{ext}")
        } else {
            title.to_string()
        };

        let total_bytes = json
            .get("filesize")
            .and_then(|v| v.as_u64())
            .or_else(|| json.get("filesize_approx").and_then(|v| v.as_u64()))
            .or_else(|| {
                json.get("formats")
                    .and_then(|f| f.as_array())
                    .and_then(|arr| {
                        arr.iter().rev().find_map(|fmt| {
                            fmt.get("filesize")
                                .and_then(|v| v.as_u64())
                                .or_else(|| fmt.get("filesize_approx").and_then(|v| v.as_u64()))
                        })
                    })
            });

        let content_type = if !ext.is_empty() {
            Some(format!("video/{ext}"))
        } else {
            Some("video/mp4".to_string())
        };

        Ok(ProbeResult {
            backend: BackendKind::YtDlp,
            filename,
            total_bytes,
            supports_ranges: false,
            etag: None,
            last_modified: None,
            content_type,
        })
    }

    async fn run(&self, ctx: JobContext) -> Result<RunOutcome, BackendError> {
        if !self.ytdlp_path.exists() {
            return Err(BackendError::YtDlp(
                "yt-dlp not found. Install it from https://github.com/yt-dlp/yt-dlp".to_string(),
            ));
        }

        let url = ctx.job.source().url().ok_or(BackendError::Unsupported)?;
        let target_path = ctx.job.target_path();
        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut cmd = build_command(&self.ytdlp_path);
        cmd.arg("-o")
            .arg(&target_path)
            .arg("--newline")
            .arg("--progress-template")
            .arg("download:%(progress._percent_str)s %(progress._total_bytes_estimate_str)s %(progress._speed_str)s");

        if let Some(format_id) = &ctx.job.options().quality {
            cmd.arg("-f").arg(format_id);
        }

        if let Some(speed_limit) = ctx.job.options().speed_limit {
            cmd.arg("--limit-rate").arg(speed_limit.to_string());
        }

        let is_resumed = ctx.job.backend_state().is_some()
            || ctx
                .store
                .get_backend_state(ctx.job.id())
                .ok()
                .flatten()
                .is_some();
        if is_resumed {
            cmd.arg("--continue");
        }

        if let Some(ua) = &ctx.job.options().user_agent {
            cmd.args(["--user-agent", ua]);
        }
        if let Some(referer) = &ctx.job.options().referer {
            cmd.args(["--referer", referer]);
        }
        if let Some(cookies) = &ctx.job.options().cookies {
            cmd.args(["--add-header", &format!("Cookie: {cookies}")]);
        }
        for (name, val) in &ctx.job.options().headers {
            cmd.args(["--add-header", &format!("{name}: {val}")]);
        }

        cmd.arg("--no-warnings");
        cmd.arg(url);

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(BackendError::Io)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| BackendError::YtDlp("failed to capture child stdout".to_string()))?;
        let stderr = child.stderr.take();

        let stderr_handle = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(mut stream) = stderr {
                let _ = stream.read_to_end(&mut buf).await;
            }
            String::from_utf8_lossy(&buf).to_string()
        });

        let mut reader = BufReader::new(stdout).lines();

        let outcome = loop {
            if ctx.control.is_cancelled() {
                let _ = child.kill().await;
                break RunOutcome::Cancelled;
            }

            if ctx.control.is_paused() {
                let _ = child.kill().await;
                let resume_state = YtDlpResumeState {
                    format_id: ctx.job.options().quality.clone(),
                    output_path: target_path.to_string_lossy().to_string(),
                };
                if let Ok(serialized) = serde_json::to_string(&resume_state) {
                    let _ = ctx.store.set_backend_state(ctx.job.id(), &serialized);
                }
                break RunOutcome::Paused;
            }

            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    // Check control token on each tick
                }
                line_res = reader.next_line() => {
                    match line_res {
                        Ok(Some(line)) => {
                            if let Some((downloaded, total)) = parse_progress_line(&line) {
                                ctx.progress.report(downloaded, total);
                            }
                        }
                        Ok(None) => {
                            let status = child.wait().await.map_err(BackendError::Io)?;
                            let stderr_text = stderr_handle.await.unwrap_or_default();
                            if status.success() {
                                return Ok(RunOutcome::Completed);
                            } else {
                                let msg = if stderr_text.trim().is_empty() {
                                    format!("yt-dlp exited with status {status}")
                                } else {
                                    stderr_text.trim().to_string()
                                };
                                return Err(BackendError::YtDlp(msg));
                            }
                        }
                        Err(err) => {
                            let _ = child.kill().await;
                            return Err(BackendError::Io(err));
                        }
                    }
                }
            }
        };

        Ok(outcome)
    }
}

/// Builds the command for an external tool. `Command` itself launches `.bat` and `.cmd`
/// wrappers and escapes their arguments, so no `cmd.exe /C` wrapper is used.
fn build_command(ytdlp_path: &Path) -> Command {
    Command::new(ytdlp_path)
}

fn parse_size(raw: &str) -> Option<u64> {
    let s = raw.trim().trim_start_matches('~');
    if s.is_empty() || s.eq_ignore_ascii_case("na") || s.eq_ignore_ascii_case("unknown") {
        return None;
    }
    if let Ok(bytes) = s.parse::<u64>() {
        return Some(bytes);
    }

    let (num_part, unit) = if let Some(idx) = s.find(|c: char| c.is_alphabetic()) {
        (&s[..idx], &s[idx..])
    } else {
        (s, "")
    };

    let num: f64 = num_part.trim().parse().ok()?;
    let multiplier: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "b" | "bytes" => 1.0,
        "kib" | "k" | "kb" => 1024.0,
        "mib" | "m" | "mb" => 1024.0 * 1024.0,
        "gib" | "g" | "gb" => 1024.0 * 1024.0 * 1024.0,
        "tib" | "t" | "tb" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };
    Some((num * multiplier).round() as u64)
}

fn parse_progress_line(line: &str) -> Option<(u64, Option<u64>)> {
    let trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix("download:") {
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        if tokens.is_empty() {
            return None;
        }
        let percent_str = tokens[0].trim_end_matches('%');
        let percent: f64 = percent_str.parse().ok()?;
        let total_bytes = tokens.get(1).and_then(|s| parse_size(s));
        let downloaded = match total_bytes {
            Some(tot) => ((tot as f64) * percent / 100.0).round() as u64,
            None => percent as u64,
        };
        return Some((downloaded, total_bytes));
    }

    if let Some(rest) = trimmed.strip_prefix("[download]") {
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        let mut percent: Option<f64> = None;
        let mut total_bytes: Option<u64> = None;

        for (i, token) in tokens.iter().enumerate() {
            if token.ends_with('%') {
                percent = token.trim_end_matches('%').parse().ok();
            }
            if *token == "of" && i + 1 < tokens.len() {
                total_bytes = parse_size(tokens[i + 1]);
            }
        }

        if let Some(pct) = percent {
            let downloaded = match total_bytes {
                Some(tot) => ((tot as f64) * pct / 100.0).round() as u64,
                None => pct as u64,
            };
            return Some((downloaded, total_bytes));
        }
    }

    None
}
