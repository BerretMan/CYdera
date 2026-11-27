//! Identité visuelle « instrument » : noir carbone mat, grille à points, graphite solide,
//! bordures nettes, couleurs de touches de synthé, monospace pour toutes les données.

use crate::model::{Cours, Kind};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, LayerId, Margin, Mesh, Painter, Pos2, Rect, Shape,
    Stroke, TextureHandle, TextureOptions, Vec2, pos2, vec2,
};
use std::sync::Arc;

// ---- Surfaces (mates, sans transparence) ----
pub const CARBON: Color32 = Color32::from_rgb(0x0E, 0x0E, 0x10);
/// Fond des panneaux : transparent pour laisser voir la grille à points.
pub const BG: Color32 = Color32::TRANSPARENT;
pub const PANEL: Color32 = Color32::from_rgb(0x12, 0x12, 0x15);
pub const CARD: Color32 = Color32::from_rgb(0x16, 0x16, 0x1A);
pub const CARD_HOVER: Color32 = Color32::from_rgb(0x1E, 0x1E, 0x24);
pub const LINE: Color32 = Color32::from_rgb(0x2A, 0x2A, 0x32);
pub const LINE_FAINT: Color32 = Color32::from_rgb(0x1D, 0x1D, 0x23);
pub const WINDOW: Color32 = Color32::from_rgb(0x16, 0x16, 0x1A);
pub const DOT: Color32 = Color32::from_rgb(0x27, 0x27, 0x2A);

// ---- Texte ----
pub const TEXT: Color32 = Color32::from_rgb(0xF4, 0xF4, 0xF5);
pub const MUTED: Color32 = Color32::from_rgb(0xA1, 0xA1, 0xAA);
/// 5,28:1 sur CARD (0x71717A ne donnait que 3,73:1, sous le seuil de 4,5:1 pour du texte de 10 px).
pub const DIM: Color32 = Color32::from_rgb(0x8A, 0x8A, 0x94);
/// Fond d'un élément pressé.
pub const PRESS: Color32 = Color32::from_rgb(0x26, 0x26, 0x2E);

// ---- Accents « touches de synthé » ----
pub const ACCENT: Color32 = Color32::from_rgb(0x00, 0xD1, 0xFF); // cyan technique
pub const NOW: Color32 = Color32::from_rgb(0xFF, 0x3B, 0x30);
pub const OK: Color32 = Color32::from_rgb(0x4A, 0xDE, 0x80);
pub const WARN: Color32 = Color32::from_rgb(0xEA, 0xB3, 0x08); // jaune chantier

pub const CM: Color32 = Color32::from_rgb(0xFF, 0x55, 0x00); // orange sécurité
pub const TD: Color32 = Color32::from_rgb(0x2E, 0x5B, 0xFF); // bleu électrique
pub const TP: Color32 = Color32::from_rgb(0xA8, 0x55, 0xF7);
pub const EXAM: Color32 = Color32::from_rgb(0xD4, 0xF0, 0x00); // lime
pub const INDISPO: Color32 = Color32::from_rgb(0xEA, 0xB3, 0x08);
pub const AUTRE: Color32 = Color32::from_rgb(0x22, 0xC5, 0x5E);

/// Angles quasi droits partout (≤ 4 px).
pub const RADIUS: u8 = 3;

pub fn kind_color(k: Kind) -> Color32 {
    match k {
        Kind::Cm => CM,
        Kind::Td => TD,
        Kind::Tp => TP,
        Kind::Exam => EXAM,
        Kind::Indispo => INDISPO,
        Kind::Autre => AUTRE,
    }
}

/// Couleur du texte posé sur l'aplat de `kind_color` (contraste maximal).
pub fn on_kind(k: Kind) -> Color32 {
    match k {
        Kind::Td => Color32::WHITE,
        _ => Color32::BLACK,
    }
}

/// Étiquette technique : « CM », « TD »…
pub fn kind_tag(c: &Cours) -> String {
    match c.kind {
        Kind::Cm => "CM".into(),
        Kind::Td => "TD".into(),
        Kind::Tp => "TP".into(),
        Kind::Exam => "EX".into(),
        Kind::Indispo => "IND".into(),
        Kind::Autre => {
            let t: String = c.category.to_uppercase().chars().take(4).collect();
            if t.is_empty() { "EVT".into() } else { t }
        }
    }
}

// ------------------------------------------------------------------ polices

/// Titres et texte : Space Grotesk (géométrique, gras).
pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// Données chiffrées : JetBrains Mono.
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

pub fn mono_bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("monobold".into()))
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let fb_prop = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let fb_mono = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();

    let data: [(&str, &'static [u8]); 4] = [
        ("sg-regular", include_bytes!("../../assets/fonts/sg-regular.ttf")),
        ("sg-bold", include_bytes!("../../assets/fonts/sg.ttf")),
        ("jbm-regular", include_bytes!("../../assets/fonts/jbm-regular.ttf")),
        ("jbm-bold", include_bytes!("../../assets/fonts/jbm-bold.ttf")),
    ];
    for (name, bytes) in data {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    let chain = |first: &str, fb: &[String]| -> Vec<String> {
        let mut v = vec![first.to_string()];
        v.extend(fb.iter().cloned());
        v
    };
    fonts.families.insert(FontFamily::Proportional, chain("sg-regular", &fb_prop));
    fonts.families.insert(FontFamily::Name("bold".into()), chain("sg-bold", &fb_prop));
    fonts.families.insert(FontFamily::Monospace, chain("jbm-regular", &fb_mono));
    fonts.families.insert(FontFamily::Name("monobold".into()), chain("jbm-bold", &fb_mono));
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut v = egui::Visuals::dark();
    v.panel_fill = CARBON;
    v.window_fill = WINDOW;
    v.extreme_bg_color = CARBON;
    v.faint_bg_color = CARD;
    v.override_text_color = Some(TEXT);
    v.window_stroke = Stroke::new(1.0, LINE);
    v.window_corner_radius = CornerRadius::same(RADIUS);
    // Ombre dure, décalée, façon sérigraphie : pas de flou.
    v.window_shadow = egui::Shadow { offset: [5, 5], blur: 0, spread: 0, color: Color32::from_black_alpha(170) };
    v.popup_shadow = v.window_shadow;
    v.selection.bg_fill = Color32::from_rgb(0x07, 0x2A, 0x33);
    v.selection.stroke = Stroke::new(1.5, ACCENT);
    v.hyperlink_color = ACCENT;

    let r = CornerRadius::same(RADIUS);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.inactive.bg_fill = CARD;
    v.widgets.inactive.weak_bg_fill = CARD;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.hovered.bg_fill = CARD_HOVER;
    v.widgets.hovered.weak_bg_fill = CARD_HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.5, ACCENT);
    v.widgets.active.bg_fill = Color32::from_rgb(0x07, 0x2A, 0x33);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(0x07, 0x2A, 0x33);
    v.widgets.active.bg_stroke = Stroke::new(1.5, ACCENT);
    ctx.set_visuals(v);

    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 7.0);
        s.spacing.button_padding = Vec2::new(10.0, 5.0);
        s.spacing.window_margin = egui::Margin::same(16);
        s.text_styles.insert(egui::TextStyle::Body, regular(14.5));
        s.text_styles.insert(egui::TextStyle::Button, mono_bold(12.0));
        s.text_styles.insert(egui::TextStyle::Small, mono(11.0));
        s.text_styles.insert(egui::TextStyle::Monospace, mono(13.0));
        s.text_styles.insert(egui::TextStyle::Heading, bold(22.0));
    });
}

// ------------------------------------------------------------------ surfaces

/// Carte graphite : aplat solide, bordure nette 1 px, angles vifs.
pub fn card_ex<R>(
    ui: &mut egui::Ui,
    fill: Color32,
    stroke: Stroke,
    radius: u8,
    margin: Margin,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::new().fill(fill).stroke(stroke).corner_radius(CornerRadius::same(radius.min(4))).inner_margin(margin).show(ui, add)
}

pub fn card<R>(ui: &mut egui::Ui, margin: Margin, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    card_ex(ui, CARD, Stroke::new(1.0, LINE), RADIUS, margin, add)
}

/// Hachures diagonales à 45° (limitées à `rect`).
pub fn hatch(painter: &Painter, rect: Rect, spacing: f32, stroke: Stroke) {
    let clip = painter.with_clip_rect(rect);
    let h = rect.height();
    let mut x = rect.left() - h;
    while x < rect.right() {
        clip.line_segment([pos2(x, rect.bottom()), pos2(x + h, rect.top())], stroke);
        x += spacing;
    }
}

/// Repères d'angle façon viseur / carte électronique : quatre petits « L » aux coins de `rect`.
pub fn corner_ticks(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.expand(2.0);
    let (l, st) = (7.0, Stroke::new(1.5, color));
    for (c, dx, dy) in [(r.left_top(), 1.0, 1.0), (r.right_top(), -1.0, 1.0), (r.left_bottom(), 1.0, -1.0), (r.right_bottom(), -1.0, -1.0)]
    {
        painter.line_segment([c, pos2(c.x + dx * l, c.y)], st);
        painter.line_segment([c, pos2(c.x, c.y + dy * l)], st);
    }
}

/// Ruban de chantier : bandes obliques `a` / `b`.
pub fn tape(painter: &Painter, rect: Rect, a: Color32, b: Color32, s: f32) {
    let clip = painter.with_clip_rect(rect);
    clip.rect_filled(rect, CornerRadius::ZERO, a);
    let h = rect.height();
    let mut x = rect.left() - h;
    while x < rect.right() {
        clip.add(Shape::convex_polygon(
            vec![pos2(x, rect.bottom()), pos2(x + s, rect.bottom()), pos2(x + s + h, rect.top()), pos2(x + h, rect.top())],
            b,
            Stroke::NONE,
        ));
        x += s * 2.0;
    }
}

/// Puce matérielle : boîtier bordé avec petites pattes (numéro de salle).
pub fn chip(painter: &Painter, anchor: Pos2, align: Align2, text: &str, font: FontId, color: Color32) -> Rect {
    let g = painter.layout_no_wrap(text.to_string(), font, color);
    let size = g.size() + vec2(12.0, 4.0);
    let rect = align.anchor_size(anchor, size);
    painter.rect_filled(rect, CornerRadius::same(2), CARBON);
    painter.rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, Color32::from_rgb(0x46, 0x46, 0x52)), egui::StrokeKind::Inside);
    let leg = Stroke::new(1.0, Color32::from_rgb(0x46, 0x46, 0x52));
    for f in [0.3, 0.7] {
        let y = rect.top() + rect.height() * f;
        painter.line_segment([pos2(rect.left() - 2.0, y), pos2(rect.left(), y)], leg);
        painter.line_segment([pos2(rect.right(), y), pos2(rect.right() + 2.0, y)], leg);
    }
    painter.galley(rect.center() - g.size() / 2.0, g, color);
    rect
}

/// Largeur d'une puce pour `text`.
pub fn chip_width(painter: &Painter, text: &str, font: FontId) -> f32 {
    painter.layout_no_wrap(text.to_string(), font, TEXT).size().x + 12.0
}

/// Opacité douce 0.3..1.0 pour faire « respirer » les deux-points de l'horloge.
pub fn blink(ctx: &egui::Context) -> f32 {
    if super::motion::reduced() {
        return 1.0;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(33));
    let t = ctx.input(|i| i.time);
    0.30 + 0.70 * (0.5 + 0.5 * (t * std::f64::consts::TAU * 0.9).sin()) as f32
}

fn dot_texture(ctx: &egui::Context) -> TextureHandle {
    let id = egui::Id::new("dot_grid_tex");
    if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return t;
    }
    let mut img = egui::ColorImage::filled([20, 20], Color32::TRANSPARENT);
    img.pixels[10 * 20 + 10] = DOT;
    let t = ctx.load_texture("dot-grid", img, TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..TextureOptions::NEAREST });
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

/// Fond noir carbone + grille à points de 20 px.
pub fn paint_background(ctx: &egui::Context) {
    let r = ctx.content_rect();
    let p = ctx.layer_painter(LayerId::background());
    p.rect_filled(r, CornerRadius::ZERO, CARBON);
    let tex = dot_texture(ctx);
    let mut m = Mesh::with_texture(tex.id());
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(r.width() / 20.0, r.height() / 20.0));
    m.add_rect_with_uv(r, uv, Color32::WHITE);
    p.add(Shape::mesh(m));
}

// ------------------------------------------------------------------ chiffres tabulaires

/// Dessine `text` caractère par caractère avec des chiffres de largeur fixe.
/// (Avec une police monospace c'est déjà le cas ; utile pour les polices proportionnelles.)
pub fn tabular(painter: &Painter, pos: Pos2, anchor: Align2, text: &str, font: FontId, color: Color32) -> Rect {
    let dw = painter.layout_no_wrap("0".into(), font.clone(), color).size().x;
    let mut parts = Vec::new();
    let (mut w, mut h) = (0.0f32, 0.0f32);
    for ch in text.chars() {
        let g = painter.layout_no_wrap(ch.to_string(), font.clone(), color);
        let adv = if ch.is_ascii_digit() { dw } else { g.size().x };
        h = h.max(g.size().y);
        parts.push((g, adv, ch.is_ascii_digit()));
        w += adv;
    }
    let rect = anchor.anchor_size(pos, vec2(w, h));
    let mut x = rect.left();
    for (g, adv, digit) in parts {
        let off = if digit { (adv - g.size().x) / 2.0 } else { 0.0 };
        painter.galley(pos2(x + off, rect.top()), g, color);
        x += adv;
    }
    rect
}

pub fn tabular_size(painter: &Painter, text: &str, font: &FontId) -> Vec2 {
    let dw = painter.layout_no_wrap("0".into(), font.clone(), Color32::WHITE).size().x;
    let (mut w, mut h) = (0.0f32, 0.0f32);
    for ch in text.chars() {
        let g = painter.layout_no_wrap(ch.to_string(), font.clone(), Color32::WHITE);
        w += if ch.is_ascii_digit() { dw } else { g.size().x };
        h = h.max(g.size().y);
    }
    vec2(w, h)
}

pub fn tabular_label(ui: &mut egui::Ui, text: &str, font: FontId, color: Color32) -> egui::Response {
    let size = tabular_size(ui.painter(), text, &font);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    tabular(ui.painter(), rect.min, Align2::LEFT_TOP, text, font, color);
    resp
}

/// Texte monospace aligné (équivalent `ui.label` avec JetBrains Mono).
pub fn mono_label(ui: &mut egui::Ui, text: impl Into<String>, size: f32, color: Color32) -> egui::Response {
    ui.label(egui::RichText::new(text.into()).font(mono(size)).color(color))
}
