//! Main entrypoint for lazer2stable graphical interface.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod icons;

use app::Lazer2StableApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([820.0, 600.0])
            .with_min_inner_size([500.0, 380.0])
            .with_title("lazer2stable - Migration Tool"),
        ..Default::default()
    };

    eframe::run_native(
        "lazer2stable",
        options,
        Box::new(|cc| Ok(Box::new(Lazer2StableApp::new(cc)))),
    )
}
