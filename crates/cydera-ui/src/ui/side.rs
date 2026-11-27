//! Panneau « Maintenant » : cours en cours, prochain cours et suite de la journée
//! (reprend la logique `renderUI` du script Obsidian).

use super::card::{capitalize_words, card_fill, pill, room_tag};
use super::theme::{self, bold, mono, mono_bold, regular};
use super::{Action, capitalize, date_longue, jour, mois};
use crate::cache::{Store, monday};
use crate::model::{Cours, fmt_duration, stats};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};
use eframe::egui::{self, CornerRadius, Margin, Rect, Sense, Stroke, pos2, vec2};

pub fn show(ui: &mut egui::Ui, store: &Store, now: NaiveDateTime, week: NaiveDate) -> Option<Action> {
    let mut action = None;
    let today = now.date();

    super::telemetry::card(ui, store, now);
    ui.add_space(12.0);

    let futurs: Vec<&Cours> = store.cours.iter().filter(|c| c.end > now).collect();
    let (en_cours, rest) = match futurs.first() {
        Some(c) if c.start <= now => (Some(*c), &futurs[1..]),
        _ => (None, &futurs[..]),
    };

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if let Some(c) = en_cours {
            let left = (c.end - now).num_minutes().max(0);
            let sub = format!("Reste {} · fin à {}", fmt_duration(left), c.end.format("%H:%M"));
            if focus_card(ui, c, "• EN COURS", theme::OK, &sub, Some(now)) {
                action = Some(Action::Detail(c.clone()));
            }
            ui.add_space(10.0);
        }

        if let Some(next) = rest.first() {
            let sub = if next.date() == today {
                format!("Dans {} · à {}", fmt_duration((next.start - now).num_minutes().max(0)), next.start.format("%H:%M"))
            } else if next.date() == today + Duration::days(1) {
                format!("Demain à {}", next.start.format("%H:%M"))
            } else {
                format!("{} à {}", capitalize(&date_longue(next.date())), next.start.format("%H:%M"))
            };
            if focus_card(ui, next, "PROCHAIN COURS", theme::ACCENT, &sub, None) {
                action = Some(Action::Detail((*next).clone()));
            }

            let suite: Vec<&&Cours> = rest[1..].iter().take_while(|c| c.date() == next.date()).collect();
            if !suite.is_empty() {
                ui.add_space(14.0);
                let titre = if next.date() == today {
                    "ENSUITE AUJOURD'HUI".to_string()
                } else {
                    format!("ENSUITE {}", jour(next.date()).to_uppercase())
                };
                ui.label(egui::RichText::new(titre).font(bold(11.5)).color(theme::DIM));
                ui.add_space(2.0);
                for c in suite {
                    if compact_row(ui, c) {
                        action = Some(Action::Detail((**c).clone()));
                    }
                }
            }
        } else if en_cours.is_none() {
            theme::card(ui, Margin::same(18), |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Plus aucun cours prévu").color(theme::MUTED));
                });
            });
        }

        // Bilan de la semaine affichée (suit la navigation)
        ui.add_space(18.0);
        week_summary(ui, store, now, monday(week));
    });
    action
}

/// Carte « en cours / prochain cours », au style des cartes de l'agenda : coins arrondis, teinte du
/// type, barre d'accent, pastille de type, étiquette de salle arrondie, barre de progression.
fn focus_card(ui: &mut egui::Ui, c: &Cours, status: &str, status_col: egui::Color32, sub: &str, progress: Option<NaiveDateTime>) -> bool {
    let col = theme::kind_color(c.kind);
    let stroke = if progress.is_some() { Stroke::new(1.5, col) } else { Stroke::new(1.0, theme::LINE) };
    let inner = egui::Frame::new()
        .fill(card_fill(c.kind, false))
        .stroke(stroke)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin { left: 18, right: 12, top: 10, bottom: 12 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 7.0;
                pill(ui, &theme::kind_tag(c), col);
                ui.label(egui::RichText::new(status).font(mono_bold(10.5)).color(status_col));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    theme::mono_label(ui, format!("{}—{}", c.start.format("%H:%M"), c.end.format("%H:%M")), 11.5, theme::DIM);
                });
            });
            ui.add_space(2.0);
            ui.add(egui::Label::new(egui::RichText::new(c.matiere.to_uppercase()).font(bold(18.0))).wrap());
            if !c.prof.is_empty() {
                ui.label(egui::RichText::new(capitalize_words(&c.prof)).font(regular(13.0)).color(theme::MUTED));
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if !c.salle_courte.is_empty() {
                    let font = mono_bold(12.0);
                    let w = ui.painter().layout_no_wrap(c.salle_courte.clone(), font.clone(), theme::TEXT).size().x + 12.0;
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::hover());
                    room_tag(ui.painter(), r.center(), egui::Align2::CENTER_CENTER, &c.salle_courte, font, theme::TEXT);
                    resp.on_hover_text(&c.salle);
                }
                ui.label(egui::RichText::new(sub).font(regular(12.5)).color(theme::TEXT));
            });
            if let Some(now) = progress {
                let t = ((now - c.start).num_seconds() as f32 / (c.end - c.start).num_seconds().max(1) as f32).clamp(0.0, 1.0);
                ui.add_space(6.0);
                let (b, _) = ui.allocate_exact_size(vec2(ui.available_width(), 3.0), Sense::hover());
                ui.painter().rect_filled(b, CornerRadius::same(2), theme::LINE);
                ui.painter().rect_filled(Rect::from_min_max(b.min, pos2(b.left() + b.width() * t, b.bottom())), CornerRadius::same(2), col);
            }
        });
    let r = inner.response.rect;
    if c.kind == crate::model::Kind::Indispo {
        theme::hatch(&ui.painter().with_clip_rect(r.shrink(3.0)), r, 8.0, Stroke::new(1.0, theme::WARN.gamma_multiply(0.1)));
    }
    ui.painter().rect_filled(
        Rect::from_min_max(pos2(r.left() + 7.0, r.top() + 10.0), pos2(r.left() + 10.0, r.bottom() - 10.0)),
        CornerRadius::same(2),
        col,
    );
    let resp = ui.interact(r, ui.id().with(("focus_click", c.start, &c.matiere)), Sense::click());
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

fn compact_row(ui: &mut egui::Ui, c: &Cours) -> bool {
    let col = theme::kind_color(c.kind);
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 44.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() || resp.has_focus() {
        p.rect_filled(r, CornerRadius::same(10), theme::CARD_HOVER);
    }
    if resp.has_focus() {
        p.rect_stroke(r, CornerRadius::same(10), Stroke::new(2.0, theme::ACCENT), egui::StrokeKind::Inside);
    }
    p.rect_filled(
        Rect::from_min_max(pos2(r.left() + 6.0, r.top() + 8.0), pos2(r.left() + 9.0, r.bottom() - 8.0)),
        CornerRadius::same(2),
        col,
    );
    theme::tabular(
        p,
        pos2(r.left() + 18.0, r.top() + 6.0),
        egui::Align2::LEFT_TOP,
        &c.start.format("%H:%M").to_string(),
        mono_bold(12.5),
        theme::TEXT,
    );
    theme::tabular(
        p,
        pos2(r.left() + 18.0, r.bottom() - 6.0),
        egui::Align2::LEFT_BOTTOM,
        &c.end.format("%H:%M").to_string(),
        mono(11.0),
        theme::DIM,
    );
    let text_rect = Rect::from_min_max(pos2(r.left() + 72.0, r.top()), pos2(r.right() - 6.0, r.bottom()));
    let pc = p.with_clip_rect(text_rect);
    pc.text(pos2(text_rect.left(), r.top() + 6.0), egui::Align2::LEFT_TOP, c.matiere.to_uppercase(), bold(12.5), theme::TEXT);
    pc.text(pos2(text_rect.left(), r.bottom() - 6.0), egui::Align2::LEFT_BOTTOM, &c.salle_courte, mono(11.0), theme::MUTED);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

fn week_summary(ui: &mut egui::Ui, store: &Store, now: NaiveDateTime, lundi: NaiveDate) {
    let dimanche = lundi + Duration::days(6);
    let semaine: Vec<&Cours> = store.cours.iter().filter(|c| c.date() >= lundi && c.date() <= dimanche).collect();
    let titre = if lundi == monday(now.date()) {
        "CETTE SEMAINE".to_string()
    } else if lundi == monday(now.date()) + Duration::days(7) {
        "SEMAINE PROCHAINE".to_string()
    } else if lundi == monday(now.date()) - Duration::days(7) {
        "SEMAINE DERNIÈRE".to_string()
    } else {
        format!("SEMAINE DU {} {}", lundi.day(), mois(lundi).to_uppercase())
    };
    ui.label(egui::RichText::new(titre).font(bold(11.5)).color(theme::DIM));
    if semaine.is_empty() {
        let msg = if store.week_age_min(lundi).is_none() { "Pas encore synchronisée" } else { "Aucun cours" };
        ui.label(egui::RichText::new(msg).color(theme::MUTED));
        return;
    }
    let total = stats(semaine.iter().copied());
    ui.label(
        egui::RichText::new(format!(
            "{} séance{} · {}",
            total.seances,
            if total.seances > 1 { "s" } else { "" },
            fmt_duration(total.minutes)
        ))
        .font(bold(15.0)),
    );
    if lundi <= now.date() && now.date() <= dimanche {
        let reste = stats(semaine.iter().copied().filter(|c| c.start > now));
        ui.label(
            egui::RichText::new(format!(
                "{} restante{} · {}",
                reste.seances,
                if reste.seances > 1 { "s" } else { "" },
                fmt_duration(reste.minutes)
            ))
            .color(theme::MUTED),
        );
    }
    // Répartition par type
    let mut parts = Vec::new();
    for (k, nom) in [
        (crate::model::Kind::Cm, "CM"),
        (crate::model::Kind::Td, "TD"),
        (crate::model::Kind::Tp, "TP"),
        (crate::model::Kind::Exam, "Examens"),
    ] {
        let st = stats(semaine.iter().copied().filter(|c| c.kind == k));
        if st.seances > 0 {
            parts.push((nom, st.seances, theme::kind_color(k)));
        }
    }
    ui.horizontal_wrapped(|ui| {
        for (nom, n, col) in parts {
            pill(ui, &format!("{nom} {n}"), col);
        }
    });
}

/// Prochain cours : (en cours, suivant).
fn next_up(store: &Store, now: NaiveDateTime) -> (Option<&Cours>, Option<&Cours>) {
    let futurs: Vec<&Cours> = store.cours.iter().filter(|c| c.end > now).collect();
    match futurs.first() {
        Some(c) if c.start <= now => (Some(*c), futurs.get(1).copied()),
        _ => (None, futurs.first().copied()),
    }
}

fn when_text(c: &Cours, now: NaiveDateTime, running: bool) -> String {
    let today = now.date();
    if running {
        format!("Reste {} · fin à {}", fmt_duration((c.end - now).num_minutes().max(0)), c.end.format("%H:%M"))
    } else if c.date() == today {
        format!("Dans {} · à {}", fmt_duration((c.start - now).num_minutes().max(0)), c.start.format("%H:%M"))
    } else if c.date() == today + Duration::days(1) {
        format!("Demain à {}", c.start.format("%H:%M"))
    } else {
        format!("{} à {}", capitalize(&date_longue(c.date())), c.start.format("%H:%M"))
    }
}

/// Bandeau de télémétrie (fenêtre moyenne ou mobile) : horloge LCD + compte à rebours,
/// prochain cours, barres de progression.
pub fn banner(ui: &mut egui::Ui, store: &Store, now: NaiveDateTime, _week: NaiveDate, wide: bool) -> Option<Action> {
    use super::telemetry as tm;
    let (en_cours, next) = next_up(store, now);
    let focus = en_cours.or(next);
    let (label, value, _) = tm::countdown(store, now);
    let col = focus.map_or(theme::ACCENT, |c| theme::kind_color(c.kind));
    let mut action = None;

    let inner = theme::card_ex(ui, theme::CARD, Stroke::new(1.0, theme::LINE), 3, Margin::ZERO, |ui| {
        ui.set_width(ui.available_width());
        tm::module_header(ui, "SYS://TÉLÉMÉTRIE", &tm::date_short(now));
        egui::Frame::new().inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
            let total = ui.available_width();
            let course = |ui: &mut egui::Ui, action: &mut Option<Action>| {
                let Some(c) = focus else {
                    ui.label(egui::RichText::new("PLUS AUCUN COURS PRÉVU").font(bold(14.0)).color(theme::MUTED));
                    return;
                };
                let ccol = theme::kind_color(c.kind);
                ui.horizontal(|ui| {
                    pill(ui, &theme::kind_tag(c), ccol);
                    ui.label(egui::RichText::new(when_text(c, now, en_cours.is_some())).font(mono(11.0)).color(theme::MUTED));
                });
                ui.add(egui::Label::new(egui::RichText::new(c.matiere.to_uppercase()).font(bold(16.0))).truncate());
                let mut meta = format!("{}—{}", c.start.format("%H:%M"), c.end.format("%H:%M"));
                if !c.salle_courte.is_empty() {
                    meta += &format!(" · {}", c.salle_courte);
                }
                if !c.prof.is_empty() {
                    meta += &format!(" · {}", c.prof);
                }
                ui.add(egui::Label::new(egui::RichText::new(meta).font(mono(11.0)).color(theme::MUTED)).truncate());
                let r = ui.min_rect();
                let resp = ui.interact(r, ui.id().with("banner_click"), Sense::click());
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    *action = Some(Action::Detail(c.clone()));
                }
            };
            if wide {
                let clock_w = 150.0;
                let bars_w = if total > 880.0 { 210.0 } else { 0.0 };
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(clock_w);
                        tm::lcd_clock(ui, now, 40.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            theme::mono_label(ui, label, 10.5, theme::DIM);
                            ui.label(egui::RichText::new(&value).font(mono_bold(14.0)).color(theme::ACCENT));
                        });
                    });
                    let (sep, _) = ui.allocate_exact_size(vec2(1.0, 62.0), Sense::hover());
                    ui.painter().rect_filled(sep, CornerRadius::ZERO, theme::LINE);
                    let mid_w = (ui.available_width() - bars_w - if bars_w > 0.0 { 14.0 } else { 0.0 }).max(120.0);
                    ui.vertical(|ui| {
                        ui.set_width(mid_w);
                        course(ui, &mut action);
                    });
                    if bars_w > 0.0 {
                        let (sep, _) = ui.allocate_exact_size(vec2(1.0, 62.0), Sense::hover());
                        ui.painter().rect_filled(sep, CornerRadius::ZERO, theme::LINE);
                        ui.vertical(|ui| {
                            ui.set_width(bars_w - 12.0);
                            ui.add_space(6.0);
                            tm::bars(ui, store, now, bars_w - 12.0);
                        });
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    tm::lcd_clock(ui, now, 34.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(&value).font(mono_bold(17.0)).color(theme::ACCENT));
                            theme::mono_label(ui, label, 10.5, theme::DIM);
                        });
                    });
                });
                ui.add_space(4.0);
                course(ui, &mut action);
                ui.add_space(4.0);
                tm::bars(ui, store, now, total);
            }
        });
    });
    let r = inner.response.rect;
    theme::corner_ticks(ui.painter(), r, theme::ACCENT.gamma_multiply(0.7));
    ui.painter().rect_filled(Rect::from_min_max(pos2(r.left(), r.top() + 22.0), pos2(r.left() + 3.0, r.bottom())), CornerRadius::ZERO, col);
    action
}

/// Capsule mobile « prochain cours » : pastille de couleur, horaire, matière, compte à rebours.
/// Un appui ouvre la fiche du cours en cours ou du prochain.
pub fn banner_mini(ui: &mut egui::Ui, store: &Store, now: NaiveDateTime) -> Option<Action> {
    use super::telemetry as tm;
    let (label, value, _) = tm::countdown(store, now);
    let (en_cours, next) = next_up(store, now);
    let focus = en_cours.or(next);
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
    let (hv, pr) = super::motion::press(ui.ctx(), resp.id, resp.hovered(), resp.is_pointer_button_down_on());
    let p = ui.painter();
    let fill = super::motion::lerp_color(super::motion::lerp_color(theme::CARD, theme::CARD_HOVER, hv), theme::PRESS, pr);
    p.rect_filled(r, CornerRadius::same(23), fill);
    p.rect_stroke(r, CornerRadius::same(23), Stroke::new(1.0, theme::LINE), egui::StrokeKind::Inside);
    let cy = r.center().y;
    let Some(c) = focus else {
        p.text(
            pos2(r.left() + 18.0, cy),
            egui::Align2::LEFT_CENTER,
            "Plus aucun cours prévu",
            egui::FontId::proportional(14.0),
            theme::MUTED,
        );
        p.text(pos2(r.right() - 18.0, cy), egui::Align2::RIGHT_CENTER, now.format("%H:%M").to_string(), mono_bold(14.0), theme::TEXT);
        return None;
    };
    let kc = theme::kind_color(c.kind);
    let live = en_cours.is_some();
    // pastille : pleine si en cours, anneau sinon
    let dot = pos2(r.left() + 20.0, cy);
    if live {
        p.circle_filled(dot, 9.0, kc.gamma_multiply(0.25));
        p.circle_filled(dot, 5.0, kc);
    } else {
        p.circle_stroke(dot, 4.5, Stroke::new(2.0, kc));
    }
    // compte à rebours dans une pilule à droite
    let (lead, v) = (if label == "RESTE" { "reste" } else { "dans" }, value.to_lowercase());
    let vg = p.layout_no_wrap(v, mono_bold(13.0), theme::ACCENT);
    let vr = Rect::from_min_max(pos2(r.right() - 12.0 - vg.size().x - 18.0, cy - 14.0), pos2(r.right() - 8.0, cy + 14.0));
    p.rect_filled(vr, CornerRadius::same(14), theme::ACCENT.gamma_multiply(0.14));
    p.galley(pos2(vr.center().x - vg.size().x / 2.0 - 2.0, cy - vg.size().y / 2.0), vg, theme::ACCENT);
    let lg = p.layout_no_wrap(lead.into(), egui::FontId::proportional(12.0), theme::DIM);
    let lx = vr.left() - 6.0 - lg.size().x;
    p.galley(pos2(lx, cy - lg.size().y / 2.0), lg, theme::DIM);
    // horaire + matière
    let x0 = r.left() + 36.0;
    let tg = p.layout_no_wrap(c.start.format("%H:%M").to_string(), mono_bold(14.0), theme::TEXT);
    let tw = tg.size().x;
    p.galley(pos2(x0, cy - tg.size().y / 2.0), tg, theme::TEXT);
    let mw = lx - 10.0 - (x0 + tw + 8.0);
    if mw > 30.0 {
        let mut job = egui::text::LayoutJob::single_section(
            super::capitalize(&c.matiere.to_lowercase()),
            egui::TextFormat { font_id: egui::FontId::proportional(14.0), color: theme::MUTED, ..Default::default() },
        );
        job.wrap.max_width = mw;
        job.wrap.max_rows = 1;
        job.wrap.overflow_character = Some('…');
        let g = p.layout_job(job);
        p.galley(pos2(x0 + tw + 8.0, cy - g.size().y / 2.0), g, theme::MUTED);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Ouvrir {}", c.matiere)));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked().then(|| Action::Detail(c.clone()))
}
