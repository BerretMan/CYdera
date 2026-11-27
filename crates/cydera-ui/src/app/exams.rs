//! Fenêtre « Examens » et examens à venir.
use super::*;

impl App {
    /// Examens (catégorie Celcat « Examens ») pas encore terminés, triés.
    pub(super) fn upcoming_exams(&self) -> Vec<&Cours> {
        let now = now();
        self.store.cours.iter().filter(|c| c.kind == Kind::Exam && c.end > now).collect()
    }

    /// Une entrée par matière : son prochain examen.
    pub(super) fn exam_refs(&self) -> Vec<ExamRef> {
        let mut v: Vec<ExamRef> = Vec::new();
        for c in self.upcoming_exams() {
            if !v.iter().any(|e| e.subject == c.matiere) {
                v.push(ExamRef { subject: c.matiere.clone(), date: c.date() });
            }
        }
        v
    }

    pub(super) fn exams_window(&mut self, ctx: &egui::Context) {
        if !self.show_exams {
            return;
        }
        let mut open = true;
        let mut picked = None;
        let mut revise: Option<String> = None;
        let today = now().date();
        let win_w = 460.0_f32.min(ctx.content_rect().width() - 24.0).max(280.0);
        let page = ui::page::is_page(ctx);
        let win = egui::Window::new(RichText::new("Examens à venir").font(bold(18.0)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, [-24.0, 70.0])
            .default_width(win_w);
        ui::page::show(ctx, egui::Id::new("exams"), "Examens à venir", win, Some(&mut open), false, |ui| {
            // Largeur explicite : sans elle, le texte vertical d'une ligne (layout droite→gauche)
            // recevait une largeur nulle et passait à la ligne lettre par lettre, en élargissant la fenêtre.
            ui.set_width(if page { ui.available_width() } else { win_w - 32.0 });
            let exams = self.upcoming_exams();
            if exams.is_empty() {
                ui.label(RichText::new("Aucun examen prévu dans l'emploi du temps synchronisé.").color(theme::MUTED));
                return;
            }
            let max_h = if page { ui.available_height() } else { 520.0 };
            egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
                // largeur figée une fois : sinon chaque carte gagnait quelques px sur la précédente
                let row_w = ui.available_width();
                ui.set_width(row_w);
                for c in exams {
                    let days = (c.date() - today).num_days();
                    let col = match days {
                        d if d <= 7 => theme::NOW,
                        d if d <= 21 => theme::EXAM,
                        _ => theme::ACCENT,
                    };
                    let mut rev_btn = egui::Rect::NOTHING;
                    let resp = theme::card(ui, Margin::symmetric(12, 10), |ui| {
                        let inner_w = row_w - 26.0; // marges 2×12 + bordures
                        ui.set_width(inner_w);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 12.0;
                            let badge = if days == 0 { "AUJ.".to_string() } else { format!("J-{days}") };
                            ui.add_sized([62.0, 40.0], egui::Label::new(RichText::new(badge).font(theme::mono_bold(20.0)).color(col)));
                            let btn_w = 92.0;
                            let text_w = (inner_w - 62.0 - btn_w - 24.0).max(80.0);
                            ui.vertical(|ui| {
                                ui.set_width(text_w);
                                ui.add(egui::Label::new(RichText::new(c.matiere.to_uppercase()).font(bold(14.0))).truncate());
                                let meta = format!(
                                    "{} {:02} {} · {}–{} · {}",
                                    ui::jour_court(c.date()),
                                    c.date().day(),
                                    ui::mois(c.date()).chars().take(3).collect::<String>().to_uppercase(),
                                    c.start.format("%H:%M"),
                                    c.end.format("%H:%M"),
                                    c.salle_courte
                                );
                                ui.add(egui::Label::new(RichText::new(meta).font(theme::mono(11.0)).color(theme::MUTED)).truncate());
                            });
                            let b = ui.add_sized([btn_w, if page { 44.0 } else { 28.0 }], egui::Button::new("⏱ RÉVISER"));
                            rev_btn = b.rect;
                        });
                    })
                    .response;
                    let click = ui
                        .interact(resp.rect, ui.id().with(("exam", c.start, &c.matiere)), egui::Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if click.clicked() {
                        if click.interact_pointer_pos().is_some_and(|p| rev_btn.contains(p)) {
                            revise = Some(c.matiere.clone());
                        } else {
                            picked = Some(c.clone());
                        }
                    }
                    ui.add_space(4.0);
                }
            });
        });
        if let Some(c) = picked {
            self.selected = c.date();
            self.detail = Some(c);
        }
        if let Some(subject) = revise {
            let part = self.chrono.part.clone();
            self.chrono.select(&subject, &part, true);
            self.show_rev = true;
        }
        if !open {
            self.show_exams = false;
        }
    }
}
