//! The SINGLEPLAYER page: game tabs, active patch, enhancements, launch button, library and workshop.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Align2, Color32, CornerRadius, Rect, Ui, pos2, vec2};

use super::Job;
use super::popups::Popup;
use super::theme::{self, *};
use crate::actions;
use crate::bfme;
use crate::hub;
use crate::i18n::{self, t};
use crate::util;
use crate::workshop::{self, model::*};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InnerTab {
    Library,
    Workshop,
}

struct TileState {
    preview: Preview,
    active: bool,
    installed: bool,
}

pub struct OfflinePage {
    pub game: usize,
    pub inner: InnerTab,
    active_entry: Option<Entry>,
    update_available: Arc<Mutex<Option<(String, bool)>>>,
    enhancements: Vec<Entry>,
    installed: [bool; 3],

    lib_search: String,
    lib_filter: usize,
    lib_job: Job<Vec<Preview>>,
    lib_tiles: Vec<TileState>,
    /// Latest server info for library entries (versions submenu, update detection).
    lib_latest: Arc<Mutex<HashMap<String, Preview>>>,
    lib_checked: std::collections::HashSet<String>,

    ws_search: String,
    ws_type: usize,
    ws_sort: usize,
    ws_job: Job<Result<Vec<Preview>, String>>,
    ws_tiles: Vec<Preview>,
    ws_error: bool,
    ws_in_library: HashMap<String, bool>,

    snapshot_job: Job<Result<(), String>>,
    clicked: Option<String>,
}

const LIB_FILTER_KEYS: [&str; 5] = [
    "LibraryPageFilterPatchesAndMods",
    "LibraryPageFilterEnhancements",
    "LibraryPageFilterMapPacks",
    "LibraryPageFilterSnapshots",
    "LibraryPageFilterEverything",
];
const LIB_FILTER_TYPES: [i32; 5] = [-2, 2, 3, 4, -1];
const WS_FILTER_KEYS: [&str; 4] = [
    "WorkshopPageFilterPatchesAndMods",
    "WorkshopPageFilterEnhancements",
    "WorkshopPageFilterMapPacks",
    "WorkshopPageFilterEverything",
];
const WS_FILTER_TYPES: [i32; 4] = [-2, 2, 3, -1];
const WS_SORT_KEYS: [&str; 3] = ["WorkshopPageSortByMostDownloads", "WorkshopPageSortByMostRecent", "WorkshopPageSortAlphabetical"];

const TILE: egui::Vec2 = vec2(196.0, 262.0);

impl OfflinePage {
    pub fn new() -> Self {
        let mut p = Self {
            game: 0,
            inner: InnerTab::Library,
            active_entry: None,
            update_available: Arc::new(Mutex::new(None)),
            enhancements: vec![],
            installed: [false; 3],
            lib_search: String::new(),
            lib_filter: 4,
            lib_job: Job::default(),
            lib_tiles: vec![],
            lib_latest: Arc::new(Mutex::new(HashMap::new())),
            lib_checked: Default::default(),
            ws_search: String::new(),
            ws_type: 3,
            ws_sort: 0,
            ws_job: Job::default(),
            ws_tiles: vec![],
            ws_error: false,
            ws_in_library: HashMap::new(),
            snapshot_job: Job::default(),
            clicked: None,
        };
        p.select_game(0);
        p
    }

    pub fn select_game(&mut self, game: usize) {
        self.game = game;
        self.lib_search.clear();
        self.ws_search.clear();
        self.refresh_state();
        self.query_library();
        if self.inner == InnerTab::Workshop {
            self.query_workshop();
        }
    }

    /// Re-reads active patch / enhancements / install state (after a sync etc).
    pub fn refresh_state(&mut self) {
        self.installed = [bfme::is_installed(0), bfme::is_installed(1), bfme::is_installed(2)];
        self.active_entry = workshop::active_patch(self.game);
        self.enhancements = workshop::active_enhancements(self.game).into_values().collect();
        for tile in &mut self.lib_tiles {
            tile.installed = self.installed[tile.preview.game.clamp(0, 2) as usize];
            tile.active = is_active(&tile.preview);
        }
        self.check_active_update();
    }

    fn check_active_update(&mut self) {
        *self.update_available.lock().unwrap() = None;
        if let Some(e) = &self.active_entry {
            let slot = self.update_available.clone();
            let guid = e.guid.clone();
            let version = e.version.clone();
            std::thread::spawn(move || {
                if let Ok(latest) = workshop::get_preview(&guid) {
                    let avail = latest.guid == guid && latest.version != version;
                    *slot.lock().unwrap() = Some((guid, avail));
                    hub::repaint();
                }
            });
        }
    }

    pub fn on_library_changed(&mut self) {
        self.refresh_state();
        self.query_library();
        self.ws_in_library.clear();
    }

    fn query_library(&mut self) {
        let (kw, game, ty) = (self.lib_search.clone(), self.game as i32, LIB_FILTER_TYPES[self.lib_filter]);
        self.lib_job.start(move || workshop::library_query(&kw, game, ty, 0));
    }

    fn query_workshop(&mut self) {
        let (kw, game, ty, sort) = (self.ws_search.clone(), self.game as i32, WS_FILTER_TYPES[self.ws_type], self.ws_sort as i32);
        self.ws_job.start(move || workshop::query(&kw, game, ty, sort, 0).map_err(|e| format!("{e:#}")));
    }

    fn poll_jobs(&mut self, popups: &mut Vec<Popup>) {
        if let Some(list) = self.lib_job.poll() {
            self.lib_tiles = list
                .into_iter()
                .map(|p| TileState { active: is_active(&p), installed: self.installed[p.game.clamp(0, 2) as usize], preview: p })
                .collect();
            // Check every tile for updates once (LibraryTile.CheckForUpdates).
            for tile in &self.lib_tiles {
                if self.lib_checked.insert(tile.preview.guid.clone()) {
                    let latest = self.lib_latest.clone();
                    let local = tile.preview.clone();
                    std::thread::spawn(move || {
                        if let Ok(server) = workshop::get_preview(&local.guid) {
                            if server.version != local.version && !server.guid.is_empty()
                                && let Ok(full) = workshop::download(&server.guid) {
                                    workshop::library_add_or_update(&full);
                                }
                            latest.lock().unwrap().insert(local.guid.clone(), server);
                            hub::repaint();
                        }
                    });
                }
            }
        }
        // Apply server info (newer version / metadata) to tiles.
        {
            let latest = self.lib_latest.lock().unwrap();
            for tile in &mut self.lib_tiles {
                if let Some(s) = latest.get(&tile.preview.guid)
                    && (tile.preview.metadata != s.metadata || tile.preview.version != s.version) {
                        if tile.preview.version != s.version {
                            tile.preview = s.clone();
                        } else {
                            tile.preview.metadata = s.metadata.clone();
                        }
                    }
            }
        }
        if let Some(r) = self.ws_job.poll() {
            match r {
                Ok(list) => {
                    self.ws_tiles = list;
                    self.ws_error = false;
                }
                Err(_) => {
                    self.ws_tiles.clear();
                    self.ws_error = true;
                }
            }
        }
        if let Some(r) = self.snapshot_job.poll() {
            match r {
                Ok(()) => self.query_library(),
                Err(e) => popups.push(Popup::Error { text: e, copied: None }),
            }
        }
        if self.clicked.is_some() && !hub::is_busy() {
            self.clicked = None;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, rect: Rect, tex: &Textures, popups: &mut Vec<Popup>) {
        self.poll_jobs(popups);
        let busy = hub::is_busy() || self.snapshot_job.running;

        // --- Game tabs --------------------------------------------------
        let tabs_rect = Rect::from_min_size(rect.min, vec2(3.0 * 64.0 + 10.0, 64.0));
        acrylic(ui, &tex.acrylic, tabs_rect, 15);
        for (i, img) in [IMG_BFME1, IMG_BFME2, IMG_ROTWK].into_iter().enumerate() {
            let r = Rect::from_min_size(tabs_rect.min + vec2(5.0 + i as f32 * 64.0, 5.0), vec2(64.0, 54.0));
            let resp = hit(ui, r, ("game_tab", i), !busy);
            let sel = self.game == i;
            let bg = if sel { W10 } else if resp.hovered() { W05 } else { Color32::TRANSPARENT };
            ui.painter().rect_filled(r.shrink2(vec2(5.0, 0.0)), CornerRadius::same(10), bg);
            let icon = Rect::from_center_size(r.center(), vec2(38.0, 38.0));
            let img = theme::image(img).fit_to_exact_size(icon.size());
            img.tint(if sel || resp.hovered() { Color32::WHITE } else { Color32::from_gray(170) }).paint_at(ui, icon);
            if sel {
                let u = Rect::from_center_size(pos2(r.center().x, r.max.y - 1.0), vec2(28.0, 2.0));
                ui.painter().rect_filled(u, CornerRadius::same(1), BLUE);
            }
            if resp.clicked() && !sel {
                self.select_game(i);
            }
        }

        let body = Rect::from_min_max(pos2(rect.min.x, tabs_rect.max.y + 20.0), rect.max);
        let left = Rect::from_min_max(body.min, pos2(body.min.x + 360.0, body.max.y));
        let right = Rect::from_min_max(pos2(left.max.x + 25.0, body.min.y), body.max);
        self.left_column(ui, left, tex, popups, busy);
        self.right_column(ui, right, tex, popups, busy);
    }

    fn left_column(&mut self, ui: &mut Ui, col: Rect, tex: &Textures, popups: &mut Vec<Popup>, busy: bool) {
        // Title image.
        let title = Rect::from_min_size(col.min, vec2(col.width(), 190.0));
        acrylic(ui, &tex.acrylic, title, 15);
        let titles = if i18n::language() == 1 { IMG_TITLES_DE } else { IMG_TITLES_EN };
        let ir = Rect::from_center_size(title.center(), vec2(300.0, 150.0));
        theme::image(titles[self.game]).fit_to_exact_size(ir.size()).paint_at(ui, ir);

        // Active entry (LibraryTileHorizontal).
        let active = Rect::from_min_size(pos2(col.min.x, title.max.y + 15.0), vec2(col.width(), 106.0));
        self.active_entry_tile(ui, active, tex, busy);

        // Launch button.
        let launch = Rect::from_min_max(pos2(col.min.x, col.max.y - 92.0), col.max);
        // Enabled enhancements.
        let enh = Rect::from_min_max(pos2(col.min.x, active.max.y + 15.0), pos2(col.max.x, launch.min.y - 15.0));
        acrylic(ui, &tex.acrylic, enh, 15);
        text(ui, enh.min + vec2(15.0, 15.0), Align2::LEFT_TOP, t("OfflinePageEnabledEnhancements"), fk(18.0), Color32::WHITE);
        let list = Rect::from_min_max(enh.min + vec2(10.0, 50.0), enh.max - vec2(10.0, 10.0));
        if self.enhancements.is_empty() {
            text(ui, list.center(), Align2::CENTER_CENTER, t("OfflinePageNoEnhancementsActive"), fs(16.0), W12);
        } else {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
            egui::ScrollArea::vertical().id_salt("enh_scroll").show(&mut child, |ui| {
                for e in self.enhancements.clone() {
                    let (r, _) = ui.allocate_exact_size(vec2(list.width() - 4.0, 64.0), egui::Sense::hover());
                    self.enhancement_tile(ui, r, &e, busy);
                }
            });
        }

        acrylic(ui, &tex.acrylic, launch, 15);
        self.launch_button(ui, launch.shrink(10.0), popups, busy);
    }

    fn active_entry_tile(&mut self, ui: &mut Ui, r: Rect, tex: &Textures, busy: bool) {
        acrylic(ui, &tex.acrylic, r, 15);
        let sync = hub::sync_state();
        let syncing_this = sync.as_ref().and_then(|s| s.entry.as_ref()).filter(|e| e.game_idx() == self.game && is_patch_like(e.r#type));
        let entry = syncing_this.cloned().or_else(|| self.active_entry.clone());
        let Some(e) = entry else {
            text(ui, r.center(), Align2::CENTER_CENTER, t("LibraryTileHorizontalNoPatchSelected"), fs(16.0), W12);
            return;
        };
        let installed = self.installed[e.game_idx()];
        let alpha = if installed { 1.0 } else { 0.5 };
        let inner = r.shrink(14.0);
        let art = Rect::from_min_size(inner.min, vec2(inner.height(), inner.height()));
        let mut img = egui::Image::new(&e.artwork_url).fit_to_exact_size(art.size()).corner_radius(10);
        if !installed {
            img = img.tint(Color32::from_gray(110));
        }
        img.paint_at(ui, art);
        let tx = art.max.x + 12.0;
        let tw = inner.max.x - tx;
        text_clipped(ui, pos2(tx, inner.min.y), Align2::LEFT_TOP, &e.name, fk(18.0), Color32::WHITE.gamma_multiply(alpha), tw);
        text_clipped(ui, pos2(tx, inner.min.y + 26.0), Align2::LEFT_TOP, &format!("{} · {}", e.version, e.author), f(12.0), W60, tw);
        let tag = text(ui, pos2(tx + 8.0, inner.max.y - 12.0), Align2::LEFT_CENTER, t(type_key(e.r#type)), fb(11.0), Color32::WHITE);
        ui.painter().rect_filled(tag.expand2(vec2(8.0, 3.0)), CornerRadius::same(8), W12);
        text(ui, pos2(tx + 8.0, inner.max.y - 12.0), Align2::LEFT_CENTER, t(type_key(e.r#type)), fb(11.0), Color32::WHITE);

        let btn = Rect::from_min_size(pos2(inner.max.x - 110.0, inner.max.y - 24.0), vec2(110.0, 24.0));
        if syncing_this.is_some() {
            spinner(ui, btn.right_center() - vec2(12.0, 0.0), 8.0, Color32::WHITE);
            text(ui, btn.right_center() - vec2(28.0, 0.0), Align2::RIGHT_CENTER, t("LibraryTileSyncing"), fb(12.0), W60);
        } else {
            let update = self.update_available.lock().unwrap().as_ref().is_some_and(|(g, a)| *a && *g == e.guid);
            let label = if update { t("LibraryTileHorizontalUpdate") } else { t("LibraryTileHorizontalSyncAgain") };
            let fill = if update { GREEN } else { W10 };
            if button(ui, btn, label, fb(12.0), fill, if update { GREEN.gamma_multiply(0.85) } else { W15 }, !busy && installed).clicked() {
                *self.update_available.lock().unwrap() = None;
                let guid = e.guid.clone();
                actions::spawn(move || actions::sync_package(&guid, false, None, None));
            }
        }
    }

    fn enhancement_tile(&mut self, ui: &mut Ui, r: Rect, e: &Entry, busy: bool) {
        ui.painter().rect_filled(r, CornerRadius::same(12), W10);
        let inner = r.shrink(8.0);
        let art = Rect::from_min_size(inner.min, vec2(inner.height(), inner.height()));
        egui::Image::new(&e.artwork_url).fit_to_exact_size(art.size()).corner_radius(8).paint_at(ui, art);
        let tx = art.max.x + 10.0;
        let tw = inner.max.x - tx - 40.0;
        text_clipped(ui, pos2(tx, inner.min.y + 2.0), Align2::LEFT_TOP, &e.name, fb(14.0), Color32::WHITE, tw);
        text_clipped(ui, pos2(tx, inner.min.y + 24.0), Align2::LEFT_TOP, &format!("{} · {} · {}", t(type_key(e.r#type)), e.version, e.author), f(11.0), W60, tw);
        let x = Rect::from_center_size(pos2(inner.max.x - 14.0, inner.center().y), vec2(28.0, 28.0));
        let resp = hit(ui, x, ("disable_enh", &e.guid), !busy).on_hover_text("Disable");
        ui.painter().circle_filled(x.center(), 14.0, if resp.hovered() { RED.gamma_multiply(0.6) } else { W10 });
        text(ui, x.center(), Align2::CENTER_CENTER, "✕", f(13.0), Color32::WHITE);
        if resp.clicked() {
            let guid = e.guid.clone();
            actions::spawn(move || actions::disable_enhancement(&guid));
        }
    }

    fn launch_button(&mut self, ui: &mut Ui, r: Rect, popups: &mut Vec<Popup>, _busy: bool) {
        if let Some(s) = hub::sync_state() {
            ui.painter().rect_filled(r, CornerRadius::same(9), BLUE.gamma_multiply(0.4));
            let bar = Rect::from_min_size(pos2(r.min.x + 15.0, r.min.y + 20.0), vec2(r.width() - 90.0, 8.0));
            let p = ui.ctx().animate_value_with_time(egui::Id::new("launch_progress"), s.progress as f32 / 100.0, 0.5);
            progress_bar(ui, bar, p, Color32::WHITE);
            text(ui, pos2(r.max.x - 15.0, bar.center().y), Align2::RIGHT_CENTER, format!("{}%", s.progress), fb(16.0), Color32::WHITE);
            text_clipped(ui, pos2(r.min.x + 15.0, r.max.y - 16.0), Align2::LEFT_CENTER, &s.status, f(13.0), W60.gamma_multiply(1.4), r.width() - 30.0);
            return;
        }
        let installed = self.installed[self.game];
        let label = if installed { t("GenericPlay") } else { t("GenericInstall") };
        if button(ui, r, label, fk(22.0), BLUE, BLUE_DARK, !hub::is_busy()).clicked() {
            if installed {
                let g = self.game;
                actions::spawn(move || actions::launch_game(g));
            } else {
                popups.push(Popup::install_game(self.game));
            }
        }
    }

    fn right_column(&mut self, ui: &mut Ui, col: Rect, tex: &Textures, popups: &mut Vec<Popup>, busy: bool) {
        // Library / Workshop pill tabs.
        let tabs = Rect::from_min_size(col.min, vec2(246.0, 46.0));
        acrylic(ui, &tex.acrylic, tabs, 23);
        let mut x = tabs.min.x + 5.0;
        for (tab, key) in [(InnerTab::Library, "OfflinePageLibraryTab"), (InnerTab::Workshop, "OfflinePageWorkshopTab")] {
            let r = Rect::from_min_size(pos2(x, tabs.min.y + 5.0), vec2(116.0, 36.0));
            let resp = hit(ui, r, ("inner_tab", key), !busy);
            if self.inner == tab {
                ui.painter().rect_filled(r, CornerRadius::same(18), W12);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(18), W05);
            }
            text(ui, r.center(), Align2::CENTER_CENTER, t(key), fb(14.0), Color32::WHITE);
            if resp.clicked() && self.inner != tab {
                self.inner = tab;
                match tab {
                    InnerTab::Library => {
                        self.lib_search.clear();
                        self.query_library();
                    }
                    InnerTab::Workshop => {
                        self.ws_search.clear();
                        self.query_workshop();
                    }
                }
            }
            x += 118.0;
        }

        let content = Rect::from_min_max(pos2(col.min.x, tabs.max.y + 15.0), col.max);
        match self.inner {
            InnerTab::Library => self.library(ui, content, popups, busy),
            InnerTab::Workshop => self.workshop(ui, content, popups, busy),
        }
    }

    fn toolbar_search(ui: &mut Ui, r: Rect, value: &mut String, placeholder: &str, enabled: bool) -> bool {
        ui.painter().rect_filled(r, CornerRadius::same(9), W10);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(vec2(12.0, 8.0))));
        if !enabled {
            child.disable();
        }
        let resp = child.add(
            egui::TextEdit::singleline(value)
                .hint_text(egui::RichText::new(placeholder).color(W40))
                .font(f(15.0))
                .frame(egui::Frame::NONE)
                .desired_width(r.width() - 24.0),
        );
        resp.changed()
    }

    fn library(&mut self, ui: &mut Ui, r: Rect, popups: &mut Vec<Popup>, busy: bool) {
        let bar_h = 40.0;
        let search = Rect::from_min_size(r.min, vec2(330.0, bar_h));
        if Self::toolbar_search(ui, search, &mut self.lib_search, t("LibraryPageSearchPlaceholder"), !busy) {
            self.query_library();
        }
        let opts: Vec<String> = LIB_FILTER_KEYS.iter().map(|k| t(k).to_string()).collect();
        let fr = Rect::from_min_size(pos2(search.max.x + 10.0, r.min.y), vec2(220.0, bar_h));
        if dropdown(ui, "lib_filter", fr, &opts, &mut self.lib_filter, !busy) {
            self.query_library();
        }
        let reload = Rect::from_min_size(pos2(fr.max.x + 10.0, r.min.y), vec2(bar_h, bar_h));
        if button(ui, reload, "⟳", f(18.0), W10, W15, !busy).on_hover_text("Reload").clicked() {
            self.lib_checked.clear();
            self.query_library();
            self.check_active_update();
        }
        let snap = Rect::from_min_size(pos2(r.max.x - 190.0, r.min.y), vec2(190.0, bar_h));
        let snap_running = self.snapshot_job.running;
        if button(ui, snap, if snap_running { "" } else { t("LibraryPageMakeSnapshot") }, fb(13.0), W10, W15, !busy && self.installed[self.game]).clicked() {
            let g = self.game;
            self.snapshot_job.start(move || {
                let _b = hub::Busy::new();
                workshop::sync::create_snapshot(g).map(|e| workshop::library_add_or_update(&e)).map_err(|e| format!("{e:#}"))
            });
        }
        if snap_running {
            spinner(ui, snap.center(), 9.0, Color32::WHITE);
        }

        let grid = Rect::from_min_max(pos2(r.min.x, r.min.y + bar_h + 15.0), r.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(grid));
        let cols = ((grid.width() + 10.0) / (TILE.x + 10.0)).floor().max(1.0) as usize;
        let show_empty = self.lib_filter != 3;
        let count = self.lib_tiles.len() + usize::from(show_empty);
        let rows = count.div_ceil(cols);
        let mut action: Option<TileAction> = None;
        egui::ScrollArea::vertical().id_salt(("lib_scroll", self.game)).show(&mut child, |ui| {
            ui.set_width(grid.width());
            let (area, _) = ui.allocate_exact_size(vec2(grid.width(), rows as f32 * (TILE.y + 10.0)), egui::Sense::hover());
            for i in 0..count {
                let rr = Rect::from_min_size(
                    area.min + vec2((i % cols) as f32 * (TILE.x + 10.0), (i / cols) as f32 * (TILE.y + 10.0)),
                    TILE,
                );
                if !ui.is_rect_visible(rr) {
                    continue;
                }
                if i < self.lib_tiles.len() {
                    if let Some(a) = self.library_tile(ui, rr, i, busy) {
                        action = Some(a);
                    }
                } else if empty_tile(ui, rr, busy) {
                    action = Some(TileAction::ShowWorkshop);
                }
            }
        });
        if let Some(a) = action {
            self.apply(a, popups);
        }
    }

    fn library_tile(&mut self, ui: &mut Ui, r: Rect, idx: usize, busy: bool) -> Option<TileAction> {
        let tile = &self.lib_tiles[idx];
        let p = tile.preview.clone();
        let enabled = tile.installed && !busy;
        let resp = hit(ui, r, ("lib_tile", &p.guid), enabled);
        let alpha = if tile.installed { 1.0 } else { 0.5 };
        ui.painter().rect_filled(r, CornerRadius::same(12), W12.gamma_multiply(alpha));
        if resp.hovered() {
            ui.painter().rect_filled(r, CornerRadius::same(12), W05);
        }
        let art = Rect::from_min_size(r.min + vec2(6.0, 6.0), vec2(r.width() - 12.0, r.width() - 12.0));
        let mut img = egui::Image::new(&p.artwork_url).fit_to_exact_size(art.size()).corner_radius(9);
        if !tile.installed {
            img = img.tint(Color32::from_gray(100));
        }
        ui.painter().rect_filled(art, CornerRadius::same(9), W05);
        img.paint_at(ui, art);

        let tx = r.min.x + 12.0;
        let tw = r.width() - 24.0;
        text_clipped(ui, pos2(tx, art.max.y + 8.0), Align2::LEFT_TOP, &p.name, fb(14.0), Color32::WHITE.gamma_multiply(alpha), tw);
        text_clipped(ui, pos2(tx, art.max.y + 28.0), Align2::LEFT_TOP, &format!("{} · {}", p.version, p.author), f(11.0), W60.gamma_multiply(alpha), tw);

        let bottom = r.max.y - 14.0;
        let sync = hub::sync_state();
        let syncing = sync.as_ref().and_then(|s| s.entry.as_ref().map(|e| (e.guid == p.guid, s.progress))).filter(|(m, _)| *m);
        let loading = syncing.is_some() || self.clicked.as_deref() == Some(&p.guid);
        if loading && is_patch_like(p.r#type) {
            let prog = syncing.map(|(_, p)| p).unwrap_or(0);
            let bar = Rect::from_min_size(pos2(tx, bottom - 3.0), vec2(tw - 40.0, 6.0));
            progress_bar(ui, bar, prog as f32 / 100.0, BLUE);
            text(ui, pos2(r.max.x - 12.0, bottom), Align2::RIGHT_CENTER, format!("{prog}%"), fb(11.0), Color32::WHITE);
        } else {
            let tag = t(type_key(p.r#type));
            let tr = text(ui, pos2(tx + 8.0, bottom), Align2::LEFT_CENTER, tag, fb(10.0), Color32::TRANSPARENT);
            ui.painter().rect_filled(tr.expand2(vec2(8.0, 4.0)), CornerRadius::same(8), W12);
            text(ui, pos2(tx + 8.0, bottom), Align2::LEFT_CENTER, tag, fb(10.0), Color32::WHITE);
            if tile.active {
                let c = pos2(r.max.x - 20.0, bottom);
                ui.painter().circle_filled(c, 9.0, GREEN);
                check_icon(ui, c, 10.0, Color32::WHITE);
                if is_patch_like(p.r#type) {
                    text(ui, c - vec2(14.0, 0.0), Align2::RIGHT_CENTER, t("LibraryTileActive"), fb(11.0), GREEN);
                }
            }
        }
        if loading && !is_patch_like(p.r#type) {
            spinner(ui, pos2(r.max.x - 20.0, bottom), 8.0, Color32::WHITE);
        }

        let mut action = None;
        if resp.clicked() {
            action = Some(if is_patch_like(p.r#type) {
                TileAction::Sync(p.guid.clone())
            } else if tile.active {
                TileAction::Disable(p.guid.clone())
            } else {
                TileAction::Enable(p.guid.clone())
            });
        }
        let active = tile.active;
        resp.context_menu(|ui| {
            ui.set_min_width(200.0);
            let patch = is_patch_like(p.r#type);
            let main = if patch {
                if active { "Sync again".to_string() } else { format!("Switch to \"{}\"", p.name) }
            } else if active {
                "Disable".to_string()
            } else {
                format!("Enable \"{}\"", p.name)
            };
            if ui.button(main).clicked() {
                action = Some(if patch {
                    TileAction::Sync(p.guid.clone())
                } else if active {
                    TileAction::Disable(p.guid.clone())
                } else {
                    TileAction::Enable(p.guid.clone())
                });
            }
            ui.separator();
            let versions: Vec<String> = p.metadata.versions.iter().filter(|v| **v != p.version).rev().cloned().collect();
            ui.add_enabled_ui(p.metadata.versions.len() > 1, |ui| {
                ui.menu_button("Sync old version", |ui| {
                    for v in versions {
                        if ui.button(&v).clicked() {
                            let g = format!("{}:{v}", p.guid);
                            action = Some(if patch { TileAction::Sync(g) } else { TileAction::Enable(g) });
                        }
                    }
                });
            });
            ui.separator();
            if ui.button("Open workshop page").clicked() {
                action = Some(TileAction::OpenPage(Box::new(p.clone())));
            }
            if patch {
                if ui.button("Open keybinds folder").clicked() {
                    util::open_folder(&workshop::keybinds_dir(&p.game_name(), &p.name));
                }
                if ui.button("Open game folder").clicked() {
                    util::open_folder(&bfme::install_path(p.game.clamp(0, 2) as usize));
                }
            }
            ui.separator();
            if ui.button("Copy package GUID").clicked() {
                util::set_clipboard(ui.ctx(), &p.guid);
            }
            ui.separator();
            if ui.button("Remove from library").clicked() {
                action = Some(TileAction::Remove(p.guid.clone()));
            }
        });
        action
    }

    fn workshop(&mut self, ui: &mut Ui, r: Rect, popups: &mut Vec<Popup>, busy: bool) {
        let bar_h = 40.0;
        let search = Rect::from_min_size(r.min, vec2(330.0, bar_h));
        if Self::toolbar_search(ui, search, &mut self.ws_search, t("WorkshopPageSearchPlaceholder"), !busy) {
            self.query_workshop();
        }
        let opts: Vec<String> = WS_FILTER_KEYS.iter().map(|k| t(k).to_string()).collect();
        let fr = Rect::from_min_size(pos2(search.max.x + 10.0, r.min.y), vec2(200.0, bar_h));
        if dropdown(ui, "ws_filter", fr, &opts, &mut self.ws_type, !busy) {
            self.query_workshop();
        }
        let sort_opts: Vec<String> = WS_SORT_KEYS.iter().map(|k| t(k).to_string()).collect();
        let sr = Rect::from_min_size(pos2(fr.max.x + 10.0, r.min.y), vec2(200.0, bar_h));
        if dropdown(ui, "ws_sort", sr, &sort_opts, &mut self.ws_sort, !busy) {
            self.query_workshop();
        }
        let reload = Rect::from_min_size(pos2(sr.max.x + 10.0, r.min.y), vec2(bar_h, bar_h));
        if button(ui, reload, "⟳", f(18.0), W10, W15, !busy).on_hover_text("Reload").clicked() {
            self.query_workshop();
        }

        let grid = Rect::from_min_max(pos2(r.min.x, r.min.y + bar_h + 15.0), r.max);
        if self.ws_error {
            text(ui, grid.center() - vec2(0.0, 14.0), Align2::CENTER_CENTER, "NO CONNECTION", fk(22.0), W40);
            text(ui, grid.center() + vec2(0.0, 14.0), Align2::CENTER_CENTER, "Couldn't reach the Workshop. Check your internet connection and reload.", f(14.0), W40);
            return;
        }
        if self.ws_job.running && self.ws_tiles.is_empty() {
            spinner(ui, grid.center(), 14.0, Color32::WHITE);
            return;
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(grid));
        let cols = ((grid.width() + 10.0) / (TILE.x + 10.0)).floor().max(1.0) as usize;
        let rows = self.ws_tiles.len().div_ceil(cols);
        let mut action = None;
        egui::ScrollArea::vertical().id_salt(("ws_scroll", self.game)).show(&mut child, |ui| {
            ui.set_width(grid.width());
            let (area, _) = ui.allocate_exact_size(vec2(grid.width(), rows as f32 * (TILE.y + 10.0)), egui::Sense::hover());
            for i in 0..self.ws_tiles.len() {
                let rr = Rect::from_min_size(
                    area.min + vec2((i % cols) as f32 * (TILE.x + 10.0), (i / cols) as f32 * (TILE.y + 10.0)),
                    TILE,
                );
                if ui.is_rect_visible(rr)
                    && let Some(a) = self.workshop_tile(ui, rr, i, busy) {
                        action = Some(a);
                    }
            }
        });
        if let Some(a) = action {
            self.apply(a, popups);
        }
    }

    fn workshop_tile(&mut self, ui: &mut Ui, r: Rect, idx: usize, busy: bool) -> Option<TileAction> {
        let p = self.ws_tiles[idx].clone();
        let in_lib = *self.ws_in_library.entry(p.guid.clone()).or_insert_with(|| workshop::is_in_library(&p.guid));
        let resp = hit(ui, r, ("ws_tile", &p.guid), !busy);
        ui.painter().rect_filled(r, CornerRadius::same(12), W12);
        if resp.hovered() {
            ui.painter().rect_filled(r, CornerRadius::same(12), W05);
        }
        let art = Rect::from_min_size(r.min + vec2(6.0, 6.0), vec2(r.width() - 12.0, r.width() - 12.0));
        ui.painter().rect_filled(art, CornerRadius::same(9), W05);
        egui::Image::new(&p.artwork_url).fit_to_exact_size(art.size()).corner_radius(9).paint_at(ui, art);
        let tx = r.min.x + 12.0;
        let tw = r.width() - 24.0;
        text_clipped(ui, pos2(tx, art.max.y + 8.0), Align2::LEFT_TOP, &p.name, fb(14.0), Color32::WHITE, tw);
        text_clipped(ui, pos2(tx, art.max.y + 28.0), Align2::LEFT_TOP, &format!("{} · {}", p.version, p.author), f(11.0), W60, tw);
        let bottom = r.max.y - 14.0;
        let tag = t(type_key(p.r#type));
        let tr = text(ui, pos2(tx + 8.0, bottom), Align2::LEFT_CENTER, tag, fb(10.0), Color32::TRANSPARENT);
        ui.painter().rect_filled(tr.expand2(vec2(8.0, 4.0)), CornerRadius::same(8), W12);
        text(ui, pos2(tx + 8.0, bottom), Align2::LEFT_CENTER, tag, fb(10.0), Color32::WHITE);
        let right = if in_lib { r.max.x - 36.0 } else { r.max.x - 12.0 };
        text(ui, pos2(right, bottom), Align2::RIGHT_CENTER, format!("⬇ {}", util::pretty_number(p.metadata.downloads)), fb(11.0), W60);
        if in_lib {
            let c = pos2(r.max.x - 20.0, bottom);
            ui.painter().circle_filled(c, 9.0, BLUE);
            check_icon(ui, c, 10.0, Color32::WHITE);
        }

        let mut action = None;
        if resp.clicked() {
            action = Some(TileAction::OpenPage(Box::new(p.clone())));
        }
        resp.context_menu(|ui| {
            ui.set_min_width(200.0);
            ui.add_enabled_ui(!in_lib, |ui| {
                if ui.button(if in_lib { "Already in library" } else { "Add to library" }).clicked() {
                    action = Some(TileAction::AddToLibrary(p.guid.clone()));
                }
            });
            ui.separator();
            if ui.button("Copy package GUID").clicked() {
                util::set_clipboard(ui.ctx(), &p.guid);
            }
        });
        action
    }

    fn apply(&mut self, a: TileAction, popups: &mut Vec<Popup>) {
        match a {
            TileAction::Sync(guid) => {
                self.clicked = Some(guid.split(':').next().unwrap_or(&guid).to_string());
                actions::spawn(move || actions::sync_package(&guid, true, None, None));
            }
            TileAction::Enable(guid) => {
                self.clicked = Some(guid.split(':').next().unwrap_or(&guid).to_string());
                actions::spawn(move || actions::enable_enhancement(&guid));
            }
            TileAction::Disable(guid) => {
                self.clicked = Some(guid.clone());
                actions::spawn(move || actions::disable_enhancement(&guid));
            }
            TileAction::OpenPage(p) => popups.push(Popup::package_page(*p)),
            TileAction::Remove(guid) => {
                workshop::library_remove(&guid);
                self.lib_tiles.retain(|t| t.preview.guid != guid);
                self.ws_in_library.remove(&guid);
            }
            TileAction::ShowWorkshop => {
                self.inner = InnerTab::Workshop;
                self.ws_search.clear();
                self.query_workshop();
            }
            TileAction::AddToLibrary(guid) => {
                let _ = &self.ws_in_library.insert(guid.clone(), true);
                actions::spawn(move || match workshop::download(&guid) {
                    Ok(e) => {
                        workshop::library_add_or_update(&e);
                        hub::send(hub::UiEvent::LibraryChanged);
                    }
                    Err(e) => hub::send(hub::UiEvent::Error(format!("{e:#}"))),
                });
            }
        }
    }
}

enum TileAction {
    Sync(String),
    Enable(String),
    Disable(String),
    OpenPage(Box<Preview>),
    Remove(String),
    ShowWorkshop,
    AddToLibrary(String),
}

fn is_active(p: &Preview) -> bool {
    let g = p.game.clamp(0, 2) as usize;
    if is_patch_like(p.r#type) { workshop::is_patch_active(g, &p.guid) } else { workshop::is_enhancement_active(g, &p.guid) }
}

fn empty_tile(ui: &mut Ui, r: Rect, busy: bool) -> bool {
    let resp = hit(ui, r, "empty_tile", !busy);
    let color = if resp.hovered() { W15 } else { W10 };
    ui.painter().rect_stroke(r.shrink(1.0), CornerRadius::same(12), egui::Stroke::new(2.0, color), egui::StrokeKind::Inside);
    text(ui, r.center() - vec2(0.0, 30.0), Align2::CENTER_CENTER, "+", fk(48.0), color.gamma_multiply(3.0));
    text(ui, r.center() + vec2(0.0, 22.0), Align2::CENTER_CENTER, t("LibraryTileEmptyInstallMore"), fb(15.0), Color32::WHITE);
    let sub = Rect::from_center_size(r.center() + vec2(0.0, 58.0), vec2(r.width() - 30.0, 40.0));
    let galley = ui.painter().layout(t("LibraryTileEmptyDiscoverMore").to_string(), f(11.0), W60, sub.width());
    let pos = pos2(sub.center().x - galley.size().x / 2.0, sub.min.y);
    ui.painter().galley(pos, galley, W60);
    resp.clicked()
}
