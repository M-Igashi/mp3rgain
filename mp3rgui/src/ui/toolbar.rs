use crate::app::{mode_reference, Mp3rgainApp};
use mp3rgain::replaygain::REPLAYGAIN_REFERENCE_DB;

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

            // Target volume. Stored on the 89 dB scale and shown on the
            // selected mode's scale, so one offset carries across modes:
            // 95 dB in RG 1.0 is -12 LUFS in RG 2.0 (issues #272, #364).
            ui.label("Target:");
            let shift = mode_reference(app.analysis_mode) - REPLAYGAIN_REFERENCE_DB;
            let mut shown = app.target_volume + shift;
            let resp = ui
                .add_enabled(
                    !app.is_processing(),
                    egui::DragValue::new(&mut shown)
                        .speed(0.1)
                        .range(75.0 + shift..=100.0 + shift)
                        .suffix(format!(" {}", app.analysis_mode.unit())),
                )
                .on_hover_text(
                    "Level to normalize to. One setting shared by all analysis \
                     modes as an offset from the reference: 95 dB in RG 1.0 is \
                     -12 LUFS in RG 2.0 and -17 LUFS in R128. Equivalent to the \
                     CLI's -d.",
                );
            // The gain columns are derived from the Target, so a sort on
            // them must be redone (issue #161 item 1).
            if resp.changed() {
                app.target_volume = shown - shift;
                app.mark_display_dirty();
            }
        });
    });
}
