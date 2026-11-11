//! Modal dialog for client and daemon settings.

use crate::desktop::ClipboardWatcher;
use crate::messages::UserAction;
use crate::state::{ThemeMode, ViewState};
use eframe::egui;
use odm_api::UpdateSettingsRequest;
use tokio::sync::mpsc;

pub fn render_settings_dialog(
    ctx: &egui::Context,
    state: &mut ViewState,
    action_tx: &mpsc::UnboundedSender<UserAction>,
    clipboard: Option<&ClipboardWatcher>,
) {
    if !state.settings_dialog.open {
        return;
    }

    let mut open = state.settings_dialog.open;
    egui::Window::new("Settings")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .min_width(400.0)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Default download directory:");
                    ui.text_edit_singleline(&mut state.settings_dialog.download_dir);
                });

                ui.horizontal(|ui| {
                    ui.label("Default segments per file:");
                    ui.add(egui::Slider::new(
                        &mut state.settings_dialog.default_segments,
                        1..=32,
                    ));
                });

                ui.horizontal(|ui| {
                    ui.label("Global speed limit (B/s, 0 = unlimited):");
                    ui.add(egui::Slider::new(
                        &mut state.settings_dialog.global_speed_limit,
                        0..=50_000_000,
                    ));
                });

                ui.checkbox(
                    &mut state.settings_dialog.clipboard_monitoring,
                    "Automatically capture download URLs from clipboard",
                );

                ui.horizontal(|ui| {
                    ui.label("Theme:");
                    ui.radio_value(&mut state.settings_dialog.theme, ThemeMode::Dark, "Dark");
                    ui.radio_value(&mut state.settings_dialog.theme, ThemeMode::Light, "Light");
                });

                ui.add_space(8.0);
                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        let update = UpdateSettingsRequest {
                            download_dir: if state.settings_dialog.download_dir.is_empty() {
                                None
                            } else {
                                Some(state.settings_dialog.download_dir.clone())
                            },
                            default_segments: Some(state.settings_dialog.default_segments),
                            global_speed_limit: if state.settings_dialog.global_speed_limit == 0 {
                                None
                            } else {
                                Some(state.settings_dialog.global_speed_limit)
                            },
                            default_retry_count: None,
                        };

                        let _ = action_tx.send(UserAction::UpdateSettings(update));

                        state.theme = state.settings_dialog.theme;

                        if let Some(watcher) = clipboard {
                            watcher.set_enabled(state.settings_dialog.clipboard_monitoring);
                        }

                        state.close_settings_dialog();
                    }

                    if ui.button("Cancel").clicked() {
                        state.close_settings_dialog();
                    }
                });
            });
        });

    if !open {
        state.close_settings_dialog();
    }
}
