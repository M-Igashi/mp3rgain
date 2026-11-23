use super::theme;
use super::widgets::badge;
use crate::app::{ClickMode, FileStatus, Mp3rgainApp, SortColumn};
use crate::worker::StoredTagsView;

/// Render the "Stored RG" column for one row.
///
/// `None` → not scanned yet. `Some(empty)` → scanned, file has no tags.
/// Otherwise → show a short summary (track gain) and reveal the full
/// per-field dump on hover.
fn render_stored_tags_cell(ui: &mut egui::Ui, tags: Option<&StoredTagsView>) {
    let Some(view) = tags else { return };
    if view.is_empty() {
        ui.weak(tr!("none"))
            .on_hover_text(tr!("No stored tags found ({} container)", view.format()));
        return;
    }
    let tags = &view.tags;
    let (undo_key, minmax_key) = tags.source.undo_minmax_keys();
    let label = tags.track_gain.as_deref().unwrap_or("—");
    ui.label(label).on_hover_ui(|ui| {
        ui.label(tr!("Container: {}", view.format()));
        ui.separator();
        for (name, value) in [
            (
                mp3rgain::TAG_REPLAYGAIN_TRACK_GAIN,
                tags.track_gain.as_deref(),
            ),
            (
                mp3rgain::TAG_REPLAYGAIN_TRACK_PEAK,
                tags.track_peak.as_deref(),
            ),
            (
                mp3rgain::TAG_REPLAYGAIN_ALBUM_GAIN,
                tags.album_gain.as_deref(),
            ),
            (
                mp3rgain::TAG_REPLAYGAIN_ALBUM_PEAK,
                tags.album_peak.as_deref(),
            ),
            (
                mp3rgain::TAG_REPLAYGAIN_ALGORITHM,
                tags.algorithm.as_deref(),
            ),
            (undo_key, tags.undo.as_deref()),
            (minmax_key, tags.minmax.as_deref()),
            (
                mp3rgain::TAG_MP3GAIN_ALBUM_MINMAX,
                tags.album_minmax.as_deref(),
            ),
        ] {
            if let Some(v) = value {
                ui.label(tr!("{name}: {value}", name = name, value = v));
            }
        }
    });
}

/// Sort direction arrows. They come from egui's bundled emoji-icon-font,
/// which every platform loads: `▴` / `▾` exist only in the monospace Hack
/// font and rendered as empty boxes in proportional text (issue #489).
const SORT_ASCENDING: &str = " ⏶";
const SORT_DESCENDING: &str = " ⏷";

/// Clickable sort-header. Shows ⏶ / ⏷ on the active column and toggles
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
    let p = theme::palette(ui);
    let active = app.sort_column == Some(column);
    let mut job = egui::text::LayoutJob::default();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let color = if active { p.text_strong } else { p.muted };
    job.append(label, 0.0, egui::TextFormat::simple(font.clone(), color));
    if active {
        let arrow = if app.sort_descending {
            SORT_DESCENDING
        } else {
            SORT_ASCENDING
        };
        job.append(arrow, 0.0, egui::TextFormat::simple(font, p.accent_text));
    }
    let resp = ui.add(egui::Button::new(job).frame(false));
    let resp = if hover_text.is_empty() {
        resp.on_hover_text(tr!("Click to sort"))
    } else {
        resp.on_hover_text(hover_text)
    };
    if resp.clicked() {
        app.toggle_sort(column);
    }
}

fn header_label(ui: &mut egui::Ui, label: String) -> egui::Response {
    let p = theme::palette(ui);
    ui.label(egui::RichText::new(label).color(p.muted))
}

/// Header cell: a rule under the header row, then the contents, right
/// aligned for the numeric columns.
fn header_cell(ui: &mut egui::Ui, numeric: bool, add_contents: impl FnOnce(&mut egui::Ui)) {
    let p = theme::palette(ui);
    let rect = ui.max_rect().expand2(ui.spacing().item_spacing * 0.5);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0_f32, p.line_strong),
    );
    if numeric {
        numeric_cell(ui, add_contents);
    } else {
        add_contents(ui);
    }
}

/// Right-aligns a number. In the sizing pass that measures an auto column it
/// lays out left to right instead, or the column would measure as wide as
/// whatever room it was offered.
fn numeric_cell(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    if ui.is_sizing_pass() {
        add_contents(ui);
        ui.add_space(CELL_PAD);
        return;
    }
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.add_space(CELL_PAD);
        add_contents(ui);
    });
}

/// Room between a right-aligned number and the next column's text.
const CELL_PAD: f32 = 10.0;

/// Path cell: the folder muted, the file name in full colour, so a long list
/// of full paths still scans by name.
fn path_job(
    ui: &egui::Ui,
    file: &crate::app::FileEntry,
    filename_only: bool,
) -> egui::text::LayoutJob {
    let p = theme::palette(ui);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    let full = file.path.to_string_lossy();
    let (dir, name) = match full.strip_suffix(file.filename.as_str()) {
        _ if filename_only => ("", file.filename.as_str()),
        Some(dir) => (dir, file.filename.as_str()),
        None => ("", full.as_ref()),
    };
    job.append(dir, 0.0, egui::TextFormat::simple(font.clone(), p.muted));
    job.append(name, 0.0, egui::TextFormat::simple(font, p.text_strong));
    job
}

/// Status cell: a coloured dot, then the label.
fn status_cell(ui: &mut egui::Ui, status: &FileStatus) -> Option<egui::Response> {
    let p = theme::palette(ui);
    let color = match status {
        FileStatus::Pending => return None,
        FileStatus::Analyzed | FileStatus::Done | FileStatus::DoneFromTags => p.accent_text,
        FileStatus::DoneWithUnparsed(_) | FileStatus::DryRunPredicted { .. } => p.warn,
        FileStatus::Error(_) => p.bad,
        FileStatus::Analyzing
        | FileStatus::Applying
        | FileStatus::Undoing
        | FileStatus::NoChangesToUndo => p.muted,
    };
    let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(dot.center(), 3.5, color);
    Some(ui.label(egui::RichText::new(status.label()).color(color)))
}

/// What a row click or its context menu asked for. Applied after the table,
/// whose row closure only borrows `app`.
enum RowAction {
    Click(usize, ClickMode),
    Reveal(std::path::PathBuf),
    TrackAnalysis,
    AlbumAnalysis,
    ApplyTrackGain,
    ApplyAlbumGain,
    Undo,
    Remove,
}

/// Right-click menu of a row. The actions act on the selection, which a
/// right-click on an unselected row has just replaced with that row.
fn row_context_menu(
    ui: &mut egui::Ui,
    path: &std::path::Path,
    busy: bool,
    action: &mut Option<RowAction>,
) {
    let mut item = |ui: &mut egui::Ui, enabled: bool, label: String, then: RowAction| {
        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
            *action = Some(then);
            ui.close_menu();
        }
    };
    item(ui, !busy, tr!("Track Analysis"), RowAction::TrackAnalysis);
    item(ui, !busy, tr!("Album Analysis"), RowAction::AlbumAnalysis);
    ui.separator();
    item(
        ui,
        !busy,
        tr!("Apply Track Gain"),
        RowAction::ApplyTrackGain,
    );
    item(
        ui,
        !busy,
        tr!("Apply Album Gain"),
        RowAction::ApplyAlbumGain,
    );
    item(ui, !busy, tr!("Undo Selected"), RowAction::Undo);
    ui.separator();
    // Issue #161 item 4: reveal in file manager.
    item(
        ui,
        true,
        tr!("Open file location"),
        RowAction::Reveal(path.to_path_buf()),
    );
    ui.separator();
    item(ui, !busy, tr!("Remove"), RowAction::Remove);
}

fn click_mode(input: &egui::InputState) -> ClickMode {
    let toggle = input.modifiers.ctrl || input.modifiers.command;
    match (input.modifiers.shift, toggle) {
        (true, true) => ClickMode::RangeAdd,
        (true, false) => ClickMode::Range,
        (false, true) => ClickMode::Toggle,
        (false, false) => ClickMode::Replace,
    }
}

pub fn render(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    let p = theme::palette(ui);
    // BS.1770 modes show loudness in LUFS instead of the 89 dB-relative
    // volume (issue #272); the headers carry the unit so mixed-up scales
    // are visible at a glance.
    let lufs = app.analysis_mode.target_lufs().is_some();
    let (volume_header, album_volume_header) = if lufs {
        (tr!("Volume (LUFS)"), tr!("Album (LUFS)"))
    } else {
        (tr!("Volume"), tr!("Album Vol"))
    };
    let volume_hover = if lufs {
        tr!("Track Analysis: BS.1770 integrated loudness in LUFS. \
             Find Max Amplitude: available headroom in dB before clipping. \
             Click to sort.")
    } else {
        tr!(
            "Track Analysis: current loudness relative to ReplayGain reference (89 dB). \
             Find Max Amplitude: available headroom in dB before clipping. \
             Click to sort."
        )
    };
    app.ensure_display_order();
    // Clicks anywhere on a row select it, so labels must not take the click
    // for text selection. The column resize rules only show while hovered,
    // and a hovered row gets a fainter shade than a hovered button.
    ui.style_mut().interaction.selectable_labels = false;
    ui.visuals_mut().widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
    ui.visuals_mut().widgets.hovered.bg_fill = p.stripe.lerp_to_gamma(p.selection, 0.35);
    // Defer row actions: the row closure borrows `app.files`, so it can't
    // also mutate the selection during the iteration.
    let mut action: Option<RowAction> = None;
    let busy = app.is_processing();
    // Horizontal scrolling only — vertical scrolling is the table's own,
    // so `body.rows` can virtualize off-screen rows (issue #190). An outer
    // vertical scroll area would hand the table unbounded height and force
    // every row to be laid out each frame.
    egui::ScrollArea::horizontal().show(ui, |ui| {
        let table = egui_extras::TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::auto().at_least(220.0)) // Path/File
            .column(egui_extras::Column::auto().at_least(70.0)) // Volume
            .column(egui_extras::Column::auto().at_least(40.0)) // Clipping
            .column(egui_extras::Column::auto().at_least(80.0)) // Track Gain
            .column(egui_extras::Column::auto().at_least(40.0)) // Clip (Track)
            .column(egui_extras::Column::auto().at_least(80.0)) // Album Volume
            .column(egui_extras::Column::auto().at_least(80.0)) // Album Gain
            .column(egui_extras::Column::auto().at_least(40.0)) // Clip (Album)
            .column(egui_extras::Column::auto().at_least(80.0)) // Stored RG
            .column(egui_extras::Column::auto().at_least(70.0)) // Bitrate
            .column(egui_extras::Column::auto().at_least(60.0)) // CBR/VBR
            .column(egui_extras::Column::remainder()) // Status
            .header(30.0, |mut header| {
                header.col(|ui| {
                    header_cell(ui, false, |ui| {
                        ui.add_space(6.0);
                        sort_header(ui, app, &tr!("Path/File"), SortColumn::Filename, "");
                    });
                });
                header.col(|ui| {
                    header_cell(ui, true, |ui| {
                        sort_header(ui, app, &volume_header, SortColumn::Volume, &volume_hover);
                    });
                });
                header.col(|ui| {
                    header_cell(ui, false, |ui| {
                        header_label(ui, tr!("Clip"));
                    });
                });
                header.col(|ui| {
                    header_cell(ui, true, |ui| {
                        sort_header(ui, app, &tr!("Track Gain"), SortColumn::TrackGain, "");
                    });
                });
                header.col(|ui| {
                    header_cell(ui, false, |ui| {
                        header_label(ui, tr!("Clip(T)"));
                    });
                });
                header.col(|ui| {
                    header_cell(ui, true, |ui| {
                        sort_header(ui, app, &album_volume_header, SortColumn::AlbumVolume, "");
                    });
                });
                header.col(|ui| {
                    header_cell(ui, true, |ui| {
                        sort_header(ui, app, &tr!("Album Gain"), SortColumn::AlbumGain, "");
                    });
                });
                header.col(|ui| {
                    header_cell(ui, false, |ui| {
                        header_label(ui, tr!("Clip(A)"));
                    });
                });
                header.col(|ui| {
                    header_cell(ui, false, |ui| {
                        header_label(ui, tr!("Stored RG")).on_hover_text(tr!(
                            "Existing ReplayGain/undo tags read from the file. \
                             Run Analysis > Check Stored Tags to populate. \
                             Hover a cell for full tag details."
                        ));
                    });
                });
                header.col(|ui| {
                    header_cell(ui, true, |ui| {
                        sort_header(
                            ui,
                            app,
                            &tr!("Bitrate"),
                            SortColumn::Bitrate,
                            &tr!("Average bitrate, read when files are added. Click to sort."),
                        );
                    });
                });
                header.col(|ui| {
                    header_cell(ui, false, |ui| {
                        sort_header(
                            ui,
                            app,
                            &tr!("CBR/VBR"),
                            SortColumn::BitrateMode,
                            &tr!(
                                "MP3 only: CBR when every frame has the same bitrate, VBR otherwise. \
                                 AAC frames vary in size whatever the encoder setting, so M4A rows \
                                 stay empty. Click to sort."
                            ),
                        );
                    });
                });
                header.col(|ui| {
                    header_cell(ui, false, |ui| {
                        sort_header(ui, app, &tr!("Status"), SortColumn::Status, "");
                    });
                });
            });
        // Borrowed after the header (which needs `app` mutably for sort
        // clicks); the body only reads `app`, so the cache and set can be
        // borrowed instead of cloned per frame.
        let display_order = app.display_order();
        let selected = &app.selected_indices;
        let target = app.target_volume;
        let mode = app.analysis_mode;
        table.body(|body| {
            // `rows` (vs per-row `body.row`) lays out only the rows
            // inside the viewport; a fixed height makes it a drop-in.
            body.rows(26.0, display_order.len(), |mut row| {
                let Some(&idx) = display_order.get(row.index()) else {
                    return;
                };
                let Some(file) = app.files.get(idx) else {
                    return;
                };
                let is_selected = selected.contains(&idx);
                row.set_selected(is_selected);

                row.col(|ui| {
                    if is_selected {
                        let r = ui.max_rect();
                        let bar = egui::Rect::from_min_max(
                            egui::pos2(r.left(), r.top() - 2.0),
                            egui::pos2(r.left() + 3.0, r.bottom() + 2.0),
                        );
                        ui.painter().rect_filled(bar, 1.5, p.accent);
                    }
                    ui.add_space(6.0);
                    // Issue #223: optionally show just the file name; the
                    // full path stays on hover either way.
                    ui.label(path_job(ui, file, app.show_filename_only))
                        .on_hover_text(file.path.to_string_lossy());
                });
                let d = file.display(target, mode);
                row.col(|ui| {
                    if let Some(v) = d.volume {
                        numeric_cell(ui, |ui| {
                            ui.label(format!("{:.1}", v));
                        });
                    }
                });
                row.col(|ui| {
                    if d.clipping {
                        badge(ui, tr!("Y"), p.bad);
                    }
                });
                row.col(|ui| {
                    if let Some(g) = d.track_gain {
                        numeric_cell(ui, |ui| {
                            let text = egui::RichText::new(tr!("{} dB", format!("{:+.1}", g)));
                            ui.label(if d.track_clip { text.color(p.bad) } else { text });
                        });
                    }
                });
                row.col(|ui| {
                    if d.track_clip {
                        badge(ui, tr!("Y"), p.bad);
                    }
                });
                row.col(|ui| {
                    if let Some(v) = d.album_volume {
                        numeric_cell(ui, |ui| {
                            ui.label(format!("{:.1}", v));
                        });
                    }
                });
                row.col(|ui| {
                    if let Some(g) = d.album_gain {
                        numeric_cell(ui, |ui| {
                            let text = egui::RichText::new(tr!("{} dB", format!("{:+.1}", g)));
                            let label =
                                ui.label(if d.album_clip { text.color(p.bad) } else { text });
                            // Which album this row was grouped into (issue
                            // #344). Under By tags the grouping is otherwise
                            // invisible: a file that fell back to its folder,
                            // or two releases that merged, look like any
                            // other row.
                            if let Some(album) = &file.album_label {
                                label.on_hover_text(tr!("Album: {}", album));
                            }
                        });
                    }
                });
                row.col(|ui| {
                    if d.album_clip {
                        badge(ui, tr!("Y"), p.bad);
                    }
                });
                row.col(|ui| {
                    render_stored_tags_cell(ui, file.stored_tags.as_ref());
                });
                row.col(|ui| {
                    if let Some(bitrate) = file.bitrate {
                        numeric_cell(ui, |ui| {
                            ui.label(tr!("{} kbps", format!("{:.0}", bitrate.kbps)));
                        });
                    }
                });
                row.col(|ui| {
                    let mode = match file.bitrate_mode_label() {
                        "CBR" => tr!("CBR"),
                        "VBR" => tr!("VBR"),
                        _ => String::new(),
                    };
                    ui.label(egui::RichText::new(mode).color(p.muted));
                });
                row.col(|ui| {
                    let Some(label) = status_cell(ui, &file.status) else {
                        return;
                    };
                    match &file.status {
                        FileStatus::DoneWithUnparsed(_) => {
                            label.on_hover_text(tr!(
                                "These AAC frames could not be parsed and kept their original \
                                 gain, which can be heard as clicks. Undo restores the file. \
                                 Please report it at https://github.com/M-Igashi/mp3rgain/issues"
                            ));
                        }
                        // The reason, e.g. "HE-AAC is not supported for gain
                        // adjustment" (issue #403), is otherwise shown nowhere.
                        FileStatus::Error(message) => {
                            label.on_hover_text(message);
                        }
                        _ => {}
                    }
                });

                let response = row.response();
                if response.clicked() {
                    action = Some(RowAction::Click(idx, response.ctx.input(click_mode)));
                } else if response.secondary_clicked() && !is_selected {
                    action = Some(RowAction::Click(idx, ClickMode::Replace));
                }
                response.context_menu(|ui| {
                    row_context_menu(ui, &file.path, busy, &mut action);
                });
            });
        });
    });
    let ctx = ui.ctx().clone();
    match action {
        Some(RowAction::Click(idx, mode)) => app.click_row(idx, mode),
        Some(RowAction::Reveal(path)) => app.reveal_in_file_manager(&path),
        Some(RowAction::TrackAnalysis) => app.start_analyze_tracks(&ctx),
        Some(RowAction::AlbumAnalysis) => app.start_analyze_album(&ctx),
        Some(RowAction::ApplyTrackGain) => app.start_apply_track_gain(&ctx),
        Some(RowAction::ApplyAlbumGain) => app.start_apply_album_gain(&ctx),
        Some(RowAction::Undo) => app.start_undo(&ctx),
        Some(RowAction::Remove) => app.remove_selected(),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No system font involved: the arrows must resolve in egui's own fonts.
    #[test]
    fn sort_arrows_exist_in_bundled_fonts() {
        let fonts = egui::epaint::text::Fonts::new(1.0, 2048, egui::FontDefinitions::default());
        let id = egui::FontId::proportional(14.0);
        for arrow in [SORT_ASCENDING, SORT_DESCENDING] {
            assert!(fonts.has_glyphs(&id, arrow.trim()), "{arrow:?}");
        }
    }
}
