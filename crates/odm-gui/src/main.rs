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

fn main() -> Result<(), eframe::Error> {
    let server_url = odm_gui::bridge::default_server_url();
    let token = odm_gui::bridge::resolve_token();

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
