use crate::app::Mp3rgainApp;
use mp3rgain::replaygain::AnalysisMode;
use mp3rgain::TagLayout;

/// Apply-options checkbox row, shown right below the toolbar.
///
/// All four toggles are disabled while a worker is running so a
/// half-applied batch can't see its settings change mid-flight.
pub fn render(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("options_panel").show(ctx, |ui| {
        ui.add_enabled_ui(!app.is_processing(), |ui| {
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

                ui.horizontal(|ui| {
                    ui.label("MP3 tags:");
                    let layout = &mut app.apply_options.tag_layout;
                    egui::ComboBox::from_id_salt("tag_layout")
                        .selected_text(match layout {
                            TagLayout::Split => "ReplayGain in ID3v2",
                            TagLayout::Ape => "All in APEv2",
                            TagLayout::Id3v2 => "All in ID3v2",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(layout, TagLayout::Split, "ReplayGain in ID3v2")
                                .on_hover_text(
                                    "Default. REPLAYGAIN_* in ID3v2 TXXX where players \
                                 look, MP3GAIN_UNDO / MINMAX in APEv2 where mp3gain \
                                 looks.",
                                );
                            ui.selectable_value(layout, TagLayout::Ape, "All in APEv2")
                                .on_hover_text(
                                    "Byte-for-byte mp3gain behaviour. ReplayGain-aware \
                                     players will not find the values. CLI -s a.",
                                );
                            ui.selectable_value(layout, TagLayout::Id3v2, "All in ID3v2")
                                .on_hover_text(
                                    "Undo and ReplayGain both in ID3v2 TXXX frames. \
                                     CLI -s i.",
                                );
                        });
                });

                ui.separator();

                ui.checkbox(&mut app.apply_options.dry_run, "Dry run")
                    .on_hover_text(
                        "Preview Apply Track / Album Gain without modifying any file. \
                         The Status column shows the steps that would be applied. \
                         CLI -n.",
                    );

                ui.separator();

                ui.checkbox(&mut app.single_album, "Single album")
                    .on_hover_text(
                        "Treat all loaded files as one album for Album Analysis / \
                         Apply Album Gain, ignoring subfolders (e.g. multi-disc sets). \
                         Off = each folder is its own album.",
                    );

                ui.checkbox(&mut app.show_filename_only, "Filename only")
                    .on_hover_text(
                        "Show only the file name in the Path/File column instead of \
                         the full path. The full path is still shown on hover.",
                    );

                ui.separator();

                // Loudness measurement mode (issue #272). Switching modes
                // invalidates cached analysis results — RG1 and BS.1770
                // numbers live on different scales.
                ui.label("Analysis:");
                let before = app.analysis_mode;
                ui.radio_value(&mut app.analysis_mode, AnalysisMode::Rg1, "RG 1.0")
                    .on_hover_text(
                        "ReplayGain 1.0 (default): mp3gain-compatible values, \
                         89 dB reference. Keeps re-scans consistent with \
                         libraries normalized by mp3gain.",
                    );
                ui.radio_value(&mut app.analysis_mode, AnalysisMode::Rg2, "RG 2.0")
                    .on_hover_text(
                        "ReplayGain 2.0: BS.1770 integrated loudness, -18 LUFS \
                         reference (same perceived level as 89 dB). \
                         CLI --rg2.",
                    );
                ui.radio_value(&mut app.analysis_mode, AnalysisMode::R128, "R128")
                    .on_hover_text(
                        "EBU R128: BS.1770 integrated loudness, -23 LUFS \
                         broadcast target. CLI --r128.",
                    );
                if app.analysis_mode != before {
                    app.invalidate_analysis_results();
                }
            });
        });
    });
}
