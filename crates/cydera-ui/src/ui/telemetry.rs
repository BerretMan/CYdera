//! Télémétrie : horloge LCD, compte à rebours, barres de progression jour / semaine.

use super::theme::{self, mono, mono_bold};
use super::{jour_court, mois};
use crate::cache::Store;
use crate::model::{Cours, Kind};
use chrono::{Datelike, NaiveDateTime, Timelike};
use eframe::egui::{self, Align2, Color32, CornerRadius, Rect, Sense, Stroke, pos2, vec2};

/// (libellé, valeur, cours visé) : « DANS 14H 24M » ou « RESTE 0H 45M ».
pub fn countdown(store: &Store, now: NaiveDateTime) -> (&'static str, String, Option<&Cours>) {
    let futurs: Vec<&Cours> = store.cours.iter().filter(|c| c.end > now && c.kind != Kind::Indispo).collect();
    let Some(first) = futurs.first().copied() else { return ("", "—".into(), None) };
    let (label, target, c) = if first.start <= now { ("RESTE", first.end, first) } else { ("DANS", first.start, first) };
    let m = (target - now).num_minutes().max(0);
    let value = if m >= 24 * 60 {
        format!("{}J {:02}H", m / 1440, m % 1440 / 60)
    } else if m >= 60 {
        format!("{}H {:02}M", m / 60, m % 60)
    } else {
        format!("{}M", m)
    };
    (label, value, Some(c))
}

/// Avancement de la journée : du premier début au dernier cours (8 h–19 h sans cours).
pub fn day_progress(store: &Store, now: NaiveDateTime) -> f32 {
    let today: Vec<&Cours> = store.day(now.date()).filter(|c| c.kind != Kind::Indispo).collect();
    let (a, b) = match (today.iter().map(|c| c.start).min(), today.iter().map(|c| c.end).max()) {
        (Some(a), Some(b)) => (a, b),
        _ => (now.date().and_hms_opt(8, 0, 0).unwrap(), now.date().and_hms_opt(19, 0, 0).unwrap()),
    };
    ((now - a).num_seconds() as f32 / (b - a).num_seconds().max(1) as f32).clamp(0.0, 1.0)
}

/// Avancement de la semaine : lundi 00:00 → samedi 00:00.
pub fn week_progress(now: NaiveDateTime) -> f32 {
    let d = now.date().weekday().num_days_from_monday() as f32;
    let h = now.time().hour() as f32 + now.time().minute() as f32 / 60.0;
    ((d * 24.0 + h) / (5.0 * 24.0)).clamp(0.0, 1.0)
}

/// Horloge façon afficheur LCD : chiffres fantômes « 88:88 » derrière, deux-points qui respirent.
pub fn lcd_clock(ui: &mut egui::Ui, now: NaiveDateTime, size: f32) {
    let font = mono_bold(size);
    let p = ui.painter().clone();
    let cw = p.layout_no_wrap("0".into(), font.clone(), Color32::WHITE).size().x;
    let h = p.layout_no_wrap("0".into(), font.clone(), Color32::WHITE).size().y;
    let (rect, _) = ui.allocate_exact_size(vec2(cw * 5.0, h), Sense::hover());
    let text = now.format("%H:%M").to_string();
    let a = theme::blink(ui.ctx());
    for (i, (ghost, real)) in "88:88".chars().zip(text.chars()).enumerate() {
        let pos = pos2(rect.left() + cw * i as f32 + cw / 2.0, rect.center().y);
        let colon = i == 2;
        let ghost_col = Color32::from_rgba_unmultiplied(255, 255, 255, if colon { 0 } else { 14 });
        p.text(pos, Align2::CENTER_CENTER, ghost, font.clone(), ghost_col);
        let col = if colon { theme::TEXT.gamma_multiply(a) } else { theme::TEXT };
        p.text(pos, Align2::CENTER_CENTER, real, font.clone(), col);
    }
}

/// Barre segmentée façon VU-mètre.
pub fn seg_bar(ui: &mut egui::Ui, width: f32, frac: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 9.0), Sense::hover());
    let n = ((width / 6.0).floor() as usize).max(4);
    let seg = width / n as f32;
    let on = (frac * n as f32).round() as usize;
    for i in 0..n {
        let r = Rect::from_min_size(pos2(rect.left() + seg * i as f32, rect.top()), vec2((seg - 2.0).max(1.0), rect.height()));
        ui.painter().rect_filled(r, CornerRadius::ZERO, if i < on { color } else { theme::LINE });
    }
}

/// Deux lignes « JOUR ▮▮▮▯▯ 62% » / « SEM ▮▮▯▯▯ 31% ».
pub fn bars(ui: &mut egui::Ui, store: &Store, now: NaiveDateTime, width: f32) {
    for (label, f, col) in [("JOUR", day_progress(store, now), theme::ACCENT), ("SEM ", week_progress(now), theme::CM)] {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            theme::mono_label(ui, label, 10.5, theme::DIM);
            seg_bar(ui, (width - 80.0).max(40.0), f, col);
            theme::mono_label(ui, format!("{:>3.0}%", f * 100.0), 10.5, theme::MUTED);
        });
    }
}

/// « JEU 26 NOV »
pub fn date_short(now: NaiveDateTime) -> String {
    let d = now.date();
    let m: String = mois(d).to_uppercase().chars().take(3).collect();
    format!("{} {:02} {}", jour_court(d), d.day(), m)
}

/// Barre d'en-tête d'un module de télémétrie.
pub fn module_header(ui: &mut egui::Ui, left: &str, right: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    ui.painter().rect_filled(r, egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, theme::LINE_FAINT);
    ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, theme::LINE));
    // petit voyant plein devant le nom du module, comme une LED de façade
    ui.painter().rect_filled(
        Rect::from_center_size(pos2(r.left() + 10.0, r.center().y), vec2(5.0, 5.0)),
        CornerRadius::ZERO,
        theme::ACCENT,
    );
    ui.painter().text(pos2(r.left() + 20.0, r.center().y), Align2::LEFT_CENTER, left, mono_bold(10.5), theme::ACCENT);
    ui.painter().text(pos2(r.right() - 8.0, r.center().y), Align2::RIGHT_CENTER, right, mono(10.5), theme::DIM);
}

/// Carte de télémétrie verticale (panneau latéral).
pub fn card(ui: &mut egui::Ui, store: &Store, now: NaiveDateTime) {
    let (label, value, next) = countdown(store, now);
    let inner = theme::card_ex(ui, theme::CARD, Stroke::new(1.0, theme::LINE), 3, egui::Margin::ZERO, |ui| {
        ui.set_width(ui.available_width());
        module_header(ui, "SYS://TÉLÉMÉTRIE", &date_short(now));
        egui::Frame::new().inner_margin(egui::Margin::same(14)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            lcd_clock(ui, now, (ui.available_width() / 3.1).clamp(40.0, 62.0));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                theme::mono_label(ui, label, 11.0, theme::DIM);
                ui.label(egui::RichText::new(&value).font(mono_bold(19.0)).color(theme::ACCENT));
            });
            if let Some(c) = next {
                ui.add(
                    egui::Label::new(egui::RichText::new(c.matiere.to_uppercase()).font(theme::bold(11.5)).color(theme::MUTED)).truncate(),
                );
            }
            ui.add_space(8.0);
            bars(ui, store, now, ui.available_width());
        });
    });
    theme::corner_ticks(ui.painter(), inner.response.rect, theme::ACCENT.gamma_multiply(0.7));
}
