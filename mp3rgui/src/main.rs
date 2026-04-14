#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod ui;

use app::Mp3rgainApp;

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 650.0])
            .with_min_inner_size([700.0, 500.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "mp3rgain",
        options,
        Box::new(|cc| Ok(Box::new(Mp3rgainApp::new(cc)))),
    ) {
        let msg = format!("Failed to start mp3rgain GUI:\n\n{e}");
        eprintln!("{msg}");
        rfd::MessageDialog::new()
            .set_title("mp3rgain Error")
            .set_description(&msg)
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
}
