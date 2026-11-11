//! Top navigation and action toolbar.

use crate::messages::UserAction;
use crate::state::{ThemeMode, ViewState};
use eframe::egui;
use tokio::sync::mpsc;

pub fn render_toolbar(
    ui: &mut egui::Ui,
    state: &mut ViewState,
    action_tx: &mpsc::UnboundedSender<UserAction>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;

        if ui.button("➕ Add URL").clicked() {
            state.open_add_dialog(None);
        }

        let selected = state.selected_id;

        ui.add_enabled_ui(selected.is_some(), |ui| {
            if ui.button("▶ Resume").clicked()
                && let Some(id) = selected
            {
                let _ = action_tx.send(UserAction::Resume(id));
            }

            if ui.button("⏸ Pause").clicked()
                && let Some(id) = selected
            {
                let _ = action_tx.send(UserAction::Pause(id));
            }

            if ui.button("🗑 Remove").clicked()
                && let Some(id) = selected
            {
                let _ = action_tx.send(UserAction::Remove {
                    id,
                    delete_file: false,
                });
            }
        });

        ui.separator();

        if ui.button("⚙ Settings").clicked() {
            state.open_settings_dialog();
        }

        let theme_label = match state.theme {
            ThemeMode::Dark => "☀ Light",
            ThemeMode::Light | ThemeMode::System => "🌙 Dark",
        };
        if ui.button(theme_label).clicked() {
            state.theme = match state.theme {
                ThemeMode::Dark => ThemeMode::Light,
                ThemeMode::Light | ThemeMode::System => ThemeMode::Dark,
            };
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if state.daemon_connected {
                ui.colored_label(egui::Color32::from_rgb(46, 204, 113), "● Daemon Connected");
            } else {
                ui.colored_label(egui::Color32::from_rgb(231, 76, 60), "○ Daemon Offline");
            }
        });
    });
}
