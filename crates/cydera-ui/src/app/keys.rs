//! Raccourcis clavier (PC) et bouton retour (Android).
use super::*;

impl App {
    pub(super) fn handle_keys(&mut self, ctx: &egui::Context) -> bool {
        let mut force = false;
        let mut quit = false;
        let mut toggle_mail = false;
        let mut toggle_resto = false;
        let mut back = false;
        let typing = ctx.egui_wants_keyboard_input();
        // Une fenêtre ouverte (mails, examens, notes…) garde le clavier : l'agenda ne bouge plus derrière.
        let overlay = self.mail.show
            || self.show_exams
            || self.show_rev
            || self.show_grades
            || self.show_about
            || self.show_resto
            || self.show_devices;
        let mut mail_nav = 0i32;
        let mut mail_del = false;
        ctx.input(|i| {
            if i.modifiers.ctrl && i.key_pressed(Key::Q) {
                quit = true;
                return;
            }
            if typing || (i.modifiers.any() && !i.modifiers.shift_only()) {
                return;
            }
            if self.mail.show {
                if i.key_pressed(Key::Delete) {
                    mail_del = true;
                }
                if i.key_pressed(Key::ArrowUp) || i.key_pressed(Key::K) {
                    mail_nav = -1;
                }
                if i.key_pressed(Key::ArrowDown) {
                    mail_nav = 1;
                }
            }
            if !overlay {
                if i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::H) {
                    self.step(-1);
                }
                if i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::L) {
                    self.step(1);
                }
                if i.key_pressed(Key::ArrowUp) {
                    self.selected -= Duration::days(7);
                }
                if i.key_pressed(Key::ArrowDown) {
                    self.selected += Duration::days(7);
                }
                if i.key_pressed(Key::T) || i.key_pressed(Key::Home) {
                    self.selected = now().date();
                }
            }
            if i.key_pressed(Key::S) || i.key_pressed(Key::W) {
                self.view = View::Week;
            }
            if i.key_pressed(Key::J) || i.key_pressed(Key::D) {
                self.view = View::Day;
            }
            if i.key_pressed(Key::Tab) {
                self.view = if self.view == View::Week { View::Day } else { View::Week };
            }
            if i.key_pressed(Key::P) {
                self.show_side = !self.show_side;
            }
            if i.key_pressed(Key::R) || i.key_pressed(Key::F5) {
                force = true;
            }
            if i.key_pressed(Key::M) {
                toggle_mail = true;
            }
            if i.key_pressed(Key::U) {
                toggle_resto = true;
            }
            if i.key_pressed(Key::C) {
                self.show_rev = !self.show_rev;
            }
            if i.key_pressed(Key::Space) && self.chrono.subject.is_some() {
                self.chrono.toggle();
            }
            if i.key_pressed(Key::E) {
                self.show_exams = !self.show_exams;
            }
            if i.key_pressed(Key::N) {
                self.show_grades = !self.show_grades;
            }
            if i.key_pressed(Key::BrowserBack) {
                back = true;
            }
            if i.key_pressed(Key::Escape) {
                if self.detail.is_some() {
                    self.detail = None;
                } else {
                    self.show_exams = false;
                    self.show_rev = false;
                    self.show_about = false;
                    self.show_resto = false;
                    if self.mail.show && !self.mail.close_message() {
                        self.mail.show = false;
                    }
                    if !self.grades_ui.back() {
                        self.show_grades = false;
                    }
                }
            }
        });
        // Bouton « retour » d'Android : ferme la feuille ou la page du dessus, sinon revient à aujourd'hui.
        if back {
            if self.show_more {
                self.show_more = false;
            } else if !self.close_top_page() {
                self.selected = now().date();
            }
            ctx.request_repaint();
        }
        if mail_nav != 0 {
            self.mail.step(ctx, mail_nav);
        }
        if mail_del {
            self.mail.trash_selected(ctx);
        }
        if toggle_mail {
            self.mail.toggle(ctx);
        }
        if toggle_resto {
            self.toggle_resto(ctx);
        }
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        force
    }
}
