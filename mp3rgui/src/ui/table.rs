use crate::app::{ClickMode, Mp3rgainApp};
use crate::worker::StoredTagsView;

/// Render the "Stored RG" column for one row.
///
/// `None` → not scanned yet. `Some(empty)` → scanned, file has no tags.
/// Otherwise → show a short summary (track gain) and reveal the full
/// per-field dump on hover.
fn render_stored_tags_cell(ui: &mut egui::Ui, tags: Option<&StoredTagsView>) {
    let Some(view) = tags else { return };
    if view.is_empty() {
        ui.weak("none").on_hover_text(format!(
            "No stored tags found ({} container)",
            view.format.unwrap_or("?")
        ));
        return;
    }
    let label = view.track_gain.as_deref().unwrap_or("—");
    ui.label(label).on_hover_ui(|ui| {
        ui.label(format!("Container: {}", view.format.unwrap_or("?")));
        ui.separator();
        for (name, value) in [
            ("REPLAYGAIN_TRACK_GAIN", view.track_gain.as_deref()),
            ("REPLAYGAIN_TRACK_PEAK", view.track_peak.as_deref()),
            ("REPLAYGAIN_ALBUM_GAIN", view.album_gain.as_deref()),
            ("REPLAYGAIN_ALBUM_PEAK", view.album_peak.as_deref()),
            ("MP3GAIN_UNDO", view.undo.as_deref()),
            ("MP3GAIN_MINMAX", view.minmax.as_deref()),
        ] {
            if let Some(v) = value {
                ui.label(format!("{}: {}", name, v));
            }
        }
    });
}

pub fn render(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    egui::ScrollArea::both().show(ui, |ui| {
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::auto().at_least(250.0)) // Path/File
            .column(egui_extras::Column::auto().at_least(70.0)) // Volume
            .column(egui_extras::Column::auto().at_least(50.0)) // Clipping
            .column(egui_extras::Column::auto().at_least(80.0)) // Track Gain
            .column(egui_extras::Column::auto().at_least(50.0)) // Clip (Track)
            .column(egui_extras::Column::auto().at_least(80.0)) // Album Volume
            .column(egui_extras::Column::auto().at_least(80.0)) // Album Gain
            .column(egui_extras::Column::auto().at_least(50.0)) // Clip (Album)
            .column(egui_extras::Column::auto().at_least(90.0)) // Stored RG
            .column(egui_extras::Column::remainder()) // Status
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.strong("Path/File");
                });
                header.col(|ui| {
                    ui.strong("Volume").on_hover_text(
                        "Track Analysis: current loudness relative to ReplayGain reference (89 dB). \
                         Find Max Amplitude: available headroom in dB before clipping.",
                    );
                });
                header.col(|ui| {
                    ui.strong("Clip");
                });
                header.col(|ui| {
                    ui.strong("Track Gain");
                });
                header.col(|ui| {
                    ui.strong("Clip(T)");
                });
                header.col(|ui| {
                    ui.strong("Album Vol");
                });
                header.col(|ui| {
                    ui.strong("Album Gain");
                });
                header.col(|ui| {
                    ui.strong("Clip(A)");
                });
                header.col(|ui| {
                    ui.strong("Stored RG").on_hover_text(
                        "Existing ReplayGain/undo tags read from the file. \
                         Run Analysis > Check Stored Tags to populate. \
                         Hover a cell for full tag details.",
                    );
                });
                header.col(|ui| {
                    ui.strong("Status");
                });
            })
            .body(|mut body| {
                // Defer click handling: the row closure borrows `app.files`,
                // so it can't also mutate `app.selected_indices` during the
                // iteration. Capture the requested action and apply it after
                // the loop.
                let mut pending_click: Option<(usize, ClickMode)> = None;
                for (idx, file) in app.files.iter().enumerate() {
                    let is_selected = app.selected_indices.contains(&idx);
                    body.row(18.0, |mut row| {
                        row.set_selected(is_selected);

                        row.col(|ui| {
                            if ui.selectable_label(is_selected, &file.filename).clicked() {
                                let mode = ui.input(|i| {
                                    let toggle = i.modifiers.ctrl || i.modifiers.command;
                                    let shift = i.modifiers.shift;
                                    match (shift, toggle) {
                                        (true, true) => ClickMode::RangeAdd,
                                        (true, false) => ClickMode::Range,
                                        (false, true) => ClickMode::Toggle,
                                        (false, false) => ClickMode::Replace,
                                    }
                                });
                                pending_click = Some((idx, mode));
                            }
                        });
                        row.col(|ui| {
                            if let Some(v) = file.volume {
                                ui.label(format!("{:.1}", v));
                            }
                        });
                        row.col(|ui| {
                            if file.clipping {
                                ui.colored_label(egui::Color32::RED, "Y");
                            }
                        });
                        row.col(|ui| {
                            if let Some(g) = file.track_gain {
                                let color = if file.track_clip {
                                    egui::Color32::RED
                                } else {
                                    ui.style().visuals.text_color()
                                };
                                ui.colored_label(color, format!("{:+.1} dB", g));
                            }
                        });
                        row.col(|ui| {
                            if file.track_clip {
                                ui.colored_label(egui::Color32::RED, "Y");
                            }
                        });
                        row.col(|ui| {
                            if let Some(v) = file.album_volume {
                                ui.label(format!("{:.1}", v));
                            }
                        });
                        row.col(|ui| {
                            if let Some(g) = file.album_gain {
                                let color = if file.album_clip {
                                    egui::Color32::RED
                                } else {
                                    ui.style().visuals.text_color()
                                };
                                ui.colored_label(color, format!("{:+.1} dB", g));
                            }
                        });
                        row.col(|ui| {
                            if file.album_clip {
                                ui.colored_label(egui::Color32::RED, "Y");
                            }
                        });
                        row.col(|ui| {
                            render_stored_tags_cell(ui, file.stored_tags.as_ref());
                        });
                        row.col(|ui| {
                            ui.label(file.status.label());
                        });
                    });
                }
                if let Some((idx, mode)) = pending_click {
                    app.click_row(idx, mode);
                }
            });
    });
}
