use crate::app::Mp3rgainApp;

pub fn render(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            file_menu(app, ui, ctx);
            analysis_menu(app, ui, ctx);
            modify_menu(app, ui, ctx);
        });
    });
}

fn file_menu(app: &mut Mp3rgainApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.menu_button("File", |ui| {
        if ui.button("Add Files...").clicked() {
            if let Some(paths) = rfd::FileDialog::new()
                .add_filter("Audio files", &["mp3", "m4a", "aac"])
                .pick_files()
            {
                app.add_files(paths);
            }
            ui.close_menu();
        }
        if ui.button("Add Folder...").clicked() {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                app.add_folder(folder, false);
            }
            ui.close_menu();
        }
        if ui.button("Add Folder (with subfolders)...").clicked() {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                app.add_folder(folder, true);
            }
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Clear File List").clicked() {
            app.clear_files();
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Exit").clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}

fn analysis_menu(app: &mut Mp3rgainApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.menu_button("Analysis", |ui| {
        ui.add_enabled_ui(!app.files.is_empty() && !app.is_processing, |ui| {
            if ui.button("Track Analysis").clicked() {
                app.start_analyze_tracks(ctx);
                ui.close_menu();
            }
            if ui.button("Album Analysis").clicked() {
                app.start_analyze_album(ctx);
                ui.close_menu();
            }
            if ui.button("Find Max Amplitude").clicked() {
                app.start_find_max_amplitude(ctx);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Check Stored Tags").clicked() {
                app.start_check_stored_tags(ctx);
                ui.close_menu();
            }
        });
    });
}

fn modify_menu(app: &mut Mp3rgainApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.menu_button("Modify Gain", |ui| {
        ui.add_enabled_ui(!app.files.is_empty() && !app.is_processing, |ui| {
            if ui.button("Apply Track Gain").clicked() {
                app.start_apply_track_gain(ctx);
                ui.close_menu();
            }
            if ui.button("Apply Album Gain").clicked() {
                app.start_apply_album_gain(ctx);
                ui.close_menu();
            }
            if ui.button("Apply Manual Gain...").clicked() {
                app.manual_gain_modal.open = true;
                ui.close_menu();
            }
            ui.separator();
            let undo_label = if app.selected_indices.is_empty() {
                "Undo All"
            } else {
                "Undo Selected"
            };
            if ui.button(undo_label).clicked() {
                app.start_undo(ctx);
                ui.close_menu();
            }
            let delete_label = if app.selected_indices.is_empty() {
                "Delete Stored Tags (All)..."
            } else {
                "Delete Stored Tags (Selected)..."
            };
            if ui.button(delete_label).clicked() {
                app.confirm_delete_tags = true;
                ui.close_menu();
            }
        });
    });
}
