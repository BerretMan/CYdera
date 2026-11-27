//! Cartes de cours, au style « agenda » mobile partout (semaine, jour, panneau) : coins arrondis,
//! teinte légère du type, barre d'accent à gauche, pastille de type, étiquette de salle arrondie.

use super::motion;
use super::theme::{self, bold, mono, mono_bold, regular};
use crate::model::{Cours, Kind, fmt_duration};
use chrono::NaiveDateTime;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, Rect, Response, Sense, Stroke, StrokeKind, TextFormat, pos2, text::LayoutJob, vec2,
};

/// Cours passés : ~80 % d'opacité, toujours parfaitement lisibles.
fn fade(c: Color32, past: bool) -> Color32 {
    if past { c.gamma_multiply(0.8) } else { c }
}

fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn clamp_job(text: &str, font: egui::FontId, color: Color32, width: f32, rows: usize) -> LayoutJob {
    let mut job = LayoutJob::single_section(text.to_string(), TextFormat { font_id: font, color, ..Default::default() });
    job.wrap.max_width = width;
    job.wrap.max_rows = rows;
    job.wrap.break_anywhere = false;
    job.wrap.overflow_character = Some('…');
    job
}

fn time_range(c: &Cours) -> String {
    format!("{}—{}", c.start.format("%H:%M"), c.end.format("%H:%M"))
}

/// Fond teinté de la couleur du type (plus discret si le cours est passé).
pub fn card_fill(kind: Kind, past: bool) -> Color32 {
    motion::lerp_color(theme::CARD, theme::kind_color(kind), if past { 0.04 } else { 0.09 })
}

/// Pastille de type arrondie (« TD », « CM »…) : fond de la couleur du type, jaune pour les indispos.
fn tag_colors(kind: Kind, past: bool) -> (Color32, Color32) {
    if kind == Kind::Indispo {
        (theme::WARN, Color32::BLACK)
    } else {
        (alpha(theme::kind_color(kind), if past { 60 } else { 90 }), fade(theme::TEXT, past))
    }
}

fn tag_pill(p: &egui::Painter, min: egui::Pos2, text: String, kind: Kind, past: bool, font: egui::FontId) -> Rect {
    let (fill, txt) = tag_colors(kind, past);
    let g = p.layout_no_wrap(text, font, txt);
    let r = Rect::from_min_size(min, g.size() + vec2(10.0, 4.0));
    p.rect_filled(r, CornerRadius::same(6), fill);
    p.galley(r.center() - g.size() / 2.0, g, txt);
    r
}

/// Étiquette de salle arrondie (fond carbone, bordure fine).
pub fn room_tag(p: &egui::Painter, anchor: egui::Pos2, align: Align2, text: &str, font: egui::FontId, color: Color32) -> Rect {
    let g = p.layout_no_wrap(text.to_string(), font, color);
    let r = align.anchor_size(anchor, vec2(g.size().x + 12.0, (g.size().y + 6.0).max(20.0)));
    p.rect_filled(r, CornerRadius::same(6), theme::CARBON);
    p.rect_stroke(r, CornerRadius::same(6), Stroke::new(1.0, theme::LINE), StrokeKind::Inside);
    p.galley(r.center() - g.size() / 2.0, g, color);
    r
}

fn room_tag_width(p: &egui::Painter, text: &str, font: egui::FontId) -> f32 {
    p.layout_no_wrap(text.to_string(), font, theme::TEXT).size().x + 12.0
}

/// Infobulle au même style que l'UI.
pub fn course_tooltip(ui: &mut egui::Ui, c: &Cours) {
    ui.set_max_width(300.0);
    ui.horizontal(|ui| {
        pill(ui, &theme::kind_tag(c), theme::kind_color(c.kind));
        theme::mono_label(ui, time_range(c), 12.5, theme::TEXT);
        theme::mono_label(ui, fmt_duration(c.minutes()), 11.5, theme::DIM);
    });
    ui.add(egui::Label::new(egui::RichText::new(c.matiere.to_uppercase()).font(bold(14.5))).wrap());
    if !c.prof.is_empty() {
        ui.label(egui::RichText::new(capitalize_words(&c.prof)).color(theme::MUTED));
    }
    if !c.salle.is_empty() {
        ui.add(egui::Label::new(egui::RichText::new(&c.salle).font(mono(11.5)).color(theme::TEXT)).wrap());
    }
}

/// Raccourcit `text` (suffixe « … ») pour que l'étiquette correspondante tienne dans `max_w`.
fn fit_tag_text(painter: &egui::Painter, text: &str, font: egui::FontId, max_w: f32) -> String {
    let mut t: Vec<char> = text.chars().collect();
    let mut s: String = t.iter().collect();
    while room_tag_width(painter, &s, font.clone()) > max_w && t.len() > 1 {
        t.pop();
        s = t.iter().collect::<String>() + "…";
    }
    s
}

/// Plus grande taille (parmi `sizes`) où le mot le plus long du titre tient sans être coupé en deux.
fn title_font(painter: &egui::Painter, text: &str, width: f32, sizes: &[f32]) -> egui::FontId {
    let longest = |f: &egui::FontId| {
        text.split_whitespace().map(|w| painter.layout_no_wrap(w.to_string(), f.clone(), Color32::WHITE).size().x).fold(0.0f32, f32::max)
    };
    for &sz in sizes {
        let f = bold(sz);
        if longest(&f) <= width {
            return f;
        }
    }
    bold(*sizes.last().unwrap_or(&11.0))
}

/// « INDISPONIBILITÉ / ÉVÉNEMENT » devient « INDISPO » si son plus long mot ne tient pas dans la colonne.
fn short_title(painter: &egui::Painter, c: &Cours, width: f32, size: f32) -> String {
    let up = c.matiere.to_uppercase();
    if c.kind == Kind::Indispo {
        let f = bold(size);
        let longest =
            up.split_whitespace().map(|w| painter.layout_no_wrap(w.to_string(), f.clone(), Color32::WHITE).size().x).fold(0.0f32, f32::max);
        if longest > width {
            return "INDISPO".to_string();
        }
    }
    up
}

/// Mot unique trop long pour la colonne : on le tronque avec « … » plutôt que de le couper en deux.
fn ellipsize_single_word(painter: &egui::Painter, text: &str, font: &egui::FontId, width: f32) -> String {
    if text.split_whitespace().count() != 1 {
        return text.to_string();
    }
    let w = |t: &str| painter.layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE).size().x;
    let mut chars: Vec<char> = text.chars().collect();
    let mut out = text.to_string();
    while w(&out) > width && chars.len() > 1 {
        chars.pop();
        out = chars.iter().collect::<String>() + "…";
    }
    out
}

/// Accessibilité : libellé lu par un lecteur d'écran (AccessKit).
fn describe(resp: &Response, c: &Cours) {
    let mut label = format!("{} {}, {}", theme::kind_tag(c), c.matiere, time_range(c));
    if !c.salle_courte.is_empty() {
        label += &format!(", salle {}", c.salle_courte);
    }
    if !c.prof.is_empty() {
        label += &format!(", {}", c.prof);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
}

/// Carte d'un cours dans un rectangle imposé par l'horaire (semaine, jour, agenda mobile).
///
/// Ligne du haut « [TD] 1 h 30 ······ E211 », titre, enseignant, progression si le cours est en cours.
/// S'adapte à la taille réelle : une seule ligne `TD  TITRE…` sous 40 px de haut, marges et polices
/// resserrées dans les colonnes étroites (semaine, cours simultanés).
pub fn course_card(ui: &mut egui::Ui, rect: Rect, c: &Cours, now: NaiveDateTime, id: egui::Id) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    let (hover, press) = motion::press(ui.ctx(), id, resp.hovered(), resp.is_pointer_button_down_on());
    describe(&resp, c);
    let past = c.is_past(now);
    let live = c.is_now(now);
    let ind = c.kind == Kind::Indispo;
    let kc = theme::kind_color(c.kind);
    let painter = ui.painter_at(rect.expand(3.0));
    let (w, h) = (rect.width(), rect.height());
    let micro = h < 40.0;
    let tiny = w < 130.0;
    let narrow = w < 220.0;
    let rad = CornerRadius::same(if micro || tiny { 8 } else { 12 });

    let fill = motion::lerp_color(motion::lerp_color(card_fill(c.kind, past), theme::CARD_HOVER, hover), theme::PRESS, press);
    painter.rect_filled(rect, rad, fill);
    if ind {
        theme::hatch(&painter.with_clip_rect(rect.shrink(3.0)), rect, 8.0, Stroke::new(1.0, alpha(theme::WARN, 26)));
    }
    let stroke = if live {
        Stroke::new(1.5, kc)
    } else if ind {
        Stroke::new(1.0, alpha(theme::WARN, 150))
    } else {
        Stroke::new(1.0, motion::lerp_color(theme::LINE, kc, hover.max(press) * 0.6))
    };
    painter.rect_stroke(rect, rad, stroke, StrokeKind::Inside);
    if resp.has_focus() {
        painter.rect_stroke(rect.expand(1.5), rad, Stroke::new(2.0, theme::ACCENT), StrokeKind::Outside);
    }
    // barre d'accent à gauche
    let bar_x = rect.left() + if tiny { 5.0 } else { 7.0 };
    let bar_m = (h * 0.18).clamp(5.0, 10.0);
    let bar = Rect::from_min_max(pos2(bar_x, rect.top() + bar_m), pos2(bar_x + 3.0, rect.bottom() - bar_m));
    painter.rect_filled(bar, CornerRadius::same(2), fade(kc, past));

    let p = painter.with_clip_rect(rect.shrink(2.0));
    let x0 = rect.left() + if tiny { 13.0 } else { 18.0 };
    let x1 = rect.right() - if tiny { 5.0 } else { 10.0 };
    let tw = (x1 - x0).max(1.0);
    let txt = fade(theme::TEXT, past);
    let muted = fade(theme::MUTED, past);
    let tag_font = mono_bold(if tiny { 9.5 } else { 10.5 });
    let tag = theme::kind_tag(c);

    // ---- micro : une seule ligne, détails dans l'infobulle
    if micro {
        let cy = rect.center().y;
        let tag_w = p.layout_no_wrap(tag.clone(), tag_font.clone(), txt).size().x + 10.0;
        let mut tx = x0;
        if tag_w + 30.0 <= tw {
            let r = tag_pill(&p, pos2(x0, cy - 8.0), tag, c.kind, past, tag_font);
            tx = r.right() + 6.0;
        }
        let f = bold(if tiny { 11.0 } else { 12.0 });
        let g = p.layout_job(clamp_job(&c.matiere.to_uppercase(), f, txt, (x1 - tx).max(1.0), 1));
        p.galley(pos2(tx, cy - g.size().y / 2.0), g, txt);
        return resp.on_hover_ui(|ui| course_tooltip(ui, c));
    }

    // ---- colonne très étroite (semaine sur téléphone) : titre sur plusieurs lignes, salle en bas ;
    // le type se lit à la couleur de la barre, le détail est dans la fiche
    if w < 90.0 {
        let room = c.salle_courte.split(',').next().unwrap_or("").trim().to_string();
        let foot_h = if !room.is_empty() && h >= 52.0 { 14.0 } else { 0.0 };
        let (t0, t1) = (rect.top() + 5.0, rect.bottom() - 4.0 - foot_h);
        let rows = (((t1 - t0) / 12.5).floor() as usize).clamp(1, 6);
        let mut job = clamp_job(&short_title(&p, c, tw, 10.0), bold(10.0), txt, tw, rows);
        job.wrap.break_anywhere = true;
        let g = p.layout_job(job);
        p.galley(pos2(x0, t0), g, txt);
        if foot_h > 0.0 {
            let g = p.layout_job(clamp_job(&room, mono(9.5), muted, tw, 1));
            p.galley(pos2(x0, rect.bottom() - 4.0 - g.size().y), g, muted);
        }
        if live {
            p.circle_filled(pos2(x1 - 3.0, rect.top() + 6.0), 3.0, theme::OK);
        }
        return resp.on_hover_ui(|ui| course_tooltip(ui, c));
    }

    // ---- ligne du haut : type, durée (ou « en cours »), salle
    let tight = h < 90.0;
    let top = rect.top() + if tight { 6.0 } else { 9.0 };
    let pill = tag_pill(&p, pos2(x0, top), tag, c.kind, past, tag_font);
    if live {
        let g = p.layout_no_wrap(if narrow { "●".into() } else { "● EN COURS".into() }, mono_bold(10.5), theme::OK);
        p.galley(pos2(pill.right() + 7.0, pill.center().y - g.size().y / 2.0), g, theme::OK);
    } else if !narrow {
        let g = p.layout_no_wrap(fmt_duration(c.minutes()), regular(12.0), theme::DIM);
        p.galley(pos2(pill.right() + 7.0, pill.center().y - g.size().y / 2.0), g, theme::DIM);
    }
    let room = c.salle_courte.as_str();
    let room_font = mono_bold(if tiny { 10.5 } else { 12.0 });
    let mut room_on_top = false;
    if !room.is_empty() {
        let reserve = if narrow { if live { 16.0 } else { 6.0 } } else { 90.0 };
        let max_w = (x1 - pill.right() - reserve).max(0.0);
        let rt = fit_tag_text(&p, room, room_font.clone(), max_w.max(40.0));
        if room_tag_width(&p, &rt, room_font.clone()) <= max_w + 1.0 {
            room_tag(&p, pos2(x1, pill.center().y), Align2::RIGHT_CENTER, &rt, room_font.clone(), txt);
            room_on_top = true;
        }
    }

    // ---- bas de carte : enseignant (+ salle si elle n'a pas tenu en haut), barre de progression
    let live_h = if live { 9.0 } else { 0.0 };
    let foot_y = rect.bottom() - if tight { 6.0 } else { 9.0 } - live_h;
    let title_top = pill.bottom() + if tight { 4.0 } else { 7.0 };
    let has_foot = !c.prof.is_empty() || (!room.is_empty() && !room_on_top);
    let line_h = if tiny { 15.0 } else { 19.0 };
    let show_foot = has_foot && foot_y - title_top >= line_h + 16.0;
    let title_bottom = if show_foot { foot_y - 17.0 } else { foot_y };
    let rows = (((title_bottom - title_top) / line_h).floor() as usize).clamp(1, 3);
    let up = short_title(&p, c, tw, 12.0);
    let sizes: &[f32] = if tiny {
        &[12.5, 11.5, 10.5]
    } else if narrow {
        &[14.5, 13.0, 12.0]
    } else {
        &[16.0, 14.5, 13.0]
    };
    let f = title_font(&p, &up, tw, sizes);
    let up = ellipsize_single_word(&p, &up, &f, tw);
    let g = p.layout_job(clamp_job(&up, f, txt, tw, rows));
    p.galley(pos2(x0, title_top), g, txt);

    if show_foot {
        let mut foot = String::new();
        if !room.is_empty() && !room_on_top {
            foot += room;
        }
        if !c.prof.is_empty() {
            if !foot.is_empty() {
                foot += " · ";
            }
            foot += &capitalize_words(&c.prof);
        }
        let g = p.layout_job(clamp_job(&foot, regular(if tiny { 11.0 } else { 12.5 }), muted, tw, 1));
        p.galley(pos2(x0, foot_y - g.size().y), g, muted);
    }
    if live {
        let t = ((now - c.start).num_seconds() as f32 / (c.end - c.start).num_seconds().max(1) as f32).clamp(0.0, 1.0);
        let b = Rect::from_min_max(pos2(x0, rect.bottom() - 10.0), pos2(x1, rect.bottom() - 7.0));
        p.rect_filled(b, CornerRadius::same(2), theme::LINE);
        p.rect_filled(Rect::from_min_max(b.min, pos2(b.left() + b.width() * t, b.bottom())), CornerRadius::same(2), kc);
    }
    resp.on_hover_ui(|ui| course_tooltip(ui, c))
}

/// « GARCIA THIERRY » → « Garcia Thierry » (plus doux à lire).
pub fn capitalize_words(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            let lower = w.to_lowercase();
            let mut ch = lower.chars();
            match ch.next() {
                Some(f) => f.to_uppercase().chain(ch).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Pause déjeuner : capsule arrondie cliquable « Pause déj · 1 h 15 ··· MENU RESTO U → »
/// (texte réduit dans les colonnes étroites). Renvoie `true` si elle est cliquée.
pub fn lunch_pill(ui: &mut egui::Ui, r: Rect, gap_min: i64, id: egui::Id) -> bool {
    let resp = ui.interact(r, id, Sense::click());
    let dur = fmt_duration(gap_min);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Menu du resto U, pause de {dur}")));
    let (hv, pr) = motion::press(ui.ctx(), id, resp.hovered(), resp.is_pointer_button_down_on());
    let p = ui.painter().with_clip_rect(r.expand(1.0));
    let rad = CornerRadius::same((r.height() / 2.0).min(18.0) as u8);
    let fill = motion::lerp_color(motion::lerp_color(theme::ACCENT.gamma_multiply(0.10), theme::CARD_HOVER, hv), theme::PRESS, pr);
    p.rect_filled(r, rad, fill);
    p.rect_stroke(r, rad, Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.6)), StrokeKind::Inside);
    let small = r.height() < 26.0;
    let font = mono_bold(if small { 10.5 } else { 12.0 });
    if r.width() >= 280.0 {
        p.text(
            pos2(r.left() + 16.0, r.center().y),
            Align2::LEFT_CENTER,
            format!("Pause déj · {dur}"),
            regular(if small { 12.0 } else { 13.0 }),
            theme::MUTED,
        );
        p.text(pos2(r.right() - 16.0, r.center().y), Align2::RIGHT_CENTER, "MENU RESTO U →", font, theme::ACCENT);
    } else {
        let label = if r.width() >= 150.0 {
            "MENU RESTO U →"
        } else if r.width() >= 95.0 {
            "RESTO U →"
        } else {
            "RESTO"
        };
        p.text(r.center(), Align2::CENTER_CENTER, label, font, theme::ACCENT);
    }
    resp.on_hover_text(format!("Menus et horaires des restos U · pause de {dur}")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Pastille arrondie (« TD », « CM 4 »…) teintée de `color`, comme sur les cartes.
pub fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    let g = ui.painter().layout_no_wrap(text.to_uppercase(), mono_bold(10.5), theme::TEXT);
    let size = g.size() + vec2(10.0, 4.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(6), alpha(color, 90));
    ui.painter().galley(rect.center() - g.size() / 2.0, g, theme::TEXT);
}
