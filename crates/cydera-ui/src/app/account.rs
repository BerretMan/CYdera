//! Compte (connexion ENT) et appareils associés (synchro pair à pair).
use super::*;

impl App {
    pub(super) fn device_name(&self) -> String {
        let n = self.cfg.device_name.trim();
        if n.is_empty() { crate::p2p::default_device_name() } else { n.to_string() }
    }

    /// Identifiants reçus du PC à l'appairage : enregistrés comme à la saisie manuelle.
    pub(super) fn apply_credentials(&mut self, c: Credentials) {
        self.cfg.username = c.username;
        self.cfg.federation_id = c.federation_id;
        self.cfg.mail_user = c.mail_user;
        self.cfg.save();
        if crate::config::save_password(&c.password).is_ok() {
            self.password = load_password();
            self.mail.set_account(&self.cfg.mail_host, &self.cfg.mail_user, load_mail_password());
            self.error = None;
            self.last_attempt = None;
        }
    }

    pub(super) fn my_credentials(&self) -> Option<Credentials> {
        Some(Credentials {
            username: self.cfg.username.clone(),
            federation_id: self.cfg.federation_id.clone(),
            password: self.password.clone()?,
            mail_user: self.cfg.mail_user.clone(),
        })
    }

    /// Appairage et synchro entre appareils : résultats, demandes entrantes, fenêtre « Appareils ».
    pub(super) fn devices_window(&mut self, ctx: &egui::Context) {
        if let Some(t) = crate::p2p::take_startup_ticket() {
            self.dev_ui.input = t;
            self.show_devices = true;
        }
        // Côté téléphone : issue de l'appairage lancé avec un ticket.
        if let Some(r) = self.p2p.take_pair_result() {
            self.dev_ui.phone_code = None;
            match r {
                Ok(out) => {
                    if let Some(c) = out.credentials {
                        self.apply_credentials(c);
                    }
                    self.dev_ui.msg = Some((format!("Associé à « {} ». Synchronisation…", out.peer_name), true));
                    self.dev_ui.input.clear();
                    self.p2p.sync_now();
                }
                Err(e) => self.dev_ui.msg = Some((e, false)),
            }
        }
        // Côté PC : demande entrante à confirmer.
        if self.pair_req.is_none()
            && let Some(r) = self.p2p.poll_pair_request()
        {
            self.dev_ui.send_creds = false;
            self.pair_req = Some(r);
            ctx.request_repaint(); // la page « Nouvel appareil » passe devant à l'image suivante
        }
        if let Some(req) = self.pair_req.take() {
            match devices::request_dialog(ctx, &req, &mut self.dev_ui.send_creds, self.password.is_some()) {
                Some(accept) => {
                    let name = req.name.clone();
                    let credentials = if accept && self.dev_ui.send_creds { self.my_credentials() } else { None };
                    req.answer(Decision { accept, credentials });
                    self.dev_ui.ticket = None; // le secret est brûlé dans tous les cas
                    self.dev_ui.msg =
                        Some(if accept { (format!("« {name} » est associé."), true) } else { ("Demande refusée.".to_string(), false) });
                    self.show_devices = true;
                    self.p2p.sync_now();
                }
                None => self.pair_req = Some(req),
            }
        }
        if self.dev_ui.ticket.as_ref().is_some_and(|(_, d)| *d <= Instant::now()) {
            self.dev_ui.ticket = None;
            self.p2p.cancel_pairing();
        }
        if !self.show_devices {
            return;
        }
        self.p2p.ensure_started(); // l'utilisateur s'intéresse aux appareils : on lance le moteur réseau
        let mut open = true;
        let (peers, status, busy) = (self.p2p.peers(), self.p2p.status(), self.p2p.pairing_busy());
        let mobile = breakpoint(ctx) == Bp::Mobile;
        let acts = devices::show(ctx, &mut open, &mut self.dev_ui, &peers, &status, busy, mobile);
        for a in acts {
            match a {
                DevAction::StartPairing => {
                    let name = self.device_name();
                    match self.p2p.start_pairing(&name) {
                        Some(t) => {
                            self.dev_ui.ticket = Some(t);
                            self.dev_ui.msg = None;
                        }
                        None => self.dev_ui.msg = Some(("Moteur P2P pas encore prêt, réessaie dans un instant.".to_string(), false)),
                    }
                }
                DevAction::CancelPairing => {
                    self.p2p.cancel_pairing();
                    self.dev_ui.ticket = None;
                }
                DevAction::Pair(text) => {
                    let name = self.device_name();
                    self.dev_ui.msg = None;
                    self.dev_ui.phone_code = self.p2p.code_for(&text);
                    self.p2p.pair_with_text(&text, &name);
                }
                DevAction::Forget(id) => self.p2p.forget(&id),
                DevAction::SyncNow => self.p2p.sync_now(),
            }
        }
        if !open {
            self.show_devices = false;
        }
        if self.dev_ui.ticket.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500)); // compte à rebours
        }
    }

    /// Demande l'identifiant et le mot de passe Celcat au premier lancement
    /// (indispensable sur Android, où il n'y a ni variable d'environnement ni `~/CELCAT_PASSWORD`).
    pub(super) fn login_window(&mut self, ctx: &egui::Context) {
        if self.password.is_some() {
            return;
        }
        if !self.login.init {
            self.login.user = self.cfg.username.clone();
            self.login.federation = self.cfg.federation_id.clone();
            self.login.init = true;
        }
        let mut submit = false;
        let win = egui::Window::new(RichText::new("Connexion Celcat").font(bold(18.0)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 125.0])
            .default_width((ctx.content_rect().width() - 32.0).clamp(260.0, 380.0));
        ui::page::show(ctx, egui::Id::new("login"), "Connexion Celcat", win, None, true, |ui| {
            ui.label(RichText::new("Identifiants CY Tech. Ils restent sur cet appareil.").font(regular(13.0)).color(theme::DIM));
            ui.label(
                RichText::new("Cydera est déjà sur ton PC ? Menu → Appareils : associe-les et les identifiants suivent.")
                    .font(regular(12.0))
                    .color(theme::MUTED),
            );
            ui.add_space(8.0);
            let w = ui.available_width();
            ui.label(RichText::new("Identifiant").font(regular(12.0)));
            ui.add(egui::TextEdit::singleline(&mut self.login.user).desired_width(w));
            ui.label(RichText::new("Numéro étudiant").font(regular(12.0)));
            ui.add(egui::TextEdit::singleline(&mut self.login.federation).desired_width(w));
            ui.label(RichText::new("Mot de passe").font(regular(12.0)));
            let r = ui.add(egui::TextEdit::singleline(&mut self.login.pwd).password(true).desired_width(w));
            // Le numéro étudiant n'est requis que s'il n'est pas déjà connu.
            let federation_ok = !self.login.federation.trim().is_empty() || !self.cfg.federation_id.is_empty();
            let ok = !self.login.user.trim().is_empty() && !self.login.pwd.trim().is_empty() && federation_ok;
            if ok && r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                submit = true;
            }
            if let Some(e) = &self.login.error {
                ui.colored_label(theme::NOW, e);
            }
            ui.add_space(8.0);
            if ui.add_enabled(ok, egui::Button::new(RichText::new("Enregistrer").font(bold(14.0))).min_size(egui::vec2(w, 48.0))).clicked()
            {
                submit = true;
            }
        });
        if submit {
            let f = &mut self.login;
            let same_mail = self.cfg.mail_user == self.cfg.username;
            self.cfg.username = f.user.trim().to_string();
            if same_mail {
                self.cfg.mail_user = self.cfg.username.clone();
            }
            if !f.federation.trim().is_empty() {
                self.cfg.federation_id = f.federation.trim().to_string();
            }
            match crate::config::save_password(&f.pwd) {
                Ok(()) => {
                    self.cfg.save();
                    self.password = load_password();
                    self.mail.set_account(&self.cfg.mail_host, &self.cfg.mail_user, load_mail_password());
                    f.pwd.clear();
                    f.error = None;
                    self.error = None;
                    self.last_attempt = None;
                    ctx.request_repaint();
                }
                Err(e) => f.error = Some(format!("Enregistrement impossible : {e}")),
            }
        }
    }
}
