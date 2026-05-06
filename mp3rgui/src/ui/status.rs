use crate::app::Mp3rgainApp;

pub fn render(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("status_panel").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Progress:");
            ui.add(
                egui::ProgressBar::new(app.total_progress)
                    .desired_width(200.0)
                    .show_percentage(),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Exit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });

        ui.separator();

        // Status bar
        ui.horizontal(|ui| {
            let file_count = app.files.len();
            if file_count == 0 {
                ui.label("No files loaded");
            } else if file_count == 1 {
                ui.label("1 file");
            } else {
                ui.label(format!("{} files", file_count));
            }

            if !app.status_message.is_empty() {
                ui.separator();
                ui.label(&app.status_message);
            }
        });
    });
}
