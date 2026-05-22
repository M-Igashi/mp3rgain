use crate::app::Mp3rgainApp;

/// Apply-options checkbox row, shown right below the toolbar.
///
/// All four toggles are disabled while a worker is running so a
/// half-applied batch can't see its settings change mid-flight.
pub fn render(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("options_panel").show(ctx, |ui| {
        ui.add_enabled_ui(!app.is_processing, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Options:");

                ui.checkbox(&mut app.apply_options.prevent_clipping, "Prevent clipping")
                    .on_hover_text(
                        "Cap the applied gain at the available headroom so the \
                         output cannot clip. Equivalent to the CLI's -k flag.",
                    );

                ui.checkbox(&mut app.apply_options.preserve_timestamp, "Preserve mtime")
                    .on_hover_text(
                        "Restore the file's last-modified timestamp after writing \
                         so it doesn't jump to the top of playlists sorted by date. \
                         CLI -p.",
                    );

                ui.checkbox(&mut app.apply_options.wrap, "Wrap mode")
                    .on_hover_text(
                        "Allow gain values to wrap around 0–255 instead of saturating. \
                         Disables the clipping check. CLI -w. Rarely useful — leave off.",
                    );

                ui.checkbox(&mut app.apply_options.use_id3v2, "Use ID3v2 (MP3)")
                    .on_hover_text(
                        "MP3 only: store undo + ReplayGain tags in ID3v2 TXXX frames \
                         instead of APE. Required for foobar2000 / Winamp / Rockbox \
                         to see ReplayGain values on MP3. CLI -s i.",
                    );

                ui.separator();

                ui.checkbox(&mut app.apply_options.dry_run, "Dry run")
                    .on_hover_text(
                        "Preview Apply Track / Album Gain without modifying any file. \
                         The Status column shows the steps that would be applied. \
                         CLI -n.",
                    );
            });
        });
    });
}
