//! Fenêtre « Révisions » : chrono par matière, basé sur les examens de l'emploi du temps.

use super::theme::{self, bold, regular};
use super::{capitalize, date_longue};
use crate::model::now;
use crate::study::{Chrono, PARTS, SessionKind, fmt_hms, fmt_precise, fmt_short};
use chrono::NaiveDate;
use eframe::egui::{self, Align, Color32, Layout, Margin, RichText, Sense, Stroke, vec2};

/// Un examen à venir (une entrée par matière, le plus proche).
#[derive(Clone)]
pub struct ExamRef {
    pub subject: String,
    pub date: NaiveDate,
}

#[derive(Default)]
pub struct RevUi {
    /// 0 = aujourd'hui, 1 = hier, 2 = avant-hier (jour visé par le sélecteur de temps).
    pub days_ago: i64,
    /// Minutes choisies au sélecteur (> 0 ajouter, < 0 retirer), appliquées par « Appliquer ».
    pub adjust: i64,
    pub new_subject: String,
    pub show_journal: bool,
    /// Matière dont la suppression attend confirmation.
    pub confirm_del: Option<String>,
}

fn jours_avant(d: NaiveDate) -> i64 {
    (d - now().date()).num_days()
}

fn badge_color(days: i64) -> Color32 {
    match days {
        d if d <= 7 => theme::NOW,
        d if d <= 21 => theme::EXAM,
        _ => theme::ACCENT,
    }
}

pub fn show(ctx: &egui::Context, ch: &mut Chrono, ui_state: &mut RevUi, exams: &[ExamRef], open: &mut bool) {
    let win = egui::Window::new(RichText::new("Révisions").font(bold(18.0)))
        .collapsible(false)
        .default_size([940.0_f32.min(ctx.content_rect().width() - 24.0), 640.0_f32.min(ctx.content_rect().height() - 48.0)]);
    super::page::show(ctx, egui::Id::new("revisions"), "Révisions", win, Some(open), false, |ui| {
        if let Some(n) = ch.notice.clone() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(n).color(theme::WARN));
                if ui.button("OK").clicked() {
                    ch.notice = None;
                }
            });
        }
        // Écran étroit : les colonnes s'empilent dans une seule zone défilante.
        if ctx.content_rect().width() < 780.0 {
            // Chrono en tête : c'est ce qu'on vient chercher sur le téléphone.
            egui::ScrollArea::vertical().id_salt("rev_narrow").auto_shrink([false, false]).show(ui, |ui| {
                timer_panel(ui, ch, ui_state, exams);
                ui.add_space(14.0);
                subject_list(ui, ch, ui_state, exams, 230.0);
                ui.add_space(10.0);
                journal(ui, ch, ui_state);
            });
            return;
        }
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(310.0, 560.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(310.0);
                subject_list(ui, ch, ui_state, exams, 470.0);
            });
            ui.separator();
            ui.vertical(|ui| {
                egui::ScrollArea::vertical().id_salt("rev_right").auto_shrink([false, false]).max_height(560.0).show(ui, |ui| {
                    timer_panel(ui, ch, ui_state, exams);
                    ui.add_space(14.0);
                    journal(ui, ch, ui_state);
                });
            });
        });
    });
}

// ---------------------------------------------------------------- liste des matières

fn subject_list(ui: &mut egui::Ui, ch: &mut Chrono, st: &mut RevUi, exams: &[ExamRef], max_h: f32) {
    ui.label(RichText::new("MATIÈRES").font(bold(11.5)).color(theme::DIM));
    ui.add_space(2.0);
    let part = ch.part.clone();
    let mut action: Option<(String, u8)> = None; // (matière, 0 sélectionner · 1 lancer/arrêter · 2 supprimer)

    egui::ScrollArea::vertical().id_salt("rev_list").auto_shrink([false, false]).max_height(max_h).show(ui, |ui| {
        if !exams.is_empty() {
            ui.label(RichText::new("Examens à venir").color(theme::MUTED));
            for e in exams {
                if let Some(a) = row(ui, ch, &e.subject, Some(e.date), false) {
                    action = Some(a);
                }
            }
        }
        let others: Vec<String> = ch.known_subjects().into_iter().filter(|s| !exams.iter().any(|e| &e.subject == s)).collect();
        if !others.is_empty() {
            ui.add_space(8.0);
            ui.label(RichText::new("Autres matières").color(theme::MUTED));
            for s in others {
                if let Some(a) = row(ui, ch, &s, None, true) {
                    action = Some(a);
                }
            }
        }
        if exams.is_empty() && ch.known_subjects().is_empty() {
            ui.label(RichText::new("Aucun examen dans l'emploi du temps : ajoute une matière ci-dessous.").color(theme::MUTED));
        }
    });

    if let Some(name) = st.confirm_del.clone() {
        ui.add_space(6.0);
        theme::card(ui, Margin::symmetric(10, 8), |ui| {
            ui.set_width(ui.available_width());
            let n = ch.session_count(&name);
            let sessions = match n {
                0 => String::new(),
                1 => " et sa session".to_string(),
                n => format!(" et ses {n} sessions"),
            };
            ui.add(egui::Label::new(RichText::new(format!("Supprimer « {name} »{sessions} ?")).color(theme::WARN)).wrap());
            ui.horizontal(|ui| {
                if ui.button("Supprimer").clicked() {
                    ch.delete_subject(&name);
                    st.confirm_del = None;
                }
                if ui.button("Annuler").clicked() {
                    st.confirm_del = None;
                }
            });
        });
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let w = if super::page::is_page(ui.ctx()) { (ui.available_width() - 100.0).max(120.0) } else { 210.0 };
        let r = ui.add(egui::TextEdit::singleline(&mut st.new_subject).hint_text("Ajouter une matière…").desired_width(w));
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("Ajouter").clicked() || enter) && !st.new_subject.trim().is_empty() {
            ch.add_custom(&st.new_subject);
            st.new_subject.clear();
        }
    });

    if let Some((s, kind)) = action {
        match kind {
            2 => st.confirm_del = Some(s),
            1 if ch.is_running_on(&s) => ch.pause(),
            k => ch.select(&s, &part, k == 1),
        }
    }
}

/// Une ligne de matière. Clic = sélectionner, bouton ▶/⏸ = lancer/arrêter.
fn row(ui: &mut egui::Ui, ch: &Chrono, subject: &str, exam: Option<NaiveDate>, deletable: bool) -> Option<(String, u8)> {
    let selected = ch.subject.as_deref() == Some(subject);
    let running = ch.is_running_on(subject);
    let mut btn_rect = egui::Rect::NOTHING;
    let mut del_rect = egui::Rect::NOTHING;
    let fill = if running {
        theme::OK.gamma_multiply(0.14)
    } else if selected {
        theme::CARD_HOVER
    } else {
        theme::CARD
    };
    let stroke = if selected { Stroke::new(1.5, theme::ACCENT) } else { Stroke::new(1.0, theme::LINE) };

    let rect = theme::card_ex(ui, fill, stroke, 10, Margin::symmetric(10, 7), |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            if let Some(d) = exam {
                let days = jours_avant(d);
                let txt = if days <= 0 { "Auj.".to_string() } else { format!("J-{days}") };
                ui.add_sized([44.0, 30.0], egui::Label::new(RichText::new(txt).font(theme::mono_bold(14.0)).color(badge_color(days))));
            }
            ui.vertical(|ui| {
                ui.add(egui::Label::new(RichText::new(subject).font(bold(14.0))).truncate());
                ui.label(
                    RichText::new(format!("auj. {} · total {}", fmt_short(ch.today(subject)), fmt_short(ch.total(Some(subject)))))
                        .font(regular(12.0))
                        .color(theme::MUTED),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (icon, col) = if running { ("⏸", theme::OK) } else { ("▶", theme::ACCENT) };
                let sz = if super::page::is_page(ui.ctx()) { vec2(48.0, 40.0) } else { vec2(34.0, 30.0) };
                let b = ui.add(egui::Button::new(RichText::new(icon).color(col).font(bold(15.0))).min_size(sz));
                btn_rect = b.rect;
                if deletable {
                    let sz = if super::page::is_page(ui.ctx()) { vec2(40.0, 40.0) } else { vec2(26.0, 30.0) };
                    let d = ui.add(egui::Button::new(RichText::new("×").color(theme::MUTED).font(bold(13.0))).frame(false).min_size(sz));
                    del_rect = d.rect;
                }
            });
        });
    })
    .response
    .rect;

    let resp = ui.interact(rect, ui.id().with(("rev_row", subject)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    ui.add_space(3.0);
    if resp.clicked() {
        let pos = resp.interact_pointer_pos();
        let kind = if pos.is_some_and(|p| del_rect.contains(p)) { 2 } else { u8::from(pos.is_some_and(|p| btn_rect.contains(p))) };
        return Some((subject.to_string(), kind));
    }
    None
}

// ---------------------------------------------------------------- chrono

fn timer_panel(ui: &mut egui::Ui, ch: &mut Chrono, st: &mut RevUi, exams: &[ExamRef]) {
    let Some(subject) = ch.subject.clone() else {
        ui.add_space(30.0);
        let (ou, geste) = if ui.ctx().content_rect().width() < 780.0 { ("ci-dessous", "Touche") } else { ("à gauche", "Clique sur") };
        ui.add(
            egui::Label::new(
                RichText::new(format!("Choisis une matière {ou}, puis lance le chrono.")).font(regular(16.0)).color(theme::MUTED),
            )
            .wrap(),
        );
        ui.label(RichText::new(format!("{geste} ▶ pour démarrer directement.")).color(theme::DIM));
        return;
    };
    let running = ch.running();

    ui.horizontal(|ui| {
        ui.label(RichText::new(&subject).font(bold(22.0)));
        if let Some(e) = exams.iter().find(|e| e.subject == subject) {
            let d = jours_avant(e.date);
            let txt = if d <= 0 { "examen aujourd'hui".to_string() } else { format!("examen dans {d} j · {}", date_longue(e.date)) };
            ui.label(RichText::new(txt).color(badge_color(d)));
        }
    });
    ui.horizontal(|ui| {
        let mut part = ch.part.clone();
        for p in PARTS {
            ui.selectable_value(&mut part, p.to_string(), p);
        }
        if part != ch.part {
            ch.select(&subject, &part, false);
        }
    });

    ui.add_space(6.0);
    let col = if running { theme::OK } else { theme::TEXT };
    let size = (ui.available_width() / 5.3).clamp(30.0, 64.0);
    theme::tabular_label(ui, &fmt_hms(ch.run_secs()), theme::mono_bold(size), col);
    ui.label(
        RichText::new(
            if running { "Session en cours" } else { "Prêt" }.to_string()
                + &format!(" · {} aujourd'hui sur {}", fmt_short(ch.today(&subject)), subject),
        )
        .color(theme::MUTED),
    );
    ui.add_space(8.0);

    let (label, fill) = if running {
        ("⏸  Pause  (Espace)", theme::WARN.gamma_multiply(0.85))
    } else {
        ("▶  Démarrer  (Espace)", theme::OK.gamma_multiply(0.85))
    };
    if ui
        .add_sized(
            [ui.available_width().min(420.0), 50.0],
            egui::Button::new(RichText::new(label).font(bold(17.0)).color(Color32::BLACK)).fill(fill),
        )
        .clicked()
    {
        ch.toggle();
    }

    ui.add_space(10.0);
    time_picker(ui, ch, st);
}

/// Sélecteur de temps : −10 −5 −1 [valeur] +1 +5 +10, jour visé, puis « Appliquer ».
fn time_picker(ui: &mut egui::Ui, ch: &mut Chrono, st: &mut RevUi) {
    let page = super::page::is_page(ui.ctx());
    // 6 boutons + la valeur sur une ligne : sur téléphone, la largeur des boutons suit l'écran.
    let value_w = 78.0;
    let gap = ui.spacing().item_spacing.x;
    let bw = ((ui.available_width() - value_w - 6.0 * gap) / 6.0).clamp(34.0, 52.0);
    let bsz = if page { vec2(bw, 44.0) } else { vec2(38.0, 28.0) };
    ui.label(RichText::new("AJOUTER OU RETIRER DU TEMPS").font(bold(11.5)).color(theme::DIM));
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        for m in [-10, -5, -1] {
            let b = egui::Button::new(RichText::new(format!("−{}", -m)).font(theme::mono_bold(14.0)).color(theme::NOW)).min_size(bsz);
            if ui.add(b).clicked() {
                st.adjust = (st.adjust + m).clamp(-600, 600);
            }
        }
        let (txt, col) = match st.adjust {
            0 => ("0 min".to_string(), theme::MUTED),
            a if a > 0 => (format!("+{a} min"), theme::OK),
            a => (format!("−{} min", -a), theme::NOW),
        };
        ui.add_sized([value_w, bsz.y], egui::Label::new(RichText::new(txt).font(theme::mono_bold(16.0)).color(col)));
        for m in [1, 5, 10] {
            let b = egui::Button::new(RichText::new(format!("+{m}")).font(theme::mono_bold(14.0)).color(theme::OK)).min_size(bsz);
            if ui.add(b).clicked() {
                st.adjust = (st.adjust + m).clamp(-600, 600);
            }
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("rev_days")
            .selected_text(["aujourd'hui", "hier", "avant-hier"][st.days_ago as usize % 3])
            .width(if page { 140.0 } else { 110.0 })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut st.days_ago, 0, "aujourd'hui");
                ui.selectable_value(&mut st.days_ago, 1, "hier");
                ui.selectable_value(&mut st.days_ago, 2, "avant-hier");
            });
        let label = match st.adjust {
            a if a > 0 => "Ajouter",
            a if a < 0 => "Retirer",
            _ => "Appliquer",
        };
        let b = egui::Button::new(RichText::new(label).font(bold(14.0))).min_size(vec2(if page { 120.0 } else { 96.0 }, bsz.y));
        if ui.add_enabled(st.adjust != 0, b).clicked() {
            ch.adjust_minutes(st.adjust, st.days_ago);
            st.adjust = 0;
        }
        if st.adjust != 0
            && ui.add(egui::Button::new(RichText::new("×").color(theme::MUTED)).frame(false).min_size(vec2(28.0, bsz.y))).clicked()
        {
            st.adjust = 0;
        }
    });
}

// ---------------------------------------------------------------- journal

fn journal(ui: &mut egui::Ui, ch: &mut Chrono, st: &mut RevUi) {
    let label = if st.show_journal { "Masquer le journal des sessions" } else { "Afficher le journal des sessions" };
    if ui.selectable_label(st.show_journal, label).clicked() {
        st.show_journal = !st.show_journal;
    }
    if !st.show_journal {
        return;
    }
    if ch.undo.is_some() {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Session supprimée.").color(theme::MUTED));
            if ui.button("Annuler").clicked() {
                ch.undo_delete();
            }
        });
    }
    let mut del = None;
    let mut last_day = None;
    for s in ch.sessions().iter().rev().take(60) {
        if last_day != Some(s.day()) {
            last_day = Some(s.day());
            ui.add_space(6.0);
            ui.label(RichText::new(capitalize(&date_longue(s.day()))).font(bold(13.0)));
        }
        ui.horizontal(|ui| {
            let (txt, col) = match s.kind {
                SessionKind::Adjust => (format!("−{}", fmt_precise(-s.secs)), theme::NOW),
                SessionKind::Manual => (format!("+{} (ajouté)", fmt_precise(s.secs)), theme::WARN),
                SessionKind::Work => (fmt_precise(s.secs), theme::OK),
            };
            ui.add_sized([150.0, 18.0], egui::Label::new(RichText::new(txt).color(col)));
            ui.label(RichText::new(format!("{} · {}", s.subject, s.part)));
            ui.label(RichText::new(s.start.format("%H:%M").to_string()).font(regular(12.0)).color(theme::DIM));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("🗑").on_hover_text("Supprimer").clicked() {
                    del = Some(s.id.clone());
                }
            });
        });
    }
    if let Some(id) = del {
        ch.delete(&id);
    }
}
