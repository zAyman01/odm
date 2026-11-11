//! Desktop client application entrypoint for the odm download manager.
#![forbid(unsafe_code)]

use eframe::egui;
use odm_gui::app::OdmGuiApp;
use odm_gui::bridge::DaemonBridge;
use odm_gui::desktop::ClipboardWatcher;
use odm_gui::messages::{GuiEvent, UserAction};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

fn resolve_token() -> Option<String> {
    if let Ok(token) = std::env::var("ODM_TOKEN") {
        return Some(token);
    }
    if let Some(dirs) = directories::ProjectDirs::from("com", "odm", "odm") {
        let token_path = dirs.config_dir().join("token");
        if let Ok(content) = std::fs::read_to_string(token_path) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn main() -> Result<(), eframe::Error> {
    let server_url =
        std::env::var("ODM_DAEMON_URL").unwrap_or_else(|_| "http://127.0.0.1:47888".to_string());
    let token = resolve_token();

    let (ui_tx, ui_rx) = mpsc::unbounded_channel::<GuiEvent>();
    let (action_tx, action_rx) = mpsc::unbounded_channel::<UserAction>();

    // Start background daemon connection bridge
    let _bridge = DaemonBridge::start(server_url, token, ui_tx.clone(), action_rx);

    // Start clipboard monitoring service
    let clipboard = ClipboardWatcher::start(ui_tx, Duration::from_millis(500));

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 640.0])
            .with_min_inner_size([720.0, 480.0])
            .with_title("odm - Open Download Manager"),
        ..Default::default()
    };

    let clipboard_arc = Arc::new(clipboard);

    eframe::run_native(
        "odm",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(OdmGuiApp::new(
                cc,
                action_tx,
                ui_rx,
                Some(clipboard_arc),
            )))
        }),
    )
}
