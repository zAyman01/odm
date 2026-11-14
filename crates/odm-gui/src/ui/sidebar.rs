//! Left navigation sidebar with category, status, and queue filters.

use crate::messages::UserAction;
use crate::state::{Filter, ViewState, is_active_state};
use eframe::egui;
use tokio::sync::mpsc;

pub fn render_sidebar(
    ui: &mut egui::Ui,
    state: &mut ViewState,
    action_tx: &mpsc::UnboundedSender<UserAction>,
) {
    ui.vertical(|ui| {
        ui.heading("Downloads");
        ui.add_space(4.0);

        let total_count = state.downloads.len();
        let active_count = state
            .downloads
            .iter()
            .filter(|d| is_active_state(&d.state))
            .count();
        let completed_count = state
            .downloads
            .iter()
            .filter(|d| d.state == "completed")
            .count();
        let torrent_count = state
            .downloads
            .iter()
            .filter(|d| d.backend.as_deref() == Some("torrent"))
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

        if ui
            .selectable_label(
                state.filter == Filter::Torrents,
                format!("🧲 Torrents ({torrent_count})"),
            )
            .clicked()
        {
            state.filter = Filter::Torrents;
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

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.new_queue_name)
                    .hint_text("New queue")
                    .desired_width(80.0),
            );
            ui.add(egui::DragValue::new(&mut state.new_queue_concurrency).range(1..=16));
            if ui.button("Add").clicked()
                && let Some(action) = state.take_new_queue_action()
            {
                let _ = action_tx.send(action);
            }
        });

        ui.separator();
        ui.label(egui::RichText::new("Categories").strong());

        for cat in &state.categories {
            let cat_filter = Filter::Category(cat.name.clone());
            let cat_count = state
                .downloads
                .iter()
                .filter(|d| d.category.as_deref() == Some(&cat.name))
                .count();

            if ui
                .selectable_label(
                    state.filter == cat_filter,
                    format!("🏷 {} ({cat_count})", cat.name),
                )
                .clicked()
            {
                state.filter = cat_filter;
            }
        }
    });
}
