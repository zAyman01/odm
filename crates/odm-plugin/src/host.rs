use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::PluginError;
use crate::manifest::PluginManifest;
use crate::protocol::{ResolverRequest, ResolverResponse};

/// Host for executing external plugin processes and managing their lifecycle.
pub struct PluginHost;

impl PluginHost {
    /// Default execution timeout for plugin resolver invocations (30 seconds).
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    /// Resolve a target URL using the specified plugin manifest and directory.
    pub async fn resolve(
        manifest: &PluginManifest,
        plugin_dir: &Path,
        url: &str,
        timeout_dur: Duration,
    ) -> Result<ResolverResponse, PluginError> {
        if manifest.command.is_empty() {
            return Err(PluginError::InvalidManifest(
                "plugin command cannot be empty".to_string(),
            ));
        }

        let raw_program = &manifest.command[0];
        let prog_path = Path::new(raw_program);
        let resolved_program: PathBuf = if prog_path.is_absolute() {
            prog_path.to_path_buf()
        } else if plugin_dir.join(prog_path).is_file() {
            plugin_dir.join(prog_path)
        } else {
            prog_path.to_path_buf()
        };

        let is_windows_batch = {
            let ext = resolved_program
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat")
        };

        let mut cmd = if cfg!(windows) && is_windows_batch {
            let mut c = tokio::process::Command::new("cmd.exe");
            c.arg("/C").arg(&resolved_program);
            for arg in &manifest.command[1..] {
                c.arg(arg);
            }
            c
        } else {
            let mut c = tokio::process::Command::new(&resolved_program);
            for arg in &manifest.command[1..] {
                c.arg(arg);
            }
            c
        };

        cmd.current_dir(plugin_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        tracing::debug!(
            plugin = %manifest.name,
            url = %url,
            program = ?resolved_program,
            "spawning plugin resolver process"
        );

        let mut child = cmd.spawn().map_err(|e| {
            PluginError::ProcessFailed(format!(
                "failed to spawn plugin '{}' ({:?}): {e}",
                manifest.name, resolved_program
            ))
        })?;

        let request = ResolverRequest::new(url);
        let req_bytes = serde_json::to_vec(&request)
            .map_err(|e| PluginError::ProcessFailed(format!("failed to serialize request: {e}")))?;

        let mut stdin = child.stdin.take();
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();

        let run_future = async {
            if let Some(mut sin) = stdin.take() {
                sin.write_all(&req_bytes).await?;
                sin.flush().await?;
                drop(sin);
            }

            let mut stdout_buf = Vec::new();
            let mut stderr_buf = Vec::new();

            let read_stdout = async {
                if let Some(mut out) = stdout.take() {
                    out.read_to_end(&mut stdout_buf).await?;
                }
                Ok::<(), std::io::Error>(())
            };

            let read_stderr = async {
                if let Some(mut err) = stderr.take() {
                    err.read_to_end(&mut stderr_buf).await?;
                }
                Ok::<(), std::io::Error>(())
            };

            let (out_res, err_res, status_res) =
                tokio::join!(read_stdout, read_stderr, child.wait());
            out_res?;
            err_res?;
            let status = status_res?;

            Ok::<_, std::io::Error>((status, stdout_buf, stderr_buf))
        };

        let (status, stdout_buf, stderr_buf) =
            match tokio::time::timeout(timeout_dur, run_future).await {
                Ok(run_result) => run_result.map_err(|e| {
                    PluginError::ProcessFailed(format!("I/O error during plugin execution: {e}"))
                })?,
                Err(_) => {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    return Err(PluginError::Timeout(format!(
                        "plugin '{}' timed out after {:?}",
                        manifest.name, timeout_dur
                    )));
                }
            };

        if !status.success() {
            let stderr_text = String::from_utf8_lossy(&stderr_buf).trim().to_string();
            return Err(PluginError::ProcessFailed(format!(
                "plugin exited with {}: {}",
                status, stderr_text
            )));
        }

        let stdout_str = String::from_utf8(stdout_buf).map_err(|e| {
            PluginError::InvalidJson(format!("plugin stdout was not valid UTF-8: {e}"))
        })?;

        let trimmed = stdout_str.trim();
        if trimmed.is_empty() {
            return Err(PluginError::InvalidJson(
                "plugin returned empty output".to_string(),
            ));
        }

        let response: ResolverResponse = serde_json::from_str(trimmed).map_err(|e| {
            PluginError::InvalidJson(format!("failed to parse plugin JSON response: {e}"))
        })?;

        if response.downloads.is_empty() {
            return Err(PluginError::InvalidJson(
                "resolver returned empty downloads list".to_string(),
            ));
        }

        Ok(response)
    }

    /// Resolve a target URL using default timeout (30 seconds).
    pub async fn resolve_default(
        manifest: &PluginManifest,
        plugin_dir: &Path,
        url: &str,
    ) -> Result<ResolverResponse, PluginError> {
        Self::resolve(manifest, plugin_dir, url, Self::DEFAULT_TIMEOUT).await
    }
}
