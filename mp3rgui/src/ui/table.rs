use crate::app::{ClickMode, Mp3rgainApp, SortColumn};
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
            (
                mp3rgain::TAG_REPLAYGAIN_TRACK_GAIN,
                view.track_gain.as_deref(),
            ),
            (
                mp3rgain::TAG_REPLAYGAIN_TRACK_PEAK,
                view.track_peak.as_deref(),
            ),
            (
                mp3rgain::TAG_REPLAYGAIN_ALBUM_GAIN,
                view.album_gain.as_deref(),
            ),
            (
                mp3rgain::TAG_REPLAYGAIN_ALBUM_PEAK,
                view.album_peak.as_deref(),
            ),
            (mp3rgain::TAG_MP3GAIN_UNDO, view.undo.as_deref()),
            (mp3rgain::TAG_MP3GAIN_MINMAX, view.minmax.as_deref()),
        ] {
            if let Some(v) = value {
                ui.label(format!("{}: {}", name, v));
            }
        }
    });
}

/// Clickable sort-header. Shows ▴ / ▾ on the active column and toggles
/// `app.sort_column` / `app.sort_descending` on click (issue #167).
/// `hover_text` is the tooltip shown when the header is hovered; pass an
/// empty string for the default "Click to sort" hint.
fn sort_header(
    ui: &mut egui::Ui,
    app: &mut Mp3rgainApp,
    label: &str,
    column: SortColumn,
    hover_text: &str,
) {
    let active = app.sort_column == Some(column);
    let arrow = if active {
        if app.sort_descending {
            " ▾"
        } else {
            " ▴"
        }
    } else {
        ""
    };
    let text = egui::RichText::new(format!("{}{}", label, arrow)).strong();
    let resp = ui.add(egui::Button::new(text).frame(false));
    let resp = if hover_text.is_empty() {
        resp.on_hover_text("Click to sort")
    } else {
        resp.on_hover_text(hover_text)
    };
    if resp.clicked() {
        app.toggle_sort(column);
    }
}

pub fn render(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    // BS.1770 modes show loudness in LUFS instead of the 89 dB-relative
    // volume (issue #272); the headers carry the unit so mixed-up scales
    // are visible at a glance.
    let lufs = app.analysis_mode.target_lufs().is_some();
    let (volume_header, album_volume_header) = if lufs {
        ("Volume (LUFS)", "Album (LUFS)")
    } else {
        ("Volume", "Album Vol")
    };
    let volume_hover = if lufs {
        "Track Analysis: BS.1770 integrated loudness in LUFS. \
         Find Max Amplitude: available headroom in dB before clipping. \
         Click to sort."
    } else {
        "Track Analysis: current loudness relative to ReplayGain reference (89 dB). \
         Find Max Amplitude: available headroom in dB before clipping. \
         Click to sort."
    };
    app.ensure_display_order();
    // Set lookup instead of `Vec::contains` per row, which made selection
    // O(rows × selected) per frame; the set allocation is reused across
    // frames instead of rebuilt (issue #190).
    app.rebuild_selection_set();
    // Defer click handling: the row closure borrows `app.files`, so it
    // can't also mutate `app.selected_indices` during the iteration.
    // Capture the requested action and apply it after the table.
    let mut pending_click: Option<(usize, ClickMode)> = None;
    let mut pending_reveal: Option<std::path::PathBuf> = None;
    // Horizontal scrolling only — vertical scrolling is the table's own,
    // so `body.rows` can virtualize off-screen rows (issue #190). An outer
    // vertical scroll area would hand the table unbounded height and force
    // every row to be laid out each frame.
    egui::ScrollArea::horizontal().show(ui, |ui| {
        let table = egui_extras::TableBuilder::new(ui)
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
                    sort_header(ui, app, "Path/File", SortColumn::Filename, "");
                });
                header.col(|ui| {
                    sort_header(ui, app, volume_header, SortColumn::Volume, volume_hover);
                });
                header.col(|ui| {
                    ui.strong("Clip");
                });
                header.col(|ui| {
                    sort_header(ui, app, "Track Gain", SortColumn::TrackGain, "");
                });
                header.col(|ui| {
                    ui.strong("Clip(T)");
                });
                header.col(|ui| {
                    sort_header(ui, app, album_volume_header, SortColumn::AlbumVolume, "");
                });
                header.col(|ui| {
                    sort_header(ui, app, "Album Gain", SortColumn::AlbumGain, "");
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
                    sort_header(ui, app, "Status", SortColumn::Status, "");
                });
            });
        // Borrowed after the header (which needs `app` mutably for sort
        // clicks); the body only reads `app`, so the cache and set can be
        // borrowed instead of cloned per frame.
        let display_order = app.display_order();
        let selected = &app.selection_set;
        table.body(|body| {
            // `rows` (vs per-row `body.row`) lays out only the rows
            // inside the viewport; fixed 18.0 height makes it a drop-in.
            body.rows(18.0, display_order.len(), |mut row| {
                let Some(&idx) = display_order.get(row.index()) else {
                    return;
                };
                let Some(file) = app.files.get(idx) else {
                    return;
                };
                let is_selected = selected.contains(&idx);
                row.set_selected(is_selected);

                row.col(|ui| {
                    let path_text = file.path.to_string_lossy();
                    // Issue #223: optionally show just the file name;
                    // the full path stays on hover either way.
                    let label_text: &str = if app.show_filename_only {
                        file.filename.as_str()
                    } else {
                        path_text.as_ref()
                    };
                    let resp = ui
                        .selectable_label(is_selected, label_text)
                        .on_hover_text(path_text.as_ref());
                    if resp.clicked() {
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
                    // Issue #161 item 4: right-click → reveal in
                    // file manager. Captured for after-loop apply.
                    resp.context_menu(|ui| {
                        if ui.button("Open file location").clicked() {
                            pending_reveal = Some(file.path.clone());
                            ui.close_menu();
                        }
                    });
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
        });
    });
    if let Some((idx, mode)) = pending_click {
        app.click_row(idx, mode);
    }
    if let Some(path) = pending_reveal {
        app.reveal_in_file_manager(&path);
    }
}
