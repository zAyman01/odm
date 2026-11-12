//! Modal dialog for client and daemon settings.

use crate::desktop::ClipboardWatcher;
use crate::messages::UserAction;
use crate::state::{SettingsTab, ThemeMode, ViewState};
use eframe::egui;
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
        .min_width(450.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut state.settings_dialog.active_tab,
                    SettingsTab::General,
                    "General",
                );
                ui.selectable_value(
                    &mut state.settings_dialog.active_tab,
                    SettingsTab::Categories,
                    "Categories",
                );
                ui.selectable_value(
                    &mut state.settings_dialog.active_tab,
                    SettingsTab::Scheduler,
                    "Scheduler",
                );
            });
            ui.separator();

            match state.settings_dialog.active_tab {
                SettingsTab::General => {
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

                        ui.horizontal(|ui| {
                            ui.label("FFmpeg path:");
                            ui.text_edit_singleline(&mut state.settings_dialog.ffmpeg_path);
                        });

                        ui.horizontal(|ui| {
                            ui.label("yt-dlp path:");
                            ui.text_edit_singleline(&mut state.settings_dialog.ytdlp_path);
                        });

                        ui.checkbox(
                            &mut state.settings_dialog.clipboard_monitoring,
                            "Automatically capture download URLs from clipboard",
                        );

                        ui.horizontal(|ui| {
                            ui.label("Theme:");
                            ui.radio_value(
                                &mut state.settings_dialog.theme,
                                ThemeMode::Dark,
                                "Dark",
                            );
                            ui.radio_value(
                                &mut state.settings_dialog.theme,
                                ThemeMode::Light,
                                "Light",
                            );
                        });

                        ui.add_space(8.0);
                        ui.separator();

                        ui.horizontal(|ui| {
                            if ui.button("Save").clicked() {
                                let update = state.settings_dialog.build_update_settings_request();

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
                }

                SettingsTab::Categories => {
                    ui.vertical(|ui| {
                        ui.label("Configure download target directories for file categories:");
                        ui.add_space(4.0);

                        for cat in &state.categories {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(&cat.name).strong());
                                    ui.label(format!(
                                        "(Extensions: {})",
                                        cat.extensions.join(", ")
                                    ));
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Directory:");
                                    let dir_val = state
                                        .settings_dialog
                                        .category_dirs
                                        .entry(cat.name.clone())
                                        .or_insert_with(|| cat.directory.clone());
                                    ui.text_edit_singleline(dir_val);
                                    if ui.button("Save").clicked() {
                                        let _ = action_tx.send(UserAction::SetCategory {
                                            name: cat.name.clone(),
                                            directory: Some(dir_val.clone()),
                                            extensions: Some(cat.extensions.clone()),
                                        });
                                    }
                                });
                            });
                        }

                        if state.categories.is_empty() {
                            ui.label("No categories found.");
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        if ui.button("Close").clicked() {
                            state.close_settings_dialog();
                        }
                    });
                }

                SettingsTab::Scheduler => {
                    ui.vertical(|ui| {
                        ui.label("Configure automatic download start/stop schedules per queue:");
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label("Queue:");
                            egui::ComboBox::from_id_salt("schedule_queue_select")
                                .selected_text(&state.settings_dialog.selected_queue)
                                .show_ui(ui, |ui| {
                                    for q in &state.queues {
                                        if ui
                                            .selectable_value(
                                                &mut state.settings_dialog.selected_queue,
                                                q.clone(),
                                                q,
                                            )
                                            .clicked()
                                            && let Some(detail) =
                                                state.queue_details.iter().find(|qd| &qd.name == q)
                                        {
                                            state.settings_dialog.queue_start_time =
                                                detail.schedule_start.clone().unwrap_or_default();
                                            state.settings_dialog.queue_stop_time =
                                                detail.schedule_stop.clone().unwrap_or_default();
                                            state.settings_dialog.queue_days = detail
                                                .schedule_days
                                                .as_ref()
                                                .map(|d| d.join(", "))
                                                .unwrap_or_default();
                                        }
                                    }
                                });
                        });

                        ui.horizontal(|ui| {
                            ui.label("Start time (HH:MM):");
                            ui.text_edit_singleline(&mut state.settings_dialog.queue_start_time);
                        });

                        ui.horizontal(|ui| {
                            ui.label("Stop time (HH:MM, optional):");
                            ui.text_edit_singleline(&mut state.settings_dialog.queue_stop_time);
                        });

                        ui.horizontal(|ui| {
                            ui.label("Active days (e.g. mon, tue, wed, fri):");
                            ui.text_edit_singleline(&mut state.settings_dialog.queue_days);
                        });

                        ui.add_space(8.0);
                        ui.separator();

                        ui.horizontal(|ui| {
                            if ui.button("Apply Schedule").clicked()
                                && !state.settings_dialog.queue_start_time.trim().is_empty()
                            {
                                let days: Vec<String> = state
                                    .settings_dialog
                                    .queue_days
                                    .split(',')
                                    .map(|s| s.trim().to_lowercase())
                                    .filter(|s| !s.is_empty())
                                    .collect();

                                let _ = action_tx.send(UserAction::UpdateQueueSchedule {
                                    name: state.settings_dialog.selected_queue.clone(),
                                    start_time: state
                                        .settings_dialog
                                        .queue_start_time
                                        .trim()
                                        .to_string(),
                                    stop_time: if state
                                        .settings_dialog
                                        .queue_stop_time
                                        .trim()
                                        .is_empty()
                                    {
                                        None
                                    } else {
                                        Some(
                                            state
                                                .settings_dialog
                                                .queue_stop_time
                                                .trim()
                                                .to_string(),
                                        )
                                    },
                                    days: if days.is_empty() { None } else { Some(days) },
                                });
                            }

                            if ui.button("Close").clicked() {
                                state.close_settings_dialog();
                            }
                        });
                    });
                }
            }
        });

    if !open {
        state.close_settings_dialog();
    }
}
