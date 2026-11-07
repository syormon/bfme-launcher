//! Colors, fonts, embedded images and small custom-painted widgets.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Image, Pos2, Rect, Response, Sense, Shape, Stroke,
    TextureHandle, Ui, Vec2, pos2, vec2,
};

pub const ACCENT: Color32 = Color32::from_rgb(0x15, 0xA7, 0xE9);
pub const BLUE: Color32 = Color32::from_rgb(0x02, 0x8B, 0xDB);
pub const BLUE_DARK: Color32 = Color32::from_rgb(0x01, 0x6F, 0xB3);
pub const GREEN: Color32 = Color32::from_rgb(0x5D, 0xAF, 0x47);
pub const RED: Color32 = Color32::from_rgb(0xFF, 0x5A, 0x5A);
pub const NAVY: Color32 = Color32::from_rgb(0x32, 0x34, 0x3B);

/// `#AARRGGBB`-style white with alpha.
pub const fn white_a(a: u8) -> Color32 {
    Color32::from_rgba_premultiplied(a, a, a, a)
}

pub const W05: Color32 = white_a(0x0C);
pub const W10: Color32 = white_a(0x19);
pub const W12: Color32 = white_a(0x1E);
pub const W15: Color32 = white_a(0x26);
pub const W40: Color32 = white_a(0x66);
pub const W60: Color32 = white_a(0x99);

macro_rules! img {
    ($name:literal) => {
        (concat!("bytes://", $name), include_bytes!(concat!("../../assets/images/", $name)) as &[u8])
    };
}

pub const IMG_BFME1: (&str, &[u8]) = img!("bfme1.png");
pub const IMG_BFME2: (&str, &[u8]) = img!("bfme2.png");
pub const IMG_ROTWK: (&str, &[u8]) = img!("RotWK.png");
pub const IMG_MAIN_ICON: (&str, &[u8]) = img!("MainIcon.png");
pub const IMG_LANG_SUPPORT: [(&str, &[u8]); 3] =
    [img!("BFME1LanguageSupport.png"), img!("BFME2LanguageSupport.png"), img!("RotwkLanguageSupport.png")];
pub const IMG_TITLES_EN: [(&str, &[u8]); 3] =
    [img!("en_bfme1_title.png"), img!("en_bfme2_title.png"), img!("en_RotWK_title.png")];
pub const IMG_TITLES_DE: [(&str, &[u8]); 3] =
    [img!("de_bfme1_title.png"), img!("de_bfme2_title.png"), img!("de_RotWK_title.png")];

const BG_MAP: &[u8] = include_bytes!("../../assets/images/BGMap.png");
const BG_ACRYLIC: &[u8] = include_bytes!("../../assets/images/BGMap_acrylic.png");

pub fn image(src: (&'static str, &'static [u8])) -> Image<'static> {
    Image::from_bytes(src.0, src.1)
}

pub struct Textures {
    pub background: TextureHandle,
    pub acrylic: TextureHandle,
}

fn load_png(ctx: &egui::Context, name: &str, bytes: &[u8]) -> TextureHandle {
    let img = image::load_from_memory(bytes).map(|i| i.to_rgba8()).unwrap_or_else(|_| image::RgbaImage::new(1, 1));
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    ctx.load_texture(name, color, egui::TextureOptions::LINEAR)
}

impl Textures {
    pub fn load(ctx: &egui::Context) -> Self {
        Self { background: load_png(ctx, "bgmap", BG_MAP), acrylic: load_png(ctx, "bgmap_acrylic", BG_ACRYLIC) }
    }
}

// ---------------------------------------------------------------------------
// Fonts

pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}
pub fn black() -> FontFamily {
    FontFamily::Name("black".into())
}
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn f(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn fb(size: f32) -> FontId {
    FontId::new(size, bold())
}
pub fn fk(size: f32) -> FontId {
    FontId::new(size, black())
}
pub fn fs(size: f32) -> FontId {
    FontId::new(size, semibold())
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut add = |name: &str, candidates: &[String]| -> bool {
        for file in candidates {
            if let Ok(data) = std::fs::read(file) {
                fonts.font_data.insert(name.to_string(), egui::FontData::from_owned(data).into());
                return true;
            }
        }
        false
    };
    let regular = add("segoe", &font_files(&["segoeui.ttf"], "sans-serif"));
    let bold_ok = add("segoe-bold", &font_files(&["segoeuib.ttf", "segoeui.ttf"], "sans-serif:bold"));
    let black_ok = add("segoe-black", &font_files(&["seguibl.ttf", "segoeuib.ttf"], "sans-serif:black"));
    let semi_ok = add("segoe-semibold", &font_files(&["seguisb.ttf", "segoeui.ttf"], "sans-serif:semibold"));
    let _ = add("segoe-symbol", &font_files(&["seguisym.ttf"], "symbol"));

    let defaults = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let family = |primary: Option<&str>| {
        let mut v: Vec<String> = primary.into_iter().map(str::to_string).collect();
        if regular {
            v.push("segoe".into());
        }
        if fonts.font_data.contains_key("segoe-symbol") {
            v.push("segoe-symbol".into());
        }
        v.extend(defaults.iter().cloned());
        v
    };
    let prop = family(regular.then_some("segoe"));
    let b = family(bold_ok.then_some("segoe-bold"));
    let k = family(black_ok.then_some("segoe-black"));
    let s = family(semi_ok.then_some("segoe-semibold"));
    fonts.families.insert(FontFamily::Proportional, prop);
    fonts.families.insert(bold(), b);
    fonts.families.insert(black(), k);
    fonts.families.insert(semibold(), s);
    ctx.set_fonts(fonts);
}

/// Font files to try, in order. Windows: Segoe UI from the Fonts folder. Linux: whatever fontconfig
/// picks for `pattern` (the desktop's sans-serif in that weight).
#[cfg(windows)]
fn font_files(windows_files: &[&str], _pattern: &str) -> Vec<String> {
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    windows_files.iter().map(|f| format!("{windir}\\Fonts\\{f}")).collect()
}

#[cfg(not(windows))]
fn font_files(_windows_files: &[&str], pattern: &str) -> Vec<String> {
    std::process::Command::new("fc-match")
        .args(["--format=%{file}", pattern])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|f| [".ttf", ".otf", ".ttc"].iter().any(|e| f.to_lowercase().ends_with(e)))
        .into_iter()
        .collect()
}

pub fn apply_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|s| {
        s.visuals = egui::Visuals::dark();
        s.visuals.override_text_color = Some(Color32::WHITE);
        s.visuals.panel_fill = Color32::TRANSPARENT;
        s.visuals.window_fill = NAVY;
        s.visuals.extreme_bg_color = Color32::from_black_alpha(90);
        s.visuals.selection.bg_fill = BLUE;
        s.visuals.widgets.inactive.bg_fill = W10;
        s.visuals.widgets.inactive.weak_bg_fill = W10;
        s.visuals.widgets.hovered.bg_fill = W15;
        s.visuals.widgets.hovered.weak_bg_fill = W15;
        s.visuals.widgets.active.bg_fill = W12;
        s.visuals.widgets.active.weak_bg_fill = W12;
        s.visuals.menu_corner_radius = CornerRadius::same(6);
        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(10.0, 5.0);
        s.spacing.scroll.bar_width = 6.0;
        s.spacing.scroll.floating = true;
        s.text_styles.insert(egui::TextStyle::Body, f(14.0));
        s.text_styles.insert(egui::TextStyle::Button, f(14.0));
        s.text_styles.insert(egui::TextStyle::Small, f(11.0));
        s.text_styles.insert(egui::TextStyle::Heading, fb(20.0));
    });
}

// ---------------------------------------------------------------------------
// Painting helpers

/// Paints the "acrylic" glass panel: the pre-blurred background image, aligned to the window.
pub fn acrylic(ui: &Ui, tex: &TextureHandle, rect: Rect, radius: u8) {
    let screen = ui.ctx().content_rect();
    let uv = Rect::from_min_max(
        pos2((rect.min.x - screen.min.x) / screen.width(), (rect.min.y - screen.min.y) / screen.height()),
        pos2((rect.max.x - screen.min.x) / screen.width(), (rect.max.y - screen.min.y) / screen.height()),
    );
    let shape = egui::epaint::RectShape::filled(rect, CornerRadius::same(radius), Color32::WHITE).with_texture(tex.id(), uv);
    ui.painter().add(Shape::Rect(shape));
    ui.painter().rect_filled(rect, CornerRadius::same(radius), Color32::from_black_alpha(25));
}

pub fn text(ui: &Ui, pos: Pos2, align: Align2, s: impl ToString, font: FontId, color: Color32) -> Rect {
    ui.painter().text(pos, align, s.to_string(), font, color)
}

/// Text truncated with "…" to fit `max_width`.
pub fn text_clipped(ui: &Ui, pos: Pos2, align: Align2, s: &str, font: FontId, color: Color32, max_width: f32) {
    let painter = ui.painter();
    let mut job = egui::text::LayoutJob::simple_singleline(s.to_string(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width);
    let galley = painter.layout_job(job);
    let rect = align.anchor_size(pos, galley.size());
    painter.galley(rect.min, galley, color);
}

pub fn wrapped_text(ui: &Ui, rect: Rect, s: &str, font: FontId, color: Color32) -> f32 {
    let galley = ui.painter().layout(s.to_string(), font, color, rect.width());
    let h = galley.size().y;
    ui.painter().with_clip_rect(rect).galley(rect.min, galley, color);
    h
}

/// An invisible clickable region.
pub fn hit(ui: &mut Ui, rect: Rect, id: impl std::hash::Hash + std::fmt::Debug, enabled: bool) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let r = ui.interact(rect, egui::Id::new(id), sense);
    if enabled { r.on_hover_cursor(CursorIcon::PointingHand) } else { r }
}

/// A flat rounded button with centered text.
pub fn button(ui: &mut Ui, rect: Rect, label: &str, font: FontId, fill: Color32, hover: Color32, enabled: bool) -> Response {
    let r = hit(ui, rect, ("btn", label, rect.min.x as i32, rect.min.y as i32), enabled);
    let fill = if !enabled { fill.gamma_multiply(0.4) } else if r.hovered() { hover } else { fill };
    ui.painter().rect_filled(rect, CornerRadius::same(((rect.height() / 2.0).min(9.0)) as u8), fill);
    let color = if enabled { Color32::WHITE } else { W60 };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, font, color);
    r
}

pub fn blue_button(ui: &mut Ui, rect: Rect, label: &str, enabled: bool) -> Response {
    button(ui, rect, label, fb(14.0), BLUE, BLUE_DARK, enabled)
}

pub fn flat_button(ui: &mut Ui, rect: Rect, label: &str, enabled: bool) -> Response {
    button(ui, rect, label, fb(14.0), W10, W15, enabled)
}

/// A horizontal progress bar.
pub fn progress_bar(ui: &Ui, rect: Rect, progress: f32, color: Color32) {
    let r = CornerRadius::same((rect.height() / 2.0) as u8);
    ui.painter().rect_filled(rect, r, W12);
    let mut fill = rect;
    fill.set_width(rect.width() * progress.clamp(0.0, 1.0));
    if fill.width() > 0.5 {
        ui.painter().rect_filled(fill, r, color);
    }
}

/// A rotating arc spinner.
pub fn spinner(ui: &Ui, center: Pos2, radius: f32, color: Color32) {
    let t = ui.input(|i| i.time) as f32;
    let start = t * 6.0;
    let n = 24;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = start + (i as f32 / n as f32) * std::f32::consts::PI * 1.4;
            center + radius * Vec2::angled(a)
        })
        .collect();
    ui.painter().add(Shape::line(pts, Stroke::new(radius * 0.28, color)));
    ui.ctx().request_repaint();
}

pub fn check_icon(ui: &Ui, center: Pos2, size: f32, color: Color32) {
    let s = size / 2.0;
    let pts = vec![center + vec2(-s * 0.8, 0.0), center + vec2(-s * 0.2, s * 0.6), center + vec2(s * 0.85, -s * 0.6)];
    ui.painter().add(Shape::line(pts, Stroke::new(size * 0.18, color)));
}

/// Gear icon (settings), drawn procedurally.
pub fn gear(ui: &Ui, center: Pos2, r: f32, color: Color32) {
    // Built from convex pieces only (a ring plus one quad per tooth): egui fills polygons as
    // convex fans, so a single gear outline would come out with flattened teeth.
    let painter = ui.painter();
    painter.circle_stroke(center, r * 0.56, Stroke::new(r * 0.36, color));
    let teeth = 8;
    for i in 0..teeth {
        let a = i as f32 / teeth as f32 * std::f32::consts::TAU;
        let (dir, side) = (Vec2::angled(a), Vec2::angled(a + std::f32::consts::FRAC_PI_2));
        let (inner, outer) = (center + dir * r * 0.66, center + dir * r);
        let pts = vec![
            inner - side * r * 0.2,
            outer - side * r * 0.14,
            outer + side * r * 0.14,
            inner + side * r * 0.2,
        ];
        painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
    }
}

/// Info icon (about), drawn procedurally: a ringed "i".
pub fn info_icon(ui: &Ui, center: Pos2, r: f32, color: Color32) {
    ui.painter().circle_stroke(center, r, Stroke::new(2.0, color));
    ui.painter().circle_filled(center + vec2(0.0, -r * 0.45), r * 0.14, color);
    ui.painter().line_segment([center + vec2(0.0, -r * 0.12), center + vec2(0.0, r * 0.55)], Stroke::new(r * 0.2, color));
}

/// A dropdown that looks like the original `DropdownPicker`.
pub fn dropdown(ui: &mut Ui, id: &str, rect: Rect, options: &[String], selected: &mut usize, enabled: bool) -> bool {
    let mut changed = false;
    let current = options.get(*selected).cloned().unwrap_or_default();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    if !enabled {
        child.disable();
    }
    child.spacing_mut().interact_size.y = rect.height();
    egui::ComboBox::from_id_salt(id)
        .width(rect.width())
        .height(400.0)
        .selected_text(egui::RichText::new(current).font(f(15.0)))
        .show_ui(&mut child, |ui| {
            for (i, o) in options.iter().enumerate() {
                if ui.selectable_label(i == *selected, egui::RichText::new(o).font(f(15.0))).clicked() && i != *selected {
                    *selected = i;
                    changed = true;
                }
            }
        });
    changed
}
