#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod auth;
mod backend;
mod cache;
mod config;
mod drive;
mod index;
mod model;

fn main() -> eframe::Result {
    let paths = config::Paths::new().expect("could not create config/data directories");
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("GDrive Operator")
            .with_app_id("gdriveoperator")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([640.0, 400.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "gdriveoperator",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, paths)?))),
    )
}
