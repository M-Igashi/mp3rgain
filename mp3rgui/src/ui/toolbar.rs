use crate::app::Mp3rgainApp;

pub fn render(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);

            // Add Files button
            if ui.button("Add Files").clicked() {
                if let Some(paths) = rfd::FileDialog::new()
                    .add_filter("Audio files", &["mp3", "m4a", "aac"])
                    .pick_files()
                {
                    app.add_files(paths);
                }
            }

            // Add Folder button
            if ui.button("Add Folder").clicked() {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    app.add_folder(folder, true);
                }
            }

            ui.separator();

            // Analysis buttons
            ui.add_enabled_ui(!app.files.is_empty() && !app.is_processing, |ui| {
                if ui.button("Track Analysis").clicked() {
                    app.analyze_tracks();
                }
                if ui.button("Album Analysis").clicked() {
                    app.analyze_album();
                }
            });

            ui.separator();

            // Gain buttons
            ui.add_enabled_ui(!app.files.is_empty() && !app.is_processing, |ui| {
                if ui.button("Track Gain").clicked() {
                    app.apply_track_gain();
                }
                if ui.button("Album Gain").clicked() {
                    app.apply_album_gain();
                }
            });

            ui.separator();

            // Remove buttons
            ui.add_enabled_ui(
                !app.selected_indices.is_empty() && !app.is_processing,
                |ui| {
                    if ui.button("Remove").clicked() {
                        app.remove_selected();
                    }
                },
            );

            ui.add_enabled_ui(!app.files.is_empty() && !app.is_processing, |ui| {
                if ui.button("Clear All").clicked() {
                    app.clear_files();
                }
            });

            ui.separator();

            // Target volume
            ui.label("Target:");
            ui.add(
                egui::DragValue::new(&mut app.target_volume)
                    .speed(0.1)
                    .range(75.0..=100.0)
                    .suffix(" dB"),
            );
        });
    });
}
