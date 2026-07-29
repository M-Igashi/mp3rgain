use crate::app::Mp3rgainApp;

pub fn render(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);

            // Add Files button
            ui.add_enabled_ui(!app.is_processing(), |ui| {
                if ui.button("Add Files").clicked() {
                    if let Some(paths) = rfd::FileDialog::new()
                        .add_filter("Audio files", mp3rgain::SUPPORTED_EXTENSIONS)
                        .pick_files()
                    {
                        app.add_files(paths);
                    }
                }

                if ui.button("Add Folder").clicked() {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        app.add_folder(folder, true);
                    }
                }
            });

            ui.separator();

            // Analysis buttons
            ui.add_enabled_ui(!app.files.is_empty() && !app.is_processing(), |ui| {
                if ui.button("Track Analysis").clicked() {
                    app.start_analyze_tracks(ctx);
                }
                if ui.button("Album Analysis").clicked() {
                    app.start_analyze_album(ctx);
                }
            });

            ui.separator();

            // Gain buttons
            ui.add_enabled_ui(!app.files.is_empty() && !app.is_processing(), |ui| {
                if ui.button("Track Gain").clicked() {
                    app.start_apply_track_gain(ctx);
                }
                if ui.button("Album Gain").clicked() {
                    app.start_apply_album_gain(ctx);
                }
            });

            ui.separator();

            // Remove buttons
            ui.add_enabled_ui(
                !app.selected_indices.is_empty() && !app.is_processing(),
                |ui| {
                    if ui.button("Remove").clicked() {
                        app.remove_selected();
                    }
                },
            );

            ui.add_enabled_ui(!app.files.is_empty() && !app.is_processing(), |ui| {
                if ui.button("Clear All").clicked() {
                    app.clear_files();
                }
            });

            ui.separator();

            // Cancel — only enabled while a worker is running.
            ui.add_enabled_ui(app.is_processing(), |ui| {
                if ui.button("Cancel").clicked() {
                    app.cancel_current_work();
                }
            });

            ui.separator();

            // Target volume
            ui.label("Target:");
            ui.add_enabled(
                !app.is_processing(),
                egui::DragValue::new(&mut app.target_volume)
                    .speed(0.1)
                    .range(75.0..=100.0)
                    .suffix(" dB"),
            );
        });
    });
}
