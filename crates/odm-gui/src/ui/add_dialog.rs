//! Modal dialog for adding and probing a new download.

use crate::messages::UserAction;
use crate::state::ViewState;
use crate::ui::format_bytes;
use eframe::egui;
use odm_api::{AddDownloadRequest, JobOptionsDto};
use tokio::sync::mpsc;

pub fn render_add_dialog(
    ctx: &egui::Context,
    state: &mut ViewState,
    action_tx: &mpsc::UnboundedSender<UserAction>,
) {
    if !state.add_dialog.open {
        return;
    }

    let mut open = state.add_dialog.open;
    egui::Window::new("Add Download")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .min_width(450.0)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label("URL:");
                    let text_edit = egui::TextEdit::singleline(&mut state.add_dialog.url)
                        .desired_width(ui.available_width() - 80.0);
                    ui.add(text_edit);

                    if ui.button("Probe").clicked() && !state.add_dialog.url.is_empty() {
                        state.add_dialog.probing = true;
                        state.add_dialog.error = None;
                        let _ = action_tx.send(UserAction::Probe(state.add_dialog.url.clone()));
                    }
                });

                if state.add_dialog.probing {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Probing server for filename and range support...");
                    });
                }

                if let Some(ref probed) = state.add_dialog.probed {
                    ui.group(|ui| {
                        let size_str = match probed.size {
                            Some(sz) => format_bytes(sz),
                            None => "Unknown (stream)".to_string(),
                        };
                        ui.label(format!("Size: {size_str}"));
                        ui.label(format!(
                            "Resumable / Segments: {}",
                            if probed.ranges { "Yes" } else { "No" }
                        ));
                    });
                }

                if let Some(ref err) = state.add_dialog.error {
                    ui.colored_label(
                        egui::Color32::from_rgb(231, 76, 60),
                        format!("Probe Error: {err}"),
                    );
                }

                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label("Filename:");
                    ui.text_edit_singleline(&mut state.add_dialog.filename);
                });

                ui.horizontal(|ui| {
                    ui.label("Save to:");
                    ui.text_edit_singleline(&mut state.add_dialog.target_dir);
                });

                ui.horizontal(|ui| {
                    ui.label("Queue:");
                    egui::ComboBox::from_id_salt("add_queue_select")
                        .selected_text(&state.add_dialog.queue)
                        .show_ui(ui, |ui| {
                            for q in &state.queues {
                                ui.selectable_value(&mut state.add_dialog.queue, q.clone(), q);
                            }
                        });
                });

                ui.horizontal(|ui| {
                    ui.label("Segments:");
                    ui.add(egui::Slider::new(&mut state.add_dialog.segments, 1..=32));
                });

                ui.horizontal(|ui| {
                    ui.label("Speed limit (B/s, 0 = unlimited):");
                    ui.add(egui::Slider::new(
                        &mut state.add_dialog.speed_limit,
                        0..=50_000_000,
                    ));
                });

                ui.add_space(8.0);
                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("Download Now").clicked() && !state.add_dialog.url.is_empty() {
                        let req = AddDownloadRequest {
                            url: state.add_dialog.url.clone(),
                            filename: if state.add_dialog.filename.is_empty() {
                                None
                            } else {
                                Some(state.add_dialog.filename.clone())
                            },
                            target_dir: if state.add_dialog.target_dir.is_empty() {
                                None
                            } else {
                                Some(state.add_dialog.target_dir.clone())
                            },
                            queue: Some(state.add_dialog.queue.clone()),
                            options: Some(JobOptionsDto {
                                segments: Some(state.add_dialog.segments),
                                speed_limit: if state.add_dialog.speed_limit == 0 {
                                    None
                                } else {
                                    Some(state.add_dialog.speed_limit)
                                },
                                headers: None,
                                user_agent: None,
                                referer: None,
                                cookies: None,
                                checksum: None,
                                mirrors: None,
                            }),
                        };

                        let _ = action_tx.send(UserAction::AddDownload(Box::new(req)));
                        state.close_add_dialog();
                    }

                    if ui.button("Cancel").clicked() {
                        state.close_add_dialog();
                    }
                });
            });
        });

    if !open {
        state.close_add_dialog();
    }
}
