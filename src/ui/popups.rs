//! Modal popups (port of `PopupVisualizer` and the `Popups/` folder).

use std::sync::{Arc, Mutex};
use std::time::Instant;

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Ui, pos2, vec2};

use super::theme::{self, *};
use crate::actions::{self, ProgressHandle};
use crate::i18n::{expand, t};
use crate::workshop::model::{Entry, Preview, type_key};
use crate::{bfme, util, workshop};

pub struct PackagePage {
    pub preview: Preview,
    pub in_library: bool,
    pub loading: Arc<Mutex<Option<Result<(), String>>>>,
    pub busy: bool,
    pub screenshot_offset: usize,
}

pub struct InstallGame {
    pub game: usize,
    pub language: usize,
    /// Parent folder; the game goes into `<location>\<BFME1|BFME2|RotWK>`.
    pub location: String,
    /// Result of an open folder dialog (`Some(None)` = cancelled).
    pub picked: Arc<Mutex<Option<Option<String>>>>,
    pub picking: bool,
}

/// Requirement name and status (0 complete, 1 action required, 2 blocked by a previous one).
pub type RequirementsSlot = Arc<Mutex<Option<Vec<(String, i32)>>>>;

pub struct Scripted {
    pub entry: Entry,
    pub requirements: RequirementsSlot,
    pub last_check: Option<Instant>,
    pub checking: Arc<std::sync::atomic::AtomicBool>,
}

pub enum Popup {
    Message { title: String, message: String },
    Error { text: String, copied: Option<Instant> },
    Progress { title: String, message: String, handle: ProgressHandle },
    InstallGame(Box<InstallGame>),
    Confirm { title: String, message: String, confirm: String, action: ConfirmAction },
    PackagePage(Box<PackagePage>),
    Scripted(Box<Scripted>),
}

/// Actions that need a yes from the user first.
#[derive(Clone)]
pub enum ConfirmAction {
    MoveGame { game: usize, to: String },
    /// Empty `to` = back to the default folder.
    MoveWorkshop { to: String },
}

pub enum PopupAction {
    Confirmed(ConfirmAction),
    InstallGame { game: usize, language: String, location: String },
    LibraryChanged,
}

pub const INSTALL_LANGUAGES: [&str; 9] =
    ["English", "French", "German", "Italian", "Spanish", "Swedish", "Dutch", "Polish", "Norwegian"];

impl Popup {
    pub fn message(title: &str, message: &str) -> Self {
        Popup::Message { title: expand(title), message: expand(message) }
    }

    pub fn install_game(game: usize) -> Self {
        Popup::InstallGame(Box::new(InstallGame {
            game,
            language: 0,
            location: util::exe_dir(),
            picked: Arc::new(Mutex::new(None)),
            picking: false,
        }))
    }

    pub fn package_page(preview: Preview) -> Self {
        let in_library = workshop::is_in_library(&preview.guid);
        Popup::PackagePage(Box::new(PackagePage {
            preview,
            in_library,
            loading: Arc::new(Mutex::new(None)),
            busy: false,
            screenshot_offset: 0,
        }))
    }

    pub fn scripted(entry: Entry) -> Self {
        Popup::Scripted(Box::new(Scripted {
            entry,
            requirements: Arc::new(Mutex::new(None)),
            last_check: None,
            checking: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }))
    }

    /// The acrylic style is used for the package page, the navy style for everything else.
    fn size(&self) -> egui::Vec2 {
        match self {
            Popup::PackagePage(_) => vec2(1100.0, 860.0),
            Popup::InstallGame(_) => vec2(820.0, 470.0),
            Popup::Error { .. } => vec2(800.0, 450.0),
            Popup::Confirm { .. } => vec2(700.0, 300.0),
            Popup::Scripted(_) => vec2(700.0, 520.0),
            _ => vec2(640.0, 260.0),
        }
    }
}

/// Draws the popup on top of everything. Returns (close?, action).
pub fn show(ui: &mut Ui, popup: &mut Popup, tex: &Textures) -> (bool, Option<PopupAction>) {
    let screen = ui.ctx().content_rect();
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("popup_layer"));
    let mut close = false;
    let mut action = None;

    let mut ui = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(screen));
    // Dim and block input below.
    ui.painter().rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha(170));
    let _blocker = ui.interact(screen, egui::Id::new("popup_blocker"), Sense::click_and_drag());

    let size = popup.size();
    let rect = Rect::from_center_size(screen.center(), size);
    match popup {
        Popup::PackagePage(_) => acrylic(&ui, &tex.acrylic, rect, 10),
        _ => {
            ui.painter().rect_filled(rect.expand(1.0), CornerRadius::same(7), Color32::from_black_alpha(120));
            ui.painter().rect_filled(rect, CornerRadius::same(6), Color32::from_rgba_premultiplied(10, 10, 12, 0xCC));
            ui.painter().rect_filled(rect, CornerRadius::same(6), NAVY.gamma_multiply(0.85));
        }
    }
    let inner = rect.shrink(25.0);

    match popup {
        Popup::Message { title, message } => {
            text(&ui, inner.left_top(), Align2::LEFT_TOP, title.as_str(), fk(22.0), Color32::WHITE);
            wrapped_text(&ui, Rect::from_min_max(inner.min + vec2(0.0, 45.0), inner.max - vec2(0.0, 50.0)), message, f(15.0), W60.gamma_multiply(1.4));
            let b = Rect::from_min_size(pos2(inner.max.x - 140.0, inner.max.y - 38.0), vec2(140.0, 38.0));
            if blue_button(&mut ui, b, t("GenericClose"), true).clicked() {
                close = true;
            }
        }
        Popup::Error { text: err, copied } => {
            text(&ui, inner.left_top(), Align2::LEFT_TOP, t("ErrorPopupTitle"), fk(22.0), RED);
            let body = Rect::from_min_max(inner.min + vec2(0.0, 45.0), inner.max - vec2(0.0, 55.0));
            ui.painter().rect_filled(body, CornerRadius::same(6), Color32::from_black_alpha(80));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body.shrink(10.0)));
            egui::ScrollArea::vertical().id_salt("err_scroll").show(&mut child, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(err.as_str()).font(egui::FontId::monospace(12.0)).color(W60.gamma_multiply(1.5))).wrap());
            });
            let b = Rect::from_min_size(pos2(inner.max.x - 140.0, inner.max.y - 38.0), vec2(140.0, 38.0));
            if blue_button(&mut ui, b, t("GenericClose"), true).clicked() {
                close = true;
            }
            let c = Rect::from_min_size(pos2(inner.max.x - 300.0, inner.max.y - 38.0), vec2(150.0, 38.0));
            let label = if copied.is_some_and(|c| c.elapsed().as_secs() < 3) { t("ErrorPopupErrorCopied") } else { t("ErrorPopupCopyError") };
            if flat_button(&mut ui, c, label, true).clicked() {
                util::set_clipboard(ui.ctx(), err);
                *copied = Some(Instant::now());
            }
        }
        Popup::Confirm { title, message, confirm, action: confirm_action } => {
            text(&ui, inner.left_top(), Align2::LEFT_TOP, title.as_str(), fk(22.0), Color32::WHITE);
            wrapped_text(&ui, Rect::from_min_max(inner.min + vec2(0.0, 45.0), inner.max - vec2(0.0, 50.0)), message, f(15.0), W60.gamma_multiply(1.4));
            let b = Rect::from_min_size(pos2(inner.max.x - 160.0, inner.max.y - 38.0), vec2(160.0, 38.0));
            if blue_button(&mut ui, b, confirm, true).clicked() {
                action = Some(PopupAction::Confirmed(confirm_action.clone()));
                close = true;
            }
            let c = Rect::from_min_size(pos2(inner.max.x - 310.0, inner.max.y - 38.0), vec2(140.0, 38.0));
            if flat_button(&mut ui, c, t("GenericCancel"), true).clicked() {
                close = true;
            }
        }
        Popup::Progress { title, message, handle } => {
            let g = handle.lock().unwrap();
            if g.done {
                close = true;
            }
            text(&ui, inner.left_top(), Align2::LEFT_TOP, title.as_str(), fk(22.0), Color32::WHITE);
            text(&ui, inner.left_top() + vec2(0.0, 45.0), Align2::LEFT_TOP, message.as_str(), f(15.0), W60.gamma_multiply(1.4));
            let bar = Rect::from_min_size(pos2(inner.min.x, inner.max.y - 40.0), vec2(inner.width() - 70.0, 10.0));
            progress_bar(&ui, bar, g.progress as f32 / 100.0, BLUE);
            text(&ui, pos2(inner.max.x, bar.center().y), Align2::RIGHT_CENTER, format!("{}%", g.progress), fb(15.0), Color32::WHITE);
            text_clipped(&ui, pos2(inner.min.x, inner.max.y - 5.0), Align2::LEFT_BOTTOM, &g.status, f(12.0), W60, inner.width());
        }
        Popup::InstallGame(p) => {
            let (c, a) = install_game(&mut ui, p, inner);
            close = c;
            action = a;
        }
        Popup::PackagePage(p) => {
            let (c, a) = package_page(&mut ui, p, inner);
            close = c;
            action = a;
        }
        Popup::Scripted(s) => {
            close = scripted(&mut ui, s, inner);
        }
    }
    (close, action)
}

fn install_game(ui: &mut Ui, p: &mut InstallGame, inner: Rect) -> (bool, Option<PopupAction>) {
    let mut close = false;
    let mut action = None;

    if let Some(result) = p.picked.lock().unwrap().take() {
        p.picking = false;
        if let Some(dir) = result {
            p.location = dir;
        }
    }

    text(ui, inner.left_top(), Align2::LEFT_TOP, t("InstallGamePopupTitle"), fk(22.0), Color32::WHITE);
    let img_rect = Rect::from_min_size(inner.min + vec2(0.0, 50.0), vec2(450.0, 270.0));
    theme::image(IMG_LANG_SUPPORT[p.game]).fit_to_exact_size(img_rect.size()).paint_at(ui, img_rect);
    let col = Rect::from_min_max(pos2(img_rect.max.x + 25.0, img_rect.min.y), pos2(inner.max.x, inner.max.y - 55.0));
    text(ui, col.left_top(), Align2::LEFT_TOP, t("InstallGamePopupLanguage"), fs(14.0), Color32::WHITE);
    let opts: Vec<String> = INSTALL_LANGUAGES.iter().map(|s| s.to_string()).collect();
    dropdown(ui, "install_lang", Rect::from_min_size(col.min + vec2(0.0, 25.0), vec2(col.width(), 36.0)), &opts, &mut p.language, !p.picking);

    // Install location: editable path + folder picker.
    text(ui, col.left_top() + vec2(0.0, 80.0), Align2::LEFT_TOP, t("InstallGamePopupLocation"), fs(14.0), Color32::WHITE);
    let field = Rect::from_min_size(col.min + vec2(0.0, 105.0), vec2(col.width(), 36.0));
    ui.painter().rect_filled(field, CornerRadius::same(6), W10);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(field.shrink2(vec2(10.0, 8.0))));
    if p.picking {
        child.disable();
    }
    child.add(
        egui::TextEdit::singleline(&mut p.location)
            .font(f(13.0))
            .frame(egui::Frame::NONE)
            .desired_width(field.width() - 20.0),
    );
    let browse = Rect::from_min_size(field.left_bottom() + vec2(0.0, 8.0), vec2(col.width(), 36.0));
    if flat_button(ui, browse, if p.picking { "..." } else { "Browse..." }, !p.picking).clicked() {
        p.picking = true;
        let slot = p.picked.clone();
        let start = p.location.clone();
        actions::spawn(move || {
            let mut dialog = rfd::FileDialog::new().set_title("Choose where to install the game");
            if util::dir_exists(&start) {
                dialog = dialog.set_directory(&start);
            }
            let dir = dialog.pick_folder().map(|d| d.to_string_lossy().into_owned());
            *slot.lock().unwrap() = Some(dir);
            crate::hub::repaint();
        });
    }

    let location = p.location.trim().to_string();
    let target = util::combine(&[&location, bfme::GAME_NAMES[p.game]]);
    let mut y = browse.max.y + 16.0;
    text(ui, pos2(col.min.x, y), Align2::LEFT_TOP, "Installs to:", f(12.0), W60);
    y += 18.0;
    wrapped_text(ui, Rect::from_min_size(pos2(col.min.x, y), vec2(col.width(), 34.0)), &target, f(12.0), Color32::WHITE);
    if p.game == 2 && !bfme::is_installed(1) {
        y += 36.0;
        let bfme2 = util::combine(&[&location, "BFME2"]);
        wrapped_text(ui, Rect::from_min_size(pos2(col.min.x, y), vec2(col.width(), 34.0)), &format!("+ {bfme2}"), f(12.0), W60);
    }
    let free = crate::platform::free_space(&location);
    if let Some(free) = free {
        let s = format!("{} GB {}", free / (1024 * 1024 * 1024), t("GenericFree"));
        text(ui, pos2(col.min.x, col.max.y), Align2::LEFT_BOTTOM, s, f(12.0), W60);
    }

    let valid = !location.is_empty() && std::path::Path::new(&location).is_absolute() && free.is_some();
    let b = Rect::from_min_size(pos2(inner.max.x - 140.0, inner.max.y - 38.0), vec2(140.0, 38.0));
    if blue_button(ui, b, t("GenericInstall"), valid && !p.picking).clicked() {
        action = Some(PopupAction::InstallGame {
            game: p.game,
            language: INSTALL_LANGUAGES[p.language].to_string(),
            location,
        });
        close = true;
    }
    let c = Rect::from_min_size(pos2(inner.max.x - 290.0, inner.max.y - 38.0), vec2(140.0, 38.0));
    if flat_button(ui, c, t("GenericCancel"), !p.picking).clicked() {
        close = true;
    }
    (close, action)
}

fn package_page(ui: &mut Ui, p: &mut PackagePage, inner: Rect) -> (bool, Option<PopupAction>) {
    let mut close = false;
    let mut action = None;
    let v = p.preview.clone();

    if let Some(result) = p.loading.lock().unwrap().take() {
        p.busy = false;
        match result {
            Ok(()) => {
                p.in_library = true;
                action = Some(PopupAction::LibraryChanged);
            }
            Err(e) => {
                crate::hub::send(crate::hub::UiEvent::Error(e));
                close = true;
            }
        }
    }

    // Header: artwork, title, author, main button, close.
    let art = Rect::from_min_size(inner.min, vec2(120.0, 120.0));
    egui::Image::new(&v.artwork_url).fit_to_exact_size(art.size()).corner_radius(12).paint_at(ui, art);
    text(ui, art.right_top() + vec2(20.0, 10.0), Align2::LEFT_TOP, &v.name, fk(30.0), Color32::WHITE);
    text(ui, art.right_top() + vec2(20.0, 52.0), Align2::LEFT_TOP, &v.author, f(16.0), W60);

    let main = Rect::from_min_size(art.right_top() + vec2(20.0, 82.0), vec2(220.0, 38.0));
    let label = if p.in_library { t("PackagePagePopupInYourLibrary") } else { t("PackagePagePopupAddToLibrary") };
    let resp = button(ui, main, if p.busy { "" } else { label }, fb(14.0), if p.in_library { W12 } else { BLUE }, if p.in_library { W12 } else { BLUE_DARK }, !p.in_library);
    if p.busy {
        spinner(ui, main.center(), 9.0, Color32::WHITE);
    }
    if resp.clicked() && !p.busy && !p.in_library {
        p.busy = true;
        let slot = p.loading.clone();
        let guid = v.guid.clone();
        actions::spawn(move || {
            let r = workshop::download(&guid).map(|e| workshop::library_add_or_update(&e)).map_err(|e| format!("{e:#}"));
            *slot.lock().unwrap() = Some(r);
            crate::hub::repaint();
        });
    }

    let x = Rect::from_min_size(pos2(inner.max.x - 36.0, inner.min.y), vec2(36.0, 36.0));
    let xr = hit(ui, x, "close_pkg", true);
    ui.painter().circle_filled(x.center(), 18.0, if xr.hovered() { W15 } else { W10 });
    text(ui, x.center(), Align2::CENTER_CENTER, "✕", f(16.0), Color32::WHITE);
    if xr.clicked() {
        close = true;
    }

    // Info strip.
    let strip = Rect::from_min_size(pos2(inner.min.x, art.max.y + 20.0), vec2(inner.width(), 70.0));
    ui.painter().rect_filled(strip, CornerRadius::same(10), W10);
    let cur_lang = bfme::language_to_code(&bfme::get(v.game.clamp(0, 2) as usize, bfme::Key::Language));
    let lang = if v.language.contains("EN") { v.language.clone() } else { format!("{} EN", v.language).trim_start().to_string() };
    let parts: Vec<&str> = lang.split(' ').collect();
    let lang_code = if lang.contains(&cur_lang) && !cur_lang.is_empty() { cur_lang.clone() } else { parts[0].to_string() };
    let lang_full = if parts.len() > 1 { format!("+ {} More", parts.len() - 1) } else { bfme::code_to_language(&lang_code) };
    let (size_n, size_u) = util::format_size(v.size);
    let cells = [
        (t("PackagePagePopupDownloads"), util::pretty_number(v.metadata.downloads), String::new()),
        (t("PackagePagePopupGame"), v.game_name(), String::new()),
        (t("PackagePagePopupType"), t(type_key(v.r#type)).to_string(), String::new()),
        (t("PackagePagePopupLanguage"), lang_code, lang_full),
        (t("PackagePagePopupSize"), size_n, size_u.to_string()),
    ];
    let w = strip.width() / cells.len() as f32;
    for (i, (label, value, extra)) in cells.iter().enumerate() {
        let cx = strip.min.x + w * (i as f32 + 0.5);
        text(ui, pos2(cx, strip.min.y + 14.0), Align2::CENTER_TOP, *label, f(12.0), W60);
        let vr = text(ui, pos2(cx, strip.min.y + 32.0), Align2::CENTER_TOP, value, fb(18.0), Color32::WHITE);
        if !extra.is_empty() {
            text(ui, pos2(vr.max.x + 4.0, vr.max.y - 3.0), Align2::LEFT_BOTTOM, extra, f(11.0), W60);
        }
        if i > 0 {
            let lx = strip.min.x + w * i as f32;
            ui.painter().line_segment([pos2(lx, strip.min.y + 15.0), pos2(lx, strip.max.y - 15.0)], egui::Stroke::new(1.0, W12));
        }
    }

    // Screenshots carousel.
    let sy = strip.max.y + 20.0;
    text(ui, pos2(inner.min.x, sy), Align2::LEFT_TOP, t("PackagePagePopupScreenshots"), fb(18.0), Color32::WHITE);
    let shots: Vec<String> = if v.screenshot_urls.is_empty() { vec![String::new(), String::new()] } else { v.screenshot_urls.clone() };
    let area = Rect::from_min_size(pos2(inner.min.x, sy + 32.0), vec2(inner.width(), 324.0));
    let item_w = 576.0;
    let max_off = ((shots.len() as f32 * (item_w + 10.0)) - area.width()).max(0.0);
    let target = (p.screenshot_offset as f32 * (item_w + 10.0)).clamp(0.0, max_off);
    let off = ui.ctx().animate_value_with_time(egui::Id::new(("shot_off", &v.guid)), target, 0.2);
    let clip = ui.painter().with_clip_rect(area);
    for (i, s) in shots.iter().enumerate() {
        let r = Rect::from_min_size(pos2(area.min.x + i as f32 * (item_w + 10.0) - off, area.min.y), vec2(item_w, 324.0));
        clip.rect_filled(r, CornerRadius::same(10), W10);
        if !s.is_empty() && r.intersects(area) {
            let mut sub = ui.new_child(egui::UiBuilder::new().max_rect(area));
            sub.set_clip_rect(area);
            egui::Image::new(s).fit_to_exact_size(r.size()).corner_radius(10).paint_at(&sub, r);
        }
    }
    if p.screenshot_offset > 0 {
        let b = Rect::from_center_size(pos2(area.min.x + 25.0, area.center().y), vec2(40.0, 40.0));
        let r = hit(ui, b, "shot_back", true);
        ui.painter().circle_filled(b.center(), 20.0, Color32::from_black_alpha(if r.hovered() { 200 } else { 140 }));
        text(ui, b.center(), Align2::CENTER_CENTER, "◀", f(16.0), Color32::WHITE);
        if r.clicked() {
            p.screenshot_offset -= 1;
        }
    }
    if p.screenshot_offset + 1 < shots.len() && target < max_off {
        let b = Rect::from_center_size(pos2(area.max.x - 25.0, area.center().y), vec2(40.0, 40.0));
        let r = hit(ui, b, "shot_fwd", true);
        ui.painter().circle_filled(b.center(), 20.0, Color32::from_black_alpha(if r.hovered() { 200 } else { 140 }));
        text(ui, b.center(), Align2::CENTER_CENTER, "▶", f(16.0), Color32::WHITE);
        if r.clicked() {
            p.screenshot_offset += 1;
        }
    }

    // Description + changelog.
    let dy = area.max.y + 20.0;
    let half = (inner.width() - 20.0) / 2.0;
    let desc = Rect::from_min_max(pos2(inner.min.x, dy), pos2(inner.min.x + half, inner.max.y));
    let chg = Rect::from_min_max(pos2(desc.max.x + 20.0, dy), inner.max);
    text(ui, desc.min, Align2::LEFT_TOP, t("PackagePagePopupDescription"), fb(18.0), Color32::WHITE);
    text(ui, chg.min, Align2::LEFT_TOP, t("PackagePagePopupWhatsNew"), fb(18.0), Color32::WHITE);
    for (r, id, body) in [(desc, "pkg_desc", None), (chg, "pkg_chg", Some(()))] {
        let body_rect = Rect::from_min_max(r.min + vec2(0.0, 32.0), r.max);
        ui.painter().rect_filled(body_rect, CornerRadius::same(10), W05);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body_rect.shrink(12.0)));
        egui::ScrollArea::vertical().id_salt(id).show(&mut child, |ui| {
            ui.set_width(body_rect.width() - 24.0);
            if body.is_none() {
                ui.add(egui::Label::new(egui::RichText::new(&v.description).font(f(14.0)).color(Color32::from_white_alpha(210))).wrap());
            } else {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&v.version).font(fb(14.0)));
                    ui.label(egui::RichText::new(util::time_ago(v.creation_time)).font(f(12.0)).color(W60));
                });
                ui.add(egui::Label::new(egui::RichText::new(&v.changelog).font(f(14.0)).color(Color32::from_white_alpha(210))).wrap());
            }
        });
    }
    (close, action)
}

fn scripted(ui: &mut Ui, s: &mut Scripted, inner: Rect) -> bool {
    use std::sync::atomic::Ordering;
    let mut close = false;

    // Re-check the requirements every 5 seconds.
    let due = s.last_check.is_none_or(|t| t.elapsed().as_secs() >= 5);
    if due && !s.checking.load(Ordering::SeqCst) {
        s.last_check = Some(Instant::now());
        s.checking.store(true, Ordering::SeqCst);
        let slot = s.requirements.clone();
        let checking = s.checking.clone();
        let mut entry = s.entry.clone();
        actions::spawn(move || {
            if let Ok(art) = workshop::script::run_if_scripted(&mut entry) {
                let mut list: Vec<(String, i32)> = vec![];
                for (name, ok) in &art.requirements {
                    let status = if *ok { 0 } else if list.iter().any(|(_, s)| *s == 1) { 2 } else { 1 };
                    list.push((name.clone(), status));
                }
                *slot.lock().unwrap() = Some(list);
            }
            checking.store(false, Ordering::SeqCst);
            crate::hub::repaint();
        });
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));

    let reqs = s.requirements.lock().unwrap().clone();
    if let Some(r) = &reqs
        && r.iter().all(|(_, st)| *st == 0) {
            let guid = s.entry.guid.clone();
            actions::spawn(move || actions::sync_package(&guid, true, None, None));
            return true;
        }

    let art = Rect::from_min_size(inner.min, vec2(80.0, 80.0));
    egui::Image::new(&s.entry.artwork_url).fit_to_exact_size(art.size()).corner_radius(10).paint_at(ui, art);
    text(ui, art.right_top() + vec2(15.0, 8.0), Align2::LEFT_TOP, &s.entry.name, fk(24.0), Color32::WHITE);
    text(ui, art.right_top() + vec2(15.0, 44.0), Align2::LEFT_TOP, &s.entry.author, f(14.0), W60);
    text(ui, inner.left_top() + vec2(0.0, 100.0), Align2::LEFT_TOP, t("ScriptedPackagePopupRequirements"), fb(16.0), Color32::WHITE);

    let mut y = inner.min.y + 135.0;
    match reqs {
        None => spinner(ui, pos2(inner.center().x, y + 30.0), 12.0, Color32::WHITE),
        Some(list) => {
            for (i, (name, status)) in list.iter().enumerate() {
                let (label, url) = match name.split_once('<') {
                    Some((l, u)) => (l.trim().to_string(), Some(u.trim_end_matches('>').trim().to_string())),
                    None => (name.trim().to_string(), None),
                };
                let (st, color) = match status {
                    0 => (t("ScriptedPackageRequirementComplete"), GREEN),
                    1 => (t("ScriptedPackageRequirementActionRequired"), Color32::from_rgb(0xF0, 0xA0, 0x30)),
                    _ => (t("ScriptedPackageRequirementCompletePrevious"), W40),
                };
                let lr = text(ui, pos2(inner.min.x, y), Align2::LEFT_TOP, &label, fs(15.0), Color32::WHITE);
                text(ui, pos2(inner.max.x, y), Align2::RIGHT_TOP, st, fb(13.0), color);
                if let Some(u) = url {
                    let link = Rect::from_min_size(pos2(lr.max.x + 10.0, y), vec2(24.0, lr.height()));
                    let r = hit(ui, link, ("req_link", i), true);
                    text(ui, link.left_top(), Align2::LEFT_TOP, "🔗", f(15.0), if r.hovered() { ACCENT } else { W60 });
                    if r.clicked() {
                        util::open_url(&u);
                    }
                }
                y += 40.0;
            }
        }
    }

    let b = Rect::from_min_size(pos2(inner.max.x - 140.0, inner.max.y - 38.0), vec2(140.0, 38.0));
    if flat_button(ui, b, t("GenericCancel"), true).clicked() {
        let guid = format!("original-{}", s.entry.game_name());
        actions::spawn(move || actions::sync_package(&guid, true, None, None));
        close = true;
    }
    close
}
