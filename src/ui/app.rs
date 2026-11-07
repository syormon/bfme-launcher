//! Main window (port of `MainWindow`).

use egui::{Color32, Rect, Ui, pos2, vec2};

use super::offline::OfflinePage;
use super::pages::{self, PickTarget, SettingsEvent, SettingsPage};
use super::popups::{self, ConfirmAction, Popup, PopupAction};
use super::theme::{self, *};
use crate::actions::{self, ProgressShared};
use crate::hub::{self, UiEvent};
use crate::i18n::{self, t};
use crate::settings::Settings;
use crate::{bfme, platform, util};

pub const WINDOW_TITLE: &str = "BFME Launcher";
/// Logical design size of the original layout; the UI is scaled to fit it.
const DESIGN: egui::Vec2 = vec2(1700.0, 1200.0);

pub struct LauncherApp {
    tex: Textures,
    settings: Settings,
    about_open: bool,
    offline: OfflinePage,
    settings_page: Option<SettingsPage>,
    popups: Vec<Popup>,
    quit_after_popup: bool,
}

impl LauncherApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        hub::set_ctx(ctx.clone());
        egui_extras::install_image_loaders(ctx);
        theme::install_fonts(ctx);
        theme::apply_style(ctx);

        platform::set_main_window(cc);

        let settings = Settings::load();
        util::set_data_dir(&settings.workshop_dir);
        i18n::set_language(settings.launcher_language);

        let mut popups = vec![];
        let mut quit_after_popup = false;

        // The launcher must not live inside a game folder (the sync would delete it).
        let exe_dir = util::exe_dir();
        for g in 0..3 {
            let install = util::trim_end_seps(&bfme::install_path(g)).to_string();
            if bfme::is_installed(g) && util::path_within(&exe_dir, &install) {
                popups.push(Popup::message(
                    "INVALID INSTALL LOCATION",
                    "bfme-launcher is located inside one of the game's folders. This is not allowed, please move it to a different location!",
                ));
                quit_after_popup = true;
                break;
            }
        }
        if !platform::has_admin() {
            popups.push(Popup::message(
                "ADMINISTRATOR RIGHTS REQUIRED",
                "The launcher is not running as administrator. Installing games, switching patches and changing game settings will not work. Please restart it with \"Run as administrator\".",
            ));
        }

        #[allow(unused_mut)]
        let mut app = Self {
            tex: Textures::load(ctx),
            settings,
            about_open: false,
            offline: OfflinePage::new(),
            settings_page: None,
            popups,
            quit_after_popup,
        };
        // Debug builds only: open a screen directly, e.g. BFME_LAUNCHER_DEBUG_VIEW=settings-game1.
        #[cfg(debug_assertions)]
        if let Ok(view) = std::env::var("BFME_LAUNCHER_DEBUG_VIEW") {
            app.popups.clear();
            match view.as_str() {
                "about" => app.about_open = true,
                v if v.starts_with("settings") => {
                    let mut page = SettingsPage::new();
                    if let Some(g) = v.strip_prefix("settings-game").and_then(|g| g.parse().ok()) {
                        page.tab = pages::SettingsTab::Game(g);
                    }
                    app.settings_page = Some(page);
                }
                _ => {}
            }
        }
        app
    }

    fn handle_events(&mut self) {
        for ev in hub::drain() {
            match ev {
                UiEvent::Message { title, message } => self.popups.push(Popup::message(&title, &message)),
                UiEvent::Error(text) => self.popups.push(Popup::Error { text, copied: None }),
                UiEvent::ScriptedPackage(entry) => self.popups.push(Popup::scripted(*entry)),
                UiEvent::SyncFinished | UiEvent::LibraryChanged => self.offline.on_library_changed(),
                UiEvent::GameExited => self.offline.refresh_state(),
                UiEvent::Progress { title, message, handle } => self.popups.push(Popup::Progress { title, message, handle }),
                UiEvent::WorkshopMoved(dir) => {
                    self.settings.workshop_dir = dir;
                    self.settings.save();
                    self.offline.on_library_changed();
                }
            }
        }
    }

    fn update_scale(&self, ctx: &egui::Context) {
        let ppp = ctx.pixels_per_point();
        let phys = ctx.content_rect().size() * ppp;
        if phys.x > 10.0 && phys.y > 10.0 {
            let desired = (phys.x / DESIGN.x).min(phys.y / DESIGN.y).clamp(0.3, 10.0);
            if (desired - ppp).abs() > 0.002 {
                ctx.set_pixels_per_point(desired);
            }
        }
    }

    /// Settings gear and the about (info) icon in the top-right corner.
    fn corner_buttons(&mut self, ui: &mut Ui, outer: Rect) {
        let busy = hub::is_busy();
        let cy = outer.min.y + 32.0;

        let gear = Rect::from_center_size(pos2(outer.max.x - 20.0, cy), vec2(40.0, 40.0));
        let resp = hit(ui, gear, "settings_gear", !busy).on_hover_text(t("SettingsPageLauncherSectionHeader"));
        let color = if busy { W40 } else if resp.hovered() { ACCENT } else { Color32::WHITE };
        theme::gear(ui, gear.center(), 16.0, color);
        if resp.clicked() {
            self.settings_page = Some(SettingsPage::new());
        }

        let info = gear.translate(vec2(-48.0, 0.0));
        let resp = hit(ui, info, "about_info", true).on_hover_text("About");
        let color = if resp.hovered() { ACCENT } else { Color32::WHITE };
        theme::info_icon(ui, info.center(), 15.0, color);
        if resp.clicked() {
            self.about_open = true;
        }
    }

    /// Checks a folder picked in the settings and asks for confirmation before anything is moved.
    fn folder_picked(&mut self, target: PickTarget, dir: String) {
        let dir = dir.trim().trim_end_matches(['\\', '/']).to_string();
        let game_dirs: Vec<(usize, String)> = (0..3)
            .filter(|&g| bfme::is_installed(g))
            .map(|g| (g, util::trim_end_seps(&bfme::install_path(g)).to_string()))
            .collect();
        let error = |this: &mut Self, msg: &str| this.popups.push(Popup::message("CAN'T USE THIS FOLDER", msg));

        match target {
            PickTarget::Game(game) => {
                let name = bfme::GAME_NAMES[game];
                let current = util::trim_end_seps(&bfme::install_path(game)).to_string();
                let has_game = util::exists(&util::combine(&[&dir, bfme::DEFAULT_GAME_EXECUTABLES[game]]));
                if util::norm_path(&dir) == util::norm_path(&current) {
                    return;
                }
                if util::path_within(&util::exe_dir(), &dir) || util::path_within(&util::data_dir(), &dir) || util::path_within(&dir, &util::data_dir()) {
                    return error(self, "The game can't go into the launcher's own folder or the workshop folder, or contain them.");
                }
                if has_game {
                    self.popups.push(Popup::Confirm {
                        title: format!("USE THIS {name} FOLDER?"),
                        message: format!("{dir} already contains {name}. The launcher will use it from now on and leave the files in {current} untouched."),
                        confirm: "Use folder".into(),
                        action: ConfirmAction::MoveGame { game, to: dir },
                    });
                    return;
                }
                if util::dir_exists(&dir) && !util::dir_is_empty(&dir) {
                    return error(self, &format!("{dir} is not empty. Choose an empty (or new) folder to move {name} into, or the folder that already contains it."));
                }
                if util::path_within(&dir, &current) {
                    return error(self, &format!("The new folder can't be inside the current {name} folder."));
                }
                if game_dirs.iter().any(|(g, d)| *g != game && util::path_within(&dir, d)) {
                    return error(self, "That folder is inside another game's folder. Switching patches there would delete it.");
                }
                self.popups.push(Popup::Confirm {
                    title: format!("MOVE {name}?"),
                    message: format!("All files in {current} will be moved to {dir}. This can take a while when moving to another drive."),
                    confirm: "Move game".into(),
                    action: ConfirmAction::MoveGame { game, to: dir },
                });
            }
            PickTarget::CustomProton => {
                if util::exists(&util::combine(&[&dir, "proton"])) {
                    self.settings.custom_proton_path = dir;
                    self.settings.save();
                } else {
                    error(self, "That folder has no `proton` script. Pick the folder of a Proton build (the one that contains `proton` and `toolmanifest.vdf`).");
                }
            }
            PickTarget::Workshop => {
                let current = util::data_dir();
                if util::norm_path(&dir) == util::norm_path(&current) {
                    return;
                }
                if util::path_within(&dir, &current) {
                    return error(self, "The new folder can't be inside the current workshop folder.");
                }
                if game_dirs.iter().any(|(_, d)| util::path_within(&dir, d)) {
                    return error(self, "The workshop folder can't be inside a game folder. Switching patches would delete it.");
                }
                let to = if util::norm_path(&dir) == util::norm_path(&util::default_data_dir()) { String::new() } else { dir.clone() };
                self.popups.push(Popup::Confirm {
                    title: "MOVE WORKSHOP FOLDER?".into(),
                    message: format!("Your library, active patches and download cache will be moved from {current} to {dir}."),
                    confirm: "Move".into(),
                    action: ConfirmAction::MoveWorkshop { to },
                });
            }
        }
    }

    fn confirmed(&mut self, action: ConfirmAction) {
        let handle = std::sync::Arc::new(std::sync::Mutex::new(ProgressShared::default()));
        match action {
            ConfirmAction::MoveGame { game, to } => {
                self.popups.push(Popup::Progress {
                    title: format!("MOVING {}", bfme::GAME_NAMES[game]),
                    message: format!("Moving to {to}"),
                    handle: handle.clone(),
                });
                actions::spawn(move || actions::move_game(game, &to, handle));
            }
            ConfirmAction::MoveWorkshop { to } => {
                let shown = if to.is_empty() { util::default_data_dir() } else { to.clone() };
                self.popups.push(Popup::Progress {
                    title: "MOVING WORKSHOP FOLDER".into(),
                    message: format!("Moving to {shown}"),
                    handle: handle.clone(),
                });
                actions::spawn(move || actions::move_workshop(&to, handle));
            }
        }
    }

    fn close_settings(&mut self) {
        let Some(page) = self.settings_page.take() else { return };
        // Resync the active patch of every game whose language changed.
        for (g, name) in [(0usize, "BFME1"), (1, "BFME2"), (2, "RotWK")] {
            if page.needs_resync[g] {
                let handle = std::sync::Arc::new(std::sync::Mutex::new(ProgressShared::default()));
                self.popups.push(Popup::Progress {
                    title: i18n::expand("{ApplyLanguageChangePopupTitle}"),
                    message: i18n::expand(&format!("{{ApplyLanguageChangePopupDescription}} {name}")),
                    handle: handle.clone(),
                });
                actions::spawn(move || actions::resync_language(g, handle));
            }
        }
        self.offline.refresh_state();
    }
}

impl eframe::App for LauncherApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_events();
        self.update_scale(&ctx);
        let screen = ctx.content_rect();

        // Background map.
        ui.painter().image(
            self.tex.background.id(),
            screen,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );

        if let Some(page) = &mut self.settings_page {
            match page.show(ui, screen, &self.tex, &mut self.settings) {
                Some(SettingsEvent::Close) => self.close_settings(),
                Some(SettingsEvent::LanguageChanged) => {}
                Some(SettingsEvent::FolderPicked(target, dir)) => self.folder_picked(target, dir),
                #[cfg(target_os = "linux")]
                Some(SettingsEvent::UpdateProton) => actions::spawn(actions::update_proton),
                #[cfg(not(target_os = "linux"))]
                Some(SettingsEvent::UpdateProton) => {}
                Some(SettingsEvent::ResetWorkshopFolder) => {
                    self.folder_picked(PickTarget::Workshop, util::default_data_dir())
                }
                None => {}
            }
        } else if self.about_open {
            if pages::about(ui, screen, &self.tex) {
                self.about_open = false;
            }
        } else {
            let outer = screen.shrink(20.0);
            self.offline.show(ui, outer, &self.tex, &mut self.popups);
            self.corner_buttons(ui, outer);
        }

        // Popups (one at a time, queued like PopupVisualizer).
        if !self.popups.is_empty() {
            let (close, action) = popups::show(ui, &mut self.popups[0], &self.tex);
            if let Some(a) = action {
                match a {
                    PopupAction::InstallGame { game, language, location } => {
                        actions::spawn(move || actions::install_game(game, &language, &location));
                    }
                    PopupAction::LibraryChanged => self.offline.on_library_changed(),
                    PopupAction::Confirmed(c) => self.confirmed(c),
                }
            }
            if close {
                self.popups.remove(0);
                if self.popups.is_empty() && self.quit_after_popup {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        if hub::is_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}
