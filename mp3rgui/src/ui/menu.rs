use crate::app::Mp3rgainApp;

pub fn render(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            file_menu(app, ui, ctx);
            analysis_menu(app, ui, ctx);
            modify_menu(app, ui, ctx);
            language_menu(app, ui, ctx);
            help_menu(app, ui);
        });
    });
}

fn language_menu(app: &mut Mp3rgainApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.menu_button(tr!("Language"), |ui| {
        if ui
            .selectable_label(app.ui_language.is_none(), tr!("System default"))
            .clicked()
        {
            app.set_ui_language(None, ctx);
            ui.close_menu();
        }
        // The source language, shown as the msgid. A catalogue's translation
        // of this entry is that language's own name, read below.
        if ui
            .selectable_label(app.ui_language.as_deref() == Some("en"), "English")
            .clicked()
        {
            app.set_ui_language(Some("en".to_string()), ctx);
            ui.close_menu();
        }
        for code in crate::i18n::CATALOGUES {
            if ui
                .selectable_label(
                    app.ui_language.as_deref() == Some(*code),
                    crate::i18n::native_name(code),
                )
                .clicked()
            {
                app.set_ui_language(Some((*code).to_string()), ctx);
                ui.close_menu();
            }
        }
    });
}

fn help_menu(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    ui.menu_button(tr!("Help"), |ui| {
        if ui.button(tr!("About mp3rgui")).clicked() {
            app.show_about = true;
            ui.close_menu();
        }
    });
}

fn file_menu(app: &mut Mp3rgainApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.menu_button(tr!("File"), |ui| {
        if ui.button(tr!("Add Files...")).clicked() {
            super::pick_files(app);
            ui.close_menu();
        }
        if ui.button(tr!("Add Folder...")).clicked() {
            super::pick_folder(app, false);
            ui.close_menu();
        }
        if ui.button(tr!("Add Folder (with subfolders)...")).clicked() {
            super::pick_folder(app, true);
            ui.close_menu();
        }
        ui.separator();
        if ui.button(tr!("Clear File List")).clicked() {
            app.clear_files();
            ui.close_menu();
        }
        ui.separator();
        if ui.button(tr!("Exit")).clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}

fn analysis_menu(app: &mut Mp3rgainApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.menu_button(tr!("Analysis"), |ui| {
        ui.add_enabled_ui(!app.files.is_empty() && !app.is_processing(), |ui| {
            if ui.button(tr!("Track Analysis")).clicked() {
                app.start_analyze_tracks(ctx);
                ui.close_menu();
            }
            if ui.button(tr!("Album Analysis")).clicked() {
                app.start_analyze_album(ctx);
                ui.close_menu();
            }
            if ui.button(tr!("Find Max Amplitude")).clicked() {
                app.start_find_max_amplitude(ctx);
                ui.close_menu();
            }
            ui.separator();
            if ui.button(tr!("Check Stored Tags")).clicked() {
                app.start_check_stored_tags(ctx);
                ui.close_menu();
            }
        });
    });
}

fn modify_menu(app: &mut Mp3rgainApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.menu_button(tr!("Modify Gain"), |ui| {
        ui.add_enabled_ui(!app.files.is_empty() && !app.is_processing(), |ui| {
            if ui.button(tr!("Apply Track Gain")).clicked() {
                app.start_apply_track_gain(ctx);
                ui.close_menu();
            }
            if ui.button(tr!("Apply Album Gain")).clicked() {
                app.start_apply_album_gain(ctx);
                ui.close_menu();
            }
            if ui.button(tr!("Apply Manual Gain...")).clicked() {
                app.manual_gain_modal.open = true;
                ui.close_menu();
            }
            if ui.button(tr!("Apply Channel Gain (MP3)...")).clicked() {
                app.channel_gain_modal.open = true;
                ui.close_menu();
            }
            ui.separator();
            let undo_label = if app.selected_indices.is_empty() {
                tr!("Undo All")
            } else {
                tr!("Undo Selected")
            };
            if ui.button(undo_label).clicked() {
                app.start_undo(ctx);
                ui.close_menu();
            }
            let delete_label = if app.selected_indices.is_empty() {
                tr!("Delete Stored Tags (All)...")
            } else {
                tr!("Delete Stored Tags (Selected)...")
            };
            if ui.button(delete_label).clicked() {
                app.confirm_delete_tags = true;
                ui.close_menu();
            }
        });
    });
}
