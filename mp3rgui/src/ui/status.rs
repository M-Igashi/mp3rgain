use super::theme;
use super::widgets::caption;
use crate::app::Mp3rgainApp;

/// Status line: the last message on the left; while a worker runs, its
/// progress and the Cancel button on the right.
pub fn render(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    let frame =
        egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(12, 6));
    egui::TopBottomPanel::bottom("status_panel")
        .frame(frame)
        .exact_height(36.0)
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                if !app.status_message.is_empty() {
                    caption(ui, &app.status_message);
                } else if app.files.is_empty() {
                    caption(ui, tr!("No files loaded"));
                }

                if !app.is_processing() {
                    return;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr!("Cancel")).clicked() {
                        app.cancel_current_work();
                    }
                    let p = theme::palette(ui);
                    ui.label(
                        egui::RichText::new(format!("{:.0}%", app.total_progress * 100.0))
                            .color(p.text_strong)
                            .monospace(),
                    );
                    ui.add(
                        egui::ProgressBar::new(app.total_progress)
                            .desired_width(220.0)
                            .desired_height(6.0)
                            .fill(p.accent)
                            .corner_radius(3),
                    );
                    caption(ui, tr!("Progress:"));
                });
            });
        });
}
