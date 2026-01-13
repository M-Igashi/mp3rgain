use crate::app::Mp3rgainApp;

pub fn render(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            file_menu(app, ui, ctx);
            analysis_menu(app, ui);
            modify_menu(app, ui);
            options_menu(ui);
            help_menu(ui);
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

fn analysis_menu(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    ui.menu_button("Analysis", |ui| {
        ui.add_enabled_ui(!app.files.is_empty() && !app.is_processing, |ui| {
            if ui.button("Track Analysis").clicked() {
                app.analyze_tracks();
                ui.close_menu();
            }
            if ui.button("Album Analysis").clicked() {
                app.analyze_album();
                ui.close_menu();
            }
        });
    });
}

fn modify_menu(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    ui.menu_button("Modify Gain", |ui| {
        ui.add_enabled_ui(!app.files.is_empty() && !app.is_processing, |ui| {
            if ui.button("Apply Track Gain").clicked() {
                app.apply_track_gain();
                ui.close_menu();
            }
            if ui.button("Apply Album Gain").clicked() {
                app.apply_album_gain();
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Apply Constant Gain...").clicked() {
                // TODO: Implement constant gain dialog
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Undo Gain Changes").clicked() {
                // TODO: Implement undo
                ui.close_menu();
            }
        });
    });
}

fn options_menu(ui: &mut egui::Ui) {
    ui.menu_button("Options", |ui| {
        if ui.button("Settings...").clicked() {
            // TODO: Implement settings dialog
            ui.close_menu();
        }
    });
}

fn help_menu(ui: &mut egui::Ui) {
    ui.menu_button("Help", |ui| {
        if ui.button("About mp3rgain").clicked() {
            // TODO: Implement about dialog
            ui.close_menu();
        }
    });
}
