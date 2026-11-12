//! Modal dialog for adding and probing a new download.

use crate::messages::UserAction;
use crate::state::ViewState;
use crate::ui::format_bytes;
use eframe::egui;
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
                    let mut url = state.add_dialog.url.clone();
                    let text_edit = egui::TextEdit::singleline(&mut url)
                        .desired_width(ui.available_width() - 80.0);
                    let resp = ui.add(text_edit);
                    if resp.changed() {
                        state.update_add_dialog_url(url);
                    }

                    if ui.button("Probe").clicked() && !state.add_dialog.url.is_empty() {
                        state.add_dialog.probing = true;
                        state.add_dialog.error = None;
                        let _ = action_tx.send(UserAction::Probe(state.add_dialog.url.clone()));
                    }
                });

                if !state.add_dialog.batch_preview.is_empty() {
                    ui.colored_label(
                        egui::Color32::from_rgb(52, 152, 219),
                        format!(
                            "Batch URL pattern: {} URLs will be added",
                            state.add_dialog.batch_preview.len()
                        ),
                    );
                }

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

                if !state.add_dialog.torrent_files.is_empty() {
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Torrent Files:").strong());
                        egui::ScrollArea::vertical()
                            .max_height(120.0)
                            .show(ui, |ui| {
                                for file in &state.add_dialog.torrent_files {
                                    let mut selected = state
                                        .add_dialog
                                        .selected_file_indices
                                        .contains(&file.index);
                                    let label =
                                        format!("{} ({})", file.path, format_bytes(file.size));
                                    if ui.checkbox(&mut selected, label).changed() {
                                        if selected {
                                            state
                                                .add_dialog
                                                .selected_file_indices
                                                .insert(file.index);
                                        } else {
                                            state
                                                .add_dialog
                                                .selected_file_indices
                                                .remove(&file.index);
                                        }
                                    }
                                }
                            });
                    });
                }

                if !state.add_dialog.qualities.is_empty() {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label("Quality:");
                            let selected_label = state
                                .add_dialog
                                .qualities
                                .iter()
                                .find(|q| Some(&q.id) == state.add_dialog.selected_quality.as_ref())
                                .map(|q| q.label.as_str())
                                .unwrap_or("Select quality");

                            let mut new_quality = None;
                            egui::ComboBox::from_id_salt("add_quality_select")
                                .selected_text(selected_label)
                                .show_ui(ui, |ui| {
                                    for q in &state.add_dialog.qualities {
                                        let is_selected =
                                            state.add_dialog.selected_quality.as_deref()
                                                == Some(&q.id);
                                        if ui.selectable_label(is_selected, &q.label).clicked() {
                                            new_quality = Some(q.id.clone());
                                        }
                                    }
                                });

                            if let Some(quality_id) = new_quality {
                                let _ =
                                    action_tx.send(UserAction::SelectQuality(quality_id.clone()));
                                state.handle_action(&UserAction::SelectQuality(quality_id));
                            }
                        });
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
                    ui.label("Category:");
                    let cat_label = state
                        .add_dialog
                        .category
                        .as_deref()
                        .unwrap_or("None (Auto)");
                    egui::ComboBox::from_id_salt("add_category_select")
                        .selected_text(cat_label)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(
                                    state.add_dialog.category.is_none(),
                                    "None (Auto)",
                                )
                                .clicked()
                            {
                                state.add_dialog.category = None;
                            }
                            for cat in &state.categories {
                                let is_selected =
                                    state.add_dialog.category.as_deref() == Some(&cat.name);
                                if ui.selectable_label(is_selected, &cat.name).clicked() {
                                    state.add_dialog.category = Some(cat.name.clone());
                                }
                            }
                        });
                });

                ui.horizontal(|ui| {
                    ui.label("Checksum:");
                    egui::ComboBox::from_id_salt("checksum_algo_select")
                        .selected_text(&state.add_dialog.checksum_algo)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut state.add_dialog.checksum_algo,
                                "sha256".to_string(),
                                "SHA-256",
                            );
                            ui.selectable_value(
                                &mut state.add_dialog.checksum_algo,
                                "sha1".to_string(),
                                "SHA-1",
                            );
                            ui.selectable_value(
                                &mut state.add_dialog.checksum_algo,
                                "md5".to_string(),
                                "MD5",
                            );
                        });
                    ui.add(
                        egui::TextEdit::singleline(&mut state.add_dialog.checksum_hash)
                            .hint_text("Expected hash digest..."),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Mirrors:");
                    ui.add(
                        egui::TextEdit::multiline(&mut state.add_dialog.mirrors)
                            .hint_text("One mirror URL per line or comma-separated")
                            .desired_rows(2),
                    );
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

                ui.horizontal(|ui| {
                    ui.label("Seed ratio (0.0 = unlimited/global):");
                    let mut ratio_val = state.add_dialog.seed_ratio.unwrap_or(0.0);
                    if ui
                        .add(
                            egui::DragValue::new(&mut ratio_val)
                                .speed(0.1)
                                .range(0.0..=100.0),
                        )
                        .changed()
                    {
                        state.add_dialog.seed_ratio = if ratio_val > 0.0 {
                            Some(ratio_val)
                        } else {
                            None
                        };
                    }
                });

                ui.add_space(8.0);
                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("Download Now").clicked() && !state.add_dialog.url.is_empty() {
                        let requests = state.build_add_download_requests();
                        for req in requests {
                            let _ = action_tx.send(UserAction::AddDownload(Box::new(req)));
                        }
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
