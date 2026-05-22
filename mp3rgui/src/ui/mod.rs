mod menu;
mod options;
mod status;
mod table;
mod toolbar;

use crate::app::Mp3rgainApp;

pub fn render(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    handle_dropped_files(app, ctx);
    handle_selection_shortcuts(app, ctx);
    menu::render(app, ctx);
    toolbar::render(app, ctx);
    options::render(app, ctx);
    status::render(app, ctx);
    render_central_panel(app, ctx);
    render_delete_confirm(app, ctx);
    render_manual_gain_modal(app, ctx);
    render_channel_gain_modal(app, ctx);
}

/// Modal for the `-l`-equivalent "Apply Channel Gain" action. MP3 only
/// (the apply pipeline rejects AAC files with `Error::ChannelGainOnAac`).
fn render_channel_gain_modal(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    if !app.channel_gain_modal.open {
        return;
    }
    let count = if app.selected_indices.is_empty() {
        app.files.len()
    } else {
        app.selected_indices.len()
    };
    let mut open = true;
    let mut close_via_cancel = false;
    let mut close_via_apply = false;
    egui::Window::new("Apply channel gain")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(format!("Target: {} file(s) (MP3 only)", count));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Channel:");
                ui.selectable_value(
                    &mut app.channel_gain_modal.channel,
                    mp3rgain::Channel::Left,
                    "Left",
                );
                ui.selectable_value(
                    &mut app.channel_gain_modal.channel,
                    mp3rgain::Channel::Right,
                    "Right",
                );
            });
            ui.horizontal(|ui| {
                ui.label("Steps:");
                ui.add(
                    egui::DragValue::new(&mut app.channel_gain_modal.steps)
                        .range(-64..=64)
                        .speed(1),
                );
                let db = mp3rgain::steps_to_db(app.channel_gain_modal.steps);
                ui.label(format!("= {:+.2} dB", db));
            });
            ui.label("Stereo / Dual Channel MP3s only. Joint-Stereo files will warn.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close_via_cancel = true;
                }
                let can_apply = app.channel_gain_modal.steps != 0;
                if ui
                    .add_enabled(can_apply, egui::Button::new("Apply"))
                    .clicked()
                {
                    close_via_apply = true;
                }
            });
        });
    if !open || close_via_cancel {
        app.channel_gain_modal.open = false;
    }
    if close_via_apply {
        let channel = app.channel_gain_modal.channel;
        let steps = app.channel_gain_modal.steps;
        app.channel_gain_modal.open = false;
        app.start_apply_channel_gain(ctx, channel, steps);
    }
}

/// Modal for the `-g`-equivalent "Apply Manual Gain" action.
fn render_manual_gain_modal(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    if !app.manual_gain_modal.open {
        return;
    }
    let count = if app.selected_indices.is_empty() {
        app.files.len()
    } else {
        app.selected_indices.len()
    };
    let mut open = true;
    let mut close_via_cancel = false;
    let mut close_via_apply = false;
    egui::Window::new("Apply manual gain")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(format!("Target: {} file(s)", count));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Steps:");
                ui.add(
                    egui::DragValue::new(&mut app.manual_gain_modal.steps)
                        .range(-64..=64)
                        .speed(1),
                );
                let db = mp3rgain::steps_to_db(app.manual_gain_modal.steps);
                ui.label(format!("= {:+.2} dB", db));
            });
            ui.label("1 step = 1.5 dB. Negative values lower the volume.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close_via_cancel = true;
                }
                let can_apply = app.manual_gain_modal.steps != 0;
                if ui
                    .add_enabled(can_apply, egui::Button::new("Apply"))
                    .clicked()
                {
                    close_via_apply = true;
                }
            });
        });
    if !open || close_via_cancel {
        app.manual_gain_modal.open = false;
    }
    if close_via_apply {
        let steps = app.manual_gain_modal.steps;
        app.manual_gain_modal.open = false;
        app.start_apply_manual_gain(ctx, steps);
    }
}

/// Modal dialog gating the destructive Delete Stored Tags worker.
fn render_delete_confirm(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    if !app.confirm_delete_tags {
        return;
    }
    let count = app.target_indices().len();
    let mut open = true;
    let mut close_via_cancel = false;
    let mut close_via_confirm = false;
    egui::Window::new("Delete stored tags")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(format!(
                "Delete stored ReplayGain / undo tags from {} file(s)?",
                count
            ));
            ui.label("This cannot be undone.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close_via_cancel = true;
                }
                if ui
                    .add(egui::Button::new("Delete").fill(egui::Color32::DARK_RED))
                    .clicked()
                {
                    close_via_confirm = true;
                }
            });
        });
    if !open || close_via_cancel {
        app.confirm_delete_tags = false;
    }
    if close_via_confirm {
        app.confirm_delete_tags = false;
        app.start_delete_tags(ctx);
    }
}

/// Global keyboard shortcuts for the file table.
///
/// - Cmd/Ctrl+A: select every loaded file
/// - Esc: clear the current selection
///
/// Both use `consume_key`, so egui text-edit widgets (e.g. the modal
/// numeric inputs) get first shot at the keys — Cmd+A in a DragValue's
/// edit mode selects the text inside, not the file table. We also skip
/// the Esc handler while any modal is up so the user can dismiss it
/// without also losing their selection underneath.
fn handle_selection_shortcuts(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
        app.select_all();
    }

    let modal_open =
        app.confirm_delete_tags || app.manual_gain_modal.open || app.channel_gain_modal.open;
    if !modal_open && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        app.clear_selection();
    }
}

fn handle_dropped_files(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    let dropped: Vec<std::path::PathBuf> = ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .filter_map(|f| f.path.clone())
            .collect()
    });
    if dropped.is_empty() {
        return;
    }

    // Expand directories recursively so dropping a folder behaves like
    // "Add Folder (with subfolders)". `add_files` itself only keeps regular
    // files, so directories would silently disappear without this.
    let mut paths = Vec::new();
    for path in dropped {
        if path.is_dir() {
            if let Ok(found) = mp3rgain::collect_audio_files(&path, true) {
                paths.extend(found);
            }
        } else {
            paths.push(path);
        }
    }
    app.add_files(paths);
}

fn render_central_panel(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        if app.files.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("Drag and drop MP3 files here, or use the toolbar buttons to add files");
            });
        } else {
            table::render(app, ui);
        }
    });
}
