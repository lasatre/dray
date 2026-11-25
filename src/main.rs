#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod auth;
mod backend;
mod cache;
mod config;
mod drive;
mod index;
mod model;
mod pointer;

fn main() -> eframe::Result {
    let paths = config::Paths::new().expect("could not create config/data directories");
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("GDrive Operator")
            .with_app_id("gdriveoperator")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([640.0, 400.0])
            .with_drag_and_drop(true),
        event_loop_builder: linux_backend_hook(),
        ..Default::default()
    };
    eframe::run_native(
        "gdriveoperator",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, paths)?))),
    )
}

/// winit has no file drag-and-drop on Wayland, so on Linux prefer X11 (XWayland) when it's
/// available. `GDO_WAYLAND=1` forces native Wayland (crisper on fractional scaling, no drops).
#[cfg(target_os = "linux")]
fn linux_backend_hook() -> Option<eframe::EventLoopBuilderHook> {
    let force_wayland = std::env::var_os("GDO_WAYLAND").is_some_and(|v| v != "0");
    if force_wayland || std::env::var_os("DISPLAY").is_none() {
        return None;
    }
    Some(Box::new(|builder| {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }))
}

#[cfg(not(target_os = "linux"))]
fn linux_backend_hook() -> Option<eframe::EventLoopBuilderHook> {
    None
}
