//! Modal dialog displaying installed plugins with security warnings.

use crate::messages::UserAction;
use crate::state::ViewState;
use eframe::egui;
use tokio::sync::mpsc;

pub fn render_plugins_dialog(
    ctx: &egui::Context,
    state: &mut ViewState,
    action_tx: &mpsc::UnboundedSender<UserAction>,
) {
    if !state.plugins.is_open {
        return;
    }

    let mut open = state.plugins.is_open;
    egui::Window::new("🔌 Plugins")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .min_width(550.0)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                // Prominent Security Warning Banner per Spec Section 4
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(50, 30, 20))
                    .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(230, 126, 34)))
                    .inner_margin(egui::Margin::same(8))
                    .corner_radius(egui::CornerRadius::same(4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                egui::Color32::from_rgb(243, 156, 18),
                                "⚠️ Security Notice:",
                            );
                            ui.label(
                                "Plugins are external programs running with full user permissions without sandboxing. Only install plugins from trusted sources.",
                            );
                        });
                    });

                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.heading("Installed Plugins");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("🔄 Refresh").clicked() {
                            state.plugins.is_loading = true;
                            let _ = action_tx.send(UserAction::FetchPlugins);
                        }
                    });
                });

                if state.plugins.is_loading {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Loading installed plugins...");
                    });
                }

                if let Some(ref err) = state.plugins.error {
                    ui.add_space(4.0);
                    ui.colored_label(
                        egui::Color32::from_rgb(231, 76, 60),
                        format!("Error: {err}"),
                    );
                }

                ui.add_space(4.0);

                if state.plugins.plugins.is_empty() && !state.plugins.is_loading {
                    ui.label("No plugins discovered in plugins directory.");
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(250.0)
                        .show(ui, |ui| {
                            egui::Grid::new("plugins_list_grid")
                                .striped(true)
                                .spacing([12.0, 6.0])
                                .min_col_width(60.0)
                                .show(ui, |ui| {
                                    ui.strong("Name");
                                    ui.strong("Version");
                                    ui.strong("Role");
                                    ui.strong("URL Patterns");
                                    ui.strong("Command");
                                    ui.end_row();

                                    for plugin in &state.plugins.plugins {
                                        ui.label(&plugin.name);
                                        ui.label(&plugin.version);
                                        ui.label(&plugin.role);
                                        ui.label(plugin.url_patterns.join(", "));
                                        ui.label(plugin.command.join(" "));
                                        ui.end_row();
                                    }
                                });
                        });
                }

                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() {
                        state.plugins.close();
                    }
                });
            });
        });

    if !open {
        state.plugins.close();
    }
}
