//! Modal dialog for the Site Grabber feature.

use crate::messages::UserAction;
use crate::state::ViewState;
use eframe::egui;
use tokio::sync::mpsc;

pub fn render_grabber_dialog(
    ctx: &egui::Context,
    state: &mut ViewState,
    action_tx: &mpsc::UnboundedSender<UserAction>,
) {
    if !state.grabber.is_open {
        return;
    }

    let mut open = state.grabber.is_open;
    egui::Window::new("🌐 Site Grabber")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .min_width(550.0)
        .min_height(400.0)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Start URL:");
                    ui.add(
                        egui::TextEdit::singleline(&mut state.grabber.url)
                            .desired_width(ui.available_width() - 90.0),
                    );

                    let can_crawl =
                        !state.grabber.url.trim().is_empty() && !state.grabber.is_crawling;
                    if ui
                        .add_enabled(can_crawl, egui::Button::new("🔍 Crawl"))
                        .clicked()
                        && let Some(req) = state.build_grab_request()
                    {
                        state.grabber.is_crawling = true;
                        state.grabber.error = None;
                        let _ = action_tx.send(UserAction::StartGrab(Box::new(req)));
                    }
                });

                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label("Depth:");
                    ui.add(egui::Slider::new(&mut state.grabber.depth, 1..=5));

                    ui.label("Max Pages:");
                    ui.add(egui::DragValue::new(&mut state.grabber.max_pages).range(1..=500));

                    ui.checkbox(&mut state.grabber.respect_robots, "Respect robots.txt");
                });

                ui.horizontal(|ui| {
                    ui.label("Extensions:");
                    ui.add(
                        egui::TextEdit::singleline(&mut state.grabber.extensions)
                            .hint_text("e.g. zip, pdf, png, jpg"),
                    );

                    ui.label("Pattern:");
                    ui.add(
                        egui::TextEdit::singleline(&mut state.grabber.pattern)
                            .hint_text("e.g. *report*"),
                    );
                });

                if state.grabber.is_crawling {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Crawling website pages on same host...");
                    });
                }

                if let Some(ref err) = state.grabber.error {
                    ui.add_space(4.0);
                    ui.colored_label(
                        egui::Color32::from_rgb(231, 76, 60),
                        format!("Error: {err}"),
                    );
                }

                ui.separator();

                ui.horizontal(|ui| {
                    ui.heading("Discovered Files");
                    ui.label(format!(
                        "({} files found across {} pages)",
                        state.grabber.files.len(),
                        state.grabber.pages_crawled
                    ));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Deselect All").clicked() {
                            state.grabber.deselect_all();
                        }
                        if ui.button("Select All").clicked() {
                            state.grabber.select_all();
                        }
                    });
                });

                ui.add_space(4.0);

                egui::ScrollArea::vertical()
                    .max_height(250.0)
                    .show(ui, |ui| {
                        let mut toggled_index = None;
                        egui::Grid::new("grabber_files_grid")
                            .striped(true)
                            .spacing([8.0, 4.0])
                            .min_col_width(20.0)
                            .show(ui, |ui| {
                                ui.strong("");
                                ui.strong("Filename");
                                ui.strong("Depth");
                                ui.strong("URL");
                                ui.end_row();

                                for (idx, file) in state.grabber.files.iter().enumerate() {
                                    let mut is_selected =
                                        state.grabber.selected_indices.contains(&idx);
                                    if ui.checkbox(&mut is_selected, "").changed() {
                                        toggled_index = Some(idx);
                                    }
                                    ui.label(&file.filename);
                                    ui.label(file.depth.to_string());
                                    ui.label(&file.url);
                                    ui.end_row();
                                }
                            });
                        if let Some(idx) = toggled_index {
                            state.grabber.toggle_file(idx);
                        }
                    });

                ui.separator();

                ui.horizontal(|ui| {
                    let selected_count = state.grabber.selected_indices.len();
                    let btn_label = format!("➕ Add Selected ({selected_count})");
                    let can_add = selected_count > 0;

                    if ui
                        .add_enabled(can_add, egui::Button::new(btn_label))
                        .clicked()
                    {
                        let add_reqs = state.build_grabbed_add_requests();
                        for req in add_reqs {
                            let _ = action_tx.send(UserAction::AddDownload(Box::new(req)));
                        }
                        state.grabber.close();
                    }

                    if ui.button("Close").clicked() {
                        state.grabber.close();
                    }
                });
            });
        });

    if !open {
        state.grabber.close();
    }
}
