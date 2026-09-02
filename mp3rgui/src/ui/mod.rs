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

/// Shared scaffolding for the small centered modals: fixed window chrome,
/// Cancel button, open-flag handling, and the commit button (label and
/// optional fill differ per modal). `body` renders the modal contents and
/// returns whether commit is enabled. Returns true when the commit button
/// was clicked, with the open flag already cleared, so the caller just runs
/// its action.
fn modal(
    ctx: &egui::Context,
    title: &str,
    open_flag: &mut bool,
    commit_label: &str,
    commit_fill: Option<egui::Color32>,
    body: impl FnOnce(&mut egui::Ui) -> bool,
) -> bool {
    if !*open_flag {
        return false;
    }
    let mut open = true;
    let mut close_via_cancel = false;
    let mut close_via_commit = false;
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ctx, |ui| {
            let can_commit = body(ui);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close_via_cancel = true;
                }
                let mut button = egui::Button::new(commit_label);
                if let Some(fill) = commit_fill {
                    button = button.fill(fill);
                }
                if ui.add_enabled(can_commit, button).clicked() {
                    close_via_commit = true;
                }
            });
        });
    if !open || close_via_cancel || close_via_commit {
        *open_flag = false;
    }
    close_via_commit
}

/// Modal for the `-l`-equivalent "Apply Channel Gain" action. MP3 only
/// (the apply pipeline rejects AAC files with `Error::ChannelGainOnAac`).
fn render_channel_gain_modal(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    if !app.channel_gain_modal.open {
        return;
    }
    let count = app.target_indices().len();
    let modal_state = &mut app.channel_gain_modal;
    let committed = modal(
        ctx,
        "Apply channel gain",
        &mut modal_state.open,
        "Apply",
        None,
        |ui| {
            ui.label(format!("Target: {} file(s) (MP3 only)", count));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Channel:");
                ui.selectable_value(&mut modal_state.channel, mp3rgain::Channel::Left, "Left");
                ui.selectable_value(&mut modal_state.channel, mp3rgain::Channel::Right, "Right");
            });
            ui.horizontal(|ui| {
                ui.label("Steps:");
                ui.add(
                    egui::DragValue::new(&mut modal_state.steps)
                        .range(-64..=64)
                        .speed(1),
                );
                let db = mp3rgain::steps_to_db(modal_state.steps);
                ui.label(format!("= {:+.2} dB", db));
            });
            ui.label("Stereo / Dual Channel MP3s only. Joint-Stereo files will warn.");
            modal_state.steps != 0
        },
    );
    if committed {
        let channel = app.channel_gain_modal.channel;
        let steps = app.channel_gain_modal.steps;
        app.start_apply_channel_gain(ctx, channel, steps);
    }
}

/// Modal for the `-g`-equivalent "Apply Manual Gain" action.
fn render_manual_gain_modal(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    if !app.manual_gain_modal.open {
        return;
    }
    let count = app.target_indices().len();
    let modal_state = &mut app.manual_gain_modal;
    let committed = modal(
        ctx,
        "Apply manual gain",
        &mut modal_state.open,
        "Apply",
        None,
        |ui| {
            ui.label(format!("Target: {} file(s)", count));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Steps:");
                ui.add(
                    egui::DragValue::new(&mut modal_state.steps)
                        .range(-64..=64)
                        .speed(1),
                );
                let db = mp3rgain::steps_to_db(modal_state.steps);
                ui.label(format!("= {:+.2} dB", db));
            });
            ui.label("1 step ≈ 1.5 dB. Negative values lower the volume.");
            modal_state.steps != 0
        },
    );
    if committed {
        let steps = app.manual_gain_modal.steps;
        app.start_apply_manual_gain(ctx, steps);
    }
}

/// Modal dialog gating the destructive Delete Stored Tags worker.
fn render_delete_confirm(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    if !app.confirm_delete_tags {
        return;
    }
    let count = app.target_indices().len();
    let committed = modal(
        ctx,
        "Delete stored tags",
        &mut app.confirm_delete_tags,
        "Delete",
        Some(egui::Color32::DARK_RED),
        |ui| {
            ui.label(format!(
                "Delete stored ReplayGain / undo tags from {} file(s)?",
                count
            ));
            ui.label("This cannot be undone.");
            true
        },
    );
    if committed {
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
    let paths = mp3rgain::expand_audio_paths(&dropped).unwrap_or_default();
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
