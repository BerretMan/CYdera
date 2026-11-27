//! Vue jour mobile « agenda » : sélecteur de jours en pastilles, puis la frise horaire `timeline`
//! (8 h 00 → 18 h 30 à échelle fixe), partagée avec la vue jour du bureau.

use super::card::{course_card, lunch_pill};
use super::motion;
use super::theme::{self, bold, mono, mono_bold, regular};
use super::{Action, capitalize, date_longue, jour_court};
use crate::cache::Store;
use crate::model::{Cours, fmt_duration, stats};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike, Weekday};
use eframe::egui::{self, Align2, Color32, CornerRadius, Rect, Sense, Stroke, pos2, vec2};

const GUTTER: f32 = 50.0; // heures à gauche
const RAIL_X: f32 = 56.0; // rail vertical
const CARDS_X: f32 = 66.0; // début des cartes

/// Plage horaire fixe de toutes les vues : 8 h 00 → 18 h 30 (en minutes depuis minuit).
pub const DAY_START: i64 = 8 * 60;
pub const DAY_END: i64 = 18 * 60 + 30;
pub const DAY_HOURS: f32 = (DAY_END - DAY_START) as f32 / 60.0;

/// Minutes depuis 8 h 00, bornées à la plage affichée.
pub fn offset_min(t: NaiveDateTime) -> f32 {
    let m = t.hour() as f32 * 60.0 + t.minute() as f32 + t.second() as f32 / 60.0;
    m.clamp(DAY_START as f32, DAY_END as f32) - DAY_START as f32
}

/// Cours qui se chevauchent, regroupés.
pub fn clusters<'a>(day: &[&'a Cours]) -> Vec<Vec<&'a Cours>> {
    let mut out: Vec<Vec<&Cours>> = Vec::new();
    let mut end = None;
    for &c in day {
        match (out.last_mut(), end) {
            (Some(g), Some(e)) if c.start < e => {
                g.push(c);
                end = Some(e.max(c.end));
            }
            _ => {
                out.push(vec![c]);
                end = Some(c.end);
            }
        }
    }
    out
}

/// Répartit les cours qui se chevauchent sur plusieurs « couloirs » : (cours, couloir, nb de couloirs).
pub fn lanes<'a>(day: &[&'a Cours]) -> Vec<(&'a Cours, usize, usize)> {
    let mut out = Vec::new();
    for g in clusters(day) {
        let mut ends: Vec<NaiveDateTime> = Vec::new();
        let mut placed = Vec::new();
        for c in g {
            let lane = match ends.iter().position(|e| *e <= c.start) {
                Some(l) => {
                    ends[l] = c.end;
                    l
                }
                None => {
                    ends.push(c.end);
                    ends.len() - 1
                }
            };
            placed.push((c, lane));
        }
        let n = ends.len().max(1);
        out.extend(placed.into_iter().map(|(c, l)| (c, l, n)));
    }
    out
}

fn span(g: &[&Cours]) -> (NaiveDateTime, NaiveDateTime) {
    let s = g.iter().map(|c| c.start).min().unwrap_or_default();
    let e = g.iter().map(|c| c.end).max().unwrap_or_default();
    (s, e)
}

pub fn is_lunch(end: NaiveDateTime, start: NaiveDateTime) -> bool {
    (start - end).num_minutes() >= 30 && end.hour() < 14 && start.hour() >= 12
}

pub fn show(ui: &mut egui::Ui, store: &Store, selected: NaiveDate, now: NaiveDateTime) -> Option<Action> {
    let mut action = week_pills(ui, store, selected, now.date());
    ui.add_space(12.0);

    let day: Vec<&Cours> = store.day(selected).collect();
    // Titre du jour + résumé en langage courant
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(egui::RichText::new(capitalize(&date_longue(selected))).font(bold(21.0)).color(theme::TEXT));
            ui.label(egui::RichText::new(summary(&day)).font(regular(13.0)).color(theme::MUTED));
        });
    });
    ui.add_space(10.0);

    // Mesurée AVANT la zone de défilement (à l'intérieur, la hauteur disponible est illimitée).
    let viewport_h = ui.available_height();
    let scroll = egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if day.is_empty() {
            empty(ui, store, selected);
            return;
        }
        if let Some(a) = timeline(ui, &day, selected, now, viewport_h - 2.0, 0.0) {
            action = Some(a);
        }
    });
    ui.data_mut(|d| d.insert_temp(egui::Id::new(super::gesture::SCROLL_Y), scroll.state.offset.y));
    action
}

/// « 4 cours · 6 h · 08:30 – 17:45 » ou « Rien de prévu ».
pub fn summary(day: &[&Cours]) -> String {
    match (day.first(), day.iter().map(|c| c.end).max()) {
        (Some(first), Some(last)) => {
            let st = stats(day.iter().copied());
            format!("{} cours · {} · {} – {}", st.seances, fmt_duration(st.minutes), first.start.format("%H:%M"), last.format("%H:%M"))
        }
        _ => "Rien de prévu".to_string(),
    }
}

/// Frise d'une journée à échelle fixe 8 h 00 → 18 h 30 : chaque cours est placé selon son horaire.
/// Rail horaire à gauche (heures pleines, heure de début de chaque cours en gras), cours simultanés
/// côte à côte, capsule resto U dans la pause déjeuner, ligne « maintenant ».
/// La journée remplit `viewport_h` si possible, sans descendre sous `min_hour_px` par heure.
pub fn timeline(
    ui: &mut egui::Ui,
    day: &[&Cours],
    selected: NaiveDate,
    now: NaiveDateTime,
    viewport_h: f32,
    min_hour_px: f32,
) -> Option<Action> {
    let mut action = None;
    let (pad_top, pad_bot) = (10.0, 14.0);
    let hour_px = ((viewport_h - pad_top - pad_bot) / DAY_HOURS).max(min_hour_px);
    let (grid, _) = ui.allocate_exact_size(vec2(ui.available_width(), hour_px * DAY_HOURS + pad_top + pad_bot), Sense::hover());
    let top = grid.top() + pad_top;
    let y_of = |t: NaiveDateTime| top + offset_min(t) / 60.0 * hour_px;
    let left = grid.left();
    let right = grid.right() - 2.0;
    let p = ui.painter().clone();

    // heures de début des cours (en gras dans la gouttière) : les heures pleines trop proches s'effacent
    let starts: Vec<(f32, NaiveDateTime, bool)> = day
        .iter()
        .map(|c| c.start)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|s| (y_of(s), s, day.iter().filter(|c| c.start == s).all(|c| c.end <= now)))
        .collect();

    // grille : heures pleines + demi-heures
    for h in 0..=((DAY_END - DAY_START) / 30) {
        let m = DAY_START + h * 30;
        let y = top + (m - DAY_START) as f32 / 60.0 * hour_px;
        let full = m % 60 == 0 || m == DAY_END;
        let col = if full { theme::LINE_FAINT } else { theme::LINE_FAINT.gamma_multiply(0.45) };
        p.line_segment([pos2(left + CARDS_X - 4.0, y), pos2(right, y)], Stroke::new(1.0, col));
        // l'heure de début en gras occupe ~[ys + 7, ys + 21] : on efface les heures pleines qui la toucheraient
        if full && !starts.iter().any(|(ys, _, _)| y - ys > -12.0 && y - ys < 30.0) {
            let label = format!("{:02}:{:02}", m / 60, m % 60);
            p.text(pos2(left + GUTTER - 4.0, y), Align2::RIGHT_CENTER, label, mono(11.0), theme::DIM);
        }
    }
    // rail sur toute la journée
    p.line_segment([pos2(left + RAIL_X, top), pos2(left + RAIL_X, top + DAY_HOURS * hour_px)], Stroke::new(2.0, theme::LINE));

    // pauses entre groupes de cours
    let groups = clusters(day);
    for w in groups.windows(2) {
        let (pe, ns) = (span(&w[0]).1, span(&w[1]).0);
        let (ya, yb) = (y_of(pe), y_of(ns));
        let gap = (ns - pe).num_minutes();
        if is_lunch(pe, ns) && yb - ya >= 26.0 {
            let bh = (yb - ya - 8.0).min(44.0);
            let r = Rect::from_center_size(pos2((left + CARDS_X + right) / 2.0, (ya + yb) / 2.0), vec2(right - left - CARDS_X, bh));
            if lunch_pill(ui, r, gap, ui.id().with(("resto_pill", pe))) {
                action = Some(Action::Restos);
            }
        } else if gap >= 15 && yb - ya >= 18.0 {
            p.text(
                pos2(left + CARDS_X + 4.0, (ya + yb) / 2.0),
                Align2::LEFT_CENTER,
                format!("Pause · {}", fmt_duration(gap)),
                regular(12.0),
                theme::DIM,
            );
        }
    }

    // cartes, placées selon l'horaire ; côte à côte si plusieurs cours en même temps
    let gap = 8.0;
    let x0 = left + CARDS_X;
    for (c, lane, n) in lanes(day) {
        let w = ((right - x0) - gap * (n as f32 - 1.0)) / n as f32;
        let (ys, ye) = (y_of(c.start) + 1.5, y_of(c.end) - 1.5);
        let r = Rect::from_min_max(pos2(x0 + lane as f32 * (w + gap), ys), pos2(x0 + lane as f32 * (w + gap) + w, ye.max(ys + 18.0)));
        let id = ui.id().with(("agenda", c.start, lane, &c.matiere));
        if course_card(ui, r, c, now, id).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            action = Some(Action::Detail(c.clone()));
        }
    }

    // heures de début + pastilles sur le rail
    for (y, s, past) in &starts {
        let tcol = if *past { theme::DIM } else { theme::TEXT };
        p.text(pos2(left + GUTTER - 4.0, y + 7.0), Align2::RIGHT_TOP, s.format("%H:%M").to_string(), mono_bold(13.0), tcol);
        let first = day.iter().find(|c| c.start == *s).copied();
        let col = first.map_or(theme::ACCENT, |c| theme::kind_color(c.kind));
        let live = day.iter().any(|c| c.start == *s && c.is_now(now));
        let pt = pos2(left + RAIL_X, y + 15.0);
        if live {
            p.circle_filled(pt, 9.0, col.gamma_multiply(0.25));
        }
        p.circle_filled(pt, 5.0, col);
        p.circle_stroke(pt, 5.0, Stroke::new(2.0, theme::CARBON));
    }

    // ligne « maintenant » (seulement aujourd'hui, dans la plage affichée)
    let nm = (now.hour() * 60 + now.minute()) as i64;
    if selected == now.date() && (DAY_START..=DAY_END).contains(&nm) {
        let y = y_of(now);
        p.line_segment([pos2(left + RAIL_X, y), pos2(right, y)], Stroke::new(1.5, theme::NOW.gamma_multiply(0.85)));
        p.circle_filled(pos2(left + RAIL_X, y), 4.5, theme::NOW);
    }
    action
}

fn empty(ui: &mut egui::Ui, store: &Store, d: NaiveDate) {
    ui.add_space(50.0);
    ui.vertical_centered(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
        let p = ui.painter();
        p.circle_filled(r.center(), 32.0, theme::ACCENT.gamma_multiply(0.10));
        // petit soleil : la journée est libre
        p.circle_stroke(r.center(), 10.0, Stroke::new(2.0, theme::ACCENT));
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::TAU / 8.0;
            let (s, c) = a.sin_cos();
            p.line_segment([r.center() + vec2(c, s) * 15.0, r.center() + vec2(c, s) * 20.0], Stroke::new(2.0, theme::ACCENT));
        }
        ui.add_space(10.0);
        ui.label(egui::RichText::new("Aucun cours").font(bold(22.0)).color(theme::TEXT));
        ui.add_space(2.0);
        let msg = if store.week_age_min(d).is_none() {
            "Semaine pas encore synchronisée — tire vers le bas pour actualiser"
        } else if matches!(d.weekday(), Weekday::Sat | Weekday::Sun) {
            "C'est le week-end, profites-en !"
        } else {
            "Journée libre"
        };
        ui.label(egui::RichText::new(msg).font(regular(14.0)).color(theme::MUTED));
    });
}

/// Sélecteur LUN–VEN (+ week-end s'il y a cours) en pastilles arrondies.
fn week_pills(ui: &mut egui::Ui, store: &Store, selected: NaiveDate, today: NaiveDate) -> Option<Action> {
    let mut action = None;
    let lundi = crate::cache::monday(selected);
    let has_we = (5..7).any(|k| store.has_day(lundi + Duration::days(k)));
    let n = if has_we { 7 } else { 5 };
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 60.0), Sense::hover());
    let gap = 6.0;
    let w = (rect.width() - gap * (n as f32 - 1.0)) / n as f32;
    let sel_x = ui.ctx().animate_value_with_time(
        ui.id().with("pill_x"),
        (selected - lundi).num_days().clamp(0, n as i64 - 1) as f32,
        if motion::reduced() { 0.0 } else { 0.18 },
    );
    let painter = ui.painter().clone();
    // pastille de sélection qui glisse d'un jour à l'autre
    let sel_r = Rect::from_min_size(pos2(rect.left() + sel_x * (w + gap), rect.top()), vec2(w, 60.0));
    for k in 0..n {
        let d = lundi + Duration::days(k as i64);
        let r = Rect::from_min_size(pos2(rect.left() + k as f32 * (w + gap), rect.top()), vec2(w, 60.0));
        let resp = ui.interact(r, ui.id().with(("pill", d)), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, d == selected, date_longue(d)));
        let fill = if resp.is_pointer_button_down_on() { theme::PRESS } else { theme::CARD };
        painter.rect_filled(r, CornerRadius::same(14), fill);
        if resp.clicked() {
            action = Some(Action::Select(d));
        }
    }
    painter.rect_filled(sel_r, CornerRadius::same(14), theme::ACCENT);
    for k in 0..n {
        let d = lundi + Duration::days(k as i64);
        let r = Rect::from_min_size(pos2(rect.left() + k as f32 * (w + gap), rect.top()), vec2(w, 60.0));
        // texte noir là où la pastille cyan passe, clair ailleurs
        let on = (1.0 - (sel_x - k as f32).abs()).clamp(0.0, 1.0);
        let base = if d == today { theme::ACCENT } else { theme::MUTED };
        let col = motion::lerp_color(base, Color32::BLACK, on);
        let num = motion::lerp_color(if d == today { theme::ACCENT } else { theme::TEXT }, Color32::BLACK, on);
        painter.text(pos2(r.center().x, r.top() + 8.0), Align2::CENTER_TOP, jour_court(d), mono_bold(10.5), col);
        painter.text(pos2(r.center().x, r.top() + 22.0), Align2::CENTER_TOP, d.day().to_string(), bold(20.0), num);
        if store.has_day(d) {
            painter.circle_filled(pos2(r.center().x, r.bottom() - 8.0), 2.5, motion::lerp_color(theme::DIM, Color32::BLACK, on));
        }
    }
    action
}
