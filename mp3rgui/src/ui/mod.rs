mod menu;
mod options;
mod status;
mod table;
pub mod theme;
mod toolbar;
mod widgets;

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
    render_about(app, ctx);
}

/// Help > About. The only other place the version appeared was the crash
/// message, so a GUI user could not say which release they were on (#367).
fn render_about(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    egui::Window::new(tr!("About mp3rgui"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut app.show_about)
        .show(ctx, |ui| {
            ui.heading(tr!("mp3rgui {}", env!("CARGO_PKG_VERSION")));
            ui.label(tr!(
                "Lossless MP3 and AAC volume normalizer using ReplayGain"
            ));
            ui.add_space(8.0);
            ui.hyperlink(env!("CARGO_PKG_REPOSITORY"));
            ui.label(tr!("{} License", env!("CARGO_PKG_LICENSE")));
        });
}

/// Shared scaffolding for the small centered modals: fixed window chrome,
/// Cancel button, open-flag handling, and the commit button (label differs
/// per modal; accent-filled, or `commit_fill` for a destructive one). `body`
/// renders the modal contents and returns whether commit is enabled. Returns true when the commit button
/// was clicked, with the open flag already cleared, so the caller just runs
/// its action.
fn modal(
    ctx: &egui::Context,
    title: impl Into<egui::WidgetText>,
    open_flag: &mut bool,
    commit_label: impl Into<egui::WidgetText>,
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
                if ui.button(tr!("Cancel")).clicked() {
                    close_via_cancel = true;
                }
                let clicked = ui
                    .add_enabled_ui(can_commit, |ui| match commit_fill {
                        Some(fill) => {
                            widgets::filled_button(ui, commit_label, fill, egui::Color32::WHITE)
                        }
                        None => widgets::primary_button(ui, commit_label),
                    })
                    .inner
                    .clicked();
                if clicked {
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
/// (the apply pipeline rejects AAC files with `Error::ChannelGainOnAac`, and
/// mono or joint-stereo MP3s with `ChannelGainOnMono` /
/// `ChannelGainOnJointStereo`).
fn render_channel_gain_modal(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    if !app.channel_gain_modal.open {
        return;
    }
    let count = app.target_count();
    let modal_state = &mut app.channel_gain_modal;
    let committed = modal(
        ctx,
        tr!("Apply channel gain"),
        &mut modal_state.open,
        tr!("Apply"),
        None,
        |ui| {
            ui.label(tr!("Target: {} file(s) (MP3 only)", count));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(tr!("Channel:"));
                ui.selectable_value(
                    &mut modal_state.channel,
                    mp3rgain::Channel::Left,
                    tr!("Left"),
                );
                ui.selectable_value(
                    &mut modal_state.channel,
                    mp3rgain::Channel::Right,
                    tr!("Right"),
                );
            });
            ui.horizontal(|ui| {
                ui.label(tr!("Steps:"));
                ui.add(
                    egui::DragValue::new(&mut modal_state.steps)
                        .range(-64..=64)
                        .speed(1),
                );
                let db = mp3rgain::steps_to_db(modal_state.steps);
                ui.label(tr!("= {} dB", format!("{:+.2}", db)));
            });
            ui.label(tr!(
                "Stereo / Dual Channel MP3s only. Mono and joint stereo files are refused."
            ));
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
    let count = app.target_count();
    let modal_state = &mut app.manual_gain_modal;
    let committed = modal(
        ctx,
        tr!("Apply manual gain"),
        &mut modal_state.open,
        tr!("Apply"),
        None,
        |ui| {
            ui.label(tr!("Target: {} file(s)", count));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(tr!("Steps:"));
                ui.add(
                    egui::DragValue::new(&mut modal_state.steps)
                        .range(-64..=64)
                        .speed(1),
                );
                let db = mp3rgain::steps_to_db(modal_state.steps);
                ui.label(tr!("= {} dB", format!("{:+.2}", db)));
            });
            ui.label(tr!("1 step ≈ 1.5 dB. Negative values lower the volume."));
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
    let count = app.target_count();
    let committed = modal(
        ctx,
        tr!("Delete stored tags"),
        &mut app.confirm_delete_tags,
        tr!("Delete"),
        Some(egui::Color32::from_rgb(0xc6, 0x28, 0x28)),
        |ui| {
            ui.label(tr!(
                "Delete stored ReplayGain / undo tags from {} file(s)?",
                count
            ));
            ui.label(tr!("This cannot be undone."));
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
/// - Delete / Backspace: remove the selected rows from the list
/// - Cmd/Ctrl+O: Add Files
///
/// They use `consume_key`, so egui text-edit widgets (e.g. the modal
/// numeric inputs) get first shot at the keys — Cmd+A in a DragValue's
/// edit mode selects the text inside, not the file table. We also skip
/// the Esc handler while any modal is up so the user can dismiss it
/// without also losing their selection underneath, and the other two while
/// a modal is up or a text field has the keyboard.
fn handle_selection_shortcuts(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
        app.select_all();
    }

    let modal_open = app.confirm_delete_tags
        || app.show_about
        || app.manual_gain_modal.open
        || app.channel_gain_modal.open;
    if !modal_open && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        app.clear_selection();
    }
    if modal_open || ctx.wants_keyboard_input() {
        return;
    }
    if ctx.input_mut(|i| {
        i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
            || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
    }) {
        app.remove_selected();
    }
    if !app.is_processing() && ctx.input_mut(|i| i.consume_shortcut(&ADD_FILES_SHORTCUT)) {
        pick_files(app);
    }
}

pub const ADD_FILES_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O);

/// Add Files: pick audio files and load them, shared by the toolbar and the
/// File menu.
fn pick_files(app: &mut Mp3rgainApp) {
    if let Some(paths) = rfd::FileDialog::new()
        .add_filter(tr!("Audio files"), mp3rgain::SUPPORTED_EXTENSIONS)
        .pick_files()
    {
        app.add_files(paths);
    }
}

/// Add Folder: pick a folder and load its audio files, with or without
/// subfolders.
fn pick_folder(app: &mut Mp3rgainApp, recursive: bool) {
    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
        app.add_folder(folder, recursive);
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
    let frame = egui::Frame::central_panel(&ctx.style())
        .fill(theme::palette_of(ctx).base)
        .inner_margin(egui::Margin {
            left: 12,
            right: 12,
            top: 8,
            bottom: 0,
        });
    let panel = egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        if app.files.is_empty() {
            render_drop_zone(app, ui);
        } else {
            render_list_bar(app, ui);
            table::render(app, ui);
        }
    });
    render_drop_overlay(ctx, panel.response.rect);
}

/// File count on the left; the view toggle and list housekeeping on the
/// right, next to the list they act on.
fn render_list_bar(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let count = app.files.len();
        let text = if count == 1 {
            tr!("1 file")
        } else {
            tr!("{} files", count)
        };
        let p = theme::palette(ui);
        ui.label(egui::RichText::new(text).color(p.text_strong).strong());

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let idle = !app.is_processing();
            ui.add_enabled_ui(idle, |ui| {
                if ui.button(tr!("Clear All")).clicked() {
                    app.clear_files();
                }
            });
            ui.add_enabled_ui(idle && !app.selected_indices.is_empty(), |ui| {
                if ui.button(tr!("Remove")).clicked() {
                    app.remove_selected();
                }
            });
            ui.add_space(8.0);
            ui.add(widgets::Check::new(
                &mut app.show_filename_only,
                tr!("Filename only"),
            ))
            .on_hover_text(tr!(
                "Show only the file name in the Path/File column instead of \
                 the full path. The full path is still shown on hover."
            ));
        });
    });
    ui.add_space(4.0);
}

/// The empty list: one drop target with the two ways to add files.
fn render_drop_zone(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    let p = theme::palette(ui);
    let area = ui.available_rect_before_wrap().shrink(8.0);
    let size = egui::vec2(area.width().min(620.0), area.height().min(300.0));
    let card = egui::Rect::from_center_size(area.center(), size);
    ui.painter().rect(
        card,
        16.0,
        p.stripe,
        egui::Stroke::new(1.5_f32, p.line_strong),
        egui::StrokeKind::Inside,
    );
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(card.shrink(24.0))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    let ui = &mut content;
    // Icon, text and buttons, give or take, to centre them in the card.
    const CONTENT_HEIGHT: f32 = 150.0;
    ui.add_space((card.height() - 24.0 * 2.0 - CONTENT_HEIGHT).max(0.0) / 2.0);
    ui.label(egui::RichText::new("🎵").size(40.0).color(p.accent_text));
    ui.add_space(10.0);
    ui.label(
        egui::RichText::new(tr!(
            "Drag and drop MP3 files here, or use the toolbar buttons to add files"
        ))
        .size(15.0)
        .color(p.text_strong),
    );
    ui.add_space(18.0);
    // Two buttons centred as a pair: a top-down layout centres one widget
    // per row, so they go in a horizontal child measured to their width.
    ui.spacing_mut().button_padding = egui::vec2(12.0, 6.0);
    let padding = 2.0 * ui.spacing().button_padding.x;
    let font = egui::TextStyle::Button.resolve(ui.style());
    let width = ui.fonts(|f| {
        let text_width = |t: String| {
            f.layout_no_wrap(t, font.clone(), egui::Color32::WHITE)
                .size()
                .x
        };
        text_width(tr!("Add Files")) + text_width(tr!("Add Folder"))
    }) + 2.0 * padding
        + ui.spacing().item_spacing.x;
    ui.allocate_ui_with_layout(
        egui::vec2(width, 30.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            if widgets::primary_button(ui, tr!("Add Files")).clicked() {
                pick_files(app);
            }
            if ui.button(tr!("Add Folder")).clicked() {
                pick_folder(app, true);
            }
        },
    );
}

/// While files are dragged over the window, outline the list in the accent
/// so it is clear the drop will land.
fn render_drop_overlay(ctx: &egui::Context, rect: egui::Rect) {
    if ctx.input(|i| i.raw.hovered_files.is_empty()) {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("drop_overlay"),
    ));
    let accent = theme::palette_of(ctx).accent;
    painter.rect(
        rect.shrink(6.0),
        12.0,
        accent.gamma_multiply(0.12),
        egui::Stroke::new(2.0_f32, accent),
        egui::StrokeKind::Inside,
    );
}
