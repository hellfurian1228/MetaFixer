#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod cmd;
mod config;
mod devices;
mod discover;
mod winops;

use app::MetaFixerApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    #[cfg(windows)]
    {
        if winops::is_windows() && !winops::is_admin() {
            winops::elevate_and_exit();
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([920.0, 720.0])
            .with_min_inner_size([680.0, 540.0])
            .with_title("Meta Fixer"),
        ..Default::default()
    };
    eframe::run_native(
        "Meta Fixer",
        options,
        Box::new(|cc| Ok(Box::new(MetaFixerApp::new(cc)))),
    )
}
