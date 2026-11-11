//! Desktop integration: clipboard URL monitoring and OS notifications.

use crate::messages::GuiEvent;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;
use url::Url;

/// Extracts and validates an HTTP or HTTPS URL from arbitrary clipboard text.
/// Returns None if the string is not a valid absolute HTTP/HTTPS URL or is empty.
pub fn extract_valid_download_url(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parsed = match Url::parse(trimmed) {
        Ok(u) => u,
        Err(_) => return None,
    };

    if (parsed.scheme() == "http" || parsed.scheme() == "https")
        && parsed.host_str().is_some_and(|h| !h.is_empty())
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Determines if an engine state transition warrants an OS desktop notification.
pub fn should_notify_state_change(state: &str) -> bool {
    matches!(state, "completed" | "failed")
}

/// Watches the system clipboard for valid download URLs in a background thread.
pub struct ClipboardWatcher {
    enabled: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

impl ClipboardWatcher {
    pub fn start(event_tx: mpsc::UnboundedSender<GuiEvent>, poll_interval: Duration) -> Self {
        let enabled = Arc::new(AtomicBool::new(true));
        let running = Arc::new(AtomicBool::new(true));

        let enabled_clone = Arc::clone(&enabled);
        let running_clone = Arc::clone(&running);

        std::thread::spawn(move || {
            let mut last_captured_url: Option<String> = None;
            while running_clone.load(Ordering::Relaxed) {
                if enabled_clone.load(Ordering::Relaxed)
                    && let Ok(mut clipboard) = arboard::Clipboard::new()
                    && let Ok(text) = clipboard.get_text()
                    && let Some(url) = extract_valid_download_url(&text)
                    && last_captured_url.as_deref() != Some(&url)
                {
                    last_captured_url = Some(url.clone());
                    let _ = event_tx.send(GuiEvent::UrlCaptured(url));
                }
                std::thread::sleep(poll_interval);
            }
        });

        Self { enabled, running }
    }

    pub fn set_enabled(&self, val: bool) {
        self.enabled.store(val, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

impl Drop for ClipboardWatcher {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

/// OS Desktop Notification service.
pub struct NotificationService;

impl NotificationService {
    pub fn notify_completion(filename: &str) {
        let _ = notify_rust::Notification::new()
            .appname("odm")
            .summary("Download Completed")
            .body(&format!("{filename} has finished downloading."))
            .show();
    }

    pub fn notify_failure(filename: &str, error: Option<&str>) {
        let msg = match error {
            Some(err) => format!("{filename} failed: {err}"),
            None => format!("{filename} download failed."),
        };
        let _ = notify_rust::Notification::new()
            .appname("odm")
            .summary("Download Failed")
            .body(&msg)
            .show();
    }
}
