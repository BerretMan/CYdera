use super::agenda::{self, DAY_END, DAY_HOURS, DAY_START};
use super::card::{course_card, lunch_pill};
use super::theme::{self, mono, mono_bold};
use super::{Action, jour_court};
use crate::cache::Store;
use crate::model::{Cours, fmt_duration, stats};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike};
use eframe::egui::{self, Align2, CornerRadius, Rect, Sense, Stroke, pos2, vec2};

const HEADER_H: f32 = 64.0;
const GUTTER_WIDE: f32 = 56.0;
const GUTTER_NARROW: f32 = 44.0;

pub fn show(ui: &mut egui::Ui, store: &Store, monday: NaiveDate, now: NaiveDateTime) -> Option<Action> {
    let mut action = None;
    let week: Vec<&Cours> = store.cours.iter().filter(|c| c.date() >= monday && c.date() < monday + Duration::days(7)).collect();

    let has = |wd: u32| week.iter().any(|c| c.date().weekday().num_days_from_monday() == wd);
    let n_days = if has(6) {
        7
    } else if has(5) {
        6
    } else {
        5
    };

    // plage fixe 8 h 00 → 18 h 30 : un cours est toujours à la même hauteur d'une semaine à l'autre
    let n_hours = DAY_HOURS;
    let today = now.date();

    // ---------- En-tête des jours (fixe) ----------
    let width = ui.available_width();
    #[allow(non_snake_case)]
    let GUTTER = if width < 700.0 { GUTTER_NARROW } else { GUTTER_WIDE };
    let col_w = (width - GUTTER) / n_days as f32;
    let (head, _) = ui.allocate_exact_size(vec2(width, HEADER_H), Sense::hover());
    let painter = ui.painter();
    for i in 0..n_days {
        let d = monday + Duration::days(i as i64);
        let r = Rect::from_min_size(pos2(head.left() + GUTTER + col_w * i as f32, head.top()), vec2(col_w, HEADER_H));
        let resp = ui.interact(r, ui.id().with(("whead", i)), Sense::click());
        if resp.hovered() {
            painter.rect_filled(r.shrink(2.0), CornerRadius::same(4), theme::CARD);
        }
        let is_today = d == today;
        let num_col = if is_today { theme::ACCENT } else { theme::TEXT };
        painter.text(pos2(r.center().x, r.top() + 6.0), Align2::CENTER_TOP, jour_court(d), mono_bold(11.0), theme::MUTED);
        painter.text(pos2(r.center().x, r.top() + 22.0), Align2::CENTER_TOP, format!("{:02}", d.day()), mono_bold(22.0), num_col);
        let day: Vec<&&Cours> = week.iter().filter(|c| c.date() == d).collect();
        if !day.is_empty() && col_w >= 105.0 {
            let st = stats(day.iter().map(|c| **c));
            painter.text(
                pos2(r.center().x, r.bottom() - 4.0),
                Align2::CENTER_BOTTOM,
                if col_w >= 135.0 {
                    format!(
                        "{} SÉANCE{} · {}",
                        st.seances,
                        if st.seances > 1 { "S" } else { "" },
                        fmt_duration(st.minutes).replace(' ', "").to_uppercase()
                    )
                } else {
                    format!("{}S · {}", st.seances, fmt_duration(st.minutes).replace(' ', "").to_uppercase())
                },
                mono(10.5),
                theme::DIM,
            );
        }
        if is_today {
            let bar = Rect::from_center_size(pos2(r.center().x, r.bottom() + 1.0), vec2(28.0, 3.0));
            painter.rect_filled(bar, CornerRadius::same(2), theme::ACCENT);
        }
        if resp.on_hover_text("Ouvrir la vue jour").clicked() {
            action = Some(Action::OpenDay(d));
        }
    }
    painter.line_segment([pos2(head.left(), head.bottom() + 3.0), pos2(head.right(), head.bottom() + 3.0)], Stroke::new(1.0, theme::LINE));
    ui.add_space(6.0);

    // ---------- Grille horaire ----------
    let avail_h = ui.available_height();
    // téléphone : toute la journée tient à l'écran (jamais de défilement) ; bureau : 50 px/h au minimum
    let min_px = if ui.ctx().content_rect().width() < 768.0 { 0.0 } else { 50.0 };
    let px_h = ((avail_h - 16.0) / n_hours).max(min_px);
    let scroll = egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let total_h = px_h * n_hours + 12.0;
        let (grid, _) = ui.allocate_exact_size(vec2(width, total_h), Sense::hover());
        let top = grid.top() + 6.0;
        let y_of = |t: NaiveDateTime| top + agenda::offset_min(t) / 60.0 * px_h;
        let col_x = |i: usize| grid.left() + GUTTER + col_w * i as f32;
        let painter = ui.painter().clone();

        // Colonne du jour
        if let Some(i) = (0..n_days).find(|i| monday + Duration::days(*i as i64) == today) {
            let r = Rect::from_min_max(pos2(col_x(i), grid.top()), pos2(col_x(i) + col_w, grid.bottom()));
            painter.rect_filled(r, CornerRadius::same(4), theme::PANEL);
        }
        // Heures pleines (+ 18:30) et demi-heures
        for k in 0..=((DAY_END - DAY_START) / 30) {
            let m = DAY_START + k * 30;
            let y = top + (m - DAY_START) as f32 / 60.0 * px_h;
            let full = m % 60 == 0 || m == DAY_END;
            if full {
                theme::tabular(
                    &painter,
                    pos2(grid.left() + GUTTER - 8.0, y),
                    Align2::RIGHT_CENTER,
                    &format!("{:02}:{:02}", m / 60, m % 60),
                    mono(11.0),
                    theme::DIM,
                );
            }
            let col = if full { theme::LINE_FAINT } else { theme::LINE_FAINT.gamma_multiply(0.45) };
            painter.line_segment([pos2(grid.left() + GUTTER, y), pos2(grid.right(), y)], Stroke::new(1.0, col));
        }
        for i in 1..n_days {
            painter.line_segment([pos2(col_x(i), grid.top()), pos2(col_x(i), grid.bottom())], Stroke::new(1.0, theme::LINE_FAINT));
        }

        // Cours
        for i in 0..n_days {
            let d = monday + Duration::days(i as i64);
            let mut day: Vec<&Cours> = week.iter().copied().filter(|c| c.date() == d).collect();
            day.sort_by_key(|c| (c.start, c.end));

            // Chaque jour est strictement confiné à sa colonne (aucun débordement sur le jour suivant).
            for (c, lane, lanes) in agenda::lanes(&day) {
                let lw = (col_w - 8.0) / lanes as f32;
                let x0 = col_x(i) + 4.0 + lw * lane as f32;
                let (ys, ye) = (y_of(c.start) + 1.5, y_of(c.end) - 1.5);
                let r = Rect::from_min_max(pos2(x0, ys), pos2(x0 + lw - 3.0, ye.max(ys + 18.0)));
                let id = ui.id().with(("block", c.start, &c.matiere, lane));
                if course_card(ui, r, c, now, id).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    action = Some(Action::Detail(c.clone()));
                }
            }

            // Pause déjeuner
            for pair in day.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let gap = (b.start - a.end).num_minutes();
                if agenda::is_lunch(a.end, b.start) && day.iter().all(|c| c.end <= b.start || c.start >= a.end) {
                    let (ya, yb) = (y_of(a.end), y_of(b.start));
                    let h = yb - ya;
                    if h >= 22.0 {
                        let r =
                            Rect::from_center_size(pos2(col_x(i) + col_w / 2.0, (ya + yb) / 2.0), vec2(col_w - 8.0, (h - 6.0).min(36.0)));
                        if lunch_pill(ui, r, gap, ui.id().with(("lunch", i))) {
                            action = Some(Action::Restos);
                        }
                    }
                }
            }
        }

        // Ligne « maintenant »
        if let Some(i) = (0..n_days).find(|i| monday + Duration::days(*i as i64) == today) {
            let t = now.time();
            let m = (t.hour() * 60 + t.minute()) as i64;
            if (DAY_START..=DAY_END).contains(&m) {
                let y = y_of(now);
                painter.line_segment([pos2(col_x(i), y), pos2(col_x(i) + col_w, y)], Stroke::new(2.0, theme::NOW));
                painter.circle_filled(pos2(col_x(i), y), 4.5, theme::NOW);
                let label = Rect::from_center_size(pos2(grid.left() + GUTTER / 2.0 - 4.0, y), vec2(GUTTER.min(44.0), 18.0));
                painter.rect_filled(label, CornerRadius::same(2), theme::NOW);
                theme::tabular(
                    &painter,
                    label.center(),
                    Align2::CENTER_CENTER,
                    &now.format("%H:%M").to_string(),
                    mono_bold(10.5),
                    theme::TEXT,
                );
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(egui::Id::new(super::gesture::SCROLL_Y), scroll.state.offset.y));
    action
}
