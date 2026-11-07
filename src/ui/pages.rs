//! ABOUT overlay and the settings overlay.

use std::sync::{Arc, Mutex};

use egui::{Align2, Color32, CornerRadius, Rect, Ui, pos2, vec2};

use super::theme::{self, *};
use crate::bfme::{self, Key};
use crate::i18n::{self, t};
use crate::settings::Settings;
use crate::util;
use crate::platform;

pub const ORIGINAL_REPO: &str = "https://github.com/MarcellVokk/BfmeFoundationProject";

/// Full-window ABOUT overlay. Returns true when it should close.
pub fn about(ui: &mut Ui, screen: Rect, tex: &Textures) -> bool {
    acrylic(ui, &tex.acrylic, screen, 0);
    ui.painter().rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha(60));
    let c = screen.center() - vec2(0.0, 170.0);
    let icon = Rect::from_center_size(c, vec2(96.0, 96.0));
    theme::image(IMG_MAIN_ICON).fit_to_exact_size(icon.size()).paint_at(ui, icon);
    text(ui, c + vec2(0.0, 85.0), Align2::CENTER_CENTER, "bfme-launcher", fk(30.0), Color32::WHITE);
    text(ui, c + vec2(0.0, 117.0), Align2::CENTER_CENTER, concat!("Version ", env!("CARGO_PKG_VERSION")), f(13.0), W60);

    let y = c.y + 170.0;
    text(ui, pos2(c.x, y), Align2::CENTER_CENTER, "Forked from the All-in-One Launcher of the Bfme Foundation Project", f(16.0), Color32::WHITE);
    let galley = ui.painter().layout_no_wrap(ORIGINAL_REPO.to_string(), f(15.0), ACCENT);
    let link = Rect::from_center_size(pos2(c.x, y + 32.0), galley.size());
    let resp = hit(ui, link, "about_repo_link", true).on_hover_text("Open the original project on GitHub");
    let color = if resp.hovered() { Color32::WHITE } else { ACCENT };
    ui.painter().galley_with_override_text_color(link.min, galley, color);
    if resp.hovered() {
        ui.painter().line_segment([link.left_bottom(), link.right_bottom()], egui::Stroke::new(1.0, color));
    }
    if resp.clicked() {
        util::open_url(ORIGINAL_REPO);
    }

    let close = Rect::from_center_size(pos2(c.x, y + 110.0), vec2(160.0, 40.0));
    flat_button(ui, close, t("GenericClose"), true).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape))
}

// ---------------------------------------------------------------------------
// Settings

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    LauncherGeneral,
    Game(usize),
}

pub struct SettingsPage {
    pub tab: SettingsTab,
    installed: [bool; 3],
    resolutions: Vec<String>,
    res_idx: [usize; 3],
    lang_idx: [usize; 3],
    pub needs_resync: [bool; 3],
    /// Result of a folder dialog: (target, picked folder or None when cancelled).
    picked: PickSlot,
    picking: bool,
}

/// Shared slot a folder-dialog thread writes its result into.
type PickSlot = Arc<Mutex<Option<(PickTarget, Option<String>)>>>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickTarget {
    Workshop,
    Game(usize),
    /// Linux: folder of a Proton build to use instead of the managed GE-Proton.
    #[cfg_attr(windows, allow(dead_code))]
    CustomProton,
}

pub const GAME_LANGUAGES: [&str; 11] =
    ["English", "French", "German", "Russian", "Italian", "Spanish", "Swedish", "Turkish", "Dutch", "Polish", "Norwegian"];
const GAME_HEADERS: [&str; 3] = ["SettingsPageBFME1SectionHeader", "SettingsPageBFME2SectionHeader", "SettingsPageRotWKSectionHeader"];

pub enum SettingsEvent {
    Close,
    LanguageChanged,
    /// A new folder was picked for a game / the workshop data (not applied yet).
    FolderPicked(PickTarget, String),
    ResetWorkshopFolder,
    /// Linux: "Update" next to the managed GE-Proton.
    #[cfg_attr(windows, allow(dead_code))]
    UpdateProton,
}

impl SettingsPage {
    pub fn new() -> Self {
        let installed = [bfme::is_installed(0), bfme::is_installed(1), bfme::is_installed(2)];
        let resolutions = platform::supported_resolutions();
        let mut res_idx = [0; 3];
        let mut lang_idx = [0; 3];
        for g in 0..3 {
            if installed[g] {
                let cur = bfme::get_option(g, "Resolution");
                res_idx[g] = cur.and_then(|c| resolutions.iter().position(|r| *r == c)).unwrap_or(0);
                let lang = bfme::get(g, Key::Language);
                lang_idx[g] = GAME_LANGUAGES.iter().position(|l| l.eq_ignore_ascii_case(&lang)).unwrap_or(0);
            }
        }
        Self {
            tab: SettingsTab::LauncherGeneral,
            installed,
            resolutions,
            res_idx,
            lang_idx,
            needs_resync: [false; 3],
            picked: Arc::new(Mutex::new(None)),
            picking: false,
        }
    }

    pub fn show(&mut self, ui: &mut Ui, screen: Rect, tex: &Textures, settings: &mut Settings) -> Option<SettingsEvent> {
        let mut event = None;
        if let Some((target, dir)) = self.picked.lock().unwrap().take() {
            self.picking = false;
            if let Some(dir) = dir {
                event = Some(SettingsEvent::FolderPicked(target, dir));
            }
        }
        let can_change = !self.picking && !crate::hub::is_busy();
        // Blurred background: draw the acrylic texture over the whole window.
        acrylic(ui, &tex.acrylic, screen, 0);
        ui.painter().rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha(60));

        let area = Rect::from_center_size(screen.center(), vec2(1100.0, (screen.height() - 160.0).min(900.0)));
        let side = Rect::from_min_size(area.min, vec2(280.0, area.height()));
        let content = Rect::from_min_max(pos2(side.max.x + 40.0, area.min.y), area.max);

        let mut y = side.min.y;
        let section = |ui: &mut Ui, y: &mut f32, header: &str| {
            text(ui, pos2(side.min.x + 12.0, *y), Align2::LEFT_TOP, header, fb(13.0), W60);
            *y += 26.0;
        };
        let tab_btn = |ui: &mut Ui, this: &mut Self, y: &mut f32, tab: SettingsTab, label: &str, enabled: bool| {
            let r = Rect::from_min_size(pos2(side.min.x, *y), vec2(side.width(), 36.0));
            let resp = hit(ui, r, ("settings_tab", label, *y as i32), enabled);
            if this.tab == tab {
                ui.painter().rect_filled(r, CornerRadius::same(6), W10);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(6), W05);
            }
            text(ui, r.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, label, f(15.0), if enabled { Color32::WHITE } else { W40 });
            if resp.clicked() {
                this.tab = tab;
            }
            *y += 40.0;
        };

        section(ui, &mut y, t("SettingsPageLauncherSectionHeader"));
        tab_btn(ui, self, &mut y, SettingsTab::LauncherGeneral, t("SettingsPageLauncherSectionGeneral"), true);
        for (g, header) in GAME_HEADERS.iter().enumerate() {
            y += 14.0;
            section(ui, &mut y, t(header));
            let installed = self.installed[g];
            tab_btn(ui, self, &mut y, SettingsTab::Game(g), t("SettingsPageBFMESectionGeneral"), installed);
        }

        let close = Rect::from_min_size(pos2(side.min.x, side.max.y - 40.0), vec2(160.0, 40.0));
        if flat_button(ui, close, t("GenericClose"), true).clicked() {
            event = Some(SettingsEvent::Close);
        }

        let row = |ui: &mut Ui, y: f32, label: &str| -> Rect {
            text(ui, pos2(content.min.x, y + 20.0), Align2::LEFT_CENTER, label, f(18.0), Color32::WHITE);
            ui.painter().line_segment(
                [pos2(content.min.x, y + 52.0), pos2(content.max.x, y + 52.0)],
                egui::Stroke::new(1.0, W12),
            );
            Rect::from_min_size(pos2(content.max.x - 250.0, y), vec2(250.0, 40.0))
        };

        match self.tab {
            SettingsTab::LauncherGeneral => {
                text(ui, content.min, Align2::LEFT_TOP, t("SettingsPageLauncherSectionHeader"), fk(28.0), Color32::WHITE);
                let r = row(ui, content.min.y + 60.0, t("SettingsPageLauncherSectionGeneralLanguage"));
                let opts: Vec<String> = i18n::LANGUAGE_NAMES.iter().map(|s| s.to_string()).collect();
                let mut lang = settings.launcher_language;
                if dropdown(ui, "launcher_lang", r, &opts, &mut lang, true) {
                    settings.launcher_language = lang;
                    i18n::set_language(lang);
                    settings.save();
                    event = Some(SettingsEvent::LanguageChanged);
                }
                let current = util::data_dir();
                let custom = !settings.workshop_dir.is_empty();
                let hint = "Library, active patches and download cache";
                match self.path_row(ui, content, content.min.y + 125.0, "Workshop folder", hint, &current, PickTarget::Workshop, can_change, custom) {
                    Some(PathRowAction::Reset) => event = Some(SettingsEvent::ResetWorkshopFolder),
                    Some(PathRowAction::Open) => util::open_folder(&current),
                    None => {}
                }
                #[cfg(target_os = "linux")]
                self.runner_rows(ui, content, content.min.y + 215.0, settings, can_change, &mut event);
            }
            SettingsTab::Game(g) => {
                text(ui, content.min, Align2::LEFT_TOP, t(GAME_HEADERS[g]), fk(28.0), Color32::WHITE);
                let r = row(ui, content.min.y + 60.0, t("SettingsPageBFMESectionLanguageLanguage"));
                let opts: Vec<String> = GAME_LANGUAGES.iter().map(|s| s.to_string()).collect();
                if dropdown(ui, &format!("game_lang_{g}"), r, &opts, &mut self.lang_idx[g], true) {
                    let lang = GAME_LANGUAGES[self.lang_idx[g]];
                    if bfme::get(g, Key::Language) != lang {
                        if g == 0 {
                            let _ = bfme::set(0, Key::Language, lang);
                            self.needs_resync[0] = true;
                        } else {
                            let _ = bfme::set(1, Key::Language, lang);
                            let _ = bfme::set(2, Key::Language, lang);
                            self.lang_idx[1] = self.lang_idx[g];
                            self.lang_idx[2] = self.lang_idx[g];
                            self.needs_resync[1] = true;
                            self.needs_resync[2] = true;
                        }
                    }
                }
                let r = row(ui, content.min.y + 125.0, t("SettingsPageBFMESectionVideoResolution"));
                if !self.resolutions.is_empty() && dropdown(ui, &format!("game_res_{g}"), r, &self.resolutions.clone(), &mut self.res_idx[g], true) {
                    bfme::set_option(g, "Resolution", &self.resolutions[self.res_idx[g]]);
                }
                let r = row(ui, content.min.y + 190.0, "Windowed mode");
                let knob = Rect::from_min_size(pos2(r.max.x - 54.0, r.min.y + 7.0), vec2(54.0, 26.0));
                if toggle(ui, knob, &mut settings.windowed[g]) {
                    settings.save();
                }
                launch_options_row(ui, content, content.min.y + 255.0, g, settings);
                let current = util::trim_end_seps(&bfme::install_path(g)).to_string();
                let hint = "Pick an empty folder to move the game there, or the folder that already holds it";
                if let Some(PathRowAction::Open) =
                    self.path_row(ui, content, content.min.y + 335.0, "Installation folder", hint, &current, PickTarget::Game(g), can_change, false)
                {
                    util::open_folder(&current);
                }
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            event = Some(SettingsEvent::Close);
        }
        event
    }
}

enum PathRowAction {
    Open,
    Reset,
}

const LAUNCH_OPTIONS_HINT: &str = if cfg!(windows) {
    "Extra arguments, e.g. -nologo. Steam style also works: VAR=value %command% -nologo"
} else {
    "Steam style, e.g. PROTON_LOG=1 gamemoderun %command% -nologo"
};

/// Free-text launch options for one game (see `crate::launch_options`).
fn launch_options_row(ui: &mut Ui, content: Rect, y: f32, game: usize, settings: &mut Settings) {
    let field_w = 470.0;
    let text_w = content.width() - field_w - 20.0;
    text(ui, pos2(content.min.x, y + 4.0), Align2::LEFT_TOP, "Launch options", f(18.0), Color32::WHITE);
    text_clipped(ui, pos2(content.min.x, y + 32.0), Align2::LEFT_TOP, LAUNCH_OPTIONS_HINT, f(11.0), W60, text_w);
    let field = Rect::from_min_size(pos2(content.max.x - field_w, y + 6.0), vec2(field_w, 40.0));
    ui.painter().rect_filled(field, CornerRadius::same(6), W10);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(field.shrink2(vec2(10.0, 9.0))));
    let resp = child.add(
        egui::TextEdit::singleline(&mut settings.launch_options[game])
            .id_salt(("launch_options", game))
            .hint_text(egui::RichText::new("%command%").color(W40))
            .font(egui::FontId::monospace(13.0))
            .frame(egui::Frame::NONE)
            .desired_width(field.width() - 20.0),
    );
    if resp.changed() {
        settings.save();
    }
    ui.painter().line_segment([pos2(content.min.x, y + 62.0), pos2(content.max.x, y + 62.0)], egui::Stroke::new(1.0, W12));
}

impl SettingsPage {
    /// A settings row showing a folder with "Change..." / "Open" buttons (and "Reset" when `resettable`).
    #[allow(clippy::too_many_arguments)]
    fn path_row(
        &mut self,
        ui: &mut Ui,
        content: Rect,
        y: f32,
        label: &str,
        hint: &str,
        current: &str,
        target: PickTarget,
        enabled: bool,
        resettable: bool,
    ) -> Option<PathRowAction> {
        let mut action = None;
        let buttons_w = if resettable { 390.0 } else { 260.0 };
        let text_w = content.width() - buttons_w - 20.0;
        text(ui, pos2(content.min.x, y + 4.0), Align2::LEFT_TOP, label, f(18.0), Color32::WHITE);
        text_clipped(ui, pos2(content.min.x, y + 32.0), Align2::LEFT_TOP, current, f(13.0), W60.gamma_multiply(1.4), text_w);
        text_clipped(ui, pos2(content.min.x, y + 52.0), Align2::LEFT_TOP, hint, f(11.0), W60, text_w);
        ui.painter().line_segment([pos2(content.min.x, y + 80.0), pos2(content.max.x, y + 80.0)], egui::Stroke::new(1.0, W12));

        let mut x = content.max.x - buttons_w;
        if resettable {
            let r = Rect::from_min_size(pos2(x, y + 18.0), vec2(120.0, 38.0));
            if flat_button(ui, r, "Reset", enabled).on_hover_text("Move back to the default folder next to the launcher").clicked() {
                action = Some(PathRowAction::Reset);
            }
            x += 130.0;
        }
        let r = Rect::from_min_size(pos2(x, y + 18.0), vec2(120.0, 38.0));
        if flat_button(ui, r, if self.picking { "..." } else { "Change..." }, enabled).clicked() {
            self.picking = true;
            let slot = self.picked.clone();
            let start = current.to_string();
            crate::actions::spawn(move || {
                let mut dialog = rfd::FileDialog::new().set_title("Choose a folder");
                if util::dir_exists(&start) {
                    dialog = dialog.set_directory(&start);
                }
                let dir = dialog.pick_folder().map(|d| d.to_string_lossy().into_owned());
                *slot.lock().unwrap() = Some((target, dir));
                crate::hub::repaint();
            });
        }
        let r = Rect::from_min_size(pos2(x + 130.0, y + 18.0), vec2(120.0, 38.0));
        if flat_button(ui, r, "Open", util::dir_exists(current)).clicked() {
            action = Some(PathRowAction::Open);
        }
        action
    }
}

/// On/off switch. Returns true when it was clicked (and `value` flipped).
fn toggle(ui: &mut Ui, r: Rect, value: &mut bool) -> bool {
    let resp = hit(ui, r, ("toggle", r.min.x as i32, r.min.y as i32), true);
    let t = ui.ctx().animate_bool(resp.id, *value);
    let fill = if *value { BLUE } else if resp.hovered() { W15 } else { W12 };
    ui.painter().rect_filled(r, CornerRadius::same((r.height() / 2.0) as u8), fill);
    let knob_x = egui::lerp((r.min.x + r.height() / 2.0)..=(r.max.x - r.height() / 2.0), t);
    ui.painter().circle_filled(pos2(knob_x, r.center().y), r.height() / 2.0 - 4.0, Color32::WHITE);
    if resp.clicked() {
        *value = !*value;
        return true;
    }
    false
}

#[cfg(target_os = "linux")]
impl SettingsPage {
    /// Linux: which Proton/Wine runs the games.
    fn runner_rows(&mut self, ui: &mut Ui, content: Rect, y: f32, settings: &mut Settings, can_change: bool, event: &mut Option<SettingsEvent>) {
        use crate::settings::RunnerKind;

        let r = Rect::from_min_size(pos2(content.max.x - 250.0, y), vec2(250.0, 40.0));
        text(ui, pos2(content.min.x, y + 20.0), Align2::LEFT_CENTER, "Compatibility tool", f(18.0), Color32::WHITE);
        let opts: Vec<String> = RunnerKind::ALL.iter().map(|k| k.label().to_string()).collect();
        let mut idx = RunnerKind::ALL.iter().position(|k| *k == settings.runner).unwrap_or(0);
        if dropdown(ui, "runner_kind", r, &opts, &mut idx, can_change) {
            settings.runner = RunnerKind::ALL[idx];
            settings.save();
        }
        ui.painter().line_segment([pos2(content.min.x, y + 52.0), pos2(content.max.x, y + 52.0)], egui::Stroke::new(1.0, W12));

        let y = y + 65.0;
        match settings.runner {
            RunnerKind::ManagedProton => {
                let status = match crate::runner::managed_version() {
                    Some(v) => format!("{v}, running inside Valve's Steam Linux Runtime"),
                    None => "Not downloaded yet: GE-Proton and the Steam Linux Runtime (about 750 MB) download the first time a game starts".into(),
                };
                text(ui, pos2(content.min.x, y + 4.0), Align2::LEFT_TOP, "GE-Proton", f(18.0), Color32::WHITE);
                text_clipped(ui, pos2(content.min.x, y + 32.0), Align2::LEFT_TOP, &status, f(12.0), W60.gamma_multiply(1.4), content.width() - 160.0);
                let b = Rect::from_min_size(pos2(content.max.x - 120.0, y + 8.0), vec2(120.0, 38.0));
                let label = if crate::runner::managed_version().is_some() { "Update" } else { "Download" };
                if flat_button(ui, b, label, can_change).on_hover_text("Get the newest GE-Proton and Steam Linux Runtime").clicked() {
                    *event = Some(SettingsEvent::UpdateProton);
                }
                ui.painter().line_segment([pos2(content.min.x, y + 62.0), pos2(content.max.x, y + 62.0)], egui::Stroke::new(1.0, W12));
            }
            RunnerKind::CustomProton => {
                let current = settings.custom_proton_path.clone();
                let shown = if current.is_empty() { "No folder chosen yet".to_string() } else { current.clone() };
                let hint = "A Proton build folder, e.g. ~/.steam/steam/compatibilitytools.d/GE-Proton10-25";
                if let Some(PathRowAction::Open) = self.path_row(ui, content, y, "Proton folder", hint, &shown, PickTarget::CustomProton, can_change, false) {
                    util::open_folder(&current);
                }
            }
            RunnerKind::SystemWine => {
                let found = std::env::var_os("PATH")
                    .and_then(|p| p.to_str().map(|s| s.split(':').map(|d| std::path::Path::new(d).join("wine")).find(|w| w.is_file())))
                    .flatten();
                let status = match found {
                    Some(w) => format!("Uses {} with the launcher's own prefix", w.display()),
                    None => "No `wine` command found on this system. Install Wine or pick GE-Proton.".into(),
                };
                text(ui, pos2(content.min.x, y + 4.0), Align2::LEFT_TOP, "System Wine", f(18.0), Color32::WHITE);
                text_clipped(ui, pos2(content.min.x, y + 32.0), Align2::LEFT_TOP, &status, f(12.0), W60.gamma_multiply(1.4), content.width());
                ui.painter().line_segment([pos2(content.min.x, y + 62.0), pos2(content.max.x, y + 62.0)], egui::Stroke::new(1.0, W12));
            }
        }
    }
}
