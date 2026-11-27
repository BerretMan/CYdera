use super::theme::{self, bold, mono, mono_bold};
use super::{Action, date_longue, jour_court};
use crate::cache::Store;
use crate::model::{Cours, fmt_duration, stats};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Weekday};
use eframe::egui::{self, Align2, CornerRadius, Rect, Sense, Stroke, pos2, vec2};

const STRIP_DAYS: i64 = 4; // de chaque côté du jour sélectionné

pub fn show(ui: &mut egui::Ui, store: &Store, selected: NaiveDate, now: NaiveDateTime, compact: bool) -> Option<Action> {
    if compact {
        return super::agenda::show(ui, store, selected, now);
    }
    let mut action = strip(ui, store, selected, now.date());
    ui.add_space(14.0);

    let day: Vec<&Cours> = store.day(selected).collect();
    let max_w = 900.0;
    let pad = ((ui.available_width() - max_w) / 2.0).max(0.0);

    // Résumé du jour
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if pad > 0.0 {
            ui.add_space(pad);
        }
        ui.label(egui::RichText::new(date_longue(selected).to_uppercase()).font(bold(20.0)));
        if let (Some(first), Some(last)) = (day.first(), day.last()) {
            let st = stats(day.iter().copied());
            ui.label(
                egui::RichText::new(format!(
                    "{} SÉANCE{} · {} · {}—{}",
                    st.seances,
                    if st.seances > 1 { "S" } else { "" },
                    fmt_duration(st.minutes).replace(' ', "").to_uppercase(),
                    first.start.format("%H:%M"),
                    day.iter().map(|c| c.end).max().unwrap_or(last.end).format("%H:%M")
                ))
                .font(mono(12.0))
                .color(theme::MUTED),
            );
        }
    });
    ui.add_space(10.0);

    // Mesurée AVANT la zone de défilement (à l'intérieur, la hauteur disponible est illimitée).
    let viewport_h = ui.available_height();
    let scroll = egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if pad > 0.0 {
                ui.add_space(pad);
            }
            ui.vertical(|ui| {
                ui.set_width(ui.available_width().min(max_w) - 12.0);
                if day.is_empty() {
                    empty(ui, store, selected);
                    return;
                }
                if let Some(a) = super::agenda::timeline(ui, &day, selected, now, viewport_h, 56.0) {
                    action = Some(a);
                }
            });
        });
    });
    ui.data_mut(|d| d.insert_temp(egui::Id::new(super::gesture::SCROLL_Y), scroll.state.offset.y));
    action
}

/// Bandeau horizontal de jours façon CYapp.
fn strip(ui: &mut egui::Ui, store: &Store, selected: NaiveDate, today: NaiveDate) -> Option<Action> {
    let mut action = None;
    let n = (STRIP_DAYS * 2 + 1) as usize;
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 84.0), Sense::hover());
    let item_w = width / n as f32;
    let painter = ui.painter().clone();

    for k in 0..n {
        let off = k as i64 - STRIP_DAYS;
        let d = selected + Duration::days(off);
        let r = Rect::from_min_size(pos2(rect.left() + item_w * k as f32, rect.top()), vec2(item_w, 76.0));
        let item = ui.interact(r, ui.id().with(("strip", d)), Sense::click());
        if item.hovered() && off != 0 {
            painter.rect_filled(r.shrink(3.0), CornerRadius::same(4), theme::CARD);
        }
        let weekend = matches!(d.weekday(), Weekday::Sat | Weekday::Sun);
        let mut col = if off == 0 {
            theme::TEXT
        } else if weekend {
            theme::DIM
        } else {
            theme::MUTED
        };
        if d == today && off != 0 {
            col = theme::ACCENT;
        }
        if off.abs() == STRIP_DAYS {
            col = col.gamma_multiply(0.45);
        }
        let (num_font, lbl_font) = if off == 0 { (mono_bold(30.0), mono_bold(13.0)) } else { (mono(24.0), mono(12.5)) };
        painter.text(pos2(r.center().x, r.top() + 4.0), Align2::CENTER_TOP, format!("{:02}", d.day()), num_font, col);
        painter.text(pos2(r.center().x, r.top() + 44.0), Align2::CENTER_TOP, jour_court(d), lbl_font, col);
        if store.has_day(d) {
            let dot = if off == 0 { theme::ACCENT } else { col };
            painter.circle_filled(pos2(r.center().x, r.bottom() + 1.0), 2.5, dot);
        }
        if item.clicked() {
            action = Some(Action::Select(d));
        }
    }
    painter.line_segment([pos2(rect.left(), rect.bottom()), pos2(rect.right(), rect.bottom())], Stroke::new(1.0, theme::LINE));

    // Molette sur le bandeau → jour précédent / suivant
    if resp.contains_pointer() {
        let acc_id = ui.id().with("strip_scroll");
        let delta = ui.input(|i| i.smooth_scroll_delta);
        let mut acc: f32 = ui.data(|d| d.get_temp(acc_id)).unwrap_or(0.0);
        acc -= delta.x + delta.y;
        if acc.abs() >= 40.0 {
            action = Some(Action::Select(selected + Duration::days(acc.signum() as i64)));
            acc = 0.0;
        }
        ui.data_mut(|d| d.insert_temp(acc_id, acc));
    }
    action
}

fn empty(ui: &mut egui::Ui, store: &Store, d: NaiveDate) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new("Aucun cours").font(bold(26.0)).color(theme::MUTED));
        ui.add_space(4.0);
        let msg = if store.week_age_min(d).is_none() {
            "Semaine pas encore synchronisée…"
        } else if matches!(d.weekday(), Weekday::Sat | Weekday::Sun) {
            "C'est le week-end, profites-en !"
        } else {
            "Journée libre"
        };
        ui.label(egui::RichText::new(msg).color(theme::DIM));
    });
}
