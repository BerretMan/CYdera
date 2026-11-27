//! Écran « Statistique » : temps de révision par jour ou par semaine, en détail.
//! Les calculs sont dans `core::stats` ; ici uniquement l'affichage.

use super::theme::{self, bold, mono, regular};
use super::{capitalize, date_longue, jour_court, mois};
use crate::cache::Store;
use crate::model::now;
use crate::stats::{self, Report};
use crate::study::{Chrono, Session, fmt_precise, fmt_short};
use chrono::{Datelike, Duration, NaiveDate};
use eframe::egui::{self, Align2, Color32, CornerRadius, Margin, Rect, RichText, Sense, Stroke, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    Day,
    #[default]
    Week,
}

#[derive(Default)]
pub struct StatsUi {
    pub mode: Mode,
    /// Jour affiché (mode Jour) ou jour de la semaine affichée (mode Semaine). `None` = aujourd'hui.
    pub day: Option<NaiveDate>,
}

/// Couleurs du lieu : pendant un cours, chez soi, ajouté à la main.
const CLASS: Color32 = theme::VELVET_PURPLE;
const HOME: Color32 = theme::OK;
const MANUAL: Color32 = theme::WARN;

const PALETTE: [Color32; 10] = [
    theme::ACCENT,
    theme::VELVET_PINK,
    theme::VELVET_ORANGE,
    theme::VELVET_PURPLE,
    theme::VELVET_GREEN,
    theme::TD,
    theme::CM,
    theme::EXAM,
    theme::VELVET_BLUE,
    theme::VELVET_RED,
];

/// Couleur stable d'une matière (même couleur dans tous les graphiques).
fn subject_color(subjects: &[String], s: &str) -> Color32 {
    let i =
        subjects.iter().position(|x| x == s).unwrap_or_else(|| s.bytes().fold(7usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize)));
    PALETTE[i % PALETTE.len()]
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(title).font(bold(11.5)).color(theme::DIM));
    ui.add_space(2.0);
}

pub fn show(ctx: &egui::Context, ch: &Chrono, store: &Store, st: &mut StatsUi, open: &mut bool) {
    let win = egui::Window::new(RichText::new("Statistique").font(bold(18.0)))
        .collapsible(false)
        .default_size([900.0_f32.min(ctx.content_rect().width() - 24.0), 680.0_f32.min(ctx.content_rect().height() - 48.0)]);
    super::page::show(ctx, egui::Id::new("stats"), "Statistique", win, Some(open), false, |ui| {
        let max_h = if super::page::is_page(ui.ctx()) { ui.available_height() } else { 620.0 };
        egui::ScrollArea::vertical().id_salt("stats_scroll").auto_shrink([false, false]).max_height(max_h).show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui, ch, store, st);
        });
    });
}

fn body(ui: &mut egui::Ui, ch: &Chrono, store: &Store, st: &mut StatsUi) {
    let today = now().date();
    let day = st.day.unwrap_or(today).min(today);
    let page = super::page::is_page(ui.ctx());
    let bh = if page { 44.0 } else { 28.0 };

    // ---- sélecteurs : Jour | Semaine (+ Aujourd'hui), puis ◀ période ▶
    let step = if st.mode == Mode::Day { 1 } else { 7 };
    let is_now = match st.mode {
        Mode::Day => day == today,
        Mode::Week => stats::week_start(day) == stats::week_start(today),
    };
    let label = match st.mode {
        Mode::Day => capitalize(&date_longue(day)),
        Mode::Week => week_label(stats::week_start(day)),
    };
    let nav = |ui: &mut egui::Ui, st: &mut StatsUi| {
        if ui.add(egui::Button::new(RichText::new("◀").font(mono(14.0))).min_size(vec2(bh, bh))).clicked() {
            st.day = Some(day - Duration::days(step));
        }
        ui.label(RichText::new(&label).font(bold(15.0)));
        if ui.add_enabled(!is_now, egui::Button::new(RichText::new("▶").font(mono(14.0))).min_size(vec2(bh, bh))).clicked() {
            st.day = Some((day + Duration::days(step)).min(today));
        }
    };
    ui.horizontal(|ui| {
        for (m, l) in [(Mode::Day, "Jour"), (Mode::Week, "Semaine")] {
            let b = egui::Button::new(RichText::new(l).font(bold(14.0)))
                .selected(st.mode == m)
                .min_size(vec2(if page { 92.0 } else { 80.0 }, bh));
            if ui.add(b).clicked() {
                st.mode = m;
            }
        }
        if !page {
            ui.add_space(8.0);
            nav(ui, st);
        }
        if !is_now && ui.add(egui::Button::new("Aujourd'hui").min_size(vec2(0.0, bh))).clicked() {
            st.day = None;
        }
    });
    if page {
        ui.horizontal(|ui| nav(ui, st));
    }

    let sessions = ch.sessions_with_live();
    let subjects = ch.known_subjects();
    let has_tt = |d: NaiveDate| store.week_age_min(d).is_some();
    match st.mode {
        Mode::Day => {
            let r = stats::report(&sessions, &store.cours, day, day, has_tt);
            day_view(ui, &r, &sessions, store, &subjects, day);
        }
        Mode::Week => {
            let mon = stats::week_start(day);
            let r = stats::report(&sessions, &store.cours, mon, mon + Duration::days(6), has_tt);
            let prev = stats::report(&sessions, &store.cours, mon - Duration::days(7), mon - Duration::days(1), has_tt);
            if let Some(d) = week_view(ui, &r, &prev, &sessions, &subjects, today) {
                st.mode = Mode::Day;
                st.day = Some(d);
            }
        }
    }
}

fn week_label(mon: NaiveDate) -> String {
    let sun = mon + Duration::days(6);
    let iso = mon.iso_week().week();
    if mon.month() == sun.month() {
        format!("{} – {} {} · S{iso}", mon.day(), sun.day(), mois(sun))
    } else {
        format!("{} {} – {} {} · S{iso}", mon.day(), mois(mon), sun.day(), mois(sun))
    }
}

// ---------------------------------------------------------------- tuiles

fn tiles(ui: &mut egui::Ui, items: &[(&str, String, Color32)]) {
    let page = super::page::is_page(ui.ctx());
    let per_row = if ui.available_width() < 520.0 {
        2
    } else if page {
        3
    } else {
        4
    }
    .min(items.len())
    .max(1);
    let gap = ui.spacing().item_spacing.x;
    let w = ((ui.available_width() - gap * (per_row as f32 - 1.0)) / per_row as f32 - 26.0).max(60.0);
    for row in items.chunks(per_row) {
        ui.horizontal(|ui| {
            for (title, value, col) in row {
                theme::card(ui, Margin::symmetric(12, 8), |ui| {
                    ui.set_width(w);
                    ui.vertical(|ui| {
                        ui.set_width(w);
                        ui.add(egui::Label::new(RichText::new(*title).font(regular(12.0)).color(theme::DIM)).truncate());
                        ui.add(egui::Label::new(RichText::new(value).font(theme::mono_bold(19.0)).color(*col)).truncate());
                    });
                });
            }
        });
    }
}

fn no_timetable_note(ui: &mut egui::Ui, r: &Report) {
    if r.no_timetable.is_empty() {
        return;
    }
    let days: Vec<String> = r.no_timetable.iter().map(|d| format!("{} {}", jour_court(*d).to_lowercase(), d.day())).collect();
    ui.add_space(6.0);
    ui.add(
        egui::Label::new(
            RichText::new(format!(
                "Emploi du temps jamais téléchargé pour : {}. Le temps de ces jours est compté « chez soi ».",
                days.join(", ")
            ))
            .font(regular(12.5))
            .color(theme::WARN),
        )
        .wrap(),
    );
}

// ---------------------------------------------------------------- lieu

/// Barre empilée en cours / chez soi / ajouté, avec légende et pourcentages.
fn place_bar(ui: &mut egui::Ui, class: i64, home: i64, manual: i64) {
    let total = (class + home + manual).max(1) as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(640.0), 16.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(4), theme::CARD);
    let mut x = rect.left();
    for (v, c) in [(class, CLASS), (home, HOME), (manual, MANUAL)] {
        if v > 0 {
            let w = rect.width() * v as f32 / total;
            p.rect_filled(Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height())), CornerRadius::same(3), c);
            x += w;
        }
    }
    ui.horizontal_wrapped(|ui| {
        for (l, v, c) in [("En cours", class, CLASS), ("Chez soi", home, HOME), ("Ajouté", manual, MANUAL)] {
            if v == 0 && l == "Ajouté" {
                continue;
            }
            legend_dot(ui, c);
            ui.label(
                RichText::new(format!("{l} {} · {:.0} %", fmt_short(v), v as f32 * 100.0 / total)).font(regular(12.5)).color(theme::MUTED),
            );
            ui.add_space(6.0);
        }
    });
}

fn legend_dot(ui: &mut egui::Ui, c: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(2), c);
}

// ---------------------------------------------------------------- barres horizontales

fn hbars(ui: &mut egui::Ui, rows: &[(String, i64, Color32)]) {
    let top = rows.iter().map(|r| r.1).max().unwrap_or(1).max(1) as f32;
    let total: i64 = rows.iter().map(|r| r.1).sum::<i64>().max(1);
    let label_w = (ui.available_width() * 0.32).clamp(90.0, 170.0);
    for (s, t, c) in rows {
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(vec2(label_w, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_width(label_w);
                ui.add(egui::Label::new(RichText::new(s).font(regular(13.0))).truncate());
            });
            let w = (ui.available_width() - 120.0).clamp(40.0, 320.0);
            let (r, _) = ui.allocate_exact_size(vec2(w, 12.0), Sense::hover());
            ui.painter().rect_filled(r, CornerRadius::same(4), theme::CARD);
            let fill = Rect::from_min_size(r.min, vec2((r.width() * *t as f32 / top).max(4.0), r.height()));
            ui.painter().rect_filled(fill, CornerRadius::same(4), *c);
            ui.label(
                RichText::new(format!("{} · {:.0} %", fmt_short(*t), *t as f32 * 100.0 / total as f32))
                    .font(regular(12.5))
                    .color(theme::MUTED),
            );
        });
    }
}

fn by_subject_and_part(ui: &mut egui::Ui, r: &Report, subjects: &[String]) {
    if !r.by_subject.is_empty() {
        section(ui, "PAR MATIÈRE");
        let rows: Vec<_> = r.by_subject.iter().map(|(s, t)| (s.clone(), *t, subject_color(subjects, s))).collect();
        hbars(ui, &rows);
    }
    if !r.by_part.is_empty() {
        section(ui, "PAR PARTIE");
        let rows: Vec<_> = r.by_part.iter().map(|(s, t)| (s.clone(), *t, theme::ACCENT.gamma_multiply(0.8))).collect();
        hbars(ui, &rows);
    }
}

// ---------------------------------------------------------------- jour

fn added_removed(r: &Report) -> String {
    match (r.manual, r.removed) {
        (0, 0) => "—".into(),
        (m, 0) => format!("+{}", fmt_short(m)),
        (0, x) => format!("−{}", fmt_short(x)),
        (m, x) => format!("+{} / −{}", fmt_short(m), fmt_short(x)),
    }
}

fn day_view(ui: &mut egui::Ui, r: &Report, sessions: &[Session], store: &Store, subjects: &[String], day: NaiveDate) {
    let blocks = stats::blocks(sessions, &store.cours, day);
    ui.add_space(10.0);
    tiles(
        ui,
        &[
            ("TOTAL", fmt_short(r.total), theme::TEXT),
            ("EN COURS", fmt_short(r.class), CLASS),
            ("CHEZ SOI", fmt_short(r.home), HOME),
            ("SESSIONS", r.sessions.to_string(), theme::TEXT),
            ("PLUS LONGUE", fmt_short(r.longest), theme::TEXT),
            ("AJOUTÉ / RETIRÉ", added_removed(r), theme::MUTED),
        ],
    );
    no_timetable_note(ui, r);
    if r.total == 0 && blocks.is_empty() {
        ui.add_space(20.0);
        ui.label(RichText::new("Aucune révision ce jour-là.").font(regular(15.0)).color(theme::MUTED));
        return;
    }

    section(ui, "OÙ");
    place_bar(ui, r.class, r.home, r.manual);

    section(ui, "FRISE DE LA JOURNÉE");
    timeline(ui, &blocks, store, subjects, day);

    by_subject_and_part(ui, r, subjects);

    if !blocks.is_empty() {
        section(ui, "SESSIONS CHRONOMÉTRÉES");
        for b in &blocks {
            ui.horizontal_wrapped(|ui| {
                legend_dot(ui, subject_color(subjects, &b.subject));
                ui.label(
                    RichText::new(format!("{}–{}", b.start.format("%H:%M"), b.end.format("%H:%M"))).font(mono(12.5)).color(theme::DIM),
                );
                ui.label(RichText::new(format!("{} · {}", b.subject, b.part)).font(regular(13.5)));
                let secs = (b.end - b.start).num_seconds();
                ui.label(RichText::new(fmt_precise(secs)).font(mono(12.5)).color(theme::OK));
                if b.class > 0 {
                    let txt = if b.class >= secs { "en cours".to_string() } else { format!("dont {} en cours", fmt_short(b.class)) };
                    ui.label(RichText::new(txt).font(regular(12.0)).color(CLASS));
                }
            });
        }
    }
}

/// Frise horaire d'un jour : créneaux de cours hachurés en fond, sessions colorées par matière.
fn timeline(ui: &mut egui::Ui, blocks: &[stats::Block], store: &Store, subjects: &[String], day: NaiveDate) {
    let slots = stats::course_slots(&store.cours, day);
    let hour = |t: chrono::NaiveDateTime| (t - day.and_hms_opt(0, 0, 0).unwrap()).num_seconds() as f32 / 3600.0;
    let lo = blocks.iter().map(|b| hour(b.start)).chain(slots.iter().map(|s| hour(s.0))).fold(8.0_f32, f32::min).floor();
    let hi = blocks.iter().map(|b| hour(b.end)).chain(slots.iter().map(|s| hour(s.1))).fold(19.0_f32, f32::max).ceil().min(24.0);
    let w = ui.available_width().min(760.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 64.0), Sense::hover());
    // marges latérales : les heures des bords restent lisibles
    let band = Rect::from_min_max(pos2(rect.left() + 12.0, rect.top() + 4.0), pos2(rect.right() - 12.0, rect.bottom() - 18.0));
    let x = |h: f32| band.left() + (h - lo) / (hi - lo) * band.width();
    let p = ui.painter_at(rect);
    p.rect_filled(band, CornerRadius::same(4), theme::CARD);
    for (a, b) in &slots {
        let r = Rect::from_min_max(pos2(x(hour(*a)), band.top()), pos2(x(hour(*b)), band.bottom()));
        p.rect_filled(r, CornerRadius::ZERO, CLASS.gamma_multiply(0.12));
        theme::hatch(&p, r, 6.0, Stroke::new(1.0, CLASS.gamma_multiply(0.35)));
    }
    let inner = band.shrink2(vec2(0.0, 8.0));
    for b in blocks {
        let r = Rect::from_min_max(pos2(x(hour(b.start)), inner.top()), pos2(x(hour(b.end)).max(x(hour(b.start)) + 2.0), inner.bottom()));
        p.rect_filled(r, CornerRadius::same(3), subject_color(subjects, &b.subject));
    }
    let step = if hi - lo > 12.0 { 2 } else { 1 };
    for h in (lo as i32..=hi as i32).step_by(step) {
        let xx = x(h as f32);
        p.line_segment([pos2(xx, band.bottom()), pos2(xx, band.bottom() + 3.0)], Stroke::new(1.0, theme::LINE));
        p.text(pos2(xx, rect.bottom()), Align2::CENTER_BOTTOM, format!("{h}h"), mono(10.0), theme::DIM);
    }
    // détail au survol / à l'appui
    if let Some(pos) = resp.hover_pos() {
        let h = lo + (pos.x - band.left()) / band.width() * (hi - lo);
        if let Some(b) = blocks.iter().find(|b| hour(b.start) <= h && h <= hour(b.end)) {
            resp.on_hover_text(format!(
                "{} · {}\n{}–{} · {}",
                b.subject,
                b.part,
                b.start.format("%H:%M"),
                b.end.format("%H:%M"),
                fmt_precise((b.end - b.start).num_seconds())
            ));
        }
    }
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(14.0, 10.0), Sense::hover());
        ui.painter().rect_filled(r, CornerRadius::ZERO, CLASS.gamma_multiply(0.25));
        ui.label(RichText::new("créneaux de cours").font(regular(12.0)).color(theme::DIM));
    });
}

// ---------------------------------------------------------------- semaine

/// Renvoie le jour touché dans le graphique (pour l'ouvrir en mode Jour).
fn week_view(
    ui: &mut egui::Ui,
    r: &Report,
    prev: &Report,
    sessions: &[Session],
    subjects: &[String],
    today: NaiveDate,
) -> Option<NaiveDate> {
    let elapsed = r.days.iter().filter(|d| d.date <= today).count().max(1) as i64;
    let best = r.days.iter().max_by_key(|d| d.total).filter(|d| d.total > 0);
    let diff = if prev.total > 0 {
        let pct = (r.total - prev.total) as f32 * 100.0 / prev.total as f32;
        (format!("{:+.0} %", pct), if pct >= 0.0 { theme::OK } else { theme::NOW })
    } else if r.total > 0 {
        ("nouveau".to_string(), theme::OK)
    } else {
        ("—".to_string(), theme::MUTED)
    };
    let end = r.to.min(today);
    let serie = stats::streak(sessions, end);
    ui.add_space(10.0);
    tiles(
        ui,
        &[
            ("TOTAL", fmt_short(r.total), theme::TEXT),
            ("MOYENNE / JOUR", fmt_short(r.total / elapsed), theme::TEXT),
            (
                "MEILLEUR JOUR",
                best.map_or("—".into(), |d| format!("{} {}", jour_court(d.date).to_lowercase(), fmt_short(d.total))),
                theme::TEXT,
            ),
            ("VS SEMAINE PRÉC.", diff.0, diff.1),
            ("EN COURS", fmt_short(r.class), CLASS),
            ("CHEZ SOI", fmt_short(r.home), HOME),
            ("SESSIONS", r.sessions.to_string(), theme::TEXT),
            ("SÉRIE", format!("{serie} j"), if serie > 0 { theme::WARN } else { theme::MUTED }),
        ],
    );
    no_timetable_note(ui, r);
    if r.total == 0 && r.sessions == 0 {
        ui.add_space(20.0);
        ui.label(RichText::new("Aucune révision cette semaine.").font(regular(15.0)).color(theme::MUTED));
        return None;
    }

    section(ui, "OÙ");
    place_bar(ui, r.class, r.home, r.manual);

    section(ui, "PAR JOUR · MATIÈRES  (touche un jour pour le détail)");
    let cols: Vec<Column> = r
        .days
        .iter()
        .map(|d| Column {
            date: d.date,
            parts: d.by_subject.iter().map(|(s, t)| (*t, subject_color(subjects, s), s.clone())).collect(),
            total: d.total,
        })
        .collect();
    let avg = if elapsed > 0 { r.total / elapsed } else { 0 };
    let mut picked = columns(ui, "stats_subj", &cols, today, Some(avg));

    section(ui, "PAR JOUR · EN COURS / CHEZ SOI");
    let cols: Vec<Column> = r
        .days
        .iter()
        .map(|d| Column {
            date: d.date,
            parts: vec![(d.class, CLASS, "En cours".into()), (d.home, HOME, "Chez soi".into()), (d.manual, MANUAL, "Ajouté".into())],
            total: d.class + d.home + d.manual,
        })
        .collect();
    picked = picked.or(columns(ui, "stats_place", &cols, today, None));

    section(ui, "HEURES DE TRAVAIL  (chronométré)");
    heatmap(ui, r, today);

    by_subject_and_part(ui, r, subjects);
    picked
}

struct Column {
    date: NaiveDate,
    /// (secondes, couleur, libellé) empilés de bas en haut.
    parts: Vec<(i64, Color32, String)>,
    total: i64,
}

/// Colonnes empilées, une par jour. Renvoie le jour touché.
fn columns(ui: &mut egui::Ui, id: &str, cols: &[Column], today: NaiveDate, avg: Option<i64>) -> Option<NaiveDate> {
    let max = cols.iter().map(|c| c.parts.iter().map(|p| p.0.max(0)).sum::<i64>()).max().unwrap_or(0).max(3600) as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(640.0), 150.0), Sense::hover());
    let p = ui.painter_at(rect);
    let slot = rect.width() / cols.len().max(1) as f32;
    let chart_top = rect.top() + 16.0;
    let chart_h = rect.height() - 38.0;
    let y = |v: f32| chart_top + chart_h - v / max * chart_h;
    let mut picked = None;
    for (i, c) in cols.iter().enumerate() {
        let x = rect.left() + slot * i as f32 + slot / 2.0;
        let bw = slot * 0.56;
        let col_rect = Rect::from_min_max(pos2(x - slot / 2.0, rect.top()), pos2(x + slot / 2.0, rect.bottom()));
        let resp = ui.interact(col_rect, ui.id().with((id, i)), Sense::click());
        p.rect_filled(Rect::from_min_max(pos2(x - bw / 2.0, chart_top), pos2(x + bw / 2.0, y(0.0))), CornerRadius::same(4), theme::CARD);
        let mut acc = 0.0;
        for (v, col, _) in &c.parts {
            if *v <= 0 {
                continue;
            }
            let (a, b) = (y(acc), y(acc + *v as f32));
            p.rect_filled(Rect::from_min_max(pos2(x - bw / 2.0, b), pos2(x + bw / 2.0, a)), CornerRadius::same(2), *col);
            acc += *v as f32;
        }
        let future = c.date > today;
        if c.total > 0 {
            p.text(pos2(x, y(acc) - 3.0), Align2::CENTER_BOTTOM, fmt_short(c.total), mono(10.0), theme::MUTED);
        }
        let lc = if c.date == today {
            theme::ACCENT
        } else if future {
            theme::LINE
        } else {
            theme::DIM
        };
        p.text(pos2(x, rect.bottom() - 2.0), Align2::CENTER_BOTTOM, format!("{} {}", jour_court(c.date), c.date.day()), mono(10.0), lc);
        if resp.hovered() {
            p.rect_stroke(col_rect.shrink(1.0), CornerRadius::same(4), Stroke::new(1.0, theme::LINE), egui::StrokeKind::Inside);
        }
        let detail: Vec<String> = c.parts.iter().filter(|p| p.0 > 0).map(|(v, _, l)| format!("{l} : {}", fmt_precise(*v))).collect();
        let tip = if detail.is_empty() { "Rien".to_string() } else { detail.join("\n") };
        if resp.on_hover_text(format!("{}\n{tip}", capitalize(&date_longue(c.date)))).clicked() && !future {
            picked = Some(c.date);
        }
    }
    if let Some(a) = avg.filter(|a| *a > 0) {
        let yy = y(a as f32);
        let s = Stroke::new(1.0, theme::WARN.gamma_multiply(0.7));
        let mut xx = rect.left();
        while xx < rect.right() {
            p.line_segment([pos2(xx, yy), pos2((xx + 5.0).min(rect.right()), yy)], s);
            xx += 9.0;
        }
        p.text(pos2(rect.right(), yy - 2.0), Align2::RIGHT_BOTTOM, format!("moy. {}", fmt_short(a)), mono(9.5), theme::WARN);
    }
    picked
}

/// Carte de chaleur heure × jour (temps chronométré).
fn heatmap(ui: &mut egui::Ui, r: &Report, today: NaiveDate) {
    let used: Vec<usize> = (0..24).filter(|h| r.hours[*h] > 0).collect();
    let lo = used.first().copied().unwrap_or(8).min(8);
    let hi = used.last().map_or(19, |h| h + 1).max(19);
    let ncols = hi - lo;
    let max = r.days.iter().flat_map(|d| d.hours[lo..hi].iter().copied()).max().unwrap_or(0).max(1) as f32;
    let label_w = 44.0;
    let w = ui.available_width().min(680.0);
    let cell_w = (w - label_w) / ncols as f32;
    let cell_h = 16.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, cell_h * 7.0 + 16.0), Sense::hover());
    let p = ui.painter_at(rect);
    for (row, d) in r.days.iter().enumerate() {
        let yy = rect.top() + row as f32 * cell_h;
        let lc = if d.date == today { theme::ACCENT } else { theme::DIM };
        p.text(pos2(rect.left(), yy + cell_h / 2.0), Align2::LEFT_CENTER, jour_court(d.date), mono(10.0), lc);
        for (k, h) in (lo..hi).enumerate() {
            let c = Rect::from_min_size(pos2(rect.left() + label_w + k as f32 * cell_w, yy), vec2(cell_w, cell_h)).shrink(1.0);
            let v = d.hours[h] as f32 / max;
            let fill = if d.hours[h] > 0 { theme::ACCENT.gamma_multiply(0.18 + 0.82 * v) } else { theme::CARD };
            p.rect_filled(c, CornerRadius::same(2), fill);
        }
    }
    let step = if cell_w < 22.0 { 2 } else { 1 };
    for (k, h) in (lo..hi).enumerate().step_by(step) {
        p.text(
            pos2(rect.left() + label_w + k as f32 * cell_w + 1.0, rect.bottom()),
            Align2::LEFT_BOTTOM,
            format!("{h}h"),
            mono(9.5),
            theme::DIM,
        );
    }
    if let Some(pos) = resp.hover_pos() {
        let row = ((pos.y - rect.top()) / cell_h) as usize;
        let k = ((pos.x - rect.left() - label_w) / cell_w).floor();
        if row < 7 && k >= 0.0 && (k as usize) < ncols {
            let (d, h) = (&r.days[row], lo + k as usize);
            resp.on_hover_text(format!("{} · {h}h–{}h\n{}", capitalize(&date_longue(d.date)), h + 1, fmt_precise(d.hours[h])));
        }
    }
}
