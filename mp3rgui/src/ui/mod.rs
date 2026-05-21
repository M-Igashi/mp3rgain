mod menu;
mod status;
mod table;
mod toolbar;

use crate::app::Mp3rgainApp;

pub fn render(app: &mut Mp3rgainApp, ctx: &egui::Context) {
    handle_dropped_files(app, ctx);
    menu::render(app, ctx);
    toolbar::render(app, ctx);
    status::render(app, ctx);
    render_central_panel(app, ctx);
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
