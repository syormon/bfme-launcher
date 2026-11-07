//! bfme-launcher — a portable Rust/egui port of the BFME All in One Launcher.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod bfme;
#[cfg(windows)]
mod dx;
mod http;
mod hub;
mod i18n;
mod launch_options;
mod platform;
#[cfg(target_os = "linux")]
mod runner;
mod settings;
mod ui;
mod util;
mod workshop;

fn main() -> eframe::Result<()> {
    // Single instance: bring the running launcher to the front instead of starting a second one.
    let Some(_guard) = platform::acquire_single_instance("bfme-launcher-single-instance") else {
        platform::focus_existing_instance(ui::app::WINDOW_TITLE);
        return Ok(());
    };

    let (w, h) = platform::work_area_logical();
    let icon = image::load_from_memory(include_bytes!("../assets/icon.ico"))
        .ok()
        .map(|i| i.to_rgba8())
        .map(|i| egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() });

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(ui::app::WINDOW_TITLE)
        .with_app_id("bfme-launcher")
        .with_inner_size([w * 0.72, h * 0.85])
        .with_min_inner_size([850.0, 600.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions { viewport, centered: true, ..Default::default() };
    eframe::run_native(ui::app::WINDOW_TITLE, options, Box::new(|cc| Ok(Box::new(ui::app::LauncherApp::new(cc)))))
}
