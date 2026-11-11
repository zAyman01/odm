//! Central downloads list and table view.

use crate::state::ViewState;
use crate::ui::{format_bytes, format_speed};
use eframe::egui;
use odm_api::DownloadItemDto;

pub fn render_downloads_table(ui: &mut egui::Ui, state: &mut ViewState) {
    let downloads: Vec<DownloadItemDto> = state.filtered_downloads().into_iter().cloned().collect();

    if downloads.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new("No downloads in this view.").italics());
        });
        return;
    }

    let mut new_selection = None;
    let selected_id = state.selected_id;

    egui::ScrollArea::both()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            egui::Grid::new("downloads_grid")
                .striped(true)
                .spacing([12.0, 6.0])
                .min_col_width(40.0)
                .show(ui, |ui| {
                    // Header
                    ui.label(egui::RichText::new("ID").strong());
                    ui.label(egui::RichText::new("Filename").strong());
                    ui.label(egui::RichText::new("Size").strong());
                    ui.label(egui::RichText::new("Progress").strong());
                    ui.label(egui::RichText::new("Speed").strong());
                    ui.label(egui::RichText::new("Status").strong());
                    ui.end_row();

                    for item in downloads {
                        let is_selected = selected_id == Some(item.id);

                        // ID column
                        let id_text = format!("#{}", item.id);
                        if ui.selectable_label(is_selected, &id_text).clicked() {
                            new_selection = Some(Some(item.id));
                        }

                        // Filename column
                        let name_text = if item.filename.is_empty() {
                            "Untitled"
                        } else {
                            &item.filename
                        };
                        if ui.selectable_label(is_selected, name_text).clicked() {
                            new_selection = Some(Some(item.id));
                        }

                        // Size column
                        let size_text = match item.total_bytes {
                            Some(total) => format_bytes(total),
                            None => format!("{} (chunked)", format_bytes(item.downloaded_bytes)),
                        };
                        ui.label(size_text);

                        // Progress bar column
                        let pct_clamped = (item.progress_pct / 100.0).clamp(0.0, 1.0);
                        let bar = egui::ProgressBar::new(pct_clamped)
                            .text(format!("{:.1}%", item.progress_pct))
                            .desired_width(120.0);
                        ui.add(bar);

                        // Speed column
                        let speed_text = format_speed(item.speed_bps);
                        ui.label(speed_text);

                        // Status column
                        let status_color = match item.state.as_str() {
                            "completed" => egui::Color32::from_rgb(46, 204, 113),
                            "downloading" => egui::Color32::from_rgb(52, 152, 219),
                            "failed" => egui::Color32::from_rgb(231, 76, 60),
                            "paused" => egui::Color32::from_rgb(241, 196, 15),
                            _ => egui::Color32::GRAY,
                        };
                        ui.colored_label(status_color, &item.state);

                        ui.end_row();
                    }
                });
        });

    if let Some(selection) = new_selection {
        state.select_download(selection);
    }
}
