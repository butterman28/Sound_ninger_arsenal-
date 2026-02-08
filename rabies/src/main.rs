mod audio;
mod gui;

use eframe::egui;

fn main() -> Result<(), eframe::Error> {
    let app = gui::AppState::default();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([700.0, 580.0])  // Slightly taller for new controls
            .with_min_inner_size([400.0, 400.0])
            .with_title("Audio Sampler"),
        ..Default::default()
    };

    eframe::run_native(
        "Audio Sampler",
        native_options,
        Box::new(|_cc| Box::new(app)),
    )
}