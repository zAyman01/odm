//! Left navigation sidebar with category, status, and queue filters.

use crate::state::{Filter, ViewState};
use eframe::egui;

pub fn render_sidebar(ui: &mut egui::Ui, state: &mut ViewState) {
    ui.vertical(|ui| {
        ui.heading("Downloads");
        ui.add_space(4.0);

        let total_count = state.downloads.len();
        let active_count = state
            .downloads
            .iter()
            .filter(|d| matches!(d.state.as_str(), "downloading" | "pending" | "probed"))
            .count();
        let completed_count = state
            .downloads
            .iter()
            .filter(|d| d.state == "completed")
            .count();

        if ui
            .selectable_label(
                state.filter == Filter::All,
                format!("📁 All ({total_count})"),
            )
            .clicked()
        {
            state.filter = Filter::All;
        }

        if ui
            .selectable_label(
                state.filter == Filter::Active,
                format!("⚡ Active ({active_count})"),
            )
            .clicked()
        {
            state.filter = Filter::Active;
        }

        if ui
            .selectable_label(
                state.filter == Filter::Completed,
                format!("✓ Completed ({completed_count})"),
            )
            .clicked()
        {
            state.filter = Filter::Completed;
        }

        ui.separator();
        ui.label(egui::RichText::new("Queues").strong());

        for queue_name in &state.queues {
            let q_filter = Filter::Queue(queue_name.clone());
            let q_count = state
                .downloads
                .iter()
                .filter(|d| d.queue == *queue_name)
                .count();

            if ui
                .selectable_label(
                    state.filter == q_filter,
                    format!("🗂 {queue_name} ({q_count})"),
                )
                .clicked()
            {
                state.filter = q_filter;
            }
        }

        ui.separator();
        ui.label(egui::RichText::new("Categories").strong());

        let categories = ["Documents", "Archives", "Media"];
        for cat in categories {
            let cat_filter = Filter::Category(cat.to_string());
            if ui
                .selectable_label(state.filter == cat_filter, format!("🏷 {cat}"))
                .clicked()
            {
                state.filter = cat_filter;
            }
        }
    });
}
