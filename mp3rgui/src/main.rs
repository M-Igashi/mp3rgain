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

    let _ = eframe::run_native(
        "mp3rgain",
        options,
        Box::new(|cc| Ok(Box::new(Mp3rgainApp::new(cc)))),
    );
}
