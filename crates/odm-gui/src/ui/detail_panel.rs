//! Bottom details panel with segment chunk visualizer.

use crate::state::ViewState;
use eframe::egui;

pub fn render_detail_panel(ui: &mut egui::Ui, state: &mut ViewState) {
    let Some(id) = state.selected_id else {
        ui.label(
            egui::RichText::new("Select a download to inspect segments and details.").italics(),
        );
        return;
    };

    let Some(item) = state.downloads.iter().find(|d| d.id == id).cloned() else {
        return;
    };

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.heading(format!("#{} {}", item.id, item.filename));
            if ui.small_button("✕ Close").clicked() {
                state.select_download(None);
            }
        });

        ui.label(format!("URL: {}", item.url));
        ui.horizontal(|ui| {
            ui.label(format!("Save to: {}", item.target_dir));
            ui.separator();
            ui.label(format!("Queue: {}", item.queue));
        });

        if item.backend.as_deref() == Some("torrent") {
            ui.horizontal(|ui| {
                ui.label(format!("Backend: Torrent | State: {}", item.state));
                if let Some(ref detail) = state.selected_detail
                    && let Some(sr) = detail.options.seed_ratio
                {
                    ui.separator();
                    ui.label(format!("Target Seed Ratio: {sr:.1}"));
                }
            });
        }

        if let Some(ref err) = item.error {
            ui.colored_label(
                egui::Color32::from_rgb(231, 76, 60),
                format!("Error: {err}"),
            );
        }

        ui.add_space(6.0);
        ui.label(egui::RichText::new("Segment Distribution:").strong());

        // Allocate canvas for segment visualizer bar
        let width = ui.available_width().max(100.0);
        let height = 18.0;
        let (rect, _response) =
            ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let painter = ui.painter();

        // Background track
        painter.rect_filled(rect, 2.0, egui::Color32::from_gray(35));

        if let Some(ref detail) = state.selected_detail
            && !detail.segments.is_empty()
        {
            let total = item
                .total_bytes
                .unwrap_or(item.downloaded_bytes.max(1))
                .max(1);
            for seg in &detail.segments {
                let x0 = rect.left()
                    + ((seg.start as f32 / total as f32) * rect.width()).clamp(0.0, rect.width());
                let x1 = rect.left()
                    + ((seg.end as f32 / total as f32) * rect.width()).clamp(0.0, rect.width());

                let seg_rect = egui::Rect::from_min_max(
                    egui::pos2(x0, rect.top()),
                    egui::pos2(x1.max(x0 + 1.0), rect.bottom()),
                );

                let color = if seg.is_complete {
                    egui::Color32::from_rgb(46, 204, 113) // Green = complete
                } else if seg.downloaded > 0 {
                    egui::Color32::from_rgb(52, 152, 219) // Blue = active
                } else {
                    egui::Color32::from_gray(80) // Gray = pending
                };

                painter.rect_filled(seg_rect, 1.0, color);
            }
        } else {
            // Fallback progress fill if detailed segment info is not yet fetched
            let pct = (item.progress_pct / 100.0).clamp(0.0, 1.0);
            let fill_rect = egui::Rect::from_min_max(
                rect.min,
                egui::pos2(rect.left() + pct * rect.width(), rect.bottom()),
            );
            painter.rect_filled(fill_rect, 2.0, egui::Color32::from_rgb(52, 152, 219));
        }
    });
}
