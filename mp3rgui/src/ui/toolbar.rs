use super::widgets::{keep_together, primary_button, step_badge};
use crate::app::Mp3rgainApp;

/// The three steps of a session, left to right: add files, analyze them,
/// apply the gain. Settings sit in the bar below, list housekeeping (Remove,
/// Clear All) above the table, and Cancel beside the progress bar.
pub fn render(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    let frame =
        egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(12, 8));
    egui::TopBottomPanel::top("toolbar")
        .frame(frame)
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().button_padding = egui::vec2(12.0, 5.0);
                let idle = !app.is_processing();
                let has_files = !app.files.is_empty();

                keep_together(ui, "add", |ui| {
                    step_badge(ui, 1);
                    ui.add_enabled_ui(idle, |ui| {
                        if ui.button(format!("➕  {}", tr!("Add Files"))).clicked() {
                            super::pick_files(app);
                        }
                        if ui.button(format!("🗁  {}", tr!("Add Folder"))).clicked() {
                            super::pick_folder(app, true);
                        }
                    });
                    next_step(ui);
                });

                keep_together(ui, "analyze", |ui| {
                    step_badge(ui, 2);
                    ui.add_enabled_ui(has_files && idle, |ui| {
                        if ui.button(tr!("Track Analysis")).clicked() {
                            app.start_analyze_tracks(ctx);
                        }
                        if ui.button(tr!("Album Analysis")).clicked() {
                            app.start_analyze_album(ctx);
                        }
                    });
                    next_step(ui);
                });

                keep_together(ui, "apply", |ui| {
                    step_badge(ui, 3);
                    ui.add_enabled_ui(has_files && idle, |ui| {
                        if primary_button(ui, tr!("Track Gain")).clicked() {
                            app.start_apply_track_gain(ctx);
                        }
                        if primary_button(ui, tr!("Album Gain")).clicked() {
                            app.start_apply_album_gain(ctx);
                        }
                    });
                });
            });
        });
}

/// The chevron between two steps. It ends a group rather than starting the
/// next, so a narrow window wraps after it.
fn next_step(ui: &mut egui::Ui) {
    let p = super::theme::palette(ui);
    ui.add_space(2.0);
    ui.label(egui::RichText::new("›").size(18.0).color(p.line_strong));
    ui.add_space(2.0);
}
