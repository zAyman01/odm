//! Desktop application implementation for egui/eframe.

use crate::desktop::{ClipboardWatcher, NotificationService, should_notify_state_change};
use crate::messages::{GuiEvent, UserAction};
use crate::state::{ThemeMode, ViewState};
use crate::ui::add_dialog::render_add_dialog;
use crate::ui::detail_panel::render_detail_panel;
use crate::ui::downloads_table::render_downloads_table;
use crate::ui::settings_dialog::render_settings_dialog;
use crate::ui::sidebar::render_sidebar;
use crate::ui::toolbar::render_toolbar;
use eframe::egui;
use odm_api::Event;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

pub struct OdmGuiApp {
    pub state: ViewState,
    pub action_tx: mpsc::UnboundedSender<UserAction>,
    pub event_rx: mpsc::UnboundedReceiver<GuiEvent>,
    pub clipboard: Option<Arc<ClipboardWatcher>>,
}

impl OdmGuiApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        action_tx: mpsc::UnboundedSender<UserAction>,
        event_rx: mpsc::UnboundedReceiver<GuiEvent>,
        clipboard: Option<Arc<ClipboardWatcher>>,
    ) -> Self {
        Self {
            state: ViewState::default(),
            action_tx,
            event_rx,
            clipboard,
        }
    }
}

impl eframe::App for OdmGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Drain incoming daemon events
        let mut got_events = false;
        while let Ok(event) = self.event_rx.try_recv() {
            got_events = true;

            // Handle OS notification triggers
            if let GuiEvent::EngineEvent(Event::JobStateChanged { id, ref state }) = event
                && should_notify_state_change(state)
            {
                let filename = self
                    .state
                    .downloads
                    .iter()
                    .find(|d| d.id == id)
                    .map(|d| d.filename.as_str())
                    .unwrap_or("Download");

                if state == "completed" {
                    NotificationService::notify_completion(filename);
                } else if state == "failed" {
                    NotificationService::notify_failure(filename, None);
                }
            }

            self.state.apply_event(event);
        }

        if got_events {
            ctx.request_repaint();
        }

        // Apply visual theme
        match self.state.theme {
            ThemeMode::Dark | ThemeMode::System => ctx.set_visuals(egui::Visuals::dark()),
            ThemeMode::Light => ctx.set_visuals(egui::Visuals::light()),
        }

        // 1. Top Panel: Actions and navigation
        egui::TopBottomPanel::top("top_toolbar").show(ctx, |ui| {
            render_toolbar(ui, &mut self.state, &self.action_tx);
        });

        // 2. Bottom Panel: Segment chunk visualizer and detail info
        if self.state.selected_id.is_some() {
            egui::TopBottomPanel::bottom("bottom_detail")
                .resizable(true)
                .default_height(140.0)
                .show(ctx, |ui| {
                    render_detail_panel(ui, &mut self.state);
                });
        }

        // 3. Left Panel: Sidebar filters and queues
        egui::SidePanel::left("left_sidebar")
            .resizable(true)
            .default_width(180.0)
            .show(ctx, |ui| {
                render_sidebar(ui, &mut self.state);
            });

        // 4. Central Panel: Downloads table
        egui::CentralPanel::default().show(ctx, |ui| {
            render_downloads_table(ui, &mut self.state);
        });

        // 5. Modal dialogs
        render_add_dialog(ctx, &mut self.state, &self.action_tx);
        render_settings_dialog(
            ctx,
            &mut self.state,
            &self.action_tx,
            self.clipboard.as_deref(),
        );

        // Keep UI smoothly animating while active downloads are running
        if self
            .state
            .downloads
            .iter()
            .any(|d| d.state == "downloading")
        {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }
}
